# CMYK document mode (#1): status

- **Status:** Open — open items remain ([#1](https://github.com/kpgalligan/doctored/issues/1),
  [#17](https://github.com/kpgalligan/doctored/issues/17), [#19](https://github.com/kpgalligan/doctored/issues/19));
  phases 1, 2, 3, 4a, 4b, 4c and 5 shipped, the selection, adjustments-and-filters, channels, retouch, artwork, styles
  and remaining-gaps batches and round 2 (kept-CMYK export)
- **Issue:** [#1](https://github.com/kpgalligan/doctored/issues/1) (the colour-mode umbrella: CMYK here; Grayscale,
  Lab, Indexed Color, Duotone, Bitmap, Multichannel and spot channels are closed — their ADRs; PDF/X and spot
  channels' open items are the rest)
- **Code:** `core/src/cmyk_*.rs`, `core/src/ffi_cmyk.rs`; `app/Sources/CmykDocument.swift`, `DoctoredCore+Cmyk.swift`,
  `CmykModeSheetController.swift`, `EditorViewController+CmykMode.swift`, `AgentServer+CmykMode.swift`, `AgentCatalog+CmykMode.swift`,
  `CmykEdit.swift`, `AgentServer+CmykEdit.swift`, `InfoPanelViewController+Cmyk.swift`, `CmykRefinementCutout.swift`; phase 5: `core/src/ffi_cmyk_colour.rs`,
  `CmykProfileEdit.swift`, `CmykProfileSheetController.swift`, `EditorViewController+CmykProfile.swift`, `CmykProof.swift`,
  `CmykToneTools.swift`, `ColorSettingsWindowController.swift`, `DoctoredCore+CmykColour.swift`, `AgentServer+CmykColour.swift`;
  adjustments and filters: `core/src/cmyk_adjust_ink.rs`, `cmyk_adjust_rgb.rs`, `cmyk_filter.rs`, `cmyk_fade.rs`,
  `app/Sources/CmykAdjustFilter.swift`; phase 4c: `core/src/cmyk_clipboard.rs`, `cmyk_clipboard_codec.rs`,
  `cmyk_payload_convert.rs`, `ffi_cmyk_clipboard.rs`; `app/Sources/CmykClipboard.swift`, `EditorViewController+MergeStamp.swift`,
  `AgentServer+StampMergeGroup.swift`, `AgentCatalog+StampMergeGroup.swift`; phase 3: `core/src/psd_import_cmyk.rs`,
  `psd_export_cmyk.rs`, `psd_adjustment_cmyk.rs`, `cmyk_tiff_read.rs`; retouch: `core/src/cmyk_passes.rs`, `cmyk_retouch.rs`,
  `cmyk_retouch_plate.rs`, `cmyk_retouch_brush.rs`, `equalize.rs`, `equalize_apply.rs`, `app/Sources/CmykRetouch.swift`,
  `Equalize.swift`; round 2: `core/src/cmyk_export_ink.rs`; artwork: `core/src/cmyk_artwork.rs`, `cmyk_artwork_render.rs`, `ffi_cmyk_artwork.rs`,
  `app/Sources/CmykArtwork.swift`, `DoctoredCore+CmykArtwork.swift`; styles: `core/src/cmyk_style.rs`,
  `cmyk_style_row.rs`, `psd_adjustment_grdm.rs`

## Next phases (contracts)

- **Still refused after phases 3, 4b, 4c and 5 and the selection, adjustments, channels, retouch, artwork, styles, remaining-gaps and Blend If batches** (by name):
  Exposure, Vibrance, Black & White, Lens Correction, the Develop Filter with Presence and Add Grain, Auto-Blend
  Layers, the RGB-only Hue Rotate, Grayscale, Sepia, White Balance, Depth Blur, Remove Red Eye and the Red Eye tool,
  Auto-Align's Vignette Removal and Geometric Distortion, Shadows/Highlights and Replace Color as layers,
  Brightness/Contrast's Saturation, HDR Toning; the Pass Through paint mode, Blend If on Red, Green or Blue; the retouch
  tools on one ink plate (as on an RGB colour plane).
- **Not in Doctored at any depth** (not CMYK gaps):
  - `OPEN` [#19](https://github.com/kpgalligan/doctored/issues/19) — Warp Text, and shape layers from a Pen path.
  - `OPEN` [#17](https://github.com/kpgalligan/doctored/issues/17) — Select ▸ Focus Area.
  - Match Color and Content-Aware Scale: not built (rows of [photo-editor-gap-review.md](photo-editor-gap-review.md)).
  - `DROPPED` (2026-10-30) — a backdrop selection from a menu or MCP (core `source: backdrop` stays internal): no
    request on record, and Sample All Layers covers composite sampling.

## Open items

- **Threshold's black on CMYK shipped as K only** (the owner's ruling): below the level it writes K 100 %, C/M/Y 0 (the
  take's T-K). Re-judged on copies of `cmyk-adjust-filter-547` take1 / take2: C/M/Y max 0 / mean 0, K equal to T-K's (mean
  7.97 / 5.98 codes), still MISS on K's tone boundary. `gray-luminance/take1` (levels 64 / 128 / 192, and 128 under Gray
  Gamma 2.2) tested the tone against T-K, the Gray working space's grey of the profile Y and L*: every candidate MISS
  (T-K closest, 2 / 3 / 0 of about 88 samples, each within 10 codes of the level; Gray 2 / 9 / 4, L* 16 / 7 / 2), and
  the Gray Gamma case read exactly what Dot Gain 20 % did, so the tone does not follow the Gray working space. T-K stays
  ([#1](https://github.com/kpgalligan/doctored/issues/1)).
- **Adjustment residuals.** Brightness/Contrast, Selective Color, Photo Filter (Photoshop moves K under Cooling),
  Shadows/Highlights and Replace Color MISS the take's bound and ship as built (the shipped record's adjustments entry);
  Gradient Map had no valid case, and Add Noise and the Clouds miss the statistical gate
  ([#1](https://github.com/kpgalligan/doctored/issues/1)).
- **Dodge/Burn/Sponge/Protect Tones on ink ship as built: take2 cannot separate the tone rule from the stroke footprint.**
  take2 (`cmyk-retouch-547`) max / mean codes, H0 vs the best alternative: Dodge/Burn 73 / 9.1 vs `twin` 71 / 9.1 (midtone
  means about 1.6), Protect Tones 86 / 7.0 vs PT-LUM 92 / 8.4, Sponge 66 / 5.1 vs `twin` 67 / 5.1; the maxima come from dab
  placement and spacing. Equalize ships as H0 at max 2 / mean 0.74 vs EQ-SUM-MID 2 / 0.80
  ([#1](https://github.com/kpgalligan/doctored/issues/1)).
- **Equalize with a selection is unmeasured.** Photoshop's `ArtLayer.equalize()` raises its Selected Area Only / Entire
  Image modal even with dialogs off (take1 stalled there), so neither choice is measured
  ([#1](https://github.com/kpgalligan/doctored/issues/1)).
- **Blend If's 16-bit form is extrapolated, not measured.** The 8-bit fit (take1) was confirmed by take2 on every
  channel (the shipped record's Blend If entries); the 16-bit knees are its ×257
  ([#1](https://github.com/kpgalligan/doctored/issues/1)).
- **Styled CMYK PSD composites outside the bound.** Doctored reading Photoshop's case PSDs (take2 judge): 66 of 89
  within the bound, every Blend If case among them; the bevel MISSES by the shared renderer's bevel geometry, the
  gradient overlay by up to 14 codes, the outer glow on edge pixels (mean ≤ 0.65), and four Normal 50 cases by exactly
  1 code on M everywhere. Photoshop's composites of Doctored's files (take2, which opened every construct): drop and
  inner shadow up to 13 codes, inner glow 48 (M) and satin 55 MISS the 2-code bound at the effects' default size
  (rendering parity). These composites were recorded before the box-pair law for the shadows and the Outer Glow
  shipped; no number here changes ([#1](https://github.com/kpgalligan/doctored/issues/1)).
- The rest of [#1](https://github.com/kpgalligan/doctored/issues/1): PDF/X, CMYK's open items above and spot
  channels' open items ([spot-channels.md](spot-channels.md)). Grayscale, Lab, Indexed Color, Duotone, Bitmap and spot
  channels closed by their ADRs; Multichannel closed by decision ([multichannel-mode.md](multichannel-mode.md)).

## Shipped runs

[cmyk-mode-shipped.md](cmyk-mode-shipped.md) — the decisions in force and phases 1, 2, Hue/Saturation, the selections,
4a, 5, 4b, the black plate, the adjustments and filters, 4c, 3, the channels batch, the retouch batch, round 2, the
artwork batch, the styles batch, the remaining gaps, Blend If and Hard Mix's exact tie.
