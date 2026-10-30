# Trimap sampling alpha candidate (ADR)

- **Status:** Accepted — open items remain (experimental kernel, failed its gate, not exposed)
- **Phase / dates:** 6C, 2026-09-09
- **Original:** [archive/phase-6-sampled-matting-candidate.md](../archive/phase-6-sampled-matting-candidate.md) — full history, measurements, receipts
- **Key commits:** 653309d

## Context
The guided-filter candidate failed the 24-case analytic gate (12/24, [phase-6-selection-quality.md](phase-6-selection-quality.md)).
This adds a trimap-driven color-sampling alpha solver as a replacement candidate, without weakening that gate or
changing its corpus, degradations or thresholds. It has no editor or MCP exposure.

## Decisions
- **Model:** solve `C = αF + (1−α)B` by projecting `C−B` onto `F−B`; rank pairs by reconstruction residual plus
  normalized spatial distance (He et al., *A Global Sampling Method for Alpha Matting*, eqs. 2–5) — with a bounded
  search, not the paper's SampleMatch. Confidence is a separation/residual diagnostic (after Wang & Cohen), **not a
  calibrated probability**.
- **Entry point:** `sampled_matting::estimate` (`../core/src/sampled_matting.rs`): finite RGB in `0..=1` in a
  caller-chosen domain (no implicit sRGB), dimensions, categorical trimap (`0`/`128`/`255` exactly), prior coverage,
  params, optional single-use job control. Inputs immutable.
- **Output:** alpha, FG/BG sample colors, confidence, sample/unresolved counts. At known pixels the observed color is
  kept for the known class and the other class is zero — **not** recovered color evidence.
- **Unresolved / refusal:** unknown pixels without an adequately separated, well-fitting pair keep their prior byte with
  zero confidence. Missing foreground **or** background boundary samples refuse. *Why:* a wholly translucent object's
  rim must not silently become a trusted opaque sample.
- **Search:** sample sets = known pixels adjacent to unknown; median-split spatial tree gives exact 8 nearest per class,
  plus up to 8 farthest-color global samples per class → ≤ 256 pairs per unknown pixel. Not a second PatchMatch.
- **Frozen defaults (set before measurement):** min separation `12/255`, max residual `8/255`, spatial weight `1/255`.
  No per-subject thresholds; no ground truth in the kernel.
- **Bounds:** `MAX_UNKNOWN_PIXELS = 1_000_000`, `MAX_BOUNDARY_SAMPLES = 250_000` (refuse, never downscale). Reservation
  `26N + 5,000,000 + 65,536` bytes excluding borrowed inputs. Polls cancellation during tree/palette build, every 64
  pixels / row in search, and every 4,096 visited tree nodes (degenerate boundaries are not assumed logarithmic).
- **Automatic trimap for qualification:** derived only from the degraded prior (≤ 8 BG, ≥ 247 FG, eroded 3 px); frozen
  for comparison, not asserted to be correct. No alpha truth repairs it.

## Consequences
- Result: alpha 10/24 analytic (all 6 glass refused), 8/24 photo crops; **0 joint alpha/color passes**. Ambiguous cases
  regress badly with zero unresolved pixels (hard-product-ambiguous-binary boundary MAD 0.0017 → 0.2024) — confidence
  must not be treated as authority to change pixels.
- Estimated alpha turns some reference-opaque pixels fractional, so the color stage edits them: a coupled failure even
  when color estimation with correct alpha works.
- Output is full-canvas; a brush-local unknown region still pays full-canvas cost.
- Tests: `../core/tests/sampled_matting.rs` (projections at α 0.03–0.97, pinned constraints, missing evidence,
  indistinguishable colors, malformed input, resource refusal, live cancellation < 500 ms).

## Open items (verified 2026-09-26)
- `DONE` — Competing-pair / ambiguity counterexample added: `../core/tests/foreground_ambiguity.rs`
  (see [phase-6-foreground-evidence.md](phase-6-foreground-evidence.md)).
- `TRACKED` — Stronger ambiguity model and better trimap/color evidence; rerun alpha+color gates → carryover P7-12 /
  9I gate v2 assisted protocol.
- `DROPPED` — Regional (brush-local) output for this kernel: 9I's declared assisted kernel is instead
  `assisted_tiled_soft_prior_v1` (`../core/src/tiled_soft_matting.rs`), built on the closed-form objective.
- `DROPPED` — Glass references/transmission: gate v2 decision 1 ([archive/phase-9i-gate-v2-status.md](../archive/phase-9i-gate-v2-status.md)).
