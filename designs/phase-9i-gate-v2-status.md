# Phase 9I cutout gate v2 (ADR)

- **Status:** Accepted — open items remain (the gate itself is frozen; no candidate passes it and the assisted run has not happened)
- **Phase / dates:** 9I, frozen 2026-09-15
- **Original:** [archive/phase-9i-gate-v2-status.md](../archive/phase-9i-gate-v2-status.md) — full history, measurements, receipts
- **Key commits:** `d5c7490` (rescoring milestone, re-scored at head `b2e0c80`)

## Context
Five predeclared automatic cutout candidates (V1–V5, see [phase-9i-cutout-status.md](phase-9i-cutout-status.md))
all failed the original Phase 6 combined alpha/color gate ("gate v1": every one of 90 cases). Across V1–V5
only 8 distinct analytic/crop cases ever passed end to end. Analysis showed parts of the corpus carry no
recoverable evidence, and that the bit-exact opaque-RGB clause measures alpha rounding rather than visible
damage. The owner made six decisions to rescope the gate **once**, without moving any threshold, and to
make 9I completion an assisted (brush-guided) result.

## Decisions
Gate v2 keeps every gate-v1 threshold (the "Independent color measurements" section of
[phase-6-matting-reference-protocol.md](phase-6-matting-reference-protocol.md)). It changes only the
analytic case scope and the opaque clause. Case ids come from `SUBJECTS` and the `composite` arms in
`core/tests/common/selection_quality.rs`.

1. **Analytic pass rule — exactly 12 cases:** `{portrait_hair, fur, hard_product}-{light, dark}-{blurred, binary}`.
   *Why:* the rest carry no recoverable evidence (below).
2. **Ambiguous diagnostic — 6 cases, reported, not gating:** `{portrait_hair, fur, hard_product}-ambiguous-{blurred, binary}`.
   *Why:* the fixture's ambiguous background differs from the foreground only in R by ≤ 3.82 codes (G/B
   identical; `selection_quality.rs:88-100`); on 11 strand rows |ΔR| < 0.5 code, so alpha is undefined by
   the pixels. Recovering it would need deconvolving the fixture's own radius-3 Gaussian — fixture
   exploitation, forbidden as truth-derived input.
3. **Glass transmission-research track — 6 cases, reported, not gating:** `translucent_glass-{light, dark, ambiguous}-{blurred, binary}`
   (glass beats the ambiguous rule). *Why:* the binary glass prior has recall 0.0916 vs 0.95 and there is no
   licensed photographic transmission evidence.
4. **Photographic corpus unchanged:** all 24 crops and all 42 full images stay pass-rule cases in the automatic report.
5. **Bounded opaque rule** replaces v1's `opaque_changed_pixels == 0`. At reference-alpha-255 pixels a pixel
   is *changed* if any channel of `round(estimated*255)` differs from `<case>-source.png`
   (`core/tests/common/matting_quality.rs`). The clause passes iff **(a)** max per-channel |delta| over changed
   opaque pixels ≤ **2 codes**; **(b)** `opaque_changed_pixels / opaque_pixels` ≤ **0.01**; **(c)** every
   changed opaque pixel is in the existing Rust `boundary` set (some pixel within Chebyshev distance ≤ 2 has
   `truth-alpha != 255`). *Why (c) is the boundary set, not "near a fractional pixel":* `hard_product` has an
   exact 0/255 outline except its hole, so a fractional-only neighbourhood would hold it to v1-exact by
   construction. The owner's 1% stands; a 2% relaxation was considered and withdrawn.
6. **Everything else identical to v1:** boundary-alpha MAD non-regression ≤ 1/255, soft-detail recall ≥ 0.95,
   alpha-weighted foreground MAD non-regression ≤ 1/255, fractional foreground MAD ≤ 0.05 with ≥ 20% reduction
   when initial > 0.02, replacement boundary MAD ≤ 0.02 on white, dark and checker, `has_fractional_reference`
   required, refusals fail.

- **Measurement precedence:** (1) record fields `opaque_changed_max_code_delta` / `opaque_changed_outside_boundary`;
  (2) decoded exported planes; (3) `opaque_changed_pixels == 0` ⇒ pass; (4) otherwise **indeterminate**, never a pass.
- **Completion criterion (decision 4):** 9I completes when the **assisted protocol passes gate v2 on a frozen
  36-case subset**; the automatic path is reported, never gating. Subset: all 24 `photo-*` crops, plus
  `full-<image>-checker-{binary,blurred}` for each of `fur-longhair`, `hard-arduboy`,
  `portrait-0051115Q_000001_{0003,0031,0060,0088}` (the two lexicographically first ids per image). The bottle is
  excluded. Serialized as `ASSISTED_COMPLETION_SUBSET` in [`scripts/phase9i_gate_v2_rescore.py`](../scripts/phase9i_gate_v2_rescore.py).
