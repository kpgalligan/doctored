# Phase 9I assisted scoring path (ADR)

- **Status:** Accepted — closed (scorer built; only a synthetic dry run executed; the real run is tracked elsewhere)
- **Phase / dates:** 9I assisted protocol phase 2, 2026-09-16
- **Original:** [archive/phase-9i-assisted-scoring-status.md](../archive/phase-9i-assisted-scoring-status.md) — dry-run receipt, hash history, test list
- **Key commits:** `5f75604`

## Context
Phase 2 of [assisted protocol v1](phase-9i-assisted-protocol-v1.md): score a workspace exported by
[phase 1](phase-9i-assisted-workspace-status.md) plus the human replays written into it. It must rebuild each
case's label plane exactly as the app rendered it, run one frozen kernel with those labels as hard constraints,
and hand results to the unchanged gate-v2 re-scorer in the automatic runs' report shape. Still a measurement
instrument: nothing reaches the app, and no assisted result exists.

## Decisions
- **One candidate:** `assisted_tiled_soft_prior_v1` ([phase-9i-assisted-scoring-v1.json](phase-9i-assisted-scoring-v1.json)):
  `tiled_soft_matting::estimate(colors, (w,h), prior, SoftPriorInput { version: 1, user_constraints: Some(labels), prior_weight: 1.0 }, &ClosedFormParams::default(), None)`.
  *Why:* the global `estimate_soft_prior` refuses every subset extent (`closed_form_matting::MAX_UNKNOWN_PIXELS = 500_000`);
  the tiled kernel ran all 90 cases inside 2 GiB in V2; `copy_coverage` copies labeled pixels verbatim so a human
  label is exact, not weighted. V3 protection is a failed hypothesis, V4 estimates nothing, V5 is not guided.
  No second kernel, tuning or protection.
- **Reads:** [`core/examples/phase9i_assisted_quality.rs`](../core/examples/phase9i_assisted_quality.rs)
  `<workspace> <mapping.json> <crops> <full> <output> [--synthetic-dry-run]`. `solver_inputs`
  (`core/tests/common/assisted_scoring.rs`) opens **only** `cases/<anon>/source.png` (must be `ImageRgb8`),
  `cases/<anon>/prior.png` (`ImageLuma8`) and `replays/<anon>.json`, checking extent and all four declared hashes.
  Reference foreground is read for metrics only, and the run **panics** unless the frozen recipe reproduces the
  workspace planes byte for byte (wrong corpus is not scoreable).
- **Replay admission (`check_scoreable`):** refuses non-final, `synthetic` (unless dry run), human-but-unscoreable
  or unattested sessions, mismatched `case_id`/`workspace_id`/extent, any mismatched `displayed_assets` hash or
  `task_card_sha256`, or no `label_plane`. Every outcome (`submitted`, `budget_exhausted`, `incomplete`) is accepted.
- **Replay hash audit (`verify_replay_hashes`):** re-derives every event's label plane from the truncated session
  and requires the app-recorded hash; `assisted_replay::label_plane` is the only rasterizer on either side.
- **Refusals are scored, not skipped:** missing, malformed, unscoreable, hash-failed replays and a failed
  `256·pixels + 64 MiB ≤ 2 GiB` admission each produce a refusal record (`before == after`, `passes_*_gate: false`,
  `alpha_candidate_refused: true`) in the denominator.
- **Exposure** (mapping flag or replay's `annotator_reports_prior_exposure`) is scored, counted and listed, never excluded.
- **Input policy:** `InputPolicy::AssistedHumanLabelsV1` and `evaluate_candidate_with_labels` in
  `core/examples/phase6_matting_quality.rs`; `evaluate_candidate` forwards with `None`, so automatic candidates are
  byte-identical. The label plane must be the case extent and categorical `{0,128,255}`; the existing
  "candidate may not change a known label" check now guards the human labels.
- **Runner:** [`scripts/phase9i_assisted_scoring.py`](../scripts/phase9i_assisted_scoring.py)
  `OUTPUT --workspace-root DIR --executable P --crops D --full D [--gate v1|v2] [--dry-run]`. Verifies both schemas,
  `workspace_id`, every `case.json`, manifest/mapping agreement; `audit_scored_workspace` = exporter audit plus one
  replay per manifest id (token check on replay string *values* only, since `alpha_write` is a key). Writes
  `predeclared.json` before scoring (declaration, executable, `PINNED_SOURCES`, manifests, protocol and task-card
  hashes, `dry_run`, `gate`), then runs the fixture verifiers, the scorer, `rescore.score_run` into
  `gate-rescore.json`, and `run.json`.
- **Verdict separate from numbers:** `completion_decidable = not dry_run and not subset_override and nothing missing`;
  `numerical_gate_passed` also needs `rescore.qualified`; `assisted_result_claimed == numerical_gate_passed`.
  A real run exits 0 only on a pass; a dry run exits 0 only when every stage succeeded and nothing was claimed.

## Consequences
- A real run (human annotator, all 36 cases, no override, no `--dry-run`) exiting 0 is the evidence 9I's
  completion criterion asks for; exiting 1 is an equally reportable failure that closes nothing.
- Pinned sources (`phase6_matting_quality.rs`, `phase9i_assisted_workspace.rs`) changed hash in this slice; a future
  run pins the new hashes and the scorer re-derives source/prior rather than trusting old pins.
- Tests: `core/tests/assisted_scoring.rs` (includes the committed app-written
  `core/tests/fixtures/assisted/drive-synthetic-replay.json`), `scripts/test_phase9i_assisted_scoring.py`.

## Open items (verified 2026-09-26)
- `TRACKED` — Score a real human run → P7-12 / plan §9I; procedure in [phase-9i-assisted-annotation-runbook.md](phase-9i-assisted-annotation-runbook.md). Only dry-run directories (`assisted-scoring-dryrun*`) exist under `~/devel/doctored-fixtures/phase9i-reports/`.
