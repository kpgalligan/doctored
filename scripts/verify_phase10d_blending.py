#!/usr/bin/env python3
"""Verify native layer blending in an owned Release app with independent pixels.

Uses an authored 3x2 RGBA16 PNG, dummy credentials and private Action/recovery/
preset stores. This qualifies the limited native blend workflow, not all10D.
"""
import argparse
import hashlib
import importlib.util
import json
import math
from pathlib import Path
import struct
import sys
import zlib

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("phase10d_support", ROOT / "scripts/retouch-probe/support.py")
support = importlib.util.module_from_spec(spec)
spec.loader.exec_module(support)
PIXELS = [(101, 202, 303, 1), (1101, 2202, 3303, 7), (11001, 22002, 33003, 32769),
          (4001, 5003, 6005, 65535), (65001, 55003, 45005, 0), (101, 65534, 513, 65535)]
SAMPLES = tuple(code for pixel in PIXELS for code in pixel)
MODES = {"Normal": 0, "Multiply": 1, "Screen": 2}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def fixture(path):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    rows = b"".join(b"\0" + struct.pack(">12H", *SAMPLES[y*12:(y+1)*12]) for y in range(2))
    path.write_bytes(b"\x89PNG\r\n\x1a\n"
                     + chunk(b"IHDR", struct.pack(">IIBBBBB", 3, 2, 16, 6, 0, 0, 0))
                     + chunk(b"sRGB", b"\0") + chunk(b"IDAT", zlib.compress(rows))
                     + chunk(b"IEND", b""))


def expected(mode):
    result = []
    for pixel in PIXELS:
        rgb = [v / 65535 for v in pixel[:3]]
        base_alpha = pixel[3] / 65535
        top_alpha = base_alpha * .625
        alpha = top_alpha + base_alpha * (1 - top_alpha)
        if alpha == 0:
            result.extend((0, 0, 0, 0))
            continue
        for color in rgb:
            blended = color * color if mode == "Multiply" else 2 * color - color * color
            numerator = (top_alpha * ((1 - base_alpha) * color + base_alpha * blended)
                         + (1 - top_alpha) * base_alpha * color)
            result.append(math.floor(65535 * numerator / alpha + .5))
        result.append(math.floor(65535 * alpha + .5))
    return tuple(result)


def rendered(call, path, mode):
    call("save_copy", path=str(path))
    image = support.png_samples(path.read_bytes())
    assert (image["width"], image["height"], image["depth"]) == (3, 2, 16), image
    actual = tuple(image["samples"])
    maximum = max(abs(a-b) for a, b in zip(actual, expected(mode), strict=True))
    assert maximum <= 1, (mode, actual, expected(mode), maximum)
    return maximum


def native(call, path, mode, base_name):
    call("save_copy", path=str(path))
    parsed = support.read_native(path)
    document = parsed["document"]
    assert parsed["version"] == 26 and document["depth"] == 16
    assert len(document["layers"]) == len(document["fallbacks"]) == 2
    for index, encoded in document["fallbacks"].items():
        samples = support.png_samples(encoded)
        assert samples["depth"] == 16 and tuple(samples["samples"]) == SAMPLES
        row = document["layers"][index]
        assert row["name"] == (base_name if index == 0 else mode + "16")
        assert row["mask"] is None and row["visible"] and (row["x"], row["y"]) == (0, 0)
        assert row["blend"] == (0 if index == 0 else MODES[mode])
        assert row["opacity"] == (1 if index == 0 else .625)
    return digest(path)


def history(call):
    row = call("list_history")
    return row["current_state_id"], row["undo_count"], row["redo_count"]


