# Indexed Color document mode (ADR)

- **Status:** Accepted — closed 2026-10-23; the Indexed Color PSD and PSB added the same day.
  [#1](https://github.com/kpgalligan/doctored/issues/1) (the colour-mode umbrella) stays open (see
  [cmyk-mode-status.md](cmyk-mode-status.md)). Grayscale is
  `designs/grayscale-mode.md`, Lab `designs/lab-mode.md`, CMYK `designs/cmyk-mode-status.md`.
- **Originals:** [archive/indexed-mode-status.md](../archive/indexed-mode-status.md) (the working record at close)
  and [archive/indexed-mode-shipped.md](../archive/indexed-mode-shipped.md) (its shipped companion).
- **Takes:** `~/devel/doctored-fixtures/photoshop/indexed-mode-547/` `take1`, `fit1`, `fit2`, `take2`,
  `palette-take1`; runners `scripts/photoshop-probe/indexed_mode_take.py` and `indexed_palette_take.py` (the
  harness is in the private archive, `kpgalligan/doctored-dev`). The PSD:
  `~/devel/doctored-fixtures/photoshop/indexed-psd-547/`, runner `indexed_psd_take.py`, fixtures
  `core/tests/fixtures/psd_photoshop/indexed_547`.
- **Code:** `core/src/indexed_document.rs`, `indexed_palette.rs`, `indexed_act.rs`, `indexed_edge.rs` and
  `indexed_dither.rs` (kernels), `indexed_paint.rs`, `indexed_geometry.rs`, `indexed_transform.rs`,
  `indexed_clipboard.rs`, `indexed_profile.rs`, `indexed_convert.rs`, `indexed_table.rs`, `indexed_io.rs`,
  `indexed_tiff.rs`, `indexed_encode.rs`, `indexed_export.rs`, `indexed_native.rs`, `indexed_psd.rs`,
  `indexed_psd_records.rs` (kernel), `ffi_indexed.rs`,
  `tiff_directory.rs` (the shared TIFF-directory leaf); `app/Sources/IndexedDocument.swift`,
  `DoctoredCore+Indexed.swift`, `IndexedModeSheetController.swift`, `IndexedColorTable.swift`,
  `ColorTableSheetController.swift`, `EditorViewController+IndexedMode.swift`,
  `EditorViewController+ColorTable.swift`, `AgentServer+IndexedColorTable.swift`, `AgentCatalog+Indexed.swift`,
  `EditorViewController+CmykMode.swift`'s indexed arms.
- **Template:** Lab Color phase 1 (`d4cf7e32`) and Grayscale phase 1a (`fa9c7b5b`): the same seams and gate shape.

## Context

Image ▸ Mode ▸ Indexed Color at 8 bits: palette PNG, GIF and TIFF open; the conversion from 8-bit RGB and
Grayscale with the deterministic palettes and dithers, and back to RGB, Grayscale, CMYK and Lab; Color Table
and `.act`; painting, geometry, paste, selections and profiles kept on the palette; the kept-palette export and
`.doctored`, from the UI, over MCP and in Actions. Every tool is admitted or refused for good by name; no refusal names a
phase.

## Decisions in force

- **Storage is the RGB document of one layer.** A `PrecisionDocument` whose `indexed` policy (`IndexedPolicy`: a
  palette of 2–256 8-bit RGB entries, duplicates allowed; an optional transparent index t; a canvas-sized plane of
  one byte a pixel) is `Some`. Its storage is ONE raster layer, "Index", full canvas at the origin, holding the
  render (`palette[i] × 257`, alpha 65535, 0 at t), in an RGB profile, so the canvas, `render`, thumbnails and
  readouts need no Indexed arm. A Grayscale source's twin becomes that profile. No new pixel type or compositor.
- **8 bits only, one layer.** The conversion takes 8-bit RGB or Grayscale; 16/32 bits refuse
  ("convert to 8 Bits/Channel first"), CMYK and Lab ("convert to RGB Color first"). Anything but one plain layer
  answers `flatten_required` ("Indexed Color holds one layer: flatten the image."), and the host asks "Flatten
  layers?".
