#!/usr/bin/env python3
"""Debug-app drive for the Rasterize-to-Doctored rename (R1 and R2).

Against one owned instance (`phase5_support.App`: a scratch agent socket and
every `DR_*_DIR` private), for the committed byte and typed `.rz` fixtures
(`core/tests/fixtures/rz_gradient_103`):

- the legacy file opens, and File > Save on it is not in place: its Save types
  lack the legacy type id, and Save As leads with `com.kgalligan.doctored-document`;
- `save_copy` to `<scratch>/x.doctored` with no `format` writes a file that
  starts `DRDC` and answers `"format": "doctored"`;
- that file reopens and renders the same pixels as the `.rz`;
- `save_copy` with the legacy token `format: "rz"` still writes a native file;
- the app's own `initialize` names its server `doctored`;
- R2's side files: a snapshot saved as `.drsnapshot` and copied to
  `legacy.rzsnapshot` loads, and `save_snapshot` to `x.png` is refused;
  `package_action` writes a `.draction` whose `action.json` holds
  `doctored_action` and no `rasterize_action`, and a copy renamed
  `legacy.rzaction` whose key is rewritten to `rasterize_action` imports and
  lists; `paste` takes a `payload_path` ending `.rzcb` or `.drcb` past its
  extension check and refuses `x.png` naming `.drcb`.

Prints PASS/FAIL per check and exits non-zero on any FAIL.
"""

import json
import os
import shutil
import sys
import tempfile
import uuid
from pathlib import Path

from phase5_support import App, fixture
from verify_phase2 import rendered

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "core/tests/fixtures/rz_gradient_103"
NEW_TYPE = "com.kgalligan.doctored-document"
LEGACY_TYPE = "com.kgalligan.rasterize-document"

failures = []


def check(ok, name, detail=""):
    print(("PASS " if ok else "FAIL ") + name + ("" if ok else f": {detail}"), flush=True)
    if not ok:
        failures.append(name)


def tool(app, tool_name, /, **arguments):
    """(is_error, text) of one tools/call, whichever way it went."""
    result = app.client.rpc("tools/call", {"name": tool_name, "arguments": arguments})
    text = next((c["text"] for c in result.get("content", []) if c["type"] == "text"), "")
    return bool(result.get("isError")), text


def parsed(text):
    try:
        value = json.loads(text)
    except (TypeError, ValueError):
        return {}
    return value if isinstance(value, dict) else {}


def copy_package(source, target):
    if source.is_dir():
        shutil.copytree(source, target)
    else:
        shutil.copy2(source, target)


