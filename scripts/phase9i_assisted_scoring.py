#!/usr/bin/env python3
"""Score an exported Phase 9I assisted workspace and its replays under gate v2.

Phase 2 of the assisted protocol. Nothing here scores a pixel: the frozen Rust
evaluator does, through core/examples/phase9i_assisted_quality.rs, and the
unchanged phase9i_gate_v2_rescore.score_run reads its report exactly as it reads
an automatic run's. This runner freezes the declaration and every program hash
before the scorer starts, re-verifies that the workspace is still the one the
annotator was handed, records what each replay is, runs the three stages and
states — separately from the numbers — whether an assisted RESULT may be
claimed at all.

A `--dry-run` accepts a synthetic replay so the plumbing can be exercised end to
end. It is a pipeline exercise and never an assisted result; the verdict says so
and the exit code demands it.
"""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import subprocess
import sys

sys.dont_write_bytecode = True
import phase9i_assisted_workspace as exporter
import phase9i_gate_v2_rescore as rescore

ROOT = Path(__file__).resolve().parents[1]
DECLARATION = ROOT / "designs/phase-9i-assisted-scoring-v1.json"
PROTOCOL_DOC = ROOT / "archive/phase-9i-assisted-protocol-v1.md"
# Everything whose bytes decide what this run measured: the replay reader and
# its rasterizer, the kernel and its inputs, the evaluator chain, the shared
# scoring seam and the three programs that drive them.
PINNED_SOURCES = [
    "core/src/assisted_replay.rs",
    "core/src/trimap_strokes.rs",
    "core/src/closed_form_matting.rs",
    "core/src/matting_laplacian.rs",
    "core/src/matting_input.rs",
    "core/src/tiled_soft_matting.rs",
    "core/examples/phase9i_assisted_quality.rs",
    "core/examples/phase9i_assisted_workspace.rs",
    "core/examples/phase6_matting_quality.rs",
    "core/examples/phase6_full_image_quality.rs",
    "core/tests/common/selection_quality.rs",
    "core/tests/common/matting_quality.rs",
    "core/tests/common/assisted_scoring.rs",
    "core/tests/common/sha256.rs",
    "scripts/phase9i_gate_v2_rescore.py",
    "scripts/phase9i_assisted_scoring.py",
    "scripts/phase9i_assisted_workspace.py",
]


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def verify_workspace(root):
    """The workspace is still exactly what the exporter wrote and the mapping
    still describes it. Every case.json is recomputed from its own planes."""
    root = Path(root)
    workspace, private = root / "workspace", root / "private"
    manifest = json.loads((workspace / "manifest.json").read_text())
    mapping = json.loads((private / "mapping.json").read_text())
    if manifest.get("workspace_schema_version") != 1 or mapping.get("mapping_schema_version") != 1:
        raise ValueError("unsupported workspace or mapping schema")
    if manifest["workspace_id"] != mapping["workspace_id"]:
        raise ValueError("the mapping belongs to another workspace")
    for entry in manifest["cases"]:
        anonymous = entry["anonymous_id"]
        directory = workspace / "cases" / anonymous
        recomputed = exporter.case_entry(directory, anonymous)
        stored = json.loads((directory / "case.json").read_text())
        if stored != recomputed:
            raise ValueError(f"case.json for {anonymous} disagrees with its own planes")
        if entry != recomputed:
            raise ValueError(f"the manifest entry for {anonymous} disagrees with its own planes")
    if {entry["anonymous_id"] for entry in manifest["cases"]} != {
            case["anonymous_id"] for case in mapping["cases"]}:
        raise ValueError("the manifest and mapping describe different cases")
    return manifest, mapping


def _values(value):
    """Every string VALUE, never a key. A replay's own schema keys contain
    `alpha` (alpha_write); only what an annotator or the app WROTE could
    unblind a case."""
    if isinstance(value, dict):
        for item in value.values():
            yield from _values(item)
    elif isinstance(value, list):
        for item in value:
            yield from _values(item)
    elif isinstance(value, str):
        yield value