- **The palettes are the deterministic ones.** Web is the 216-colour cube (`36r + 6g + b`, level `51i`). Uniform
  is the largest `k`-level cube (k in 2..=6) with `|forced ∪ cube_k| + t ≤ colors`, the forced colours first.
  System (Mac OS) and System (Windows) are the platforms' fixed tables, typed out in `indexed_palette`. Exact is
  the distinct opaque colours in raster order, refused past `colors − t`, padded to 2. Forced (Black and White,
  Primaries (red, green, blue, cyan, magenta, yellow, black, white), Web, Custom) applies to Uniform and Adaptive only.
- **Adaptive is median cut after Heckbert (1982)** over one 2²⁴ `u32` histogram (`indexed_palette::
  ADAPTIVE_BYTES`, plus 8 B a distinct colour): the largest-count splittable box splits on its widest channel at
  the count median; each entry the count-weighted mean; forced colours lead, the cut runs at `colors − |forced| −
  t` (refused below 1).
- **Transparency and Matte** read 8-bit codes (`(v + 128) / 257`): with Transparency and no matte alpha < 128 is
  transparent; with a matte alpha 0 is, the rest composited over it; without Transparency every pixel is
  composited over the matte (white). A transparent pixel appends one entry (the matte, else white); a System
  palette makes entry 255 t instead.
- **Mapping and dither.** Nearest entry by least squared RGB distance, ties to the lowest index, never t,
  memoized in one 2²⁴ `u16` table (32 MiB, charged exactly). Diffusion is Floyd–Steinberg in raster order with
  Amount and Preserve Exact. Pattern and Noise (`indexed_dither`) move each opaque pixel by `(t − ½)·DITHER_SPREAD`
  (51) before mapping: Pattern's `t` from Bitmap's 8 × 8 Bayer matrix at the origin (`bitmap_method::bayer`),
  Noise's from Dissolve's canvas hash; with Exact both run the None arm.
- **Back out.** Indexed → RGB is the storage (exact), narrowed to 8 bits by the host; → Grayscale is the Gray
  conversion; → CMYK and → Lab expand the palette exactly (`CmykModeConversion.indexedExpanded`) and run the RGB
  routes at 8 bits, one undo step, equal to RGB Color then CMYK (or Lab).
- **Color Table and `.act`.** Image ▸ Mode ▸ Color Table… and `set_color_table` replace the palette and t over the
  same plane (`IndexedPolicy::with_table`, `indexed_table`), one undo step, the same table no step; a table too
  short for the plane's indices, or outside 2–256, refuses. `.act` (`indexed_act`): 768 bytes (256 entries, no t)
  or 772 with a big-endian count and transparent index; the writer always writes 772.
- **Custom, Previous and Forced Custom.** Custom is `palette custom` with `table` (2–256 entries); 256 entries
  plus a transparent pixel refuses. Previous is the host's `IndexedPalettes` (the last conversion's palette minus
  t, sent as Custom; refused when there is none). Forced Custom is `forced custom` with `forced_table`, leading
  Uniform or Adaptive in order.
- **Open.** An indexed PNG (colour type 3, depth 1/2/4/8, tRNS alphas 0 or 255 with at most one 0, no acTL, an RGB
  or no profile), a GIF (one full-screen image, a colour table) and a palette TIFF (Photometric 3, 1/2/4/8 bits,
  optional straight alpha whose clear pixels are exactly one index, read through the shared `tiff_directory`
  leaf) open Indexed with palette, t and every index kept; every other file keeps its open. A colour-mode-2 PSD
  or PSB (`indexed_psd`, `indexed_psd_records`) opens from its stored composite, its one
  choice: the 768-byte planar table, count from 1046 (else 256), t from 1047 when inside it, every index as
  stored, an RGB 1039 profile kept (else sRGB, warned). Depth 8, Raw or RLE; a ZIP composite, a table not 768
  bytes and a palette outside 2–256 refuse in the report and the open by one sentence; an index outside the
  palette refuses at the open. `support_refusal` refuses colour 2 by name for every other open.
