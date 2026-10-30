#!/usr/bin/env python3
"""Phase 2 MCP regression: scoped edits, selection history and Actions.

Run after building the app, or set DR_VERIFY_APP to an isolated executable.
Uses the Phase 1 runner's private ports, dummy credentials and owned processes.
The PNG reader below independently decodes small 8-bit verification renders.
"""

import base64
from pathlib import Path
import shutil
import struct
import tempfile
import zlib

from verify_phase1 import instance, png


def read_png(data):
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    offset, packed = 8, b""
    while offset < len(data):
        size = struct.unpack_from(">I", data, offset)[0]
        kind, payload = data[offset + 4:offset + 8], data[offset + 8:offset + 8 + size]
        offset += size + 12
        if kind == b"IHDR":
            width, height, depth, color, _, _, interlace = struct.unpack(">IIBBBBB", payload)
            assert depth == 8 and interlace == 0, (depth, interlace)
        elif kind == b"IDAT":
            packed += payload
    channels = {0: 1, 2: 3, 4: 2, 6: 4}[color]
    stride = width * channels
    raw, previous, pixels = zlib.decompress(packed), bytearray(stride), []
    for y in range(height):
        start = y * (stride + 1)
        filter_type, row = raw[start], bytearray(raw[start + 1:start + 1 + stride])
        for i in range(stride):
            left = row[i - channels] if i >= channels else 0
            up = previous[i]
            corner = previous[i - channels] if i >= channels else 0
            if filter_type == 0:
                predictor = 0
            elif filter_type == 1:
                predictor = left
            elif filter_type == 2:
                predictor = up
            elif filter_type == 3:
                predictor = (left + up) // 2
            else:
                assert filter_type == 4
                p = left + up - corner
                distances = [abs(p - left), abs(p - up), abs(p - corner)]
                predictor = [left, up, corner][distances.index(min(distances))]
            row[i] = (row[i] + predictor) & 255
        for x in range(width):
            value = row[x * channels:(x + 1) * channels]
            if color == 0:
                pixels.append(tuple(value) * 3 + (255,))
            elif color == 4:
                pixels.append((value[0],) * 3 + (value[1],))
            else:
                pixels.append(tuple(value) + ((255,) if color == 2 else ()))
        previous = row
    return width, height, pixels


def rendered(client, document, **args):
    response = client.rpc("tools/call", {
        "name": "render", "arguments": {"document_id": document, "max_side": 4096, **args},
    })
    assert not response.get("isError"), response
    data = next(item["data"] for item in response["content"] if item["type"] == "image")
    return read_png(base64.b64decode(data))


