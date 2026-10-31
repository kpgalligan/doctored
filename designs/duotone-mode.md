# Duotone document mode (ADR)

- **Status:** Accepted — closed 2026-10-23. [#1](https://github.com/kpgalligan/doctored/issues/1) (the colour-mode
  umbrella) stays open (see [cmyk-mode-status.md](cmyk-mode-status.md)). Grayscale is
  `designs/grayscale-mode.md`, Lab `designs/lab-mode.md`, Indexed Color `designs/indexed-mode.md`, Bitmap
  `designs/bitmap-mode.md`, CMYK `designs/cmyk-mode-status.md`.
- **Original:** [archive/duotone-mode-status.md](../archive/duotone-mode-status.md) (the working record at close).
- **Takes:** `~/devel/doctored-fixtures/photoshop/duotone-mode-547/smoke5`, `smoke6`, `take1`; runner
  `scripts/photoshop-probe/duotone_take.py` in the private archive (`kpgalligan/doctored-dev`). No take was run for the close: take1's receipts settle it.
- **Code:** `core/src/duotone_ink.rs` and `duotone_ado.rs` (kernels), `duotone_document.rs`, `duotone_native.rs`,
  `duotone_files.rs`, `duotone_convert.rs`, `duotone_gray.rs`, `ffi_duotone.rs`, `gray_document.rs`'s
  `GrayPolicy.duotone`; `app/Sources/DuotoneDocument.swift`, `DuotoneOptions.swift`, `DoctoredCore+Duotone.swift`,
  `DuotoneSheetController.swift`, `DuotoneInkSheets.swift`, `DuotonePreview.swift`,
  `EditorViewController+DuotoneMode.swift`, `AgentServer+Duotone.swift`, `AgentCatalog+Duotone.swift`.
- **Template:** the Grayscale ADR and Indexed phase 1a (`451c329f`): the same gate shape and word slot.

## Context

Image ▸ Mode ▸ Duotone at 8 and 16 bits: Monotone, Duotone, Tritone and Quadtone over a Grayscale document,
the inks' curves and overprint colours, the view through the inks, `.doctored`, Duotone PSD/PSB, `.ado` files, the
conversions out (RGB, CMYK, Lab, Indexed, Grayscale, Bitmap), the exports through the inks and the items
refused in Duotone, from the UI, over MCP and in Actions. No refusal or catalog note names a phase.

## Decisions in force

- **The model.** A Duotone document IS a Grayscale document (8 or 16 bits, layers and all) whose `GrayPolicy`
  carries a description: 1–4 inks, each a name (1–64 characters), a D50 CIE Lab colour and a curve of 13 slots
  at the inputs 0, 5, 10, 20, 30, 40, 50, 60, 70, 80, 90, 95 and 100 % (0 % and 100 % set), plus one overprint
  colour per combination of two or more inks (authored Lab or the default). The descriptor keeps `color_mode`
  `"gray"` (so every Gray rule applies) and adds `"duotone":{"type","inks"}`; the host's `colorMode` answers
  `duotone`. Ink colours are D50 Lab, as the PSD duotone record carries them.
