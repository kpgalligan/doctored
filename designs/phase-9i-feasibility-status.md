# Phase 9I feasibility prerequisite decision (ADR)

- **Status:** Superseded by [phase-9i-gate-v2-status.md](phase-9i-gate-v2-status.md) (work package 1 closed with no pipeline selected)
- **Phase / dates:** 9I work package 1, 2026-09-13; closed 2026-09-15
- **Original:** [archive/phase-9i-feasibility-status.md](../archive/phase-9i-feasibility-status.md) — dependency table, review receipts, Phase 10 progress notes
- **Key commits:** `195efe7`, `d5c7490`

## Context
Work package 1 of [phase-9i-next-steps.md](phase-9i-next-steps.md) was a prerequisite review before any further
scored candidate: define assisted refinement, pick one established pipeline, and audit the evidence. Its three
products are the [assisted protocol](phase-9i-assisted-protocol-v1.md), the
[pipeline decision](phase-9i-established-pipeline-decision.md) and the
[evidence audit](phase-9i-feasibility-evidence-audit.md).

## Decisions
- **Closing decision (2026-09-15): no pipeline selected; RVM dropped; no neural matting model is added; Apple Vision
  stays the automatic prior** (reasons in [gate v2](phase-9i-gate-v2-status.md) decision 5).
- At the time, the plan's **"missing inputs, rights or reference evidence"** branch applied: stop at the
  prerequisite decision. *Do not download weights, run an undeclared model, tune V5 again, enable controls or mark
  the original quality gate complete to fill the gap.* This milestone added no model and began no new custom variants.
- Declaration files distinguish fixed protocol choices from missing acquisition/input/replay hashes; **null hashes
  are readiness blockers, never a frozen run** ([phase-9i-assisted-v1.template.json](phase-9i-assisted-v1.template.json)).
- Preserved V3/V4 reports remain global historical controls, but their recorded executable path has since been
  rebuilt: a same-named current binary must never be represented as the measured one.
- The automatic comparison and the assisted scoring are independent: neither waits on the other, both retain every
  declared case and refusal and independently recompute their gates.

## Consequences
- Future work must not repeat the three-candidate survey automatically.
- Phase 10 proceeded independently; starting or finishing it never closes 9I.

## Open items (verified 2026-09-26)
- `DROPPED` — Exact RVM checkpoint terms, runtime/input adapter, and the one automatic comparison (gate v2 decision 5).
- `DONE` — Source-only workspace, replay tool and frozen geometry policy (`de70228`, [phase-9i-assisted-workspace-status.md](phase-9i-assisted-workspace-status.md)).
- `DROPPED` (2026-10-30) — Actual blinded human sessions under v1: the gate is no longer a ship condition ([gate v2](phase-9i-gate-v2-status.md)).
- `DROPPED` — glass/transmission research ([gate v2](phase-9i-gate-v2-status.md)).
