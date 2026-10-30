# Phase 6 — selection and mask workflows (ADR)

- **Status:** Accepted — open items remain (6A–6D shipped, Select and Mask included; the matting research gate is no longer a ship condition)
- **Phase / dates:** 6A–6D, 2026-09-09 – 2026-09-15 (9I status notes folded in)
- **Original:** [archive/phase-6-selection-mask-status.md](../archive/phase-6-selection-mask-status.md) — full history, measurements, receipts
- **Key commits:** 83de68c, 653309d, 0155d41

## Context
Phase 6 of the [implementation plan](professional-photo-editor-implementation-plan.md) gives a photographer reusable,
refinable, repositionable masks. Its completion gate asks for portrait, fur, translucent-object and hard-product
cutouts at 100%, reversible refinement, reusable masks, pixel-preserving mask transforms and native round trips of mask
properties. The plan explicitly allows shipping selection essentials, masks and the initial workspace without waiting for
matting research, so the functional slices shipped while the advanced cutout gate stayed open.

## Decisions
- **6A Selection essentials (shipped):** freehand lasso, sampled/tonal Color Range (document-profile Lab ΔE76 or L*),
  selection-only transforms, shared Phase 2 selection history. Selections stay canvas-sized coverage with Phase 2's
  clean Undo/Redo. Kernel contracts: [phase-6-selection-quality.md](phase-6-selection-quality.md).
- **6B Independent masks (shipped):** layer masks keep raw bytes at their own canvas position with independent bounds;
  content geometry moves linked masks only, canvas geometry moves both; paint/filter/fill edit original coverage
  without baking density/feather. Density lowers hiding strength; feather uses the bounded multiresolution Gaussian
  with radius in canvas pixels. Described-layer rerenders keep the independent mask.
- **Native v10 mask record:** stores independent dimensions, placement, linking, density and feather; v1–9 migrate with
  linked / full density / no feather. Writers cap aggregate logical mask storage at 6.8 billion pixels (independent of
  working-memory admission); rare legacy files over it need mask reduction before saving.
- **6C Refinement workspace (basic shipped):** five inspection backgrounds, before/after, Fit/100%, Smooth/Feather/
  Shift/Contrast, output to selection or mask (`../app/Sources/SelectionRefinement.swift`,
  `../app/Sources/RefineSelectionSheetController.swift`; typed twin `../app/Sources/NativeSelectionRefinement.swift`).
  Mask output creates/replaces the raw mask and resets it to linked/full-density/no-feather; selection output changes
  only transient coverage. Save Selection/alpha channels keep coverage reusable.
- **6C advanced (implemented, unexposed):** categorical trimap strokes (`rz_trimap_strokes`,
  `../core/src/trimap_strokes.rs`, `../app/Sources/RefinementTrimap.swift` — Unknown strokes release a constraint),
  sampled and closed-form alpha solvers, foreground-color estimation, and reversible foreground+mask output
  ([cutout contract](phase-6-cutout-output-contract.md)). MCP refuses advanced/unknown refinement fields with exact
  coverage and history preserved. *Why:* every estimated-alpha pipeline failed the combined alpha/color gate.
- **6D Assisted selection (shipped):** seeded Quick Selection (connected growth, not a semantic brush), Vision
  foreground-instance object targets (hit-tested on the coarse instance map) and one aggregate people selection.
- **Select and Mask, brush Quick Selection, Object Selection (2026-10-11):** the owner
  dropped the matting quality gate as a ship condition on 2026-10-10 (v1 is a competent editor, not a matting
  benchmark), so the edge controls ship as colour-guided tools that say what they are. Select and Mask adds Edge
  Detection (`../core/src/edge_refine.rs`: a guided filter per RGB channel inside a radius band, blended by how well
  each channel explains the coverage; Smart Radius fades it where the picture has no edge), the Refine Edge brush
  (an area plane: never / automatic / always), Decontaminate Colors (`foreground_solve.rs`, at 8 and 16 bits) and
  four outputs (selection, layer mask, New Layer, New Layer with Layer Mask) in the sheet, MCP `refine_selection`
  and Actions. Quick Selection (`selection_quick.rs`) paints with a sized brush, pre-smooths without crossing an
  edge, fills enclosed gaps and can Enhance Edge; the Object Selection tool (`object_select`) turns a dragged
  rectangle or lasso into Vision's instance inside it, refined. The byte cutout keeps the foreground's own alpha,
  as the typed one always did, so a cutout of a cutout works. Accuracy is tested against known-alpha composites
  (`../core/tests/edge_refine.rs`, `../scripts/verify_v1_selection.py`), not against the research corpus. Not
  learned matting: hair against a busy background and glass stay approximate.
- **Display bound:** masks over 200,000 contour vertices show a bounds outline; coverage/dimming stay exact and UI/MCP
  report the fallback.
- **Cancellation:** selection kernels are cooperatively cancellable; mask kernels and Vision workers have responsive
  but noncancellable progress. Masked Free Transform previews use admitted core geometry/projection.