def _strings(value):
    """Keys and values both, as the exporter's own audit reads them."""
    if isinstance(value, dict):
        for key, item in value.items():
            yield key
            yield from _strings(item)
    elif isinstance(value, list):
        for item in value:
            yield from _strings(item)
    elif isinstance(value, str):
        yield value


_MALFORMED = object()


def audit_scored_workspace(workspace, forbidden):
    """The exporter's audit, widened by exactly one thing: one replay file per
    manifest anonymous id. An annotated workspace must still be unable to
    unblind itself."""
    workspace = Path(workspace)
    manifest = json.loads((workspace / "manifest.json").read_text())
    anonymous = [entry["anonymous_id"] for entry in manifest["cases"]]
    expected = {Path("manifest.json"), Path("TASK-CARD.md")}
    for value in anonymous:
        for name in ("source.png", "prior.png", "case.json"):
            expected.add(Path("cases") / value / name)
    permitted = {Path("replays") / f"{value}.json" for value in anonymous}
    present = {path.relative_to(workspace) for path in workspace.rglob("*") if path.is_file()}
    unexpected = (present - expected - permitted) | (expected - present)
    if unexpected:
        raise ValueError(f"unexpected workspace files: {sorted(map(str, unexpected))}")
    directories = {path.relative_to(workspace) for path in workspace.rglob("*") if path.is_dir()}
    allowed = {Path("cases"), Path("replays")} | {Path("cases") / value for value in anonymous}
    if directories != allowed:
        raise ValueError(
            f"unexpected workspace directories: {sorted(map(str, directories ^ allowed))}")
    tokens = tuple(token.lower() for token in forbidden)
    for path in sorted(workspace.rglob("*")):
        text = str(path.relative_to(workspace)).lower()
        for token in tokens:
            if token in text:
                raise ValueError(f"workspace path names {token}: {path}")
    for path in sorted(workspace.rglob("*.json")):
        text = path.read_text()
        if path.parent.name == "replays":
            # A replay is whatever came back; a malformed one is a REFUSAL the
            # scorer records, not a traceback here. It still has to be audited,
            # so its RAW text is scanned for every forbidden token instead.
            document = _MALFORMED
            try:
                document = json.loads(text)
            except json.JSONDecodeError:
                pass
            if document is _MALFORMED:
                lowered = text.lower()
                for token in tokens:
                    if token in lowered:
                        raise ValueError(f"{path} names {token}")
                continue
            strings = _values(document)
        else:
            strings = _strings(json.loads(text))
        for value in strings:
            lowered = value.lower()
            for token in tokens:
                if token in lowered:
                    raise ValueError(f"{path} names {token}")
    for entry in manifest["cases"]:
        directory = workspace / "cases" / entry["anonymous_id"]
        source = exporter.png_header((directory / "source.png").read_bytes())
        prior = exporter.png_header((directory / "prior.png").read_bytes())
        if source[2:4] != (8, 2) or prior[2:4] != (8, 0):
            raise ValueError(f"unexpected PNG storage in {directory}")
        if source[:2] != (entry["width"], entry["height"]) or prior[:2] != source[:2]:
            raise ValueError(f"case dimensions disagree in {directory}")
        for kinds in (source[4], prior[4]):
            if not set(kinds) <= {"IHDR", "IDAT", "IEND"}:
                raise ValueError(f"unexpected PNG chunks in {directory}: {sorted(set(kinds))}")


