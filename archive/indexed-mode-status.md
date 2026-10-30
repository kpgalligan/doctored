# Indexed Color document mode (#1): status

- **Status:** Open — phases 1a (`451c329f`), 1b (`ac77c338`), 1c (`b7ba1382`), 2a (`8bb6008c`), 2b
  (`8ab6ccf2`), 2c (`70362821`) and 2d (`f9550c65`) shipped, completing phase 2; phase 3 is split: 3a
  (`3efbddb3`), 3b (`01e278dd`), 3c (`6c7446f2`: Pattern and Noise, Forced Custom, the render channels
  and every tool decided) and 3d (the 3d commit: one Photoshop take; the four palettes refused for good,
  Pattern and Noise kept and disclosed) shipped; the close is next.
  The decisions in force and the shipped phases' lines are in [indexed-mode-shipped.md](indexed-mode-shipped.md)
- **Issue:** [#1](https://github.com/kpgalligan/doctored/issues/1) (the colour-mode umbrella; CMYK is
  `designs/cmyk-mode-status.md`, Grayscale `designs/grayscale-mode.md`, Lab `designs/lab-mode.md`)
- **Code:** `core/src/indexed_document.rs`, `indexed_palette.rs`, `indexed_act.rs`, `indexed_edge.rs` and
  `indexed_dither.rs` (kernels), `indexed_paint.rs`, `indexed_geometry.rs`, `indexed_transform.rs`, `indexed_clipboard.rs`,
  `indexed_profile.rs`,
  `indexed_convert.rs`, `indexed_table.rs`, `indexed_io.rs`, `indexed_tiff.rs`, `indexed_encode.rs`,
  `indexed_export.rs`, `indexed_native.rs`, `ffi_indexed.rs`, `tiff_directory.rs` (the shared TIFF-directory
  leaf); `app/Sources/IndexedDocument.swift`, `RasterCore+Indexed.swift`, `IndexedModeSheetController.swift`,
  `IndexedColorTable.swift`, `ColorTableSheetController.swift`, `EditorViewController+IndexedMode.swift`,
  `EditorViewController+ColorTable.swift`, `AgentServer+IndexedColorTable.swift`, `AgentCatalog+Indexed.swift`,
  `EditorViewController+CmykMode.swift`'s indexed arms
- **Template:** Lab Color phase 1 (`d4cf7e32`) and Grayscale phase 1a (`fa9c7b5b`): the same seams, gate shape
  and phase numbering

## Phases

### 3d — one Photoshop take (shipped, the 3d commit)

- ONE take, `scripts/photoshop-probe/indexed_palette_take.py` (a sibling of `indexed_mode_take.py`; its
  docstring fixed the scenes, candidates, gates and build table before smoke1; the jsx job `indexed_palette`).
  Five smokes fixed only keys and one refusal rule, each stated there: Dither None OMITS `Dthr` (char or string
  `None` is refused), and a Master palette Photoshop refuses on every case is recorded REFUSED, not INVALID.
- Verdicts (rows below): Local Perceptual and Local Selective MISS, Master Perceptual and Master Selective
  REFUSED by Photoshop, Pattern MISS, Noise REFUSED by Photoshop. No candidate passed, so no fit or confirm
  take ran.
- So `indexed_convert::UNPUBLISHED_PALETTES` (all four) refuse for good: "The <title> palette is not available:
  its algorithm is unpublished. Use Adaptive (median cut) instead." The sheet lists them disabled; the catalog
  keeps the names. Pattern and Noise keep the declared rules (`DITHER_SPREAD` 51, Bayer 8 × 8 from the origin,
  Dissolve's hash) with these numbers disclosed. Tests `indexed_palette_take` (record, Pattern replay, Noise
  refusal) and `indexed_dither`; drive I53; no new plan, no RZDC change.

