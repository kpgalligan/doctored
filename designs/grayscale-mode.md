# Grayscale document mode (ADR)

- **Status:** Accepted — closed 2026-10-21. [#1](https://github.com/kpgalligan/doctored/issues/1) (the colour-mode
  umbrella) stays open (see [cmyk-mode-status.md](cmyk-mode-status.md)). Lab Color is [lab-mode.md](lab-mode.md).
- **Name:** this ADR is `grayscale-mode.md`, as the closing request named it, not the `-status` basename of the
  `v1-stacks-status.md` precedent.
- **Originals:** [archive/grayscale-mode-status.md](../archive/grayscale-mode-status.md) (the working record at close) and
  [archive/grayscale-mode-shipped.md](../archive/grayscale-mode-shipped.md) (its shipped companion).
- **Takes:** `~/devel/doctored-fixtures/photoshop/gray-luminance/take1`, `gray-mode-547/take1`,
  `gray-files-547/take1` to `take4`; the runners are `scripts/photoshop-probe/gray_mode_take.py`,
  `gray_files_take.py`, `gray_export_take.py` and `gray_export_take4.py` in the private archive
  (`kpgalligan/doctored-dev`).
- **Code:** `core/src/gray_*.rs`, `psd_import_gray*.rs`, `psd_export_gray*.rs`, `ffi_gray*.rs`, `icc_builtin.rs`
  (`gray_blob`, `twin_blob`); `app/Sources/GrayDocument.swift`, `GrayAdjust.swift`, `GrayClipboard.swift`,
  `GrayProfileEdit.swift`, `GrayArtwork.swift`, `AgentServer+GrayColour.swift`, `DoctoredCore+Gray.swift`,
  `AgentCatalog+Gray.swift`, `EditorViewController+GrayMode.swift`.
- **Template:** CMYK document mode, phase 1 (`ad8db093`): the same seams and gate shape.

## Context

Image ▸ Mode ▸ Grayscale, at 8, 16 and 32 bits: New, open, both conversions, `.doctored`, layer structure, paint,
adjustments, filters, retouch, effects, Gray PSD/PSB/TIFF in and out, geometry, selections, clipboard,
profiles and Bits/Channel, proof, type/shape/fill layers, comps, analysis, multi-image, neural and generative
tools, from the UI, over MCP and in Actions. Every tool the catalog offers is admitted or refused for good by name.

## Decisions in force

- **Storage is the RGB precision document with R = G = B.** A Gray document is a `PrecisionDocument` whose `gray`
  policy (`GrayPolicy`: the Gray ICC bytes verbatim, the declared depth, the RGB twin) is `Some`. 8 and 16 bits
  store u16 (8 bits stores each code ×257 and every conversion into it rounds to that grid; paint and adjustments
  may leave the grid, an 8-bit save rounds `round(v/257)`); 32 bits stores linear float, R = G = B = luminance `Y`.
  No new pixel type, no new compositor.
- **The twin is the view.** The document's and every layer's profile is the policy's RGB twin
  (`icc_builtin::twin_blob`: sRGB's D50 columns and `chad`, the Gray profile's `desc` and `kTRC` as its three TRCs),
  so a neutral pixel's luminance is `kTRC(code)` and the view, render, sample and compositor are the RGB ones.
  `dr_precision_doc_profile` and the host's `iccProfile` answer the twin.
- **Built-in Gray profiles.** The seven Color Settings spaces (Dot Gain 10–30 %, Gray Gamma 1.8/2.2) and sGray
  are built by `icc_builtin::gray_blob` (v2.1 `mntr` GRAY, a 4096-entry `kTRC`; sGray sRGB's 1024-entry IEC
  table). No Adobe file is read.
