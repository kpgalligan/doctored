"""Isolated app ownership and state comparisons for Phase 5 verification.

The harness owns every process and fixture. Restarting with the same private
recovery directory models a crash without touching the user's recovery store.
"""

import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

import app_under_test
from mcp_client import remove_scratch, scratch_socket, wait_ready
from verify_phase1 import APP, Client, png
from verify_phase2 import rendered
from verification_isolation import PRESET_DIRECTORIES, PROVIDER_BASE_URLS


class App:
    def __init__(self, folder, recovery, *, interval=1, extra_defaults=()):
        self.folder = Path(folder)
        self.socket = None  # this instance's scratch DR_AGENT_SOCKET, made at __enter__
        self.recovery = Path(recovery)
        self.interval = interval
        self.extra_defaults = list(extra_defaults)
        self.process = None
        self.client = None
        self.log = None

    def __enter__(self):
        app_under_test.require_fresh_app(APP)
        self.folder.mkdir(parents=True, exist_ok=True)
        if not self.recovery.exists():
            self.recovery.mkdir(parents=True)
        self.socket = scratch_socket()
        # DR_EDR_HEADROOM 1 unless the caller set one: an owned instance never engages HDR Preview
        # because of the screen it lands on (v1.x), so no drive's view state depends on the display.
        environment = dict(os.environ, ANTHROPIC_API_KEY="dummy", DR_AUTOMATION_ALERTS="record", DR_WORKING_LIMIT_BYTES=os.environ.get("DR_WORKING_LIMIT_BYTES", "2147483648"), DR_EDR_HEADROOM=os.environ.get("DR_EDR_HEADROOM", "1"), DR_AGENT_SOCKET=str(self.socket),
                           DR_ACTIONS_DIR=str(self.folder / "actions"),
                           DR_RECOVERY_DIR=str(self.recovery))
        for name, leaf in PRESET_DIRECTORIES.items():
            environment.setdefault(name, str(self.folder / leaf))
        for name, url in PROVIDER_BASE_URLS.items():
            environment.setdefault(name, url)
        if self.interval is None:
            environment.pop("DR_RECOVERY_INTERVAL_SECONDS", None)
        else:
            environment["DR_RECOVERY_INTERVAL_SECONDS"] = str(self.interval)
        self.log = (self.folder / "app.log").open("w")
        self.process = subprocess.Popen(
            [str(APP), "-ApplePersistenceIgnoreState", "YES",
             "-RecoveryDiscoverySuppressed", "YES", "-ColorWorkingSpace", "srgb",
             *self.extra_defaults],
            env=environment, stdout=self.log, stderr=subprocess.STDOUT)
        self.client = Client(self.socket)
        try:
            wait_ready(self.client, self.process, 30, self.log.name)
            return self
        except BaseException:
            self.stop()
            raise

    def stop(self, *, crash=False):
        """Signal only the Popen process owned by this instance."""
        if self.process is not None and self.process.poll() is None:
            if crash:
                self.process.kill()
            else:
                self.process.terminate()
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=10)
        if self.log is not None:
            self.log.close()
            self.log = None
        remove_scratch(self.socket)

    def crash(self):
        self.stop(crash=True)

    def __exit__(self, *_):
        self.stop()

    def call(self, tool_name, **arguments):
        return self.client.call(tool_name, **arguments)

    def open(self, path):
        return self.client.open(path)


def wait_for(query, predicate, *, timeout=12, detail="state transition"):
    deadline = time.monotonic() + timeout
    latest = None
    while True:
        latest = query()
        if predicate(latest):
            return latest
        assert time.monotonic() < deadline, (f"Timed out waiting for {detail}", latest)
        time.sleep(0.05)


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def fixture(path, width=128, height=96):
    pixels = bytes(value for y in range(height) for x in range(width)
                   for value in (30 + (x * 17 + y * 3) % 190,
                                 20 + (x * 5 + y * 11) % 200,
                                 40 + (x ^ y) % 180, 255))
    png(Path(path), width, height, pixels)


def decorate(app, document):
    """A small layered editing state containing every recovery-gate payload."""
    call = lambda tool_name, **arguments: app.call(tool_name, document_id=document, **arguments)
    call("set_layer_properties", index=0, name="Recovery Base")
    call("select_rect", x=9, y=7, width=84, height=65)
    call("modify_selection", operation="feather", radius=3)
    call("add_layer_mask", layer=0, kind="from_selection")
    call("add_channel", name="Recovery Coverage", **{"from": "selection"})
    call("add_guide", orientation="vertical", position=25)
    call("add_guide", orientation="horizontal", position=41)
    call("add_text_layer", text="Recover", x=18, y=15, size=16, font="Helvetica Neue", color="#2974bc")
    call("select_rect", x=13, y=11, width=61, height=47)
    call("set_active_layer", index=0)


def editing_state(app, document):
    """Compare semantic state plus independently returned PNG pixel payloads.

    Process IDs, source paths, saved flags and import-time reports deliberately
    remain separate lifecycle assertions rather than part of pixel equality.
    """
    state = app.call("get_document", document_id=document)
    names = ("width", "height", "layers", "global_light", "resolution", "channels", "guides",
             "selection", "selected_layers", "active_layer", "edit_target")
    result = {name: state.get(name) for name in names}
    # A row's `id` is the layer's stable identity WITHIN one document
    # (AgentServer+Groups.swift `structureFields`); two documents opened from
    # the same file number their layers independently, so it is not state.
    result["layers"] = [{key: value for key, value in layer.items() if key != "id"}
                        for layer in state.get("layers") or []]
    profile = state.get("color_profile", {})
    result["profile"] = {key: profile.get(key) for key in ("name", "kind", "source", "size_bytes")
                         if key in profile}
    result["ruler"] = {key: value for key, value in state.get("ruler", {}).items() if key != "unit"}
    result["composite"] = rendered(app.client, document)
    result["planes"] = {}
    for layer in state["layers"]:
        index = layer["index"]
        # Byte rows carry `has_mask`; typed rows carry `mask` (null or an
        # object, precision_inspect.rs) and group kinds ending in "group".
        if not str(layer.get("kind", "")).endswith("group"):
            result["planes"][f"layer:{index}"] = rendered(app.client, document, layer=index)
        if layer.get("has_mask") or layer.get("mask"):
            result["planes"][f"mask:{index}"] = rendered(app.client, document, layer=index, channel="mask")
    for channel in state.get("channels", []):
        name = channel["name"]
        result["planes"][f"channel:{name}"] = rendered(app.client, document, channel=name)
    return result


def write_report(path, report):
    Path(path).write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
