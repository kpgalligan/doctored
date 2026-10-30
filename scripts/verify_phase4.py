#!/usr/bin/env python3
"""Qualify Phase 4 workers and live resource accounting against a built app.

DR_VERIFY_APP selects an isolated executable. The Phase 1 harness provides a
private MCP socket/Actions directory, dummy credentials and owned-process cleanup.
No user's documents, key material or running application processes are touched.
Run without --skip-large to include the fixed 24/48 MP resource corpus, which
runs at the 8 GiB working limit of the 16 GB minimum Mac; --large-only
runs that corpus alone, without the timed jobs instance.
"""

import argparse
import json
from pathlib import Path
import shutil
import struct
import tempfile
import time
import zlib

from verify_phase1 import instance, png
from verify_phase2 import rendered


CANCEL_BOUND_SECONDS = 2.0
READ_BOUND_SECONDS = 1.0
# The 16 GB minimum Mac's automatic working limit; the large corpus runs at it.
MINIMUM_MAC_LIMIT = 8_589_934_592
TERMINAL = {"completed", "cancelled", "stale", "unchanged", "failed"}


def wait_job(client, job_id, timeout=30):
    deadline = time.monotonic() + timeout
    while True:
        status = client.call("job_status", job_id=job_id)
        if status["state"] in TERMINAL:
            assert status["scratch_estimate_bytes"] == 0, status
            return status
        assert time.monotonic() < deadline, status
        time.sleep(0.02)


def retouch_fixture(path, width, height):
    # A bounded deterministic texture with a conspicuous defect. Successful
    # synthesis must change the defect, so "unchanged" cannot pass the test.
    pixels = bytearray()
    for y in range(height):
        for x in range(width):
            if width // 3 < x < width // 2 and height // 3 < y < height // 2:
                color = (250, 8, 22, 255)
            else:
                color = (60 + (x * 3 + y * 7) % 150,
                         40 + (x * 11 + y * 5) % 170,
                         30 + (x * 7 + y * 13) % 180, 255)
            pixels.extend(color)
    png(path, width, height, pixels)


def prepare_large_fill(client, document, layer=0):
    client.call("select_rect", document_id=document, x=400, y=200, width=640, height=640)
    start = time.monotonic()
    result = client.call("content_aware_fill", document_id=document, layer=layer,
                         ring=96, seed=17, background=True)
    assert result["state"] == "running", result
    assert time.monotonic() - start < READ_BOUND_SECONDS, "Starting a worker blocked MCP"
    return result


