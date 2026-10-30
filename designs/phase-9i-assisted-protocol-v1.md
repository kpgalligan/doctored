# Phase 9I assisted edge refinement protocol v1 (ADR)

- **Status:** Accepted — open items remain (tooling built; no human annotation collected or scored)
- **Phase / dates:** 9I, defined 2026-09-13; gate v2 amendment 2026-09-15; tooling pins 2026-09-15; scoring pins 2026-09-16
- **Original:** [archive/phase-9i-assisted-protocol-v1.md](../archive/phase-9i-assisted-protocol-v1.md) — full contract text, geometry and refusal wording; the hashed contract (the live `protocol_sha256` input)
- **Key commits:** `195efe7` (protocol), `d5c7490` (gate v2 amendment), `de70228` (tooling pins), `5f75604` (scoring pins)

## Context
Automatic candidates cannot recover edge detail that is not in the pixels
([phase-9i-gate-v2-status.md](phase-9i-gate-v2-status.md)). Protocol `phase9i-assisted-edges-v1` is a separate,
source-only human-guidance experiment: a user supplies categorical intent where the image is ambiguous, and a
frozen kernel refines under it. Assisted inputs change the available evidence, so it has its own identity,
declaration and reports, and can never be relabeled as the automatic benchmark's result. This page is the
scorer-facing contract; the annotator sees only the task card ([phase-9i-assisted-annotation-task-v1.md](phase-9i-assisted-annotation-task-v1.md)).

## Decisions
- **Population:** 78 ordinary-edge cases — 18 analytic (`portrait_hair`/`fur`/`hard_product` × light/dark/ambiguous ×
  blurred/binary, 320×256), 24 PhotoMatte crops (1280×1280 from crop `(512,384)`), 6 `fur-longhair` (763×764),
  6 `hard-arduboy` (944×1338), 24 full portraits (2304×3456). Listed with dimensions in
  [phase-9i-assisted-v1.template.json](phase-9i-assisted-v1.template.json). The 6 `translucent_glass-*` and 6
  `full-glass-bottle-*` cases are excluded before annotation (out of optical scope; bottle is silhouette-only).
  Changing the population requires a v2 protocol.
- **Gate v2 amendment:** 9I completion is decided on the **36-case subset** of gate v2; opaque preservation uses
  gate v2's bounded rule ((a) ≤ 2 codes, (b) ≤ 1%, (c) inside the Rust `boundary` set); the owner is the annotator
  under the exposure-disclosure clause. The 78-case population and 78/78 report text were kept unchanged.
- **Inputs:** `I` captured straight RGB8 (encoded-RGB domain), `S` source alpha (=255 everywhere in v1), `M0`
  byte prior; native extent, row 0 at top, original profiles, unchanged Phase 6 source/prior recipes
  ([phase-6-matting-reference-protocol.md](phase-6-matting-reference-protocol.md),
  [phase-6-full-image-reference-protocol.md](phase-6-full-image-reference-protocol.md)). No resize, linear-light
  substitution, profile conversion or prior replacement.
- **Output:** `(F, S, M)` — straight foreground RGB8, **unchanged** source alpha, separate editable matte M8;
  effective coverage `(S/255)(M/255)`. Never a flattened alpha product. FG means `M=full`, not `S=full`.
  Partially transparent sources, RGBA16/HDR/linear inference and u16 estimation are **unsupported and refuse** in v1.
- **Raw-output validation before any conversion:** exact extent/channels, finite, in `[0,1]`; malformed output
  refuses (no clipping first). Quantize once with `Q8(x)=floor(255x+0.5)`; protected/zero-write paths copy bytes
  directly. Score the stored bytes. A confidence field never authorizes writes.
- **Guidance planes (full canvas):** prior `M0` (uncertain, even at 0/255); labels `L` u8 `0=Unknown, 1=FG, 2=BG`;
  alpha-write `A` u16; RGB-write `C` u16 (independent of `A`); protection `P` binary; analysis `R` = full canvas.
  Initial: all Unknown, `A=C=65535`, `P=0`. No cutoff or automatic trimap ever becomes a human label.
- **Write equations** (`a=A/65535`, `c=C/65535`, both 0 where `P=1`; FG/BG replace `m_hat` by the endpoint):
  `M = Q8((1-a)·M0/255 + a·m_hat)`, `F = Q8((1-c)·I/255 + c·f_hat)` per channel, `S` = exact copy.
  Feathers are real fractional coverage.
- **Conflicts refuse, never pick a winner:** simultaneous FG+BG, invalid values/extents, missing provenance or
  unsupported precision; protected FG/BG whose `M0` is not already the endpoint; FG/BG with `A<65535` whose `M0`
  is not the endpoint. A conflicting gesture is rejected whole; never repaired by enlarging a write region.
- **Blinding:** fresh annotator, one attempt per case, never exposed to references, cutout originals, candidate
  outputs, scores or error maps; no agent that has read reference/score context may author strokes. The
  annotation directory holds only anonymous ids, source RGB, prior, dimensions/profile, task card and replay
  writer; the exporter/scorer may read references, the annotation process may not.
