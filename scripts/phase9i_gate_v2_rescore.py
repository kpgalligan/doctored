#!/usr/bin/env python3
"""Re-score preserved Phase 9I candidate records under gate v1 and gate v2.

Gate v2 is frozen verbatim in archive/phase-9i-gate-v2-status.md before this
script was written: the analytic pass rule keeps only the twelve recoverable
cases, ambiguous and glass cases become reported diagnostics, and v1's
"reference-opaque RGB must not change at all" clause becomes a bounded rule.
Nothing here re-runs a candidate. The preserved metrics.json files and the
exported PNG planes are the only inputs; a case the preserved record cannot
decide is reported indeterminate and never counted as a v2 pass.
"""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import subprocess
import sys

sys.dont_write_bytecode = True
from verify_phase2 import read_png

ROOT = Path(__file__).resolve().parents[1]
REPORTS_ROOT = Path.home() / "devel/doctored-fixtures/phase9i-reports"
CANDIDATES = {
    "v1": "rz-phase9i-soft-prior-v1",
    "v2": "rz-phase9i-soft-prior-v2",
    "v3": "rz-phase9i-soft-prior-v3",
    "v4": "rz-phase9i-soft-prior-v4",
    "v5": "rz-phase9i-joint-bayesian-v5",
}
# Case ids come from SUBJECTS and the composite background arms in
# core/tests/common/selection_quality.rs; glass takes precedence over ambiguous.
RECOVERABLE_SUBJECTS = ("portrait_hair", "fur", "hard_product")
VARIANTS = ("blurred", "binary")
PASS_RULE_ANALYTIC = frozenset(
    f"{subject}-{background}-{variant}"
    for subject in RECOVERABLE_SUBJECTS
    for background in ("light", "dark")
    for variant in VARIANTS
)
AMBIGUOUS_DIAGNOSTIC = frozenset(
    f"{subject}-ambiguous-{variant}" for subject in RECOVERABLE_SUBJECTS for variant in VARIANTS
)
GLASS_RESEARCH = frozenset(
    f"translucent_glass-{background}-{variant}"
    for background in ("light", "dark", "ambiguous")
    for variant in VARIANTS
)
OPAQUE_MAX_CODE_DELTA = 2
OPAQUE_MAX_CHANGED_FRACTION = 0.01
OPAQUE_BOUNDARY_CHEBYSHEV = 2
PHOTO_CROPS = tuple(
    f"photo-0051115Q_000001_{index}-{background}-{variant}"
    for index in ("0003", "0031", "0060", "0088")
    for background in ("light", "dark", "checker")
    for variant in VARIANTS
)
ASSISTED_FULL_IMAGES = (
    "fur-longhair",
    "hard-arduboy",
    "portrait-0051115Q_000001_0003",
    "portrait-0051115Q_000001_0031",
    "portrait-0051115Q_000001_0060",
    "portrait-0051115Q_000001_0088",
)
# The two lexicographically-first case ids of each non-bottle full image.
ASSISTED_COMPLETION_SUBSET = frozenset(PHOTO_CROPS) | frozenset(
    f"full-{image}-checker-{variant}" for image in ASSISTED_FULL_IMAGES for variant in VARIANTS
)
# Serialized copy of color_gate_contract() in core/examples/phase6_matting_quality.rs.
COLOR_GATE = {
    "fractional_foreground_mad_max": 0.05,
    "visible_fringe_initial_mad": 0.02,
    "visible_fringe_min_improvement": 0.2,
    "recomposite_boundary_mad_max": 0.02,
    "opaque_changed_pixels_max": 0,
    "weighted_mad_regression_allowance": 1.0 / 255.0,
}
CORPORA = {"analytic": "analytic", "photographic_recompositions": "crops", "full_images": "full"}
INDETERMINATE_REASON = (
    "no exported planes and no v2 metric fields; only the changed fraction is known"
)


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def _plane(path, cache, gray):
    """Decode a PNG plane once per distinct file, kept as compact bytes.

    Source and truth-alpha planes are byte-identical across candidates, so the
    sha256 key collapses them to a single decode. The key is (digest, gray),
    not the digest alone: the same file read gray and read RGB yields different
    samples, so one key per mode. No pixel cap and no rescaling: unlike
    phase6_matting_diagnostics.opaque_delta this measurement gates, so a
    2304x3456 portrait is decoded in full however long that takes.
    """
    data = Path(path).read_bytes()
    digest = (hashlib.sha256(data).hexdigest(), gray)
    cached = cache.get(digest)
    if cached is None:
        width, height, pixels = read_png(data)
        if gray:
            samples = bytes(pixel[0] for pixel in pixels)
        else:
            samples = bytes(channel for pixel in pixels for channel in pixel[:3])
        cached = (width, height, samples)
        cache[digest] = cached
    return cached