def scoped_edits(client, folder, fixture):
    document = client.open(fixture)
    call = lambda tool, **args: client.call(tool, document_id=document, **args)
    assert not call("get_document")["unsaved_changes"]
    call("select_rect", x=7, y=5, width=9, height=8)
    call("modify_selection", operation="feather", radius=1)
    assert not call("get_document")["unsaved_changes"], "Selection must not dirty the file"
    call("save_selection", name="Region")
    coverage = [pixel[0] for pixel in rendered(client, document, channel="Region")[2]]
    assert 0 in coverage and 255 in coverage and any(0 < c < 255 for c in coverage)
    call("deselect")
    call("add_layer_mask")
    call("gradient", target="mask", x0=0, y0=0, x1=24, y1=0,
         start_color="#303030", end_color="#D0D0D0")
    # A linear ramp is invariant under symmetric blur away from its edges.
    # Add a narrow stripe so the selected neighborhood has a real blur result.
    call("fill", target="mask", x=10, y=8, tolerance=0, color="#FFFFFF")
    call("add_channel", name="Data", **{"from": "plane", "plane": "red"})
    for target, render_args in [
        ("layer", {"layer": 0}), ("mask", {"channel": "mask", "layer": 0}),
        ("red", {"channel": "red", "layer": 0}), ("channel:Data", {"channel": "Data"}),
    ]:
        for filter_name, options in [("invert", {}), ("blur", {"sigma": 1.4})]:
            call("deselect")
            before = rendered(client, document, **render_args)[2]
            layer_before = rendered(client, document, layer=0)
            call("apply_filter", target=target, filter=filter_name, **options)
            full = rendered(client, document, **render_args)[2]
            call("undo")
            assert rendered(client, document, **render_args)[2] == before
            call("load_selection", channel="Region")
            call("apply_filter", target=target, filter=filter_name, **options)
            scoped = rendered(client, document, **render_args)[2]
            assert scoped != before, (target, filter_name, "no selected changes")
            for i, (old, new, actual, weight) in enumerate(zip(before, full, scoped, coverage)):
                expected = tuple((a * (255 - weight) + b * weight + 127) // 255
                                 for a, b in zip(old, new))
                assert actual == expected, (target, filter_name, i, weight, old, new, actual, expected)
            if target in ("mask", "channel:Data"):
                assert rendered(client, document, layer=0) == layer_before
            call("undo")
            assert rendered(client, document, **render_args)[2] == before
    # Fills and ramps use the same selection on each scalar target.
    for target, render_args in [("mask", {"channel": "mask", "layer": 0}),
                                ("channel:Data", {"channel": "Data"})]:
        for tool, options in [
            ("fill", {"x": 10, "y": 8, "tolerance": 255, "color": "#FFFFFF"}),
            ("gradient", {"x0": 0, "y0": 0, "x1": 24, "y1": 0,
                          "start_color": "#000000", "end_color": "#FFFFFF"}),
        ]:
            before = rendered(client, document, **render_args)[2]
            layer_before = rendered(client, document, layer=0)
            call(tool, target=target, **options)
            after = rendered(client, document, **render_args)[2]
            assert after != before
            assert all(a == b for a, b, c in zip(before, after, coverage) if c == 0)
            assert rendered(client, document, layer=0) == layer_before
            call("undo")
    # Invalid combinations and locks must fail with their target unchanged.
    before = rendered(client, document, layer=0)
    call("apply_filter", target="luma", filter="invert", fail=True)
    call("set_layer_lock", locks=["all"])
    call("apply_filter", target="mask", filter="invert", fail=True)
    call("fill", target="mask", x=10, y=8, fail=True)
    assert rendered(client, document, layer=0) == before
    call("undo")
    print("PASS: feathered filter/blur scope, mask/channel fills and gradients, target refusal", flush=True)


def selection_history(client, fixture):
    document = client.open(fixture)
    call = lambda tool, **args: client.call(tool, document_id=document, **args)
    call("select_rect", x=3, y=4, width=12, height=9)
    first = call("get_document")["selection"]
    call("deselect")
    assert "selection" not in call("get_document")
    call("undo")
    assert call("get_document")["selection"] == first
    call("redo")
    assert "selection" not in call("get_document")
    call("reselect")
    assert call("get_document")["selection"] == first
    assert not call("get_document")["unsaved_changes"]
    call("crop", x=3, y=4, width=12, height=9)
    assert "selection" not in call("get_document")
    call("undo")
    info = call("get_document")
    assert (info["width"], info["height"]) == (24, 20)
    assert info["selection"] == first and not info["unsaved_changes"]
    call("redo")
    assert call("get_document")["width"] == 12
    call("undo")
    call("undo")  # Reselect, independently of the crop.
    assert "selection" not in call("get_document")
    assert not call("get_document")["unsaved_changes"]
    print("PASS: transient selection undo/redo, Reselect and crop-plus-selection history", flush=True)


def actions(client, first, second):
    doc1, doc2 = client.open(first), client.open(second)
    steps = [
        {"tool": "select_rect", "arguments": {"x": 7, "y": 5, "width": 9, "height": 8}},
        {"tool": "modify_selection", "arguments": {"operation": "feather", "radius": 1}},
        {"tool": "apply_filter", "arguments": {"filter": "invert", "target": "red"}},
        {"tool": "auto_contrast", "arguments": {"target": "blue", "clip": 0}},
    ]
    for step in steps:
        client.call(step["tool"], document_id=doc1, **step["arguments"])
    client.call("save_action", name="Phase 2 scoped replay", steps=steps)
    client.call("run_action", name="Phase 2 scoped replay", document_id=doc2)
    assert rendered(client, doc1, layer=0) == rendered(client, doc2, layer=0)
    client.call("undo", document_id=doc2)
    assert not client.call("get_document", document_id=doc2)["unsaved_changes"]
    assert "selection" not in client.call("get_document", document_id=doc2)
    client.call("redo", document_id=doc2)
    assert rendered(client, doc1, layer=0) == rendered(client, doc2, layer=0)
    client.call("save_action", name="Phase 2 selection only", steps=steps[:2])
    client.call("undo", document_id=doc2)
    client.call("run_action", name="Phase 2 selection only", document_id=doc2)
    assert not client.call("get_document", document_id=doc2)["unsaved_changes"]
    assert "selection" in client.call("get_document", document_id=doc2)
    client.call("undo", document_id=doc2)
    assert "selection" not in client.call("get_document", document_id=doc2)
    print("PASS: direct MCP/Actions pixel equivalence, grouped replay and selection-only dirty state", flush=True)


