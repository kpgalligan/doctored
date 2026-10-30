"""Actual forced-termination and recovery lifecycle checks over private MCP."""

import json
import shutil
import stat
import time

from phase3_fixtures import synthetic_dng
from phase5_snapshots import new_snapshot
from phase5_support import App, decorate, digest, editing_state, fixture, wait_for, write_report


def status(app, document):
    return app.call("get_recovery_status", document_id=document)["document"]


def available(app):
    return app.call("get_recovery_status")["available"]


def protected(app, document, *, automatic=False):
    if not automatic:
        # Queueing is valid when another document is checkpointing; poll the
        # durable serial rather than treating a request acknowledgement as saved.
        app.call("write_recovery", document_id=document, wait=False)
    return wait_for(lambda: status(app, document),
                    lambda value: value.get("state") == "protected" and not value["pending"],
                    timeout=12, detail="durable current recovery generation")


def recover(app, recovery_id):
    reply = app.call("recover_document", recovery_id=recovery_id, show=True)
    document = reply["document"]["id"]
    state = app.call("get_document", document_id=document)
    assert state["path"] is None and state["unsaved_changes"], state
    return document, reply


def default_interval_checkpoint(folder):
    """Qualify the real 30-second default, with no interval override at all."""
    folder.mkdir()
    source = folder / "interval.png"
    native = folder / "interval.rz"
    fixture(source)
    with App(folder / "app", folder / "recovery", interval=None) as app:
        seed = app.open(source)
        app.call("save_copy", document_id=seed, path=str(native), format="rz")
        document = app.open(native)
        initial = status(app, document)
        assert initial["interval_seconds"] == 30 and not initial["needed"], initial
        started = time.monotonic()
        app.call("set_layer_properties", document_id=document, index=0, name="Default Interval Edit")
        time.sleep(1.1)
        early = status(app, document)
        assert early["pending"] and not early.get("generation"), early
        checkpoint = wait_for(lambda: status(app, document),
            lambda value: value.get("state") == "protected" and not value["pending"],
            timeout=34, detail="the default 30-second native recovery checkpoint")
        elapsed = time.monotonic() - started
        assert elapsed <= 35, (elapsed, checkpoint)
        assert app.call("get_document", document_id=document)["unsaved_changes"]
        durable = folder / "recovery" / checkpoint["recovery_id"] / "manifest.json"
        assert durable.is_file()
        manifest = json.loads(durable.read_text())
        assert manifest["current"]["serial"] == checkpoint["captured_serial"]
        assert manifest["current"]["revision"] == checkpoint["captured_revision"]
        assert (durable.parent / manifest["current"]["folder"] / "image.rz").is_file()
        write_report(folder / "default-interval-metrics.json", {
            "interval_seconds": 30, "elapsed_seconds": elapsed, "early_status": early,
            "durable_status": checkpoint,
        })
        print(f"PASS: default 30-second scheduling persisted a dirty native checkpoint in {elapsed:.3f}s without an interval override", flush=True)


