# Lab Color document mode (ADR)

- **Status:** Accepted — open items remain ([#5](https://github.com/kpgalligan/doctored/issues/5)); the mode closed
  2026-10-22. [#1](https://github.com/kpgalligan/doctored/issues/1) (the colour-mode umbrella) stays open (see
  [cmyk-mode-status.md](cmyk-mode-status.md)). Grayscale is `designs/grayscale-mode.md`, Indexed Color
  `designs/indexed-mode.md`.
- **Originals:** [archive/lab-mode-status.md](../archive/lab-mode-status.md) (the working record at close) and
  [archive/lab-mode-shipped.md](../archive/lab-mode-shipped.md) (its shipped companion).
- **Takes:** `~/devel/doctored-fixtures/photoshop/lab-mode-547/take1`, `lab-blend-681/take2`,
  `lab-files-547/take2`, `lab-export-547/take1`, `lab-select-547/take1`, `lab-gray-547/take1`,
  `lab-cmyk-547/take1`, `lab-close-547/take1`; runners `scripts/photoshop-probe/lab_*_take.py` in the private
  archive (`kpgalligan/doctored-dev`).
- **Code:** `core/src/lab_*.rs`, `psd_import_lab.rs`, `psd_export_lab.rs`, `ffi_lab.rs`, `icc_builtin.rs`
  (`lab_twin_blob`); `app/Sources/LabDocument.swift`, `LabAnalysis.swift`, `LabAdjust.swift`, `LabInk.swift`,
  `LabDepthEdit.swift`, `LabClipboard.swift`, `LabProof.swift`, `EditorViewController+LabProfile.swift`,
  `AgentServer+LabColour.swift`, `DoctoredCore+Lab.swift`, `AgentCatalog+Lab.swift`,
  `EditorViewController+LabMode.swift`.
- **Template:** Grayscale document mode, phase 1a (`fa9c7b5b`): the same seams and gate shape.

## Context

Image ▸ Mode ▸ Lab Color at 8 and 16 bits: New, both conversions (RGB, Gray, CMYK), `.doctored`, the view,
layer structure, paint, adjustments, filters, retouch, effects, Lab TIFF/PSD/PSB in and out, geometry,
selections, clipboard, Bits/Channel, proof and the profile commands, type/shape/fill layers, comps, analysis,
multi-image, neural and generative tools, from the UI, over MCP and in Actions. Every tool the catalog offers is
admitted, refused for good by name, or (Select Similar and Grow at 8 bits) names its issue. No refusal names a
phase.

## Decisions in force

- **Storage is the RGB precision document holding L\*, a\*, b\* codes:** a `PrecisionDocument` whose `lab`
  policy (`LabPolicy`: depth, twin) is `Some`; u16 only, R, G, B = L\*, a\*, b\* in the ICC v4 16-bit Lab encoding
  (L\* = 100·v, a\*/b\* = 255·v − 128; a\* = b\* = 0 is 0x8080). 8 bits stores each code ×257 and every
  conversion into it rounds to that grid; paint, geometry and stack results may land off it until an 8-bit save.
  **No 32-bit Lab**.
- **The twin is the profile** of the document and every layer (`lab_twin_blob`: data `RGB `, PCS `Lab `, D50,
  identity tables), parsed as `RgbLut`; Normal compositing is the RGB compositor's linear mix of the codes.
  **The core makes the view:** `lab_convert::lab_display` into the built-in Display P3 at u16; the
  host draws Display P3 and ColorSync never decodes the twin. Panels never label codes as RGB: Channels shows the
  raw Lightness, a and b, Info reads L\*a\*b\*.
- **One conversion rule both ways:** each u16 pixel through `ColorPipeline`, clamped and quantized, alpha kept.
  RGB/Gray → Lab is relative colorimetric from the document profile, no BPC; Lab → RGB takes the destination's
  intent and BPC. CMYK ↔ Lab goes through 16-bit ProPhoto RGB (relative + BPC). Lab → Grayscale is each pixel's
  L\* alone through the Gray curve. 8/16 Bits/Channel rounds every plane to the grid (16 → 8) or only relabels.
  A record Lab cannot hold (a CMYK adjustment layer, a refused mode, effect or Blend If channel, a placed or
  warped layer) is named with `flatten_required`; type, shape and fill layers convert as pixels (the host
  re-renders the description); a Generative Layer converts as its pixels and its variations detach, as Convert
  to Profile detaches them on RGB (the close).
- **No document profile.** Proof Setup, Proof Colors and Gamut Warning proof the Display P3 view
  through the RGB proof; Convert to Profile is Image ▸ Mode's conversion into a CMYK, RGB or Gray profile;
  Assign Profile refuses for good (`disabledTools`, `PROFILE_EDIT_REFUSAL`, `NO_PROFILE_OPS`).
- **Files are measured.** PSD/PSB/layered TIFF: 8 bits P8, 16 bits L\* = 100v/65535 and a\*/b\* on Photoshop's
  15-bit grid; TIFF Photometric 8 measured at 8 and 16 bits, Photometric 9 from the spec. Lab files are written
  in the PSD's measured Lab form (colour mode 9, no ICC, adjustment layers as their stamps, a reported loss). The Lab
  TIFF save is TIFF 6.0 CIELab of the composite. `.doctored` word 107 (legacy `.rz` still read) with its `LAB `
  table; `DRDC_VERSION` unchanged. File ▸ Save keeps a Lab TIFF in place only while `LabDocument.saveLosses` is
  empty (Grayscale's rule): the core's `flat_state` list and the capture packets; the TIFF keeps alpha, paths,
  resolution and packets, and a spot channel, which the save refuses, is a line of its own.
- **The gate.** The core's `lab_document::admit_edit` (first in `ffi_precision_edit::request`) and
  `precision_capability::LAB_FAMILIES`; the host's `LabDocument`, asked first in `DocumentPrecision` and
  `validatePrecisionItem`. Admitted edits run the RGB kernel on the codes; every colour a request names
  (ink, fill, stroke, pad, Clouds) becomes codes by Mode ▸ Lab Color's rule (`prepare_colors`, `lut_ink`). Since
  the close every other request reaches its own parser and answers as on RGB; a scalar read of the layer names
  the targets Lab takes; nothing names a phase.
- **Blend modes are measured and composite by Lab arms.** `LAB_BLEND_MODES` (host `labModes`): the 19
  offered; `LAB_NOT_OFFERED` (Darken, Color Burn, Lighten, Color Dodge, Difference, Exclusion,
  Subtract, Divide) refuse by name. Every blend (layers, merges, paint, Fade, effects, Clone Stamp,
  Art History) routes through `lab_blend`.
- **Adjustments run the RGB kernel in a Lab form** (`lab_adjust`, `lab_tone`): Levels, Curves and
  Brightness/Contrast on Lightness (r, g, b settings = Lightness, a, b), Invert and Posterize per channel,
  Threshold black and white, Desaturate keeps L\*; the colour ops (Hue/Saturation, Color Balance, Photo Filter,
  Shadows/Highlights, Replace Color) through encoded 16-bit ProPhoto RGB, Color Lookup and Gradient Map through
  sRGB. Exposure, Vibrance, Black & White, Channel Mixer, Selective Color, HDR Toning and Auto Color refuse
  for good ("<name> is unavailable in a Lab Color document."; the first five `Unavailable` in `lab_adjust`'s
  table and the host's `LabAdjust`, HDR Toning in `LabAdjust`, Auto Color there and in
  `lab_adjust::admit_selected`), Doctored's RGB-only ops as such; Auto Tone, Auto Contrast and Equalize act on Lightness. RGB ↔
  Lab carries adjustment layers, channel Levels/Curves and Blend If by channel index.
- **Filters and retouch are per channel on the codes;** the five luma filters run in one-channel passes
  (`lab_channel_filter`); Dodge and Burn shape L\* alone, the Sponge scales chroma about a\* = b\* = 0; the
  History and Art History Brushes need a Lab state; Remove reads the P3 view; Red Eye corrects in ProPhoto.
- **Effects and patterns are authored sRGB,** converted at composite (`AuthoredColors::LabTwin`); Blend If on
  Lightness, a and b (Gray refuses). **Artwork is authored sRGB** (`lab_artwork`), rendered into the twin.
- **Geometry, selections, clipboard:** the RGB kernels on the codes; a Lab warp applies at once with no
  binding; sampled Color Range by the measured `RANGE_RULE`, the other ranges by their space; the Magic Wand
  and Paint Bucket (both depths) and Similar and Grow (16 bits) by `lab_select::wand_key`; Quick Selection
  ΔE76 on the codes; the model selections, the Magnetic Lasso and Detect Horizon read the Display P3 analysis
  composite; the clipboard keeps the codes (RZCB v6) and converts across modes by the Mode rules.
- **Channel math, stacks, multi-image:** Apply Image and Calculations blend each channel's codes alone; Stack
  Modes per channel; Merge to HDR through ProPhoto (no 32-bit Lab); Auto-Align fits CIE Y; Merge Panorama blends a
  Lab stack on L\* alone (`precision_auto_blend`'s Lab arm, kept for it).
- **Neural and generative tools: the view in, the Mode rule out** (`lab_neural`): the model or provider reads
  the Display P3 view and the result returns to codes by Mode ▸ Lab Color's rule.
- **Place renders through ProPhoto (the close):** `SourceEdit::Place` on Lab renders the asset into the
  built-in ProPhoto RGB at u16 and publishes it converted to codes, keeping no retained source; the plan prices
  the colour visit against ProPhoto plus the 8 B/px converted image and the pipeline's scratch. An asset
  already in the twin (Load Files into Stack's Lab clip) places unchanged. A camera RAW file refuses
  (`LabDocument.rawRefusal`).
- **Exports go through RGB in the core** (`lab_rgb::lab_to_rgb_image`; profile `document` writes sRGB): Export
  As, Export Layers, PDF; a contact sheet cell is the Display P3 view tagged Display P3; Separate Color's `rgb`
  is sRGB; the luminosity masks read Lightness.
- **Refused for good, by name:** the disabled items (`LabProof.disabledTools`: Assign Profile, Auto-Blend
  Layers; the adjustments above and the filters `lab_filter` marks `Unavailable`: Lens Correction, the Develop
  Filter and Difference Clouds); Sky Replacement (Doctored's sentence); the RAW tools, the 8-bit byte
  tools, linear-light evaluation and the 32-bit tone map (`LabDocument.rawTools`, `byteTools`,
  `evaluationTools`, `toneMapTools`); any other tool outside `LabDocument.tools` with "Doctored offers it on
  RGB documents only".

## Photoshop's measured facts

Photoshop 26.10.0.

- **Encoding** (`lab-mode-547/take1`): Mode ▸ Lab Color matches the sRGB → Lab rule, max/mean ≤ 0.753/0.260
  codes at 8 bits, ≤ 0.017/0.007 at 16 (CONFIRMED; `--judge` PASS).
- **Blend modes** (same take): 19 offered at 8 and 16 bits; the eight above not offered. Every arm within one
  8-bit code at both depths (`lab-blend-681/take2`).
- **Disabled in Lab** (`lab-mode-547/take1`, Image and Filter): 32 Bits/Channel; Exposure, Vibrance, Black &
  White, Channel Mixer, Selective Color, HDR Toning, Match Color; Auto Color; Camera Raw Filter, Lens
  Correction, Vanishing Point, Difference Clouds, Lens Flare, Oil Paint, Solarize, NTSC Colors, HSB/HSL.
- **Files** (`lab-files-547/take2`, `lab-export-547/take1`): the encodings above, VALID and PASS.
- **Color Range** (`lab-select-547/take1`): sampled ranges by C2 (per-channel L\*a\*b\* units); the Magic Wand
  fit no candidate at 16 bits; Select Similar unmeasured by script.
- **Wand, Similar, Grow** (`lab-select-682/take1`, `lab_select_682_take.py`; Similar and Grow run by
  `executeAction` of `Smlr`/`Grow` with no descriptor, smoke1): exact on every pixel, the wand by C6 (8-bit
  codes, halved cyclic differences) at 8 bits and C9 (C6 on bytes from Photoshop's 15-bit grid) at 16, both
  takes; Similar and Grow C9 at 16 bits; at 8 bits they fit nothing (best C6, 512/609 and 525/626 px over).
- **Lab → Grayscale** (`lab-gray-547/take1`): L\* direct, max |r| 0.267 code at 8 bits, 0.011 at 16.
- **Lab → CMYK and the profile items** (`lab-cmyk-547/take1`): MISS, max/mean 6.06/1.04 codes at 8 bits,
  5.88/1.02 at 16; Assign Profile disabled, Convert to Profile and the proof items enabled.
- **The close's layered walk** (`lab-close-547/take1`, `lab_close_take.py`; Edit, Layer, Select, Filter, File,
  Image on a two-layer 16-bit RGB document and the same converted to Lab, walked with both layers targeted and
  with one): VALID. Auto-Align enabled in Lab; Auto-Blend Layers disabled in Lab; Sky Replacement disabled in
  Lab; Select ▸ Subject, Sky and Neural Filters enabled; Focus Area enabled; Place Embedded/Linked, File Info
  and Distort enabled; Photomerge, Layers to Files and Contact Sheet II have no name in `menuBarInfo` (NOT
  FOUND). Also disabled in Lab: Assign Profile, Layer ▸ Copy CSS/SVG, the five Lab-disabled adjustment layers, and the
  Image/Filter items above. Nothing is enabled only in Lab. `lab_close_photoshop.rs` pins the table.

## Shipped

- **1** (`d4cf7e32`): the model, New 8/16, Mode ▸ Lab Color from RGB, Gray and CMYK and back, the view, `.rz`.
  **1b** (`fc013245`): structure, Fill, alpha channels, the Lab TIFF, exports. **1c** (`740003b7`): panels and
  readouts. **Phase 2's take** (`e6c31f84`): encoding, blend gate, menu walk.
- **2a/2b** (`d009826a`): paint, adjustments. **2c** (`38f5a091`): filters, Fade, Auto, Equalize. **2d**
  (`ed2f675d`): colour ops through ProPhoto, plane statistics, mask filters, samplers. **2e** (`fb60e994`):
  retouch, effects, the Lab ink, plane targets. **2f** (`be2d0252`, `11184e3d`, `71c6b5d6`): luma passes, Blend
  If, patterns, Stroke…, Remove, Art History, Red Eye.
- **3** (`c47305cb`): the Lab arms of every mode. **3a** (`4b67b0a3`): Lab files open. **3b**
  (`1e20fde8`): Export as PSD.
- **4a** (`06acd034`), **4a-ii** (`da5a9431`), **4b-i** (`aa55803b`), **4b-ii** (`9200efcd`), **4c**
  (`8e7fa3ff`): geometry, warps, selected pixels, selections, sampled Color Range, the clipboard.
- **5a** (`e88d8164`): Lab → CMYK, Bits/Channel. **5a-ii** (`7be03800`): Lab → Grayscale. **5b** (`5f2f3892`):
  proof, Convert to Profile, Assign refused.
- **6a-i** (`e0d670c5`): type, shape, fill layers, paths. **6b-i** (`bf95b803`): Histogram, channel math,
  stacks, Merge to HDR, Color Range ranges. **6a-ii** (`9aa9a847`): comps, links, snapshots, legacy ops, Clear.
  **6b-ii** (`50de4191`): Auto-Align, Auto-Blend, Merge Panorama, the selections. **6b-iii** (`d6d571ea`): neural and
  generative tools; Sky Replacement refused.
- **The close** (2026-10-22): `lab_close_take.py` take1; every tool no phase named given one fate (below); the
  core gate answers as on RGB and `refused_sentence` names no phase; `import_channel` and the legacy `rasterize`
  admitted; Place through ProPhoto (`LAB_FAMILIES` `source_place`); a Generative Layer converts; Auto-Blend
  Layers refused for good; a contact sheet cell tagged Display P3. Tests `core/tests/lab_close.rs`,
  `lab_close_photoshop.rs`; drive `scripts/verify_lab_mode.py` L93–L98.
- **The Wand, Paint Bucket, Similar and Grow**: the Magic Wand and Paint Bucket at 8 and 16 bits and Select
  Similar and Grow at 16 by `lab_select`'s key; Similar and Grow at 8 bits refuse naming #5 (`SELECTION_REFUSALS`). Tests
  `core/tests/lab_wand.rs`, `lab_wand_photoshop.rs`; drive L99–L101.

## The close's fates

| Tools | Fate |
| :--- | :--- |
| `arrange_layer`, `auto_select_layer`, `find_layers`, `get_mask_properties`, `merge_group` | lifted: the RGB paths on the codes |
| `list_views`, `open_second_view`, `close_second_view`, `get_view`, `set_view`, `save_workspace`, `restore_workspace` | lifted: view state |
| `get_file_info`, `set_file_info`, `get_metadata`, `set_resolution`, `set_ruler_origin`, `set_history_budget`, `recover_document`, `discard_recovery` | lifted: document state |
| `distort_layer`, `modify_scalar`, `invert_channel`, `add_luminosity_masks` | lifted: the RGB kernels on the codes; a red/green/blue target refuses; the masks read Lightness |
| `detect_horizon`, `select_magnetic` (and the Magnetic Lasso), `list_lens_profiles`, `separate_color` | lifted: the Display P3 analysis composite; Separate Color's rgb is sRGB |
| `export_layers`, `export_pdf`, `contact_sheet`, `contact_sheet_from_sources` | lifted: the Lab → RGB export; a sheet cell is the P3 view |
| `place_image`, `add_live_photo_layer` | lifted: rendered into ProPhoto, published as codes, no retained source |
| `import_channel`, the legacy `rasterize` op | lifted in the core gate |
| `auto_blend_layers` | refused for good (`LabProof.disabledTools`) |
| the fifteen RAW and image-source tools (`LabDocument.rawTools`) | refused for good |
| `add_text`, `put_pixels` | refused for good: 8-bit byte tools |
| `get_evaluation`, `set_evaluation` | refused for good: linear-light evaluation is RGB's |
| `get_tone_map`, `set_tone_map` | refused for good: a 32-bit document's |
| `depth_blur`, `auto_color`, `hdr_toning`, `assign_profile`, `sky_replacement` | refused for good (earlier phases) |
| `select_magic_wand` (and the Paint Bucket) | lifted: `lab_select` at both depths |
| `select_similar` (and Grow) | lifted at 16 bits; at 8 bits refuse naming #5 |

## Open items

- `OPEN` [#5](https://github.com/kpgalligan/doctored/issues/5) — Lab Select Similar and Grow at 8 bits fit no
  declared rule; Photoshop selects less than the references' box over a dithered field.
- `DROPPED` (2026-10-30) — confirming Lab → CMYK against a take (`lab_cmyk_photoshop.rs` records its MISS): it
  would re-measure a shipped conversion whose CMS residual the owner accepted (`rgb-cmyk-684/take1`).
