#!/usr/bin/env python3
"""Owned production MCP drive of the Phase 9I assisted annotation workspace.

Runs a SYNTHETIC session end to end against a real built app and a real
exported workspace: the case list, the gesture and time bookkeeping, a
recorded rasterizer-limit refusal, the written replay and the one-attempt
lock. A synthetic session records `annotator_id: null`, `origin: synthetic`
and `scoreable: false`, so nothing this script produces is a measurement.

The last check launches a second instance WITHOUT the flag and confirms the
tools refuse. Only processes this script started are ever terminated.
"""
import argparse
import contextlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
import app_under_test
from mcp_client import Client, remove_scratch, scratch_socket, wait_ready
from verification_isolation import preset_environment

# core/src/trimap_strokes.rs MAX_VISITS; a corner-to-corner capsule's clipped
# segment box is the whole canvas, so this many of them refuse.
MAX_VISITS = 64_000_000
DIAMETER = 81


@contextlib.contextmanager
def instance(executable, folder, enabled):
    app_under_test.require_fresh_app(executable)
    agent_socket = scratch_socket()
    # DR_EDR_HEADROOM 1 unless the caller set one: an owned instance never engages HDR Preview
    # because of the screen it lands on (v1.x), so no drive's view state depends on the display.
    env = dict(os.environ, DR_AGENT_SOCKET=str(agent_socket), ANTHROPIC_API_KEY="dummy", DR_AUTOMATION_ALERTS="record", DR_WORKING_LIMIT_BYTES=os.environ.get("DR_WORKING_LIMIT_BYTES", "2147483648"), DR_EDR_HEADROOM=os.environ.get("DR_EDR_HEADROOM", "1"),
               DR_ACTIONS_DIR=str(folder / "actions"), DR_RECOVERY_DIR=str(folder / "recovery"),
               **preset_environment(folder))
    if enabled:
        env["DR_ASSISTED_ANNOTATION"] = "1"
    else:
        env.pop("DR_ASSISTED_ANNOTATION", None)
    log = folder / f"app-{'on' if enabled else 'off'}.log"
    with log.open("w") as stream:
        process = subprocess.Popen(
            [str(executable), "-ApplePersistenceIgnoreState", "YES",
             "-ColorWorkingSpace", "srgb"],
            env=env, stdout=stream, stderr=subprocess.STDOUT)
        try:
            client = Client(agent_socket)
            wait_ready(client, process, 30, log)
            yield client
        finally:
            process.terminate()
            try:
                process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            remove_scratch(agent_socket)


def corner_stroke(index, width, height):
    """Two points across the whole canvas, nudged so no two strokes repeat."""
    step = index % 4
    left, right = index % 8, width - 1 - index % 8
    top, bottom = 0, height - 1
    if step in (0, 2):
        return [[left, top], [right, bottom]]
    return [[right, top], [left, bottom]]