def opaque_rule_from_metrics(after):
    """Gate v2 opaque rule from the v2 fields the Rust evaluator now emits."""
    if "opaque_changed_max_code_delta" not in after or "opaque_changed_outside_boundary" not in after:
        return None
    opaque = after["opaque_pixels"]
    changed = after["opaque_changed_pixels"]
    delta = after["opaque_changed_max_code_delta"]
    outside = after["opaque_changed_outside_boundary"]
    fraction = changed / opaque if opaque else 0.0
    return {
        "source": "metrics",
        "measured": True,
        "reason": None,
        "reference_opaque_pixels": opaque,
        "changed_pixels": changed,
        "max_rgb_code_delta": delta,
        "changed_fraction": fraction,
        "changed_outside_boundary": outside,
        "passes": delta <= OPAQUE_MAX_CODE_DELTA
        and fraction <= OPAQUE_MAX_CHANGED_FRACTION
        and outside == 0,
    }


def opaque_rule_from_planes(root, case, cache):
    """Gate v2 opaque rule recomputed from the exported source/estimate/truth planes.

    Changed, max delta and the boundary test repeat the Rust definitions in
    core/tests/common/matting_quality.rs: a reference-opaque pixel is changed when
    any channel of the estimated foreground differs from the source byte, and it is
    inside the boundary set when some pixel within Chebyshev distance 2 (clipped to
    the image) has truth-alpha != 255.
    """
    root = Path(root)
    names = [
        f"{case}-source.png",
        f"{case}-estimated-foreground.png",
        f"{case}-truth-alpha.png",
    ]
    if not all((root / name).exists() for name in names):
        return {
            "source": None,
            "measured": False,
            "reason": INDETERMINATE_REASON,
            "reference_opaque_pixels": None,
            "changed_pixels": None,
            "max_rgb_code_delta": None,
            "changed_fraction": None,
            "changed_outside_boundary": None,
            "passes": None,
        }
    width, height, source = _plane(root / names[0], cache, gray=False)
    estimate = _plane(root / names[1], cache, gray=False)
    alpha = _plane(root / names[2], cache, gray=True)
    assert estimate[:2] == (width, height) and alpha[:2] == (width, height), case
    estimate, alpha = estimate[2], alpha[2]
    reach = OPAQUE_BOUNDARY_CHEBYSHEV
    opaque, changed, maximum, outside = 0, 0, 0, 0
    for index in range(width * height):
        if alpha[index] != 255:
            continue
        opaque += 1
        base = index * 3
        delta = max(abs(source[base + c] - estimate[base + c]) for c in range(3))
        if not delta:
            continue
        changed += 1
        maximum = max(maximum, delta)
        y, x = divmod(index, width)
        varying = any(
            alpha[ny * width + nx] != 255
            for ny in range(max(y - reach, 0), min(y + reach, height - 1) + 1)
            for nx in range(max(x - reach, 0), min(x + reach, width - 1) + 1)
        )
        if not varying:
            outside += 1
    fraction = changed / opaque if opaque else 0.0
    return {
        "source": "planes",
        "measured": True,
        "reason": None,
        "reference_opaque_pixels": opaque,
        "changed_pixels": changed,
        "max_rgb_code_delta": maximum,
        "changed_fraction": fraction,
        "changed_outside_boundary": outside,
        "passes": maximum <= OPAQUE_MAX_CODE_DELTA
        and fraction <= OPAQUE_MAX_CHANGED_FRACTION
        and outside == 0,
    }


