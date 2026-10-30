#!/usr/bin/env python3
"""Independent Release checks for the next native separable blend family.

Asymmetric RGBA16 fixtures exercise branch neighbors, endpoints and partial/
tiny alpha. Equations follow https://www.w3.org/TR/compositing-1/#blendingseparable.
Product math is not imported into this oracle.
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
spec = importlib.util.spec_from_file_location("separable_support", ROOT / "scripts/retouch-probe/support.py")
support = importlib.util.module_from_spec(spec)
spec.loader.exec_module(support)
MODES = {"Overlay": 3, "Soft Light": 4, "Hard Light": 5, "Darken": 6,
         "Lighten": 7, "Difference": 8, "Exclusion": 9}
BACKDROP = [(0, 65535, 32767, 65535), (65535, 0, 32768, 65535),
            (16383, 16384, 16385, 65535), (32767, 32768, 32769, 65535),
            (8192, 49152, 23567, 32771), (50000, 23000, 4000, 1),
            (30000, 50000, 7000, 7), (65001, 55003, 45005, 0),
            (18000, 37000, 59000, 65535), (10001, 20003, 30007, 12345),
            (65535, 65535, 65535, 0), (17, 32767, 65501, 65535)]
FOREGROUND = [(65535, 0, 32768, 65535), (32767, 32768, 32769, 32768),
              (49152, 50000, 65535, 65535), (16384, 16383, 50000, 65535),
              (57000, 10001, 30000, 45000), (12000, 43000, 61000, 32769),
              (62000, 36000, 11000, 1), (15001, 25003, 35005, 32771),
              (65000, 31000, 1000, 0), (255, 256, 257, 7),
              (1003, 2005, 3007, 0), (1, 16384, 32768, 65535)]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def samples(pixels):
    return tuple(code for pixel in pixels for code in pixel)


def png(path, pixels):
    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))
    rows = b"".join(b"\0" + struct.pack(">16H", *samples(pixels[y*4:y*4+4])) for y in range(3))
    path.write_bytes(b"\x89PNG\r\n\x1a\n"
                     + chunk(b"IHDR", struct.pack(">IIBBBBB", 4, 3, 16, 6, 0, 0, 0))
                     + chunk(b"sRGB", b"\0") + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))


def blend(mode, b, s):
    if mode == "Overlay":
        return 2*b*s if b <= .5 else 1-2*(1-b)*(1-s)
    if mode == "Hard Light":
        return 2*b*s if s <= .5 else 1-2*(1-b)*(1-s)
    if mode == "Soft Light":
        if s <= .5:
            return b-(1-2*s)*b*(1-b)
        d = ((16*b-12)*b+4)*b if b <= .25 else math.sqrt(b)
        return b+(2*s-1)*(d-b)
    if mode == "Darken":
        return min(b, s)
    if mode == "Lighten":
        return max(b, s)
    if mode == "Difference":
        return abs(b-s)
    if mode == "Exclusion":
        return b+s-2*b*s
    raise AssertionError(mode)


def expected(mode):
    output = []
    for back, front in zip(BACKDROP, FOREGROUND, strict=True):
        ba, sa = back[3]/65535, front[3]/65535*.625
        alpha = sa+ba*(1-sa)
        if alpha == 0:
            output.extend((0, 0, 0, 0))
            continue
        for bc, sc in zip(back[:3], front[:3], strict=True):
            b, s = bc/65535, sc/65535
            premultiplied = sa*((1-ba)*s+ba*blend(mode, b, s))+(1-sa)*ba*b
            output.append(math.floor(65535*premultiplied/alpha+.5))
        output.append(math.floor(65535*alpha+.5))
    return tuple(output)


def history(call):
    value = call("list_history")
    return value["current_state_id"], value["undo_count"], value["redo_count"]


def rendered(call, path, mode):
    call("save_copy", path=str(path))
    value = support.png_samples(path.read_bytes())
    assert (value["width"], value["height"], value["depth"]) == (4, 3, 16)
    delta = max(abs(a-b) for a, b in zip(value["samples"], expected(mode), strict=True))
    assert delta <= 1, (mode, delta, value["samples"], expected(mode))
    return delta


def native(call, path, mode, base_name, baseline):
    call("save_copy", path=str(path))
    result = support.read_native(path)
    doc = result["document"]
    assert result["version"] == 27 and (doc["width"], doc["height"], doc["depth"]) == (4, 3, 16)
    assert doc["profile"] == baseline["document"]["profile"]
    assert len(doc["layers"]) == len(doc["fallbacks"]) == 2
    for index, expected_pixels in enumerate((BACKDROP, FOREGROUND)):
        row = doc["layers"][index]
        value = support.png_samples(doc["fallbacks"][index])
        assert (value["width"], value["height"], value["depth"]) == (4, 3, 16)
        assert tuple(value["samples"]) == samples(expected_pixels)
        assert row["name"] == (base_name if index == 0 else mode + " native")
        assert (row["width"], row["height"]) == (4, 3)
        assert row["blend"] == (0 if index == 0 else MODES[mode])
        assert row["opacity"] == (1 if index == 0 else .625)
        assert row["visible"] and row["mask"] is None and (row["x"], row["y"]) == (0, 0)
    # A placed PNG is an ordinary raster layer: the file keeps no master of it.
    assert not result["assets"] and not result["bindings"]
    for key in ("assets", "bindings", "resources"):
        assert result[key] == baseline[key], key
    return digest(path)


def exercise(app, call, mode, folder):
    folder.mkdir()
    before = history(call)
    base_name = call("get_document")["layers"][0]["name"]
    baseline_path = folder / "baseline.rz"
    call("save_copy", path=str(baseline_path))
    baseline = support.read_native(baseline_path)
    baseline_sha = digest(baseline_path)

    def undo(label):
        call("undo")
        assert history(call)[0] == before[0]
        saved = folder / (label + ".rz")
        call("save_copy", path=str(saved))
        assert digest(saved) == baseline_sha

    app.call("start_recording")
    call("set_layer_properties", index=1, blend_mode=mode, opacity=.625, name=mode + " native")
    action = app.call("stop_recording", name="Native separable " + mode)
    assert [step["tool"] for step in action["steps"]] == ["set_layer_properties"]
    assert action["steps"][0]["arguments"]["blend_mode"] == mode
    assert history(call)[1] == before[1]+1
    error = rendered(call, folder / "output.png", mode)
    original = folder / "output.rz"
    sha = native(call, original, mode, base_name, baseline)
    undo("undone")
    call("redo")
    assert native(call, folder / "redo.rz", mode, base_name, baseline) == sha
    undo("before-replay")
    assert call("run_action", name="Native separable " + mode)["ok"]
    assert history(call)[1] == before[1]+1
    assert native(call, folder / "action.rz", mode, base_name, baseline) == sha
    identifier = app.call("open_document", path=str(original), precision="preserve")["document"]["id"]
    reopened = support.call_for(app, identifier, folder / "reopen-calls.jsonl")
    assert native(reopened, folder / "reopened.rz", mode, base_name, baseline) == sha
    assert rendered(reopened, folder / "reopened.png", mode) == error
    undo("action-undone")
    return {"maximum_u16_error": error, "native_sha256": sha, "exact_sources_and_plain_raster_layer": True,
            "one_undo_redo_and_action_replay": True, "native_reopen_exact": True}


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
        parser.error("output must be new and outside the repository")
    output.mkdir(exist_ok=False)
    back, front = output / "backdrop16.png", output / "foreground16.png"
    png(back, BACKDROP)
    png(front, FOREGROUND)
    executable = (args.app / "Contents/MacOS/Doctored" if args.app.suffix == ".app" else args.app).resolve()
    report = {"app": str(executable), "app_sha256": digest(executable), "script_sha256": digest(Path(__file__)),
              "fixture_sha256": {p.name: digest(p) for p in (back, front)}, "modes": {},
              "dependency_sha256": {str(p): digest(p) for p in dependencies()}}
    with support.HostApp(output / "app", output / "recovery", executable) as app:
        identifier = app.call("open_document", path=str(back), precision="preserve")["document"]["id"]
        call = support.call_for(app, identifier, output / "calls.jsonl")
        assert call("get_precision")["mode"] == "rgba16"
        placed = call("place_image", path=str(front), storage="embedded")
        assert placed["layer"] == 1 and placed["source"] is None, placed
        call("set_active_layer", index=1)
        for mode in MODES:
            report["modes"][mode] = exercise(app, call, mode, output / mode.lower().replace(" ", "-"))
    assert all(digest(p) == report["fixture_sha256"][p.name] for p in (back, front))
    report["fixtures_unchanged"] = True
    report["checks_passed"] = ["seven asymmetric native blend oracles", "branch neighbors/endpoints and fractional/tiny alpha",
                              "exact raw RGBA16 of a plain placed layer", "version27 native roundtrip",
                              "atomic property edit and one Undo", "Redo and exact Actions replay"]
    (output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"output": str(output), "modes": report["modes"], "checks_passed": report["checks_passed"]}, indent=2))


if __name__ == "__main__":
    main()
