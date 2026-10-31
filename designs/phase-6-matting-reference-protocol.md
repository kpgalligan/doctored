# Matting reference and color-gate protocol — gate v1 (ADR)

- **Status:** Accepted — closed (thresholds bind the research gate only; see [gate v2](phase-9i-gate-v2-status.md))
- **Phase / dates:** 6C, 2026-09-09 (gate v2 note added 2026-09-15)
- **Original:** [archive/phase-6-matting-reference-protocol.md](../archive/phase-6-matting-reference-protocol.md) — full history, measurements, receipts
- **Key commits:** 653309d, d5c7490

## Context
The original 24-case analytic gate ([phase-6-selection-quality.md](phase-6-selection-quality.md)) measured only alpha.
Advanced refinement also needs foreground-color decontamination, which needs independent foreground-color truth. This
protocol adds licensed photographic references and frozen color measurements, with rules that keep oracle-alpha color,
estimated-alpha color and end-to-end results as separate conclusions. It does not by itself qualify any editor feature.

## Decisions
- **Photographic references:** four PhotoMatte85 portraits (archive positions 0, 28, 56, 84; 2304×3456 RGBA8),
  selected before scoring. Publisher states commercial and noncommercial use; no survey/account/form submitted; files are
  never committed, bundled or used for training. Acquisition `../scripts/phase6_reference_fixtures.py` (tests:
  `../scripts/test_phase6_reference_fixtures.py`) reads only the public archive, refuses login/captcha/terms forms,
  preserves mismatching or partial files, and pins page JS, archive and member SHA-256s.
- **Crop:** fixed native crop `(x:512, y:384, 1280×1280)`, no rescaling; cropping precedes encoded-RGB compositing.
  Crops do not qualify full-frame runtime or accuracy (see [full-image protocol](phase-6-full-image-reference-protocol.md)).
- **Refused sources:** Adobe Composition-1k (noncommercial), alphamatting.com (permission unclear), withoutBG100
  (contact required).
- **Frozen alpha protocol:** original 24 analytic cases unchanged (4×4 area sampling, encoded compositing, radius-3
  blur, binary threshold). Alpha gate: boundary-MAD regression ≤ 1/255 and soft-detail recall ≥ 95%. Refusals fail.
- **Frozen automatic trimap:** from the initial mask only — bytes ≤ 8 background, ≥ 247 foreground, each known set
  eroded by a clipped 7×7 square, rest 128. Reads neither source colors nor reference alpha; **no oracle trimap repair**.
- **Photo cases:** 4 portraits × light/dark/checker × blurred/binary = 24, reported separately from the analytic 24.

### Independent color measurements
All arithmetic is **encoded RGB** (candidates are called with `color_space:"encoded"`); it makes no linear-light claim.
Reference foreground is the authored/supplied foreground, never the contaminated composite. Metrics: alpha-weighted
foreground RGB MAD (normalized by 3 × total reference alpha); fractional-only foreground MAD (0 < α < 1); white/dark/
checker replacement-background recomposition MAD and boundary MAD; changed source RGB at reference-opaque pixels;
mean false-positive alpha on reference background.

**Gate v1 color thresholds (frozen before measurement, unchanged in v2):** preserve opaque source RGB exactly;
alpha-weighted foreground MAD regression ≤ 1/255; fractional foreground MAD ≤ 0.05; where initial fractional error
> 0.02, reduce it ≥ 20%; replacement-background boundary MAD ≤ 0.02 on all three backgrounds. A reference with no
fractional pixels is diagnostic, not a color pass. Rust evaluators record v1 as `passes_end_to_end_gate`; `--gate v1`
still reproduces it.

- **Three separate conclusions:** oracle-alpha color (isolates the estimator); sampled/estimated-alpha color (coupled
  result); end-to-end (alpha gate + coupled color gate). A refused alpha cannot pass end-to-end; oracle-alpha success
  never substitutes for alpha qualification.
- **Runner:** `../scripts/phase6_matting_qualification.py` (new output dir outside the repo; verifies pins, records
  executable hashes, reruns the `phase6_selection_quality` analytic baseline (the guided 12/24 result until
  `guided_selection.rs` was deleted on 2026-10-11), writes `run.json`, `matting/metrics.json`,
  `failure-reasons.json`; `--require-pass` fails unless every case passes; `--gate v1|v2`). Diagnostics:
  `../scripts/phase6_matting_diagnostics.py`. Metric tests: `../core/tests/matting_qualification.rs`.

## Consequences
- First run (2026-09-09): sampled alpha 10/24 analytic (6 glass refusals), 8/24 photo; oracle color 20/24 and 22/24;
  coupled color and end-to-end **0/24** in both. Failure criteria overlap and must not be summed.
- Gate v2 keeps every threshold above, restricts the analytic pass rule to the 12 recoverable cases and replaces
  "exactly" with the bounded opaque rule; 9I completion = assisted protocol passing v2 on a 36-case subset.
- The `designs/phase-9i-*.json` manifests cite "unchanged phase-6-matting-reference-protocol" thresholds — this ADR
  keeps them verbatim.

## Open items (verified 2026-09-26)
- `DONE` — Fur and hard-product photographic references added by the
  [full-image protocol](phase-6-full-image-reference-protocol.md) (`../scripts/phase6_full_reference_fixtures.py`).
- `DROPPED` — Photographic glass/transmission reference: never obtained; glass left the pass rule (gate v2 decision 1,
  [archive/phase-9i-gate-v2-status.md](../archive/phase-9i-gate-v2-status.md)).
- `DROPPED` (2026-10-30) — Passing the combined alpha/color gate: the gate is not a ship condition (2026-10-10),
  gate v2's assisted run is dropped, and shipped Select and Mask uses `edge_refine.rs`, so this research gates nothing.
