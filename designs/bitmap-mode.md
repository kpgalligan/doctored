# Bitmap document mode (ADR)

- **Status:** Accepted, closed 2026-10-23. Bitmap closes here; [#1](https://github.com/kpgalligan/doctored/issues/1)
  (the colour-mode umbrella) stays open for CMYK (spot channels, PDF/X; `designs/cmyk-mode-status.md`).
  Grayscale is `designs/grayscale-mode.md`, Lab `designs/lab-mode.md`, Indexed Color
  `designs/indexed-mode.md`, Duotone `designs/duotone-mode.md`, Multichannel `designs/multichannel-mode.md`.
- **Originals:** [archive/bitmap-mode-status.md](../archive/bitmap-mode-status.md) (the working record at close)
  and [archive/bitmap-mode-shipped.md](../archive/bitmap-mode-shipped.md) (its shipped companion).
- **Takes:** `~/devel/doctored-fixtures/photoshop/bitmap-mode-547/` `smoke1`, `fit1`, `smoke2`, `take1`, `walk1`,
  `walk2` and `halftone1`; runner `scripts/photoshop-probe/bitmap_take.py`.
- **Code:** `core/src/bitmap_document.rs`, `bitmap_method.rs` (kernel), `bitmap_convert.rs`, `bitmap_native.rs`,
  `bitmap_codec.rs` (kernel), `bitmap_io.rs`, `bitmap_export.rs`, `bitmap_psd.rs`, `bitmap_edit.rs`,
  `bitmap_tools.rs`, `ffi_bitmap.rs`; `app/Sources/BitmapDocument.swift`, `DoctoredCore+Bitmap.swift`,
  `BitmapModeSheetController.swift`, `EditorViewController+BitmapMode.swift`, `AgentCatalog+Bitmap.swift`,
  `EditorViewController+CmykMode.swift`'s bitmap arms.
- **Template:** Indexed Color phase 1a (`451c329f`): the same seams and gate shape; Grayscale is the source mode.

## Context

Image ▸ Mode ▸ Bitmap: one 1-bit plane converted from 8-bit Grayscale by five methods and back to Grayscale
with a Size Ratio; the 1-bit PNG, TIFF, BMP and PSD/PSB files; painting in black and white and the
geometry, each landing back on the plane by 50% Threshold's rule; from the UI, over MCP and in Actions. Every tool
is admitted or refused for good by name; no refusal names a phase.

## Decisions in force

- **Storage is 8-bit Gray of one layer; the packed plane is the authority.** The 8-bit Gray `PrecisionDocument`
  (the source's Gray ICC and its RGB twin) of ONE opaque raster layer, "Bitmap", canvas-sized at the origin, each
  pixel black or white, plus `bitmap: Some(BitmapPolicy)`: the packed plane, `ceil(w/8)` bytes a row, the leftmost
  pixel in the most significant bit, 1 white, pad bits 0 (12.5 MB at `MAX_PIXELS`). The layer is the plane's render,
  so the view, `render`, thumbnails and readouts need no Bitmap arm. No alpha channel; guides, samplers, paths, the
  light and metadata carry. The core's `is_gray()` is true and the host's `isGray` false (`color_mode` "bitmap"):
  every core gate asks `is_bitmap()` before `is_gray()`, every host gate asks Bitmap before Indexed, Lab, Gray and
  CMYK.
- **To Bitmap from 8-bit Grayscale** (and from Duotone through its Grayscale, below). A Bitmap document refuses
  "The document is already in Bitmap mode."; another mode "Bitmap converts an 8-bit Grayscale document; convert
  to Grayscale first."; 16/32-bit Gray "…; convert to 8 Bits/Channel first."; more than one
  plain layer `flatten_required` ("Bitmap holds one layer: flatten the image.", the host asks "Flatten layers?").
  Each pixel is read as 8-bit codes over white (`indexed_palette::prepare`) and its gray goes through the method.
- **The Output resolution decides each axis on its own.** Omitted or equal to the axis's ppi (as `f32`), the axis
  keeps its pixels; otherwise it becomes `max(1, round(n·r/ppi))` pixels at `r` (1…30000), resampled first by
  Image Size's Bicubic. The sheet sends no `resolution` while its field holds the document's value.
- **The methods** (`bitmap_method`; `g` an 8-bit code, 1 white):
  - 50% Threshold: white iff `g ≥ 128` (`THRESHOLD_WHITE`).
  - Pattern Dither: the 8 × 8 table `PATTERN_THRESHOLDS` at `T[y mod 8][x mod 8]` from the origin, white iff
    `g ≥ T`: PT8-MID, each entry `⌊(lo + hi)/2⌋` of the gray interval smoke1 left at that position (widths ≤ 7).
    It is not Bayer's order; `bayer` and `PATTERN_ORDER` stay for `indexed_dither`.
  - Diffusion Dither: `DIFFUSION`, Sierra Lite (2/4 ahead, 1/4 below-behind, 1/4 below), serpentine, unclamped,
    white iff `w ≥ 128`, error dropped past the edges, f64 in a fixed order (`DiffusionRule`; Floyd–Steinberg's
    master copy stays `indexed_palette::Diffusion`).
  - Halftone Screen: `frequency` lines per inch of the output (1…999.999, 53), `angle` (−180…180°, 45), `shape`
    (round, ellipse, line, square, cross, diamond; round): Adobe's predefined spot functions (ISO 32000-1
    §10.5.3) over the screen turned counterclockwise, cells at the corner; white iff `g > 0.5 + 254·F`, `F` the
    share of a 256² grid of cell points whose spot is below the pixel's (AREA order, 512 KiB). `HALFTONE` holds
    area/turn/centred.
  - Custom Pattern: `style_pattern_fill::parse_pattern`'s pattern object (the four presets or an inline tile);
    each texel over white, its `blend::luma_code`, is the threshold tiled from the origin: white iff `g >` it.
  - A method's key beside another method refuses ("mode option frequency applies to method halftone", "mode
    option pattern applies to method custom"); ranges refuse by sentence; the host reads `frequency` and `angle`
    as numbers or strings and the core owns every range. The options JSON is bounded at 2 MiB
    (`ffi_bitmap::OPTIONS_MAX`).
- **Grayscale back with a Size Ratio.** Ratio 1 is the storage (`without_bitmap`), exact. Ratio `r` in 2…16
  averages each `r × r` block onto `ceil(w/r) × ceil(h/r)` at `ppi/r`: `(510·W + n) / (2n)`, the chrome moving as
  Image Size moves it. Every other mode refuses: "<Mode> is unavailable in a Bitmap document; convert to
  Grayscale first."
- **`.rz` word 110** in the CMYK slot (Duotone holds 109), table `BTMP` after GRAY and before `LIDS`: u32 schema 1,
  u32 width, u32 height, the packed rows. Reader and writer run `bitmap_native::check` (8-bit Gray, one canvas
  raster at the origin, no alpha channel, pad bits 0, body equal to the render). `DRDC_VERSION` stays 89.
- **The 1-bit files.** `bitmap_codec`'s header-only claim (`dr_path_bitmap_source`, after Indexed): PNG colour type
  0 depth 1 without `tRNS` or `acTL`; a classic TIFF of one 1-bit sample, Photometric 0 or 1, FillOrder 1,
  lossless; BMP 1 bpp BI_RGB with a black-and-white palette. An ICC must be Gray; otherwise sGray for PNG `sRGB`,
  else the working Gray. Pad bits are cleared once (`clear_pad_bits`), WhiteIsZero TIFF and white-first BMP
  inverted once (`flip_polarity`); `precision rgba8` keeps the old RGB open. Kept export (profile `document`):
  PNG and TIFF BlackIsZero with the Gray ICC and metadata, BMP neither, `bit_depth` 1; JPEG is the Gray render;
  other formats refuse by sentence. PSD/PSB colour mode 0: depth 1, 1 is black, Raw or RLE in, RLE out, no
  profile written, no layers ("A Bitmap file holds no layers; open its stored composite."); the import report
  frames the composite (`composite_frames`) without decoding it.
- **The gate: three tiers, no phase.** `bitmap_document::admit_edit` (before the Gray gate) routes a request
  `BitmapRoute::Edit` (`bitmap_edit`) or `BitmapRoute::Shared` (`Edit::parse`). Edit: `paint_blend`,
  `bucket_fill`, `stroke_layer_edge`, `gradient` and `retouch_clone` with `blend_mode` in `BLEND_MODES` (Normal 0,
  Darken 6, Lighten 7; else `BLEND_REFUSAL`: "The Brush, Fill, Stroke, the
  Paint Bucket, the Gradient and the Clone Stamp paint in Normal, Darken or Lighten mode in a Bitmap
  document."), `paint`, `erase`, `erase_native`, `crop`, `canvas_size`, the quarter turns,
  the flips, `straighten`, `resize`, `affine`, `perspective`, `affine_set`, `perspective_set`, `move_selected` and
  `perspective_crop`. Shared (`SHARED`, no bit moves): `rename_layer`, `layer_label`, `layer_lock`, `liquify` and
  a `selection` of any kind but `magic_wand`, `color_range`, `similar` and `refine_edge`. `admit_selected` takes
  `liquify` only. Every other op refuses by `refused_sentence`: "<op> is not available in Bitmap mode."
  (`DISABLED_OPS`: Indexed's list but its retouch, Gradient, Liquify and layers-paste ops (`content_aware_move`,
  `dodge_burn`, `gradient`, `heal`, `liquify`, `paste_layers`, `red_eye`, `retouch_brush`, `retouch_heal`,
  `spot_heal`), plus `magic_wand`, `transform_selected`, `color_assign`, `color_convert`, `profile`,
  `rotate_canvas`, `color_range`, `similar` and `paste`); "<op> needs more
  than one layer, and a Bitmap document holds one." (`ONE_LAYER`); else "<op> is unavailable in a Bitmap
  document: Doctored offers it on Grayscale and colour documents only; convert to Grayscale first."
  `PrecisionDocument::duplicate_layer` refuses a Bitmap document. `precision_capability::BITMAP_FAMILIES` is
  the family table. The host's `BitmapDocument` is the same gate:
  `tools`, `selectors` and `allows` (the selections, the view, Brush, Eraser, Paint Bucket, Crop, Move, Gradient,
  Clone Stamp, History Brush, Quick Selection, Magnetic Lasso, Eyedropper, Color Sampler, Ruler, Pen,
  Perspective Crop); `disabledTools`, `oneLayer`, `channelTools` (`channelRefusal`), `save_selection`
  (`saveSelectionRefusal`) and every other tool `unoffered`; `save_copy` takes rz, png, tif/tiff, bmp or
  jpg/jpeg (`saveRefusal`), `crop`'s `fill` other than `background` refuses, a copy move `copyRefusal`, `rotate`
  at a filtered angle `arbitraryRotationRefusal`, `set_layer_properties` past the name `layerPropertiesRefusal`,
  `render` past `composite`/`bitmap` `renderRefusal`. `AgentServer+Registry` requires every listed name to be a
  registered tool.
- **The edits through the adapter** (`bitmap_edit`, `bitmap_tools`; documented, unmeasured). Each runs on the Gray
  storage through its existing kernel, then `republish` puts each pixel back by ONE rule,
  `bitmap_method::white_over`: white iff `gray·alpha + bg·(255 − alpha) ≥ 128·255`. Antialiasing is thresholded,
  so a soft brush lands hard-edged. A paint's ink snaps to black or white by its gray at full alpha, opacity
  kept (under 50 % changes nothing); the Gradient's stops are not snapped, so the ramp lands hard where it
  crosses 128. The background bit: a Straighten's own `background`, else `indexed_background` (the UI's swatch),
  else white. A result equal in plane, canvas, resolution and chrome is NULL; otherwise the layer is re-rendered
  and the document rebuilt (`indexed_paint::rebuild`'s shape). The resampled geometry (Image Size, the Crop
  tool's straighten, Move) and the selected pixels' moves run their own sampler on the gray, uncovered area
  taking the background bit; a whole-pixel move or a nearest sampler keeps each bit. Cut is the Gray Cut then
  `republish`. Perspective Crop's outside takes the background bit. The Clone Stamp (in place; `output`
  `new_layer` refuses as `add_layer`), Liquify and the History Brush (`bitmap_tools`, over white) copy bits where
  their coverage is full. Inverse, Modify and Transform Selection change the host's selection only.
