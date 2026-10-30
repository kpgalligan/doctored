# Phase 9I assisted scoring path — status

**What this is.** Phase 2 of the
[assisted edge refinement protocol v1](phase-9i-assisted-protocol-v1.md): the
scorer for a workspace exported by
[phase 1](phase-9i-assisted-workspace-status.md) and the human replays written
back into it. It rebuilds each case's label plane from its replay exactly as the
app rendered it, runs ONE frozen existing kernel with those labels as hard
constraints, and hands the result to the unchanged
`scripts/phase9i_gate_v2_rescore.py` `score_run` in the automatic runs' own
report shape.

It is still a **measurement instrument**. No user-facing control is enabled, no
candidate or score reaches the app, and **no assisted result exists**: the owner has
not annotated, so the real 36-case session has not been run. The only thing
executed here is a synthetic dry run that proves plumbing, and the report,
the verdict and the exit rule all refuse to call it a result.
[Gate v2](phase-9i-gate-v2-status.md) is untouched; 9I and P7-12 remain open.

## The declared candidate

[`phase-9i-assisted-scoring-v1.json`](../designs/phase-9i-assisted-scoring-v1.json),
candidate `assisted_tiled_soft_prior_v1`:

```
tiled_soft_matting::estimate(colors, (w, h), prior,
    SoftPriorInput { version: 1, user_constraints: Some(labels), prior_weight: 1.0 },
    &ClosedFormParams::default(), None)
```

**Owner decision 2 — why this kernel, and only this one.** These are V2's
`tiled_soft_prior_v2` parameters unchanged, with no foreground protection. The
global `estimate_soft_prior` refuses every extent in the subset
(`MAX_UNKNOWN_PIXELS = 500_000`; the V1 receipts refuse all 24 crops at
1,638,400 unknowns and every full image), while the tiled kernel ran all 90
cases in V2 inside the 2 GiB harness — roughly 1.1 s per crop and 4.3 s per
portrait. `closed_form_matting::estimate_input_job` copies labeled pixels
verbatim through `copy_coverage` and solves only the 128 set, so a human label
is an EXACT constraint rather than a weighted preference. V3's protection is a
recorded failed hypothesis, V4 estimates nothing, and V5 is not the guided path.
No second kernel, no parameter tuning and no protection is in scope.

Owner decisions 1 and 3–5 are recorded in the declaration JSON and in the
sections below; they are not reopened here.

## What the scorer reads, and what it refuses to read

`core/examples/phase9i_assisted_quality.rs`:

```
phase9i_assisted_quality <workspace> <mapping.json> <crops> <full> <output> [--synthetic-dry-run]
```

Per case, in case-id order:

1. `core/tests/common/assisted_scoring.rs::solver_inputs` opens **only**
   `cases/<anon>/source.png`, `cases/<anon>/prior.png` and
   `replays/<anon>.json`. The source must decode as `ImageRgb8` and the prior as
   `ImageLuma8` (any alpha channel refuses), both at the declared extent, and
   all four declared PNG and decoded-plane sha256 must match. `SolverInputs.read_files`
   records exactly the files opened and is carried into the record.
2. The reference foreground is then read for **metrics only**, through the same
   `workspace::case_foreground` the exporter used, and the run PANICS unless
   `DynamicImage::ImageRgba8(frozen::photo_source(&fg, background)).to_rgb8()`
   and `frozen::prior(&truth, start)` equal the workspace planes byte for byte.
   A mismatch means the wrong corpus, which is not a scoreable condition.
3. The solver still receives `colors`, `prior` and `user_constraints` alone
   (`EstimateAlpha`); truth reaches `evaluate_candidate_with_labels` for metrics
   exactly as it does in every existing evaluator.

**Decision 3 — which replays may be scored.** `check_scoreable` refuses, each
with its own reason: a session that is not final; a `synthetic` origin (unless
`--synthetic-dry-run`); a human session that is not `scoreable` or carries no
`blinding_attestation`; a `case_id`, `workspace_id` or extent that is not this
case's; any of the four `displayed_assets` hashes or the `task_card_sha256` that
is not the workspace's; and a replay with no recorded `label_plane`. Every
recorded OUTCOME — `submitted`, `budget_exhausted`, `incomplete` — is accepted,
because an unfinished plane is still what the annotator drew.

