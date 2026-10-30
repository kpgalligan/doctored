#!/usr/bin/env python3
"""Freeze candidate/source hashes, verify pinned references, retain every result.

This runner never constructs inference inputs from reference planes. The shared
Rust evaluator owns the unchanged fixture generation and independent scoring.
"""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import subprocess
import sys

sys.dont_write_bytecode = True
import phase9i_gate_v2_rescore as rescore

ROOT = Path(__file__).resolve().parents[1]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("output", type=Path)
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--crops", type=Path, required=True)
    parser.add_argument("--full", type=Path, required=True)
    parser.add_argument("--tiled", action="store_true")
    parser.add_argument("--protect", action="store_true")
    parser.add_argument("--stable-prior", action="store_true")
    parser.add_argument("--bayesian", action="store_true")
    parser.add_argument("--gate", choices=["v1", "v2"], default="v2",
                        help="pass rule applied to the retained results, default %(default)s; v1 stays reproducible")
    args = parser.parse_args()
    if sum([args.tiled, args.protect, args.stable_prior, args.bayesian]) > 1:
        parser.error("select only one candidate variant")
    output = args.output.resolve()
    if output.is_relative_to(ROOT) or args.output.is_symlink():
        parser.error("output must be new, outside the repository, and not a symlink")
    output.mkdir(exist_ok=False)
    version = "v4" if args.stable_prior else "v3" if args.protect else "v2" if args.tiled else "v1"
    declaration = ROOT / ("designs/phase-9i-bayesian-v5.json" if args.bayesian else f"designs/phase-9i-soft-prior-{version}.json")
    sources = [ROOT / "core/src/closed_form_matting.rs", ROOT / "core/src/matting_laplacian.rs",
               ROOT / "core/src/matting_input.rs", ROOT / "core/src/tiled_soft_matting.rs", ROOT / "core/src/foreground_protection.rs", ROOT / "core/examples/phase9i_soft_prior_quality.rs",
               ROOT / "core/examples/phase6_matting_quality.rs", ROOT / "core/examples/phase6_full_image_quality.rs",
               ROOT / "core/tests/common/selection_quality.rs", ROOT / "core/tests/common/matting_quality.rs",
               ROOT / "scripts/phase9i_gate_v2_rescore.py"]
    if args.bayesian:
        sources += [ROOT / "core/src/bayesian_matting.rs", ROOT / "core/src/bayesian_color.rs",
                    ROOT / "core/src/matting_samples.rs", ROOT / "core/src/doc_select.rs",
                    ROOT / "core/examples/phase9i_bayesian_quality.rs"]
    predeclared = {"frozen_before_scoring_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                   "candidate": json.loads(declaration.read_text()), "declaration_sha256": sha(declaration),
                   "executable": str(args.executable.resolve()), "executable_sha256": sha(args.executable),
                   "source_sha256": {str(p.relative_to(ROOT)): sha(p) for p in sources},
                   "reference_manifest_sha256": {str(p): sha(p) for p in [args.crops / "references.json", args.full / "full-references.json"]}}
    (output / "predeclared.json").write_text(json.dumps(predeclared, indent=2) + "\n")
    candidate = [str(args.executable.resolve())] + (["--stable-prior"] if args.stable_prior else ["--protected"] if args.protect else ["--tiled"] if args.tiled else [])
    commands = [
        [sys.executable, "-B", str(ROOT / "scripts/phase6_reference_fixtures.py"), str(args.crops), "--verify-only"],
        [sys.executable, "-B", str(ROOT / "scripts/phase6_full_reference_fixtures.py"), str(args.full), "--verify-only"],
        candidate + ["analytic", str(output / "analytic-crops"), str(args.crops)],
        candidate + ["full", str(output / "full"), str(args.full)],
    ]
    stages = []
    for number, command in enumerate(commands):
        with (output / f"stage-{number}.log").open("xb") as log:
            result = subprocess.run(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=False)
        stages.append({"command": command, "returncode": result.returncode})
        if result.returncode:
            (output / "run.json").write_text(json.dumps({"predeclared": predeclared, "stages": stages, "runner_failed": True}, indent=2) + "\n")
            return result.returncode
    crop_report = json.loads((output / "analytic-crops/metrics.json").read_text())
    full_report = json.loads((output / "full/metrics.json").read_text())
    summaries = {"analytic": crop_report["analytic_summary"], "crops": crop_report["photo_summary"], "full": full_report["full_image_summary"]}
    # The Rust evaluator keeps writing the v1 passes_end_to_end_gate; the pass
    # decision is the re-scorer's, so gate v2 and gate v1 read the same results.
    scored = rescore.score_run(output)
    (output / "gate-rescore.json").write_text(json.dumps(scored, indent=2, sort_keys=True) + "\n")
    gate_summaries = rescore.gate_report(scored["summaries"])
    indeterminate = {corpus: row["v2_indeterminate"] for corpus, row in scored["summaries"].items()}
    qualified = rescore.qualified(scored["summaries"], args.gate)
    result = {"predeclared": predeclared, "stages": stages, "summaries": summaries, "gate": args.gate,
              "gate_summaries": gate_summaries, "v2_indeterminate": indeterminate,
              "numerical_gate_passed": qualified,
              "physical_translucency_reference_available": False, "advanced_controls_enabled": False}
    (output / "run.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"summaries": summaries, "gate": args.gate, "gate_summaries": gate_summaries,
                      "numerical_gate_passed": qualified, "v2_indeterminate": indeterminate}, indent=2), flush=True)
    return 0 if qualified else 1


if __name__ == "__main__":
    sys.exit(main())