def side_files(app, scratch):
    """R2: the legacy side-file extensions and the legacy action key are still read."""
    image = scratch / "side.png"
    fixture(image, 32, 24)
    document = app.open(image)

    # Snapshots.
    snapshot = app.call("create_snapshot", document_id=document, name="R2")["snapshot_id"]
    saved = scratch / "s.drsnapshot"
    failed, text = tool(app, "save_snapshot", document_id=document, snapshot_id=snapshot,
                        path=str(saved))
    check(not failed and saved.exists(), "save_snapshot writes a .drsnapshot", text)
    legacy = scratch / "legacy.rzsnapshot"
    if saved.exists():
        copy_package(saved, legacy)
    failed, text = tool(app, "load_snapshot", document_id=document, path=str(legacy))
    check(not failed and "snapshot_id" in parsed(text), "load_snapshot reads a legacy .rzsnapshot",
          text)
    failed, text = tool(app, "save_snapshot", document_id=document, snapshot_id=snapshot,
                        path=str(scratch / "x.png"))
    check(failed and ".drsnapshot" in text, "save_snapshot to x.png is refused", text)

    # Actions.
    steps = [{"tool": "select_rect", "arguments": {"x": 2, "y": 2, "width": 8, "height": 6}}]
    reply = app.call("save_action", name="r2-side", steps=steps, overwrite=True)
    check(bool(reply.get("ok")), "save_action saves the one-step action", reply)
    bundles = scratch / "bundles"
    bundles.mkdir()
    packaged = app.call("package_action", name="r2-side", destination=str(bundles))
    bundle = Path(packaged.get("path", ""))
    check(bundle.name.endswith(".draction"), "package_action writes a .draction bundle", packaged)
    manifest = json.loads((bundle / "action.json").read_text())
    check("doctored_action" in manifest and "rasterize_action" not in manifest,
          "the packaged action.json names doctored_action only", sorted(manifest))
    old_bundle = scratch / "legacy.rzaction"
    shutil.copytree(bundle, old_bundle)
    manifest["rasterize_action"] = manifest.pop("doctored_action")
    (old_bundle / "action.json").write_text(json.dumps(manifest, indent=2, sort_keys=True))
    failed, text = tool(app, "import_action", path=str(old_bundle), name="r2-legacy")
    check(not failed, "import_action installs a legacy .rzaction with rasterize_action", text)
    failed, text = tool(app, "list_actions", name="r2-legacy")
    check(not failed and parsed(text).get("action", {}).get("name") == "r2-legacy",
          "list_actions finds the imported legacy action", text)

    # Clipboard payloads.
    for name in ("absent.rzcb", "absent.drcb"):
        failed, text = tool(app, "paste", document_id=document, payload_path=str(scratch / name))
        check("ending in" not in text, f"paste payload_path {name} passes the extension check",
              text)
    failed, text = tool(app, "paste", document_id=document, payload_path=str(scratch / "x.png"))
    check(failed and "ending in .drcb" in text, "paste payload_path x.png is refused naming .drcb",
          text)


def main():
    os.environ.setdefault("DR_PASTEBOARD_NAME", f"doctored-rename-{uuid.uuid4().hex}")
    with tempfile.TemporaryDirectory(prefix="doctored-rename-") as scratch:
        scratch = Path(scratch)
        with App(scratch / "app", scratch / "recovery") as app:
            started = app.client.rpc("initialize", {
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "verify_doctored_rename", "version": "1"}})
            check(started.get("serverInfo", {}).get("name") == "doctored",
                  "initialize names the server doctored", started.get("serverInfo"))
            for name in ("byte", "typed"):
                legacy = FIXTURES / f"{name}.rz"
                check(legacy.read_bytes()[:4] == b"RZDC", f"{name}: the fixture is a legacy file")
                document = app.open(legacy)
                info = app.call("get_document", document_id=document)
                preservation = info["save_preservation"]
                save_types = preservation["save_types"]
                save_as = preservation["save_as_types"]
                check(LEGACY_TYPE not in save_types and LEGACY_TYPE not in save_as,
                      f"{name}: Save never writes the legacy type", preservation)
                check(bool(save_as) and save_as[0] == NEW_TYPE,
                      f"{name}: Save As leads with the Doctored document", save_as)
                check("doctored" in preservation["losses_by_format"]
                      and "rz" not in preservation["losses_by_format"],
                      f"{name}: losses_by_format names doctored", sorted(preservation["losses_by_format"]))
                before = rendered(app.client, document)

                target = scratch / f"{name}.doctored"
                reply = app.call("save_copy", document_id=document, path=str(target))
                check(reply.get("format") == "doctored", f"{name}: save_copy answers doctored", reply)
                check(target.read_bytes()[:4] == b"DRDC", f"{name}: the copy starts DRDC",
                      target.read_bytes()[:4])
                reopened = app.open(target)
                check(rendered(app.client, reopened) == before,
                      f"{name}: the .doctored renders the .rz's pixels")

                alias = scratch / f"{name}-alias.rz"
                reply = app.call("save_copy", document_id=document, path=str(alias), format="rz")
                check(reply.get("format") == "doctored" and alias.read_bytes()[:4] == b"DRDC",
                      f"{name}: format rz still writes a native file", reply)
            side_files(app, scratch)
    if failures:
        print(f"FAIL {len(failures)} check(s): {', '.join(failures)}")
        return 1
    print("PASS all checks")
    return 0


if __name__ == "__main__":
    sys.exit(main())