def opaque_rule(record, root, case, cache):
    """Measurement precedence: v2 metric fields, exported planes, nothing changed, indeterminate."""
    after = record.get("after")
    if after is None:
        return {
            "source": None,
            "measured": False,
            "reason": "the color record refused; no opaque measurement exists",
            "reference_opaque_pixels": None,
            "changed_pixels": None,
            "max_rgb_code_delta": None,
            "changed_fraction": None,
            "changed_outside_boundary": None,
            "passes": None,
        }
    from_metrics = opaque_rule_from_metrics(after)
    if from_metrics is not None:
        return from_metrics
    if after["opaque_changed_pixels"] > 0:
        measured = opaque_rule_from_planes(root, case, cache)
        if measured["measured"]:
            return measured
        measured["changed_fraction"] = (
            after["opaque_changed_pixels"] / after["opaque_pixels"] if after["opaque_pixels"] else 0.0
        )
        measured["reference_opaque_pixels"] = after["opaque_pixels"]
        measured["changed_pixels"] = after["opaque_changed_pixels"]
        return measured
    return {
        "source": "zero_changed",
        "measured": True,
        "reason": None,
        "reference_opaque_pixels": after["opaque_pixels"],
        "changed_pixels": 0,
        "max_rgb_code_delta": 0,
        "changed_fraction": 0.0,
        "changed_outside_boundary": 0,
        "passes": True,
    }


def coupled_key(record):
    return "estimated_alpha_color" if "estimated_alpha_color" in record else "sampled_alpha_color"


def is_refused(record):
    alpha = record.get("alpha_candidate", record.get("sampling", {}))
    return "refusal" in alpha or record[coupled_key(record)].get("alpha_candidate_refused", False)


def alpha_passes(record):
    """Mirror of quality::alpha_gate plus the refusal and fractional-reference guards."""
    before, after = record["before"], record["after"]
    return (
        not is_refused(record)
        and record.get("has_fractional_reference", False)
        and after["boundary_mad"] <= before["boundary_mad"] + 1.0 / 255.0
        and after["soft_detail_recall"] >= 0.95
    )


def color_failures(record, gate, opaque):
    """Reason names reused from phase6_matting_diagnostics.failures.

    Gate v2 swaps the exact-zero opaque clause for the bounded rule; everything
    else is byte-for-byte the v1 decision.
    """
    if "refusal" in record:
        return ["foreground_refusal"]
    before, after = record["before"], record["after"]
    result = []
    if gate == "v1":
        if after["opaque_changed_pixels"] > COLOR_GATE["opaque_changed_pixels_max"]:
            result.append("reference_opaque_rgb_changed")
    elif opaque is None:
        pass  # A refused candidate fails on its own reason; no opaque measurement exists.
    elif opaque["passes"] is None:
        result.append("opaque_rule_indeterminate")
    elif not opaque["passes"]:
        result.append("opaque_rule_failed")
    if after["alpha_weighted_foreground_mad"] > (
        before["alpha_weighted_foreground_mad"] + COLOR_GATE["weighted_mad_regression_allowance"]
    ):
        result.append("alpha_weighted_foreground_regression")
    if after["fractional_foreground_mad"] > COLOR_GATE["fractional_foreground_mad_max"]:
        result.append("fractional_foreground_mad_above_limit")
    if before["fractional_foreground_mad"] > COLOR_GATE["visible_fringe_initial_mad"] and after[
        "fractional_foreground_mad"
    ] > (
        before["fractional_foreground_mad"] * (1 - COLOR_GATE["visible_fringe_min_improvement"])
    ):
        result.append("insufficient_visible_fringe_improvement")
    if any(
        after["recomposite"][name]["boundary_mad"] > COLOR_GATE["recomposite_boundary_mad_max"]
        for name in ["white", "dark", "checker"]
    ):
        result.append("replacement_boundary_mad_above_limit")
    if not record.get("has_fractional_reference", False):
        result.append("no_fractional_reference")
    if record.get("alpha_candidate_refused", False):
        result.append("alpha_candidate_refused")
    return result


