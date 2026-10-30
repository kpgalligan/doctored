# Selection kernels and refinement qualification (ADR)

- **Status:** Accepted (kernels shipped; the guided candidate is replaced by `edge_refine.rs` and decontamination ships, 2026-10-11)
- **Phase / dates:** 6A–6C, 2026-09-09 – 2026-09-10
- **Original:** [archive/phase-6-selection-quality.md](../archive/phase-6-selection-quality.md) — full history, measurements, receipts
- **Key commits:** 83de68c, 653309d, 0155d41

## Context
Phase 6 needed Color Range, Quick Selection, selection-only transforms and ordinary refinement (Smooth/Feather/Shift/
Contrast) built on existing profile, resampling and morphology code, plus an honest test of whether an edge-aware
"refine brush" and foreground-color decontamination were good enough to expose. The essentials shipped; the guided
candidate failed its predeclared gate and stays unexposed. The workspace edits reusable coverage without claiming
automatic high-quality matting.

## Decisions
- **Four FFI exports in `../core/src/ffi_selection_tools.rs`** (kernels in `selection_tools.rs`, `selection_refine.rs`):
  inputs immutable; output is canvas-sized top-first u8 coverage published **only on success** (failure/cancel leave
  it untouched; output may alias an input); identity/zero-control succeeds with identical bytes and the host compares
  before registering history; NULL job token = same kernel without cancellation; tokens single-use.
  - `dr_selection_color_range` — sampled mode (1–32 samples, tolerance 0–200, softness 0–100, CIE76 ΔE in the
    document's D50 Lab via the ICC path) or tonal mode (L* bounds with linear shoulders). Source alpha multiplies
    coverage. **LUT profiles refuse** rather than assume sRGB.
  - Color Range phase 1 adds `mode: hue` (Reds … Magentas, `../core/src/color_range_band.rs`:
    `adjust_math::decompose`'s amount × (2 − max) since phase 2), `invert` (every mode, before the alpha scale) and
    Localized Color Clusters (`points` + `cluster_range`: each sample's ramp × `1 − d/R`,
    `R = range/100 · max(w, h)`), one kernel at 8/16/32 bits and on a CMYK view; the wand gains Sample Size (the
    truncating box mean about the seed, clipped, through `ops_stats::block_mean`). Photoshop 26.10.0 take1
    (`scripts/photoshop-probe/color_range_671_take.py`, receipt `doctored-fixtures/photoshop/color-range-671/take1`):
    bands MISS, the fitted `DEC(1.25)` ships at max 42–47 / mean 2.87–3.24 codes on the held-out chart (the undeclared
    `amount · (2 − max)` reproduces all twelve masks exactly and awaits a confirming take); Invert exact; clusters and
    Sample Size UNMEASURED (the descriptor carries no cluster position; a scripted wand click no Sample Size), so
    their declared defaults ship.
    Phase 2 adds Out Of Gamut (`range: out_of_gamut`, `../core/src/color_range_gamut.rs`,
    `dr_selection_out_of_gamut`): the gamut warning's one rule (`soft_proof::GAMUT_DELTA_E`) as coverage over the
    8-bit codes the warning proofs at each depth, through the window's proof setup, else the default preset, else
    Working CMYK; a CMYK document selects nothing. take2 (`scripts/photoshop-probe/color_range_671_take2.py`,
    receipt `doctored-fixtures/photoshop/color-range-671/take2`, held-out HW3 at 8 and 16 bits): the declared
    `amount · (2 − max)` SHIPS in place of `DEC(1.25)`: 55 held-out rows at max 0 codes (8-bit) and ≤ 0.51 / mean
    0.084 (16-bit, 8-bit scale), exact on take1's 13 masks. Out Of Gamut MISSES its 5 % gate: 3376 of 28800 HW3
    pixels disagree at 8 and 16 bits, nearly all dark saturated colours Doctored flags and Photoshop does not; the
    rule ships unchanged, the count pinned in `color_range_gamut.rs`. Photoshop's mask is binary. CMYK not
    measured: the scripted CMYK Color Range crashed Photoshop in smoke2, so take2 dropped that case.
    Phase 3 adds Skin Tones (`range: skin_tones`, core `mode: skin` with `fuzziness` 0–200, default 40;
    `color_range_band::skin_coverage` in the same kernel at 8/16/32 bits and on a CMYK view): ΔE76 from
    Lab (70, 18, 19), full within 18, then a shoulder 0.25·F wide. take3 (`scripts/photoshop-probe/color_range_671_take3.py`,
    receipt `doctored-fixtures/photoshop/color-range-671/take3`): the fitted LAB member MISSES its gate (held-out
    SK3 and the NASA portrait, max 221–255 / mean 25–46 codes) and ships pinned, no agreement claimed; Photoshop selects
    nothing at F 0 and its peak falls as F rises, a shape no declared family has. Detect Faces is described only (it
    re-weights the whole frame, mean 59.3 → 44.8 on the portrait) and stays open.
    Phase 4 (`scripts/photoshop-probe/color_range_671_take4.py`, receipt `…/color-range-671/take4`, fresh charts
    sk4g/sk4r at F 10–170, the NASA portrait and a second NASA public-domain face): two Photoshop-shaped members
    fitted on take3 (P per pixel; A re-centred on the image's own colours) both lose to today's band (mean of row
    means 31.5 and 33.0 vs 24.7 codes; neither is below it on every row), so the LAB band is KEPT unchanged. Detect
    Faces' two members (DF-C re-centre on the face rectangles, DF-S scale by the faces' mean colour) do not beat
    "faces off" on every face row (27.6 and 20.5 vs 19.6), so Detect Faces stays UNBUILT: Photoshop's Skin Tones is
    image-adaptive (equal colours read 0 in one photo, 250 in another) and Detect Faces lowers coverage outside the
    faces, which no centre move or uniform scale reproduces. Unknown or missing `mode` now refuses before the
    `fuzziness`/`band` checks.
  - `dr_selection_quick` — ≤ 4096 seeds, deterministic 4-connected BFS; a neighbour must be within the local `edge`
    threshold **and** within `tolerance` of its originating seed (bounds gradient drift). First claim wins;
    transparent pixels are barriers. The host samples the stroke and combines add/subtract/intersect; not semantic.
  - `dr_selection_transform` — `[a,b,c,d,tx,ty]`, filters nearest 0 / bilinear 1 / Catmull–Rom 2 / Lanczos3 3,
    fixed canvas, zero outside, singular/nonfinite refuse. Never transforms layer pixels.
  - `dr_selection_refine` — smooth → signed contour shift (regenerates the AA ramp from the 50% contour) → feather →
    contrast (hard 128 threshold at 1). Smooth/feather 0–64 px, shift ±64 px, contrast 0–1. Internal `edge_radius`
    0–16 / `edge_strength` 0–1 drive the guided candidate (positive radius requires a brush mask) — host-gated off.
- **Strict params:** strict JSON, unknown fields refuse, params > 128 KiB refuse. Core borrows the captured source,
  never flattens a second projection. Cancellation per row and ≤ 4096 pixels. Legacy sync entry points reuse the kernels.
- **Working reservations** (N canvas px, M max dimension; exclude borrowed source and host output): Color Range
  `N + 256 KiB`; Quick Selection `21N + 1 MiB`; transform `2N + 4096`; refinement `5N + 256 KiB`, with shift
  `9N + 20M + 4 + 256 KiB`; guided candidate `64N + 256 KiB`. No reduced-resolution authoritative path.
- **Guided candidate** (`../core/src/guided_selection.rs`): He/Sun/Tang guided filter eqs. 5, 6, 8; encoded luma
  guidance, ε 0.0004. A coverage filter only — not color estimation or semantic matting.
- **Analytic corpus** (`../core/tests/common/selection_quality.rs`): portrait/hair, fur, translucent glass, hard product
  × light/dark/ambiguous × blurred/thresholded = 24 cases at 320×256, alpha from 4×4 area samples of explicit shapes.
  **Predeclared alpha gate:** boundary-MAD regression ≤ 1/255 and ≥ 95% soft-detail recall (truth 32–223 retained if
  result ≥ 16); boundary = fractional pixels plus pixels whose 5×5 neighbourhood varies. Must be paired with native-pixel
  photographic inspection.
- **Foreground-color estimator** (`../core/src/foreground_estimate.rs`, FFI `dr_image_estimate_foreground` in
  `../core/src/ffi_foreground.rs`): Germer et al. multilevel estimation, fixed sweeps (10 at ≤ 32 px, 2 above),
  gradient weight 0.1, regularizer 0.005. Strict JSON requires `color_space: "encoded" | "linear"` (linear uses the
  document ICC decoder/encoder; LUT profiles refuse); optional `strength` 0–1. Returns estimated RGB with **unchanged
  source alpha — it does not apply the matte**; RGB at matte 0/255 exact. Zero strength / binary matte / byte-identical
  → NULL without error; a spatially constant fractional matte refuses. Scratch `128N + 1 MiB`.
- **Photographic inspection** (no alpha truth, never scored as accuracy): four checksum-pinned public-domain/CC0 photos
  via `../scripts/phase6_quality_fixtures.py`; initial masks from the app's `select_subject` over MCP
  (`../scripts/phase6_initial_masks.py`); comparisons from `../core/examples/phase6_selection_quality.rs`.
- **Masked Free Transform preview:** geometry and projection are separate `PreviewRenderer` stages, each admitted on the
  main thread before worker allocation; linked independent masks admitted at 1 B/px; a true perspective mask reserves
  the core maximum (100 MB). Pending/failed previews are labelled; stale generations never publish. Probe:
  `../scripts/selection-probe/mask_transform_preview.swift`.

- **Superseded 2026-10-11:** `dr_selection_quick` also takes `radius` (a brush disc per
  seed, then enclosed-hole fill) and `smooth` (range-gated Lab pre-smoothing), reserving the same `21N + 1 MiB`;
  kernel `../core/src/selection_quick.rs`. `dr_selection_refine` runs an EDGE stage first (`edge_radius` 0–64,
  `smart_radius`; `edge_strength` is gone) and its `brush` argument is the area plane (0 never / 128 automatic / 255
  always), reserving `64N + 256 KiB` when that stage runs. `guided_selection.rs` is deleted: its luma guide could
  not separate colours of equal luminance, so `../core/src/edge_refine.rs` filters per RGB channel and weights each
  by the share of the coverage it explains. The foreground estimate has a 16-bit twin
  (`dr_precision_doc_estimate_foreground`, encoded domain) over the shared `foreground_solve.rs`. The owner dropped
  the corpus gate as a ship condition on 2026-10-10; the new kernels are tested on known-alpha composites.

## Rejected alternatives
- Exposing the guided "refine edge" brush — 12/24, damages accurate contours when guidance is weak (e.g. ambiguous
  product 0.0017 → 0.3871).
- Enabling decontamination on the strength of simple FFI fixtures — needs trustworthy F/B and representative bleed
  evaluation.

## Consequences
- Guided 12/24 is the frozen baseline every later runner re-verifies; replacement candidates are in
  [phase-6-sampled-matting-candidate.md](phase-6-sampled-matting-candidate.md),
  [phase-6-closed-form-matting-candidate.md](phase-6-closed-form-matting-candidate.md) and
  [phase-6-vitmatte-spike.md](phase-6-vitmatte-spike.md), under the [reference/color protocol](phase-6-matting-reference-protocol.md).
- Photos show concrete limits: widened hair halos, missing whiskers, spreading product fringe, glass stays an opaque
  silhouette. Ordinary controls are user-driven coverage edits; they do not repair structure or color.
- Color output must keep originals and apply colors plus a separate editable mask as one Undo.

## Open items (verified 2026-09-26)
- `DONE` — Colors + separate editable mask as one undoable operation: `dr_doc_add_refinement_cutout`
  ([phase-6-cutout-output-contract.md](phase-6-cutout-output-contract.md), `../core/tests/refinement_cutout.rs`).
- `DONE` — Edge brushing and color decontamination ship in Select and Mask (sheet, `refine_selection`,
  Actions), no longer gated on the corpus (owner direction 2026-10-10).