def replay_manifest(workspace, manifest):
    """What came back per case, without parsing a replay as this protocol: the
    core is the only reader that validates one. A case with no replay is None."""
    workspace = Path(workspace)
    result = {}
    for entry in manifest["cases"]:
        anonymous = entry["anonymous_id"]
        path = workspace / "replays" / f"{anonymous}.json"
        if not path.exists():
            result[anonymous] = None
            continue
        try:
            document = json.loads(path.read_text())
        except json.JSONDecodeError:
            # Recorded, never raised: the file is what came back, and a case
            # whose replay will not parse is refused with that as its reason.
            result[anonymous] = {"sha256": sha(path), "malformed": True}
            continue
        plane = document.get("label_plane") or {}
        exposure = document.get("annotator_reports_prior_exposure")
        result[anonymous] = {
            "sha256": sha(path),
            "origin": document.get("origin"),
            "scoreable": document.get("scoreable"),
            "outcome": document.get("outcome"),
            "annotator_reports_prior_exposure": exposure,
            "label_plane_sha256": plane.get("sha256"),
        }
    return result


def _refusal_reason(replay):
    """What the workspace alone can say about a refused case. The scorer's own
    reason is the record's; this names the common, decidable ones."""
    if replay is None:
        return "no replay"
    if replay.get("malformed"):
        return "malformed replay"
    if replay.get("outcome") is None:
        return "replay refused: the session is still in progress"
    if not replay.get("scoreable"):
        return "replay refused: the replay declares itself unscoreable"
    return "refused by the scorer; see matting/metrics.json"