- **Mode ▸ Grayscale is the source profile's relative PCS `Y` carried into the Gray space** (`gray_convert::
  gray_image`, the one home of the rule): `cmyk_color::document_xyz` at 8/16 bits, the source's Y row on linear
  floats at 32. CMYK → Gray goes through 16-bit ProPhoto RGB (relative + BPC); Gray → RGB is Convert to Profile
  from the twin; Gray → CMYK is the RGB → CMYK conversion on the twin at 8/16 bits (32 refuses).
- **Which files open as Gray:** exactly those `dr_path_gray_source` answers for: PNG colour type 0/4 at 8/16 bits,
  a BlackIsZero TIFF of 1–2 unsigned 8/16-bit samples, a one-component 8-bit JPEG, each untagged or with a usable
  Gray ICC (a PNG `sRGB` chunk is sGray; untagged is the working Gray under an import warning); a mode-1 PSD, PSB
  or layered gray TIFF at 8/16 bits, a 32-bit Gray PSD or PSB, and a one-channel float TIFF with a Gray profile.
- **`.doctored` word 106** (legacy `.rz` still read) in the CMYK slot; its `GRAY` table (schema 1, depth, ICC) is
  the last before LIDS. `DRDC_VERSION` stays 89. The reader requires word ⇔ table, depth ⇔ storage and the body
  profile equal to the table's twin.
- **The gate.** The core's `gray_document::admit_edit` (asked first by `ffi_precision_edit::request` and the
  plate retouch) and `precision_capability::GRAY_FAMILIES`; the host's `GrayDocument` asked first in
  `DocumentPrecision` and `validatePrecisionItem`. Every parser op is admitted or has its own arm; a request RGB
  also rejects (an unknown op, a malformed style, an unknown adjustment) gets RGB's own error. The admitted
  paint, adjustment, filter, retouch, channel, geometry, selection, clipboard, proof, artwork, analysis and
  multi-image tools (`GrayDocument.depthRuled`) then answer the depth's own rules; the other lifted tools run
  their handlers' own depth checks.
- **Blend modes: 21 in Grayscale** (`GRAY_BLEND_MODES`, `NativeLayerBlending.grayModes`):
  every mode but Darker Color, Lighter Color, Hue, Saturation, Color and Luminosity, which refuse "The `<mode>`
  blend mode is unavailable in a Grayscale document."; RGB → Gray's blocker reads the same list.
  At 32 bits the intersection with the float document's admitted modes (12).
- **K % is the code's complement**, `100 × (1 − code / max)` (`GrayDocument.kPercent`: the Info panel, `sample_color`
  and each Color Sampler row's `gray_k_percent` at 8/16 bits).
- **Ink is gray by the Mode rule** (`precision_paint::prepare_colors`, `float_paint_ink::linear_inks`): a Fill of a
  colour equals that colour converted. Brush, Fill's `paint`, Paint Bucket, Gradient and Stroke… take the Grayscale
  modes; the Eraser is `erase_native`.
- **Adjustments, filters and retouch run the RGB kernels on R = G = B.** `gray_adjust::GRAY_ADJUSTMENTS` (host
  `GrayAdjust`): Levels, Curves, Brightness/Contrast, Exposure, Invert, Posterize, Threshold and Gradient Map as
  layers and pixels; Shadows/Highlights and Replace Color pixels only; Auto Tone, Auto Contrast and Equalize as
  their own tools (`apply_filter auto_*` refuses, naming the tool). Levels' and Curves' r/g/b must be alike; a
  Gradient Map's stops are stored greyed. Filters fold each sample (`gray_filter`: the value when R = G = B, else
  Rec. 709 luma; Add Noise one delta; Clouds' colours greyed). The Sponge moves gray levels around middle gray;
  the History Brush and plate clones take Gray sources only; the Art History Brush runs at 8 bits.
- **Effects and Blend If:** every effect, its colours stored as their gray (`gray_style`); Blend If on Gray only.
  The Gray channel target is the layer; colour planes refuse for good (`PLANE_REFUSAL`); alpha channels follow the
  mask everywhere; a scalar read or write of the Gray layer is `scalar_edit` target `gray` (Apply Image) only.
- **Profile and depth edits are the RGB ones on the twin** (`gray_profile`); the RGB profile exports refuse a Gray
  document; 32 → 8/16 is HDR Toning's.
- **Artwork is gray** (`gray_artwork`): a type, shape or fill description stores each colour as its gray, decided
  when the host places it; the RGB renderer runs in the twin and its output is folded (`fold_artwork`). Comps,
  links and snapshots are `doc_comps`' rules with a Gray Apply that skips a mode or style the mode cannot hold.
- **Outside colour is folded at publication** (`gray_artwork::folded_result`, plan `fold_bytes`: 8 B/px, 16 at
  32 bits): Super Zoom, JPEG Artifacts Removal, every Generative picture, Merge to HDR and (the close) a placed
  image. **Place keeps no retained source** on a Gray document: re-rendering from the colour master would bring
  colour back, so the placed layer is plain folded pixels and the host's chained `rasterize` changes nothing.
- **RGB → Gray and a Generative Layer** (the close): from a document in the Gray profile's twin the binding stays
  live (tag and table carried, rebound to the converted raster); from any other profile the layer converts as a
  plain raster with its variations detached, as Convert to Profile detaches them on RGB, since a picture's codes
  are read in the document's profile. Gray → RGB does not carry a binding.
- **Saves and exports keep one channel** (`gray_export`): profile `document` with PNG, TIFF or JPEG writes R (and
  alpha when some pixel is not opaque) with the Gray ICC embedded, default depth 16 (8 for JPEG and an 8-bit
  document); a 32-bit TIFF is a one-channel float TIFF; Export Layers takes the same path. PSD/PSB/layered TIFF are
  `export_psd`'s, in Photoshop's mode-1 layout. File ▸ Save keeps an 8- or 16-bit PNG, TIFF or JPEG in place only
  while `GrayDocument.saveLosses` is empty: the core's `flat_state` list read off the typed records
  (`dr_precision_doc_flat_state_losses`, the byte list's words), plus depth, profile and metadata; `save_copy` and
  `get_document.save_preservation` answer from the same list.
- **Refused for good, by name**: Remove Red Eye; Auto Color and the adjustments `GRAY_ADJUSTMENTS` marks
  `Unavailable` (Vibrance, Hue/Saturation, Color Balance, Black & White, Photo Filter, Channel Mixer, Color
  Lookup, Selective Color: "<name> is unavailable in a Grayscale document."); Neural Colorize (a Gray layer
  holds no colour); Sky Replacement (its output's colour Curves); camera RAW layers and their fifteen tools
  ("Doctored keeps camera RAW layers on RGB documents only"; placing a RAW file too); `add_text` and
  `put_pixels` (8-bit byte tools); linear-light evaluation (`get/set_evaluation`: the Gray profile edit never
  rebinds the transfer); and every RGB-only family (`gray_document::refused_sentence`: "… Doctored offers it on
  RGB documents only.").

## Photoshop's measured facts

- **Luminance** (`gray-luminance/take1`, then `gray-mode-547/take1`, Photoshop 26.10): Mode ▸ Grayscale matches
  the PCS-`Y` rule at Dot Gain 20 % and Gray Gamma 2.2, 8 and 16 bits, max / mean residual 0.625 / 0.245 8-bit
  codes (gate 2 / 0.5; Photoshop dithers its 8-bit conversion). CONFIRMED.
- **K %**: Photoshop's sampler reads the complement within 0.39 points at 8 bits and 0.005 at 16. MEETS.
- **Blend modes**: 21 offered, the same at 8 and 16 bits under both spaces.
- **Disabled in Grayscale**: Vibrance, Hue/Saturation, Color Balance, Black & White, Photo Filter, Channel Mixer,
  Color Lookup, Selective Color, Desaturate, Match Color, Auto Color, Vanishing Point, Lens Flare, Oil Paint, NTSC
  Colors, HSB/HSL; Replace Color and Neural Filters stay enabled.
- **Gray files** (`gray-files-547`): Photoshop keeps a Grayscale Levels in record 1 and Blend If in slot 1; take1
  crashed Photoshop on the first writer (removed); take4 PASS (26 of 26 rasters within bound) for the core writer.
  A 32-bit Gray PSD embeds a linear (gamma 1.0) twin of its Gray profile.

## Shipped

- **1a** (`fa9c7b5b`): the model, New at 8/16/32, the gray opens, Mode ▸ Grayscale from RGB and CMYK and RGB Color
  back, `.rz`, CMYK phase 1's structure edits. Admission `scripts/gray-admission/measure.py`
  (`OPEN_BYTES_PER_PIXEL`, `TO_GRAY_BYTES_PER_PIXEL`, both 41).
- **1b** (`705509ee`): layer structure, Fill, the one-channel saves and exports (`EXPORT_BYTES_PER_PIXEL` 4).
- **1c and phase 2's take** (`fb2a97f1`): the Channels and Info panels, `get_precision`'s own lists, `render`
  channel gray, Export As; `gray-mode-547/take1`.
- **2a-ii, 2b** (`6e6e49e3`): painting and adjustments.
- **2c** (`5b4ede91`): filters, Fade, Actions and the retouch tools.
- **2d** (`833f7455`): layer effects, Blend If, alpha-channel targets.
- **3a** (`e68eec52`): gray PSD, PSB and layered TIFF open. **3b** (`7d1dc2f0` the import fix, `4e293c02` the core
  writer, `0f4e93a7` the export after take4 PASS). **3c** (`f48f53c1`): 32-bit gray files.
- **2e** (`27c06fc0`): Stroke…, Content-Aware Fill/Move, Remove, the 8-bit Art History Brush, HDR Toning, Import
  Channel, RGB → Gray with adjustment layers.
- **4** (`eefdb4e9`): geometry, the selected pixels, selections, Layer Via, Stamp Selected, the clipboard (RZCB v5,
  the one-channel pasteboard TIFF).
- **5a** (`2b0e275c`): Assign and Convert to Profile between Gray profiles, 8/16/32 Bits/Channel, 32-bit HDR
  Toning; admission plan/E 1.283–1.968, no constant raised.
- **5b** (`42bd2ad1`): Proof Setup, Proof Colors and Gamut Warning; Mode ▸ CMYK Color from Gray.
- **6a-i** (`71daa1b1`): type, shape and fill layers, paths and vector masks.
- **6a-ii** (`ee2a233b`): comps, links, snapshots, the legacy ops and Edit ▸ Clear; RGB/CMYK → Gray keeps artwork
  editable and carries comps.
- **6b** (`11878a3c`): Histogram, Apply Image, Calculations, Stack Modes and the stack tools, Merge to HDR,
  Merge Panorama, the model selections, the neural and generative tools.
- **The close** (2026-10-21): the tools no phase named, each given one fate (below); Place folded with no retained
  source (`GRAY_FAMILIES` `source_place`); RGB → Gray carries a Generative Layer; RGB's own errors for requests RGB
  rejects; this ADR. Tests `core/tests/gray_close.rs` (each against the RGB twin, exact); drive
  `scripts/verify_gray_mode.py` G78–G85.

## The close's fates

| Tools | Fate |
| :--- | :--- |
| `arrange_layer`, `distort_layer`, `auto_select_layer`, `find_layers`, `get_mask_properties`, `modify_scalar` | lifted: the RGB paths on R = G = B (a colour-plane target refuses) |
| `list_views`, `open_second_view`, `close_second_view`, `get_view`, `set_view`, `save_workspace`, `restore_workspace` | lifted: view state |
| `get_color_samplers`, `set_color_samplers` (and the Color Sampler tool) | lifted; rows report `gray_k_percent` |
| `get_file_info`, `set_file_info`, `get_metadata`, `set_resolution`, `set_ruler_origin`, `set_history_budget`, `recover_document`, `discard_recovery` | lifted: document state |
| `detect_horizon`, `list_lens_profiles`, `separate_color`, `save_proof_preset` | lifted: reads of the twin's R = G = B |
| `export_layers`, `export_pdf`, `contact_sheet`, `contact_sheet_from_sources` | lifted: the export paths (Export Layers one-channel with profile document) |
| `get_tone_map`, `set_tone_map` (Image ▸ Tone Map at 32 bits) | lifted: a float document's |
| `place_image`, `add_live_photo_layer` (Place Embedded/Linked, Live Photo) | lifted: folded, no retained source |
| the fifteen RAW and image-source tools (`GrayDocument.rawTools`) | refused for good |
| `add_text`, `put_pixels` | refused for good: 8-bit byte tools (`add_text_layer` adds type) |
| `get_evaluation`, `set_evaluation` | refused for good: linear-light evaluation is RGB's |
| `auto_color`, `red_eye`, `red_eye_auto`, `neural_colorize`, `sky_replacement` | refused for good (earlier phases) |

## Open items

None.
