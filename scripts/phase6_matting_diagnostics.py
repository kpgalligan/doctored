#!/usr/bin/env python3
"""Explain frozen qualification failures without rerunning candidate kernels.

Optional PNG byte-delta inspection covers exported artifacts up to2M pixels.
Missing or larger artifacts are explicitly unmeasured, never counted as zero.
"""
import argparse
from collections import Counter
import json
from pathlib import Path
import struct
import sys

sys.dont_write_bytecode = True
from verify_phase2 import read_png


def failures(record, gate):
    if "refusal" in record:
        return ["foreground_refusal"]
    before, after = record["before"], record["after"]
    result = []
    if after["opaque_changed_pixels"] > gate["opaque_changed_pixels_max"]:
        result.append("reference_opaque_rgb_changed")
    if after["alpha_weighted_foreground_mad"] > (
        before["alpha_weighted_foreground_mad"] + gate["weighted_mad_regression_allowance"]
    ):
        result.append("alpha_weighted_foreground_regression")
    if after["fractional_foreground_mad"] > gate["fractional_foreground_mad_max"]:
        result.append("fractional_foreground_mad_above_limit")
    if (
        before["fractional_foreground_mad"] > gate["visible_fringe_initial_mad"]
        and after["fractional_foreground_mad"] > (
            before["fractional_foreground_mad"] * (1 - gate["visible_fringe_min_improvement"])
        )
    ):
        result.append("insufficient_visible_fringe_improvement")
    if any(
        after["recomposite"][name]["boundary_mad"] > gate["recomposite_boundary_mad_max"]
        for name in ["white", "dark", "checker"]
    ):
        result.append("replacement_boundary_mad_above_limit")
    if not record.get("has_fractional_reference", False):
        result.append("no_fractional_reference")
    if record.get("alpha_candidate_refused", False):
        result.append("alpha_candidate_refused")
    return result


def opaque_delta(root, case):
    names = [f"{case}-source.png", f"{case}-estimated-foreground.png", f"{case}-truth-alpha.png"]
    if not all((root / name).exists() for name in names):
        return {"measured": False, "reason": "this case has no exported foreground artifact"}
    with (root / names[0]).open("rb") as source:
        header = source.read(24)
    width, height = struct.unpack(">II", header[16:24])
    if width * height > 2_000_000:
        return {"measured": False, "reason": "byte-delta diagnostic decode exceeds2M pixels; no rescale"}
    images = [read_png((root / name).read_bytes()) for name in names]
    assert all(image[:2] == images[0][:2] for image in images)
    source, estimate, alpha = [image[2] for image in images]
    maximum, changed, count = 0, 0, 0
    for observed, predicted, coverage in zip(source, estimate, alpha):
        if coverage[0] != 255:
            continue
        count += 1
        delta = max(abs(observed[c] - predicted[c]) for c in range(3))
        maximum = max(maximum, delta)
        changed += delta != 0
    return {
        "measured": True,
        "reference_opaque_pixels": count,
        "changed_pixels": changed,
        "maximum_absolute_rgb_byte_delta": maximum if count else None,
    }


def summarize(directory, *, opaque_artifacts=False, gate_report=None):
    directory = Path(directory)
    path = next(
        (directory / name for name in ["matting/metrics.json", "images/metrics.json", "metrics.json"]
         if (directory / name).exists()),
        None,
    )
    if path is None:
        raise ValueError("No qualification metrics found")
    report = json.loads(path.read_text())
    gate = report.get("color_gate")
    if gate is None and gate_report is not None:
        gate = json.loads(Path(gate_report).read_text())["color_gate"]
    if gate is None:
        raise ValueError("Older report lacks serialized gate; supply its frozen --gate-report explicitly")
    result = {
        "schema_version": 1,
        "source_metrics": str(path.relative_to(directory)),
        "gate": gate,
        "gate_report": str(gate_report) if gate_report is not None else None,
        "interpretation": (
            "Counts overlap. Reference-opaque preservation is stricter than preserving RGB "
            "where the supplied estimated alpha is255. Oracle-alpha isolates the color estimator; "
            "estimated-alpha results also include alpha errors."
        ),
        "artifact_scope": "Only exported artifacts up to2M pixels; other byte deltas explicitly unmeasured.",
    }
    groups = [key for key in ["analytic", "photographic_recompositions", "full_images"] if key in report]
    for key in groups:
        rows = report[key]
        cases = []
        coupled = "estimated_alpha_color" if rows and "estimated_alpha_color" in rows[0] else "sampled_alpha_color"
        counts = {name: Counter() for name in ["oracle_alpha_color", coupled]}
        for row in rows:
            case = {"case": row["case"]}
            for pipeline in counts:
                reasons = failures(row[pipeline], gate)
                assert bool(reasons) != row[pipeline]["passes_color_gate"], (row["case"], pipeline, reasons)
                case[pipeline] = reasons
                counts[pipeline].update(reasons)
            if opaque_artifacts:
                delta = opaque_delta(path.parent, row["case"])
                case["opaque_artifact_delta"] = delta
                if delta["measured"]:
                    assert delta["changed_pixels"] == row[coupled]["after"]["opaque_changed_pixels"]
            cases.append(case)
        result[key] = {
            "cases": cases,
            "failure_counts": {name: dict(counter) for name, counter in counts.items()},
        }
    (directory / "failure-reasons.json").write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--opaque-artifacts", action="store_true")
    parser.add_argument("--gate-report", type=Path)
    args = parser.parse_args()
    result = summarize(args.directory, opaque_artifacts=args.opaque_artifacts, gate_report=args.gate_report)
    print(json.dumps({name: value["failure_counts"] for name, value in result.items()
                      if isinstance(value, dict) and "failure_counts" in value}, indent=2))


if __name__ == "__main__":
    main()