def verdict(scored, mapping, replays, gate, dry_run, scorer_refusals=None):
    """The claim decision, kept apart from the numbers. A dry run, a subset
    override or a missing completion-subset case makes completion undecidable,
    and nothing undecidable can claim an assisted result however well it
    scores.

    `scorer_refusals` is `matting/metrics.json`'s own `refused` list
    (`[{"case", "reason"}]`, written by
    core/examples/phase9i_assisted_quality.rs). The scorer saw the replay and
    the planes, so ITS reason is the record's; `_refusal_reason` answers only
    the cases it has none for."""
    summaries = scored["summaries"]
    cases = mapping["cases"]
    named = sorted(case["case"] for case in cases)
    by_case = {case["case"]: case["anonymous_id"] for case in cases}
    subset_override = bool(mapping.get("subset_override"))
    missing = sorted(set(rescore.ASSISTED_COMPLETION_SUBSET) - set(named))
    scorer_reasons = {entry["case"]: entry["reason"] for entry in (scorer_refusals or [])}
    refused = [
        {"case": row["case"],
         "reason": scorer_reasons.get(
             row["case"], _refusal_reason(replays.get(by_case.get(row["case"]))))}
        for row in scored["cases"] if row["refused"]
    ]
    exposed = sorted(case["case"] for case in cases if case.get("annotator_exposed") or
                     (replays.get(case["anonymous_id"]) or {}).get(
                         "annotator_reports_prior_exposure"))
    completion_decidable = not dry_run and not subset_override and missing == []
    numerical_gate_passed = completion_decidable and rescore.qualified(summaries, gate)
    if dry_run:
        reason = "synthetic dry run: no assisted result is claimable from it"
    elif subset_override:
        reason = "the workspace is a declared subset override, not the completion subset"
    elif missing:
        reason = f"{len(missing)} completion-subset cases are not in this workspace"
    elif not numerical_gate_passed:
        reason = f"gate {gate} is not passed by the scored cases"
    else:
        reason = None
    return {
        "gate": gate,
        "summaries": summaries,
        "gate_summaries": rescore.gate_report(summaries),
        "v2_indeterminate": {corpus: row["v2_indeterminate"] for corpus, row in summaries.items()},
        "scored_cases": len(scored["cases"]),
        "subset_override": subset_override,
        "missing_subset_cases": missing,
        "refused_cases": refused,
        "exposed_cases": exposed,
        "completion_decidable": completion_decidable,
        "numerical_gate_passed": numerical_gate_passed,
        "assisted_result_claimed": numerical_gate_passed,
        "dry_run": dry_run,
        "reason": reason,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--workspace-root", type=Path, required=True)
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--crops", type=Path, required=True)
    parser.add_argument("--full", type=Path, required=True)
    parser.add_argument("--gate", choices=["v1", "v2"], default="v2",
                        help="pass rule applied to the retained results, default %(default)s")
    parser.add_argument("--dry-run", action="store_true",
                        help="accept a synthetic replay; never an assisted result")
    args = parser.parse_args()

    root = args.workspace_root.resolve()
    workspace, private = root / "workspace", root / "private"
    manifest, mapping = verify_workspace(root)
    forbidden = (exporter.BASE_FORBIDDEN
                 + tuple(case["case"] for case in mapping["cases"])
                 + tuple(case["reference_id"] for case in mapping["cases"]))
    audit_scored_workspace(workspace, forbidden)
    replays = replay_manifest(workspace, manifest)

    output = exporter.checked_output(args.output)
    predeclared = {
        "frozen_before_scoring_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "candidate": json.loads(DECLARATION.read_text()),
        "declaration_sha256": sha(DECLARATION),
        "executable": str(args.executable.resolve()),
        "executable_sha256": sha(args.executable),
        "source_sha256": {name: sha(ROOT / name) for name in PINNED_SOURCES},
        "reference_manifest_sha256": {str(path): sha(path) for path in
                                      [args.crops / "references.json",
                                       args.full / "full-references.json"]},
        "workspace_id": manifest["workspace_id"],
        "workspace_manifest_sha256": sha(workspace / "manifest.json"),
        "mapping_sha256": sha(private / "mapping.json"),
        "mapping_program_sha256": mapping["program_sha256"],
        "protocol_sha256": sha(PROTOCOL_DOC),
        "task_card_sha256": manifest["task_card_sha256"],
        "replays": replays,
        "dry_run": args.dry_run,
        "gate": args.gate,
    }
    (output / "predeclared.json").write_text(json.dumps(predeclared, indent=2) + "\n")

    commands = [
        [sys.executable, "-B", str(ROOT / "scripts/phase6_reference_fixtures.py"),
         str(args.crops), "--verify-only"],
        [sys.executable, "-B", str(ROOT / "scripts/phase6_full_reference_fixtures.py"),
         str(args.full), "--verify-only"],
        [str(args.executable.resolve()), str(workspace), str(private / "mapping.json"),
         str(args.crops), str(args.full), str(output / "matting")]
        + (["--synthetic-dry-run"] if args.dry_run else []),
    ]
    stages = []
    for number, command in enumerate(commands):
        with (output / f"stage-{number}.log").open("xb") as log:
            result = subprocess.run(command, cwd=ROOT, stdout=log,
                                    stderr=subprocess.STDOUT, check=False)
        stages.append({"command": command, "returncode": result.returncode})
        if result.returncode:
            (output / "run.json").write_text(json.dumps(
                {"predeclared": predeclared, "stages": stages, "runner_failed": True},
                indent=2) + "\n")
            return result.returncode

    scored = rescore.score_run(output)
    (output / "gate-rescore.json").write_text(json.dumps(scored, indent=2, sort_keys=True) + "\n")
    metrics = output / "matting/metrics.json"
    scorer_refusals = json.loads(metrics.read_text()).get("refused") if metrics.exists() else None
    decision = verdict(scored, mapping, replays, args.gate, args.dry_run, scorer_refusals)
    (output / "run.json").write_text(json.dumps({
        "predeclared": predeclared, "stages": stages, "summaries": scored["summaries"],
        "gate": args.gate, "gate_summaries": decision["gate_summaries"],
        "v2_indeterminate": decision["v2_indeterminate"],
        "numerical_gate_passed": decision["numerical_gate_passed"], "verdict": decision,
        "physical_translucency_reference_available": False,
        "advanced_controls_enabled": False}, indent=2) + "\n")
    print(json.dumps(decision, indent=2), flush=True)
    if args.dry_run:
        clean = all(stage["returncode"] == 0 for stage in stages)
        return 0 if clean and not decision["assisted_result_claimed"] else 1
    return 0 if decision["numerical_gate_passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