def quick_mask(client, first, second):
    doc1, doc2 = client.open(first), client.open(second)
    call = lambda tool, **args: client.call(tool, document_id=doc1, **args)
    points = [{"x": x, "y": y} for y in range(20) for x in range(24)]

    def coverage(doc):
        return [sample["coverage"] for sample in client.call(
            "get_selection", document_id=doc, points=points)["samples"]]

    # Two exact overlay operations, authored independently of the Swift encoder.
    overlays = [[(0, 0)] * 480 for _ in range(2)]
    overlays[0][2 * 24 + 2] = (128, 128)
    overlays[1][7 * 24 + 8] = (0, 96)
    encoded = [base64.b64encode(b"".join(struct.pack("<IBB", 1, *pixel)
                                         for pixel in overlay)).decode()
               for overlay in overlays]
    call("start_recording")
    call("select_rect", x=7, y=5, width=9, height=8)
    call("set_quick_mask", active=True)
    call("apply_filter", filter="invert", fail=True)
    call("fill", x=8, y=7, target="mask", fail=True)
    before = coverage(doc1)
    snapshots = [before]
    for overlay, payload in zip(overlays, encoded):
        call("quick_mask_stroke", width=24, height=20, coverage_rle=payload)
        expected = [gray + (old * (255 - alpha) + 127) // 255
                    for old, (gray, alpha) in zip(snapshots[-1], overlay)]
        assert coverage(doc1) == expected
        snapshots.append(expected)
    assert not call("get_document")["unsaved_changes"]
    call("undo")
    assert coverage(doc1) == snapshots[1]
    call("undo")
    assert coverage(doc1) == before
    call("redo")
    call("redo")
    assert coverage(doc1) == snapshots[2]
    call("set_quick_mask", active=False)
    call("stop_recording", name="Phase 2 Quick Mask")
    # Read/undo/redo did not add recording steps; actual overlay bytes replay.
    client.call("run_action", name="Phase 2 Quick Mask", document_id=doc2)
    assert coverage(doc2) == snapshots[2]
    assert not client.call("get_document", document_id=doc2)["unsaved_changes"]
    client.call("undo", document_id=doc2)
    assert coverage(doc2) == [0] * 480
    client.call("redo", document_id=doc2)
    assert coverage(doc2) == snapshots[2]
    call("quick_mask_stroke", width=1, height=1, coverage_rle=encoded[0], fail=True)
    call("quick_mask_stroke", width=24, height=20, coverage_rle="AAAAAA==", fail=True)
    assert coverage(doc1) == snapshots[2]
    print("PASS: individual Quick Mask stroke undo, independent coverage math, exact Actions replay", flush=True)


def main():
    folder = Path(tempfile.mkdtemp(prefix="rasterize-phase2-"))
    print(f"Verification scratch: {folder}", flush=True)
    try:
        pixels = bytes(value for y in range(20) for x in range(24)
                       for value in (40 + (x * 37 + y * 29) % 170,
                                     30 + (x * 3 + y * 7) % 190, 50 + (x * 9 + y * 5) % 150, 255))
        fixtures = [folder / f"source-{i}.png" for i in range(6)]
        for fixture in fixtures:
            png(fixture, 24, 20, pixels)
        with instance(folder) as client:
            scoped_edits(client, folder, fixtures[0])
            selection_history(client, fixtures[1])
            actions(client, fixtures[2], fixtures[3])
            quick_mask(client, fixtures[4], fixtures[5])
    except BaseException:
        print(f"Failure artifacts retained: {folder}", flush=True)
        raise
    shutil.rmtree(folder)


if __name__ == "__main__":
    main()
