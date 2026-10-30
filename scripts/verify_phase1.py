#!/usr/bin/env python3
"""Exercise Phase 1 against a freshly built app, on an isolated scratch MCP socket.

Run after make app. Uses only the Python standard library, temporary fixtures,
dummy credentials and processes it owns. Does not modify the user's documents,
action library or saved preferences. Core tests provide the numeric oracles.
"""

import contextlib
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import zlib

import app_under_test
from mcp_client import Client, remove_scratch, scratch_socket, wait_ready
from verification_isolation import preset_environment

ROOT = Path(__file__).resolve().parents[1]
APP = app_under_test.default_app_executable()
NATIVE = "com.kgalligan.doctored-document"


def png(path, width, height, pixels):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    rows = b"".join(b"\0" + pixels[y * width * 4:(y + 1) * width * 4] for y in range(height))
    path.write_bytes(b"\x89PNG\r\n\x1a\n"
                     + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
                     + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))


@contextlib.contextmanager
def instance(folder, budget=0, working_limit=None):
    app_under_test.require_fresh_app(APP)
    agent_socket = scratch_socket()
    # DR_EDR_HEADROOM 1 unless the caller set one: an owned instance never engages HDR Preview
    # because of the screen it lands on (v1.x), so no drive's view state depends on the display.
    env = dict(os.environ, DR_AGENT_SOCKET=str(agent_socket), ANTHROPIC_API_KEY="dummy", DR_AUTOMATION_ALERTS="record", DR_WORKING_LIMIT_BYTES=str(working_limit) if working_limit is not None else os.environ.get("DR_WORKING_LIMIT_BYTES", "2147483648"), DR_EDR_HEADROOM=os.environ.get("DR_EDR_HEADROOM", "1"),
               DR_ACTIONS_DIR=str(folder / "actions"), DR_RECOVERY_DIR=str(folder / "recovery"),
               **preset_environment(folder))
    with (folder / f"app-{budget}.log").open("w") as log:
        process = subprocess.Popen(
            [str(APP), "-ApplePersistenceIgnoreState", "YES",
             "-ColorWorkingSpace", "srgb", "-RenderGroupPixelBudget", str(budget)],
            env=env, stdout=log, stderr=subprocess.STDOUT)
        try:
            client = Client(agent_socket)
            wait_ready(client, process, 25, log.name)
            yield client
        finally:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            remove_scratch(agent_socket)


def preservation(client, folder, fixture):
    document = client.open(fixture)
    call = lambda tool_name, **args: client.call(tool_name, document_id=document, **args)
    original = fixture.read_bytes()
    initial = call("get_document")
    assert "public.png" in initial["save_preservation"]["save_types"]
    for name, args, expected in [
        ("add_layer_mask", {"layer": 0}, "layer masks"),
        ("add_guide", {"orientation": "vertical", "position": 2}, "guides and ruler origin"),
        ("save_layer_comp", {"name": "A"}, "layer comps"),
    ]:
        call(name, **args)
        info = call("get_document")
        assert info["layer_count"] == 1
        assert info["save_preservation"]["save_types"] == [NATIVE]
        assert expected in info["save_preservation"]["losses_by_format"]["png"]
        call("undo")
        assert "public.png" in call("get_document")["save_preservation"]["save_types"]
        call("redo")
        assert expected in call("get_document")["save_preservation"]["losses_by_format"]["png"]
        call("undo")
    call("add_layer_mask", layer=0)
    call("add_guide", orientation="vertical", position=2)
    call("select_rect", x=1, y=1, width=3, height=2)
    call("save_selection", name="Subject")
    call("set_resolution", ppi_x=300, ppi_y=300)
    native = folder / "rich.rz"
    call("save_copy", path=str(native))
    flat = call("save_copy", path=str(folder / "export.png"))
    assert "saved selection channels" in flat["flat_save_losses"]
    assert call("get_document")["unsaved_changes"]
    assert fixture.read_bytes() == original
    reopened = client.open(native)
    info = client.call("get_document", document_id=reopened)
    assert info["layer_count"] == 1 and info["layers"][0]["has_mask"]
    assert info["channels"][0]["name"] == "Subject"
    assert info["guides"][0]["position"] == 2
    assert info["save_preservation"]["save_types"] == [NATIVE]
    print("PASS: single-layer Save policy, mask/guide undo-redo, native reopen, explicit export and dirty state", flush=True)


def transparent_edges(client, folder):
    source = folder / "edge.png"
    png(source, 2, 1, bytes([230, 40, 80, 255, 0, 200, 240, 0]))
    document = client.open(source)
    call = lambda tool_name, **args: client.call(tool_name, document_id=document, **args)
    call("image_size", width=3, height=1, filter="bilinear")
    pixel = call("sample_color", x=1, y=0)
    # These channels are straight, document-space values, not display bytes.
    assert [pixel[c] for c in ("r", "g", "b")] == [230, 40, 80], pixel
    assert 120 <= pixel["a"] <= 135, pixel
    call("undo")
    call("apply_filter", layer=0, filter="blur", sigma=1)
    pixel = call("sample_color", x=1, y=0)
    assert [pixel[c] for c in ("r", "g", "b")] == [230, 40, 80], pixel
    call("save_copy", path=str(folder / "blurred.png"))
    print("PASS: alpha-correct document resize, blur and rendered-file output", flush=True)


def render_failure(client, folder, fixture):
    document = client.open(fixture)
    call = lambda tool_name, **args: client.call(tool_name, document_id=document, **args)
    assert call("get_document")["preview_is_current"]
    call("group_layers", layers=[0], name="Budget test")
    call("set_layer_properties", index=1, blend_mode="normal")
    info = call("get_document")
    assert not info["preview_is_current"] and "budget" in info["projection_error"]
    assert "budget" in call("render", fail=True)
    destination = folder / "previous-good.png"
    destination.write_bytes(b"previous good file")
    assert "budget" in call("save_copy", path=str(destination), fail=True)
    assert destination.read_bytes() == b"previous good file"
    native = folder / "failed-preview.rz"
    call("save_copy", path=str(native))
    reopened = client.open(native)
    assert client.call("get_document", document_id=reopened)["layer_count"] == 2
    call("undo")
    assert call("get_document")["preview_is_current"]
    call("redo")
    assert not call("get_document")["preview_is_current"]
    print("PASS: explicit render refusal, preserved destination, native rescue, undo-redo", flush=True)


def main():
    folder = Path(tempfile.mkdtemp(prefix="rasterize-phase1-"))
    try:
        fixture = folder / "plain.png"
        png(fixture, 8, 6, bytes([230, 40, 80, 255]) * 48)
        with instance(folder) as client:
            preservation(client, folder, fixture)
            transparent_edges(client, folder)
        with instance(folder, budget=1) as client:
            render_failure(client, folder, fixture)
    except BaseException:
        print(f"Verification files and app logs retained at {folder}", flush=True)
        raise
    else:
        shutil.rmtree(folder)


if __name__ == "__main__":
    main()