- **Export keeps Indexed** for profile `document` with PNG, GIF or TIFF (`indexed_export`): palette and plane
  encoded directly (PNG type 3 at the least depth with tRNS; GIF89a padded to a power of two with a GCE index;
  TIFF 8-bit Photometric 3 with a second sample when t is set). Another bit depth, an output size, an active
  sharpen, or a hidden layer (`HIDDEN_LAYER`) refuses by name; other requests export the RGB storage. Save As and
  `save_copy` write `.doctored`, PNG, GIF or TIFF; `IndexedDocument.saveLosses` names what a copy drops. Export Layers
  of the Index layer untrimmed takes the kept path. Export as PSD (`export_psd`, both modes) writes a
  colour-mode-2 PSD or PSB in Photoshop's form (take1): one channel, the table zero-padded, 1046 and 1047
  (0xFFFF with no t) after the shared resources, an empty layer section (Photoshop writes no layer record, t or
  not: the image is a Background), the RLE plane; a renamed layer is a loss (`NAME_LOSS`); a TIFF, a side past
  the container and a hidden layer refuse. Plan `indexed_psd_worst_case` + pixels × `EXPORT_BYTES_PER_PIXEL` +
  bookkeeping.
- **`.doctored` word 108** (legacy `.rz` still read) with its `INDX` table (u32 schema 1, u16 count, count × 3 RGB, i16 transparent, u32 width,
  u32 height, the plane), last before LIDS. `DRDC_VERSION` stays 89 (a conditional word like 90–107). Reader and
  writer both require word ⇔ table, one canvas-sized raster at the origin, every index inside the palette and the
  body equal to the table's render.
- **The gate and the refusal classes.** The core's `indexed_document::admit_edit` (in `ffi_precision_edit::request`
  and `ffi_retouch`) routes each admitted request by `IndexedRoute` (`Paint`, `Geometry`, `Selected`,
  `Structure`, `Shared`); `precision_capability::INDEXED_FAMILIES` is the family table. The host's
  `IndexedDocument` is asked first in `DocumentPrecision` and `validatePrecisionItem` (`tools`, `selectors`,
  canvas tools). Every other tool refuses for good in one of three sentences, mirrored by the host's
  `IndexedDocument.refusal`: disabled (`DISABLED_OPS`, the host's `disabledTools`: "<op> is not available in
  Indexed Color.": new layers, Layer via Copy and Cut, Duplicate Layer, New Fill Layer, Perspective Warp,
  Content-Aware Fill, Liquify, the merges and stamps, masks, styles, blend modes, clipping, adjustments,
  filters and artwork, the layers paste, Puppet Warp, a mask edit, HDR Toning, Auto Tone/Contrast/Color and
  Equalize, the Gradient, the retouch brushes, Refine Edge, the Global Light, Stack Modes); one layer
  (`ONE_LAYER`, the host's `oneLayer`: "<op> needs more than one layer, and an Indexed Color document holds
  one.": Align, Distribute, Flatten, Delete Layer, Auto-Align, Auto-Blend, Arrange, Link); else "Doctored
  offers it on RGB documents only.". Red, green, blue, alpha and ink targets refuse (`INDEX_CHANNEL_REFUSAL`);
  opacity and blend mode of the row refuse. Info reads "Index";
  Channels shows one "Index" row; `render` draws `composite`/`index` or an alpha channel
  (`IndexedDocument.renderRefusal` otherwise).
- **Paint and the edge ink.** Brush, Fill, Stroke, the Paint Bucket, the Eraser, Clear, Cut, Clone Stamp and the
  History Brush run the RGB op on the storage, then `indexed_edge::reindex` maps the result back against the
  plane from before: an unchanged pixel keeps its index; alpha < 128 takes `clear_index`; otherwise by the
  `EdgeInk`: `Fixed` (the paint colour: E, the entry nearest the ink, when the coverage
  α = clamp(Σ(c−s)(ink−s) / Σ(ink−s)², 0, 1) ≥ 131/255, else the old index; THRESH k = 131), `Own` (each
  pixel's own colour, its nearest entry), `Clear` (an erase: the old index kept) and `Blend` (a mode but Normal:
  THRESH against the mode's full-strength colour T, `blend::blend_kind`). A plane equal to the old is no step.
  **The background rule:** `clear_index` is t, else the entry nearest the request's `indexed_background` (the UI's
  background swatch key; MCP sends none, so white). Sample all layers is off.
- **Exact geometry and resampling.** Every move and Nearest pick (crop, canvas size, quarter turns, flips,
  Nearest resize/rotate/straighten/perspective crop, whole-pixel `affine`/`affine_set`, Nearest perspective,
  hard `transform_selected`/`move_selected`, a whole-pixel warp `set`, the Move tool) goes through one carrier,
  `indexed_geometry::carried`: the plane encoded as `[i·257, i·257, i·257, 65535]` (t as clear), the same request
  run on it and the indices read back, so duplicate entries keep their own index; new area is `clear_index`
  (a padded Background crop E of its colour). Any other filter resamples the storage and re-indexes `Own`;
  a resample that keeps the canvas re-indexes against the old plane (`compare`). The layer stays canvas-sized at
  the origin; a copy transform or move refuses.
- **Paste, warp, the row, targets and profiles.** Paste lands IN the one layer (`indexed_clipboard`): the
  payload pasted above, masked, merged down and re-indexed `Own`; indices never cross the clipboard. Warp keeps
  no binding. The row takes name, label, locks, visibility, order and disclosure. The selected pixels of an
  alpha channel and the channel commands are admitted (`Shared`); a mask target refuses (no mask in Indexed).
  Assign Profile keeps every index and the table's codes; Convert to Profile converts the colour table and keeps
  every index (`indexed_table::retabled`); both take an RGB profile only (`PROFILE_NOT_RGB`). Selections sample
  the palette colours.
- **Memory** (24 MP, Release, 3 runs, `scripts/indexed-admission/measure.py`): `indexed_convert::
  TO_INDEXED_BYTES_PER_PIXEL` 41 (conversion and Color Table); `indexed_io::OPEN_BYTES_PER_PIXEL` 42 (open);
  `indexed_export::EXPORT_BYTES_PER_PIXEL` 6 (export); `indexed_paint::INDEXED_PAINT_BYTES_PER_PIXEL` 10
  (`brush-indexed` plan 1,293,444,640 ≥ ⌈1.25 × 570,982,784⌉ = 713,728,480); `indexed_geometry::
  GEOMETRY_BYTES_PER_PIXEL` 21 (`transform-nearest-indexed` plan 1,047,848,838 ≥ 992,830,370);
  `indexed_clipboard::PASTE_MERGE_BYTES_PER_PIXEL` 72 (`paste-indexed` plan 3,734,801,442 ≥ 1,239,450,620);
  Convert to Profile with no new constant (`convert-profile-indexed` plan 988,207,124 ≥ 959,898,290).

## Doctored's own rules

- **The Brush edge rule is THRESH k = 131.** It matches 751 of 806 changed pixels on take1, 749 of 803 on take2's
  twin and 992 of 1156 on its fresh stroke, every ink pixel exact; no candidate reached the 99 % gate, and
  Photoshop's own edge in Indexed Color is nondeterministic (take2's twin strokes differ on 64 of 803 changed
  pixels).