- **Quality evidence is kept separate from functional qualification.** Candidate results (all against unchanged gates):
  guided 12/24 analytic; sampled 10/24 · 8/24 crops · 15/42 full; closed-form 10/24 · 9/24 · 15/42; ViTMatte 8–9/24,
  0 admitted full images. **Zero combined alpha/color passes** before 9I. Oracle-alpha color passes 20/24 · 22/24 ·
  30/42 — the color estimator works when alpha is right. A constraint audit shows the frozen automatic trimap makes
  6/24, 6/24 and 6/42 cases impossible regardless of solver. Details: [matting reference protocol](phase-6-matting-reference-protocol.md),
  [full-image protocol](phase-6-full-image-reference-protocol.md), [foreground evidence](phase-6-foreground-evidence.md),
  [ViTMatte spike](phase-6-vitmatte-spike.md).
- **Scheduling:** the gate is owned by 9I / carryover P7-12 and rescoped once as [gate v2](phase-9i-gate-v2-status.md)
  (thresholds unchanged; analytic pass rule = 12 recoverable cases; ambiguous and glass cases reported only; bounded
  opaque rule; completion = assisted protocol passing v2 on a declared 36-case subset). Phase 7A proceeded
  independently and does not waive this gate.

## Rejected alternatives
- Holding freehand selection / mask controls until matting research finished — plan boundary for Phase 6.
- Treating automatically thresholded prior labels as trustworthy hard constraints — the constraint audit proves they
  block passes; 9I separates automatic priors from explicit user constraints instead.
- Repairing inputs with reference alpha or lowering thresholds — forbidden by every Phase 6 protocol.

## Consequences
- Superseded 2026-10-10: advanced controls no longer wait on the gate. Colour-changing output still goes through the
  reversible foreground+mask path (New Layer outputs); selection and mask outputs carry coverage only.
- Mask records and the v1–9 migration defaults are persistent contracts; the 6.8 billion aggregate cap binds writers.
- Test drivers: `../scripts/selection-probe/verify.py`, `../scripts/verify_phase6.py`, `../scripts/verify_phase6c.py`;
  corpus and reports under `~/devel/doctored-fixtures/` (the `/tmp` receipts in the original were purged).

## Open items (verified 2026-09-26)
- `DONE` — Edge brushing and color decontamination ship in Select and Mask (2026-10-11). `DROPPED` — the
  gate v2 assisted protocol as their ship condition (owner direction 2026-10-10); the research estimators stay unexposed.
- `DONE` — Distinguish tentative automatic labels from explicit user strokes: soft-prior `user_constraints` in
  `closed_form_matting::estimate_soft_prior` and `../core/src/tiled_soft_matting.rs`; `InputPolicy` in
  `../core/examples/phase6_matting_quality.rs`. Qualifying that policy is part of the TRACKED item above.
- `DROPPED` — Improve coupled RGB recovery under the research gates: no longer a v1 condition (2026-10-10).
- `DONE` — Controls exposed; `refine_selection` refuses only fields it does not take.
- `DROPPED` — Stronger translucent-object evidence and a photographic transmission reference: glass left the pass rule
  for a reported-only glass diagnostic (transmission research declined; gate v2 decision 1, [archive/phase-9i-gate-v2-status.md](../archive/phase-9i-gate-v2-status.md)).
- `DROPPED` — Qualifying a deployed learned-matting implementation (MPS/Core ML): gate v2 decision 5, no neural
  matting model is added.
- `DONE` Part A — the Magnetic Lasso (6D follow-on): the core live-wire tracer (`edge_trace.rs`,
  `dr_selection_trace_edge`), the L-group tool with hover tracing and fastening, and MCP `select_magnetic`, at 8
  and 16 bits. `TRACKED` Part B — ⌥-drag freehand segments, the Caps Lock width cursor, the `,`/`.` contrast keys,
  stylus pressure, sub-pixel and direction cost terms, a cached per-capture edge map.
- `TRACKED` — Specialized subject models (6D follow-on) → plan §6D "individually selectable
  follow-ons". Part 0 (All People refuses a picture with no detected person) shipped in d6a9d77; Part B, Select ▸ Face Parts… and
  `inspect_faces` / `select_face_parts` (face, skin, eyes, irises, eyebrows, lips, mouth as u16 coverage from Vision landmarks through
  the core's `landmark_region`), shipped in the commit that adds `core/src/landmark_region.rs`. At 32-bit float the parts come
  from Vision's landmarks on the display rendering, so they can sit a fraction of a pixel from the 8-bit result. Part A, Select ▸
  Select People… and `model "people"` on `inspect_subjects` / `select_subjects` (people numbered left to right; each person is the
  accurate aggregate where an argmax over `generateMask(forInstances:)` names them, owning pixels only within one coarse cell of their
  map cells; `app/Sources/PersonInstances.swift`), shipped with the old one-shot renamed Select ▸ All People. Vision may merge people
  who stand close together; Select ▸ All People still covers them. Part C (sky/hair/skin/clothes model) stays open.
