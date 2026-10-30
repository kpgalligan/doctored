# Indexed Color document mode (#547): shipped

The settled record of the shipped Indexed Color phases, moved verbatim out of
[indexed-mode-status.md](indexed-mode-status.md) (the open phase's status doc, which keeps the
head, the contracts of the phases not yet shipped, the measured Photoshop take and open items).

## Decisions in force

- **Storage is the RGB document of one layer.** An Indexed document is a `PrecisionDocument` whose `indexed`
  policy (`IndexedPolicy`: a palette of 2–256 8-bit RGB entries, duplicates allowed; an optional transparent
  index; a canvas-sized plane of one byte a pixel) is `Some`. Its storage is ONE raster layer, "Index", full
  canvas at the origin, holding the policy's render: `palette[i] × 257`, alpha 65535, alpha 0 at the transparent
  index. The profile is an RGB profile (data space RGB, not the Lab twin), the palette's space, so the canvas,
  `render`, thumbnails and the readouts read the palette colours through it with no Indexed arm. A Grayscale
  source's twin becomes that profile. No new pixel type, no new compositor.
- **8 bits only, one layer.** Image ▸ Mode ▸ Indexed Color converts an 8-bit RGB or 8-bit
  Grayscale document; 16- and 32-bit documents refuse ("convert to 8 Bits/Channel first"), CMYK and Lab refuse
  ("convert to RGB Color first"). Anything but one plain layer (full canvas, at the origin, opacity 1, visible,
  Normal, no mask, style, clipping, Blend If, description or binding) answers `flatten_required` ("Indexed Color
  holds one layer: flatten the image."), and the host asks "Flatten layers?".
- **The palettes are the deterministic ones.** Web is the 216-colour cube (`36r + 6g + b`, level `51i`).
  Uniform is the `k`-level cube (levels `round(255i/(k−1))`, r slowest) for the largest `k` in 2..=6 with
  `|forced ∪ cube_k| + t ≤ colors` (`t` 1 when a transparent entry is appended), the forced colours first; no
  such `k` refuses with `N = |forced ∪ cube_2| + t`. System (Mac OS) and System (Windows) are the platforms'
  fixed 256-entry tables, typed out in `indexed_palette`. Exact is the distinct opaque colours in
  first-occurrence raster order, refused past `colors − t`, padded to 2. Forced (Black and White; Primaries in
  the order red, green, blue, cyan, magenta, yellow, black, white; Web) is Uniform's and
  Adaptive's ("Forced colours apply to the Uniform and Adaptive palettes only.").
- **Adaptive is median cut after Heckbert (1982)**, not claimed as Photoshop's Local or Master Adaptive. The
  opaque prepared pixels are counted in one 2²⁴ `u32` histogram (64 MiB, charged exactly with 8 B a distinct
  colour, Adaptive only); the distinct colours start as one box in key order. While fewer boxes than the budget,
  the splittable box (≥ 2 colours) with the largest total count splits (ties to the lowest box), on the channel of
  the largest max − min (ties r, g, b), sorted by it (ties by key), at the smallest k with 2·Σ_{i<k} ≥ total (else
  n − 1); the lower part keeps its index, the upper is appended. Each entry is the count-weighted mean, rounded
  half up. Forced colours lead; the cut runs at `colors − |forced| − t` (refused below 1: "The Adaptive palette
  needs at least N colours with these options."), entries equal to a forced colour drop, and Exact's padding
  fills to 2. An empty histogram (all transparent) cuts nothing and pads.
- **Transparency and Matte** read 8-bit codes (`(v + 128) / 257`). With Transparency and no matte, alpha < 128
  is transparent and the rest keeps its colour unmatted; with a matte, alpha 0 is transparent and the rest is
  composited over it. Without Transparency every pixel is composited over the matte (white when none). A
  transparent pixel appends one transparent entry (the matte, else white); a System palette makes entry 255
  the transparent index instead.
- **Mapping and dither.** The nearest entry is the least squared RGB distance, ties to the lowest index, never
  the transparent entry, memoized in one 2²⁴-entry `u16` table (32 MiB, charged exactly). Diffusion is
  Floyd–Steinberg (1976) in raster order: `w = clamp(src + e, 0, 255)`, the entry nearest `round(w)`,
  `(w − entry)·amount/100` spread 7/16, 3/16, 5/16, 1/16, dropped past the edges; a transparent pixel takes and
  emits none; Preserve Exact maps a source colour equal to an entry to it and emits none. It is the published
  algorithm, not claimed as Photoshop's.
- **Back out.** Indexed → RGB is the RGB document the storage is (every sample shared, exact), narrowed to
  8 bits by the host; Indexed → Grayscale is the unchanged Gray conversion (each pixel a function of its palette
  entry). Since phase 1c Image ▸ Mode ▸ CMYK Color… and Lab Color convert an Indexed document directly:
  the host expands the palette exactly (`CmykModeConversion.indexedExpanded`, the RGB
  document the storage is) and runs the existing RGB → CMYK and RGB → Lab routes at 8 bits, one undo step; no
  second conversion exists, so the result equals RGB Color then CMYK (or Lab) exactly.
- **Color Table (phase 1c).** Image ▸ Mode ▸ Color Table… and `set_color_table` replace the palette and the
  transparent index over the same plane (`IndexedPolicy::with_table`, `indexed_table`): every pixel keeps its
  index, the one layer is re-rendered, one undo step; the same table is no step (`changed` false). A table of
  n entries refuses when the plane uses an index ≥ n ("The colour table holds n colours, but the image uses
  index m; it needs at least m+1."); a table holds 2–256 entries ("a colour table holds 2 to 256 colours").
  The plan is canvas × `TO_INDEXED_BYTES_PER_PIXEL` + bookkeeping. `get_color_table` reads it.
- **`.act` (phase 1c).** Adobe Color Table: 256 RGB triples (768 bytes); a 772-byte file adds a big-endian
  u16 count (2–256) and transparent index (0xFFFF none, else below the count). A 768-byte file is 256 entries
  with no transparent index; any other length, count or index refuses. The writer always writes 772 bytes,
  unused entries zero (`indexed_act`, a kernel); Load… and Save… in the dialog, `path` on both tools
  (declared in `AgentPathGuard.toolPaths`).
- **Custom and Previous (phase 1c).** Custom is `palette custom` with `table` (2–256 entries, required with
  it and refused elsewhere; `colors` refuses with it): each pixel its nearest entry under the same mapping and
  dither, a transparent pixel appending the transparent entry (so 256 entries then refuse: "The Custom
  palette holds 256 colours, leaving no room for the transparent colour; turn Transparency off or use 255
  colours."). Previous is the host's (`IndexedPalettes`, app-wide): the last Indexed Color conversion's
  palette minus its transparent entry, sent as Custom; a conversion leaving fewer than 2 opaque entries
  clears it, and Previous then refuses ("There is no previous Indexed Color palette: convert a document to
  Indexed Color first.").
- **Open.** An indexed PNG (colour type 3 at depth 1/2/4/8, PLTE 2–256, a tRNS of alphas 0 or 255 with at most
  one 0, no acTL, an RGB `iCCP`, `sRGB` or no profile) and a GIF (one image descriptor covering the logical
  screen, a colour table, no ICC extension) open as Indexed documents, palette, transparent index and every
  index kept; every other file keeps today's open. A palette TIFF (classic, Photometric 3 as a SHORT, one sample
  of 1/2/4/8 bits or [8, 8] with ExtraSamples [2], chunky, unsigned, a lossless compression, ColorMap 3·2^bits,
  an RGB ICC or none) opens through the shared `tiff_directory` leaf (the first-IFD walk, the readers and the
  `Overlay`, moved verbatim out of `lab_tiff_read.rs`), decoded as Photometric 1 through the overlay; entry i is
  `code8` of the ColorMap. With alpha, every alpha is 0 or 255, and the alpha-0 pixels must be exactly one
  index's pixels, which becomes the transparent index (none when no pixel is clear).
- **Export keeps Indexed** for profile `document` with PNG, GIF or TIFF (`indexed_export`): the palette and plane
  are encoded directly (no render, no quantize): PNG colour type 3 at the least depth, PLTE the palette, tRNS
  clear at t; GIF89a with the global table padded black to a power of two and a GCE transparent index, no
  profile or metadata; TIFF 8-bit Photometric 3, ColorMap padded black to 256, a straight-alpha second sample
  when t is set (so a t no pixel uses is not recorded). `bit_depth` other than 8, an output size, or an active
  sharpen refuses by name; every other request runs the RGB stages over the storage. Save As and `save_copy`
  write `.rz`, PNG, GIF or TIFF; `IndexedDocument.saveLosses` names what a copy drops, `[]` only when it
  reopens as the same document.
- **`.rz` word 108** sits in the CMYK slot (never beside 82–88, 106 or 107); its `INDX` table (u32 schema 1,
  u16 count, count × 3 RGB, i16 transparent, u32 width, u32 height, the plane) is the last before LIDS.
  `RZDC_VERSION` stays 89: 108 is a conditional word like 90–107, and every earlier file reads unchanged. The
  reader requires word ⇔ table, u16 storage, one canvas-sized raster at the origin, every index inside the
  palette and the body equal to the table's render; the writer runs the same check first. Lane C may claim 108
  for Lab; the controller renumbers at merge.
- **The gate:** the core's `indexed_document::admit_edit` (in `ffi_precision_edit::request` and
  `ffi_retouch`) routes each admitted request by `IndexedRoute` (`Paint`, `Geometry`, `Selected`, `Structure`,
  `Shared`); Cut is `indexed_paint::cut` in `ffi_clipboard`. `precision_capability::INDEXED_FAMILIES` is the
  admitted family table. The host's `IndexedDocument` is asked first in `DocumentPrecision` and
  `validatePrecisionItem` (its `tools`, `selectors` and canvas tools). Since phase 2a the lists follow take1's
  build table (the status doc); since 2b Auto Tone, Auto Contrast, Auto Color and Equalize (read disabled) refuse
  for good by the controller's ruling of 2026-10-22. Info shows the palette index under the cursor ("Index");
  Channels shows one "Index" row. Every refusal is for good since phase 3c (below).
- **The Brush (phase 2b)** paints the RGB storage with the native `paint_blend` (`NativePaintEdit`, Normal
  only), then `indexed_edge::reindex` maps the result back against the plane from before the stroke: an
  unchanged pixel keeps its index; a changed one at alpha < 128 takes the transparent index t when there is one;
  a changed one on t at alpha ≥ 128 takes E, the entry nearest the ink (`NearestMap`, never t); every other
  changed pixel takes E when its coverage α = clamp(Σ(c−s)(ink−s) / Σ(ink−s)², 0, 1) (α = 1 when the ink is the
  backdrop) is ≥ 131/255, else keeps its index (THRESH, k = 131). The ink is the request's colour as the paint
  prepares it, at 8-bit codes. A plane that equals the old one is no step; otherwise the one layer is re-rendered
  from the new policy (`indexed_table`'s rebuild), one undo step, and `.rz` keeps it.
- **The edge rule is a disclosed fallback, not Photoshop's.** Photoshop's edge in Indexed Color is not
  deterministic: take2 painted take1's stroke twice from one history state and the two edges differ on 64 of
  803 changed pixels. No candidate of either declared family reached the 99 % gate, so THRESH at take1's best
  k ships (the controller's RULING-1): full-ink pixels exact on every take; the edge matches 751 of 806 changed
  pixels on take1 (93.2 %), 749 of 803 on take2's twin (93.3 %) and 992 of 1156 on its fresh stroke (85.8 %).
- **Memory.** `indexed_convert::TO_INDEXED_BYTES_PER_PIXEL` is 41 (Gray's measured shape) beside the exact
  memo, plus `indexed_palette::ADAPTIVE_BYTES` and 8 B a distinct colour for Adaptive (also in the host's
  8-bit source reservation, `EditorViewController+CmykMode.swift`'s `scratch`, inside which the core reserves);
  `indexed_io::OPEN_BYTES_PER_PIXEL` 42 (41 missed `open-indexed-png` at 24 MP by 0.5 %), which the palette TIFF
  open shares plus pixels × samples; `indexed_export::EXPORT_BYTES_PER_PIXEL` 6 plus bookkeeping (no codec term).
  Phase 1c's table edit charges canvas × `TO_INDEXED_BYTES_PER_PIXEL` + bookkeeping; the direct CMYK and Lab
  conversions take the expanded palette's own plan, never less than Indexed → RGB's 41 B a pixel (the host's
  Indexed arm in `scratch`: the CMYK plan alone missed `to-cmyk-from-indexed`).
  Phase 2b's Brush plans the native paint's own figure plus canvas × `indexed_paint::
  INDEXED_PAINT_BYTES_PER_PIXEL` (10: the new plane, the re-rendered layer and the row of codes), the 32 MiB
  nearest-colour memo and bookkeeping (`brush-indexed` at 24 MP, Release, 3 runs: plan 1,293,444,640 ≥
  ⌈1.25 × max E 570,982,784⌉ = 713,728,480, 2.27×). `scripts/indexed-admission/measure.py` holds them.
  Phase 2c's paint family and Cut plan the inner op's own figure plus 2b's re-index term; its geometry plans
  the RGB op's figure plus (old + new canvas) × `indexed_geometry::GEOMETRY_BYTES_PER_PIXEL` (10 then, 21
  since phase 2d's miss: the carrier or the row of codes, the new plane and the re-rendered layer), the memo and
  bookkeeping
  (24 MP, Release, 3 runs: `rotate90-indexed`, the exact path, plan 1,094,824,248 ≥ ⌈1.25 × max E
  572,965,488⌉ = 716,206,860, 1.91×; `rotate-indexed`, the resample path, plan 1,375,413,078 ≥ ⌈1.25 × max E
  945,881,976⌉ = 1,182,352,470, 1.45×).
- **The ink and the backdrop (phase 2c).** `indexed_edge::reindex` takes an `EdgeInk` and an optional
  backdrop: `Fixed` (the paint colour, 2b's THRESH against the backdrop), `Own` (each pixel's own colour, its
  nearest entry) and `Clear` (an erase: the old index kept). Per changed pixel (every pixel with no backdrop):
  alpha < 128 takes t; a t backdrop or none takes E of the ink or of the pixel's own colour, never t; `Clear`
  keeps the old index. The 2b Brush's results are unchanged. Fill, Stroke and the Paint Bucket are `Fixed`;
  the Eraser, Clear and Cut `Clear`; a resample `Own` with no backdrop. The result's canvas window is read
  through the layer's offset (a stroke that grows the layer past the canvas is clipped to it) and republished
  by 2b's rebuild (`indexed_paint::republish`). Sample all layers is off on an Indexed document.
- **No transparent index (phase 2c)** refused by name; phase 3a's background rule (below) replaced it.
- **Exact geometry and resampling (phase 2c).** A move or a Nearest Neighbor pick (`crop`, `canvas_size`, the
  quarter turns and flips, and Nearest `resize`, `rotate_canvas`, `straighten` and `perspective_crop`) carries
  the index plane itself: the plane is encoded as a carrier layer `[i·257, i·257, i·257, 65535]`, the same
  request runs on it and the canvas window is read back (`R/257`), so duplicate palette entries keep their own
  index; new area is t (or the refusal). A zero-angle `straighten` carrying `background` (the Crop tool and MCP
  `crop` past the canvas) is exact: where the RGB route would pad the Background (`background_pad_bytes` > 0:
  opaque record 0, Delete Cropped Pixels on), the new area is E of the prepared background colour. Any other
  filter runs on the RGB storage and maps each pixel to its nearest entry (`Own`, no backdrop).
- **The exact rule extended (phase 2d).** One carrier path, `indexed_geometry::carried`, serves every move:
  `affine` and `affine_set` with filter Nearest or a matrix `[1, 0, 0, 1, tx, ty]` of integer tx, ty (re-run
  Nearest); `perspective` and `perspective_set` with Nearest; `transform_selected` under the same rule (its quad
  form Nearest only) and every coverage sample 0 or 65535; a warp `set` whose field is a whole-pixel translation.
  Where the new area is t the carrier encodes t as `[0, 0, 0, 0]`, so a floated t lands nothing and leaves the
  destination's index, as the RGB storage does (only the padded Background crop keeps t opaque, its new area
  being E).
- **`compare` (phase 2d).** A resample that keeps the canvas (the layer transforms, `transform_selected`, Warp,
  Paste) re-indexes against the old plane (`EdgeInk::Own` with a backdrop): an unchanged pixel keeps its index,
  so duplicate entries survive outside the edit; 2c's whole-canvas geometry keeps no backdrop.
- **Paste lands IN the one layer (phase 2d).** The payload (RGB, Gray, CMYK or Lab) is pasted onto the RGB
  storage above the Index layer by today's paste, masked by Paste Into's or Outside's selection (`set_mask`),
  merged down (Merge Down's lock rule refuses a locked Index layer) and re-indexed `Own` against the old plane:
  a pasted colour equal to an entry takes it (the lowest of duplicates), so a paste from an Indexed document of
  the same palette with no duplicates keeps every index; indices never cross the clipboard (an Indexed copy is an
  RGB payload). A paste that moves no index still answers the document (a history step). The
  reply's `layer` is 0. The core paste begins its own job control, so the inner RGB paste runs uncontrolled.
- **Warp keeps no binding (phase 2d).** `indexed_paint::rebuild` drops the layer's binding, so a set is applied
  at once (the float, CMYK, Gray and Lab rule): `warp_info` answers null, Clear and Bake change nothing.
- **Layer row and export (phase 2d).** Rename, label and locks are the shared structure edit (the plane and
  pixels kept); `set_layer_properties` takes only `name` (opacity and blend mode not available in Indexed;
  visibility, offset and disclosure phase 3's). Export Layers of the Index layer untrimmed with
  profile document writes the kept-Indexed export; trimmed or any other request exports the RGB storage's layer.
  Duplicate Layer refuses for good (`DISABLED_OPS`), and a copy transform (`duplicate` on
  `transform_layers` or `transform_again`) refuses by name, since the mode holds one layer.
- **Memory (phase 2d).** The layer transforms, `transform_selected` and Warp plan the RGB op's figure plus
  `indexed_geometry::transform_scratch` (2c's (old + new canvas) × `GEOMETRY_BYTES_PER_PIXEL`, the memo and
  bookkeeping, one home); Paste plans the RGB paste's own term, (canvas + payload) × `indexed_clipboard::
  PASTE_MERGE_BYTES_PER_PIXEL` (72, declared before measuring) and the re-index's bytes. 24 MP, Release, 3
  runs (`scripts/indexed-admission/measure.py`): `transform-nearest-indexed` (a nearest whole-pixel layer
  translation, whose inner plan is near nothing) missed at `GEOMETRY_BYTES_PER_PIXEL` 10 (plan 519,848,838
  against max E 794,313,424, 0.654×), so the term is 21: plan 1,047,848,838 ≥ ⌈1.25 × max E 794,264,296⌉ =
  992,830,370, 1.32×; `transform-indexed` (bicubic 5°) plan 1,858,236,998 ≥ 995,595,230, 2.33×;
  `paste-indexed` (a full-canvas RGB copy) plan 3,734,801,442 ≥ 1,239,450,620, 3.77×.
- **The background rule (phase 3a).** A pixel an edit leaves at alpha < 128 (an erase, Cut, Clear, a canvas
  extension, an uncovered transform, move or warp area, a selection's hole) takes `indexed_edge::clear_index`:
  the transparent index t, else E of `BACKGROUND`, the default background colour white, as a Background
  layer paints one; one home, used by `reindex` and every `carried` call. 2c's padded-crop E (the
  crop request's background colour where the RGB route pads) is unchanged. Since 3b the colour is the request's.
- **The Move tool (phase 3a).** `move_layers`, the canvas Move tool and the row's offset are `affine_set`
  whole-pixel translations (`follow_links` false for the offset) on 2d's exact carrier path; `move_selected`
  is always exact under a hard selection (else `Own` against the old plane). The one layer stays canvas-sized
  at the origin, so its offset reads 0 after and pixels moved past the canvas are dropped (the
  Background-layer clip). A copy move (`move_layers` `duplicate`) refuses as a copy transform does.
- **Targets (phase 3a).** The selected pixels' move and transform of an alpha channel run on the RGB storage
  with the policy re-attached, the layer and plane untouched. A mask target (a layer transform's `target mask`,
  `transform_selected`/`move_selected` `mask`, `transform_mask`) refuses for good: an Indexed layer holds no
  mask. Red, green, blue, alpha and ink targets refuse (`INDEX_CHANNEL_REFUSAL`): the document has one
  channel, Index.
- **The row (phase 3a).** `layer_visible` hides the layer (plane and samples kept; `.rz` keeps it hidden); a
  kept-Indexed export (and Export Layers' kept path) refuses while it is hidden (`HIDDEN_LAYER`), since the
  file would hold a layer the composite does not show. `move_layer_to` and `layer_open` are the shared
  structure edits (no change on the one raster; the host answers `open` with its groups-only sentence).
  Opacity and blend mode refuse for good. Reveal All always answers no change: the layer holds no pixel
  outside the canvas.
- **Profiles (phase 3a).** Assign Profile keeps every index, the table's codes and every sample (the storage's
  assign, the policy re-attached): the table's meaning changes. Convert to Profile converts the colour table
  and keeps every index and t: the palette as a one-row RGBA16 strip in the
  old profile runs the shared `convert_to_profile_with` under the request's options, each entry is read back
  at its 8-bit code, and the storage assigned to the destination is re-rendered (`indexed_table::retabled`).
  Both take an RGB profile only (`PROFILE_NOT_RGB`; the FFI's profile argument and the host's `resolveProfile`
  refuse other data spaces first). Plans: Assign the storage's assign plan, Convert that plus the Color
  Table edit's re-render (`set_color_table_scratch`). `convert-profile-indexed` (24 MP, Display P3, Release,
  3 runs): plan 988,207,124 ≥ ⌈1.25 × max E 767,918,632⌉ = 959,898,290, 1.29×, with no constant raised.
- **For good (phase 3a).** `DISABLED_OPS` adds `paste_layers` (Layer ▸ New ▸ Layer is disabled, so a
  paste cannot add layers), `puppet_selected`, `puppet_warp` and `transform_mask`; `ONE_LAYER` (`align_set`,
  `distribute_set`, `flatten`, `remove_layers`; the host's `align_layers`, `distribute_layers`,
  `delete_layer`, `flatten_image`) answers "<op> needs more than one layer, and an Indexed Color document
  holds one.".
- **Every blend mode (phase 3b).** The Brush, Fill, Stroke and the Paint Bucket paint the RGB storage in their
  mode; a mode but Normal re-indexes with `EdgeInk::Blend`: over an opaque backdrop s, T is the mode's
  full-strength colour at 8-bit codes (`blend::blend_kind`, round(255·B(s/255, ink/255)), Dissolve the ink) and
  2b's THRESH against T takes the entry nearest T, else the old index; a t backdrop takes E of the ink. Normal
  stays `Fixed`; the plan is 2b's. The app offers neither Behind nor Clear on any document.
- **The background colour (phase 3b).** `clear_index` takes the colour: t, else `nearest_entry` of it, looked up
  on the first clear pixel only (no memo). The request's `indexed_background` (`CropBackground`'s RGB shape,
  prepared by `indexed_paint::background_codes`) rides every paint, geometry and selected-pixel request, Cut and
  a warp `set`; a straighten's own `background` wins; absent is white. The UI sends the background swatch from
  the Eraser, Clear, Cut, Canvas Size, Image Rotation, Perspective Crop, Warp and the geometry gestures (not a
  duplicate, Align or Distribute, whose core refusals stand); MCP sends none (white) but `crop`'s
  `background_color`, so a replayed Action's erase is white.
- **Selections (phase 3b).** `IndexedRoute::Shared` (the shared parser; layer and plane untouched): the source
  selections `color_range`, `similar` and `alpha` sample the palette colours (a duplicate entry's pixels select
  with its twin's); `set_channel` and `read_plane` of an alpha channel serve Save and Load Selection and Quick
  Mask; `color`, `luminance` and a red, green, blue or alpha plane refuse with `INDEX_CHANNEL_REFUSAL`.
  Modify and Transform Selection need only the host's gate. Since 3c `magic_wand` and `quick` join them.
- **For good (phase 3b).** `hdr_toning` joins `DISABLED_OPS` (under Image ▸ Adjustments, disabled in take1)
  and `auto_blend_layers` `ONE_LAYER` (the host's `oneLayer` adds `auto_align_layers`); the core's HDR and
  Auto-Blend entry points refuse. Convert to Profile's no-change return precedes its admission.
- **Pattern and Noise (phase 3c), declared; kept by 3d.** `indexed_dither` (a kernel): each opaque pixel
  moves by `(t − ½)·DITHER_SPREAD` (51, the Web step) on r, g and b, clamped and rounded, then takes its nearest
  entry; Pattern's `t = (2·M + 1)/128` of Bitmap's 8 × 8 Bayer matrix from the origin (`bitmap_method::bayer`),
  Noise's Dissolve's canvas hash. With Exact both run the None arm; `amount` and `preserve_exact` are
  Diffusion's ("mode options amount and preserve_exact apply to Diffusion only"). 3d's take: Pattern missed,
  Noise refused by Photoshop; both kept, numbers in the status doc's palette-take1 rows.
- **Forced Custom (phase 3c)** is `forced custom` with `forced_table` (2–256 entries, required with it and
  refused elsewhere): its colours lead Uniform or Adaptive in order, a repeat dropped, under the existing
  budgets.
- **Render channels (phase 3c).** `render` channel `composite` or `index` draws the composite (the Index
  channel through the palette), an alpha channel's name its plane, never with a layer; anything else answers
  `IndexedDocument.renderRefusal`.
- **Tool classes (phase 3c).** Every tool and op is admitted or refuses for good: disabled in the mode
  (`DISABLED_OPS` adds the Gradient, the retouch brushes but the Clone Stamp and the History Brush, Refine
  Edge, the Global Light and Stack Modes; the host's list the measured Filter, Generative and Select Subject
  families, the artwork and mask edits and Mode ▸ 16/32 Bits), one layer (`ONE_LAYER` adds Arrange and Link),
  else "<op> is unavailable in an Indexed Color document: Rasterize offers it on RGB documents only."
  (`DEFAULT_PHASE` and `laterPhases` deleted). Admitted: the Magic Wand, Quick Selection, Magnetic Lasso, the
  alpha channels' commands and scalar edits (`Shared`), the readouts, history, snapshots, proof, view, paths,
  samplers and Straighten. `rz_precision_doc_retouch_from` refuses as Content-Aware Move.
- **Clone Stamp and the History Brush (phase 3c)** paint the RGB storage (`retouch_clone` in place;
  `history_brush` from an earlier state), then re-index with `EdgeInk::Own` against the old plane (white as the
  background: the swatch key is not sent); a clone onto a new layer refuses as `add_layer`. Plans are the inner
  op's plus `reindex_bytes`.

## Shipped runs

- **1a** (`451c329f`): indexed PNG and GIF open; Image ▸ Mode ▸ Indexed Color… from 8-bit RGB and Grayscale
  (Exact, System (Mac OS), System (Windows), Web, Uniform; Colors; Forced; Transparency; Matte; Dither None or
  Diffusion), one undo step, over MCP (`convert_mode` `indexed`) and in Actions; RGB Color and Grayscale back;
  Info's Index readout; `.rz` word 108. Tests `indexed_palette`, `indexed_open`, `indexed_convert`,
  `indexed_native`; drive `scripts/verify_indexed_mode.py` I1–I8; admission `scripts/indexed-admission/measure.py`.
- **1b** (`ac77c338`): the Adaptive palette (median cut) with Forced; PNG, GIF and TIFF export, Quick Export and
  Save As keep the palette, transparent index and every index (`indexed_export`); palette TIFFs open as Indexed
  through the shared `tiff_directory` leaf. Tests `indexed_export`, `indexed_tiff`; drive I9–I14; admission
  `to-indexed-adaptive`, `open-indexed-tiff`, the `export-indexed-*` cases and their `-alpha` twins.
- **1c** (`b7ba1382`): Image ▸ Mode ▸ Color Table… (swatch grid, Color well, Transparent, `.act` Load… and
  Save…), one undo step; `get_color_table` and `set_color_table`; the Custom (with Table…) and Previous palettes
  in the sheet and `convert_mode`; Image ▸ Mode ▸ CMYK Color… and Lab Color from an Indexed document directly.
  Tests `indexed_color_table`, `indexed_custom`; drive I15–I21; admission `set-color-table`,
  `to-cmyk-from-indexed`, `to-lab-from-indexed`.
- **2a** (`8bb6008c`): the Indexed Color sheet keeps the user's Colors after Custom or Previous; the Color Table
  dialog shows entries in the document's RGB space; take1 (menus and one Brush stroke, in the status doc) sets the refusals
  (`DISABLED_OPS`, `LATER_PHASES`). Tests `indexed_convert`, `indexed_photoshop_take`; drive I7, I22.
- **2b** (`8ab6ccf2`): the painting edge rule (THRESH, k = 131, the disclosed fallback) in the `indexed_edge`
  kernel and the Brush in Normal mode over it (`indexed_paint`, `brush_stroke` and the canvas tool); Auto Tone,
  Auto Contrast, Auto Color and Equalize refuse for good (read disabled; the controller's ruling of 2026-10-22).
  Tests `indexed_paint`, `indexed_photoshop_take` (the rule replayed on take1's and take2's pixels); drive I7,
  I22–I26; admission `brush-indexed`.
- **2c** (`70362821`): Copy, Copy Merged, Cut and Clear, Fill, Stroke, the Paint Bucket and the Eraser
  (`indexed_paint`, the RGB op re-indexed by `EdgeInk`), and Image Size, Canvas Size, Trim, Image Rotation,
  flip, Crop, Perspective Crop and Fit Crop (`indexed_geometry`: exact on a carrier of the plane for moves and
  Nearest, else resampled and re-indexed); the no-transparent-index refusal; Puppet Warp, Reveal All, Align and
  Distribute move to phase 3, Paste, the transforms, Warp and the layer row to 2d. Tests `indexed_edits`; drive
  I27–I33; admission `rotate90-indexed`, `rotate-indexed`.
- **2d** (`f9550c65`): Paste and Paste Special (`indexed_clipboard`, `rz_precision_doc_indexed_paste`),
  Free Transform, Transform, Distort and Transform Again on the one layer (`indexed_geometry`'s layer
  transforms), the selected pixels' transform and Warp (`indexed_transform`), the layer row's name, label and
  locks, and Export Layers (`indexed_export`); Duplicate Layer refuses for good, a copy transform and every
  layer property but the name by name; phase 2 is done, everything else names phase 3. Tests
  `indexed_transforms`, `indexed_paste`; drive I7, I33–I39; admission `transform-nearest-indexed`,
  `transform-indexed`, `paste-indexed`.
- **3a** (`3efbddb3`): the Move tool (`move_layers`, `move_selected_pixels`, the
  canvas tool), the row's visibility, offset, order and disclosure, the selected pixels of an alpha channel,
  Reveal All, Assign Profile and Convert to Profile (`indexed_profile`), and the background rule in place of
  2c's refusal; a mask target, the layers paste and Puppet Warp refuse for good, Align, Distribute, Delete
  Layer and Flatten Image need more than one layer, everything else names phase 3b. Tests
  `indexed_layer_ops`, `indexed_profile`; drive I22, I23, I28–I30, I33–I35, I38, I40–I45; admission
  `convert-profile-indexed`.
- **3b** (`01e278dd`): the Brush, Fill, Stroke and the Paint Bucket in every blend
  mode; Select ▸ Color Range, Similar, Grow, Modify, Transform Selection, Quick Mask, Save and Load Selection;
  the background swatch as a clear pixel's colour; HDR Toning, Auto-Align and Auto-Blend refuse for good;
  everything else names 3c. Tests `indexed_blend_modes`, `indexed_selections`, `indexed_background`; drive
  I22, I23, I26, I27, I44, I46–I50; no new plan.
- **3c** (`6c7446f2`): Dither Pattern and Noise (declared, unmeasured), Forced
  Custom, the phase 3d palettes refused by name, `render`'s Index and alpha channels, the Magic Wand, Quick
  Selection, the channel commands, Clone Stamp, the History Brush, and every other tool decided for good; the
  swatch key spliced into a request's text; Cut's own JSON error prefix. Tests `indexed_dither`,
  `indexed_tools`, `indexed_background`; drive I22, I23, I50–I58; no new plan.
