# Phase 6 — selection and mask workflows (ADR)

- **Status:** Accepted — open items remain ([#17](https://github.com/kpgalligan/doctored/issues/17)); 6A–6D shipped, Select and Mask included; the matting research gate is no longer a ship condition
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
- **6C advanced (implemented; the trimap strokes and the sampled and closed-form alpha solvers stay unexposed):**
  categorical trimap strokes (`rz_trimap_strokes`, `../core/src/trimap_strokes.rs`,
  `../app/Sources/RefinementTrimap.swift` — Unknown strokes release a constraint), sampled and closed-form alpha
  solvers, foreground-color estimation, and reversible foreground+mask output
  ([cutout contract](phase-6-cutout-output-contract.md)). Foreground-color estimation and the foreground+mask output
  have shipped since 2026-10-11 as Decontaminate Colors and the New Layer outputs (below). The research estimators
  stay unexposed; `refine_selection` refuses only fields it does not take. *Why:* every estimated-alpha pipeline failed the combined alpha/color gate.
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
- **Scheduling:** the gate went to 9I / carryover P7-12 and was rescoped once as [gate v2](phase-9i-gate-v2-status.md)
  (thresholds unchanged; analytic pass rule = 12 recoverable cases; ambiguous and glass cases reported only; bounded
  opaque rule; completion = assisted protocol passing v2 on a declared 36-case subset). The gate is not a ship
  condition (2026-10-10), and its assisted run is dropped.

## Magnetic Lasso cost rule
The live wire (`../core/src/edge_trace.rs`, its terms in `../core/src/edge_trace_cost.rs`) is the cheapest 8-connected
pixel path inside the corridor within Width of the trail.
- **Edge sample.** Per pixel, the Sobel gradient (the one `ops_filters::sobel_gradient`) of each of R·a, G·a, B·a and A
  in unit range, borders replicated. The pixel's gradient `g` is the first of the four with the largest magnitude; its
  strength `s = min(1, |g|/4)`, so a full unit step reads 1.
- **Step cost.** A step of length ℓ (1 or √2) along lattice direction `u` into pixel `q` costs
  `ℓ·(LAMBDA + 1 − A(q)·D(u,q)·C(q) + TRAIL_PULL·min(d(q), width)/width)`, `LAMBDA` 0.05, `TRAIL_PULL` 0.02, `d` the
  chamfer distance from the trail.
  - **Attraction** `A = s/c_max` where `s ≥ contrast/100`, else 0; `c_max` is the strongest `s` in the corridor, so
    the corridor's own strongest edge attracts fully.
  - **Direction** `D(u,q)`. With `φ` the angle between the step and the edge's tangent (`g` turned 90°), `D` is 1 for
    `φ ≤ 22.5°`, 0 for `φ ≥ 45°`, and between them `(cos φ − cos 45°)/(cos 22.5° − cos 45°)`. The lattice directions
    are 45° apart, so every tangent lies within 22.5° of one, which keeps the whole attraction. A step 45° or more off
    the tangent gains nothing and costs what flat ground costs: a diagonal across an axis-aligned edge, a zigzag across
    hatching. An edge attracts only along itself.
  - **Sub-pixel** `C(q) = 1 − SUBPIXEL·o(q)`, `SUBPIXEL` 0.1. `n` is the lattice direction nearest `g`: horizontal
    when `|gy| ≤ (√2−1)·|gx|`, vertical when `|gx| ≤ (√2−1)·|gy|`, else the diagonal whose sign is that of `gx·gy`.
    With `s₋, s₀, s₊` the strengths at `q−n, q, q+n` (coordinates clamped to the image), when `s₀ ≥ s₋`, `s₀ ≥ s₊` and
    `k = s₋ − 2s₀ + s₊ < 0`, `o = |s₋ − s₊|/(−k)`: twice the distance, in samples, from the pixel centre to the vertex
    of the parabola through the three strengths (0 when the edge runs through the centre, 1 half a sample away). Every
    other pixel has `o = 1`. `SUBPIXEL` is `2·LAMBDA`: half a pixel off costs two pixels of perfect path, enough to
    choose the centred row, far too little to leave an edge (flat ground costs ≥ 1.05).
- Every factor lies in 0…1, so a unit cost stays in `[LAMBDA, LAMBDA + 1 + TRAIL_PULL]`, the range the exact bucket
  queue relies on. The snap end, `c_max`, the corridor and fastening work in pixels.
- **Consequences.** A hard step aligned with the pixel grid lies between two rows, so both rows have `o = 1` and cost
  `LAMBDA + SUBPIXEL` (0.15) a pixel, against 1.05 for flat ground; an anti-aliased edge whose middle row is half-way
  costs `LAMBDA`. A plateau wider than two pixels is not centred (three samples cannot see its middle).
- **Vertex placement.** A path pixel `q` is a *located* edge pixel when `s(q) ≥ contrast/100` (the attraction's
  condition) and it is a strict-curvature maximum along `n` (`s₀ ≥ s₋`, `s₀ ≥ s₊`, `k < 0`). Its vertex is
  `centre(q) + δ·n` with `δ = (s₊ − s₋)/(−2k)`, clamped to `[−½, ½]`: the parabola's vertex, signed, in samples along
  `n` (so `o = 2|δ|`; along a diagonal `n` a sample is √2 px). Every other pixel (flat ground, a slope, a plateau, an
  edge under Contrast) keeps its centre. This holds for every vertex, `from`'s pixel and the end's included.
- **A vertex stays in its own pixel.** Each coordinate is written in its pixel's half-open range `[i, i + 1)`: one that
  would reach `i + 1` (a tie's +½, or a rounding to `f32`) is written as the largest `f32` below it. So `floor` of a
  returned vertex is its pixel, and a host passes a vertex back as `from` unchanged (`from`'s pixel is by `floor`).
  The cost is one `f32` step (2⁻¹⁷ px below 128, 2⁻¹¹ px below 8192) on the lower of a tie's two crest pixels.
  Returning each vertex's pixel beside it would be exact, but adds an output and a parallel array through every
  mutation of the host's session to save that one step.
- **Output.** The placed points, collinear runs dropped, then Douglas–Peucker with each point's own tolerance (a span
  keeps the point most over its tolerance), separately on `[0, fasten]` and `[fasten, end]`: 0.25 px for a located
  vertex, 0.5 px for a centre, which is known no better. At 0.5 px everywhere the chord from a corner's centre, half a
  pixel off a hard side, to a vertex far along that side stood in for the side and half-covered its pixels; at 0.25
  the side's vertex next to the corner is kept. Tighter costs vertices where located points follow a binary
  staircase's steps (a radius-1000 binary disc in four traces: 1,136 vertices; 2,629 at 1/8 px; 291 at 0.5).
- **Placement consequences.** A hard pixel-aligned step is outlined on the step from either crest row, and a
  box-filtered straight edge (one row covered `c` by the far side) exactly on its boundary, `1 − c` into that row. A
  corner is cut by a fraction of a pixel: three samples along one normal cannot see a corner.
- **The host's points.** The Magnetic Lasso's starting click becomes its pixel's placed vertex (the session asks the
  core for it with a one-pixel trace), so a closing trace back to the start ends on it. A straight (⌥) click's point
  is kept as given.
- **Edge map.** The samples come from a per-capture map (`../core/src/edge_map.rs`, `dr_edge_map_create`) that the
  host's Magnetic Lasso session holds: it fills 16×16 tiles on first touch, so a pointer move computes no sample a
  previous one computed, and holds at most 8,388,608 samples (64 MB), past which it starts afresh. A trace is the
  same with any map of its image or none; a call with none builds one for itself. `WORK_BUDGET` stays 131,072.

## Magnetic Lasso input
The session is `../app/Sources/MagneticLassoSession.swift`; the canvas half `ImageCanvasView+MagneticLasso.swift`.
- **Freehand segment.** While an outline is open, each tick of a drag with ⌥ held is joined to the path as given, with
  no tracing (ticks within half a screen pixel of the last are coalesced). The first such tick fixes the live wire as
  it stands, as an anchor, and the segment continues from its end; after an ⌥-press that is the press point, which
  the straight click already fixed. The segment ends at the first tick without ⌥ or at mouse-up, and its last point
  becomes an anchor that tracing resumes from. The drag of the press that starts the outline traces whatever ⌥ says,
  since ⌥ at that press chose Subtract. One ⌫ removes the whole segment; when the core refuses the re-trace along a
  removed path (a segment past the corridor cap), the wire rests on the anchor until the next move.
  `select_magnetic`'s `freehand` lists point indices joined the same way: a run of them is one segment, the wire
  traced up to it is kept, and its last point is an anchor.
- **Contrast keys.** Bare `,` lowers and `.` raises Contrast by 1, within 1–100; a held key repeats. The value persists,
  and an open outline takes it from its next trace, as `[` / `]` do for Width.
- **Pressure.** A pointer event that carries pen pressure `p` (0–1) traces at `max(1, Width·(1 − p))`: a hovering pen
  and a mouse trace at the Width set, and bearing down narrows the corridor toward the pointer. The unfastened wire is
  traced at the latest event's width, the fastened path is never re-traced, and the close traces at the Width set.
  The options bar's Pen Pressure checkbox (on by default, persisted) turns it off. Narrowing rather than widening
  keeps the Width set the ceiling, so the work budget and the 1–256 range hold. `select_magnetic` takes no pressure:
  an agent sets `width` per call.
- **Precise cursor.** With Caps Lock on and the pointer over the canvas, the pointer is replaced by a ring of radius
  the current trace width (the corridor the tracer searches) with a centre cross, drawn as the brush outline is;
  under 6 screen points across, the crosshair stays. Caps Lock toggled under a still pointer redraws it.

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
  `../core/examples/phase6_matting_quality.rs`.
- `DROPPED` — Improve coupled RGB recovery under the research gates: no longer a v1 condition (2026-10-10).
- `DONE` — Controls exposed; `refine_selection` refuses only fields it does not take.
- `DROPPED` — Stronger translucent-object evidence and a photographic transmission reference: glass left the pass rule
  for a reported-only glass diagnostic (transmission research declined; gate v2 decision 1, [archive/phase-9i-gate-v2-status.md](../archive/phase-9i-gate-v2-status.md)).
- `DROPPED` — Qualifying a deployed learned-matting implementation (MPS/Core ML): gate v2 decision 5, no neural
  matting model is added.
- `DONE` Part A — the Magnetic Lasso (6D follow-on): the core live-wire tracer (`edge_trace.rs`,
  `dr_selection_trace_edge`), the L-group tool with hover tracing and fastening, and MCP `select_magnetic`, at 8
  and 16 bits.
- `DONE` — Magnetic Lasso Part B, input: ⌥-drag freehand segments (and `select_magnetic`'s `freehand`), the Caps
  Lock width cursor, the `,`/`.` contrast keys, stylus pressure ("Magnetic Lasso input" above), shipped in the commit
  that adds `../scripts/verify_magnetic_lasso.py`. `OPEN` [#17](https://github.com/kpgalligan/doctored/issues/17) —
  hand checks: the ring and the wire by eye, a real pen.
- `DONE` — Magnetic Lasso Part B, tracing: the direction and sub-pixel terms and the per-capture edge map (the cost
  rule above), shipped in the commit that adds `core/src/edge_map.rs`.
- `DONE` — Specialized subject models (6D follow-on). Part 0 (All People refuses a picture with no detected person) shipped in d6a9d77; Part B, Select ▸ Face Parts… and
  `inspect_faces` / `select_face_parts` (face, skin, eyes, irises, eyebrows, lips, mouth as u16 coverage from Vision landmarks through
  the core's `landmark_region`), shipped in the commit that adds `core/src/landmark_region.rs`. At 32-bit float the parts come
  from Vision's landmarks on the display rendering, so they can sit a fraction of a pixel from the 8-bit result. Part A, Select ▸
  Select People… and `model "people"` on `inspect_subjects` / `select_subjects` (people numbered left to right; each person is the
  accurate aggregate where an argmax over `generateMask(forInstances:)` names them, owning pixels only within one coarse cell of their
  map cells; `app/Sources/PersonInstances.swift`), shipped with the old one-shot renamed Select ▸ All People. Vision may merge people
  who stand close together; Select ▸ All People still covers them. Part C, Select ▸ Sky (`ModelSelection.swift`,
  `select_sky`) and Select ▸ Person Parts… (hair, skin and clothes; `PersonParts.swift`, `select_person_parts`).