def role_of(case, corpus):
    if corpus != "analytic":
        return "pass_rule"
    if case in GLASS_RESEARCH:
        return "glass_research"
    if case in AMBIGUOUS_DIAGNOSTIC:
        return "ambiguous_diagnostic"
    assert case in PASS_RULE_ANALYTIC, case
    return "pass_rule"


def case_verdict(record, corpus, root, cache):
    case = record["case"]
    coupled = record[coupled_key(record)]
    refused = is_refused(record)
    alpha_pass = alpha_passes(record)
    v1_failures = color_failures(coupled, "v1", None)
    v1_pass = alpha_pass and not v1_failures
    assert v1_pass == record["passes_end_to_end_gate"], (case, v1_pass, v1_failures)
    after = coupled.get("after")
    # Planes are decoded only for a case that could still pass: not refused, some
    # reference-opaque pixel changed, and the v2 metric fields absent.
    opaque = None if refused or after is None else opaque_rule(coupled, root, case, cache)
    if opaque is not None and opaque["source"] == "planes":
        assert opaque["changed_pixels"] == after["opaque_changed_pixels"], (
            case,
            opaque["changed_pixels"],
            after["opaque_changed_pixels"],
        )
    v2_failures = color_failures(coupled, "v2", opaque)
    if not alpha_pass:
        v2_pass = False
    elif v2_failures == ["opaque_rule_indeterminate"]:
        v2_pass = None
    else:
        v2_pass = not v2_failures
    return {
        "case": case,
        "corpus": corpus,
        "role": role_of(case, corpus),
        "refused": refused,
        "alpha_pass": alpha_pass,
        "v1_pass": v1_pass,
        "v2_pass": v2_pass,
        "v1_failures": v1_failures,
        "v2_failures": v2_failures,
        "opaque": opaque,
    }


def score_metrics(metrics_path, cache):
    metrics_path = Path(metrics_path)
    report = json.loads(metrics_path.read_text())
    assert report["color_gate"] == COLOR_GATE, metrics_path
    verdicts = {}
    for key, corpus in CORPORA.items():
        if key not in report:
            continue
        verdicts[corpus] = [
            case_verdict(record, corpus, metrics_path.parent, cache) for record in report[key]
        ]
    return verdicts


def score_run(run_dir, cache=None):
    run_dir = Path(run_dir)
    cache = {} if cache is None else cache
    candidates = [
        ["analytic-crops/metrics.json", "full/metrics.json"],
        ["matting/metrics.json"],
        ["images/metrics.json"],
    ]
    paths = next(
        (
            [run_dir / name for name in group]
            for group in candidates
            if all((run_dir / name).exists() for name in group)
        ),
        None,
    )
    if paths is None:
        raise ValueError(f"No qualification metrics found under {run_dir}")
    verdicts = {}
    for path in paths:
        for corpus, rows in score_metrics(path, cache).items():
            verdicts.setdefault(corpus, []).extend(rows)
    return {
        "directory": str(run_dir),
        "metrics_sha256": {str(path.relative_to(run_dir)): sha256(path) for path in paths},
        "summaries": {corpus: summarize(rows) for corpus, rows in verdicts.items()},
        "cases": [row for rows in verdicts.values() for row in rows],
    }


def summarize(verdicts):
    return {
        "cases": len(verdicts),
        "pass_rule_cases": sum(1 for row in verdicts if row["role"] == "pass_rule"),
        "v1_passes": sum(1 for row in verdicts if row["v1_pass"]),
        "v2_passes": sum(1 for row in verdicts if row["role"] == "pass_rule" and row["v2_pass"]),
        "v2_indeterminate": sum(
            1 for row in verdicts if row["role"] == "pass_rule" and row["v2_pass"] is None
        ),
        "refusals": sum(1 for row in verdicts if row["refused"]),
        "ambiguous_diagnostic": sum(1 for row in verdicts if row["role"] == "ambiguous_diagnostic"),
        "glass_research": sum(1 for row in verdicts if row["role"] == "glass_research"),
    }