- **The view (measured, [A]).** Per gray code g (u16), k = 1 − g/65535; each ink's coverage cᵢ = Sᵢ(255k)/255
  (Curves' natural spline through the set slots ×2.55), through the Gray tone curve c′ᵢ = 1 − kTRC(1 − cᵢ); the
  Neugebauer sum over every ink subset in PCS XYZ (paper Lab (100, 0, 0), an authored overprint, else the paper
  times the inks' ratios to it); the twin's matrix (clamped) and encoder into RGBA16 in the document's own
  profile, alpha kept, from a 65 536-entry table built once a use (`DuotoneViewTable`). Overprint colours change
  the display only.
- **The render** (`duotone_convert`) is the conversions' and exports' one use of the view: an RGB document in
  the twin, each raster leaf through the view, the record tree, masks, clipping, modes, a Gray Blend If, comps,
  channels, guides, paths, samplers, light, metadata and resolution carried (`duotone_leaves_mapped`, the tree
  assembly the Grayscale conversion shares). Blockers (first in postorder): an adjustment layer and a layer style
  with effects, named with `flatten_required`; `flatten` runs Flatten Image first. Plan: raster pixels ×
  `RENDER_BYTES_PER_PIXEL` (8), the view's table, Flatten's scratch, bookkeeping.
- **Conversions out.** RGB Color is the render then Convert to Profile (`convert_duotone_to_rgb`), narrowed to 8
  bits from an 8-bit source; CMYK, Lab and (8-bit) Indexed Color take the rendered document through their own
  routes. **Grayscale renders the inks into the gray** (the close, `duotone_gray`): per u16
  code g the mix's PCS luminance Y at k = 1 − g/65535, clamped to [0, 1], through the document's own Gray
  profile at its depth (`GrayPolicy::encode` then `code`, Gray's `luminance_code` rule), mapped through a
  65 536-entry table by the render's tree assembly; the inks drop and every other record and chrome carries. No
  "Discard color information?" and no `gray_space` (the profile is the document's). Its plan: raster pixels (the
  canvas when flattening) × `RENDER_BYTES_PER_PIXEL`, the table (`GRAY_TABLE_BYTES`), Flatten's scratch, the
  result's display preparation (canvas × `TO_DUOTONE_BYTES_PER_PIXEL`) and bookkeeping. **Bitmap…** is that
  Grayscale rule then Gray → Bitmap through the Bitmap sheet (`convert_duotone_to_bitmap`), with Bitmap's own
  blockers and its 8 Bits/Channel step; its plan the Grayscale stage without the display term plus Bitmap's.
  Bitmap → Duotone stays refused. One undo step each; every target asks the Flatten
  question for a blocker.
- **Exports** render RGB through the view in the core, and every export reply carries `duotone`
  (`exportNote`). A save that is not `.doctored` refuses (`saveRefusal`): PSD and PSB are File ▸ Export as PSD's.
- **PSD and PSB** (colour mode 8, 8 or 16 bits). The colour-mode data is Photoshop's duotone specification
  (`duotone_ado`, 524 bytes; Lab written to 0.01, curves to 0.1 %). The open is the Gray open with the inks on
  the policy, carrying the file's block (the data and every 1014/1017/1018/1063 resource); the save is the Gray
  writer post-processed (`into_duotone_file`), so unchanged inks write their data and resources back byte for
  byte. The block lives in memory only: `.doctored` keeps the inks. A layered TIFF refuses. Since spot channels
  1b-2 ([spot-channels.md](spot-channels.md)), a 16-bit document's PSD/PSB is written at 8 bits from its Image ▸ Mode ▸ 8 Bits/Channel copy (the inks
  kept, the document unchanged; one loss line, the report's depth 8), because Photoshop, the compatibility target, opened no 16-bit Duotone
  file.
- **`.ado`.** A Duotone Options file is the same specification (the sheet's Load…/Save…, `set_duotone path`,
  `get_duotone path overwrite`). No Pantone or licensed library ships.
- **`.doctored`** (legacy `.rz` still read). Word 109 in the CMYK slot; `GRAY` then `DUOT` where the CMYK table goes (`duotone_native`). Float
  storage under 109 refuses. `DRDC_VERSION` unchanged.
- **Memory.** Mode ▸ Duotone… shares every sample (canvas × `TO_DUOTONE_BYTES_PER_PIXEL` (31) and bookkeeping);
  the render, the conversions, the exports and the close's Grayscale and Bitmap are held by
  `scripts/duotone-admission/measure.py`; a miss raises `RENDER_BYTES_PER_PIXEL` or that case's term only.
- **The gate.** `DuotoneDocument.refusal(tool:arguments:)`, asked by `DocumentPrecision.refusal` first, and
  `refuses(selector:proofColorsOn:)` in the Gray branch of `validatePrecisionItem`; anything else runs as Gray.
  Refused for good, "<tool> is not available in Duotone." (`DuotoneDocument.disabledTools`, `disabledFilters`,
  `refusedSelectors`): Assign and Convert to Profile (the core refuses them too, `ASSIGN_REFUSAL`,
  `CONVERT_REFUSAL`), Proof Setup, Proof Colors and `render_proof`, Content-Aware Fill, Puppet Warp, Perspective
  Warp, Select Subject and Sky, Depth Blur, Super Zoom, JPEG Artifacts Removal and Colorize, and the filters
  `develop_filter`, `lens_correction` and `tilt_shift`; 16 Bits/Channel (`DEPTH16_REFUSAL`) and 32 Bits/Channel
  (`DEPTH32_REFUSAL`); 16 → 8 keeps the inks. **Proof Colors is let through while proofing is on**, so a
  document converted into Duotone with Proof Colors on can turn it off; over MCP `set_proof_setup` is admitted
  when every key but `document_id` is `proof_colors` or `gamut_warning`, each false (`turnsProofingOff`). CMYK
  and Indexed admit Proof Colors, so only Duotone needed it. Merge to HDR is admitted as Gray admits it.
- **Panels.** The Layers and Channels thumbnails go through the inks (fitted at u16 first); the Channels row is
  named after the type; Duotone Options' Preview shows the canvas through the sheet's inks (view-only). New from
  Clipboard of a Duotone copy is a Grayscale document of the copied gray.

## Photoshop's measured facts

Photoshop 26.10.0, take1 (`duotone_take.py --take`, `decided.json` VALID, decision A).

- **The view** (Mode ▸ RGB Color of Doctored-authored PSDs, 26 patch centres in 8-bit RGB, gate max |Δ| ≤ 3
  and every channel's mean ≤ 1): two inks [A] max 0.49, means 0.08 / 0.14 / 0.16; three inks [A] max 0.56, means
  0.13 / 0.18 / 0.24. [B] (the sum on encoded sRGB) missed both, max 54.90 / 56.99.
- **Duotone → Grayscale** (the close's gate on take1's `gray.tif`, declared before comparing: every pixel's
  8-bit result within one code of each Photoshop code recorded for its source code): two inks max |Δ| 1 over
  1760 pixels, then three inks, nothing refit, max |Δ| 1 over 1760 pixels. PASS
  (`duotone_photoshop_take.rs`'s `photoshops_grayscale_of_a_duotone_is_the_inks_luminance`).
- **The PSD readback**: Photoshop opens Doctored's PSD as Duotone with its inks and curves, and its own save
  holds colour-mode data byte-equal to Doctored's (both cases), adds 1014 and 1017 and drops the ICC.
- **The menu walk** (the opened two-ink document and its Grayscale conversion): Assign and Convert to Profile,
  Proof Setup and Proof Colors DISABLED; Merge to HDR ENABLED; Image ▸ Mode ▸ Bitmap ENABLED; 32 Bits/Channel
  DISABLED (confirmed); the channels named Duotone and Tritone.

## Known differences

- **Duotone → Bitmap is composed and unmeasured:** Doctored's Grayscale rule then its Bitmap conversion; no
  take measured Photoshop's Bitmap from Duotone.
- **16-bit Duotone is kept** (Duotone from 16-bit Grayscale, 16 → 8 keeping the inks); unmeasured. Measured at
  the file: Photoshop 26.10 refuses to open a 16-bit Duotone PSD/PSB (`spot_psd_readback_take.py` take1),
  so `.doctored` keeps 16 bits and PSD/PSB write 8.
- **Unmeasured for good** (no dialog-free reading): the Layers and Channels thumbnails, Duotone Options'
  Preview, and New from Clipboard.
- **EPS and DCS 2.0** are not planned.

## Shipped

- **1a** (`00c31aa1`): the model, the sheet, the view, Duotone ↔ Grayscale, `.rz`, MCP; drive D1–D9.
- **1b** (`7f05bfe9`): PSD/PSB, the conversions out, exports, `.ado`, the sheet's number fields; drive D10–D16.
- **2a** (`761abdd0`): profiles, proof, thumbnails, the Channels row, Preview, New from Clipboard; drive D17–D21.
- **2b** (`9fd3eabd`): take1 (the view, the Photoshop-written PSD, the menu walk); drive D22–D23.
- **The close** (2026-10-23): Grayscale renders the inks (gated against take1), Bitmap… from Duotone, the
  refusals by menu below (16 Bits/Channel among them), Proof Colors' way off, the unreachable arms removed
  (`exactProofImage`, Gray's profile route on a Duotone document, the `assign-duotone16`/`convert-duotone16`
  admission cases, the NULL drop of `dr_precision_doc_set_duotone`). Tests `core/tests/duotone_close.rs` and the take's Grayscale gate; drive
  `scripts/verify_duotone_mode.py` D24–D27.

## Refused in Duotone, by menu

The close refused these for good ("<tool> is not available in Duotone.", the gate above), by the menu item that
reaches each:

| Menu item | Refused tools and selectors |
| :--- | :--- |
| Edit ▸ Content-Aware Fill | `content_aware_fill`, `preview_content_aware_fill`, `discard_content_aware_fill_preview`, `contentAwareFill:` |
| Edit ▸ Puppet Warp | `puppet_warp`, `showPuppetWarp:` |
| Edit ▸ Perspective Warp | `perspective_warp`, `showPerspectiveWarp:` |
| Image ▸ Mode ▸ 16 Bits/Channel | `convert_precision` rgba16 and gray16 from 8 bits, `convertToSixteenBits:` |
| Select ▸ Subject | `select_subject`, `selectSubject:` |
| Select ▸ Sky | `select_sky`, `selectSky:` |
| Filter ▸ Neural Filters | `depth_blur`, `super_zoom`, `jpeg_artifacts_removal`, `neural_colorize` and their four sheets |
| Filter ▸ Develop Filter | `apply_filter` develop_filter, `showDevelopFilter:` |
| Filter ▸ Lens Correction | `apply_filter` lens_correction, `showLensCorrection:`; `list_lens_profiles` stays admitted (a read) |
| Filter ▸ Blur Gallery ▸ Tilt-Shift | `apply_filter` tilt_shift, `showTiltShift:` |
| View ▸ Proof Setup (the submenu) | refused before the close, with Proof Colors' way off above |

take1's menu walk (Photoshop 26.10.0) read 25 paths enabled in Gray and disabled in Duotone that no row of it
names: the eleven above; File ▸ Save and File ▸ Revert, disabled only because the walked document was unmodified
(both admitted); and twelve that reach no Doctored tool (File ▸ Automate ▸ Crop and Straighten Photos, Edit ▸
Content-Aware Scale, Layer ▸ Mask All Objects, Select ▸ Focus Area, Filter ▸ Adaptive Wide Angle, Filter ▸ Blur
Gallery and its Field, Iris, Path and Spin Blur, Filter ▸ Video and Video ▸ De-Interlace).

## Open items

None.