- **Interaction budget:** **300 s and 20 modifying gestures per case**, timer from first source/prior display;
  zoom/pan/overlay toggles cost time, not gestures; no pauses or budget transfer. Brush diameters 3/9/27/81;
  ≤ 256 points per stroke; hard binary footprints (pixel centres within `diameter/2` of the polyline). Zero
  candidate-preview rounds; no imported masks, auto-selection, prompts, model suggestions or reuse.
  Budget expiry keeps the last valid state (`budget_exhausted`); abandonment is `incomplete`, not dropped.
- **Tooling pins (2026-09-15):** replay format is [`core/src/assisted_replay.rs`](../core/src/assisted_replay.rs)
  (replay schema 1; its module doc is the schema), read only through `dr_assisted_replay_plane` /
  `dr_assisted_replay_summary`. The label plane is an all-128 base with the ACTIVE strokes applied in order through
  the unchanged `rz_trimap_strokes` (`radius = diameter/2`, points at `[x+0.5, y+0.5]`), so its 64M segment-box
  visit cap applies. Strokes are deduplicated, clamped, decimated to 256 points; **recorded points are what is
  rasterized and shown.** Over-time and rasterizer-refused strokes are recorded `rejected:time_budget` /
  `rejected:rasterizer_limit`, cost a gesture, leave the plane unchanged. The 20th gesture ends the case.
  **Collection is labels-only**: no protect, region, polygon or feather tools, so replays declare constant
  `A=C=65535`, `P=0`; the region clauses are specified but unexercised. `scoreable == (origin == "human")`; a
  human session needs `annotator_id` + `source_only` attestation and refuses agent strokes/finalize. Every
  outcome writes `<workspace>/replays/<case_id>.json` immediately, and its existence is the lock (a crash leaves
  the case unlocked — one attempt is then enforced administratively).
- **Scoring pins (2026-09-16):** single candidate `assisted_tiled_soft_prior_v1`
  ([phase-9i-assisted-scoring-v1.json](phase-9i-assisted-scoring-v1.json)) — unchanged V2
  `tiled_soft_matting::estimate` with `SoftPriorInput { version: 1, user_constraints: Some(labels), prior_weight: 1.0 }`,
  `ClosedFormParams::default()`, no protection, no tuning. Solver reads only `cases/<anon>/{source,prior}.png` and
  `replays/<anon>.json`; reference foreground is read for metrics only, after proving the workspace planes
  byte-equal the frozen recipe. `InputPolicy::AssistedHumanLabelsV1` makes the human label plane the hard
  constraint; the automatic trimap is recorded only as `legacy_automatic_constraint_audit`. Missing/malformed/
  unscoreable/hash-failed replays and harness-admission failures are scored refusals in the denominator. Exposed
  cases are scored and listed, never excluded. A RESULT requires a non-dry run, no subset override, every subset
  case present and `qualified` under the chosen gate. Details: [phase-9i-assisted-scoring-status.md](phase-9i-assisted-scoring-status.md).
- **Resources:** native extents; the full-image `256·N + 64 MiB` harness admission under 2 GiB; any new model must
  forecast its combined peak; no concurrent large jobs without a combined forecast.
- **Pass rule per case:** admitted output, valid bounded replay, resource compliance, every gate-v2 quality clause,
  plus contract checks (exact constraints, unchanged `S`, unchanged matte at `A=0`, RGB at `C=0`, exact `P`).
  Baseline is always the captured `(I, M0)`. Refusals, mistakes, unfinished and budget cases stay in the
  denominator; a budget-exhausted valid replay can pass, an incomplete one cannot.

## Rejected alternatives
- AI-suggested labels in v1 — a later version must record model identity, source-only inputs, the uncertain suggestion and an explicit user acceptance event.
- Preview-feedback rounds — a later protocol must declare its preview source and rounds; never truth/score feedback.
- Candidate-specific human correction — outside v1; all compared candidates use the same frozen guidance.

## Consequences
- A changed implementation, input policy or guidance collection is a new frozen experiment; post-score tuning cannot alter a result.
- Passing v1 establishes only the assisted ordinary-edge experiment on annotated references — not physical
  opacity, transmission, typed estimation or public UI/MCP/Actions workflows.
- `scripts/phase9i_assisted_workspace.py` and `scripts/phase9i_assisted_scoring.py` record the SHA-256 of the archived
  original (`archive/phase-9i-assisted-protocol-v1.md`) as `protocol_sha256`; editing *that* file changes what future
  runs pin; this ADR is not hashed.

## Open items (verified 2026-09-26)
- `TRACKED` — Collect the blinded human session and score it → P7-12 / plan §9I "Next assignment"; procedure in [phase-9i-assisted-annotation-runbook.md](phase-9i-assisted-annotation-runbook.md).
- `OPEN` — The 78-case population / 78/78 whole-run report cannot be produced: the exporter and scorer accept only the 36-case `ASSISTED_COMPLETION_SUBSET` (`scripts/phase9i_assisted_workspace.py` refuses other cases). Either extend the tooling or record that v1 reports only the 36-case subset.
- `DONE` — Source-only exporter, replay format/rasterizer and blinded annotation window (`de70228`; [phase-9i-assisted-workspace-status.md](phase-9i-assisted-workspace-status.md)).
- `DONE` — Scorer and frozen candidate (`5f75604`; [phase-9i-assisted-scoring-status.md](phase-9i-assisted-scoring-status.md)).
- `DROPPED` — Protection/write-region collection tools: v1 collection is labels-only by the tooling pin above.
