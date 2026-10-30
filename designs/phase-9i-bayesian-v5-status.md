# Phase 9I joint Bayesian candidate V5 (ADR)

- **Status:** Accepted — closed (candidate failed; kept as a diagnostic branch)
- **Phase / dates:** 9I, declaration frozen 2026-09-13; results recorded 2026-09-13 → 2026-09-15
- **Original:** [archive/phase-9i-bayesian-v5-status.md](../archive/phase-9i-bayesian-v5-status.md) — per-criterion failure counts, audits, pinned report hashes
- **Key commits:** `2e26401` (candidate + run), `d5c7490` (gate v2 re-score)

## Context
V1–V4 ([phase-9i-cutout-status.md](phase-9i-cutout-status.md)) estimated alpha, then foreground with alpha fixed,
so a wrong alpha is absorbed into color: `F_est − F_true = (α_true/α_est − 1)(F_true − B)`. V5 tested **joint**
alpha/foreground/background estimation instead of adjusting protection thresholds, under the unchanged original gate.

## Decisions
- **Model** (`core/src/bayesian_matting.rs`, `core/src/bayesian_color.rs`; declaration [phase-9i-bayesian-v5.json](phase-9i-bayesian-v5.json),
  SHA-256 `77eeceb6…c6e59f143`): Chuang et al. Gaussian compositing objective plus a finite Gaussian alpha prior
  (σ 0.5). Inputs are source RGB and the automatic prior only; caller labels 0/255 alone pin alpha.
  Automatic endpoints (≤ 8 / ≥ 247) give uncertain color support only ≥ 8 px inside their contour. Samples: eight
  nearest spatial plus ≤ 8 global palette colors; one inverse-distance-weighted full RGB covariance per class —
  an explicit simplification of mixture Bayesian matting. Observation variance is the byte quantization floor
  `1/(12·255²)`. Per-alpha F/B from a 3×3 Schur complement; bounded search of ≤ 21 objective evaluations; joint
  MAP, not global convergence. Output-alpha endpoints keep source RGB exactly.
- **Work bounds:** ≤ 200M joint objective evaluations, ≤ 2B spatial-tree node visits, existing 250,000-boundary-sample
  cap; scratch `64·N + 16 MiB` (+14N borrowed planes) inside the unchanged `256·N + 64 MiB` harness. Missing
  evidence or an exhausted budget refuses the whole candidate; unchanged-prior diagnostics never pass in its place.
- A pre-score review fixed a covariance mirror-rounding bug (one triangular entry computed and mirrored); a
  correlated-RGB regression test pins it. Not a threshold change.

**Result (preserved at `~/devel/doctored-fixtures/phase9i-reports/rz-phase9i-joint-bayesian-v5/`):**

| Corpus | Cases | Admitted | Alpha passes | Combined passes | Refusals |
| --- | ---: | ---: | ---: | ---: | ---: |
| Analytic | 24 | 18 | 12 | 2 | 6 |
| Crops | 24 | 24 | 4 | 0 | 0 |
| Full | 42 | 18 | 3 | 0 | 24 |

The two passes are light-background analytic fur. Six glass cases refuse (no foreground support); all 24 full
portraits exhaust the 2B spatial-node budget. On four light/blurred crops 149,855–259,322 opaque pixels change by up
to 122–225 codes — a structural mismatch: a boundary-only sample population applied to all pixels, and one Gaussian
per class lets interior color variation read as transparency. Under gate v2 V5 still passes 2 analytic, 0 crops, 0 full.

## Rejected alternatives
- Raising an alpha cutoff, lowering a residual or enlarging the search cap — none establishes reliable foreground evidence.

## Consequences
- Any custom V5 successor must address spatially representative interior samples, multiple color populations per
  class, explicit uncertainty and bounded full-size queries (recorded in [phase-9i-next-steps.md](phase-9i-next-steps.md)).
- Reproduction: `scripts/phase9i_soft_prior_qualification.py <new-dir> --gate v2 --bayesian --executable <phase9i_bayesian_quality> --crops … --full …` (output directory must be new).

## Open items (verified 2026-09-26)
- `DROPPED` — A successor experiment with interior/multi-mode color populations: the automatic algorithm line is stopped ([phase-9i-gate-v2-status.md](phase-9i-gate-v2-status.md) decisions 4–5; [phase-9i-next-steps.md](phase-9i-next-steps.md) recorded outcome).
