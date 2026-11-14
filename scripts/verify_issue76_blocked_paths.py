#!/usr/bin/env python3
"""MCP drive: every tool argument that names a file refuses a
sensitive location (`app/Sources/AgentPathGuard.swift`).

For every `AgentPathGuard.toolPaths` entry (parsed from the Swift source, so a
new entry without a case here fails the drive) it makes one call with a blocked
path, which must answer in-band with the `Refused:` sentence naming the
location, and one with an allowed temporary path, which must succeed. Only
`add_live_photo_layer`, `get_raw_local_mask`, `apply_raw_settings`, the two
`assisted_*` tools, `merge_panorama` and `sky_replacement` may instead answer an
error that is not the guard's (a PNG is not a movie, the document has no RAW
layer, the assisted experiment is off, two identical frames do not register,
no sky model is installed). Allowed inputs are the app's own outputs: a
`save_copy` PNG, `export_psd` -> `inspect_psd`, `export_pdf` ->
`inspect_vector`, a copied sRGB ICC, a hand-written `.cube`, `.acv` and `.hdt`
(`verify_v1_hdr_toning.hdt`), `package_action` -> `import_action`,
`export_style_presets` -> `import_style_presets`, `export_brush_presets` ->
`import_brush_presets` and `export_patterns` -> `import_patterns` (of a preset
and a pattern saved first), `get_color_table` -> `set_color_table` and
`get_duotone` -> `set_duotone`.

It also checks the export-preset folder and profile read at use (a preset JSON
rewritten in the instance's own export-presets folder to a blocked destination)
for quick_export, export_layers, contact_sheet and run_batch; package_action
and run_action of a saved step naming `~/.ssh/<uuid>`; a symlink, `..` and case
through open_document; a path into the instance's own export-presets folder;
and the three read carve-outs (a `$asset` replay, a recorded `.drcb` paste and
a ColorSync profile).

Safety: a blocked write names a fresh `<uuid>` folder that does not exist, the
tools that create folders (run_batch output, assisted_*, package_action) are
pointed only at /etc or /System, and after the run the drive asserts that no
blocked path it named exists.

Usage: python3 scripts/verify_issue76_blocked_paths.py [--keep]
(against the Debug app unless DR_VERIFY_APP says otherwise).
"""

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import struct
import sys
import tempfile
import time
import uuid

from verify_v1_hdr_toning import hdt

ROOT = Path(__file__).resolve().parents[1]
GUARD = ROOT / "app/Sources/AgentPathGuard.swift"
SRGB = Path("/System/Library/ColorSync/Profiles/sRGB Profile.icc")
# A proof profile needs an A2B/B2A table, which the sRGB matrix profile lacks.
CMYK = Path("/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc")
HOME = Path.home()
# `merge_panorama`'s two identical flat PNGs give Auto-Align no registration; the instance's
# private models folder holds no sky model, so `sky_replacement` answers "not installed".
TOLERATED = {"add_live_photo_layer", "get_raw_local_mask", "assisted_workspace_status",
             "assisted_open_case", "merge_panorama", "sky_replacement", "apply_raw_settings"}


def declared_entries():
    """(tool, "key.path") for every `toolPaths` declaration in the Swift source."""
    text = GUARD.read_text()
    block = text[text.index("static let toolPaths"):text.index("static let nonFilePathArguments")]
    entries = set()
    for tools, keys, _ in re.findall(r'add\(\[(.*?)\],\s*\[(.*?)\],\s*\.(read|write)', block, re.S):
        key = ".".join(re.findall(r'"([^"]+)"', keys))
        for tool in re.findall(r'"([^"]+)"', tools):
            entries.add((tool, key))
    return entries


