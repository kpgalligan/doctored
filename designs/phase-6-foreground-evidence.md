# Foreground evidence and opacity ambiguity (ADR)

- **Status:** Accepted — closed (diagnostic; its conclusion feeds the gate v2 opaque rule)
- **Phase / dates:** 6C, 2026-09-10 (reproduction paths updated 2026-09-15)
- **Original:** [archive/phase-6-foreground-evidence.md](../archive/phase-6-foreground-evidence.md) — full history, measurements, receipts
- **Key commits:** 0155d41, d5c7490

## Context
The multilevel foreground estimator (Germer et al., *Fast Multi-Level Foreground Estimation*) is good when supplied
alpha is accurate, but every estimated-alpha pipeline changed RGB at reference-opaque pixels. The question was whether
a color-only correction could certify which pixels are truly opaque. It cannot: for a gray observed pixel between a
black background and white foreground, an opaque gray mark (`α=1, F=C`) and translucent white (`α≈0.5, F=1, B=0`)
yield identical RGB, prior and valid trimap. No deterministic operation on those inputs can satisfy both.

## Decisions
- **Record the obstruction as regressions**, not as a relaxed gate: `../core/tests/foreground_ambiguity.rs`
  (`identical_composites_and_valid_constraints_admit_conflicting_foreground_truths`,
  `foreground_color_stage_with_estimated_alpha_does_not_certify_opaque_preservation`,
  `exact_reconstruction_and_gamut_feasibility_do_not_identify_foreground_color`). The sampled candidate reports
  maximum confidence at the ambiguous pixel; projecting `(F,B)` to exact compositing consistency can *increase*
  foreground error even with correct alpha. *Why:* a small residual or perfect reconstruction is not certainty.
- **Feasibility projection is a diagnostic hypothesis, not a core operation.** Per channel, with observed byte `C` and
  alpha byte `A>0`, feasible foreground is
  `[max(0, ceil(255(C−0.5−(255−A))/A)), min(255, floor(255(C+0.5)/A))]` (unconstrained at `A=0`, identity at `A=255`).
  Clamping estimated foreground into it changed essentially nothing (analytic 11,502 → 11,502 changed opaque pixels;
  photo 20,699 → 20,669) — the range is too broad. **Not integrated as a correction option.**
- The diagnostic (`../scripts/phase6_foreground_evidence.py`, tested by `../scripts/test_phase6_foreground_evidence.py`)
  reads only source RGB, estimated alpha and estimated foreground; reference alpha is used afterwards to count damage.
  It never overwrites metrics, images or gates.
- **Rules that still bind any correction:** a user's Unknown brush authorizes estimation, it does not prove
  translucency; any correction must pass the opaque, alpha, foreground and replacement-background checks; no
  truth-derived mask may be supplied to a candidate.

## Rejected alternatives
- Feasibility clamp as a shipped "decontaminate" option — measured no improvement.
- Protecting only alpha ≈ 255 — ambiguous analytic cases contain truly opaque pixels with estimated alpha < 128.

## Consequences
- This analysis is the basis for gate v2 decision 2 (bounded opaque rule: ≤ 2 code levels, ≤ 1% of opaque pixels, all
  inside the boundary set) — see [phase-9i-gate-v2-status.md](phase-9i-gate-v2-status.md).
- Better results need more evidence (reliable user constraints, captured background, or learned priors), not a
  cleverer color-only step.

## Open items (verified 2026-09-26)
- `DONE` — Learned-prior direction (ViTMatte) was tried as an offline spike ([phase-6-vitmatte-spike.md](phase-6-vitmatte-spike.md)).
- `DROPPED` — Integrating ViTMatte or BackgroundMattingV2: gate v2 decision 5 adds no neural matting model; Apple
  Vision stays the automatic prior ([archive/phase-9i-gate-v2-status.md](../archive/phase-9i-gate-v2-status.md)).
- `DROPPED` (2026-10-30) — Reliable constraints / explicit correction footprint to fix coupled color: the gate is not
  a ship condition (2026-10-10) and gate v2's assisted run is dropped.