def crash_and_restart(folder):
    folder.mkdir()
    recovery = folder / "recovery"
    imported = folder / "source.png"
    native = folder / "master.rz"
    jpeg = folder / "original.jpg"
    raw = folder / "original.dng"
    fixture(imported)
    synthetic_dng(raw)
    expected, checkpoint, original_hashes = {}, {}, {}
    with App(folder / "first", recovery) as app:
        seed = app.open(imported)
        app.call("save_copy", document_id=seed, path=str(native), format="rz")
        app.call("save_copy", document_id=seed, path=str(jpeg), format="jpg")
        for label, source in [("native", native), ("jpeg", jpeg), ("raw", raw)]:
            original_hashes[label] = digest(source)
            document = app.open(source)
            decorate(app, document)
            new_snapshot(app, document, name=f"Recovered {label} snapshot")
            expected[label] = editing_state(app, document)
            # The native document earns its first checkpoint automatically,
            # without the explicit write command supplying the durability.
            started = time.monotonic()
            checkpoint[label] = protected(app, document, automatic=label == "native")
            assert digest(source) == original_hashes[label]
            assert app.call("get_document", document_id=document)["unsaved_changes"]
            if label == "native":
                assert time.monotonic() - started <= 12
        assert available(app) == [], "An open document's leased recovery must not be offered for recovery"
        app.crash()
    assert digest(native) == original_hashes["native"]
    assert digest(jpeg) == original_hashes["jpeg"]
    assert digest(raw) == original_hashes["raw"]
    moved = folder / "moved-original.jpg"
    jpeg.rename(moved)
    with App(folder / "restart", recovery) as app:
        candidates = available(app)
        by_id = {entry["recovery_id"]: entry for entry in candidates}
        assert set(by_id) == {entry["recovery_id"] for entry in checkpoint.values()}, candidates
        assert by_id[checkpoint["jpeg"]["recovery_id"]]["source_exists"] is False
        native_recovered = None
        for label in ("native", "jpeg", "raw"):
            document, reply = recover(app, checkpoint[label]["recovery_id"])
            assert editing_state(app, document) == expected[label], (label, reply)
            snapshots = app.call("list_snapshots", document_id=document)["snapshots"]
            assert [item["name"] for item in snapshots] == [f"Recovered {label} snapshot"], snapshots
            if label == "native":
                native_recovered = document
        assert digest(native) == original_hashes["native"]
        assert digest(moved) == original_hashes["jpeg"]
        assert digest(raw) == original_hashes["raw"]
        assert available(app) == []
        assert native_recovered is not None
        app.call("set_layer_properties", document_id=native_recovered, index=0, name="Untitled Second Session")
        expected_untitled = editing_state(app, native_recovered)
        second = protected(app, native_recovered)
        app.crash()
    with App(folder / "second-restart", recovery) as app:
        document, _ = recover(app, second["recovery_id"])
        assert editing_state(app, document) == expected_untitled
        assert app.call("get_document", document_id=document)["path"] is None
        app.call("discard_recovery", recovery_id=second["recovery_id"], fail=True)
        # A simultaneous app cannot offer, claim or discard the open record.
        with App(folder / "concurrent", recovery) as other:
            assert second["recovery_id"] not in {entry["recovery_id"] for entry in available(other)}
            other.call("recover_document", recovery_id=second["recovery_id"], fail=True)
            other.call("discard_recovery", recovery_id=second["recovery_id"], fail=True)
    write_report(folder / "checkpoint-metrics.json", checkpoint)
    print("PASS: automatic native checkpoint, forced-kill JPEG/RAW/native recovery, moved originals preserved, full states/snapshots and untitled second crash", flush=True)
    print("PASS: open recovery leases exclude simultaneous recovery/discard across app instances", flush=True)


