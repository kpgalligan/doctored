#!/usr/bin/env python3
"""Verify source pins and qualify seven full images; original gates stay frozen."""
import argparse
import json
from pathlib import Path
import subprocess
import sys
import time

sys.dont_write_bytecode = True
import phase6_full_reference_fixtures as fixtures
from phase6_reference_fixtures import ROOT, sha256
from phase6_matting_diagnostics import summarize as failure_summary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("references", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument(
        "--binary", type=Path,
        default=ROOT / "core/target/release/examples/phase6_full_image_quality",
    )
    parser.add_argument("--working-limit-mib", type=int, default=2048)
    parser.add_argument("--require-pass", action="store_true")
    args = parser.parse_args()
    if not 64 <= args.working_limit_mib <= 2048:
        parser.error("working limit must be 64..2048 MiB; this control may only lower the default")
    references = fixtures.acquire(args.references, verify_only=True)
    output = args.output.resolve()
    if output == ROOT or ROOT in output.parents:
        raise ValueError("Reference artifacts must stay outside the repository")
    output.mkdir(parents=True, exist_ok=False)
    binary = args.binary.resolve()
    binary_sha = sha256(binary)
    start = time.time()
    command = [
        str(binary), str(output / "images"), str(args.references.resolve()),
        str(args.working_limit_mib * 1024 * 1024),
    ]
    with (output / "full-images.log").open("x") as log:
        subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, check=True)
    report = json.loads((output / "images/metrics.json").read_text())
    summary = report["full_image_summary"]
    passed = (
        summary["cases"] == 6 * len(references["references"])
        and summary["end_to_end_passes"] == summary["cases"]
    )
    run = {
        "schema_version": 1,
        "binary": str(binary),
        "binary_sha256": binary_sha,
        "reference_manifest_sha256": sha256(args.references / "full-references.json"),
        "seconds": time.time() - start,
        "working_limit_mib": args.working_limit_mib,
        "color_gate": report.get("color_gate"),
        "full_image_summary": summary,
        "numerical_gate_passes": passed,
        "physical_translucency_reference_available": False,
    }
    (output / "run.json").write_text(json.dumps(run, indent=2, sort_keys=True) + "\n")
    if report.get("color_gate") is not None:
        failure_summary(output)
    print(json.dumps(run, indent=2))
    if args.require_pass and not passed:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