- **Adaptive is median cut after Heckbert.**
- **Diffusion is Floyd–Steinberg,** the published algorithm.
- **Pattern and Noise are as declared.** Pattern's Bayer rule matches 99.21 % of Photoshop's Web ramp and 62.88 %
  of its System (Mac OS) ramp; no fitted family passed both. Photoshop refused Noise through every descriptor
  tried, so it is unmeasured.
- **Local and Master Perceptual and Selective refuse for good** (`indexed_convert::UNPUBLISHED_PALETTES`):
  "The <title> palette is not available: its algorithm is unpublished. Use Adaptive (median cut) instead."
  The Local pair missed every candidate and
  Photoshop refused the Master pair; no palette ships under Photoshop's name. The sheet lists them disabled.

## Photoshop's measured facts

Photoshop 26.10.0.

- **take1** (`indexed-mode-547/take1`, `indexed_mode_take.py`; a 64 × 32 six-band scene converted Exact, menus
  walked, one Brush stroke): VALID. 495 items a walk; 164 enabled on RGB are disabled in Indexed; Color Table is
  the one item enabled only in Indexed. Disabled: Perspective Warp, Content-Aware and Generative Fill, Sky
  Replacement, Apply Image, Calculations, New Layer, Layer via Copy/Cut, Duplicate Layer, New Fill Layer, Select
  Subject/Sky/Select and Mask, Liquify, Neural Filters, the adjustments and filter families. The stroke changed
  806 pixels, 406 the ink.
