# CMYK document mode (#547): status

- **Status:** Open — phases 1, 2, 3, 4a, 4b, 4c and 5 shipped, the selection, adjustments-and-filters, channels,
  retouch, artwork, styles and remaining-gaps batches and round 2 (kept-CMYK export)
- **Issue:** [#547](https://github.com/kpgalligan/doctored-dev/issues/547) (the colour-mode umbrella: CMYK here; Grayscale,
  Lab, Indexed Color, Duotone, Bitmap, Multichannel and spot channels are closed — their ADRs; PDF/X is the
  rest)
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
  Brightness/Contrast's Saturation; the Pass Through paint mode, Blend If on Red, Green or Blue; the retouch tools on one ink plate (as on an RGB colour plane). Not in Doctored at any depth: Pattern fill layers, Warp Text and
  Pen-made shape layers, Focus Area,
  Match Color, HDR Toning, a backdrop selection from a menu or MCP (core `source: backdrop` only), and the retouch batch's
  deviations (open items).

## Open items

- **Threshold's black on CMYK shipped as K only** (the owner's ruling): below the level it writes K 100 %, C/M/Y 0 (the
  take's T-K). Re-judged on copies of `cmyk-adjust-filter-547` take1 / take2: C/M/Y max 0 / mean 0, K equal to T-K's (mean
  7.97 / 5.98 codes), still MISS on K's tone boundary. `gray-luminance/take1` (levels 64 / 128 / 192, and 128 under Gray
  Gamma 2.2) tested the tone against T-K, the Gray working space's grey of the profile Y and L*: every candidate MISS
  (T-K closest, 2 / 3 / 0 of about 88 samples, each within 10 codes of the level; Gray 2 / 9 / 4, L* 16 / 7 / 2), and
  the Gray Gamma case read exactly what Dot Gain 20 % did, so the tone does not follow the Gray working space. T-K stays
  ([#547](https://github.com/kpgalligan/doctored-dev/issues/547)).
- **Adjustment residuals.** Brightness/Contrast, Selective Color, Photo Filter (Photoshop moves K under Cooling),
  Shadows/Highlights and Replace Color MISS the take's bound and ship as built (the shipped record's adjustments entry);
  Gradient Map had no valid case, and Add Noise and the Clouds miss the statistical gate
  ([#547](https://github.com/kpgalligan/doctored-dev/issues/547)).
- **Dodge/Burn/Sponge/Protect Tones on ink ship as built: take2 cannot separate the tone rule from the stroke footprint.**
  take2 (`cmyk-retouch-547`) max / mean codes, H0 vs the best alternative: Dodge/Burn 73 / 9.1 vs `twin` 71 / 9.1 (midtone
  means about 1.6), Protect Tones 86 / 7.0 vs PT-LUM 92 / 8.4, Sponge 66 / 5.1 vs `twin` 67 / 5.1; the maxima come from dab
  placement and spacing. Equalize ships as H0 at max 2 / mean 0.74 vs EQ-SUM-MID 2 / 0.80
  ([#547](https://github.com/kpgalligan/doctored-dev/issues/547)).
- **Equalize with a selection is unmeasured.** Photoshop's `ArtLayer.equalize()` raises its Selected Area Only / Entire
  Image modal even with dialogs off (take1 stalled there), so neither choice is measured
  ([#547](https://github.com/kpgalligan/doctored-dev/issues/547)).
- **Absent at every depth** (a deviation of the retouch batch, not a CMYK gap): Content-Aware Scale.
  The **Art History Brush shipped**: on 8-bit RGB and on 8-bit CMYK ink
  (`art_history_stroke.rs`, `cmyk_art_history.rs`); 16 and 32 bits and 16-bit CMYK refuse it by name
  (admission row `art-history-8`). `art-history-658/take1` recorded Photoshop refusing it at those depths.
  Batch 2 refit it to take1's whole strokes (one colour each, from the source at the start): every path style heads
  along the source's luma contours, travelling with the brighter side on its right, curls turn the heading
  angle down, and `L`, `κ` and a per-style density `ν` are fitted (`art_history_take.py` THE REFIT).
  `art-history-658/take2` confirmed the travel rule (BRIGHT-RIGHT) and the heading on held-out fixtures B, C
  and CMYK D. Residual per style, Doctored against Photoshop: loose_medium CONFIRMED; tight_short,
  tight_medium, tight_long and loose_long fewer or shorter strokes (Len, N) on some fixtures, Areas or sizes;
  the four curls also Curl, and tight_curl_long the travel (Photoshop turns further); D (CMYK) Len at one
  click only. Spot Healing's Proximity Match and Create Texture shipped at 8/16/32 bits and on ink
  (`spot_heal_type.rs`); `spot-heal-types-656-657/take1` MISSED both rules, so H0's settings
  ship (band 4, tile 7, detrended); after the take Create Texture's pool rule changed to leave out the blemish (pixels outside the 4 px band's residual range):
  re-judged on take1's recorded outputs U/G/T pass and S misses (Photoshop scrambles the
  stripes); that rule is unconfirmed by a fresh take.
- **Sponge Vibrance shipped**: the Vibrance stage at ±100 as the target, at 8/16/32 bits and on ink (pass A, K held);
  `retouch-options-659-660/take1` MISSED every candidate, so H0 ships.
- **Sample All Layers on Blur, Sharpen and Smudge shipped**: all visible layers (take1 undecided), every depth and ink;
  the picture is charged once beside the walk.
- **Blend If on ink: confirmed (take2).** Fitted on take1 (`cmyk_styles_take.py --fit`, THE BLEND IF FIT): Gray G-LUM/INC,
  Cyan, Magenta and Black P/INC, Yellow P/HALF (a tie-break: INC also met). take2 BUILT every channel under it (worst max /
  mean 1 / 0.026 Gray, 1 / 0.066 C, 1 / 0.082 M, 2 / 0.135 Y — INC 1 / 0.105 —, 1 / 0.003 K), and the judge's 11 `bi_*`
  Doctored renders PASS (max ≤ 2). The 16-bit form is extrapolated, not measured
  ([#547](https://github.com/kpgalligan/doctored-dev/issues/547)).
- **Styled CMYK PSDs read back.** Doctored reading Photoshop's case PSDs (take2 judge): 66 of 89 within the bound —
  every Blend If case now; the bevel MISSES by the shared renderer's bevel geometry, the gradient overlay by up to 14
  codes, the outer glow on edge pixels (mean ≤ 0.65), and four Normal 50 cases by exactly 1 code on M everywhere.
  Photoshop reading Doctored's: fixed. The bisect (`cmyk-psd-open-662/take1`, `cmyk_psd_open_take.py`) blamed only the
  Gradient Map record — a `grdm` body two bytes short of a multiple of 4 failed the whole PSD/PSB and flattened the
  TIFF; the light resources, `lfx2`/`CMYC`, `iOpa`, `knko`, the ranges, Generic CMYK and the other eight blocks opened.
  take2 opened every construct and the full styled export layered at psd8/psd16/psb16/tif16, plus RGB psd8/psd16. Its
  composites, recorded: drop and inner shadow up to 13 codes, inner glow 48 (M) and satin 55 MISS the 2-code bound at
  the effects' default size (rendering parity, not the `grdm` open failure). These composites were recorded before the
  shadows' and the Outer Glow's box-pair law (#578 part 2); no number here changes.
- The rest of [#547](https://github.com/kpgalligan/doctored-dev/issues/547): PDF/X and the Indexed Color PSD (Grayscale,
  Lab, Indexed Color, Duotone, Bitmap and spot channels closed by their ADRs, Multichannel closed by decision,
  `designs/multichannel-mode.md`). Spot channels: [spot-channels.md](spot-channels.md).

## Shipped runs

[cmyk-mode-shipped.md](cmyk-mode-shipped.md) — the decisions in force and phases 1, 2, Hue/Saturation, the selections,
4a, 5, 4b, the black plate, the adjustments and filters, 4c, 3, the channels batch, the retouch batch, round 2, the
artwork batch, the styles batch, the remaining gaps, Blend If and Hard Mix's exact tie.