**The replay hash audit.** `verify_replay_hashes` re-derives event *i*'s label
plane from the session truncated after event *i* and requires its sha256 to
equal the hash the app recorded, for every event, plus the document's own
`label_plane.sha256` against the last event's and against the whole replay's
plane. `assisted_replay::label_plane` is the only rasterizer on either side.

**Refusals are scored, not skipped.** A missing replay (`no replay`), a
malformed one (`malformed replay: …`), an unscoreable one (`replay refused: …`)
and a failed audit (`replay hash audit failed: …`) each produce a refusal record
carrying `before == after` (the prior, diagnostic only), `passes_*_gate: false`
and `estimated_alpha_color.alpha_candidate_refused: true` — the shape
`score_run` already reads — so they stay in the denominator. So does the
`256 * pixels + 64 MiB <= 2 GiB` harness admission.

**Exposure.** A case declared `annotator_exposed` in the mapping, or whose
replay reports `annotator_reports_prior_exposure`, is scored, counted and
LISTED (`annotator_exposed` per record, `exposed` in the report,
`exposed_cases` in the verdict). It is never excluded.

## The input policy

`core/examples/phase6_matting_quality.rs` gains
`InputPolicy::AssistedHumanLabelsV1` and the eight-argument
`evaluate_candidate_with_labels`; `evaluate_candidate` is now a one-line forward
with `None`, so every automatic candidate is byte-identical. The assisted arm
asserts the label plane is the case extent and categorical `{0, 128, 255}`, uses
it as `constraints`, and records

```json
{"version":1,"automatic_prior":"soft","user_constraints":"assisted_human_labels_v1",
 "automatic_labels_used_as_hard_constraints":false,
 "human_labels_used_as_hard_constraints":true,
 "labeled_foreground_pixels":n,"labeled_background_pixels":n,"unknown_pixels":n}
```

beside the same `legacy_automatic_constraint_audit` the soft policy writes. The
existing constraint check — a candidate may not change a known categorical
label — now guards the HUMAN labels.

## The runner

`scripts/phase9i_assisted_scoring.py OUTPUT --workspace-root DIR --executable P
--crops D --full D [--gate v1|v2] [--dry-run]`

`verify_workspace` re-verifies both schemas, the shared `workspace_id`, every
`case.json` recomputed from its own planes through the exporter's own
`case_entry`, and that the manifest and mapping describe the same cases.
`audit_scored_workspace` is the exporter's file-set / PNG-storage / token audit
widened by exactly one thing — one `replays/<anon>.json` per MANIFEST anonymous
id — and it token-checks a replay on string VALUES only, because the replay
schema's own keys contain `alpha` (`alpha_write`). `replay_manifest` records what
came back per case, with `None` for a case never opened.

`predeclared.json` is written BEFORE the scorer runs: the declaration and its
hash, the executable and its hash, `PINNED_SOURCES` (the replay reader and its
rasterizer, the kernel and its inputs, the evaluator chain, the shared scoring
seam, and the three driving scripts), the two reference manifests, the
workspace id, the workspace manifest and mapping hashes, the mapping's copied
`program_sha256`, the current protocol-document hash, the task-card hash, the
replay manifest, `dry_run` and `gate`.

Stages: the two pinned `--verify-only` fixture checks, then the scorer; then the
unchanged `rescore.score_run(OUTPUT)` into `gate-rescore.json`, and `run.json`.

**The verdict is separate from the numbers.** `completion_decidable` is
`not dry_run and not subset_override and no missing completion-subset case`;
`numerical_gate_passed` requires that AND `rescore.qualified`; and
`assisted_result_claimed` is exactly `numerical_gate_passed`. A real run exits
`0` only when the gate passes. A dry run exits `0` only when every stage
succeeded **and** nothing was claimed.

## Dry-run receipt (2026-09-16)

`~/devel/rasterize-fixtures/phase9i-reports/assisted-scoring-dryrun-fixpass`, over the
phase-1 drive workspace `assisted-workspace-drive` (two cases, one synthetic
replay), with `--dry-run`. Exit 0.

- `scored_cases: 2`, `dry_run: true`, `assisted_result_claimable: false`,
  `completion_decidable: false`, `assisted_result_claimed: false`,
  `subset_override: true`, 34 `missing_subset_cases`.