- **What moves no bit.** Copy reads the Gray storage (a Gray payload of 0 and 255). Quick Mask, Quick Selection,
  the Magnetic Lasso and the source selections edit the selection only. `render` draws the composite for
  `composite` or `bitmap`. Straighten Layer is the straightened crop (`BitmapDocument.straightenWindow`: the Crop
  tool's `CropFit.fitted` of the canvas at the line's `straightenRotation`, `fit_crop`'s angle), one step whose
  reply is `crop`'s with `rotation`. The layer row takes its name, label and lock, kept in `.rz`. Info reads K
  (0 % white, 100 % black), as Grayscale's.
- **The Magic Wand and a copy move refuse for good.** The Magic Wand is in `DISABLED_OPS` (above); a copy move
  "A copy move makes a second layer, which a Bitmap document cannot hold."
- **Plans.** To Bitmap: Flatten Image's render when it flattens + the larger of the resample's geometry plan and
  output pixels × `TO_BITMAP_BYTES_PER_PIXEL` (41) + the method's scratch (Sierra Lite's two error rows, the
  512 KiB spot order, the tile) + bookkeeping. To Gray: output pixels × 41 + bookkeeping. Open: pixels ×
  `bitmap_io::OPEN_BYTES_PER_PIXEL` (42) + the file. Kept export: pixels × `bitmap_export::EXPORT_BYTES_PER_PIXEL`
  (2); PSD export: the file's worst case + the inverted plane. An edit: the inner plan + output pixels ×
  `bitmap_edit::BITMAP_EDIT_BYTES_PER_PIXEL` (34; declared 31, raised after `crop-extend-bitmap` missed). Held by
  `scripts/bitmap-admission/measure.py`; a miss raises the case's own term only.

