#!/usr/bin/env python3
"""Verify an explicitly supplied final Release app through isolated external MCP.

Copies/signs the app under a private temporary root and changes only that copy's
bundle identifier. Uses the existing Phase6 mask suite, then checks that
refine_selection names each field it does not take as unknown (trimap,
trimap_strokes, foreground, color_space, a typo) and exact basic refinement
Undo/Redo. No build is started;
only owned Popen children are signalled by the shared verification harness.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile
import uuid

# Keep both this process's local imports and the child harness out of the repo.
sys.dont_write_bytecode = True
os.environ["PYTHONDONTWRITEBYTECODE"] = "1"

ROOT = Path(__file__).resolve().parents[1]


def fingerprint(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def refinement_gate(folder):
    # Import only after DR_VERIFY_APP points at the newly copied private app.
    from phase5_support import App, fixture, write_report
    from verify_phase6 import render_to

    folder.mkdir()
    source = folder / "source.png"
    fixture(source, width=64, height=48)
    source_hash = fingerprint(source)
    with App(folder / "app", folder / "recovery", interval=3600) as app:
        document = app.open(source)
        call = lambda tool, **args: app.call(tool, document_id=document, **args)
        points = [{"x": x, "y": y} for y in range(48) for x in range(64)]
        coverage = lambda: call("get_selection", points=points)["samples"]
        original = render_to(app, document, layer=0)
        call("select_rect", x=12, y=10, width=29, height=24)
        initial = coverage()
        history = call("list_history")
        errors = {}
        for key, value in [("trimap", {}), ("trimap_strokes", []),
                           ("foreground", True), ("color_space", "linear"), ("typo", 1)]:
            error = call("refine_selection", **{key: value}, fail=True)
            expected = "unknown"
            assert expected in error.lower(), error
            errors[key] = error
            after = call("list_history")
            for field in ("current_state_id", "undo_count", "redo_count"):
                assert after[field] == history[field], (key, field, history, after)
            assert len(after["states"]) == len(history["states"])
            assert coverage() == initial
        result = call("refine_selection", feather=2)
        assert result["changed"] is True, result
        refined = coverage()
        assert refined != initial and any(0 < point["coverage"] < 255 for point in refined)
        after = call("list_history")
        assert after["undo_count"] == history["undo_count"] + 1
        assert len(after["states"]) == len(history["states"]) + 1
        assert render_to(app, document, layer=0) == original
        assert not call("get_document")["unsaved_changes"]
        call("undo")
        assert call("list_history")["current_state_id"] == history["current_state_id"]
        assert coverage() == initial
        call("redo")
        assert coverage() == refined
        assert not call("get_document")["unsaved_changes"]
        assert fingerprint(source) == source_hash
        report = {"passed": True, "endpoint": app.client.socket_path, "refusals": errors,
                  "before_history": history, "after_history": after,
                  "source_sha256": source_hash, "unchanged_raw_pixels": original}
        write_report(folder / "report.json", report)
    print("PASS: final Release external MCP rejects advanced/unknown parameters atomically; basic feather has exact clean one-step Undo/Redo", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True, help="Final built Release .app bundle; no build is performed")
    parser.add_argument("--keep", action="store_true")
    arguments = parser.parse_args()
    source_app = arguments.app.resolve()
    source_executable = source_app / "Contents/MacOS/Doctored"
    assert source_app.suffix == ".app" and source_executable.is_file(), source_app
    folder = Path(tempfile.mkdtemp(prefix="rasterize-phase6c-release-"))
    print(f"Final Release verification scratch: {folder}", flush=True)
    try:
        executable_hash = fingerprint(source_executable)
        app = folder / "Doctored.app"
        shutil.copytree(source_app, app)
        executable = app / "Contents/MacOS/Doctored"
        assert fingerprint(executable) == executable_hash, "Private copy must begin with the exact Release executable"
        info = app / "Contents/Info.plist"
        properties = plistlib.loads(info.read_bytes())
        identifier = "com.kgalligan.doctored.phase6c-verification." + uuid.uuid4().hex
        properties["CFBundleIdentifier"] = identifier
        properties["CFBundleDisplayName"] = "Doctored Phase 6C Verification"
        info.write_bytes(plistlib.dumps(properties))
        subprocess.run(["codesign", "--force", "--deep", "--sign", "-", str(app)], check=True)
        os.environ["DR_VERIFY_APP"] = str(executable)
        os.environ["ANTHROPIC_API_KEY"] = "dummy"
        (folder / "release.json").write_text(json.dumps({
            "source_app": str(source_app), "source_executable_sha256": executable_hash,
            "private_executable_sha256": fingerprint(executable), "private_bundle_id": identifier,
        }, indent=2) + "\n")
        with (folder / "mask-suite.log").open("w") as log:
            subprocess.run([sys.executable, str(ROOT / "scripts/verify_phase6.py"), "--suite", "masks", "--keep"],
                           env=os.environ.copy(), stdout=log, stderr=subprocess.STDOUT, check=True, timeout=300)
        print((folder / "mask-suite.log").read_text(), end="")
        refinement_gate(folder / "refinement")
        assert fingerprint(source_executable) == executable_hash, "Verification must not modify the source app"
    except BaseException:
        print(f"Failure artifacts retained: {folder}", flush=True)
        raise
    if not arguments.keep:
        shutil.rmtree(folder)


if __name__ == "__main__":
    main()
