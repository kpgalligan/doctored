#!/usr/bin/env python3
"""Verify pinned references and run fixed alpha/color qualification binaries.

Build with: cd core && cargo build --release --example phase6_selection_quality
--example phase6_matting_quality. This wrapper intentionally does not build or
alter candidates; hashes identify the measured binaries. Exit0 means checks ran,
not that experimental algorithms passed their quality gates. --require-pass makes
the distinct release gate fail unless every numerical case passes end-to-end.
"""
import argparse
import json
from pathlib import Path
import subprocess
import sys
import time

sys.dont_write_bytecode = True
import phase6_reference_fixtures as fixtures
from phase6_matting_diagnostics import summarize as failure_summary
import phase9i_gate_v2_rescore as rescore


def run(binary, arguments, log):
    with log.open("x") as output:
        subprocess.run([str(binary), *map(str, arguments)], stdout=output,
                       stderr=subprocess.STDOUT, check=True)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("references",type=Path)
    parser.add_argument("output",type=Path)
    parser.add_argument("--bin-directory",type=Path,default=fixtures.ROOT/"core/target/release/examples")
    parser.add_argument("--require-pass",action="store_true")
    parser.add_argument("--gate",choices=["v1","v2"],default="v2",
                        help="pass rule applied to the analytic and photographic corpora, default %(default)s")
    args=parser.parse_args()
    references=fixtures.acquire(args.references,verify_only=True)
    output=args.output.resolve()
    if output==fixtures.ROOT or fixtures.ROOT in output.parents:
        raise ValueError("Photographic artifacts must stay outside the repository")
    output.mkdir(parents=True,exist_ok=False)
    names=["phase6_selection_quality","phase6_matting_quality"]
    binaries={name:{"path":str((args.bin_directory/name).resolve()),"sha256":fixtures.sha256(args.bin_directory/name)} for name in names}
    started=time.time()
    run(Path(binaries[names[0]]["path"]),[output/"guided-baseline"],output/"guided.log")
    run(Path(binaries[names[1]]["path"]),[output/"matting",args.references.resolve()],output/"matting.log")
    baseline=json.loads((output/"guided-baseline/metrics.json").read_text())
    measured=json.loads((output/"matting/metrics.json").read_text())
    # This protects the original gate's known result during infrastructure changes.
    if (baseline["analytic_cases"],baseline["analytic_passes"])!=(24,12):
        raise ValueError("Original guided24-case result changed; investigate before interpreting new measurements")
    # Gate v1 demanded 24/24 on both corpora. Gate v2 keeps the same measurements
    # and rescopes the pass rule; the Rust evaluator still records only v1.
    scored=rescore.score_run(output)
    # The original gate's SHAPE, not only its result: two corpora of 24 cases
    # each. A rescore that silently dropped a corpus or a case would otherwise
    # qualify on whatever it still found.
    if set(scored["summaries"])!={"analytic","crops"}:
        raise ValueError(f"the rescore found corpora {sorted(scored['summaries'])}, not analytic and crops")
    for corpus,row in scored["summaries"].items():
        if row["cases"]!=24:
            raise ValueError(f"corpus {corpus} scored {row['cases']} cases, not the gate's 24")
    passed=rescore.qualified(scored["summaries"],args.gate)
    # schema_version stays 1 (no reader exists to migrate): from this run on
    # "gate_summaries" carries rescore.gate_report's shape.
    report={"schema_version":1,"seconds":time.time()-started,"binaries":binaries,
            "archive_sha256":references["archive_sha256"],"baseline":{"cases":24,"passes":12},
            "analytic":measured["analytic_summary"],"photographic":measured["photo_summary"],
            "gate":args.gate,"gate_summaries":rescore.gate_report(scored["summaries"]),
            "v2_indeterminate":{corpus:row["v2_indeterminate"] for corpus,row in scored["summaries"].items()},
            "numerical_release_gate_passes":passed,
            "limits":"Numerical reference crops do not replace full-frame photographic inspection or provide fur/glass/product photographic truth."}
    (output/"run.json").write_text(json.dumps(report,indent=2,sort_keys=True)+"\n")
    failure_summary(output)
    print(json.dumps(report,indent=2))
    if args.require_pass and not passed: raise SystemExit(1)


if __name__=="__main__": main()