## Duotone and Bitmap

Duotone → Bitmap is built (`designs/duotone-mode.md`): Duotone's Grayscale rule (the inks rendered into the gray),
then this mode's conversion through the Bitmap sheet (`duotone_gray::convert_duotone_to_bitmap`, host
`CmykModeConversion.duotoneToBitmap`), with Bitmap's blockers and its 8 Bits/Channel step, one undo step. It is
composed and unmeasured. Bitmap → Duotone stays refused.

## The leaf rule

`rotate` at an angle that resamples refuses ("Arbitrary rotation is not available in Bitmap mode.") and
`rotate_canvas` is in `DISABLED_OPS`; the quarter turns and flips stay. `depth_blur` refuses. Move
and Image Size keep their admissions in the gate above, Free Transform and Transform Again stay refused
(`disabledTools`; the core's `transform_selected` in `DISABLED_OPS`), and the Crop tool's straighten is admitted
with Crop.

## Known differences

- **Lens Blur is not built.** `apply_filter` refuses by `disabledTools`, and `filter` `lens_blur` by "Lens Blur
  on a Bitmap document is not built in Doctored; convert to Grayscale first." (`lensBlurRefusal`).
- **Halftone Screen is declared (AREA-CCW-CORNER) and unmeasured for good.** Two Photoshop sessions (smoke1,
  halftone1) hung on the conversion (−1712) with different descriptors, and what else the HlfS descriptor needs
  cannot be read without a ScriptListener; `bitmap_photoshop_take`'s halftone test SKIPs with that reason.
- **Diffusion is Sierra Lite and not matched** (69.9 % on take1): Photoshop's diffusion is in no kernel tried and
  varies between sessions, so there is no further fit.
- **Save Selection is not built.** A Bitmap document holds no alpha channel: "Save Selection on a Bitmap
  document is not built in Doctored: a Bitmap document holds no alpha channel; convert to Grayscale first."
  The alpha-channel tools and Load Selection refuse by `channelRefusal`.
- **Liquify's workspace preview** shows the gray proxy (the warp unthresholded); Apply thresholds it.
- **Duotone → Bitmap is composed and unmeasured** (above).
- **Custom Pattern is unmeasured by design** (Photoshop's patterns are its own).

## Photoshop's measured facts

Photoshop 26.10.0, `bitmap_take.py`, every gate declared before its session, under the probe lock.

- **smoke1**: the −1712. `#Lns` resolved to no Photoshop unit, so the HlfS `Frqn` was unreadable and the Halftone
  Screen dialog opened under DialogModes.NO. Fixed: `Frqn` in `#Rsl` and an ID guard.
- **fit1** (offline, smoke1's TIFFs): Pattern fits an 8 × 8 threshold table at all 64 positions (widths ≤ 7;
  PT8-MID, LO and HI 16384 / 16384, Bayer 8 8876); Diffusion's best of 64 kernels LITE-SERP-FREE-128, 9982 / 16384.
- **take1** (scene2, 256 × 64, `g = (⌊9x/7⌋ + 13y) mod 256`): Threshold CONFIRMED (TH128 16384 / 16384); Pattern
  PASS (PT8-MID 16253 / 16384 = 99.20 %; LO 16183, HI 16143, Bayer 8 10655); Diffusion MISS-FITTED
  (LITE-SERP-FREE-128 11460 on D and D2, 69.9 %, against Floyd–Steinberg's 11250; smoke2's D differs, 11378
  equal). `core/tests/bitmap_photoshop_take.rs` replays each count.
- **walk2** (walk1 equal but for its unread counts; 495 items each): DISABLED in Bitmap: Edit ▸ Free Transform and
  Transform, Perspective Warp, Assign and Convert to Profile, Image ▸ Trim, Apply Image, Calculations, Layer ▸
  New Layer, Layer via Copy and Cut, Duplicate Layer, New Fill Layer, Select ▸ Color Range, Subject, Sky, Select
  and Mask, Similar and Grow, Neural Filters. ENABLED: Cut/Copy/Clear, Fill, Stroke, Image Size, Canvas Size,
  Image Rotation (Arbitrary excepted), Crop, Select ▸ Modify, Transform Selection, Quick Mask, Save Selection,
  Liquify, Lens Blur. Fill took Normal, Darken, Lighten and Dissolve (the other 23 refused): a black fill over
  (40, 8, 120 × 40) left 10745 black pixels in Normal and Darken and 8330 in Lighten, each the W3C prediction. The
  wand CONFIRMED refused ("does not work with Bitmap images").
- **halftone1**: STOPPED. With `Frqn` in `#Rsl` and every ID registered, the conversion still hung (−1712) and
  Photoshop stayed on the dialog after the quit; it was left for the owner and halftone2 was not run.

## Admission

`DR_VERIFY_APP=build/Build/Products/Release/Doctored.app python3 -B scripts/bitmap-admission/measure.py
--runs 3` (24 MP; plan ≥ ⌈1.25 × max E⌉; 1b's run for the conversions, 1c's (`--cases
open-bitmap-png,open-bitmap-tiff,open-bitmap-psd,export-bitmap-tiff,export-bitmap-psd`) for the files, 2a-i's
(`--cases brush-bitmap,rotate90-bitmap,crop-extend-bitmap`) for the edits at 34 B/px, and 2a-ii's (`--cases
rotate5-bitmap`, then `--cases transform-bitmap`) for the resampled geometry, unchanged at 34; 2b's for
`to-bitmap-diffusion`, whose scratch is now two error rows; 2b retired the two 2a-ii cases with their tools):

| Case | Plan | max E | Ratio |
|---|---:|---:|---:|
| `to-bitmap-threshold` | 985,048,576 | 771,785,256 | 1.276 |
| `to-bitmap-diffusion` (2b's Sierra Lite, `--cases to-bitmap-diffusion`; 1.320 with 1b's memo) | 985,144,576 | 770,605,632 | 1.278 |
| `to-bitmap-resample` (1.25 × ppi; 2.108 before 1b's fix) | 1,572,463,008 | 1,208,960,056 | 1.301 |
| `to-bitmap-halftone` (53 lpi, 45°, round) | 985,572,864 | 771,260,968 | 1.278 |
| `to-bitmap-custom` (inline 512 × 512 tile) | 985,310,720 | 778,338,904 | 1.266 |
| `to-gray-from-bitmap` | 985,048,576 | 576,438,680 | 1.709 |
| `to-gray-ratio-4` | 62,548,576 | 38,567,960 | 1.622 |
| `open-bitmap-png` (1c) | 1,009,071,434 | 771,916,328 | 1.307 |
| `open-bitmap-tiff` (1c) | 1,012,048,710 | 771,686,928 | 1.311 |
| `open-bitmap-psd` (1c, RLE) | 1,012,080,616 | 771,473,936 | 1.312 |
| `export-bitmap-tiff` (1c, kept) | 49,048,576 | 6,209,536 | 7.899 |
| `export-bitmap-psd` (1c, flattened) | 7,080,644 | 3,489,792 | 2.029 |
| `brush-bitmap` (2a-i, size 200) | 1,837,516,356 | 595,853,768 | 3.084 |
| `rotate90-bitmap` (2a-i) | 1,397,269,816 | 772,538,920 | 1.809 |
| `crop-extend-bitmap` (2a-i, 7000 × 5000, Delete Cropped off; 1.168 at 31 B/px) | 1,196,294,406 | 934,445,728 | 1.280 |
| `rotate5-bitmap` (2a-ii, 5° bicubic; the case and arbitrary rotation gone since 2b) | 1,786,281,238 | 918,733,472 | 1.944 |
| `transform-bitmap` (2a-ii; the case and the tool gone since 2b) | 1,632,682,566 | 772,883,008 | 2.112 |

## Shipped

- **1a** (`4d226f92`): the model, Mode ▸ Bitmap… from 8-bit Grayscale (Threshold, Diffusion, Output resolution),
  Grayscale back with a Size Ratio, `.rz` word 110, the closed gate, `convert_mode`, Actions, the catalog.
- **1b** (`aaf2593e`): Pattern Dither, Halftone Screen and Custom Pattern; the resample plan's overlap fixed;
  `bitmap_take.py`, whose smoke STOPPED on a −1712.
- **1c** (`f7f07122`): the 1-bit PNG, TIFF, BMP and PSD/PSB open and write; JPEG through the Gray render; the
  exports admitted.
- **2a-i** (`baf92c05`): the Brush, Eraser, Fill, Stroke, Paint Bucket and Clear, and Crop, Canvas Size, the
  quarter turns and the flips through `republish` and `white_over`.
- **2a-ii** (`25eed3cd`): Image Size, the Crop tool's straighten, Move, the selected pixels' moves, the
  selection's edits and Cut; the Magic Wand and a copy move refused; Info reads K.
- **2b** (`e2dd2552`): take1 and the walk: the fitted Pattern table, Sierra Lite, Threshold confirmed; the refusals
  `transform_selected`, `color_assign`, `color_convert` and `profile` added to `DISABLED_OPS`, Darken and
  Lighten; the leaf rule; Halftone Screen unmeasured for good.
- **3** (`b3069fe6`): the Gradient, the Clone Stamp, the History Brush, Liquify, Perspective Crop, Copy, Quick
  Mask, Quick Selection, the Magnetic Lasso, the render channels, the layer row and Straighten Layer; every other
  tool refused for good in three tiers.
- **The close** (this batch): the docs condensed here. Drive `scripts/verify_bitmap_mode.py` B31 calls the
  admitted tools with valid arguments (resolution, File Info, ruler origin, history budget, Fit Crop, the
  Magnetic Lasso, samplers, paths, snapshots, views, workspaces, proofs, PDF, the contact sheets and an Action's
  flip) and each reaches the core; the one error, `save_proof_preset` over the working CMYK profile ("A proof
  preset needs a profile that came from a file…"), is a Gray twin's word for word. B33 sweeps the whole catalog
  on a Bitmap document and pins each refusal's class; no refusal names a phase. No gap was found.

## Open items

- None for Bitmap. What remains of [#1](https://github.com/kpgalligan/doctored/issues/1) is CMYK's (spot
  channels, PDF/X).
