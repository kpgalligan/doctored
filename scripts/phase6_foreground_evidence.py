#!/usr/bin/env python3
"""Read-only diagnostics for the frozen coupled foreground qualification.

A foreground color must admit some B in[0,1] satisfying C=aF+(1-a)B,
within half a source byte of quantization. Projecting F into that feasible box
uses source/estimated alpha/estimated F only, never reference alpha. This script
measures the projection's effect; it is not an editor correction or quality
qualification. Reference opacity is consulted only afterward to count damage.
"""
import argparse
from collections import Counter
import hashlib
import json
import math
from pathlib import Path
import sys

sys.dont_write_bytecode = True
from verify_phase2 import read_png


def foreground_byte_bounds(source, alpha):
    """Exact integer F range consistent with a quantized encoded-RGB source."""
    if alpha == 0:
        return 0, 255
    lower = max(0, math.ceil(255 * (source - 0.5 - (255 - alpha)) / alpha))
    upper = min(255, math.floor(255 * (source + 0.5) / alpha))
    assert lower <= source <= upper, "F=B=C must always be feasible"
    return lower, upper


def alpha_bin(alpha):
    if alpha == 255:
        return "255"
    if alpha >= 247:
        return "247..254"
    if alpha >= 224:
        return "224..246"
    if alpha >= 128:
        return "128..223"
    return "0..127"


def case_record(folder, row):
    name = row["case"]
    names = ["source", "estimated-foreground", "sampled-alpha", "truth-alpha"]
    paths = [folder / f"{name}-{kind}.png" for kind in names]
    if not all(path.is_file() for path in paths):
        return {"case": name, "measured": False, "reason": "no saved foreground artifacts for this case"}
    decoded = [read_png(path.read_bytes()) for path in paths]
    dimensions = decoded[0][:2]
    assert all(image[:2] == dimensions for image in decoded)
    opaque_bins, changed_bins, projected_bins = Counter(), Counter(), Counter()
    changed_pixels = projected_changes = violations = 0
    maximum = projected_maximum = 0
    for source, estimated, alpha, truth in zip(*(image[2] for image in decoded)):
        a = alpha[0]
        projected = []
        for c in range(3):
            lower, upper = foreground_byte_bounds(source[c], a)
            violations += not lower <= estimated[c] <= upper
            projected.append(min(upper, max(lower, estimated[c])))
        projected_changes += any(projected[c] != estimated[c] for c in range(3))
        if truth[0] != 255:
            continue
        key = alpha_bin(a)
        opaque_bins[key] += 1
        delta = max(abs(source[c] - estimated[c]) for c in range(3))
        projected_delta = max(abs(source[c] - projected[c]) for c in range(3))
        if delta:
            changed_bins[key] += 1
            changed_pixels += 1
        if projected_delta:
            projected_bins[key] += 1
        maximum = max(maximum, delta)
        projected_maximum = max(projected_maximum, projected_delta)
    # The independent byte decode must agree with the unchanged scorer.
    assert changed_pixels == row["sampled_alpha_color"]["after"]["opaque_changed_pixels"]
    return {"case": name, "measured": True, "dimensions": dimensions,
            "reference_opaque_by_estimated_alpha": dict(opaque_bins),
            "changed_opaque_by_estimated_alpha": dict(changed_bins),
            "after_feasibility_projection_changed_opaque_by_estimated_alpha": dict(projected_bins),
            "changed_opaque_pixels": changed_pixels,
            "after_feasibility_projection_changed_opaque_pixels": sum(projected_bins.values()),
            "maximum_opaque_rgb_byte_change": maximum,
            "after_feasibility_projection_maximum_opaque_rgb_byte_change": projected_maximum,
            "foreground_channels_without_feasible_background": violations,
            "feasibility_projection_changed_pixels": projected_changes}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path, help="Qualification directory containing matting/metrics.json")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--include-photographs", action="store_true",
                        help="Also inspect the4saved1280x1280 photo cases; uses approximately600MiB Python storage")
    arguments = parser.parse_args()
    folder = arguments.directory / "matting"
    metric_bytes = (folder / "metrics.json").read_bytes()
    metrics = json.loads(metric_bytes)
    groups = ["analytic"] + (["photographic_recompositions"] if arguments.include_photographs else [])
    output = {"schema_version": 1, "metrics_sha256": hashlib.sha256(metric_bytes).hexdigest(),
              "experiment": "encoded-RGB byte-gamut feasibility projection; no fitted thresholds and no kernel changes",
              "projection_inputs": ["observed RGB", "estimated alpha", "estimated foreground RGB"],
              "reference_use": "only post-projection damage measurement; never projection input",
              "quality_gate_changed": False, "cases": {}}
    for group in groups:
        output["cases"][group] = [case_record(folder, row) for row in metrics[group]]
    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    arguments.output.write_text(json.dumps(output, indent=2, sort_keys=True) + "\n")
    for group, rows in output["cases"].items():
        measured = [row for row in rows if row["measured"]]
        print(json.dumps({"group": group, "measured": len(measured),
                          "opaque_changed_before": sum(row["changed_opaque_pixels"] for row in measured),
                          "opaque_changed_after_projection": sum(row["after_feasibility_projection_changed_opaque_pixels"] for row in measured),
                          "cases_with_remaining_opaque_damage": sum(row["after_feasibility_projection_changed_opaque_pixels"] > 0 for row in measured)}))


if __name__ == "__main__":
    main()