- **Annotator clause:** the owner annotates. Before the first annotation the owner lists every case already inspected
  (source, reference, panel or scores); those are annotated unblinded, flagged `annotator_exposed: true`, and
  stay in the denominator.
- **Decision 5 — RVM dropped; no neural matting model is added; Apple Vision stays the automatic prior.**
  *Why:* its weight-license issue #281 is unanswered, its GPL-3.0 code cannot link into all-rights-reserved
  Doctored, and it is trimap-free human-video work that cannot consume brush guidance. Closes work package 1
  of [phase-9i-next-steps.md](phase-9i-next-steps.md) with no pipeline selected.
- **Decision 6 — persistent storage:** canonical corpus is `~/devel/doctored-fixtures/phase6/{references,photos,full-references}`;
  preserved reports are `~/devel/doctored-fixtures/phase9i-reports/` (V1–V5 runs, V5 diagnostics, both feasibility
  inventories, `gate-v2/rescore.{json,md}`). *Why:* macOS purged `/private/tmp` and emptied the corpus on
  2026-09-14/15. Any `/private/tmp/rz-*` path cited by older docs is a historical record, not a file.
- **Test-support metric:** `color_metrics` in `core/tests/common/matting_quality.rs` additionally emits
  `opaque_changed_max_code_delta` and `opaque_changed_outside_boundary`; report `schema_version` stays 1;
  `color_gate`, `color_gate_contract()` and `passes_end_to_end_gate` (still v1) are untouched. Pinned by two
  assertions in `core/tests/matting_qualification.rs`. Only the Python runners' pass decision moved; `--gate v1`
  reproduces the old decision exactly.
- **Re-scorer:** `scripts/phase9i_gate_v2_rescore.py` (stdlib, no pixel cap, no rescaling) asserts its recomputed
  v1 verdict equals every Rust-recorded one; refuses an output path inside the repository.

Re-score of the preserved candidates (v1 passes / v2 passes; v2 counts pass-rule cases only):

| candidate | analytic | crops | full |
| --- | --- | --- | --- |
| V1 | 1 / 3 | 0 / 0 | 0 / 0 |
| V2 | 1 / 3 | 0 / 0 (+6 indeterminate) | 0 / 0 (+3 indeterminate) |
| V3 | 1 / 1 | 3 / 3 | 4 / 4 |
| V4 | 0 / 0 | 3 / 3 | 4 / 4 |
| V5 | 2 / 2 | 0 / 0 | 0 / 0 |

No candidate passes gate v2 on any corpus; the union of analytic pass-rule passes is 6/12.

## Rejected alternatives
- 2% changed-fraction bound — motivated by V5 `portrait_hair-light-binary`, which fails clause (c) anyway (249 of 350 changed pixels are interior).
- "Near a fractional pixel" for clause (c) — would make `hard_product` v1-exact by construction.
- Another automatic candidate or tuning V5 — the recorded stop rule ("same interior damage / preservation-vs-decontamination tradeoff → stop that line") already applied.
- Deconvolving the known fixture blur for ambiguous cases — fixture-recipe exploitation.

## Consequences
- Supersedes: "preserve opaque source RGB exactly" in the Phase 6 matting protocol (for 9I completion); "the
  original quality criteria" in P7-12 and plan 9I now means gate v2 with the assisted criterion; RVM's conditional
  selection in [phase-9i-feasibility-status.md](phase-9i-feasibility-status.md) and
  [phase-9i-established-pipeline-decision.md](phase-9i-established-pipeline-decision.md); the completion decision and
  opaque bullet of [phase-9i-assisted-protocol-v1.md](phase-9i-assisted-protocol-v1.md). Historical numbers stand.
- A further rescoping would be a new, explicit gate version; do not edit v2's constants.
- New runs from the rebuilt evaluators are fully v2-decidable; preserved records stay partly indeterminate forever
  (nine V2 cases), and cannot be resolved without re-running.
- Re-running any purged check needs a new output directory under `~/devel/doctored-fixtures/` and a new receipt.

## Open items (verified 2026-09-26)
- `TRACKED` — Run the assisted protocol over the 36-case subset (human annotation session, then scoring) → carryover P7-12 ("deferred at the owner's discretion"), plan §9I "Next assignment"; steps in [phase-9i-assisted-annotation-runbook.md](phase-9i-assisted-annotation-runbook.md). No `assisted-run-*` directory exists under `~/devel/doctored-fixtures/phase9i-reports/`.
- `DROPPED` — Glass/transmission research: out of scope (dropped 2026-09-28); the six analytic glass cases stay reported diagnostics under frozen gate v2 and the bottle stays silhouette-only.
- `DONE` — Corpus re-acquired and verified under `~/devel/doctored-fixtures/phase6/` (present on disk).
- `DONE` — Assisted workspace/replay tool and scorer built (`de70228`, `5f75604`).
- `DROPPED` — RVM / established neural pipeline comparison (decision 5, this doc).
- `DROPPED` — Resolving the nine indeterminate V2 verdicts (would require re-running a stopped candidate line).
