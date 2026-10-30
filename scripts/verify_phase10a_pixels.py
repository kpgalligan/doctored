#!/usr/bin/env python3
"""Verify complete PSD raster storage and off-canvas moves in an owned app.

Consumes authored fixtures exported by the PSD pixel FFI tests. Pixel and
placement expectations come from adjacent manifests, never importer output.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("pixel_import_support", ROOT / "scripts/retouch-probe/support.py")
support = importlib.util.module_from_spec(spec)
spec.loader.exec_module(support)
CASES = {"negative_raw", "negative_rle", "offcanvas_rle", "ending_zero_rle",
         "opaque_raw", "grayscale_rle", "mixed_channels", "duplicate_group",
         "grayscale_opaque_raw", "repeat_rle", "signed_extreme", "opaque_small", "malformed_rle"}
MODES = {"normal": 0, "multiply": 1, "screen": 2, "pass through": 27}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def pixels(call, path, canvas, wanted):
    call("save_copy", path=str(path))
    decoded = support.png_samples(path.read_bytes())
    assert (decoded["width"], decoded["height"], decoded["depth"]) == (*canvas, 8), decoded
    assert tuple(decoded["samples"]) == tuple(wanted), (path, decoded["samples"], wanted)


def stored(call, path, expected, moved=False):
    call("save_copy", path=str(path))
    parsed = support.read_native(path)
    doc = parsed["document"]
    if "native_version" in expected:
        # The ordinary-mask corpus has no retained sources or private artwork.
        # Its explicit expected wrapper must hold through edits and reopen too.
        assert parsed["version"] == expected["native_version"], (path, parsed["version"], expected["native_version"])
        # 10 is drdc::LEGACY_DRDC_VERSION (the plain byte body) and 28 below is
        # mask_outside_native::BYTE_VERSION (the conditional MOUT tag): fixed tags
        # under the identity wrapper `read_native` peels, never the writer version.
        assert doc["body_version"] == 10
        assert not any(parsed[key] for key in ("assets", "bindings", "resources", "artwork"))
        outside = [index for index, layer in enumerate(expected["layers"])
                   if layer.get("mask") is not None and layer["mask"]["outside_coverage"] == 255]
        assert doc["mask_version"] == (28 if outside else None)
        assert doc.get("mask_outside_records", []) == outside
    assert (doc["width"], doc["height"], doc["depth"]) == (*expected["canvas"], 8)
    assert set(doc["layers"]) == set(range(len(expected["layers"])))
    for index, wanted in enumerate(expected["layers"]):
        row = doc["layers"][index]
        assert row["name"] == wanted["name"]
        assert row["kind"] == (wanted["kind"] == "group")
        assert row["tree_depth"] == wanted["depth"]
        offset = expected["moved"]["offset"] if moved and index == expected["moved"]["layer"] else wanted["offset"]
        if moved:
            offset = expected["moved"].get("layer_offsets", {}).get(str(index), offset)
        assert (row["x"], row["y"]) == tuple(offset), (index, row, wanted)
        assert row["visible"] == wanted["visible"]
        assert row["opacity"] == struct.unpack("<f", struct.pack("<f", wanted["opacity"]))[0]
        mode = wanted["blend"]
        assert row["blend"] == (MODES[mode.lower()] if isinstance(mode, str) else mode)
        mask = wanted.get("mask")
        if mask is None:
            assert row["mask"] is None
        else:
            actual = row["mask"]
            assert actual is not None, (index, wanted)
            assert (actual["x"], actual["y"]) == tuple(mask["offset"])
            assert (actual["width"], actual["height"]) == tuple(mask["size"])
            assert actual["data"] == bytes(mask["coverage"])
            for key in ("enabled", "linked", "density", "feather", "outside_coverage"):
                assert actual[key] == mask[key], (index, key, actual, mask)
        if wanted["kind"] == "group":
            assert row["expanded"] == wanted["open"]
        decoded = support.png_samples(doc["fallbacks"][index])
        assert (decoded["width"], decoded["height"], decoded["depth"]) == (*wanted["size"], 8)
        assert tuple(decoded["samples"]) == tuple(wanted["rgba"]), (index, decoded, wanted)
    return digest(path)


def history(call):
    value = call("list_history")
    return value["current_state_id"], value["undo_count"], value["redo_count"]


def refused_case(app, source, expected, report, folder):
    assert report["layered_available"] is False and report["stored_composite_available"]
    reason = expected["layered_refusal_contains"]
    assert reason in report["layered_refusal"], report
    error = app.call("open_document", path=str(source), psd_mode="layers",
                     psd_fingerprint=report["source_fingerprint"], fail=True)
    assert reason in error, error
    opened = app.call("open_document", path=str(source), psd_mode="composite",
                      psd_fingerprint=report["source_fingerprint"])
    assert opened["psd_import"]["chosen_mode"] == "composite"
    call = support.call_for(app, opened["document"]["id"], folder / "calls.jsonl")
    pixels(call, folder / "composite.png", expected["canvas"], expected["stored_composite_rgba"])
    return {"source_sha256": digest(source), "layered_refused": True, "refusal": error,
            "explicit_composite_exact": True, "preflight": report}


def case(app, source, expected, folder):
    folder.mkdir()
    report = app.call("inspect_psd", path=str(source))
    if expected.get("layered_available") is False:
        return refused_case(app, source, expected, report, folder)
    assert report["layered_available"] and report["stored_composite_available"], report
    assert report["requires_explicit_choice"]
    assert not any("cannot retain its off-canvas pixels" in s for s in report["verified_losses"])
    opened = app.call("open_document", path=str(source), psd_mode="layers",
                      psd_fingerprint=report["source_fingerprint"])
    assert opened["psd_import"]["chosen_mode"] == "layers"
    call = support.call_for(app, opened["document"]["id"], folder / "calls.jsonl")
    original = folder / "original.rz"
    sha = stored(call, original, expected)
    pixels(call, folder / "original.png", expected["canvas"], expected["composite_rgba"])
    before = history(call)
    if "moved" in expected:
        move = expected["moved"]
        call("set_layer_properties", index=move["layer"], offset_x=move["offset"][0], offset_y=move["offset"][1])
        assert history(call)[1] == before[1] + 1
        moved_sha = stored(call, folder / "moved.rz", expected, moved=True)
        pixels(call, folder / "moved.png", expected["canvas"], move["composite_rgba"])
        call("undo")
        assert history(call)[0] == before[0]
        assert stored(call, folder / "undone.rz", expected) == sha
        call("redo")
        assert stored(call, folder / "redone.rz", expected, moved=True) == moved_sha
        call("undo")
    reopened = app.call("open_document", path=str(original))["document"]["id"]
    again = support.call_for(app, reopened, folder / "reopened-calls.jsonl")
    assert stored(again, folder / "reopened.rz", expected) == sha
    pixels(again, folder / "reopened.png", expected["canvas"], expected["composite_rgba"])
    # Opening a fresh URL keeps the composite choice distinct from the open layer document.
    copy = folder / "composite-choice.psd"
    copy.write_bytes(source.read_bytes())
    inspected = app.call("inspect_psd", path=str(copy))
    composite = app.call("open_document", path=str(copy), psd_mode="composite",
                        psd_fingerprint=inspected["source_fingerprint"])
    assert composite["psd_import"]["chosen_mode"] == "composite"
    flat = support.call_for(app, composite["document"]["id"], folder / "composite-calls.jsonl")
    pixels(flat, folder / "composite.png", expected["canvas"], expected["stored_composite_rgba"])
    return {"source_sha256": digest(source), "native_sha256": sha, "preflight": report,
            "complete_rgba_and_bounds_exact": True, "native_roundtrip_exact": True,
            "move_undo_redo_checked": "moved" in expected, "independent_composite_exact": True}


def dependencies():
    """The code files `dependency_sha256` hashes: the script's whole load chain."""
    return [Path(__file__),
            ROOT / "scripts/host-source-probe/support.py", ROOT / "scripts/live-filter-probe/native.py",
            ROOT / "scripts/phase5_support.py", ROOT / "scripts/precision-probe/fixtures.py",
            ROOT / "scripts/precision-probe/retained_fixtures.py",
            ROOT / "scripts/precision-probe/retained_linked_fixtures.py", ROOT / "scripts/raw-probe/native.py",
            ROOT / "scripts/retouch-probe/fixtures.py", ROOT / "scripts/retouch-probe/support.py",
            ROOT / "scripts/verification_isolation.py",
            ROOT / "scripts/app_under_test.py", ROOT / "scripts/verify_phase1.py",
            ROOT / "scripts/verify_phase2.py", ROOT / "scripts/verify_phase8b.py",
            *sorted((ROOT / "core/src").rglob("*.rs"))]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--fixtures", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    if output.is_relative_to(ROOT) or args.output.is_symlink():
        parser.error("output must be new and outside the repository")
    manifests = {p.name.removesuffix(".psd.json"): p for p in args.fixtures.resolve().glob("*.psd.json")}
    assert set(manifests) == CASES, manifests
    executable = (args.app / "Contents/MacOS/Doctored" if args.app.suffix == ".app" else args.app).resolve()
    output.mkdir(exist_ok=False)
    fixture_hashes = {str(p): digest(p) for manifest in manifests.values()
                      for p in (manifest, manifest.with_suffix(""))}
    report = {"app": str(executable), "app_sha256": digest(executable),
              "script_sha256": digest(Path(__file__)), "fixture_sha256": fixture_hashes, "cases": {},
              "dependency_sha256": {str(p): digest(p) for p in dependencies()}}
    with support.HostApp(output / "app", output / "recovery", executable) as app:
        for name, manifest in sorted(manifests.items()):
            expected = json.loads(manifest.read_text())
            assert expected["canvas"] == [2, 2]
            result = case(app, manifest.with_suffix(""), expected, output / name)
            result["expectation_sha256"] = digest(manifest)
            report["cases"][name] = result
    assert all(digest(Path(p)) == sha for p, sha in fixture_hashes.items())
    report["fixtures_unchanged"] = True
    assert report["cases"]["offcanvas_rle"]["move_undo_redo_checked"]
    assert report["cases"]["ending_zero_rle"]["move_undo_redo_checked"]
    report["checks_passed"] = ["twelve complete raw/RLE raster imports", "exact signed bounds and raw RGBA",
                              "hidden RGB and absent-alpha/grayscale semantics", "wholly off-canvas move and Undo/Redo",
                              "native reopen and exact roundtrip", "folder topology and properties",
                              "explicit independent saved composite", "malformed RLE refuses layers with explicit composite available"]
    (output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"output": str(output), "app_sha256": report["app_sha256"],
                      "checks_passed": report["checks_passed"]}, indent=2))


if __name__ == "__main__":
    main()