def mode_workflow(app, call, mode, folder):
    folder.mkdir()
    baseline = history(call)
    base_name = call("get_document")["layers"][0]["name"]
    app.call("start_recording")
    call("set_layer_properties", index=1, blend_mode=mode, opacity=.625, name=mode + "16")
    action = app.call("stop_recording", name="Native " + mode)
    assert [r["tool"] for r in action["steps"]] == ["set_layer_properties"], action
    assert action["steps"][0]["arguments"]["blend_mode"] == mode, action
    assert history(call)[1] == baseline[1] + 1
    delta = rendered(call, folder / "rendered.png", mode)
    original = folder / "native.rz"
    sha = native(call, original, mode, base_name)
    call("undo")
    assert history(call)[0] == baseline[0]
    call("redo")
    assert native(call, folder / "redone.rz", mode, base_name) == sha
    call("undo")
    assert call("run_action", name="Native " + mode)["ok"]
    assert history(call)[1] == baseline[1] + 1
    assert native(call, folder / "action-replayed.rz", mode, base_name) == sha

    identifier = app.call("open_document", path=str(original), precision="preserve")["document"]["id"]
    again = support.call_for(app, identifier, folder / "reopened-calls.jsonl")
    assert again("get_precision")["mode"] == "rgba16"
    assert native(again, folder / "reopened.rz", mode, base_name) == sha
    assert rendered(again, folder / "reopened.png", mode) == delta
    call("undo")
    assert history(call)[0] == baseline[0]
    return {"mode": mode, "maximum_u16_difference_from_f64_oracle": delta,
            "native_sha256": sha, "action": action, "source_samples_exact": True,
            "one_undo_redo_and_action_replay": True, "native_reopen_exact": True}


def atomic_refusals(call, output):
    before = history(call)
    original = output / "before-refusals.rz"
    call("save_copy", path=str(original))
    selection = call("get_selection")
    dirty = call("get_document")["unsaved_changes"]
    cases = [
        ({"blend_mode": "Pass Through", "name": "Must not publish"}, "Pass Through requires a group"),
        ({"blend_mode": "Multiply", "name": "x" * 65537}, "64 KiB"),
        ({"blend_mode": "Multiply", "opacity": "invalid"}, "Invalid native layer property: opacity"),
        ({"blend_mode": "Multiply", "visible": [True]}, "Invalid native layer property: visible"),
        ({"blend_mode": "Multiply", "offset_x": "invalid"}, "Invalid native layer property: offset_x"),
        ({"blend_mode": "Multiply", "open": False}, "open applies to groups only"),
    ]
    refusals = []
    for index, (arguments, fragment) in enumerate(cases):
        error = call("set_layer_properties", index=1, fail=True, **arguments)
        assert fragment.lower() in error.lower(), (fragment, error)
        assert history(call) == before
        assert call("get_selection") == selection
        assert call("get_document")["unsaved_changes"] == dirty
        saved = output / f"refusal-{index}.rz"
        call("save_copy", path=str(saved))
        assert digest(saved) == digest(original)
        refusals.append({"case": index, "refusal": error, "state_and_native_bytes_unchanged": True})
    return refusals


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
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    if output.is_relative_to(ROOT) or args.output.is_symlink():
        parser.error("output must be new, outside the repository, and not a symlink")
    output.mkdir(exist_ok=False)
    source = output / "authored16.png"
    fixture(source)
    executable = (args.app / "Contents/MacOS/Doctored" if args.app.suffix == ".app" else args.app).resolve()
    receipt = {"app": str(executable), "app_sha256": digest(executable),
               "verification_script_sha256": digest(Path(__file__)), "fixture_sha256": digest(source),
               "dependency_sha256": {str(p): digest(p) for p in dependencies()},
               "source_samples": list(SAMPLES), "modes": []}
    with support.HostApp(output / "app", output / "recovery", executable) as app:
        identifier = app.call("open_document", path=str(source), precision="preserve")["document"]["id"]
        call = support.call_for(app, identifier, output / "calls.jsonl")
        assert call("get_precision")["mode"] == "rgba16"
        call("duplicate_layer", index=0)
        assert len(call("get_document")["layers"]) == 2
        call("set_active_layer", index=1)
        for mode in ("Multiply", "Screen"):
            receipt["modes"].append(mode_workflow(app, call, mode, output / mode.lower()))
        receipt["atomic_refusals"] = atomic_refusals(call, output)
    receipt["checks_passed"] = ["native Multiply and Screen independent f64 pixels",
                                "exact source RGB/alpha including hidden samples", "native v26 reopen",
                                "compound property edit in one Undo", "Redo and exact Actions replay",
                                "six atomic refusals preserve bytes/history/selection/dirty state"]
    (output / "result.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({"output": str(output), "app_sha256": receipt["app_sha256"],
                      "checks_passed": receipt["checks_passed"]}, indent=2))


if __name__ == "__main__":
    main()
