# Bitmap document mode (#547): shipped

The settled record of the shipped Bitmap phases, moved verbatim out of
[bitmap-mode-status.md](bitmap-mode-status.md) (the open phase's status doc, which keeps the head, the
contract of the phase not yet shipped, the measured Photoshop take and walk, the admission rows and open
items).

## Decisions in force

1. **Storage is 8-bit Gray of one layer; the packed plane is the authority.** A Bitmap document is the 8-bit
   Gray `PrecisionDocument` (gray policy `Some`, depth 8, the source's Gray ICC and its RGB twin) of ONE raster
   layer, "Bitmap", canvas-sized at the origin, opaque, each pixel `[0, 0, 0, 65535]` or `[65535; 4]`, plus
   `bitmap: Some(BitmapPolicy)`: the packed 1-bit plane, `ceil(w/8)` bytes a row, the leftmost pixel in the most
   significant bit, 1 white, 0 black, pad bits 0. The layer is the plane's render (Indexed's rule), so the view,
   `render`, thumbnails and the readouts need no Bitmap arm. Packed because its files (decision 10) are packed
   rows and it costs `ceil(w/8)·h` bytes (12.5 MB at `MAX_PIXELS`). No alpha channel; guides, samplers, paths,
   the light and metadata carry. The core's `is_gray()` is true on a Bitmap document and the host's `isGray` is
   false (it reports `color_mode` "bitmap", keeping the `gray` key): every core gate asks `is_bitmap()` before
   `is_gray()` (since 1c also `doc_export`'s and the PSD report and save arms), and every host gate asks Bitmap
   before Indexed, Lab, Gray and CMYK.
2. **To Bitmap from 8-bit Grayscale only.** A Bitmap document refuses "The document is already in Bitmap
   mode."; any other mode, a Duotone document included, "Bitmap converts an 8-bit Grayscale document; convert
   to Grayscale first." (and Duotone refuses a Bitmap document by its own source sentence);
   16- and 32-bit Gray "…; convert to 8 Bits/Channel first.". Anything but one plain layer answers
   `flatten_required` ("Bitmap holds one layer: flatten the image.") and the host asks "Flatten layers?". Each
   pixel is read as 8-bit codes, put over white (`indexed_palette::prepare` without Transparency) and its gray
   goes through the method.
3. **The Output resolution decides each axis on its own.** Omitted, or equal to an axis's stored ppi (as
   `f32`), the axis keeps its pixels and ppi; otherwise the axis becomes `max(1, round(n·r/ppi))` pixels at `r`
   ppi, `r` finite in 1…30000. A changed canvas is resampled first by Image Size's own Bicubic
   (`precision_geometry`'s `Resize`). The sheet sends no `resolution` while its Output field holds the
   document's value.
4. **The methods** (`bitmap_method`; gray `g` an 8-bit code, 1 white; since 2b measured by take1, decision 9):
   - 50% Threshold: white iff `g ≥ 128` (`THRESHOLD_WHITE`), CONFIRMED by take1 on every pixel.
   - Pattern Dither: the 8 × 8 threshold table `PATTERN_THRESHOLDS` read at `T[y mod 8][x mod 8]` from the
     canvas origin, white iff `g ≥ T`: the take's PT8-MID, each entry `⌊(lo + hi)/2⌋` of the gray interval
     1b's smoke1 left at that position (widths ≤ 7), PASS on take1 at 99.20 %. It is not Bayer's order;
     `bayer` and `PATTERN_ORDER` stay for `indexed_dither` (whose doc still calls the Bayer matrix Bitmap
     Pattern Dither's master copy).
   - Diffusion Dither: `DIFFUSION`, Sierra Lite (2/4 ahead, 1/4 below-behind, 1/4 below), serpentine, unclamped,
     white iff `w ≥ 128`, error dropped past the edges, f64 in a fixed order (`DiffusionRule`; Floyd–Steinberg's
     master copy stays `indexed_palette::Diffusion`). The take's MISS-FITTED kernel: disclosed as NOT matching
     Photoshop's (69.9 % on take1), whose diffusion is in no kernel tried and differs between sessions.
   - Halftone Screen: `frequency` lines per inch of the OUTPUT (1…999.999, 53), `angle` (−180…180°, 45) and
     `shape` (round, ellipse, line, square, cross, diamond; round), Adobe's predefined spot functions
     (ISO 32000-1 §10.5.3) over the cell coordinates `s, t` of the screen turned counterclockwise by the angle,
     cells at the corner; white iff `g > 0.5 + 254·F`, `F` the share of a 256² grid of cell points whose spot is
     below the pixel's (AREA order, sorted once a conversion: 512 KiB). `HALFTONE` holds area/turn/centred.
     Rasterize's declared rule, unmeasured for good in this phase (RULING-1): 1b's smoke1 and 2b's halftone1
     both hung on Photoshop's conversion (−1712) with different descriptors (the status doc's known
     differences).
   - Custom Pattern: `style_pattern_fill::parse_pattern`'s pattern object, the one store every Pattern Overlay,
     Pattern stroke and pattern fill layer takes (`pattern_tile`'s four presets or an inline tile, checkerboard
     by default; the app has no other pattern library). Each texel over white, its `blend::luma_code` as an
     8-bit code, is the threshold, tiled from the origin: white iff `g >` it. Unmeasured by design.
   - A method's key beside another method refuses ("mode option frequency applies to method halftone",
     "mode option pattern applies to method custom"); ranges refuse by sentence. The host reads `frequency` and
     `angle` as numbers or their string forms and passes `shape` and `pattern` on; the core owns every range.
     The options JSON is bounded at 2 MiB (`ffi_bitmap::OPTIONS_MAX`: a 512² tile's base64 is 1.4 MB).
5. **Grayscale back with a Size Ratio.** Ratio 1 is the Gray document the storage already is
   (`without_bitmap`): every sample shared, exact. Ratio `r` in 2…16 averages each `r × r` block onto a
   `ceil(w/r) × ceil(h/r)` canvas at `ppi/r`: `(510·W + n) / (2n)`, the partial edge blocks over their own
   pixels; the chrome moves as Image Size moves it. Every other mode refuses on a Bitmap document: "<Mode> is
   unavailable in a Bitmap document; convert to Grayscale first."
6. **`.rz` word 110 in the CMYK slot** (Duotone holds 109). Tables: GRAY then `BTMP`: `"BTMP"`, u32 schema 1,
   u32 width, u32 height, `stride · height` packed bytes, last before `LIDS`. The reader requires 8-bit Gray,
   one canvas raster at the origin, no alpha channel, the plane's dimensions, pad bits 0 and the body equal to
   the render (`bitmap_native::check`); the writer runs the same check first. `RZDC_VERSION` stays 89; phases
   1c to 3 changed nothing in `.rz`.
7. **The gate (rewritten by phase 3: no refusal names a phase).** `bitmap_document::admit_edit` (asked
   before the Gray gate, which still runs) routes a request `BitmapRoute::Edit` (`Edit::Bitmap`, `bitmap_edit`)
   or `BitmapRoute::Shared` (the shared parser, `Edit::parse`). Edit: since 2a-i `paint_blend`, `bucket_fill`
   and `stroke_layer_edge`, since 3 `gradient` and `retouch_clone`, each with `blend_mode` absent or in
   `BLEND_MODES` (since 2b Normal 0, Darken 6 and Lighten 7, each checked by walk2's Fill against the predicted
   result; else `BLEND_REFUSAL`: "The Brush, Fill, Stroke, the Paint Bucket, the Gradient and the Clone Stamp
   paint in Normal, Darken or Lighten mode in a Bitmap document."), `paint`, `erase`, `erase_native`, `crop`,
   `canvas_size`, the quarter turns, the flips and `straighten`, since 2a-ii `resize`, `affine`, `perspective`,
   `affine_set`, `perspective_set` and the layer's `move_selected`, and since 3 `perspective_crop`. Shared
   (`SHARED`, no pixel and no bit moves): `rename_layer`, `layer_label`, `layer_lock`, `liquify` (no edit-request
   op on any document; the shared parser answers it) and a `selection` of any kind but `magic_wand`,
   `color_range`, `similar` and `refine_edge`, which refuse for good by their kind (an absent or unknown kind
   routes Shared too). `admit_selected` (`ffi_selected_adjustment::request`, before the Gray gate) takes `liquify`
   only; the adjustments, Auto, Equalize and Levels by channel refuse as `adjustment`, every filter as `filter`.
   Every other op refuses by `refused_sentence` in three tiers: "<op> is not available in Bitmap mode." for
   `DISABLED_OPS` (Indexed Color's list, `duplicate_layer`, `magic_wand`, 2b's `transform_selected`,
   `color_assign`, `color_convert`, `profile`, RULING-1's `rotate_canvas` and `filter`, and since 3 `color_range`,
   `similar`, `refine_edge`, `hdr_toning`, `puppet_selected`, `puppet_warp`, `transform_mask`, `global_light`,
   `stack_mode` and `paste`); "<op> needs more than one layer, and a Bitmap document holds one." for `ONE_LAYER`
   (`align_set`, `arrange_layer`, `auto_blend_layers`, `distribute_set`, `flatten`, `link_layers`,
   `move_layer_to`, `remove_layers`, `unlink_layers`); else
   "<op> is unavailable in a Bitmap document: Rasterize offers it on Grayscale and colour documents only;
   convert to Grayscale first." (Smudge, the Art History Brush, the other retouch brushes and commands, layer
   comps, and `rz_precision_doc_retouch_from` as `content_aware_move`). Paste refuses in `dispatch_paste`
   before the Gray arm. `PrecisionDocument::duplicate_layer` refuses a Bitmap document for good.
   `BITMAP_FAMILIES` adds `export` in 1c, the paint and exact geometry families in 2a-i, Cut, the resampled
   geometry and `selected_pixels_move` in 2a-ii, and `clipboard_copy`, `layer_name`, `layer_label`,
   `layer_lock`, `selection`, `clone` and `histogram` in 3. The host's `BitmapDocument` is the same gate's
   host side: `tools` (Indexed phase 1a's list, `save_copy` as rz, png, tif/tiff, bmp or jpg/jpeg; 1c's
   exports; 2a-i's paints and exact geometry, `crop`'s `fill` other than `background` refused; 2a-ii's Image
   Size, Move, the selected pixels' moves, the selection edits and Cut, `duplicate` refused by `copyRefusal`;
   since 3 the Gradient, Copy, Quick Mask, Liquify, the Clone Stamp, the History Brush, Quick Selection, the
   Magnetic Lasso, Perspective and Fit Crop, Detect Horizon, Straighten Layer, Reveal All, the layer row, and
   the reads, views, snapshots, recovery, proofs, PDF, workspaces, samplers and paths that move no bit),
   `disabledTools` (2b added the transforms, Trim, the profile ops, Color Range and Similar; RULING-1
   kept `apply_filter`, `lens_blur` refusing by `lensBlurRefusal`, and added `depth_blur`; 3 added Copy Merged,
   Paste, Puppet Warp, the mask transform, the mask and vector-mask rows, HDR Toning, Mode ▸ 8/16/32 Bits,
   Select ▸ Subject's tools, Content-Aware and Generative Fill's, the new layers' contents, the global light,
   style presets and the stack modes), `oneLayer` (3: Align, Distribute, Delete Layer, Flatten Image,
   Auto-Align, Auto-Blend, link, arrange and reorder, Merge to HDR), `channelTools` (3: the alpha-channel tools
   and Load Selection, `channelRefusal`), `save_selection` (`saveSelectionRefusal`) and every other tool
   `unoffered` (the core's default sentence); `rotate` at a filtered angle refuses by
   `arbitraryRotationRefusal`. `AgentServer+Registry` requires every name in `tools`, `disabledTools`,
   `oneLayer` and `channelTools` to be a registered tool. The canvas tools (`allows`): the selections, the
   view, the Brush, Eraser, Paint Bucket, Crop, Move and since 3 the Gradient, Clone Stamp, History Brush,
   Quick Selection, Magnetic Lasso, Eyedropper, Color Sampler, Ruler, Pen and Perspective Crop; the menu items
   (`selectors`) follow the same lists.
8. **Plans.** To Bitmap: Flatten Image's render when it flattens + the LARGER of the resample's geometry plan
   (when it resamples) and output pixels × `TO_BITMAP_BYTES_PER_PIXEL` (41) — the resampled gray is the
   output-sized source the per-pixel shape counts, so 1a's sum charged it twice — + the method's scratch
   (Diffusion: since 2b Sierra Lite's two error rows, 1b's 32 MiB memo gone; Halftone: the 512 KiB spot order;
   Custom: its tile) + bookkeeping. To Gray: output pixels × 41 + bookkeeping. Since 1c, a 1-bit open (PNG,
   TIFF, BMP, and the PSD's stored composite): pixels × `bitmap_io::OPEN_BYTES_PER_PIXEL` (42, Indexed's open
   shape) + the file + bookkeeping; the kept export: pixels × `bitmap_export::EXPORT_BYTES_PER_PIXEL` (2) +
   bookkeeping; the PSD export: the file's worst case + the inverted plane + bookkeeping. Since 2a-i, an edit:
   the inner op's plan on the Gray storage + output pixels × `bitmap_edit::BITMAP_EDIT_BYTES_PER_PIXEL` (34:
   the plane, the re-rendered layer, a row and the host's display preparation; declared 31 from
   `to-gray-from-bitmap`'s 24.02 B/px × 1.25, raised after `crop-extend-bitmap`, whose inner plan is near
   zero, missed at 31 with a 1.25× need of 33.2 B/px) + bookkeeping. Held by
   `scripts/bitmap-admission/measure.py`; a miss raises the case's own term only.
9. **The take (2b).** 1b's smoke1 STOPPED on a −1712: its `photoshop.json`, written after the dialog was
   dismissed, shows `#Lns` resolving to no Photoshop unit, so the HlfS object's `Frqn` was unreadable and
   Halftone Screen opened its dialog. Phase 2b (RULING-1) fitted Pattern and Diffusion offline from smoke1's
   TIFFs (fit1), then ran smoke2 and take1 on a new scene, scene2 (`g = (⌊9x/7⌋ + 13y) mod 256`, ≥ 200
   grays at every 8 × 8 position), with every gate declared before it in `bitmap_take.py`. take1: Threshold
   CONFIRMED (TH128 on 16384 / 16384), Pattern PASS (PT8-MID 16253 / 16384), Diffusion MISS-FITTED
   (LITE-SERP-FREE-128 11460 on D and D2 against Floyd–Steinberg's 11250; D == D2 within the session, but
   smoke2's D differs, so Photoshop's diffusion changes between sessions). `core/tests/bitmap_photoshop_take.rs`
   replays each case's recorded count. Custom Pattern is unmeasured by design (Photoshop's patterns are its
   own).