- **fit1 / fit2** (the edge rule): best THRESH k = 131 751 of 806; MIX 745 of 804; H0 737 of 805; error diffusion
  best JJN 748; ordered dither best 733; distance threshold best 740. None reached 99 %.
- **take2**: the same stroke twice from one history state differs on 64 of 803 pixels (NONDETERMINISTIC); THRESH
  131 matches 749 of 803 on the twin, 992 of 1156 on a fresh stroke (13 px, hardness 50).
- **palette-take1** (`indexed_palette_take.py`; scenes A, B, R; gate Chebyshev Hausdorff ≤ 2 codes and ≥ 99 %
  within 2): Local Perceptual MISS (best PC2 0 of 4, worst Hausdorff 92); Local Selective MISS (best SC2 0 of 4,
  worst 80); Master Perceptual and Master Selective REFUSED by Photoshop on every case; Pattern MISS (99.21 % Web,
  62.88 % System (Mac OS); best fitted Bayer 99.27 / 62.99 %); Noise REFUSED by Photoshop.
- **The PSD, take1** (`indexed-psd-547/take1`, `indexed_psd_take.py`; three scenes converted Exact, saved PSD and
  PSB): all five AUTHORED and reopened as one Background, a transparent index included. One channel; the table
  zero-padded; 1046 and 1047 in every file, 1047 0xFFFF with no t; no layer record (tail blocks only); RLE
  composites, Raw for the 256-colour one. **take2** (`take2`): Photoshop opens Doctored's PSD and PSB (six
  colours reversed, t = 5, 256 entries) as Indexed and its own copy holds our count, palette, t and every index;
  so do the observed t = 1 and duplicate-entry files.

## Shipped

- **1a** (`451c329f`): PNG and GIF open; Mode ▸ Indexed Color (Exact, System, Web, Uniform, Forced, Transparency,
  Matte, None/Diffusion) and back; Info's Index; `.rz` word 108.
- **1b** (`ac77c338`): Adaptive; PNG, GIF and TIFF export keeping the palette; palette TIFFs open.
- **1c** (`b7ba1382`): Color Table and `.act`; Custom and Previous; CMYK and Lab directly from Indexed.
- **2a** (`8bb6008c`): take1 and the refusals (`DISABLED_OPS`). **2b** (`8ab6ccf2`): the edge rule and the Brush;
  Auto Tone/Contrast/Color and Equalize refused.
- **2c** (`70362821`): clipboard, Fill, Stroke, Paint Bucket, Eraser; Image Size, Canvas Size, Trim, rotation, flip,
  crops on the carrier.
- **2d** (`f9550c65`): Paste, the layer transforms, the selected pixels' transform, Warp, the row's name, Export
  Layers.
- **3a** (`3efbddb3`): the Move tool, the row, alpha-channel targets, Reveal All, Assign/Convert to Profile, the
  background rule. **3b** (`01e278dd`): every blend mode, the selections, the background swatch.
- **3c** (`6c7446f2`): Pattern, Noise, Forced Custom, render channels, Magic Wand, Quick Selection, Clone Stamp,
  History Brush; every tool decided for good. **3d** (`06635454`): palette-take1; the four palettes refused for
  good; Pattern and Noise disclosed.
- **The close** (2026-10-23): the docs condensed here; drive `scripts/verify_indexed_mode.py` I59 sweeps the
  whole catalog on an Indexed document and pins each refusal's class; no refusal was found naming a phase.
- **The Indexed Color PSD** (2026-10-23): colour-mode-2 PSD and PSB read and write (Decisions: Open,
  Export), fixtures `indexed_547`, take2 PASS; `dr_precision_doc_open_indexed_psd`; drive I60. The
  jobs-probe case "indexed background key" runs `PrecisionGeometryWork.compute`'s merged-key branch
  (move, transform, crop past the canvas) with and without the swatch.
- **The MCP precision names** (2026-11-12): `get_precision` answers `mode` indexed8, `bits_per_channel` 8, the
  palette's `colors` and this gate's own lists (`IndexedDocument.support`, also `get_document`'s
  `precision_support`), and the document summary's `precision` is indexed8 (`RasterDocument.precisionName`).
  Jobs-probe case "precision reply modes".

## Open items

- None.
