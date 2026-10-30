#!/usr/bin/env python3
"""Verify PSD folder-state import through an isolated Release application.

Fixtures and handwritten expectations are exported by core/tests/psd_group_import.rs
with DR_PHASE10A_FIXTURES. Uses generated documents, dummy credentials, an owned
process and private Actions, recovery and preset stores.
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
spec = importlib.util.spec_from_file_location("phase10a_support", ROOT / "scripts/retouch-probe/support.py")
support = importlib.util.module_from_spec(spec)
spec.loader.exec_module(support)
MODES = {"normal": 0, "multiply": 1, "screen": 2, "pass through": 27}
EXPECTED_CASES = {"group-hidden", "group-multiply", "group-normal", "group-transparent",
                  "group-pass", "group-isolated", "group-nested"}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def pixels(call, path, expected):
    call("save_copy", path=str(path))
    value = support.png_samples(path.read_bytes())
    assert (value["width"], value["height"], value["depth"]) == (1, 1, 8), value
    assert tuple(value["samples"]) == tuple(expected), (path, value["samples"], expected)


def topology(case):
    if case == "group-nested":
        return [("Backdrop", False, 0), ("Inside base", False, 1), ("Art", False, 2),
                ("Inner", True, 1), ("Outer", True, 0)]
    child = "Child multiply" if case in ("group-pass", "group-isolated") else "Art"
    return [("Backdrop", False, 0), (child, False, 1), ("Folder", True, 0)]


def stored_groups(call, path, expected, case):
    call("save_copy", path=str(path))
    parsed = support.read_native(path)
    assert parsed["document"]["depth"] == 8
    rows = parsed["document"]["layers"]
    assert [(r["name"], r["kind"], r["tree_depth"]) for r in rows.values()] == topology(case)
    groups = {r["name"]: r for r in rows.values() if r["kind"]}
    assert set(groups) == {r["name"] for r in expected}, groups
    for wanted in expected:
        actual = groups[wanted["name"]]
        assert actual["visible"] == wanted["visible"]
        assert actual["expanded"] == wanted["open"]
        # Stored PSD opacity is the original u8 fraction converted to f32.
        opacity = struct.unpack("<f", struct.pack("<f", wanted["opacity"]))[0]
        assert actual["opacity"] == opacity, (actual, wanted)
        mode = wanted["blend"]
        assert actual["blend"] == (MODES[mode.lower()] if isinstance(mode, str) else mode)
    return rows


def verify_case(app, source, expected, folder):
    folder.mkdir()
    preflight = app.call("inspect_psd", path=str(source))
    assert preflight["layered_available"] and preflight["stored_composite_available"], preflight
    assert preflight["requires_explicit_choice"]
    opened = app.call("open_document", path=str(source), psd_mode="layers",
                      psd_fingerprint=preflight["source_fingerprint"])
    assert opened["psd_import"]["chosen_mode"] == "layers"
    call = support.call_for(app, opened["document"]["id"], folder / "calls.jsonl")
    info = call("get_document")
    shown = {r["name"]: r for r in info["layers"] if r["kind"] == "group"}
    for group in expected["groups"]:
        assert shown[group["name"]]["visible"] == group["visible"]
        assert shown[group["name"]]["open"] == group["open"]
    pixels(call, folder / "imported.png", expected["layered_rgba"])
    original = folder / "imported.rz"
    rows = stored_groups(call, original, expected["groups"], source.stem)

    group_index = next(index for index, row in rows.items() if row["kind"])
    original_name = rows[group_index]["name"]
    before = call("list_history")
    call("set_layer_properties", index=group_index, name="Verified folder edit")
    changed = call("list_history")
    assert changed["undo_count"] == before["undo_count"] + 1
    call("undo")
    stored_groups(call, folder / "undone.rz", expected["groups"], source.stem)
    assert digest(original) == digest(folder / "undone.rz")
    call("redo")
    assert call("get_document")["layers"][group_index]["name"] == "Verified folder edit"
    call("undo")
    assert call("get_document")["layers"][group_index]["name"] == original_name

    reopened = app.call("open_document", path=str(original))["document"]["id"]
    again = support.call_for(app, reopened, folder / "reopened-calls.jsonl")
    stored_groups(again, folder / "reopened.rz", expected["groups"], source.stem)
    assert digest(original) == digest(folder / "reopened.rz")
    pixels(again, folder / "reopened.png", expected["layered_rgba"])
    return {"source": str(source), "source_sha256": digest(source), "preflight": preflight,
            "expected_groups": expected["groups"], "expected_rgba": expected["layered_rgba"],
            "native_sha256": digest(original), "undo_redo_and_reopen_exact": True}


def verify_refusals(app, fixtures, output):
    # A valid folder blend-key field becomes an unknown same-length value.
    original = fixtures / "group-multiply.psd"
    data = original.read_bytes()
    assert b"8BIMmul " in data
    source = output / "unknown-folder-blend.psd"
    source.write_bytes(data.replace(b"8BIMmul ", b"8BIMzzzz", 1))
    report = app.call("inspect_psd", path=str(source))
    assert not report["layered_available"] and report["stored_composite_available"], report
    assert "group blend key" in report["layered_refusal"], report
    refused = app.call("open_document", path=str(source), psd_mode="layers",
                       psd_fingerprint=report["source_fingerprint"], fail=True)
    assert "group blend key" in refused, refused
    opened = app.call("open_document", path=str(source), psd_mode="composite",
                      psd_fingerprint=report["source_fingerprint"])
    assert opened["psd_import"]["chosen_mode"] == "composite"
    call = support.call_for(app, opened["document"]["id"], output / "composite-calls.jsonl")
    expected = json.loads((fixtures / "group-multiply.psd.json").read_text())
    pixels(call, output / "explicit-composite.png", expected["stored_composite_rgba"])
    stale = output / "stale-fingerprint.psd"
    stale.write_bytes(data)
    error = app.call("open_document", path=str(stale), psd_mode="layers",
                     psd_fingerprint="stale", fail=True)
    assert "changed" in error.lower(), error
    return {"unknown_blend_refusal": refused, "explicit_composite_preserved": True,
            "stale_fingerprint_refusal": error}


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
        parser.error("output must be new, outside the repository, and not a symlink")
    fixtures = args.fixtures.resolve()
    manifests = {p.name.removesuffix(".psd.json"): p for p in fixtures.glob("*.psd.json")}
    assert set(manifests) == EXPECTED_CASES, manifests
    executable = (args.app / "Contents/MacOS/Doctored" if args.app.suffix == ".app" else args.app).resolve()
    output.mkdir(exist_ok=False)
    receipt = {"app": str(executable), "app_sha256": digest(executable),
               "verification_script_sha256": digest(Path(__file__)), "cases": [],
               "dependency_sha256": {str(p): digest(p) for p in dependencies()}}
    with support.HostApp(output / "app", output / "recovery", executable) as app:
        for name, manifest in sorted(manifests.items()):
            expected = json.loads(manifest.read_text())
            assert expected["size"] == [1, 1] and expected["groups"]
            result = verify_case(app, manifest.with_suffix(""), expected, output / name)
            result["expectation_sha256"] = digest(manifest)
            receipt["cases"].append(result)
        receipt["refusals"] = verify_refusals(app, fixtures, output)
    receipt["checks_passed"] = ["seven explicit layered imports", "independent 1px composites",
                                "exact persisted folder properties", "one-edit Undo/Redo",
                                "native reopen and exact roundtrip", "unknown blend layered refusal",
                                "explicit saved composite", "stale source refusal"]
    (output / "result.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({"output": str(output), "app_sha256": receipt["app_sha256"],
                      "cases": len(receipt["cases"]), "checks_passed": receipt["checks_passed"]}, indent=2))


if __name__ == "__main__":
    main()
