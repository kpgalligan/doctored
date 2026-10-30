#!/usr/bin/env python3
"""Phase6 external MCP preservation and real-photo workflow verification.

Set DR_VERIFY_APP to the private built executable. Only owned Popen processes
are signalled. Actions, recovery, native copies and exports use private roots;
source photographs are read-only. Workflow success is not a matting-quality
claim: these photographs have no alpha ground truth, so the edge stage's
accuracy is measured on known-alpha composites (scripts/verify_v1_selection.py).
"""

import argparse
import base64
import hashlib
import json
from pathlib import Path
import re
import shutil
import struct
import sys
import tempfile
import zlib

sys.dont_write_bytecode = True
from phase5_recovery import protected, recover
from phase5_snapshots import new_snapshot
from phase5_support import App, digest, fixture, write_report
from verify_phase2 import rendered

ROOT = Path(__file__).resolve().parents[1]


def native_byte_version():
    """The byte `.doctored` identity word the core writes today (`identity_native::BYTE_VERSION`)."""
    source = (ROOT / "core/src/identity_native.rs").read_text()
    found = re.findall(r"pub const BYTE_VERSION: u32 = (\d+);", source)
    assert len(found) == 1, found
    return int(found[0])


def assert_current_native(path):
    """A byte `.doctored` written now carries the current identity word; returns it."""
    data = path.read_bytes()
    assert data[:4] in (b"RZDC", b"DRDC"), data[:4]
    version = struct.unpack_from("<I", data, 4)[0]
    expected = native_byte_version()
    assert version == expected, (version, expected)
    return version


def png_fingerprint(data):
    """Hash independently reconstructed RGBA rows without millions of tuples."""
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    position, packed = 8, bytearray()
    while position < len(data):
        size = struct.unpack_from(">I", data, position)[0]
        kind = data[position + 4:position + 8]
        payload = data[position + 8:position + 8 + size]
        if kind == b"IHDR":
            width, height, depth, color, _, _, interlace = struct.unpack(">IIBBBBB", payload)
            assert depth == 8 and interlace == 0, (depth, interlace)
        elif kind == b"IDAT":
            packed.extend(payload)
        position += size + 12
    channels = {0: 1, 2: 3, 4: 2, 6: 4}[color]
    stride = width * channels
    decoded = zlib.decompress(packed)
    assert len(decoded) == height * (stride + 1)
    previous = bytearray(stride)
    fingerprint = hashlib.sha256()
    for y in range(height):
        start = y * (stride + 1)
        kind, row = decoded[start], bytearray(decoded[start + 1:start + 1 + stride])
        if kind:
            for i in range(stride):
                left = row[i - channels] if i >= channels else 0
                up = previous[i]
                corner = previous[i - channels] if i >= channels else 0
                if kind == 1:
                    predictor = left
                elif kind == 2:
                    predictor = up
                elif kind == 3:
                    predictor = (left + up) // 2
                else:
                    assert kind == 4
                    p = left + up - corner
                    a, b, c = abs(p - left), abs(p - up), abs(p - corner)
                    predictor = left if a <= b and a <= c else up if b <= c else corner
                row[i] = (row[i] + predictor) & 255
        if channels == 4:
            fingerprint.update(row)
        else:
            rgba = bytearray(width * 4)
            for x in range(width):
                src, dst = x * channels, x * 4
                if color in (0, 4):
                    rgba[dst:dst + 4] = bytes((row[src], row[src], row[src],
                                               row[src + 1] if color == 4 else 255))
                else:
                    rgba[dst:dst + 3] = row[src:src + 3]
                    rgba[dst + 3] = 255
            fingerprint.update(rgba)
        previous = row
    return {"width": width, "height": height, "rgba_sha256": fingerprint.hexdigest()}


def render_to(app, document, path=None, **arguments):
    result = app.client.rpc("tools/call", {
        "name": "render", "arguments": {"document_id": document, "max_side": 4096, **arguments}})
    assert not result.get("isError"), result
    encoded = next(item["data"] for item in result["content"] if item["type"] == "image")
    data = base64.b64decode(encoded)
    if path is not None:
        Path(path).write_bytes(data)
    return png_fingerprint(data)


def mask_state(app, document):
    info = app.call("get_document", document_id=document)
    return {
        "properties": app.call("get_mask_properties", document_id=document, layer=0),
        "geometry": {key: info["layers"][0][key]
                     for key in ("width", "height", "offset_x", "offset_y")},
        "pixels": render_to(app, document, layer=0),
        "mask": render_to(app, document, layer=0, channel="mask"),
        "composite": render_to(app, document),
    }