- `full-portrait-0051115Q_000001_0003-checker-binary` scored:
  `replay_hash_audit {"events": 14, "prefix_planes_verified": 14}` — all
  fourteen app-recorded plane hashes reproduced, no refusal, 11 active strokes,
  `labeled_foreground_pixels: 679200`, `labeled_background_pixels: 102`,
  `unknown_pixels: 7283322`, candidate 4.58 s over 126 tiles. The gate is NOT
  passed (`boundary_mad` 0.0972 → 0.1409): a synthetic corner-to-corner scribble
  is not an annotation, and this number is a plumbing artifact, not a
  measurement of assisted quality.
- `photo-0051115Q_000001_0003-light-blurred` refused `no replay`, and is counted
  in the denominator.
- `gate-rescore.json` written by the unchanged `score_run`:
  `crops {cases 1, v1_passes 0, v2_passes 0, refusals 1}`,
  `full {cases 1, v1_passes 0, v2_passes 0, refusals 0}`.

## Changed recipe hashes

`core/examples/phase6_matting_quality.rs` gained the assisted policy and the
label-carrying evaluator, and `core/examples/phase9i_assisted_workspace.rs`
gained three visibility words (`pub(crate)` on `mod full`, `parse_case` and
`case_foreground`) so the scorer reuses the exporter's own case resolution
instead of copying it. Both files' sha256 are pinned by
`mapping.json.program_sha256` and by the declaration template's
`recipe_source_pins`. As gate v2 and phase 1 both established, a future run pins
the new hashes; the template is not edited and no preserved run is touched.

- `phase6_matting_quality.rs` old `52822167684dad69a3f26de98869d3c2ff5d2c11926e7b11ea103c7256f14351`
- `phase6_matting_quality.rs` new `3ec88571602ff2290321d04130b85c799291440a7818d70bb0c642da7a078647`
- `phase9i_assisted_workspace.rs` old `e0af61dd336bef0371462ed45304c73d76ec27156f810fbddf7b7e97901825a8`
- `phase9i_assisted_workspace.rs` new `773de2f3973c70792247c6adfdfcef4fa3a316789157abbfd45ac76f426251a9`

The drive workspace's own `mapping.json` still pins the OLD
`phase6_matting_quality.rs` hash; that is expected and is why the scorer
re-derives the source and prior and compares them byte for byte rather than
trusting the pin.

## Verification

- `core/tests/assisted_scoring.rs` — the two published SHA-256 vectors; the
  committed `tests/fixtures/assisted/drive-synthetic-replay.json` (the phase-1
  drive's own replay, app-written, anonymous, no pixels) parsing and all
  fourteen per-event plane hashes reproduced, including event 12's
  `rejected:rasterizer_limit` leaving the plane hash equal to event 11's, and
  refused as synthetic but accepted under `allow_synthetic`; a hand-built human
  replay accepted for each of the three outcomes and refused with the named
  reason for in-progress, synthetic, self-declared unscoreable, missing
  attestation, `case_id` / `workspace_id` / dimension mismatch, each of the four
  displayed-asset hashes, the task card and a null label plane; `solver_inputs`
  over a fabricated tempdir workspace returning exactly the three relative
  paths, `Missing` without a replay, `Malformed` for bad JSON, and refusing an
  RGBA source, a wrong-size prior and a tampered hash; the assisted policy on
  the 9x1 contract fixture, including the `known categorical` refusal when a
  candidate overwrites a human label; and the refusal record's shape.
- `scripts/test_phase9i_assisted_scoring.py` — the pins all exist, the widened
  audit accepts a manifest replay and refuses a stray `replays/notes.txt`, an
  unknown-anon replay and a case id inside a replay `detail` while not tripping
  on the `alpha_write` KEY, `replay_manifest`'s `None`, and the five verdict
  cases (dry run, subset override with 34 missing, complete passing, complete
  failing, and a replay's self-reported exposure listed with no mapping flag).
- The dry run above, end to end, through the unchanged re-scorer.

## What a real run would mean

A REAL run — a human annotator, all 36 completion-subset cases, no subset
override, `--dry-run` absent — that exits 0 records the assisted capability per
[the execution plan](phase-9i-next-steps.md) row 2 and gate v2 decision 4, and
is the evidence 9I's completion criterion asks for. A real run that exits 1
records that the assisted protocol does not pass gate v2 on the declared subset,
which is an equally reportable outcome and closes nothing.

**This run produces neither.** It is a dry run over two cases with a synthetic
replay. 9I and P7-12 stay open.