def gate_report(summaries):
    """Both gates side by side, so --gate v1 stays readable from a v2 run."""
    return {
        "v1": {corpus: {"cases": row["cases"], "passes": row["v1_passes"],
                        "qualified": row["cases"] == row["v1_passes"]}
               for corpus, row in summaries.items()},
        "v2": {corpus: {"pass_rule_cases": row["pass_rule_cases"], "passes": row["v2_passes"],
                        "indeterminate": row["v2_indeterminate"],
                        "qualified": row["pass_rule_cases"] == row["v2_passes"]}
               for corpus, row in summaries.items()},
    }


def qualified(summaries, gate):
    if gate == "v1":
        return all(row["cases"] == row["v1_passes"] for row in summaries.values())
    return all(row["pass_rule_cases"] == row["v2_passes"] for row in summaries.values())


def render_table(report):
    order = ["analytic", "crops", "full"]
    lines = [
        "| candidate | " + " | ".join(order) + " |",
        "| --- | " + " | ".join("---" for _ in order) + " |",
    ]
    for name, candidate in report["candidates"].items():
        cells = []
        for corpus in order:
            summary = candidate["summaries"].get(corpus)
            if summary is None:
                cells.append("-")
                continue
            cell = f"{summary['v1_passes']} / {summary['v2_passes']}"
            if summary["v2_indeterminate"]:
                cell += f" (+{summary['v2_indeterminate']} indeterminate)"
            cells.append(cell)
        lines.append(f"| {name} | " + " | ".join(cells) + " |")
    lines.append("")
    lines.append(
        "Cells are v1 passes / v2 passes. v1 counts every case in the corpus; v2 counts only "
        "gate v2 pass-rule cases (analytic drops 6 ambiguous diagnostics and 6 glass "
        "transmission-research cases; crops 24 and full 42 stay whole)."
    )
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reports-root", type=Path, default=REPORTS_ROOT)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--candidates", nargs="+", default=sorted(CANDIDATES))
    parser.add_argument("--table", type=Path)
    args = parser.parse_args()
    output = (args.output or args.reports_root / "gate-v2/rescore.json").resolve()
    if output.is_relative_to(ROOT):
        parser.error("re-scoring output must stay outside the repository")
    table = (args.table or output.with_suffix(".md")).resolve()
    if table.is_relative_to(ROOT):
        parser.error("re-scoring table must stay outside the repository")
    head = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True
    ).stdout.strip()
    cache = {}
    report = {
        "schema_version": 1,
        "captured_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "repo_head": head,
        "gate_v2": {
            "pass_rule_analytic": sorted(PASS_RULE_ANALYTIC),
            "ambiguous_diagnostic": sorted(AMBIGUOUS_DIAGNOSTIC),
            "glass_research": sorted(GLASS_RESEARCH),
            "opaque_max_code_delta": OPAQUE_MAX_CODE_DELTA,
            "opaque_max_changed_fraction": OPAQUE_MAX_CHANGED_FRACTION,
            "opaque_boundary_chebyshev": OPAQUE_BOUNDARY_CHEBYSHEV,
            "color_gate": COLOR_GATE,
        },
        "assisted_completion_subset": sorted(ASSISTED_COMPLETION_SUBSET),
        "candidates": {},
    }
    for name in args.candidates:
        scored = score_run(args.reports_root / CANDIDATES[name], cache)
        report["candidates"][name] = scored
        print(name, json.dumps(scored["summaries"], sort_keys=True), flush=True)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    table.parent.mkdir(parents=True, exist_ok=True)
    table.write_text(render_table(report))
    print(render_table(report))
    print(f"{output} {sha256(output)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