def mask_preservation(folder):
    folder.mkdir()
    source, native = folder / "source.png", folder / "independent.rz"
    fixture(source)
    original_hash = digest(source)
    recovery_root = folder / "recovery"
    with App(folder / "first", recovery_root, interval=3600) as app:
        document = app.open(source)
        call = lambda tool, **args: app.call(tool, document_id=document, **args)
        original = rendered(app.client, document, layer=0)
        call("select_rect", x=20, y=15, width=50, height=42)
        call("add_layer_mask", layer=0, kind="from_selection")
        call("deselect")
        coverage = rendered(app.client, document, layer=0, channel="mask")
        call("set_mask_properties", layer=0, linked=False, density=0.6, feather=2)
        assert rendered(app.client, document, layer=0) == original
        assert rendered(app.client, document, layer=0, channel="mask") == coverage
        call("transform_mask", layer=0, matrix=[1, 0, 0, 1, 7, 5], filter="nearest")
        assert rendered(app.client, document, layer=0) == original
        shifted = rendered(app.client, document, layer=0, channel="mask")
        width, height, before = coverage
        expected = [before[(y - 5) * width + x - 7] if x >= 7 and y >= 5
                    else (0, 0, 0, 255) for y in range(height) for x in range(width)]
        assert shifted == (width, height, expected)
        properties = call("get_mask_properties", layer=0)
        assert properties["offset_x"] == 7 and properties["offset_y"] == 5
        call("set_layer_properties", index=0, offset_x=4, offset_y=3)
        assert call("get_mask_properties", layer=0) == properties
        assert rendered(app.client, document, layer=0, channel="mask") == shifted
        assert rendered(app.client, document, layer=0) == original
        call("fill", layer=0, target="mask", x=30, y=25, tolerance=0,
             contiguous=True, color="#404040")
        filled = rendered(app.client, document, layer=0, channel="mask")
        assert filled != shifted
        assert call("get_mask_properties", layer=0) == properties
        call("apply_filter", layer=0, target="mask", filter="invert")
        assert rendered(app.client, document, layer=0, channel="mask") != filled
        # Invert with no selection covers the whole canvas: the coverage
        # outside the mask's stored rectangle inverts too, and nothing else moves.
        assert properties["outside_coverage"] == 0
        assert call("get_mask_properties", layer=0) == dict(properties, outside_coverage=255)
        assert rendered(app.client, document, layer=0) == original
        expected = mask_state(app, document)
        snapshot = new_snapshot(app, document, name="Independent Mask State")
        call("set_mask_properties", density=0.25, feather=0, linked=True)
        assert mask_state(app, document) != expected
        call("restore_snapshot", snapshot_id=snapshot)
        assert mask_state(app, document) == expected
        call("save_snapshot", snapshot_id=snapshot, path=str(folder / "mask.drsnapshot"))
        call("save_copy", path=str(native), format="rz")
        version = assert_current_native(native)
        reopened = app.open(native)
        assert mask_state(app, reopened) == expected
        checkpoint = protected(app, document)
        assert digest(source) == original_hash
        app.crash()
    with App(folder / "restart", recovery_root, interval=3600) as app:
        available = app.call("get_recovery_status")["available"]
        assert checkpoint["recovery_id"] in {entry["recovery_id"] for entry in available}
        recovered, reply = recover(app, checkpoint["recovery_id"])
        assert "warning" not in reply, reply
        assert mask_state(app, recovered) == expected
        snapshots = app.call("list_snapshots", document_id=recovered)["snapshots"]
        assert [entry["name"] for entry in snapshots] == ["Independent Mask State"]
        app.call("set_mask_properties", document_id=recovered, density=0.2)
        app.call("restore_snapshot", document_id=recovered, snapshot_id=snapshots[0]["snapshot_id"])
        assert mask_state(app, recovered) == expected
        assert digest(source) == original_hash
        render_to(app, recovered, folder / "recovered-mask.png", layer=0, channel="mask")
        app.call("save_copy", document_id=recovered, path=str(folder / "recovered-composite.png"), format="png")
    write_report(folder / "report.json", {"passed": True, "mask_state": expected,
                 "source_sha256": original_hash, "recovery_id": checkpoint["recovery_id"]})
    print(f"PASS: raw pixels, independent mask placement/properties, raw fill/filter, native v{version}, snapshots and forced-crash recovery", flush=True)