class Drive:
    def __init__(self, app, folder):
        self.app = app
        self.folder = folder
        self.allowed = folder / "allowed"
        self.allowed.mkdir()
        self.unique = uuid.uuid4().hex
        self.named = []
        self.failures = []
        self.blocked_cases = set()
        self.allowed_cases = set()
        self.report = []

    # -- calls --

    def raw(self, tool, **arguments):
        result = self.app.client.rpc("tools/call", {"name": tool, "arguments": arguments})
        text = next((c["text"] for c in result.get("content", []) if c["type"] == "text"), "")
        return bool(result.get("isError")), text

    def record(self, ok, name, detail):
        print(("PASS " if ok else "FAIL ") + name + ("" if ok else f": {detail}"), flush=True)
        self.report.append({"case": name, "ok": ok, "detail": detail})
        if not ok:
            self.failures.append(name)

    def refused(self, tool, key, label, **arguments):
        error, text = self.raw(tool, **arguments)
        ok = error and text.startswith("Refused: ") and label in text
        self.record(ok, f"blocked {tool} {key}", text[:300])
        if key:
            self.blocked_cases.add((tool, key))
        return text

    def succeeded(self, tool, key, **arguments):
        error, text = self.raw(tool, **arguments)
        if error:
            ok = tool in TOLERATED and not text.startswith("Refused:")
            self.record(ok, f"allowed {tool} {key}", "non-guard error: " + text[:300])
            reply = None
        else:
            self.record(True, f"allowed {tool} {key}", text[:200])
            try:
                reply = json.loads(text)
            except json.JSONDecodeError:
                reply = text
        if key:
            self.allowed_cases.add((tool, key))
        return reply

    def call(self, tool, **arguments):
        error, text = self.raw(tool, **arguments)
        assert not error, (tool, arguments, text)
        try:
            return json.loads(text)
        except json.JSONDecodeError:
            return text

    # -- blocked locations: each names a fresh folder that does not exist --

    def home(self, root, leaf):
        path = f"~/{root}/{self.unique}/{leaf}"
        self.named.append(HOME / root / self.unique)
        return path

    def system(self, root, leaf=""):
        path = f"{root}/{self.unique}" + (f"/{leaf}" if leaf else "")
        self.named.append(Path(root) / self.unique)
        return path

    def ssh(self, leaf):
        return self.home(".ssh", leaf), "SSH keys"

    def aws(self, leaf):
        return self.home(".aws", leaf), "AWS credentials"

    def gnupg(self, leaf):
        return self.home(".gnupg", leaf), "GnuPG keys"

    def keychains(self, leaf):
        return self.home("Library/Keychains", leaf), "the user's keychains"

    def support(self, leaf):
        return (self.home("Library/Application Support/Doctored", leaf),
                "Doctored's Application Support folder")

    def cookies(self, leaf):
        return self.home("Library/Cookies", leaf), "the user's cookies"

    def etc(self, leaf=""):
        return self.system("/etc", leaf), "the system configuration folder"

    def system_folder(self, leaf=""):
        return self.system("/System", leaf), "the macOS system folder"


def write_cube(path):
    lines = ['TITLE "identity"', "LUT_3D_SIZE 2", "DOMAIN_MIN 0.0 0.0 0.0",
             "DOMAIN_MAX 1.0 1.0 1.0"]
    for b in range(2):
        for g in range(2):
            for r in range(2):
                lines.append(f"{r:.6f} {g:.6f} {b:.6f}")
    path.write_text("\n".join(lines) + "\n")


def write_acv(path):
    points = [(0, 0), (140, 128), (255, 255)]
    body = struct.pack(">HHH", 4, 1, len(points))
    for output, input_ in points:
        body += struct.pack(">HH", output, input_)
    path.write_bytes(body)


def wait_batch(drive, batch_id):
    deadline = time.monotonic() + 60
    while True:
        status = drive.call("batch_status", batch_id=batch_id)
        if not status["running"]:
            return status
        assert time.monotonic() < deadline, status
        time.sleep(0.2)