10. **The 1-bit files (1c).** Which files open as Bitmap is `bitmap_codec`'s header-only claim
    (`rz_path_bitmap_source`), asked after Indexed: PNG colour type 0 depth 1 with no `tRNS` or `acTL`; a classic
    TIFF, one 1-bit sample, Photometric 0 or 1, FillOrder 1, a lossless Compression; BMP 1 bpp BI_RGB with a
    black-and-white palette in either order. An ICC must be Gray. Every other file keeps its old open (a palette
    PNG stays Indexed, a coloured 1-bit BMP and a 1-bit PNG with `tRNS` stay RGB), and `precision rgba8` keeps the
    old 8-bit RGB open of a claimed one. Every source's pad bits are cleared (`clear_pad_bits`, the one clear);
    WhiteIsZero TIFF (patched to 1 through the `Overlay`) and a white-first BMP are inverted once
    (`flip_polarity`). The profile is the file's Gray ICC, else sGray for a PNG `sRGB`, else the working Gray,
    with no untagged warning. Kept export (profile document): PNG colour type 0 depth 1 and TIFF BlackIsZero
    carry the Gray ICC and metadata, BMP (black, then white) neither; `bit_depth` 8 in, 1 out; a size, sharpen
    or other depth refuses; JPEG is the Gray render; any other format refuses by sentence. PSD/PSB colour mode 0
    (Adobe's specification): depth 1, 1 is BLACK, Raw or RLE in, RLE out, no profile written (a loss), resource
    1039 read when Gray; it holds no layers ("A Bitmap file holds no layers; open its stored composite."); a
    layered TIFF and a side past the container refuse. The import report frames the composite
    (`composite_frames`: the admission, the raw length or every RLE row by `validate_packbits`) without
    decoding it, and refuses what the open refuses by the open's sentence.
11. **The edits (2a-i and 2a-ii, `bitmap_edit`; unmeasured).** Each runs on the Gray storage (`without_bitmap`)
    through its existing kernel, then `republish` reads each canvas pixel of the one layer through its offset
    (`indexed_paint::canvas_row`) and puts it back by ONE rule, `bitmap_method::white_over`: white iff
    `gray·alpha + bg·(255 − alpha) ≥ 128·255`, 50% Threshold's boundary over the background. So antialiasing is
    thresholded, not turned off: an edge pixel turns where its coverage reaches about half, and a soft brush
    lands hard-edged. The ink snap: a paint's colour is prepared on the Gray storage (the Gray luminance), its
    8-bit code snapped by the same rule at full alpha, and the op re-parsed with that black or white in
    `document` space; opacity is kept, so a stroke under 50 % changes nothing. The blend modes of decision 7
    only. The background bit: a Straighten's own `background` (an MCP past-canvas `crop` with Delete Cropped
    off pads nothing, so its new area comes back clear for this colour), else the request's
    `indexed_background` (the UI's swatch), else white; each read on the Gray storage by
    `indexed_paint::background_codes` and snapped. Purity: a plane, canvas, resolution and chrome (guides,
    ruler origin, samplers, paths, comps) equal to the old are NULL with no error (2a-ii added the resolution
    and chrome, so a symmetric plane's quarter turn still swaps an anisotropic resolution and a uniform plane's
    flip still moves its guides); otherwise the "Bitmap" layer is re-rendered at the origin with no warp and
    the document rebuilt (`indexed_paint::rebuild`'s shape), so `bitmap_native::check` accepts every result.
    Sample all layers is off (the one layer is the composite). Since 2a-ii the resampled geometry (Image Size,
    the Crop tool's straighten and Move; arbitrary rotation, Free Transform, Distort and Transform Again until
    2b refused them) and the selected pixels' moves run with their own sampler on the gray and are thresholded
    by the same rule; area they uncover or vacate comes back alpha 0 and takes the background bit; a
    whole-pixel move or a nearest sampler keeps each bit. Cut (`bitmap_edit::cut`) is the Gray storage's Cut
    then `republish`, so the cut pixels take the background bit (`indexed_background`, else white), admitted at
    the canvas × `BITMAP_EDIT_BYTES_PER_PIXEL` beside the payload held twice. Inverse, Modify and Transform
    Selection change the host's selection only. Since 3: the Gradient is the Gray storage's
    `NativePaintEdit::Gradient`, its stops prepared by the inner op and NOT snapped (no ink snap), so the ramp
    lands hard where it crosses 128 (Normal, Darken and Lighten; no dither). Perspective Crop
    (`BitmapOp::PerspectiveCrop`) resamples the gray and outside the quad takes the background bit. The Clone
    Stamp (`BitmapOp::Clone`, in place: `indexed_paint`'s Clone arm; `output` `new_layer` refuses as
    `add_layer`) carries no ink. Liquify (the selected edit) and the History Brush run in `bitmap_tools` on the
    Gray storage, then `republish` over white; each plan is the inner plan + canvas × 34 + bookkeeping, no new
    measurement. So a hard clone copies the source bits, full History Brush coverage gives the state's bits
    back, and a soft edge of either is thresholded.
12. **Info reads K (2a-ii; unmeasured).** Bitmap's values are Grayscale's: the K row shows on a Bitmap document
    as on Grayscale, 0 % for white and 100 % for black.
13. **The Magic Wand refuses for good (2a-ii; confirmed by 2b's walk).** walk1 and walk2's wand `setd` on a
    Bitmap document answered "Could not complete the command because it does not work with Bitmap images"
    while Gray's selected (verdict CONFIRMED). The core's `selection` kind `magic_wand` refuses "magic_wand is
    not available in Bitmap mode.", the host's `select_magic_wand` is in
    `disabledTools`, and the canvas `.wand` stays disallowed. A copy move (`move_layers` with `duplicate`
    true, and `duplicate_layer`) refuses for good too: it makes a second layer ("A copy move makes a second
    layer, which a Bitmap document cannot hold.").
14. **What moves no bit (3).** Copy reads the Gray storage (`clipboard_copy`): its payload is a Gray payload
    of the bits as 0 and 255. Quick Mask, Quick Selection, the Magnetic Lasso and the source selections edit
    the host's selection only (the Gray gate's `selection` through the Shared route). `render` draws the
    composite for `channel` `composite` or `bitmap` (the one channel is the plane; `AgentServer.render`'s
    condition); any other channel, or a channel with a `layer`, refuses by `renderRefusal`. Straighten Layer
    is the straightened crop: the layer cannot turn inside a fixed canvas without corners no bit describes, so
    the Ruler's Straighten Layer (`straightenBitmapWithRuler`) and MCP `straighten_layers` crop at
    `BitmapDocument.straightenWindow` (the Crop tool's `CropFit.fitted` of the whole canvas at the line's
    `straightenRotation`, which equals `fit_crop`'s angle for that line) through `straightenedCrop` / `crop`,
    one "Straighten Layer" step recorded as the `straighten_layers` step; the reply is `crop`'s with
    `rotation`. The layer row takes its name, label and lock (Shared; the plane untouched, `.rz` keeps them);
    `set_layer_properties` of anything else refuses by `layerPropertiesRefusal`, and a lock refuses a later
    edit by the lock's own sentence.

## Shipped runs

- **1a** (`4d226f92`): the model, Image ▸ Mode ▸ Bitmap… from 8-bit Grayscale (50% Threshold, Diffusion
  Dither, Output resolution and its unit), Grayscale back with a Size Ratio, `.rz` word 110, the closed gate,
  MCP (`convert_mode`), Actions and the catalog.
- **1b** (`aaf2593e`): Pattern Dither, Halftone Screen (frequency, angle, six shapes) and Custom Pattern
  in the core, the sheet, `convert_mode` and Actions; the resample plan's overlap fixed (the larger of the
  geometry plan and the per-pixel term, not their sum); the anisotropic test resamples its y axis;
  `bitmap_take.py`, whose smoke STOPPED on a −1712 (decision 9).
- **1c** (`f7f07122`): the 1-bit files (decision 10): 1-bit PNG, TIFF and BMP and colour-mode-0 PSD/PSB
  open as Bitmap documents; Save a Copy, `save_copy` and Export keep the plane in PNG, TIFF and BMP (reply
  `bit_depth` 1), JPEG goes through the Gray render, Export as PSD writes the 1-bit PSD/PSB; MCP
  `export_image`, `export_preview`, `quick_export`, `export_psd`, `inspect_psd_export` admitted. 1b's review
  nits fixed (the take's replay pinned to 72 ppi; Ellipse, Cross and Diamond oracles; the header's wrap). Later
  slices must know: `.rz` is unchanged, and the Gray profile travels in PNG and TIFF only.
- **2a-i** (`baf92c05`): painting in black and white and the bit-exact geometry (decisions 7 and 11):
  the Brush, Eraser, Fill, Stroke, the Paint Bucket and Clear in Normal mode, and Crop (past the canvas too),
  Canvas Size, Trim, the quarter turns and the flips, in the canvas, the menus and MCP, each one undo step;
  `PNG_SIGNATURE`'s one home (`metadata`), and the PSD report frames its composite (`composite_frames`)
  instead of decoding it. Later slices must know: `bitmap_edit::republish` is the one way back onto the
  plane, `white_over` the one rule, and `indexed_background` (the host's `backgroundFields`) the swatch's key
  on Bitmap too; `.rz` is unchanged.
- **2a-ii** (`25eed3cd`): Image Size, arbitrary rotation, the Crop tool's straighten, Free Transform,
  Transform Again, Distort, the Move tool, the selected pixels' moves and transforms, Inverse, Modify and
  Transform Selection and Cut, each one undo step through `bitmap_edit` (decisions 7 and 11); the Magic Wand
  and a copy transform or move refuse for good (decision 13); Info reads K (decision 12); `LATER_PHASE` and
  `laterPhases` are gone. 2a-i's review nits fixed: the 0.5/0.51 opacity boundary pinned, and `republish`'s
  purity also compares the resolution and the chrome (a uniform plane's flip still moves its guides). Later
  slices must know: `.rz` is unchanged, and Cut's background key is `indexed_background`, as every edit's.
- **2b** (`e2dd2552`): 1d's take and the menu walk (decision 9; the status doc's measured section). Shipped:
  Pattern Dither's fitted table, Sierra Lite diffusion (disclosed), Threshold confirmed; decision 7 updated in
  core and host: Free Transform, Distort, Transform Again, the selected pixels' transforms and Trim refuse for
  good, Darken and Lighten paint, `DEFAULT_PHASE` 3; smoke1's −1712 diagnosed (`#Lns`) and the descriptor
  fixed with an ID guard, yet halftone1 STOPPED on a −1712 too, so Halftone Screen stays declared and
  unmeasured for good (RULING-1). Under RULING-1 arbitrary rotation and Depth Blur refuse for good, and every
  filter, Lens Blur by a sentence naming the gap. Later slices must know: Lens Blur on a Bitmap document is
  a recorded known difference (not built), not phase 3; `.rz` is unchanged.
- **3** (this batch): what the walk left (decisions 7, 11 and 14): the Gradient (the Gray ramp thresholded),
  the Clone Stamp, the History Brush, Liquify and Perspective Crop through the adapter (`bitmap_edit`,
  `bitmap_tools`); Copy, Quick Mask, Quick Selection, the Magnetic Lasso, the render channels, the layer
  row's name, label and lock and Straighten Layer as the straightened crop; everything else refuses for good
  in three tiers (not available in Bitmap mode, it needs a second layer, or Rasterize offers it on Grayscale
  and colour documents only), the alpha-channel tools and Save Selection by their own sentences; `DEFAULT_PHASE`
  is gone and no Bitmap refusal names a phase. `BLEND_REFUSAL` covers the Gradient and the Clone Stamp.
  The drive's B31 called every newly admitted tool with `document_id` alone and none answered a Bitmap
  refusal, so no tool moved to `unoffered`. Later slices must know: `.rz` is unchanged and no admission was
  re-measured (each new plan composes the inner plan with the measured 34 B/px).
