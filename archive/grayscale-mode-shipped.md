# Grayscale document mode (#1): shipped runs

The settled record of [#1](https://github.com/kpgalligan/doctored/issues/1)'s Grayscale phases: decisions in force
and each shipped phase's few lines, added by the batch that ships it. [grayscale-mode-status.md](grayscale-mode-status.md)
keeps the head, unshipped contracts and `## Open items`. Takes live under `rasterize-fixtures/photoshop/`, runners in
`scripts/photoshop-probe/`.

## Decisions in force

- **Storage is the RGB precision document with R = G = B.** A Gray document is a `PrecisionDocument` whose
  `gray` policy (`GrayPolicy`: the Gray ICC bytes verbatim, the declared depth, the RGB twin) is `Some`. 8 and
  16 bits store u16 (8-bit stores each code ×257 and every conversion into it rounds to that grid); 32 bits
  stores the linear-light float, R = G = B = relative luminance `Y`. No new pixel type, no new compositor.
- **The twin is the view.** `PrecisionDocument::profile` and every layer's profile is the policy's RGB twin
  (`icc_builtin::twin_blob`): sRGB's D50 columns and `chad`, the Gray profile's `desc` and `kTRC` bytes verbatim
  as its three TRCs, the Gray profile's header version. A neutral pixel's luminance is then `kTRC(code)`, so the
  view, render, sample and compositor are the RGB ones unchanged.
- **Built-in Gray profiles.** The seven Color Settings spaces (Dot Gain 10–30 %, Gray Gamma 1.8/2.2) and sGray
  are built by `icc_builtin::gray_blob`: v2.1 `mntr` GRAY/XYZ, `desc` = the space's display name, `kTRC` a
  4096-entry `curv` of `gray_working::linear` (sGray: sRGB's own 1024-entry IEC table). No Adobe file is read.
- **Mode ▸ Grayscale follows `gray-luminance/take1`'s measured rule:** the source profile's relative PCS `Y`
  carried into the Gray space (`Y^e`; sGray the IEC encode), through `cmyk_color::document_xyz` at 8/16 bits
  and the source profile's Y row on linear floats at 32. **Confirmed** against Photoshop 26.10's Mode ▸
  Grayscale (`gray-mode-547/take1`, `scripts/photoshop-probe/gray_mode_take.py`): Dot Gain 20 % and Gray
  Gamma 2.2 at 8 and 16 bits, max / mean residual ≤ 0.625 / 0.245 8-bit codes (gate 2 / 0.5; Photoshop
  dithers its 8-bit conversion); `--judge` holds Rasterize's own conversion to the same gate.
- **CMYK → Grayscale goes through 16-bit ProPhoto RGB** (relative + BPC) in one job; Gray → CMYK is the RGB → CMYK
  conversion (`cmyk_convert`) on the twin's R = G = B, at the declared 8 or 16 bits (since 5b; 32 bits refuses).
- **Which files open as Gray:** exactly those `rz_path_gray_source` answers for (header only): a PNG of colour
  type 0/4 at 8/16 bits, a BlackIsZero TIFF of 1–2 unsigned 8/16-bit samples, a one-component 8-bit JPEG, each
  with no profile or a usable Gray ICC (a PNG's `gAMA`/`cHRM` only beside `sRGB` or that ICC). An embedded Gray
  ICC is kept; a PNG `sRGB` chunk is sGray; no profile is the working Gray under an import warning. Every other
  file keeps today's open (a gray PNG with an RGB `iCCP` or a bare `gAMA` refuses or opens flat as before).
- **`.rz` word 106** sits in the CMYK slot (never beside a CMYK word); its `GRAY` table (schema 1, depth, ICC)
  is the last before LIDS. `RZDC_VERSION` stays 89: 106 is a conditional word like 90–105. The reader requires
  word ⇔ table, depth ⇔ storage, and the body profile byte-equal to the table's twin.
- **The gate:** the core's `gray_document::admit_edit` in `ffi_precision_edit::request` and
  `precision_capability::GRAY_FAMILIES`; the host's `GrayDocument` asked first in `DocumentPrecision` and
  `validatePrecisionItem`. Every refusal is the core's sentence: "`<operation>` is unavailable in a Grayscale
  document until #1 phase `<p>`.", `p` from `gray_document::LATER_PHASES` (host: `GrayDocument.laterPhases`,
  checked against the registry at server start): phase 6 is the only later phase (since 5b). The host's
  paint, adjustment, filter and retouch tools (`GrayDocument.depthRuled`) pass the gray gate, then answer the
  depth's own rules as on an RGB document.
- **Blend modes: 21 in Grayscale, measured against Photoshop** (`GRAY_BLEND_MODES`,
  `NativeLayerBlending.grayModes`; `gray-mode-547/take1`, the same at 8 and 16 bits under both spaces): every
  mode but Darker Color, Lighter Color, Hue, Saturation, Color and Luminosity, which refuse permanently ("The
  `<mode>` blend mode is unavailable in a Grayscale document."), and RGB → Gray's blocker
  reads the same list. They composite through `blend.rs` on R = G = B. A 32-bit Gray document offers their
  intersection with the float document's admitted modes (12), the others refusing with the float sentence;
  the host asks gray before float.
- **K % is the code's complement**, `100 × (1 − code / max)` (`GrayDocument.kPercent`: the Info panel's K
  row and `sample_color`'s `gray_k_percent` at 8 and 16 bits); Photoshop's sampler reads it within 0.39
  points at 8 bits and 0.005 at 16 (take1).
- **Ink is gray by Mode ▸ Grayscale's rule.** `precision_paint::prepare_colors` and `float_paint_ink::linear_inks`
  turn every ink on a Gray document into the colour's luminance code (`GrayPolicy::luminance_code`) or linear `Y`
  (`linear_luminance`), the helpers `gray_image` uses, so a Fill of a colour equals that colour converted.
  The Brush, Fill's `paint`, the Paint Bucket and the Gradient (stops included) take their modes from the
  Grayscale list; the Eraser is `erase_native`.
- **Adjustments run the RGB kernels on R = G = B** (`gray_adjust::GRAY_ADJUSTMENTS`, host `GrayAdjust`): an op
  whose per-channel arithmetic is the same for R, G and B keeps the equality, so no one-channel kernel. Levels,
  Curves, Brightness/Contrast, Exposure, Invert, Posterize, Threshold and Gradient Map run destructively and as
  layers; Shadows/Highlights and Replace Color destructively only ("A `<Name>` adjustment layer is unavailable in
  a Grayscale document: `<Name>` applies to pixels only."); Auto Tone, Auto Contrast and Equalize run
  (Auto at 8/16 bits). The adjustments the table marks `Unavailable` and Auto Color refuse "`<Name>` is
  unavailable in a Grayscale document.", Hue Rotate, Grayscale, Sepia and White Balance "… Rasterize offers it
  on RGB documents only.". Levels' and Curves'
  `r`/`g`/`b` (and the `*_map`s) must be absent or alike; a Gradient Map's stops are stored as the gray of their
  luminance. `with_gray` checks every adjustment record against the table on open. At 32 bits the float
  document's own list applies (it refuses Invert).
- **Filters run the RGB kernels on R = G = B, folded to one channel** (`gray_filter`): `admit_filter` (asked
  by `gray_adjust::admit_selected`) admits every selected filter (Liquify since phase 4), and each filtered
  sample is folded before the coverage mix (`selected_adjustment` at 8/16 bits, `float_filter_apply` at 32) by
  the float plane filters' Result rule (`float_plane_filter::result_of`): the value when R = G = B bit for bit,
  so a kernel that keeps the equality is the RGB twin's result exactly, else the Rec. 709 luma (Lens
  Correction's fringes, the Develop Filter's Temperature/Tint, Depth Blur's Warmth). **Add Noise takes channel 0's
  draw**, one full-amplitude delta (a luma of three independent draws would carry about 0.75 of the Amount).
  **Clouds' colours are greyed** before the kernel by Mode ▸ Grayscale's rule (`gray_adjust::grey_hex`, 8-bit
  hex), as inks and Gradient Map stops are. Take1's other disabled filters (Vanishing Point, Lens Flare, Oil
  Paint, NTSC Colors, HSB/HSL) have no Rasterize op, so nothing refuses them. Fade (`rz_precision_doc_fade`,
  the Fade sheet's popup) takes the Grayscale modes; Repeat Last Filter and Actions pass the same gate.
- **Retouch runs the RGB kernels on R = G = B**: Dodge/Burn, Blur, Sharpen, Smudge, Clone Stamp (Grayscale
  modes), Healing, Spot Healing, Patch and the History Brush. **The Sponge moves gray levels around the middle
  gray**: Saturate `m + 2·(p − m)`, Desaturate `m`, mixed by coverage × strength,
  `m` 0.5 at 8/16 bits and the linear value of code 0.5 at 32 (`retouch_brush::Vibrancy::Gray`); clamped to
  0…1 at 8/16 bits and unclamped at 32, as the RGB float Sponge, so a dark 32-bit Saturate can go negative;
  Vibrance is ignored. Rasterize's rule: no take measured Photoshop's magnitude. **The History Brush paints a
  Gray document from a Gray state only**, and a clone from plates takes Gray plates only (refused, not
  converted). **Remove Red Eye** (`red_eye`, `red_eye_auto`) refuses for good: Rasterize offers it on RGB
  documents only.
- **Phase 2e's tools run on R = G = B through the RGB kernels.** Stroke… (its mode in the Grayscale list),
  Content-Aware Fill and Move and Import Channel need nothing more; the Remove model's composited pixel and
  each HDR Toning pixel are folded by the filters' rule (`gray_filter::fold`). **HDR Toning keeps the
  document Gray at its own 8 or 16 bits** (no narrowing); a 32-bit one tones into 16 bits, or 8 through the Gray
  depth edit (5a). **The Art History Brush
  runs at 8 bits only** (`gray_art_history`, a 16-bit Gray document refuses as a 16-bit one), from a Gray
  state, its modes Normal, Darken and Lighten. **RGB → Gray carries every adjustment layer the Gray table
  admits** (a Gradient Map's stops greyed from the source profile, `gray_adjust::converted_description`);
  a refused record is named "“`<name>`” cannot become Grayscale: `<reason>` Flatten the image to convert
  it", and a carried one still meets the effect-mode, Blend If and blend-mode blockers.
- **Profile and depth edits are the RGB ones on the twin** (5a, `gray_profile`): no Gray converter. Convert
  rewrites R = G = B from the converted R (on the ×257 grid at 8 bits); Assign keeps every sample; 32 → 8/16 is HDR
  Toning's; the RGB profile exports refuse a Gray document.
- **Artwork is gray** (6a-i, `gray_artwork`). A type, shape or fill description stores each colour as its gray by
  the Mode rule, decided once when the host places it (`RasterDocument.grayArtwork`, `rz_precision_doc_gray_artwork_
  colors`): an sRGB colour with r = g = b stays, any other becomes the unrounded sRGB value of its luminance
  (`gray_style::srgb_gray`); a linear one (32 bits, a fill's `linear`, `stop_linears`) its luminance `Y`
  (`linear_gray`); a custom pattern tile's non-neutral texels the twin's gray code (`grey_pattern`, the Pattern
  Overlay rule); a stop hex `CmykArtworkHex.six` of its gray. The one RGB renderer runs unchanged in the twin, and
  `artwork_unlocked` folds what it publishes (`fold_artwork`: `gray_image` with `keep_neutral`), so a colour glyph
  stores its gray and an 8-bit artwork layer is on the ×257 grid, alpha too. Gray → RGB and → CMYK, Assign and
  Convert re-render captured layers from their gray colours; Bits/Channel keeps them. Paths and vector masks need
  nothing: they persist as on RGB.
- **Comps, links, snapshots and the carry** (6a-ii, `gray_comps`). Every comp rule is `doc_comps`'s; a Gray
  Apply skips a recorded blend outside the Gray list (32 bits: the float list) and a recorded style with a refused
  effect mode or a non-Gray Blend If, leaving the layer's own value. RGB → Gray carries comps, each recorded style
  greyed as the layers' own (`gray_style::greyed_json`, every tile texel mapped); a flatten clears them. Links are
  `StructureEdit` bookkeeping, snapshots the Gray `.rz` (`EditingStateArchive`). The legacy `PaintEdit` ops run
  unchanged (`clone`/`heal` keep R = G = B, `erase` touches alpha, `rasterize` inks greyed), u16 storage only, so
  32 bits refuses "high-depth paint requires u16 storage"; Edit ▸ Clear reaches `erase`. RGB/CMYK → Gray converts
  a described layer as its pixels, description dropped, and the host re-renders each capture through
  `nativeArtwork`'s Gray arm (a CMYK source's inks on the pixels' `rgbStageOptions` leg).
- **Outside colour is folded at publication** (6b). A result whose colour comes from outside the document — Super
  Zoom's new document, JPEG Artifacts Removal's layer, every Generative Fill/Expand/Variations picture, the Merge to
  HDR image — goes through `gray_artwork::folded_result` (`fold_artwork` under the document's policy; Merge to HDR's
  at 32 bits), so `gray_convert::gray_image` stays the Mode rule's one home; its plan carries `fold_bytes` (8 B/px, 16
  at 32 bits). The model and provider inputs are the twin's R = G = B pixels, as the view shows them. Apply Image
  onto a Gray layer is one plane: the layer's luminance blended with the source plane, written through `scalar_edit`
  target `gray` (component 0 read, R, G and B written; `replace_canvas` only); Histogram reports the one Gray row;
  Stack Modes and the stack tools run on R = G = B. A later file of a Gray stack (Load Files into Stack, Merge to
  HDR's files, Merge Panorama) is converted into the stack's Gray profile and depth first (`GrayLayerClip.converted`).
- **Neural Colorize and Sky Replacement refuse for good** (6b, Rasterize's decision, not measured on Photoshop):
  Colorize's colour has nowhere to go in a one-channel layer, and Sky Replacement's New Layers output carries colour
  Curves the Gray adjustment table refuses (`GrayDocument.colorizeRefusal`, `skyRefusal`).
- **8-bit Gray paints and adjusts off the ×257 grid** (it stores u16), and an 8-bit save rounds `round(v/257)`,
  as CMYK's 8-bit document does.
- **Saves and exports keep one channel.** Profile `document` with PNG, TIFF or JPEG is the kept-gray export
  (`gray_export`): the RGB stages to sharpen, then R (and alpha, only when some pixel is not opaque) of the u16
  composite or its 8-bit reduction, JPEG flattened over white, the Gray ICC bytes embedded verbatim. Every
  host save path (`CapturedSave`, `save_copy`) goes through `PrecisionExportCapture`'s gray branch to those
  stages; the default depth is 16, or 8 for JPEG and an 8-bit document. A preserving PNG/TIFF/JPEG save needs
  one visible Normal raster layer at full opacity with no mask or style (JPEG: an 8-bit document). At 32 bits
  (3c) a kept-gray TIFF is the float stage's one-channel float TIFF (`float_io::encode_float_gray`: R, plus
  alpha only when some pixel is not opaque; f32 or f16), its default depth there 32; a 32-bit PNG or JPEG has
  no form and refuses.

## Shipped phases

### 1a — the model, New, open, convert, view, `.rz` (shipped, `fa9c7b5b`)

New ▸ Grayscale at 8/16/32 bits in any built-in space or a Gray ICC; a gray PNG, TIFF or JPEG opens as a Gray
document; Image ▸ Mode ▸ Grayscale from RGB (8/16/32 bits) and from CMYK, and RGB Color back, one undo step each,
over MCP (`convert_mode` `gray`, `gray_space`) and in Actions; `.rz` round-trips at every depth; CMYK phase 1's
layer structure edits. A record a Gray document cannot hold is named, and Flatten converts. Tests
`gray_document.rs`, `gray_files.rs`; drive `scripts/verify_gray_mode.py`. Admission: `scripts/gray-admission/
measure.py` holds the opens, New and both conversions through `gray_io::OPEN_BYTES_PER_PIXEL` (41) and
`gray_convert::TO_GRAY_BYTES_PER_PIXEL` (41); a later slice's new plan joins that drive.

### 1b — layer structure and the one-channel saves (shipped, `705509ee`)

New Layer, groups, masks, clipping, merges, Stamp Visible, Fill Opacity, knockout, the Grayscale blend modes and
Edit ▸ Fill; one-channel PNG, TIFF and JPEG saves, `save_copy` and `export_image` with the Gray ICC. Tests
`gray_structure.rs`, `gray_export.rs`; drive G9–G14. Admission: the three export cases hold the kept-gray plan
through `gray_export::EXPORT_BYTES_PER_PIXEL` (4).

### 1c and phase 2's take (shipped, `fb2a97f1`)

The Channels panel's one Gray row, the Info panel's K row, `get_precision`'s own lists (`GrayDocument.support`),
`render channel gray`, `sample_color`'s `gray_k_percent`, Export As / Quick Export through the kept-gray export;
drive G15–G18. Take `gray-mode-547/take1` (`scripts/photoshop-probe/gray_mode_take.py`, Photoshop 26.10.0):
luminance CONFIRMED, K MEETS, 21 blend modes. **Disabled in Grayscale**: Vibrance, Hue/Saturation, Color Balance,
Black & White, Photo Filter, Channel Mixer, Color Lookup, Selective Color, Desaturate, Match Color, Auto Color,
Vanishing Point, Lens Flare, Oil Paint, NTSC Colors, HSB/HSL; Replace Color and Neural Filters stay enabled.

### 2a-ii, 2b — painting and adjustments (shipped, `6e6e49e3`)

Brush, Eraser, Paint Bucket, Fill's `paint` and the Gradient at 8/16/32 bits; every adjustment of the table,
destructively, as a layer and through the selected edit. Tests `gray_paint.rs`, `gray_adjust.rs`; drive G19–G24.

### 2c — filters, Fade, Actions, retouch (shipped, `5b4ede91`)

Every filter but Liquify, Fade, Repeat Last Filter, Actions and the retouch tools at 8/16/32 bits. Tests
`gray_filter.rs`, `gray_retouch.rs` (oracle: the RGB twin, folded); drive G25–G27.

### 2d — layer effects, Blend If, alpha-channel targets (shipped, `833f7455`)

Every effect at 8/16/32 bits through the RGB style path on R = G = B, each colour stored as its gray
(`gray_style::greyed`); Blend If on Gray only; the Gray target is the layer, colour planes refuse for good
(`PLANE_REFUSAL`), alpha channels follow the mask everywhere. RGB → Gray carries effects and a Gray Blend If greyed
(`grey_records`, tiles charged by `tile_bytes`); the open check is `gray_style::check_records`. Tests
`gray_style.rs`, `gray_channels.rs`; drive G28–G31.

### 3a, 3b and their takes — gray PSD, PSB and layered TIFF (shipped)

3a (`e68eec52`): an 8/16-bit mode-1 PSD, PSB or big-endian layered gray TIFF opens as a Gray document
(`psd_import_gray`); Photoshop keeps a Grayscale Levels in record 1 and Blend If in slot 1 (measured on take1's files,
`core/tests/fixtures/psd_photoshop/gray/`). 3a's take (`gray-files-547/take1`): Photoshop crashed opening our first
writer's file; that writer was removed. 3b's import fix (`7d1dc2f0`, `psd_import_gray::imports_nothing`), take2
MISSES, 3b-ii (`4e293c02`: the writer in core, `psd_export_gray.rs`; the jsx readback fix) and take3 INVALID led to
take4 PASS (`gray-files-547/take4`, `scripts/photoshop-probe/gray_export_take4.py`: the clipped-layer rule, 26 of 26
rasters within bound). 3b (`0f4e93a7`): `export_psd` / `inspect_psd_export` write a mode-1 PSD, PSB or layered TIFF
at 8 or 16 bits. Tests `gray_psd.rs`, `gray_psd_photoshop.rs`, `gray_psd_export.rs`; drive G32–G35.

### 3c — 32-bit gray files (shipped, `f48f53c1`)

A 32-bit gray PSD embeds `icc_builtin::gray_linear_twin` of the Gray profile (an identity-`kTRC` profile is kept
verbatim), and on open a twin naming a built-in relabels to it (`gray_linear_twin_source`); samples never change.
`psd_import_gray32` opens one (open32's structure, gray rules in float form), `save_gray_psd32` writes one (one
channel, mode 1, no `Mt32`; layered TIFF refused); a kept-gray TIFF at 32 bits is a one- or two-sample float TIFF,
and `from_float_tiff` opens one as a 32-bit Gray document. Tests `gray32_files.rs`; drive G35–G36.

### 2e — the rest of phase 2 (shipped, `27c06fc0`)

Stroke…, Content-Aware Fill/Move, Remove (model folded, 8/16 bits), the 8-bit Art History Brush
(`gray_art_history.rs`), HDR Toning at 8/16 bits, Import Channel, RGB → Gray with adjustment layers, the Gray Mode
popups; default phase 6. Admission: `art-history-gray-8` plan/E 1.341, `to-gray-16-adjustments` 1.714. Tests
`gray_tools.rs`; drive G9, G37–G41.

### 4 — structure, geometry, selections, clipboard (shipped, `eefdb4e9`)

Geometry, the selected pixels, the selections, Duplicate to Document, Layer Via, Stamp Selected and Merge Group on
R = G = B (`admit_edit`, `GRAY_FAMILIES`); a Gray warp is applied at once. The clipboard is `gray_clipboard` (RZCB
v5; across modes by the Mode rules, Don't Convert refuses across modes; the one-channel pasteboard TIFF both ways;
a layers clip crosses on the host, `GrayLayerClip`). Admission: `paste-rgb-into-gray-16` plan/E 1.637,
`copy-gray-16` 1.936. Tests `gray_geometry.rs`, `gray_clipboard.rs`; drive G42–G47.

### 5a — profiles, Bits/Channel, 32-bit HDR Toning (shipped)

Edit ▸ Assign / Convert to Profile between Gray profiles (the eight built-in spaces, an installed or file Gray ICC;
an RGB or CMYK profile refuses by name, `gray_profile::non_gray_refusal`), Image ▸ Mode ▸ 8/16/32 Bits/Channel (one
undo step each; 32 → 8/16 refuses, `FLOAT_DEPTH_REFUSAL`, and goes through HDR Toning) and HDR
Toning of a 32-bit Gray document into 16 bits, or 8 through the Gray depth edit inside the same job, over the UI
(`GrayProfileSheetController`, the Mode menu), MCP (`assign_profile`, `convert_profile`, `convert_precision
gray8|gray16|gray32`, `hdr_toning`) and Actions. All on the twin through the RGB kernels (`gray_profile.rs`): Assign
the RGB Assign, Convert the RGB conversion twin → twin with R = G = B rewritten from R (`eight` at 8 bits; at 32 bits
intent and BPC do not apply, linear Y being the same under every twin), Depth `eight`/`map_cmyk_tree` and
`promote_to_float`. The four RGB profile exports refuse a Gray document (`RGB_PROFILE_EDIT_REFUSAL`). Admission
(Release, 3 runs + baseline, `RZ_VERIFY_APP=build/Build/Products/Release/Rasterize.app python3 -B scripts/gray-admission/
measure.py --runs 3 --cases assign-gray-16,convert-gray-16,depth-gray-8,depth-gray-32,hdr-gray-32-16,hdr-gray-32-8`):
plan/E 1.711, 1.283, 1.285, 1.707, 1.766, 1.968; no constant raised. Tests `gray_colour.rs` (oracle: the twin through the RGB exports, each profile's own `kTRC` read from its bytes);
drive G48–G53.

### 5b — proof and Gray → CMYK (shipped)

View ▸ Proof Setup, Proof Colors and Gamut Warning (`GrayDocument.proofSelectors`) and `get_proof_setup` /
`set_proof_setup` / `render_proof` at 8 and 16 bits: the unchanged RGB proof (`soft_proof`, `ProofRenderer`'s
`.display` arm) of the R = G = B composite tagged with the twin; 32 bits refuses as a float document. Image ▸ Mode ▸
CMYK Color and `convert_mode cmyk` from Gray: `convert_to_cmyk` on the Gray document itself, one undo step, the
declared depth kept, blockers and `flatten` as from RGB; Actions replay it. No new plan or constant. Tests
`gray_proof_cmyk.rs` (oracles: the twin's RGB → CMYK and RGB proof, exact; a gamma-1.8 closed form through the
synthetic press's B2A1); drive G54–G56.

### 6a-i — type, shape and fill layers, paths, vector masks (shipped)

The Type, shape and Pen tools, Solid Color / Gradient / Pattern fill layers, Layer Content Options, Rasterize and
Artwork Colors, the Paths panel, Fill and Stroke Path and the vector masks on a Gray document (`GrayDocument`'s
lists, `GRAY_FAMILIES` `rasterize`), over the UI and MCP (`add_/edit_text_layer`, `_shape_layer`, `_fill_layer`,
`rasterize_artwork`, the thirteen path and vector-mask tools); colours stored as their gray ("Artwork is gray",
above), `ink` refusing as on RGB. No RZDC change (descriptions, PATH and vector masks already persist; word 108
free), no new plan or constant. Comps, links, snapshots and the legacy ops are 6a-ii. Tests `gray_artwork.rs`
(oracles: sRGB's published row over the IEC curve; the twin's render of the greyed colour, exact at 16 and 32 bits,
on the grid at 8); drive G57–G62.

### 6a-ii — comps, links, snapshots, the legacy ops, the artwork carry (shipped)

Layer comps (the six tools, Layer Comps window), Link/Unlink Layers, the seven snapshot tools and Snapshots window,
the legacy `erase`/`clone`/`heal`/`rasterize` ops and Edit ▸ Clear (`clear_selection`) on a Gray document
(`GRAY_FAMILIES` `layer_link`, `layer_comp`; `ADMITTED_EDITS`; `GrayDocument`'s lists); RGB/CMYK → Gray keeps type,
shape and fill layers editable and carries comps (decision above). `first_gray_blocker` no longer names a
description; placed images, warps and Generative Layers still block. No RZDC change (word 108 free), no new plan or
constant (`gray_comps::carried_bytes` is a term of the RGB → Gray plan). Tests `gray_comps.rs` (comp and `.rz` round
trips, links on the twin, the comp carry against sRGB's published row), `gray_retouch.rs` (legacy ops against the
twin), `gray_artwork.rs` (the carry against a fresh Gray artwork layer); drive G63–G68 (G7, G9 rewritten).

### 6b — analysis, multi-image, neural and generative tools (shipped)

Histogram (`GrayDocument.histogramReport`: `channels` ["gray"], one row, no luma; the Info panel's Gray plot), Apply
Image and Calculations (sources gray, alpha or an alpha channel; `NativeChannelMath.applyGray`), Stack Modes, Reduce
Noise from Stack, Auto-Align, Auto-Blend, Load Files into Stack, Merge to HDR (files and layers: a 32-bit Gray
document), Merge Panorama, Select Sky, Person Parts and Object Finder, Super Zoom, JPEG Artifacts Removal and Generative
Fill, Expand and Variations on a Gray document (`GrayDocument`'s lists; `GRAY_FAMILIES` `histogram`,
`layer_stack_mode`; `ADMITTED_EDITS` `stack_mode`); a layers clip keeps type, shape and fill layers editable across
modes (`GrayLayerClip`'s artwork carry); the Text session previews each run in its gray (`grayDisplayColor`); Neural
Colorize and Sky Replacement refuse (decision above). No RZDC change (word 108 free), no constant; the plan terms
are `fold_bytes` and Merge to HDR's 16 B/px fold. Tests `gray_analysis.rs` (Histogram, Apply Image, Calculations and
Stack Modes against the twin's RGB path, exact), `gray_neural.rs` (each fold against the twin's result through
Mode ▸ Grayscale, exact; the plan plus 8·n); drive G69–G77 (G9 rewritten).

## Shipped runs

Every entry above is one run and its commit; this list adds nothing.