**The close** (after 3d): condense this doc and the companion into the ADR `designs/indexed-mode.md` (as Lab's)
(the decisions in force, each phase's commit, the measured takes' headline rows and the disclosed fallbacks:
the edge rule, median cut, and whatever 3d leaves declared), `git mv` both originals to `archive/` with their
`archive/INDEX.md` lines, sweep the README's Indexed paragraph, and close #1's Indexed part.

## Photoshop in Indexed Color (measured)

take1 (`~/devel/rasterize-fixtures/photoshop/indexed-mode-547/take1`, 2026-10-22, Photoshop 26.10.0;
`scripts/photoshop-probe/indexed_mode_take.py`, whose docstring fixed the scene, rule, candidates and build
table before the smoke): a 64 × 32 8-bit sRGB scene of six palette bands, converted in the session with ONE
`CnvM` to Indexed (Exact, no dither); Edit, Image, Layer, Select and Filter walked with a selection before and
after; one Brush stroke (9 px, hardness 100) in (160, 100, 200) on each.

- **Menus** (VALID): 495 items a walk; 164 enabled on RGB are disabled in Indexed; Image ▸ Mode ▸ Color Table is
  the one item enabled only in Indexed. As documented: Layer ▸ New ▸ Layer, New Adjustment Layer, Layer Mask,
  Image ▸ Adjustments, the Filter Gallery and every filter family are disabled, Edit ▸ Fill is enabled.
- **Disabled in Indexed** (refuse for good): Perspective Warp; Content-Aware Fill, Generative Fill; Sky
  Replacement; Apply Image, Calculations; Layer ▸ New ▸ Layer, Layer via Copy and Cut; Duplicate Layer; New
  Fill Layer; Select ▸ Subject, Sky, Select and Mask; Filter ▸ Liquify, Neural Filters.
- **Unmeasured, kept permanent** (disabled on the one-Background-layer RGB control too): Layer Style, New
  Adjustment Layer, Layer Mask, Vector Mask, Create Clipping Mask, Group and Ungroup Layers, Merge, Rasterize, the
  Filter menu's other leaves; text and shape layers, Stamp Selected and blend modes are unwalked.
- **Painting**: 806 pixels changed, 406 of them the ink; H0 737 of 805 (1 tie), H1 217 of 806; MISS (2b).
- **The edge rule (2b).** The runner's docstring fixed each candidate family and the gate (≥ 99 % of changed
  pixels exact, every ink pixel) before take1's edge pixels were read again. fit1 (`indexed-mode-547/fit1`):
  best THRESH k = 131 751 of 806, MIX 745 of 804, Floyd–Steinberg over MIX 743, H0 737 of 805, BAYER (16, 13, 5)
  737; none passes. fit2 (`fit2`, the controller's RULING-1 families): error diffusion of the coverage, best
  JJN canvas raster 748 of 806; ordered dither, best 8 × 8 from the stroke start 733; distance threshold, best
  rgb r = 30/32 740; none passes. Every changed edge pixel is E or its own backdrop.
- **take2** (`indexed-mode-547/take2`, 2026-10-22, Photoshop 26.10.0): VALID; the same stroke, ink and palette
  painted twice from one history state (`twin`) differ on 64 of 803 changed pixels (NONDETERMINISTIC; the first
  equals take1's plane); a fresh stroke, palette and ink (`fresh`, 13 px hardness 50, (230, 120, 60) on seven
  bands). Verdict NOT CONFIRMED, so THRESH k = 131 ships: twin 749 of 803, fresh 992 of 1156 (its own best there
  is k = 55, 1060), every ink pixel exact.
- **palette-take1** (`indexed-mode-547/palette-take1`, 2026-10-23, Photoshop 26.10.0;
  `python3 scripts/photoshop-probe/indexed_palette_take.py --take`, then `--decide`): VALID. Scenes A
  (ColorChecker patches and four ramps, 256 × 160), B (synthetic sky and hills, 384 × 256) and R (grey and
  colour 32-px blocks, 512 × 1024). Palette gate per case: Chebyshev Hausdorff ≤ 2 codes between the used
  entries and ≥ 99 % of pixels within 2 codes; a palette needs all four cases (A at 16 and 64, B at 32 and 256).
  - Local Perceptual: MISS. Best PC2 (k-means in CIELAB from the luma-weighted median cut) 0 of 4: worst
    Hausdorff 92, worst agreement 0.0 % (its best case, B 256: 14, 96.7 %); PC1 0 of 4 (114, 0.0 %).
  - Local Selective: MISS. Best SC2 (Adaptive snapped to the Web cube, fitted r = 23) 0 of 4: worst 80,
    6.8 % (best case B 256: 23, 25.7 %); SC1 (r = 18) 0 of 4 (102, 0.0 %).
  - Master Perceptual, Master Selective: REFUSED, every case ("The parameters for command "Convert Mode" are
    not currently valid."); Master Adaptive too (smoke5), while Local Adaptive converts.
  - Pattern (gate ≥ 99 % exact on every converted case): MISS. The shipped rule 520 128 / 524 288
    (99.21 %) on Web, 329 664 (62.88 %) on System (Mac OS); best fitted Bayer (8, origin, M/64, S 51)
    99.27 % / 62.99 %; a per-cell 8 × 8 table read off the Web ramp 100 % / 62.97 %.
  - Noise: REFUSED by Photoshop through the descriptor under `Nose`, `Nois`, `BNoi` and `noiseDither`; not
    measured.

## Open items

- The swatch's merged-key branch (`PrecisionGeometryWork.compute` with a background key: the Move tool, Free
  Transform and the Crop tool past the canvas on an Indexed document) is reachable only from a canvas gesture;
  its request shapes have a core check (`indexed_background.rs`), the Swift branch none.

## Shipped runs

Moved to [indexed-mode-shipped.md](indexed-mode-shipped.md): the decisions in force and each shipped phase's lines.
