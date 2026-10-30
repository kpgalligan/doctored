# Phase 9I execution plan after the automatic candidates failed (ADR)

- **Status:** Accepted — open items remain (work package 1 closed by [gate v2](phase-9i-gate-v2-status.md); the assisted run is pending)
- **Phase / dates:** 9I, 2026-09-13 (after `2e26401`); recorded outcome 2026-09-15
- **Original:** [archive/phase-9i-next-steps.md](../archive/phase-9i-next-steps.md) — failure analysis table, subagent plan, resume instructions
- **Key commits:** `19b7928` (plan), `195efe7` (execution update), `d5c7490` (recorded outcome)

## Context
Five predeclared candidates had failed the original combined alpha/color gate (V5: 2/90). Failures were
structural — unsuitable color populations, full-image search cost, RGB/prior evidence that cannot distinguish an
opaque gray mark from translucent white ([phase-6-foreground-evidence.md](phase-6-foreground-evidence.md)), and no
transmission truth. They justified changing the approach, not concluding matting is unachievable, and not another
round of tuned variants.

## Decisions
- **Product contract to target:** *assisted refinement of an existing selection or mask* for portraits, hair, fur and
  product edges; output stays reversible as foreground pixels plus a separate editable refinement mask. Independent
  concepts: automatic prior (uncertain), FG/BG constraints (exact, provenance recorded), Unknown (releases, does not
  assert translucency), alpha-refinement region, separate color-correction region, protected interior (exact;
  contradictions refuse), analysis region (read ≠ write). For typed sources, source alpha and refinement coverage
  stay separate; FG never forces stored source alpha to 65535. Before/After shows the captured original and the
  exact candidate; Apply publishes that candidate without recomputing. Formalized in [phase-9i-assisted-protocol-v1.md](phase-9i-assisted-protocol-v1.md).
- **Two protocols, never merged:** the original automatic 90-case benchmark is preserved intact (no truth-derived
  inputs, no resizing, no per-case algorithm selection, refusals fail); the assisted protocol gets its own identity,
  declaration, reports and completion claim. An assisted pass cannot silently satisfy the automatic gate.
- **Work packages:** (1) contract + one established pipeline + evidence audit; (2) one bounded, frozen comparison;
  (3) record a decision from the table and stop. Decision table: full qualification → typed estimator then public
  workflow; assisted-only pass → record a distinct capability, explicit scope change; partial improvement → record
  failures and one next hypothesis, no controls; same interior damage / preservation-vs-decontamination tradeoff /
  resource refusal → **stop that algorithm line**; missing inputs/rights/evidence → record the dependency.
- **Recorded outcome (2026-09-15):** rows 4 and 5 applied. The automatic line is stopped, RVM is dropped, no neural
  model is added, Apple Vision stays the automatic prior, and the gate is rescoped once as
  [gate v2](phase-9i-gate-v2-status.md): completion is the assisted protocol passing gate v2 on a 36-case subset.
- **Post-score correction rule:** numerical defects may be fixed and tested, but a corrected scored pipeline gets a new
  identity and the old run is retained.
- **Glass/transmission is a separate research track:** silhouette selection, scalar transmission and refractive
  recomposition are different claims; RGBA cannot reproduce refraction; the bottle is silhouette-only evidence; a real
  independently captured empty background is an option only as a separate workflow. Transmission research itself is declined.
- **After an applicable gate qualifies**, integrate as separate slices: typed estimation (retain source RGB/alpha,
  profiles, hidden colors; disclose analysis precision); captured refinement workspace (Before/After, exact
  Apply/Cancel); UI/MCP/Actions parity with versioned replay and one Undo; reusable masks and persistence; final
  execution/compatibility using the existing compound controls, progress factory and admission.

## Rejected alternatives
- Another protection cutoff, larger search cap or V5 retune — does not address the failure causes.
- Treating a "continue" as permission to tune V5, enable advanced controls or restart completed 9A–9H slices.

## Consequences
- Advanced controls stay disabled until quality *and* integration gates pass.
- Any custom V5 successor must address interior sampling, multiple color populations, uncertainty and bounded full-size queries.
- Phase 10 proceeds independently and never closes 9I.

## Open items (verified 2026-09-26)
- `TRACKED` — Run the assisted protocol (human session + scoring) → P7-12 / plan §9I "Next assignment"; [runbook](phase-9i-assisted-annotation-runbook.md).
- `TRACKED` — Typed estimator + captured workspace + UI/MCP/Actions + persistence slices after a pass → plan §9I / P7-12.
- `DROPPED` — Glass/transmission research: out of scope (closed 2026-09-28, not planned); the six analytic glass cases stay reported diagnostics under frozen gate v2 and the bottle stays silhouette-only.
- `DONE` — Work package 1 deliverables: [protocol](phase-9i-assisted-protocol-v1.md), [pipeline decision](phase-9i-established-pipeline-decision.md), [evidence audit](phase-9i-feasibility-evidence-audit.md) (`195efe7`).
- `DROPPED` — Work package 2's automatic established-pipeline comparison (gate v2 decision 5).
- `DONE` — Independent Phase 10 work (10A closed; Phase 10 leftovers closed in `fa336cf`).