def failed_write_and_previous_generation(folder):
    folder.mkdir()
    recovery = folder / "recovery"
    source = folder / "fault.png"
    fixture(source)
    with App(folder / "first", recovery, interval=60) as app:
        document = app.open(source)
        decorate(app, document)
        new_snapshot(app, document, name="First Checkpoint Snapshot")
        older_state = editing_state(app, document)
        first = protected(app, document)
        app.call("set_layer_properties", document_id=document, index=0, name="Newer Checkpoint")
        new_snapshot(app, document, name="Second Checkpoint Snapshot")
        newer_state = editing_state(app, document)
        second = protected(app, document)
        assert second["generation"] != first["generation"]
        directory = recovery / second["recovery_id"]
        manifest_before = (directory / "manifest.json").read_bytes()
        app.call("set_layer_properties", document_id=document, index=0, name="Failed Write Must Stay Dirty")
        mode = stat.S_IMODE(directory.stat().st_mode)
        try:
            directory.chmod(0o500)
            app.call("write_recovery", document_id=document, wait=True, fail=True)
            failure = status(app, document)
            assert failure["state"] == "failed" and failure.get("error"), failure
            assert (directory / "manifest.json").read_bytes() == manifest_before
            assert app.call("get_document", document_id=document)["unsaved_changes"]
        finally:
            directory.chmod(mode)
        app.crash()

    # Keep a copy of the last two valid generations to test both failed-write
    # recovery and fallback after later corruption independently.
    backup = folder / "copy-of-last-good-store"
    shutil.copytree(recovery, backup)
    missing_snapshots = folder / "missing-snapshot-store"
    shutil.copytree(recovery, missing_snapshots)
    zero_mask = folder / "zero-mask-store"
    shutil.copytree(recovery, zero_mask)
    offcanvas = folder / "offcanvas-selection-store"
    shutil.copytree(recovery, offcanvas)
    with App(folder / "failed-write-restart", recovery, interval=60) as app:
        document, _ = recover(app, second["recovery_id"])
        assert editing_state(app, document) == newer_state
        assert len(app.call("list_snapshots", document_id=document)["snapshots"]) == 2
    corrupt_directory = backup / second["recovery_id"] / second["generation"]
    (corrupt_directory / "state.json").write_text('{"version":999}')
    with App(folder / "corrupt-restart", backup, interval=60) as app:
        document, reply = recover(app, second["recovery_id"])
        assert editing_state(app, document) == older_state, reply
        assert "previous" in str(reply).lower(), reply
        assert len(app.call("list_snapshots", document_id=document)["snapshots"]) == 1
    shutil.rmtree(missing_snapshots / second["recovery_id"] / second["generation"] / "snapshots")
    with App(folder / "missing-snapshot-restart", missing_snapshots, interval=60) as app:
        document, reply = recover(app, second["recovery_id"])
        assert editing_state(app, document) == older_state, reply
        assert "previous" in str(reply).lower(), reply
        assert len(app.call("list_snapshots", document_id=document)["snapshots"]) == 1
    # Syntactically valid but unmaterializable selections must fail while the
    # current-to-previous fallback is still possible, not after it returns.
    for label, root in [("zero-mask", zero_mask), ("offcanvas", offcanvas)]:
        state_file = root / second["recovery_id"] / second["generation"] / "state.json"
        manifest = json.loads(state_file.read_text())
        if label == "zero-mask":
            manifest["selection"] = {"kind": "mask", "coordinates": [], "maskFile": "selection.mask"}
            (state_file.parent / "selection.mask").write_bytes(bytes(manifest["width"] * manifest["height"]))
        else:
            manifest["selection"] = {"kind": "rect", "coordinates": [99999, 99999, 10, 10]}
        state_file.write_text(json.dumps(manifest))
        with App(folder / f"{label}-restart", root, interval=60) as app:
            document, reply = recover(app, second["recovery_id"])
            assert editing_state(app, document) == older_state, reply
            assert "previous" in str(reply).lower(), reply
    print("PASS: write failure preserves last durable generation and dirty state; corrupt current archive falls back explicitly to previous checkpoint", flush=True)

    # Invalid manifests remain visible as errors and explicitly discardable.
    broken_root = folder / "broken-store"
    broken_id = "d9f6f8b6-24eb-40f8-a042-8414b33bf06f"
    broken = broken_root / broken_id
    broken.mkdir(parents=True)
    (broken / "manifest.json").write_text("{malformed")
    with App(folder / "discard", broken_root, interval=60) as app:
        candidates = available(app)
        assert len(candidates) == 1 and not candidates[0]["recoverable"] and candidates[0].get("error"), candidates
        app.call("recover_document", recovery_id=broken_id, fail=True)
        app.call("discard_recovery", recovery_id=broken_id)
        assert available(app) == [] and not broken.exists()
        app.call("discard_recovery", recovery_id="../outside", fail=True)
    with App(folder / "discard-restart", broken_root, interval=60) as app:
        assert available(app) == []
    print("PASS: damaged manifest reports an error, explicit discard persists across restart, unsafe recovery IDs refuse", flush=True)