def cancellation_and_stale(client, folder):
    source = folder / "large-fill.png"
    retouch_fixture(source, 1536, 1024)
    document = client.open(source)
    initial = rendered(client, document, layer=0)
    initial_resources = client.call("resource_usage")
    job = prepare_large_fill(client, document)
    readings = []
    for _ in range(3):
        start = time.monotonic()
        client.call("get_document", document_id=document)
        elapsed = time.monotonic() - start
        assert elapsed < READ_BOUND_SECONDS, elapsed
        readings.append(elapsed)
        state = client.call("job_status", job_id=job["job_id"])
        assert state["state"] == "running", state
        time.sleep(0.02)
    resources = client.call("resource_usage")
    assert resources["transient_estimate_bytes"] > 0, resources
    assert resources["active_job"]["job_id"] == job["job_id"], resources
    client.call("content_aware_fill", document_id=document, layer=0,
                background=True, fail=True)
    start = time.monotonic()
    client.call("cancel_job", job_id=job["job_id"])
    cancelled = wait_job(client, job["job_id"], timeout=CANCEL_BOUND_SECONDS)
    cancellation_seconds = time.monotonic() - start
    assert cancelled["state"] == "cancelled", cancelled
    assert cancellation_seconds <= CANCEL_BOUND_SECONDS, cancellation_seconds
    after = client.call("resource_usage")
    assert after["transient_estimate_bytes"] == 0 and "active_job" not in after, after
    for category in ["layer_pixels", "image_pixels"]:
        assert after["categories"].get(category) == initial_resources["categories"].get(category), (category, initial_resources, after)
    assert rendered(client, document, layer=0) == initial
    assert not client.call("get_document", document_id=document)["unsaved_changes"]

    # A real pixel edit while the immutable worker input is running survives the
    # worker's completion. This catches overwrites that a rename-only test misses.
    stale = prepare_large_fill(client, document)
    client.call("apply_filter", document_id=document, layer=0, filter="invert")
    edited = rendered(client, document, layer=0)
    assert edited != initial
    result = wait_job(client, stale["job_id"])
    assert result["state"] == "stale", result
    assert rendered(client, document, layer=0) == edited
    client.call("undo", document_id=document)
    assert rendered(client, document, layer=0) == initial

    # Reordering the captured layer independently invalidates positional target
    # identity; duplicate pixel equality must not allow the old result to land.
    client.call("duplicate_layer", document_id=document, index=0)
    client.call("set_layer_properties", document_id=document, index=1, name="Captured target")
    stale = prepare_large_fill(client, document, layer=1)
    client.call("reorder_layer", document_id=document, **{"from": 1, "to": 0})
    reordered = client.call("get_document", document_id=document)
    result = wait_job(client, stale["job_id"])
    assert result["state"] == "stale", result
    final = client.call("get_document", document_id=document)
    assert [layer["name"] for layer in final["layers"]] == [layer["name"] for layer in reordered["layers"]]
    assert rendered(client, document, layer=0) == initial
    metrics = {"cancel_seconds": cancellation_seconds,
               "responsive_reads_seconds": readings, "cancelled": cancelled,
               "post_cancel_resources": after}
    (folder / "job-metrics.json").write_text(json.dumps(metrics, indent=2) + "\n")
    print(f"PASS: responsive fill, cancellation {cancellation_seconds:.3f}s <= 2s, released scratch, pixel-edit/reorder stale refusal", flush=True)


def success_history_and_actions(client, folder):
    source = folder / "small-fill.png"
    retouch_fixture(source, 256, 192)
    replay_path = folder / "small-replay.png"
    shutil.copyfile(source, replay_path)
    document = client.open(source)
    initial = rendered(client, document, layer=0)
    client.call("start_recording", document_id=document)
    client.call("select_rect", document_id=document, x=70, y=50, width=70, height=55)
    selected = client.call("get_document", document_id=document)["selection"]
    job = client.call("content_aware_fill", document_id=document, layer=0,
                      ring=32, seed=23, background=True)
    status = wait_job(client, job["job_id"])
    assert status["state"] == "completed", status
    filled = rendered(client, document, layer=0)
    assert filled != initial
    client.call("stop_recording", document_id=document, name="Phase Four Fill")
    client.call("undo", document_id=document)
    info = client.call("get_document", document_id=document)
    assert not info["unsaved_changes"] and info["selection"] == selected, info
    assert rendered(client, document, layer=0) == initial
    client.call("redo", document_id=document)
    assert rendered(client, document, layer=0) == filled
    native = folder / "filled.rz"
    client.call("save_copy", document_id=document, path=str(native), format="rz")
    reopened = client.open(native)
    assert rendered(client, reopened, layer=0) == filled
    replay = client.open(replay_path)
    client.call("run_action", document_id=replay, name="Phase Four Fill")
    assert rendered(client, replay, layer=0) == filled
    client.call("undo", document_id=replay)
    assert rendered(client, replay, layer=0) == initial
    assert not client.call("get_document", document_id=replay)["unsaved_changes"]
    print("PASS: fill commits one undo step, redo/native reopen and recorded Action match", flush=True)