def photo_workflow(folder, source):
    folder.mkdir()
    source_hash = digest(source)
    with App(folder / "app", folder / "recovery", interval=3600) as app:
        document = app.open(source)
        call = lambda tool, **args: app.call(tool, document_id=document, **args)
        info = call("get_document")
        dimensions = {"width": info["width"], "height": info["height"]}
        assert max(dimensions.values()) <= 4096, "MCP render limit would shrink full-size mask export"
        original = render_to(app, document, folder / "source.png", layer=0)
        names = {entry["name"] for entry in app.client.rpc("tools/list", {})["tools"]}
        selector = "select_subjects" if "select_subjects" in names else "select_subject"
        selected = call(selector)
        assert "selection" in call("get_document"), selected
        call("save_selection", name="Vision Subject")
        initial = render_to(app, document, folder / "initial-mask.png", channel="Vision Subject")
        call("refine_selection", feather=1, output="mask", layer=0)
        assert call("get_document")["layers"][0]["has_mask"]
        refined = render_to(app, document, folder / "refined-mask.png", layer=0, channel="mask")
        assert render_to(app, document, layer=0) == original
        call("undo")
        assert not call("get_document")["layers"][0]["has_mask"]
        call("redo")
        assert render_to(app, document, layer=0, channel="mask") == refined
        history = call("list_history")["current_state_id"]
        assert call("refine_selection", feather=1, output="mask", layer=0)["changed"] is False
        assert call("list_history")["current_state_id"] == history
        refused = call("refine_selection", edge_strength=1, fail=True)
        assert "unknown" in refused.lower(), refused
        assert call("list_history")["current_state_id"] == history
        call("load_selection", **{"from": "layer_mask"}, layer=0)
        call("save_selection", name="Reusable Refined Subject")
        channel = render_to(app, document, folder / "saved-channel.png", channel="Reusable Refined Subject")
        assert channel == refined
        call("deselect")
        call("load_selection", channel="Reusable Refined Subject")
        call("add_layer_mask", layer=0, kind="from_selection")
        assert render_to(app, document, layer=0, channel="mask") == refined
        composite = render_to(app, document)
        native = folder / "reusable-mask.rz"
        call("save_copy", path=str(native), format="rz")
        assert_current_native(native)
        call("save_copy", path=str(folder / "composite.png"), format="png")
        exported = png_fingerprint((folder / "composite.png").read_bytes())
        reopened = app.open(native)
        assert render_to(app, reopened, layer=0) == original
        assert render_to(app, reopened, layer=0, channel="mask") == refined
        assert render_to(app, reopened, channel="Reusable Refined Subject") == refined
        assert render_to(app, reopened) == composite
        # Compare each path to itself: AppKit's render preview can round
        # low-alpha RGB during premultiply/unpremultiply, while native PNG
        # export keeps the core's straight RGBA8 bytes.
        app.call("save_copy", document_id=reopened,
                 path=str(folder / "reopened-composite.png"), format="png")
        assert png_fingerprint((folder / "reopened-composite.png").read_bytes()) == exported
        for image in [initial, refined, channel, composite, exported]:
            assert {key: image[key] for key in dimensions} == dimensions
        assert digest(source) == source_hash
        report = {"passed": True, "dimensions": dimensions, "selector": selector,
                  "selection_reply": selected, "source_sha256": source_hash,
                  "initial_mask": initial, "refined_mask": refined, "composite": composite,
                  "native_png_export": exported,
                  "quality_gate": "not evaluated here; these photographs have no alpha ground truth"}
        write_report(folder / "report.json", report)
    print(f"PASS: {source.stem} full-size subject→basic refinement→mask→undo/redo→channel reuse→native reopen", flush=True)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--keep", action="store_true")
    parser.add_argument("--photos", type=Path, default=Path.home() / "devel/doctored-fixtures/phase6/photos")
    parser.add_argument("--suite", choices=["all", "masks", "photos"], default="all")
    arguments = parser.parse_args()
    folder = Path(tempfile.mkdtemp(prefix="rasterize-phase6-"))
    print(f"Verification scratch: {folder}", flush=True)
    try:
        report = {}
        if arguments.suite in ("all", "masks"):
            mask_preservation(folder / "masks")
            report["mask_preservation"] = True
        if arguments.suite in ("all", "photos"):
            report["photos"] = {}
            for name in ("portrait.png", "fur.png", "glass.jpg", "product.jpg"):
                source = arguments.photos / name
                assert source.is_file(), f"Missing photo fixture: {source}"
                report["photos"][source.stem] = photo_workflow(folder / source.stem, source)
        write_report(folder / "report.json", report)
    except BaseException:
        print(f"Failure artifacts retained: {folder}", flush=True)
        raise
    if not arguments.keep:
        shutil.rmtree(folder)


if __name__ == "__main__":
    main()