def run(drive):
    d = drive
    a = d.allowed
    icc = a / "srgb.icc"
    shutil.copyfile(SRGB, icc)
    proof = a / "cmyk.icc"
    shutil.copyfile(CMYK, proof)
    cube = a / "identity.cube"
    write_cube(cube)
    acv = a / "lift.acv"
    write_acv(acv)
    file_settings = {"format": "png", "bit_depth": 8, "profile": "file",
                     "profile_path": str(icc)}

    def blocked_settings(path):
        return {"format": "png", "bit_depth": 8, "profile": "file", "profile_path": path}

    # new_document profile, then the working document.
    path, label = d.aws("blocked.icc")
    d.refused("new_document", "profile", label, width=64, height=48, profile=path)
    made = d.succeeded("new_document", "profile", width=64, height=48, background="white",
                       profile=str(icc))
    doc = made["document"]["id"]
    d.call("new_layer", document_id=doc)

    # save_copy, export_psd -> inspect_psd, export_pdf -> inspect_vector.
    png = a / "a.png"
    png2 = a / "b.png"
    path, label = d.ssh("out.png")
    d.refused("save_copy", "path", label, document_id=doc, path=path)
    d.succeeded("save_copy", "path", document_id=doc, path=str(png))
    d.call("save_copy", document_id=doc, path=str(png2))
    psd = a / "a.psd"
    path, label = d.support("out.psd")
    d.refused("export_psd", "path", label, document_id=doc, path=path)
    d.succeeded("export_psd", "path", document_id=doc, path=str(psd))
    path, label = d.aws("a.psd")
    d.refused("inspect_psd", "path", label, path=path)
    d.succeeded("inspect_psd", "path", path=str(psd))
    pdf = a / "a.pdf"
    path, label = d.keychains("out.pdf")
    d.refused("export_pdf", "path", label, document_id=doc, path=path)
    path, label = d.gnupg("p.icc")
    d.refused("export_pdf", "settings.profile_path", label, document_id=doc,
              path=str(a / "never.pdf"), settings={"bit_depth": 8, "profile": "file",
                                                   "profile_path": path})
    d.succeeded("export_pdf", "path", document_id=doc, path=str(pdf),
                settings={"bit_depth": 8, "profile": "file", "profile_path": str(icc)})
    d.allowed_cases.add(("export_pdf", "settings.profile_path"))
    path, label = d.etc("a.pdf")
    d.refused("inspect_vector", "path", label, path=path)
    d.succeeded("inspect_vector", "path", path=str(pdf))

    # open_document, place_image, add_live_photo_layer.
    path, label = d.ssh("a.png")
    d.refused("open_document", "path", label, path=path)
    opened = d.succeeded("open_document", "path", path=str(png))
    second = opened["document"]["id"]
    path, label = d.keychains("a.png")
    d.refused("place_image", "path", label, document_id=doc, path=path)
    d.succeeded("place_image", "path", document_id=doc, path=str(png))
    path, label = d.system_folder("a.mov")
    d.refused("add_live_photo_layer", "path", label, document_id=doc, path=path)
    d.succeeded("add_live_photo_layer", "path", document_id=doc, path=str(png))

    # The colour tools.
    path, label = d.gnupg("a.icc")
    d.refused("describe_profile", "path", label, path=path)
    d.succeeded("describe_profile", "path", path=str(icc))
    for tool in ["assign_profile", "convert_profile"]:
        path, label = d.cookies("a.icc")
        d.refused(tool, "path", label, document_id=second, profile="file", path=path)
        d.succeeded(tool, "path", document_id=second, profile="file", path=str(icc))
    for tool in ["set_proof_setup", "render_proof"]:
        path, label = d.etc("a.icc")
        d.refused(tool, "path", label, document_id=doc, profile="file", path=path)
        d.succeeded(tool, "path", document_id=doc, profile="file", path=str(proof))
    path, label = d.aws("a.icc")
    d.refused("save_document_preset", "profile", label, name="Issue 76", width=64, height=48,
              profile=path)
    d.succeeded("save_document_preset", "profile", name="Issue 76", width=64, height=48,
                profile=str(icc), replace=True)

    # Adjustment presets and LUT files.
    path, label = d.ssh("a.acv")
    d.refused("read_photoshop_adjustment_preset", "path", label, path=path)
    d.succeeded("read_photoshop_adjustment_preset", "path", path=str(acv))
    path, label = d.support("a.cube")
    d.refused("add_adjustment_layer", "params.file", label, document_id=doc,
              op="color_lookup", params={"file": path})
    added = d.succeeded("add_adjustment_layer", "params.file", document_id=doc,
                        op="color_lookup", params={"file": str(cube)})
    lut_layer = added.get("layer", added.get("index"))
    path, label = d.ssh("a.cube")
    d.refused("edit_adjustment_layer", "params.file", label, document_id=doc, layer=lut_layer,
              params={"file": path})
    d.succeeded("edit_adjustment_layer", "params.file", document_id=doc, layer=lut_layer,
                params={"file": str(cube)})
    path, label = d.etc("a.cube")
    d.refused("apply_filter", "params.file", label, document_id=second, filter="color_lookup",
              params={"file": path})
    d.succeeded("apply_filter", "params.file", document_id=second, filter="color_lookup",
                params={"file": str(cube)})
    path, label = d.gnupg("a.cube")
    d.refused("save_adjustment_preset", "params.file", label, name="Issue 76 LUT",
              op="color_lookup", params={"file": path})
    d.succeeded("save_adjustment_preset", "params.file", name="Issue 76 LUT",
                op="curves", params={"file": str(cube)}, replace=True)

    # Export settings and destinations.
    path, label = d.aws("p.icc")
    d.refused("export_preview", "settings.profile_path", label, document_id=doc,
              settings=blocked_settings(path))
    d.succeeded("export_preview", "settings.profile_path", document_id=doc,
                settings=file_settings)
    path, label = d.support("out.png")
    d.refused("export_image", "path", label, document_id=doc, path=path, settings=file_settings)
    path, label = d.keychains("p.icc")
    d.refused("export_image", "settings.profile_path", label, document_id=doc,
              path=str(a / "never.png"), settings=blocked_settings(path))
    d.succeeded("export_image", "path", document_id=doc, path=str(a / "exported.png"),
                settings=file_settings, overwrite=True)
    d.allowed_cases.add(("export_image", "settings.profile_path"))
    quick = a / "quick"
    quick.mkdir()
    path, label = d.ssh("")
    d.refused("quick_export", "destination", label, document_id=doc, settings=file_settings,
              destination=path, pattern="{name}-q")
    path, label = d.cookies("p.icc")
    d.refused("quick_export", "settings.profile_path", label, document_id=doc,
              settings=blocked_settings(path), destination=str(quick), pattern="{name}-q")
    d.succeeded("quick_export", "destination", document_id=doc, settings=file_settings,
                destination=str(quick), pattern="{name}-q")
    d.allowed_cases.add(("quick_export", "settings.profile_path"))
    path, label = d.support("")
    d.refused("save_export_preset", "destination", label, name="Issue 76",
              settings=file_settings, destination=path, pattern="{name}-p")
    path, label = d.etc("p.icc")
    d.refused("save_export_preset", "settings.profile_path", label, name="Issue 76",
              settings=blocked_settings(path), destination=str(quick), pattern="{name}-p")
    d.succeeded("save_export_preset", "destination", name="Issue 76", settings=file_settings,
                destination=str(quick), pattern="{name}-p", replace=True)
    d.allowed_cases.add(("save_export_preset", "settings.profile_path"))
    layers_out = a / "layers"
    layers_out.mkdir()
    path, label = d.keychains("")
    d.refused("export_layers", "destination", label, document_id=doc, items="all",
              settings=file_settings, destination=path, pattern="{name}-{index}")
    path, label = d.ssh("p.icc")
    d.refused("export_layers", "settings.profile_path", label, document_id=doc, items="all",
              settings=blocked_settings(path), destination=str(layers_out),
              pattern="{name}-{index}")
    d.succeeded("export_layers", "destination", document_id=doc, items="all",
                settings=file_settings, destination=str(layers_out), pattern="{name}-{index}")
    d.allowed_cases.add(("export_layers", "settings.profile_path"))

    # Contact sheets.
    sheets = a / "sheets"
    sheets.mkdir()
    for tool, extra in [("contact_sheet", {"document_id": doc, "items": "all"}),
                        ("contact_sheet_from_sources", {"files": [str(png), str(png2)]})]:
        path, label = d.gnupg("")
        d.refused(tool, "output.destination", label, **extra,
                  output={"kind": "export", "settings": file_settings, "destination": path,
                          "pattern": "{name}-" + tool})
        path, label = d.aws("p.icc")
        d.refused(tool, "output.settings.profile_path", label, **extra,
                  output={"kind": "export", "settings": blocked_settings(path),
                          "destination": str(sheets), "pattern": "{name}-" + tool})
        d.succeeded(tool, "output.destination", **extra,
                    output={"kind": "export", "settings": file_settings,
                            "destination": str(sheets), "pattern": "{name}-" + tool})
        d.allowed_cases.add((tool, "output.settings.profile_path"))
    path, label = d.ssh("a.png")
    d.refused("contact_sheet_from_sources", "files", label, files=[str(png), path])
    d.succeeded("contact_sheet_from_sources", "files", files=[str(png), str(png2)])

    # Stacks.
    for tool, extra in [("load_files_into_stack", {}),
                        ("merge_to_hdr", {"evs": [0, 2], "align": False}),
                        ("merge_panorama", {"blend": False})]:
        path, label = d.support("a.png")
        text = d.refused(tool, "paths", label, paths=[str(png), path], **extra)
        d.record("paths[1]" in text, f"blocked {tool} names the element", text[:200])
        d.succeeded(tool, "paths", paths=[str(png), str(png2)], **extra)

    # Style libraries: export_style_presets -> import_style_presets; sky_replacement's file.
    asl = a / "styles.asl"
    path, label = d.ssh("out.asl")
    d.refused("export_style_presets", "path", label, path=path)
    d.succeeded("export_style_presets", "path", path=str(asl))
    path, label = d.aws("a.asl")
    d.refused("import_style_presets", "path", label, path=path, group="Issue 76")
    d.succeeded("import_style_presets", "path", path=str(asl), group="Issue 76")

    # Brush presets and patterns: export -> import of a preset and a pattern saved first.
    d.call("save_brush_preset", name="Issue 76", hardness=40, flow=90, spacing=30, angle=10,
           roundness=80)
    brushes = a / "issue76.doctoredbrushes"
    path, label = d.ssh("out.doctoredbrushes")
    d.refused("export_brush_presets", "path", label, path=path)
    d.succeeded("export_brush_presets", "path", path=str(brushes))
    path, label = d.gnupg("a.doctoredbrushes")
    d.refused("import_brush_presets", "path", label, path=path)
    d.succeeded("import_brush_presets", "path", path=str(brushes))
    d.call("define_pattern", document_id=doc, name="Issue 76")
    patterns = a / "issue76.doctoredpatterns"
    path, label = d.aws("out.doctoredpatterns")
    d.refused("export_patterns", "path", label, path=path)
    d.succeeded("export_patterns", "path", path=str(patterns))
    path, label = d.cookies("a.doctoredpatterns")
    d.refused("import_patterns", "path", label, path=path)
    d.succeeded("import_patterns", "path", path=str(patterns))

    # HDR Toning presets: a hand-written .hdt into the user library.
    toning = hdt(a / "Issue 76.hdt")
    path, label = d.ssh("a.hdt")
    d.refused("import_hdr_toning_preset", "path", label, path=path)
    d.succeeded("import_hdr_toning_preset", "path", path=str(toning))
    path, label = d.keychains("sky.png")
    d.refused("sky_replacement", "sky_path", label, document_id=second, layer=0, sky_path=path)
    d.succeeded("sky_replacement", "sky_path", document_id=second, layer=0, sky_path=str(png))

    # Snapshots.
    before = {s["snapshot_id"] for s in d.call("list_snapshots", document_id=doc)["snapshots"]}
    d.call("create_snapshot", document_id=doc, name="Issue 76")
    snapshot = next(s["snapshot_id"] for s in d.call("list_snapshots", document_id=doc)["snapshots"]
                    if s["snapshot_id"] not in before)
    package = a / "s.drsnapshot"
    path, label = d.keychains("s.drsnapshot")
    d.refused("save_snapshot", "path", label, document_id=doc, snapshot_id=snapshot, path=path)
    d.succeeded("save_snapshot", "path", document_id=doc, snapshot_id=snapshot,
                path=str(package))
    path, label = d.etc("s.drsnapshot")
    d.refused("load_snapshot", "path", label, document_id=doc, path=path)
    d.succeeded("load_snapshot", "path", document_id=doc, path=str(package))

    # get_raw_local_mask and the assisted tools.
    path, label = d.ssh("m.drmask")
    d.refused("get_raw_local_mask", "path", label, document_id=doc, id="mask", path=path)
    d.succeeded("get_raw_local_mask", "path", document_id=doc, id="mask",
                path=str(a / "m.drmask"))
    for tool in ["assisted_workspace_status", "assisted_open_case"]:
        path, label = d.system_folder()
        d.refused(tool, "workspace", label, workspace=path)
        d.succeeded(tool, "workspace", workspace=str(a / "workspace"))

    # copy, cut, copy_merged with a private pasteboard.
    for tool in ["copy", "copy_merged", "cut"]:
        path, label = d.support("clip.tiff")
        d.call("select_all", document_id=second)
        d.refused(tool, "public_copy_path", label, document_id=second, public_copy_path=path)
        d.succeeded(tool, "public_copy_path", document_id=second,
                    public_copy_path=str(a / f"{tool}.tiff"))

    # paste: a recorded payload (carve-out 2) and a copy of it in a temp folder.
    d.call("copy", document_id=second)
    d.call("start_recording", discard=True)
    d.call("paste", document_id=doc, record_payload=True)
    stopped = d.call("stop_recording")
    payload = next(s["arguments"]["payload_path"] for s in stopped["steps"]
                   if s["tool"] == "paste")
    path, label = d.ssh("a.drcb")
    d.refused("paste", "payload_path", label, document_id=doc, payload_path=path)
    copied = a / "payload.drcb"
    shutil.copyfile(payload, copied)
    d.succeeded("paste", "payload_path", document_id=doc, payload_path=str(copied))
    d.succeeded("paste", "", document_id=doc, payload_path=payload)
    d.record(Path(payload).resolve().parent == (d.folder / "app/clipboard-payloads").resolve(),
             "carve-out: the recorded payload lives in the payloads folder", payload)

    # Actions: package -> import, then a `$asset` replay (carve-out 1).
    d.call("save_action", name="Issue 76 Place", overwrite=True,
           steps=[{"tool": "place_image", "arguments": {"path": str(png)}}])
    bundles = a / "bundles"
    bundles.mkdir()
    path, label = d.etc()
    d.refused("package_action", "destination", label, name="Issue 76 Place", destination=path)
    packaged = d.succeeded("package_action", "destination", name="Issue 76 Place",
                           destination=str(bundles), overwrite=True)
    bundle = packaged["path"]
    path, label = d.ssh("x.draction")
    d.refused("import_action", "path", label, path=path)
    d.succeeded("import_action", "path", path=bundle, name="Issue 76 Imported", overwrite=True)
    layers_before = len(d.call("get_document", document_id=second)["layers"])
    d.succeeded("run_action", "", document_id=second, name="Issue 76 Imported")
    layers_after = len(d.call("get_document", document_id=second)["layers"])
    d.record(layers_after == layers_before + 1, "carve-out: the $asset replay placed its layer",
             f"{layers_before} -> {layers_after}")

    # A saved step naming a blocked file: refused when packaged and when run.
    path, label = d.ssh("x.png")
    d.call("save_action", name="Issue 76 Blocked", overwrite=True,
           steps=[{"tool": "place_image", "arguments": {"path": path}}])
    text = d.refused("package_action", "", label, name="Issue 76 Blocked",
                     destination=str(bundles), overwrite=True)
    d.record("steps[0].path" in text, "package_action names the step", text[:200])
    error, text = d.raw("run_action", document_id=second, name="Issue 76 Blocked")
    d.record("Refused: path points into SSH keys" in text, "run_action step refused",
             text[:300])

    # Batch.
    inbox = a / "batch-in"
    inbox.mkdir()
    shutil.copyfile(png, inbox / "one.png")
    d.call("save_action", name="Issue 76 Batch", overwrite=True,
           steps=[{"tool": "new_layer", "arguments": {}}])
    path, label = d.etc()
    d.refused("run_batch", "input", label, name="Issue 76 Batch", input=path,
              output=str(a / "batch-never"))
    path, label = d.system_folder()
    d.refused("run_batch", "output", label, name="Issue 76 Batch", input=str(inbox),
              output=path)
    started = d.succeeded("run_batch", "input", name="Issue 76 Batch", input=str(inbox),
                          output=str(a / "batch-out"))
    d.allowed_cases.add(("run_batch", "output"))
    status = wait_batch(d, started["batch_id"])
    d.record(status["report"].get("written") == 1, "the allowed batch wrote its file",
             json.dumps(status["report"])[:300])

    # Export presets are re-checked at use.
    preset_out = a / "preset-out"
    preset_out.mkdir()
    d.call("save_export_preset", name="Issue 76 Blocked Preset",
           settings={"format": "png", "bit_depth": 8, "profile": "srgb"},
           destination=str(preset_out), pattern="{name}-{index}", replace=True)
    blocked_destination, label = d.etc()
    rewritten = 0
    for stored in (d.folder / "app/export-presets").glob("*.json"):
        try:
            data = json.loads(stored.read_text())
        except (OSError, json.JSONDecodeError):
            continue
        if isinstance(data, dict) and data.get("destination") == str(preset_out):
            data["destination"] = blocked_destination
            stored.write_text(json.dumps(data))
            rewritten += 1
    d.record(rewritten == 1, "the preset was rewritten to a blocked destination", rewritten)
    preset = "Issue 76 Blocked Preset"
    sentence = f"export preset \"{preset}\"'s destination points into {label}"
    for tool, arguments in [
        ("quick_export", {"document_id": doc, "preset": preset}),
        ("export_layers", {"document_id": doc, "items": "all", "preset": preset}),
        ("contact_sheet", {"document_id": doc, "items": "all",
                           "output": {"kind": "export", "preset": preset}}),
        ("run_batch", {"name": "Issue 76 Batch", "input": str(inbox), "preset": preset}),
    ]:
        error, text = d.raw(tool, **arguments)
        d.record(error and sentence in text, f"preset destination {tool}", text[:300])

    # open_document through a symlink, `..`, case and the instance's own folder.
    link = a / "to-ssh"
    link.symlink_to(HOME / ".ssh")
    d.named.append(HOME / ".ssh" / d.unique)
    d.refused("open_document", "", "SSH keys", path=str(link / d.unique / "a.png"))
    d.refused("open_document", "", "SSH keys", path=f"~/Pictures/../.ssh/{d.unique}/a.png")
    d.refused("open_document", "", "SSH keys", path=f"~/.SSH/{d.unique}/a.png")
    d.refused("open_document", "", "SSH keys",
              path=str(HOME).upper() + f"/.ssh/{d.unique}/a.png")
    own = d.folder / "app/export-presets/x.png"
    d.refused("open_document", "", "DR_EXPORT_PRESETS_DIR", path=str(own))

    # Carve-out 3: an installed ColorSync profile is read by path.
    d.succeeded("describe_profile", "", path=str(SRGB))
    d.succeeded("assign_profile", "", document_id=second, profile="file", path=str(SRGB))

    # convert_mode's CMYK profile file (#1), on a document of its own.
    cmyk_doc = d.call("new_document", width=32, height=24, background="white",
                      profile="srgb")["document"]["id"]
    path, label = d.gnupg("p.icc")
    d.refused("convert_mode", "path", label, document_id=cmyk_doc, mode="cmyk", profile="file",
              path=path)
    d.succeeded("convert_mode", "path", document_id=cmyk_doc, mode="cmyk", profile="file",
                path=str(proof))

    # The Indexed colour table's `.act` and the Duotone options' `.ado` (#1), each on a
    # document converted from its own copy of the flat PNG: get_* writes, set_* reads it back.
    shutil.copyfile(png, a / "indexed.png")
    indexed_doc = d.call("open_document", path=str(a / "indexed.png"))["document"]["id"]
    d.call("convert_mode", document_id=indexed_doc, mode="indexed", palette="exact",
           dither="none")
    act = a / "table.act"
    path, label = d.ssh("t.act")
    d.refused("get_color_table", "path", label, document_id=indexed_doc, path=path)
    d.succeeded("get_color_table", "path", document_id=indexed_doc, path=str(act))
    path, label = d.aws("t.act")
    d.refused("set_color_table", "path", label, document_id=indexed_doc, path=path)
    d.succeeded("set_color_table", "path", document_id=indexed_doc, path=str(act))
    shutil.copyfile(png, a / "duotone.png")
    duotone_doc = d.call("open_document", path=str(a / "duotone.png"))["document"]["id"]
    d.call("convert_mode", document_id=duotone_doc, mode="gray")
    line = lambda low, high: [low] + [None] * 11 + [high]
    inks = [{"name": "Warm Black", "lab": [20, 2, 4], "curve": line(0, 100)},
            {"name": "Rust", "lab": [55, 45, 55], "curve": line(10, 70)}]
    d.call("convert_mode", document_id=duotone_doc, mode="duotone", inks=inks)
    ado = a / "options.ado"
    path, label = d.gnupg("d.ado")
    d.refused("get_duotone", "path", label, document_id=duotone_doc, path=path)
    d.succeeded("get_duotone", "path", document_id=duotone_doc, path=str(ado))
    path, label = d.keychains("d.ado")
    d.refused("set_duotone", "path", label, document_id=duotone_doc, path=path)
    d.succeeded("set_duotone", "path", document_id=duotone_doc, path=str(ado))

    # apply_raw_settings' Masking sync masks: the working document has no RAW layer.
    path, label = d.ssh("m.drmask")
    d.refused("apply_raw_settings", "mask_paths", label, document_id=doc, locals=[],
              mask_paths=[path])
    d.succeeded("apply_raw_settings", "mask_paths", document_id=doc, locals=[],
                mask_paths=[str(a / "m.drmask")])

    expected = declared_entries()
    missing_blocked = sorted(expected - d.blocked_cases)
    missing_allowed = sorted(expected - d.allowed_cases)
    d.record(not missing_blocked, "every toolPaths entry has a blocked case", missing_blocked)
    d.record(not missing_allowed, "every toolPaths entry has an allowed case", missing_allowed)
    d.record(len(expected) > 40, "toolPaths parsed", len(expected))


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--keep", action="store_true")
    arguments = parser.parse_args()
    os.environ["DR_BACKGROUND"] = "1"
    os.environ.setdefault("DR_PASTEBOARD_NAME",
                          f"com.kgalligan.doctored.verify-issue76.{os.getpid()}")
    sys.path.insert(0, str(ROOT / "scripts"))
    import phase5_support

    folder = Path(tempfile.mkdtemp(prefix="rasterize-issue76-"))
    print(f"scratch: {folder}", flush=True)
    drive = None
    try:
        with phase5_support.App(folder / "app", folder / "recovery") as app:
            drive = Drive(app, folder)
            run(drive)
    finally:
        if drive is not None:
            existing = [str(path) for path in drive.named if os.path.lexists(path)]
            drive.record(not existing, "no blocked path was created", existing)
            (folder / "report.json").write_text(json.dumps(drive.report, indent=1))
    ok = drive is not None and not drive.failures
    print(f"issue76: {'PASS' if ok else 'FAIL'} "
          f"({len(drive.report) if drive else 0} checks, "
          f"{len(drive.failures) if drive else '?'} failed)", flush=True)
    if ok and not arguments.keep:
        shutil.rmtree(folder, ignore_errors=True)
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