def corpus_png(path, width, height):
    """Same formula as core/tests/common/phase4_corpus.rs, streamed in rows.

    The pattern repeats every 256 columns/rows, avoiding a 192MB Python object
    while preserving every byte of the fixed Rust qualification fixture.
    """
    def chunk(kind, payload):
        return (struct.pack(">I", len(payload)) + kind + payload
                + struct.pack(">I", zlib.crc32(kind + payload)))

    rows = []
    for y in range(256):
        tile = bytes(value for x in range(256)
                     for value in ((x * 17 + y * 3) % 256,
                                   (x * 5 + y * 11) % 256, (x ^ y) % 256, 255))
        rows.append(b"\0" + (tile * ((width + 255) // 256))[:width * 4])
    compressor = zlib.compressobj(level=3)
    with path.open("wb") as output:
        output.write(b"\x89PNG\r\n\x1a\n")
        output.write(chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)))
        for y in range(height):
            packed = compressor.compress(rows[y % 256])
            if packed:
                output.write(chunk(b"IDAT", packed))
        output.write(chunk(b"IDAT", compressor.flush()))
        output.write(chunk(b"IEND", b""))


def retained_bytes(report):
    return report["categories"]["layer_pixels"]["unique_bytes"]


def corpus_resources(folder, width, height):
    path = folder / "corpus.png"
    corpus_png(path, width, height)
    pixel_bytes = width * height * 4
    with instance(folder, working_limit=MINIMUM_MAC_LIMIT) as client:
        document = client.open(path)
        baseline = client.call("resource_usage")
        assert retained_bytes(baseline) == pixel_bytes, baseline
        for index in range(1, 5):
            client.call("set_layer_properties", document_id=document, index=0, name=f"History {index}")
        client.call("duplicate_layer", document_id=document, index=0)
        shared = client.call("resource_usage")
        assert retained_bytes(shared) == pixel_bytes, shared
        assert shared["document_handle_count"] > baseline["document_handle_count"], (baseline, shared)
        assert shared["shared_bytes_saved"] >= pixel_bytes * 4, shared
        client.call("apply_filter", document_id=document, layer=1, filter="invert")
        edited = client.call("resource_usage")
        assert retained_bytes(edited) == pixel_bytes * 2, edited
        client.call("undo", document_id=document)
        undone = client.call("resource_usage", evict_caches=True)
        # Redo owns the edited allocation after Undo; weak registry ownership
        # must include it until redo/history actually releases it.
        assert retained_bytes(undone) == pixel_bytes * 2, undone
        assert undone["known_unaccounted"] and undone["host_tracking"], undone
        observations = dict(baseline=baseline, shared=shared, edited=edited, undone=undone)
        (folder / "resource-metrics.json").write_text(json.dumps(observations, indent=2) + "\n")
    print(f"PASS: {width * height // 1_000_000}MP live undo/projection registry, Arc-shared snapshots, unique edit and redo retention", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--keep", action="store_true")
    parser.add_argument("--skip-large", action="store_true", help="Skip separately qualified 24/48MP app corpus")
    parser.add_argument("--large-only", action="store_true", help="Run only the 24/48MP app corpus, at 8 GiB")
    args = parser.parse_args()
    if args.large_only and args.skip_large:
        parser.error("--large-only and --skip-large are mutually exclusive")
    folder = Path(tempfile.mkdtemp(prefix="rasterize-phase4-"))
    print(f"Verification scratch: {folder}", flush=True)
    try:
        if not args.large_only:
            jobs = folder / "jobs"
            jobs.mkdir()
            with instance(jobs) as client:
                cancellation_and_stale(client, jobs)
                success_history_and_actions(client, jobs)
        if not args.skip_large:
            for width, height in [(6000, 4000), (8000, 6000)]:
                corpus = folder / f"{width * height // 1_000_000}mp"
                corpus.mkdir()
                corpus_resources(corpus, width, height)
    except BaseException:
        print(f"Failure artifacts retained: {folder}", flush=True)
        raise
    if not args.keep:
        shutil.rmtree(folder)


if __name__ == "__main__":
    main()