def drive(client, workspace):
    manifest = json.loads((workspace / "manifest.json").read_text())
    cases = {entry["anonymous_id"]: entry for entry in manifest["cases"]}
    assert len(cases) == 2, cases

    refused = client.call("assisted_synthetic_stroke", fail=True, tool="foreground",
                          diameter=9, points=[[0, 0]])
    assert "assisted_open_case" in refused, refused

    status = client.call("assisted_workspace_status", workspace=str(workspace))
    assert status["ok"] and status["workspace_id"] == manifest["workspace_id"], status
    assert sorted(status["pending"]) == sorted(cases), status
    assert all(not entry["locked"] for entry in status["cases"]), status

    case_id = max(cases, key=lambda key: cases[key]["width"] * cases[key]["height"])
    width, height = cases[case_id]["width"], cases[case_id]["height"]
    opened = client.call("assisted_open_case", workspace=str(workspace), case=case_id,
                         synthetic=True)
    assert opened == {"ok": True, "case_id": case_id, "width": width, "height": height,
                      "origin": "synthetic", "state": "active"}, opened

    state = client.call("assisted_session_state")
    assert state["ok"] and state["active"] and state["case_id"] == case_id, state
    assert state["modifying_gestures"] == 0 and state["active_strokes"] == 0, state
    assert state["remaining_gestures"] == 20 and state["remaining_ms"] > 0, state

    plane = state["label_plane_sha256"]
    gestures = 0
    # The Unknown stroke retraces the Foreground one: Unknown RELEASES a
    # constraint, so on untouched canvas it would change nothing at all.
    short = [("foreground", [[10, 10], [60, 60]]), ("background", [[10, 90], [60, 140]]),
             ("unknown", [[10, 10], [60, 60]])]
    for tool, points in short:
        result = client.call("assisted_synthetic_stroke", tool=tool, diameter=9, points=points)
        gestures += 1
        assert result["accepted"] and result["result"] == "accepted", result
        assert result["modifying_gestures"] == gestures, result
        assert result["label_plane_sha256"] != plane, (tool, result)
        plane = result["label_plane_sha256"]

    # Each corner-to-corner capsule's clipped box is the whole canvas, so the
    # rasterizer's cumulative visit cap refuses one of the first few.
    cap = MAX_VISITS // (width * height) + 1
    assert 1 < cap <= 20 - gestures, (cap, width, height)
    rejected = None
    for index in range(cap):
        result = client.call("assisted_synthetic_stroke", tool="foreground", diameter=DIAMETER,
                             points=corner_stroke(index, width, height))
        gestures += 1
        assert result["modifying_gestures"] == gestures, result
        if not result["accepted"]:
            rejected = result
            assert result["result"] == "rejected:rasterizer_limit", result
            assert result["label_plane_sha256"] == plane, result
            break
        assert result["result"] == "accepted", result
        assert result["label_plane_sha256"] != plane, result
        plane = result["label_plane_sha256"]
    assert rejected is not None, "no corner-to-corner stroke reached the rasterizer limit"

    final = client.call("assisted_finalize_case")
    assert final["ok"] and final["outcome"] == "submitted", final
    assert final["human_origin"] is False and final["scoreable"] is False, final
    path = Path(final["replay_path"])
    assert path == workspace / "replays" / f"{case_id}.json", final
    assert path.exists(), final

    replay = json.loads(path.read_text())
    assert replay["replay_schema_version"] == 1, replay["replay_schema_version"]
    assert replay["protocol_id"] == manifest["protocol_id"], replay["protocol_id"]
    assert replay["case_id"] == case_id and replay["origin"] == "synthetic", replay["case_id"]
    assert replay["human_origin"] is False and replay["scoreable"] is False, replay
    assert replay["annotator_id"] is None, replay["annotator_id"]
    assert replay["blinding_attestation"] is None, replay["blinding_attestation"]
    assert replay["modifying_gestures"] == gestures, (replay["modifying_gestures"], gestures)
    assert replay["outcome"] == "submitted", replay["outcome"]
    assert replay["budget"] == manifest["budget"], replay["budget"]
    assert replay["displayed_assets"]["source_plane_sha256"] == \
        cases[case_id]["source_plane_sha256"], replay["displayed_assets"]
    assert replay["displayed_assets"]["task_card_sha256"] == manifest["task_card_sha256"], \
        replay["displayed_assets"]
    events = replay["events"]
    assert events[-1]["kind"] == "submit", events[-1]
    assert replay["label_plane"]["sha256"] == events[-1]["label_plane_sha256"], replay["label_plane"]
    assert replay["label_plane"]["encoding"] == "trimap_u8_0_128_255", replay["label_plane"]
    # A rejected gesture costs a gesture and leaves the plane exactly as it was.
    rejections = 0
    for index, event in enumerate(events):
        assert event["index"] == index, event
        assert event["origin"] == "synthetic", event
        if event["result"].startswith("rejected:"):
            rejections += 1
            assert event["kind"] == "stroke", event
            assert event["label_plane_sha256"] == events[index - 1]["label_plane_sha256"], event
    assert rejections == 1, rejections

    locked = client.call("assisted_open_case", fail=True, workspace=str(workspace), case=case_id)
    assert "locked" in locked, locked
    status = client.call("assisted_workspace_status", workspace=str(workspace))
    assert status["pending"] == [key for key in sorted(cases) if key != case_id], status
    assert any(entry["locked"] for entry in status["cases"]), status
    return {"case_id": case_id, "width": width, "height": height, "gestures": gestures,
            "events": len(events), "rejected_gestures": rejections,
            "replay": str(path), "label_plane_sha256": replay["label_plane"]["sha256"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--workspace", type=Path, required=True)
    args = parser.parse_args()
    workspace = args.workspace.expanduser().resolve()
    folder = Path(tempfile.mkdtemp(prefix="rasterize-phase9i-assisted-"))
    print("Assisted annotation drive:", folder, flush=True)
    with instance(args.executable, folder, enabled=True) as client:
        receipt = drive(client, workspace)
    with instance(args.executable, folder, enabled=False) as client:
        message = client.call("assisted_workspace_status", fail=True, workspace=str(workspace))
        assert "internal Phase 9I experiment and is off" in message, message
        receipt["disabled_message"] = message
    print(json.dumps(receipt, indent=2, sort_keys=True), flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
