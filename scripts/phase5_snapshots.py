"""Named full-state snapshots and independently inspectable package persistence."""

import json
from pathlib import Path
import re
import shutil

from phase5_support import App, digest, editing_state, fixture


def archive_version():
    """The package version the app declares (`EditingStateArchive.version`)."""
    source = Path(__file__).resolve().parents[1] / "app/Sources/EditingStateArchive.swift"
    match = re.search(r"static let version = (\d+)", source.read_text())
    if match is None:
        raise RuntimeError(f"No `static let version = N` in {source}")
    return int(match.group(1))


def new_snapshot(app, document, *, name=None, path=None):
    before = {entry["snapshot_id"] for entry in app.call("list_snapshots", document_id=document)["snapshots"]}
    if path is None:
        app.call("create_snapshot", document_id=document, name=name)
    else:
        app.call("load_snapshot", document_id=document, path=str(path))
    entries = app.call("list_snapshots", document_id=document)["snapshots"]
    added = [entry for entry in entries if entry["snapshot_id"] not in before]
    assert len(added) == 1, (before, entries)
    return added[0]["snapshot_id"]


def snapshot_flows(folder):
    folder.mkdir()
    source = folder / "snapshots.png"
    fixture(source)
    package = folder / "remembered.drsnapshot"
    native = folder / "ordinary.rz"
    with App(folder / "app", folder / "recovery", interval=60) as app:
        document = app.open(source)
        call = lambda tool_name, **arguments: app.call(tool_name, document_id=document, **arguments)
        initial = editing_state(app, document)
        baseline = new_snapshot(app, document, name="Clean Baseline")
        call("select_rect", x=7, y=8, width=55, height=43)
        call("modify_selection", operation="feather", radius=3)
        selected = editing_state(app, document)
        selection_only = new_snapshot(app, document, name="Selection Only")
        assert not call("get_document")["unsaved_changes"]
        call("deselect")
        call("apply_filter", layer=0, filter="invert")
        changed = editing_state(app, document)
        before_restore = call("list_history")["undo_count"]
        call("restore_snapshot", snapshot_id=selection_only)
        assert editing_state(app, document) == selected
        assert not call("get_document")["unsaved_changes"]
        assert call("list_history")["undo_count"] == before_restore + 1
        call("undo")
        assert editing_state(app, document) == changed
        call("redo")
        assert editing_state(app, document) == selected
        before_noop = call("list_history")["current_state_id"]
        call("restore_snapshot", snapshot_id=selection_only)
        assert call("list_history")["current_state_id"] == before_noop
        call("rename_snapshot", snapshot_id=selection_only, name="Remembered Selection")
        call("save_snapshot", snapshot_id=selection_only, path=str(package))
        assert not call("get_document")["unsaved_changes"]
        call("save_copy", path=str(native), format="rz")
        call("restore_snapshot", snapshot_id=baseline)
        assert editing_state(app, document) == initial
        call("delete_snapshot", snapshot_id=baseline)
        call("restore_snapshot", snapshot_id=baseline, fail=True)
        for invalid in ("", " \n", "Control\x01Name", "x" * 257):
            call("create_snapshot", name=invalid, fail=True)
        print("PASS: full-state snapshot restore, clean identity, one undo/redo, no-op and naming/ID refusals", flush=True)

    assert package.is_dir()
    manifest = json.loads((package / "state.json").read_text())
    assert manifest["version"] == archive_version(), manifest["version"]
    assert manifest["width"] == 128 and manifest["height"] == 96
    kind = manifest["selection"]["kind"]
    assert kind in ("mask", "mask16"), kind
    assert (package / "selection.mask").stat().st_size == 128 * 96 * (2 if kind == "mask16" else 1)
    assert json.loads((package / "snapshot.json").read_text())["name"] == "Remembered Selection"
    stored = {str(path.relative_to(package)): digest(path) for path in package.iterdir() if path.is_file()}
    with App(folder / "reopened", folder / "recovery-second", interval=60) as app:
        document = app.open(native)
        assert app.call("list_snapshots", document_id=document)["snapshots"] == []
        assert "selection" not in app.call("get_document", document_id=document)
        loaded = new_snapshot(app, document, path=package)
        assert not app.call("get_document", document_id=document)["unsaved_changes"]
        app.call("restore_snapshot", document_id=document, snapshot_id=loaded)
        assert editing_state(app, document) == selected
        # A loaded package is a distinct native image; restore is an edit even
        # when its bytes happen to equal the current saved file.
        assert app.call("get_document", document_id=document)["unsaved_changes"]
        assert {str(path.relative_to(package)): digest(path) for path in package.iterdir() if path.is_file()} == stored
        print("PASS: portable snapshot package preserves mask and editing state across process restart; ordinary native policy is explicit", flush=True)

        before = app.call("list_snapshots", document_id=document)
        for name, mutate in [
            ("future", lambda target: rewrite_json(target / "state.json", version=999)),
            ("bad-dimensions", lambda target: rewrite_json(target / "state.json", width=129)),
            ("truncated-mask", lambda target: (target / "selection.mask").write_bytes(b"\0")),
            ("path-traversal", lambda target: rewrite_selection_path(target / "state.json", "../outside.mask")),
        ]:
            broken = folder / f"{name}.drsnapshot"
            shutil.copytree(package, broken)
            mutate(broken)
            app.call("load_snapshot", document_id=document, path=str(broken), fail=True)
            after = app.call("list_snapshots", document_id=document)
            assert after["snapshots"] == before["snapshots"], (name, before, after)
        print("PASS: unsupported versions, dimension mismatch, truncated masks and unsafe archive paths refuse without partial insertion", flush=True)

        for index in range(1, 32):
            new_snapshot(app, document, name=f"Shared State {index}")
        bounded = app.call("list_snapshots", document_id=document)
        assert len(bounded["snapshots"]) == bounded["maximum_count"] == 32, bounded
        app.call("create_snapshot", document_id=document, name="Past Limit", fail=True)
        assert len(app.call("list_snapshots", document_id=document)["snapshots"]) == 32
        assert bounded["incremental_bytes"] <= bounded["budget_bytes"], bounded
        print("PASS: snapshot count/memory limits refuse growth while retaining the existing shared states", flush=True)


def rewrite_json(path, **changes):
    value = json.loads(path.read_text())
    value.update(changes)
    path.write_text(json.dumps(value))


def rewrite_selection_path(path, value):
    manifest = json.loads(path.read_text())
    manifest["selection"]["maskFile"] = value
    path.write_text(json.dumps(manifest))
