# Phase 9I cutout quality candidates and native cutout foundation (ADR)

- **Status:** Accepted — closed (no candidate qualifies; gate rescoped by [gate v2](phase-9i-gate-v2-status.md); P7-12's other half shipped 2026-10-11 without it)
- **Phase / dates:** 9I, 2026-09-12 → 2026-09-15
- **Original:** [archive/phase-9i-cutout-status.md](../archive/phase-9i-cutout-status.md) — per-candidate numbers, failure counts, receipts, reproduction commands
- **Key commits:** `d5ee109` (native foundation integrated over 9G `f457cdb` / 9H `93a45cf`), `2e26401` (V5 + execution continuation), `d5c7490` (gate v2 rescoping)

## Context
Phase 6 left edge-refinement brushing and color decontamination behind an unmet alpha/color quality gate
([phase-6-matting-reference-protocol.md](phase-6-matting-reference-protocol.md), 90 cases: 24 analytic, 24
photographic crops, 42 full images). 9I tried to pass it with predeclared candidates while building the typed
output foundation a qualified estimator would need. Five candidates each improved something; none passed the
unchanged combined gate, and all measurements stand under both gates.

## Decisions
- **Input seam:** `core/src/matting_input.rs` defines a version-1 input: automatic prior bytes are **uncertain**
  (including 0 and 255); only caller-supplied categorical user labels are exact constraints; Unknown (128) releases
  a constraint. Unknown versions, malformed labels, nonfinite weights refuse. No automatic threshold/erosion is
  ever reinterpreted as a user stroke.
- **Soft prior:** `closed_form_matting::estimate_soft_prior` adds `λ·Σ(α − prior)²` to the same color-line
  Laplacian as the categorical closed form; both share stencils, PCG, true-residual confirmation, cancellation and
  refusal. The prior term removes the nullspace — numerical solvability, not evidence of opacity.
- **Evaluator input policy field:** old candidates keep their mode; soft-prior candidates get all-Unknown user labels
  plus the unchanged degraded prior and never read case names or references. Saved `trimap` PNGs are legacy
  diagnostics, not the soft candidate's constraints.
- **Predeclaration discipline:** each candidate's declaration and executable/source/manifest hashes were written to
  `predeclared.json` before scoring; no in-run parameter changes; every case and refusal retained. Declarations:
  [V1](phase-9i-soft-prior-v1.json), [V2](phase-9i-soft-prior-v2.json), [V3](phase-9i-soft-prior-v3.json),
  [V4](phase-9i-soft-prior-v4.json), [V5](phase-9i-bayesian-v5.json).
- **Candidates:**
  - V1 global soft prior (`λ=0.01`, ≤ 500 PCG iterations, RMS tol 1e-6) — the 500,000-unknown / 20-billion-term caps refuse every photographic case.
  - V2 tiled soft prior (`core/src/tiled_soft_matting.rs`): 256-px output tiles, 16-px halo, `λ=1`, native resolution,
    each centre copied once (no overlap blending), halo repeats counted against the 20-billion-term allowance, one
    unconverged tile refuses the image. Admits all 90 cases. This kernel is the one later frozen for assisted scoring.
  - V3 source-only RGB protection (`core/src/foreground_protection.rs`): correct color only where alpha 1–127, prior < 128
    and label Unknown; protect when nearby observed foreground (radius 16, ≤ 8 codes) exists; 250M-check cap. Cuts
    opaque damage but raises contamination failures.
  - V4 stable prior: keeps the prior exactly (except user labels) + V3 color — isolates missing-input and color
    limits from solver regression.
  - V5 joint Bayesian alpha/color — see [phase-9i-bayesian-v5-status.md](phase-9i-bayesian-v5-status.md).
- **Native output foundation:** `core/src/precision_cutout.rs` / `ffi_precision_cutout.rs`
  (`dr_precision_doc_refinement_cutout_plan`, `dr_precision_doc_add_refinement_cutout`) follow the byte cutout's
  top-root insertion and original-root hiding. They share the supplied RGBA16 foreground (including alpha 1, partial
  alpha, hidden RGB) and copy an independent canvas-sized u16 refinement matte into a normal editable mask — never
  flattening the alpha product. Plans account retained storage, the borrowed matte and scratch; equal-byte profiles
  rebind to the captured owner; mismatched profile/extent/storage/budget refuses; empty coverage is a no-op; old
  roots survive hidden. **No native format bump** (native v25 already represents it).
- **Swift side:** `NativeRefinementCutout.swift` reuses the byte cutout's captured document/selection/trimap guards,
  separately admits assembly and display preparation, checks revision/session/cancel/closed-owner before
  publishing through the central one-edit Undo path; accepts `PrecisionScalarBuffer` with its registered lease.
  Execution details: [phase-9i-execution-status.md](phase-9i-execution-status.md).
- **No candidate was promoted** into typed refinement, UI, MCP or Actions. Edge brushing and decontamination ship
  in Select and Mask since 2026-10-11, without the gate ([typed cutouts](phase-9i-typed-cutouts-status.md)).

## Rejected alternatives
- Another protection/alpha threshold — V3/V4 measurements show missing input detail and residual foreground evidence, not a tunable cutoff.
- Repairing inputs from reference alpha/foreground, or reconstructing a "captured background" from references — contaminates inference.
- Transparent-460, alphamatting.com, BackgroundMattingV2 as transmission evidence — permission unestablished / needs a real empty-background capture; bottle stays silhouette-only.

## Consequences
- The foundation backs Select and Mask's typed New Layer outputs since 2026-10-11
  ([typed cutouts](phase-9i-typed-cutouts-status.md)); the V1–V5 estimators were never qualified.
- The legacy `oracle_alpha_color` rows measure the old *unprotected* estimator with reference alpha; they cannot
  qualify protected V3/V4 or any new pipeline.
- Canonical report and corpus locations are `~/devel/doctored-fixtures/` (gate v2 decision 6).

## Open items (verified 2026-09-26)
- `DROPPED` (2026-10-30) — Recover missing input detail via a genuinely blinded user-constraint workflow (the gate v2 assisted run): the gate is no longer a ship condition ([gate v2](phase-9i-gate-v2-status.md)).
- `DONE` — typed estimator and Select and Mask UI/MCP/Actions outputs at 8 and 16 bits shipped 2026-10-11 without the gate ([typed cutouts](phase-9i-typed-cutouts-status.md)).
- `DROPPED` — glass/transmission research ([gate v2](phase-9i-gate-v2-status.md)).
- `DROPPED` — Better automatic source-only foreground reconstruction (automatic line stopped; [gate v2](phase-9i-gate-v2-status.md) decisions 4–5).
- `DROPPED` — Measuring a protected reference-alpha variant (automatic line stopped).
- `DONE` — Basic typed refinement into the u16 mask ([phase-9i-typed-cutouts-status.md](phase-9i-typed-cutouts-status.md), `b2e0c80`).
