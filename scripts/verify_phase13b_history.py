#!/usr/bin/env python3
"""13B-5: what one edit adds to a document's history, read from the history report.

Default (the checks): an owned Debug instance opens a 1500x1000 PNG at 8 bits and again
converted to 16 bits, and asserts on `list_history.retained_bytes` (the union of every
history state's handles, so its change is what one edit retains):

- a 16-bit `move_layers` retains under 65,536 bytes (it moves an offset; no pixels change);
- `add_layer_mask reveal_all` retains under one plane (a uniform mask is a filled plane);
- a white `brush_stroke target=mask` over a reveal-all mask answers `changed: false` and adds
  no history state.

`--measure 8|16` runs the 24 MP history phase instead (6000x4000, the corpus of
`core/tests/common/phase4_corpus.rs`, at an 8 GiB working limit) and prints one JSON row per
edit: the change in `retained_bytes` and in each resource category, then the named snapshots'
figures after a capture and after one whole-layer edit. Byte counts only: nothing is timed.

Usage: verify_phase13b_history.py [--measure 8|16] [--out <folder>]
"""
import argparse
import json
import os
import sys
import tempfile
from pathlib import Path

os.environ.setdefault("DR_BACKGROUND", "1")
sys.path.insert(0, str(Path(__file__).resolve().parent))

from verify_phase1 import instance  # noqa: E402
from verify_phase4 import corpus_png  # noqa: E402

PLANE_W, PLANE_H = 1500, 1000


def session(folder, **arguments):
    """An owned instance (`verify_phase1.instance`) logging into its own new folder."""
    folder.mkdir(parents=True, exist_ok=True)
    return instance(folder, **arguments)


def categories(report):
    return {k: v["unique_bytes"] for k, v in report["categories"].items()}


class Tracker:
    """Reads `list_history` and `resource_usage` after each edit; prints the deltas."""

    def __init__(self, client, document, verbose):
        self.client, self.document, self.verbose = client, document, verbose
        self.prev = None

    def call(self, tool, **arguments):
        return self.client.call(tool, document_id=self.document, **arguments)

    def snap(self, label):
        history = self.call("list_history")
        report = self.client.call("resource_usage")
        row = {
            "label": label,
            "states": len(history["states"]),
            "retained_bytes": history["retained_bytes"],
            "incremental_history_bytes": history["incremental_history_bytes"],
            "budget_bytes": history["budget_bytes"],
            "evicted_states": history["evicted_states"],
            "categories": categories(report),
        }
        if self.prev is not None:
            row["d_retained"] = row["retained_bytes"] - self.prev["retained_bytes"]
            row["d_states"] = row["states"] - self.prev["states"]
            row["d_categories"] = {
                k: row["categories"].get(k, 0) - self.prev["categories"].get(k, 0)
                for k in sorted(set(row["categories"]) | set(self.prev["categories"]))
                if row["categories"].get(k, 0) != self.prev["categories"].get(k, 0)
            }
        if self.verbose:
            print(json.dumps({k: v for k, v in row.items() if k != "categories"}), flush=True)
        self.prev = row
        return row

    def step(self, label, tool, **arguments):
        result = self.call(tool, **arguments)
        return result, self.snap(label)


def checks(folder):
    png = folder / "plane.png"
    corpus_png(png, PLANE_W, PLANE_H)
    plane = PLANE_W * PLANE_H
    failures = []

    def expect(ok, text):
        print(("PASS " if ok else "FAIL ") + text, flush=True)
        if not ok:
            failures.append(text)

    for depth in (8, 16):
        with session(folder / f"checks{depth}") as client:
            tracker = Tracker(client, client.open(png), verbose=False)
            if depth == 16:
                tracker.call("convert_precision", mode="rgba16", structure="preserve")
            tracker.snap("open")
            _, moved = tracker.step("move", "move_layers", layers=[0], dx=10, dy=10)
            if depth == 16:
                expect(moved["d_retained"] < 65_536,
                       f"16-bit move_layers retains {moved['d_retained']} bytes (< 65,536)")
            _, masked = tracker.step("mask", "add_layer_mask", layer=0, kind="reveal_all")
            bound = plane * depth // 8
            expect(masked["d_retained"] < bound // 3,
                   f"{depth}-bit add_layer_mask reveal_all retains {masked['d_retained']} "
                   f"bytes (< {bound // 3}, a third of the {bound}-byte plane)")
            result, stroked = tracker.step("white on white", "brush_stroke", layer=0,
                                           target="mask", points=[[700, 500]], size=100)
            expect(result.get("changed") is False and stroked["d_states"] == 0,
                   f"{depth}-bit white mask stroke over reveal-all: changed "
                   f"{result.get('changed')!r}, {stroked['d_states']} new history states (0)")
    if failures:
        print(f"verify_phase13b_history: FAIL ({len(failures)})")
        return 1
    print("verify_phase13b_history: PASS")
    return 0


def measure(folder, depth):
    """The 24 MP history phase (the spike's drive, counts only)."""
    corpus = folder / "corpus.png"
    if not corpus.exists():
        corpus_png(corpus, 6000, 4000)
    with session(folder / f"measure{depth}", working_limit=8_589_934_592) as client:
        client.timeout = 900
        tracker = Tracker(client, client.open(corpus), verbose=True)
        if depth != 8:
            tracker.call("convert_precision", mode=f"rgba{depth}", structure="preserve")
        tracker.snap("open")
        step = tracker.step
        step("brush dot size 100", "brush_stroke", layer=0, points=[[1150, 1150]], size=100,
             color="#ff0000")
        step("brush line 1000 px size 40", "brush_stroke", layer=0,
             points=[[2000, 2000], [3000, 2000]], size=40, color="#0000ff")
        step("add_layer_mask reveal_all", "add_layer_mask", layer=0, kind="reveal_all")
        step("mask stroke that changes nothing", "brush_stroke", layer=0, target="mask",
             points=[[1150, 2150]], size=100)
        step("mask dot size 100", "eraser_stroke", layer=0, target="mask",
             points=[[1150, 2150]], size=100)
        step("add_adjustment_layer levels", "add_adjustment_layer", op="levels",
             params={"black": 0.04, "white": 0.94, "gamma": 1.1})
        step("edit_adjustment_layer levels", "edit_adjustment_layer", layer=1,
             params={"black": 0.08, "white": 0.9, "gamma": 1.2})
        step("move_layers by (10, 10)", "move_layers", layers=[0], dx=10, dy=10)
        step("invert whole layer", "apply_filter", layer=0, filter="invert")
        keys = ("retained_bytes", "incremental_bytes", "budget_bytes", "maximum_count")
        tracker.call("create_snapshot", name="A")
        captured = tracker.call("list_snapshots")
        print(json.dumps({"snapshot": "after capture A",
                          **{k: captured.get(k) for k in keys}}), flush=True)
        tracker.call("apply_filter", layer=0, filter="invert")
        tracker.call("create_snapshot", name="B")
        edited = tracker.call("list_snapshots")
        print(json.dumps({"snapshot": "after one whole-layer invert and capture B",
                          **{k: edited.get(k) for k in keys}}), flush=True)
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--measure", type=int, choices=(8, 16))
    parser.add_argument("--out", type=Path)
    arguments = parser.parse_args()
    folder = arguments.out or Path(tempfile.mkdtemp(prefix="phase13b-history-"))
    folder.mkdir(parents=True, exist_ok=True)
    if arguments.measure:
        return measure(folder, arguments.measure)
    return checks(folder)


if __name__ == "__main__":
    sys.exit(main())
