#!/usr/bin/env python3
"""The #1 Lab Color phase-1, 1b, 1c, phase-2-take, 2a, 2b, 2c, 2d, 2e, 2f, 2f-second, 2f-third, 3, 3a, 3b, 4a, 4a-ii, 4b-i, 4b-ii, 4c, 5a, 5a-ii, 5b, 6a-i, 6b-i, 6a-ii, 6b-ii, 6b-iii and close MCP drive: Lab
documents are
made New, converted from RGB, Grayscale and CMYK and back to RGB, shown through
the core's view, saved as `.rz` and reopened over MCP on the Debug app; their
layer structure edits (phase 1b's New Layer, groups, masks, blend modes,
clipping, Fill Opacity, merges, Stamp Visible and alpha channels too) are one
undo step each; Fill, the Lab TIFF save and the exports through RGB land on
their gates; phase 1c's readouts — `get_precision`, `sample_color` /
`sample_pixel`, `render`'s Lab channels and `histogram` — answer L*a*b*; phase
2a's Brush, Eraser, Bucket, Fill and Gradient paint L*a*b* and phase 2b's
adjustments run in their Lab forms, each one undo step, and replay through an
Action; phase 2c's filters, Fade, colour adjustments, Desaturate, Auto and
Equalize run on the L*a*b* values; phase 2d's scalar histogram of Lightness, a
and b, filters on a mask and an alpha channel, Color Samplers and RGB → Lab
carrying an adjustment layer answer; phase 2e's retouch tools, Content-Aware
Fill, layer effects and the Lightness, a, b and alpha targets run; phase 2f's
luma filters, Offset's background fill on a plane, Blend If on Lightness, a and
b, the channel Levels carry and Levels Auto on Lightness answer; 2f's second
part's Pattern Overlay and Pattern strokes and Edit ▸ Stroke… run; 2f's third
part's Remove, Art History Brush and Red Eye run; phase 3's Lab arms composite
every mode; phase 3a's Lab TIFF, PSD, PSB and layered TIFF files open as
Lab Color documents; phase 3b's Export as PSD writes them; phase 4a's
resamples and transforms run on the codes; phase 4a-ii's warps, Liquify,
Align and Distribute and the Move tool run on the codes; since the close
(L93–L98), no refusal names a phase; and since L99–L101 every tool is lifted
or refused for good by name: the Magic Wand, the Paint Bucket and Select ▸
Similar and Grow run at both depths by the takes' Lab rules.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. The instance is owned by this file
(`phase5_support.App` on its own scratch MCP socket: a dummy API key and a
PRIVATE directory for every `DR_*_DIR`, through `verification_isolation`'s
`preset_environment` list) and is terminated by its own context manager; no
Doctored process this file did not start is signalled. It never launches
Photoshop. Every fixture is written HERE in pure Python under `--output`.

`render` answers the canvas converted to sRGB; a Lab document's canvas is the
core's Display P3 view of its u16 Lab codes (`lab_convert::lab_display`, the
controller's ruling after this drive's first run missed L3 and L6 with
ColorSync drawing the twin), so it proves the view shows Lab. The gates below
are the ones fixed before that first run, unchanged.

- **L1 New.** `new_document color_mode lab` white at depth 8 and 16 answers
  `summary.color_mode` "lab"; `render`'s centre pixel is ≥ 254 on R, G and B.
- **L2 New 32.** `new_document color_mode lab depth 32` refuses with
  `NEW_32_REFUSAL` exactly.
- **L3 RGB → Lab.** A 16-bit sRGB patch PNG converts (`convert_mode lab`,
  depth 16 kept); `render` is within 2 codes of each source patch on R, G and
  B; undo answers rgb, redo lab (one undo step).
- **L4 Lab → RGB.** `convert_mode rgb rgb_space srgb` on the L3 document
  answers rgb at 16 bits; `sample_pixel` is within 1 8-bit code of each patch.
- **L5 8 bits.** An 8-bit sRGB patch PNG converts to lab (depth 8) and back to
  rgb, and is `rgba8` again.
- **L6 Gray → Lab.** A 16-bit Grayscale document of #808080 converts to lab;
  `render` is neutral (|r − g|, |g − b| ≤ 1) at every pixel.
- **L7 CMYK → Lab.** A 16-bit CMYK document converts to lab in one undo step
  (undo cmyk, redo lab).
- **L8 32 bits.** A 32-bit RGB document refuses lab with text containing
  "convert to 16 Bits/Channel first"; it stays rgb.
- **L9 flatten.** A 16-bit RGB document with a Darken layer refuses lab with
  text containing "is a layer in the Darken blend mode, which a Lab Color
  document cannot hold"; `flatten: true` converts to one layer in one undo
  step (undo restores rgb and both layers). (Hue until phase 3 admitted it,
  updated before phase 3's run; the sentence lost its "yet" in the close,
  updated before the close's run.)
- **L10 .rz.** `save_copy rz` of a 16-bit and an 8-bit Lab document reopens
  (`open_document`) as lab with the same layer count and a `render` whose
  PNG pixels equal the source's exactly.
- **L11 refusals.** Exactly: `save_copy png` → `SAVE_REFUSAL` (phase 4c
  admits `copy`, which stood here refusing with phase 4's sentence; updated
  before 4c's run; phase 4a admits `crop`, which stood here before that),
  `get_tone_map` → `TONE_MAP_REFUSAL` of it, refused for good (the close
  lifts `distort_layer`, which stood here refusing with phase 6's sentence;
  updated before the close's run; phase 6a-ii admits
  `link_layers`, which stood here before; updated before 6a-ii's run; phase
  6a-i admits `add_text_layer`, which stood here before that; updated before
  6a-i's run; 2f's
  third part admits `art_history_brush_stroke`, which stood here refusing with
  `PHASE_2F`, and makes 6 the default phase; updated before its run; phase 2e
  admitted `clone_stamp`, which stood here before that). (Phase 5a converts
  `convert_mode cmyk`, which stood here refusing with `CMYK_REFUSAL`; updated
  before 5a's run. Phase 5a-ii converts `convert_mode gray`, which stood here
  refusing with `GRAY_REFUSAL`; updated before 5a-ii's run.)
- **L12 structure.** On a 16-bit Lab document `duplicate_layer`,
  `set_layer_properties opacity 0.5`, `reorder_layer`, `set_layer_properties
  name`, a second `duplicate_layer`, `delete_layer` and `flatten_image` (of
  the two layers left) each make ONE undo step: undo
  restores `get_document`'s layer list as it was, redo as it became, and the
  document stays lab.
- **L13 1b structure.** On a 16-bit Lab document `new_layer`, `add_layer_mask`,
  `set_layer_properties` Multiply + opacity 0.5, a second `new_layer`,
  `set_layer_clipped`, `set_layer_style {"fill_opacity":0.5}`, `stamp_visible`,
  `merge_down`, `group_layers`, `add_channel`, `rename_channel` and
  `delete_channel` each make ONE undo step (undo restores `get_document`'s
  layers and `list_channels` as they were, redo as they became), and the
  document stays lab throughout; `blend_mode` Subtract and Darken (not offered
  in Lab, phase 2's take; Darken replaced Hue, admitted by phase 3,
  before phase 3's run) refuse with the Lab limitation (`LAB_LIMITATION_HEAD` …
  `LAB_LIMITATION_TAIL`, naming neither).
- **L14 Fill.** On a white 16-bit Lab document a new layer filled
  `fill_selection color #ff0000` renders its centre within `FILL_BOUND8` (2)
  codes of (255, 0, 0); after `add_layer_mask` a `fill_selection target mask
  #000000` renders the centre as the white backdrop's exactly.
- **L15 Lab TIFF.** `save_copy tiff` of 8- and 16-bit white and #808080 Lab
  documents writes PhotometricInterpretation 8, SamplesPerPixel 3,
  BitsPerSample 8/16 and no tag 34675; white's first pixel is (max, 0, 0)
  exactly; gray's |a|, |b| ≤ 1 (8 bits) / ≤ 128 (16 bits, 1/256 units);
  `bit_depth 16` on the 8-bit document refuses with `TIFF_DEPTH_REFUSAL_8`;
  `get_document`'s `save_preservation.save_types` of that plain document holds
  `public.tiff`, and after `add_guide` lacks it, its `losses_by_format.tiff`
  naming "guides and ruler origin".
- **L16 exports.** A Lab document of L3's patches: `export_image png 8 srgb`
  writes colour type 2 or 6 within `EXPORT_BOUND8` (2) codes of `render` per
  patch; `png 16 profile document` writes depth 16, colour type 2 or 6, an
  embedded sRGB profile; `export_preview png` answers an image; `quick_export
  {png, 8, srgb}` writes a PNG.
- **L17 .rz.** `save_copy rz` of the L13 document reopens as lab with the same
  layer count and a `render` whose PNG pixels equal the source's exactly.
- **L18 get_precision (1c).** On L1's New Lab 8 and 16 white: `color_mode` lab,
  `mode` lab8 / lab16, `bits_per_channel` 8 / 16; `supported_tools` equals
  `LAB_TOOLS` (1b's list + the 4 readouts + phase 2a/2b's eight + 2c's four
  + 2d's `get_color_samplers` and `set_color_samplers` + 2e's seventeen + 2f
  second part's `stroke_selection`, updated before 2f-second's run, + 2f third
  part's `remove_stroke`, `art_history_brush_stroke`, `red_eye` and
  `red_eye_auto`, updated before 2f-third's run, + 3b's `export_psd` and
  `inspect_psd_export`, updated before 3b's run, + 4a's fifteen geometry
  tools, updated before 4a's run, + 4a-ii's eight (`get_warp`, `warp_layer`,
  `puppet_warp`, `perspective_warp`, `liquify`, `align_layers`,
  `distribute_layers`, `move_layers`), updated before 4a-ii's run, + 4b-i's
  eight (`move_selected_pixels`, `transform_selected_pixels`, `layer_via_copy`,
  `layer_via_cut`, `stamp_selected`, `save_selection`, `load_selection`,
  `modify_selection`), updated before 4b-i's run, + 5a's `convert_precision`,
  updated before 5a's run, + 4b-ii's `select_color_range` + 4c's six: `copy`,
  `cut`, `copy_merged`, `paste`, `duplicate_to_new_document`,
  `copy_layers_to_document`, updated before 4c's run, + 5b's
  `convert_profile`, `get_proof_setup`, `set_proof_setup`, `render_proof` and
  `save_proof_preset`, updated before 5b's run, + 6a-i's twenty: the
  type, shape and fill layer tools, `rasterize_artwork`, the path tools and
  the vector mask tools, Gray 6a-i's list, updated before 6a-i's run,
  + 6b-i's `apply_image`,
  `calculations`, `stack_mode`, `reduce_noise_from_stack` and
  `merge_layers_to_hdr`, updated before 6b-i's run,
  + 6a-ii's sixteen: `link_layers`, `unlink_layers`, the six layer comp tools,
  the seven snapshot tools and `clear_selection`, Gray 6a-ii's list, updated
  before 6a-ii's run, + 6b-ii's sixteen: `auto_align_layers`,
  `auto_blend_layers`, `quick_select`, the Select Subject family,
  `object_select`, `refine_selection`, `set_quick_mask`, `quick_mask_stroke`,
  `select_sky`, `select_person_parts` and `object_finder_select`, updated
  before 6b-ii's run, + `select_magic_wand` and `select_similar`,
  updated before L99's run)
  sorted and holds `clone_stamp` (phase 2e; it lacked it before);
  `blend_modes` equals `LAB_BLEND_NAMES` + "Pass Through (groups)"; and
  `get_document`'s `precision_support.supported_tools` is equal.
- **L19 samples (1c).** L3's patches converted to Lab at 16 and 8 bits: each
  `sample_color` `lab` is within `LAB_BOUND[depth]` (L*, a*/b*) of this file's
  sRGB → D50 L*a*b* oracle (the profile's Bradford-adapted matrix, the IEC
  curve, the CIE formula); `lab` equals (100·n0, 255·n1 − 128, 255·n2 − 128) of
  its `normalized_rgba` within `LAB_EXACT` (0.001); `srgb_rgba` is absent;
  `sample_pixel`'s `lab` equals `sample_color`'s; and L1's white New Lab 8 and
  16 read (100, 0, 0) within `WHITE_LAB_BOUND` (0.1).
- **L20 render channels (1c).** On the 8-bit patch document `render channel
  lightness|a|b` at each patch equals round(code / 257) of `sample_pixel`'s
  `mean_rgba_codes` within `CHANNEL_BOUND8` (1); `channel red`, `composite` and
  `lightness layer 0` refuse with `RENDER_REFUSAL` exactly; after
  `add_layer_mask hide_all` `channel mask layer 0` reads 0 everywhere; after
  `add_channel` the channel by name reads 0 everywhere.
- **L21 histogram (1c).** On a white 16-bit New Lab: `channels` ==
  ["lightness", "a", "b"] and `bins` has 3 rows; all of `bin_weight_units`'
  weight sits in bins 255 / 128 / 128; no key ends `_luma`; `histogram source
  scalar target red` refuses with `SCALAR_TARGET_REFUSAL` exactly (phase 2d,
  updated before its run; its text names alpha since 2e); after
  `add_layer_mask hide_all`, `histogram source scalar target mask layer 0`
  answers the scalar shape untouched (no `lab_bin_rule`, `channels` not
  ["lightness", "a", "b"]).
- **L22 the measured blend gate (phase 2's take).** `LAB_BLEND_NAMES` is
  `lab_document::LAB_BLEND_MODES` after the take
  (`scripts/photoshop-probe/lab_mode_take.py`, `lab-mode-547/take1`, 2026-10-21,
  Photoshop 26.10.0: 1b's 15 less Subtract and Divide). On a 16-bit Lab
  document with two layers the top layer is set to Multiply (one undo step),
  then `set_layer_properties blend_mode` takes each of the 27
  `ALL_BLEND_NAMES` in Photoshop's menu order: a name of
  `LAB_BLEND_NAMES` that differs from the layer's current mode succeeds in
  ONE undo step (`list_history` `undo_count` + 1), one equal to it succeeds
  with zero; every other name refuses with text starting
  `LAB_LIMITATION_HEAD` and adds no undo step; `get_precision`'s
  `blend_modes` is L18's.

Phase 2a and 2b (L23–L27), fixed before the run. "One undo step" means
`list_history`'s `undo_count` rises by exactly 1, undo restores the sampled
codes and redo returns them.

- **L23 Brush and Eraser.** On a 16-bit white Lab document with a new layer,
  `brush_stroke #ff0000` covering the canvas: `sample_color`'s `lab` at the
  centre is within `LAB_BOUND[16]` of `srgb_to_lab((255, 0, 0))`, one undo
  step; `eraser_stroke` over the same path leaves the layer's alpha
  (`sample_pixel source layer`) 0, one undo step; `brush_stroke blend_mode
  Darken` refuses with text starting `LAB_LIMITATION_HEAD`.
- **L24 Bucket, Fill and Gradient.** On 16-bit white Lab documents: `fill`
  #0000ff (the bucket) and `fill contiguous false` #00ff00 each read within
  `LAB_BOUND[16]` of the colour's `srgb_to_lab` at the centre, and `gradient`
  #000000 → #ffffff along a row reads black's and white's L*a*b* within
  `LAB_BOUND[16]` at its ends with L* never falling along it; one undo step
  each.
- **L25 8 bits and Actions.** On an 8-bit white Lab document `brush_stroke
  #ff0000` reads within `LAB_BOUND[8]` of red's L*a*b* and every code of the
  centre's `mean_rgba_codes` is a multiple of 257; `save_action` of one
  `brush_stroke` and one `apply_filter levels gamma 1.4` step, run by
  `run_action` on a fresh 8-bit white Lab document, gives exactly the codes of
  the same two calls made directly on another.
- **L26 adjustments.** On L19's 16-bit patch document: `apply_filter levels
  gamma 1.4` keeps every patch's a and b codes exactly; `apply_filter invert`
  gives 65535 − c on L, a and b and keeps alpha; `add_adjustment_layer
  threshold level 0.5` composites a = b = 0x8080 and L 0 or 65535 at every
  patch (`sample_pixel`); one undo step each (the first two undone after);
  `save_copy rz` reopens lab with the same layer count and an adjustment
  layer (`is_adjustment`); `convert_mode rgb rgb_space srgb` answers rgb and
  keeps an adjustment layer.
- **L27 refusals (rewritten for 2c).** Exactly: `apply_filter exposure`,
  `add_adjustment_layer black_and_white`, `auto_color` and `hdr_toning` →
  `UNAVAILABLE_REFUSAL` of Exposure, Black & White, Auto Color and HDR Toning;
  `apply_filter sepia` → `RGB_ONLY_REFUSAL` of Sepia; `brush_stroke target
  red` → `PLANE_TARGET_REFUSAL` (since phase 2e: a Lab document's planes are
  lightness, a and b; a phase sentence before).

Phase 2c (L28–L31), fixed before the run, on fresh 16-bit Lab documents of
L19's patches (each a new PNG copy converted with `convert_mode lab`). "One
undo step" is L23's.

- **L28 filters.** `apply_filter blur sigma 2`, `unsharp_mask`, `add_noise`
  and `pixelate` each change some patch sample in one undo step (undone
  after); `clouds #ff0000/#0000ff` keeps every patch's L*, a* and b* between
  red's and blue's `srgb_to_lab` within `LAB_BOUND[16]`. Exactly:
  `lens_correction`, `develop_filter`, `difference_clouds` → `UNAVAILABLE_REFUSAL`
  of Lens Correction, Develop Filter, Difference Clouds; `presence` →
  `DEVELOP_FILTER_REFUSAL` of Presence; the `depth_blur` tool →
  `RGB_ONLY_REFUSAL` of Depth Blur. (Updated before 2f's run: `apply_filter
  emboss` stood here refusing with "Emboss " + `PHASE_2F`; it runs since phase
  2f, L39. Updated before 4a-ii's run: the `liquify` tool stood here refusing
  with "liquify " + phase 4's sentence; it runs since 4a-ii, L62.)
- **L29 Fade and Actions.** After `apply_filter blur sigma 2`, `fade opacity
  0.5` puts every patch code within `FADE_BOUND` (1) of the midpoint of the
  before and after codes, the Fade replaces the blur's one undo step, and one
  undo restores the pre-blur codes; `fade mode
  Darken` refuses with text starting `LAB_LIMITATION_HEAD`. `save_action` of
  `apply_filter blur sigma 2` + `apply_filter hue_saturation {hue 30,
  saturation 0.2}`, run by `run_action` on a fresh patch document, gives
  exactly the codes of the same two calls made directly on another.
- **L30 colour adjustments.** `apply_filter hue_saturation {saturation -1}`
  keeps every patch's L code exactly and sets a and b to 0x8080 exactly;
  `apply_filter levels g {gamma 1.4}` keeps L and b codes exactly and changes
  some a; `apply_filter adjust saturation -0.5` (a scale of 0.5: the kernel's
  `1 + saturation`) keeps L exactly and puts each a*, b* within `HALF_BOUND`
  (0.5) of half its value; `add_adjustment_layer hue_saturation {hue 180}`
  changes `sample_pixel` in one undo step; `add_adjustment_layer
  shadows_highlights` → `PIXELS_ONLY_REFUSAL` of Shadows/Highlights.
- **L31 Auto and Equalize.** On a fresh 16-bit Lab document of L19's patches
  but black and white, `auto_tone`, `auto_contrast` and `equalize` each change
  some L code, keep every a and b code exactly, in one undo step (undone
  after).

Phase 2d (L32–L35), fixed before the run, on fresh 16-bit Lab documents of
L19's patches (each a new PNG copy converted with `convert_mode lab`). "One
undo step" is L23's.

- **L32 scalar histogram.** `histogram source scalar target lightness|a|b
  layer 0` answers `target` equal to the name, carries `lab_value_rule`, and
  its `bin_weight_units` sum > 0; `target red` refuses with
  `SCALAR_TARGET_REFUSAL` exactly.
- **L33 mask and alpha filters.** `add_channel`, `select_rect` over patches
  2–4, `fill_selection target channel:<name> #ffffff` and `deselect`; then
  `apply_filter blur sigma 2 target channel:<name>` changes the channel's
  `render` (by name) in one undo step, at the rect's edge pixels 1 and 5
  among them. On a layer mask (`add_layer_mask hide_all`, the same rect
  filled white, `deselect`) the same holds for `apply_filter blur sigma 2
  target mask` and for `apply_filter emboss target mask` (`render channel mask
  layer 0`). `apply_filter lens_correction target mask` refuses with
  `UNAVAILABLE_REFUSAL` of Lens Correction exactly.
- **L34 Color Samplers.** `set_color_samplers` at patches 0 and 3 answers
  count 2; each row's `lab` equals `sample_pixel`'s `lab` there exactly; and
  `get_color_samplers`' `samplers` equals the reply's. `[]` answers count 0.
- **L35 RGB → Lab carries adjustment layers.** A 16-bit sRGB patch document
  with `add_adjustment_layer hue_saturation {hue 30}` converts with
  `convert_mode lab` to 2 layers, one `is_adjustment`, in one undo step (undo
  answers rgb, redo lab). One with `add_adjustment_layer levels {g {gamma
  1.4}}` converts too, its layer's `adjustment` (`get_document`) unchanged
  (updated before 2f's run: it refused with `CHANNEL_CARRY_TEXT` before).

Phase 2e (L36–L38), fixed before the run, on fresh 16-bit Lab documents of
L19's patches (each a new PNG copy converted with `convert_mode lab`); "one
undo step" is L23's. Content-Aware Fill needs picture around its hole, so its
document is L19's patches tiled over 48 × 32 (`TILE`, patch (x // 6 + y // 4)
mod 8), converted the same way, with the hole (`HOLE`) magenta (255, 0, 255),
a colour found nowhere else. (Fixture corrected after drive-2: the hole first
held the periodic tiling itself, which the fill rebuilt code for code, so the
gate saw no change; the gate is unchanged.)

- **L36 retouch.** A stroke covering the row (`cover36`): `dodge_burn` keeps
  every a and b code exactly, lowers no L code and raises some, in one undo
  step; `sponge_stroke mode desaturate` keeps every L code exactly, moves no
  a or b code away from 0x8080 and some toward it, in one undo step;
  `blur_stroke` changes some code in one undo step; on the tiled document
  `select_rect` of `HOLE` then `content_aware_fill` changes the codes
  inside the rect in one undo step; `clone_stamp blend_mode Darken` refuses
  with text starting `LAB_LIMITATION_HEAD`.
- **L37 effects.** On a white 16-bit Lab document `set_layer_style` of a
  `color_overlay #ff0000` reads, at the centre (`sample_color`'s `lab`), within
  `OVERLAY_BOUND` (0.5 L*, 1.0 a*/b*) of a second white Lab document after
  `fill_selection #ff0000`; (the Pattern Overlay refusal that stood here is
  L43's run since 2f's second part; updated before 2f-second's run) a 16-bit
  sRGB patch document with a
  `drop_shadow` style converts with `convert_mode lab` in one undo step and
  `get_document`'s layer `style` stays non-null.
- **L38 planes.** `fill_selection target a color #000000` sets every a code
  to 0 (a* −128) and keeps L and b exactly; `apply_filter blur sigma 2 target
  lightness` changes some L code and keeps every a and b code exactly, in one
  undo step; `apply_filter emboss target lightness` changes some L code;
  `histogram source scalar target alpha layer 0` answers with
  `bin_weight_units` whose sum > 0; `brush_stroke target red` refuses with
  `PLANE_TARGET_REFUSAL` exactly.

Phase 2f (L39–L42), fixed before the run, on fresh 16-bit Lab documents
converted with `convert_mode lab` ("one undo step" is L23's).

- **L39 luma filters.** A 16 × 8 sRGB document of grays with an L* edge at
  x = 8 and a ±4 code texture (`edge_gray`, neutral: a* = b* = 0): `apply_filter`
  `emboss`, `edge_detect`, `lens_blur`, `reduce_noise` {radius 2, luminance
  0.25, detail 0} and `smart_sharpen` each change some code of row 3 in one
  undo step (undone after); every sampled a* and b* stays within `AB_BOUND_2F`
  (0.5) of 0 (Emboss and Edge Detect by the Lightness-only rule) and some L*
  code moves.
- **L40 Offset fill on a.** On L19's patches, `apply_filter offset horizontal 2
  vertical 0 target a undefined_areas background background_color #ffffff`
  leaves the vacated patches 0 and 1 with a* within `AB_BOUND_2F` of 0, keeps
  every L and b code exactly, in one undo step.
- **L41 Blend If.** L19's patches, `duplicate_layer index 0`, `fill_selection
  layer 0 #808080`; `set_layer_style layer 1 blend_if {channel green,
  this_layer [128, 128, 255, 255]}`: in `render` a patch whose a code is at
  most 0x8080 − 257 shows the gray within `RENDER_BOUND8` (2) of (128, 128,
  128) and one at least 0x8080 + 257 equals its pre-Blend If render within 1;
  both kinds occur. `blend_if {channel gray}` refuses with
  `LAB_BLEND_IF_GRAY` exactly.
- **L42 carries and Auto.** On L19's patches `add_adjustment_layer levels {g
  {gamma 1.4}}`, then `convert_mode rgb rgb_space srgb`: the layer's
  `adjustment` is the one before, exactly. On a fresh Lab document of
  `AUTO_PATCHES` (no black or white) `add_adjustment_layer levels auto {mode
  tone}` adds a layer whose `adjustment.params` holds, of the channel stages
  `r`, `g` and `b`, only `r`; `auto {mode color}` refuses with
  `UNAVAILABLE_REFUSAL` of Auto Color exactly. (Transcription corrected after
  drive-1: it first read "params with only r" as the whole object, which
  always carries the composite's identity black, white and gamma; the spec's
  gate names the channel stages, and its bound is unchanged.) The composite
  `black`, `white` and `gamma` (absent or) identity, 0, 1 and 1, and `r` not
  identity (2f review nit 1; updated before 2f-second's run).

Phase 2f, second part (L43–L44), fixed before the run.

- **L43 patterns.** On `new_lab(16, 8, 4)`: `set_layer_style` of a
  `pattern_overlay` of the 2 × 2 tile red, blue / green, white (`TILE43`,
  custom `rgba`, scale 1) is one undo step and `lab_at` at (0, 0), (1, 0),
  (0, 1) and (1, 1) is `within` `LAB_BOUND[8]` of its texel's sRGB (the tile is
  authored sRGB, each texel 8-bit Lab codes). On a second `new_lab(16, 8, 4)`
  a `stroke` with `fill_type pattern` (inside, size 2, the same tile) changes
  some `lab_at` of row 0 in one undo step. `rgb_patches()` with that
  `pattern_overlay` converts with `convert_mode lab` in one undo step, its
  layer `style` equal to before.
- **L44 Stroke….** On `new_lab(16, 16, 8)`: `select_rect` (2, 0, 12, 7) then
  `stroke_selection width 1 location inside color #ff0000` is one undo step;
  the band pixel (2, 3) is within `OVERLAY_BOUND` of L37's red Fill reading and
  (8, 3), 3 px inside, reads white exactly as before. On a second
  `new_lab(16, 16, 8)`: `new_layer`, `select_rect` (4, 2, 6, 4),
  `fill_selection layer 1 #0000ff` and `deselect`, then `stroke_selection
  layer 1 width 1 location outside #ff0000` (the layer's edge) is one undo
  step and (6, 1), just outside the rect, is within `OVERLAY_BOUND` of L37's
  red Fill reading. `stroke_selection blend_mode Darken` refuses with text
  starting `LAB_LIMITATION_HEAD` (Hue until phase 3 admitted it).

Phase 2f, third part (L45–L47), fixed before the run. The Remove model needs
MI-GAN: before the instance starts, `DR_MODELS_DIR` is the read-only fixture
store `~/devel/doctored-fixtures/models` when it holds a verified
`doctored/migan-512-places2` (`model_store_paths`); otherwise L45's model half is skipped, the
drive prints `SKIP l45 model` and exits 2 after the other gates, so a missing
store is never read as a pass. "One undo step" is `list_history`'s
`undo_count` rising by exactly one; "codes" are `sample_pixel`'s
`mean_rgba_codes`.

- **L45 Remove.** A 64 × 48 sRGB field (60, 140, 90) with a ±6-code checker
  of 4 px cells (`field45`) converted with `convert_mode lab` at 16 and 8
  bits: `remove_stroke mode content_aware points (20, 24)–(44, 24) size 8
  seed 7`, then on a fresh conversion the same with no mode (the model): each
  one undo step and the document stays lab; some pixel whose centre is within
  2 px of the segment changes its codes, and every pixel whose centre is
  farther than 5 px keeps its codes exactly; the model's (32, 24) `lab`
  (`sample_color`) is within `REMOVE_FIELD_BOUND` (5.0 on each of L*, a*, b*)
  of `srgb_to_lab((60, 140, 90))`. (Fixture corrected after drive-1: the
  periodic checker alone let Content-Aware rebuild the hole code for code, so
  the gate saw no change, as L36's first hole did; the field now holds a
  magenta (255, 0, 255) bar, rows 23–25 from x 22 to 42, inside the stroke and
  found nowhere else. The gates are unchanged.)
- **L46 Art History.** `new_document lab depth 8` white 32 × 24, its New
  state's `current_state_id` from `list_history`, `fill_selection #3366cc`,
  then `art_history_brush_stroke` (layer 0, points [[16, 12]], size 5, style
  tight_long, area 30, tolerance 0, seed 4, `state_id` the New state) is one
  undo step and some sampled L* is at least the fill's + 10; on that document
  `blend_mode Darken` answers `ART_DARKEN` exactly and `blend_mode Hue` paints
  in one undo step (it refused until phase 3), and a separate
  `new_document lab depth 16` answers `ART16_REFUSAL`
  exactly. (The RGB-source sentence is `core/tests/lab_tools.rs`'s: an 8-bit
  sRGB state is a byte state, which the host refuses first with its own
  bit-depth sentence.)
- **L47 Red Eye.** A 24 × 16 sRGB gray 128 with a hard (220, 30, 30) disc of
  radius 3 at (12, 8) (`eye47`) converted with `convert_mode lab` at 16 and 8
  bits: `red_eye x 6 y 2 width 12 height 12` is one undo step, (12, 8)'s a*
  drops by at least `RED_EYE_A_DROP` (20) and its L* drops, and (7, 3) keeps
  its codes exactly; on a fresh conversion `red_eye_auto eyes [{x 12, y 8,
  radius 2}]` is one undo step and (12, 8)'s codes equal `red_eye`'s.

Phase 3, L48–L50, fixed before the run. The oracle is `lab_arm`
below, written from `scripts/photoshop-probe/lab_blend_take.py`'s declaration
and take2's verdicts (16 bits: C5 for Multiply and Screen, the a*/b* sum for
Linear Burn and Linear Dodge (Add), C4 for Vivid Light, C2a for Hard Mix,
L*-pick for Darker and Lighter Color, the L*/chroma swaps for Color and
Luminosity, CHEB-H and CHEB-S for Hue and Saturation), on `sample_pixel`'s
u16 codes. "Within a code" is |composite/65535 − oracle|·255 ≤
`BLEND_BOUND8` (1) on each of L, a and b.

- **L48 RGB → Lab with the re-armed modes.** For each of Multiply, Screen,
  Linear Burn, Linear Dodge (Add), Vivid Light and Hard Mix: the 16-bit
  `PATCHES` row, a `new_layer` filled `BLEND_TOP` (#4a9ad0) at opacity 0.6 in
  that mode, `convert_mode lab`: lab in one undo step (undo rgb, redo lab);
  each composite pixel is within a code of `lab_arm` over the two layers' own
  codes (`sample_pixel source layer`).
- **L49 the six that joined.** On that conversion's Lab document (Hard Mix's),
  `set_layer_properties blend_mode` Darker Color, Lighter Color, Hue,
  Saturation, Color and Luminosity at opacity 1 are one undo step each and each
  composite pixel is within a code of `lab_arm`.
- **L50 what still refuses.** `set_layer_properties blend_mode Darken`
  answers text starting `LAB_LIMITATION_HEAD` and ending
  `LAB_LIMITATION_TAIL`, and no tool description in `tools/list` contains
  "until phase 3" next to a blend mode ("blend mode" or "blend_mode" in the
  same note).

Phase 3a, L51–L54, fixed before the run. Expected codes come from the
published encodings (`lab_tiff_codes` below: TIFF 6.0 §23 CIELab, L* =
100v/255 or 100v/65535, a*/b* the signed byte or the signed 16-bit value /
256; TIFF Technote 4 ICCLab, 8 bits L* = 100v/255 and a* = v − 128, 16 bits
L* = 100v/65280 and a* = v/256 − 128), as the document's ICC v4 codes.

- **L51 flat Lab TIFFs open.** Python-written CIELab 8 (MM), CIELab 16 (II,
  SampleFormat [1, 2, 2]), ICCLab 8, ICCLab 16 and CIELab 16 with an
  unassociated alpha (ExtraSamples 2), 8 × 4 each: `open_document` gives
  `color_mode` lab and `get_precision` `mode` lab{depth}; `sample_pixel
  source layer` at 4 points equals the expected codes exactly at 8 bits and
  within 1 code at 16. An 8-bit CIELab TIFF with an ASSOCIATED alpha
  (ExtraSamples 1) does not open as lab.
- **L52 the Lab TIFF round trip.** `save_copy tiff` of a Lab 8 and a Lab 16
  document (the patch row converted), then `open_document`: lab at that
  depth, and `sample_pixel` at 5 points: every channel equal at 8 bits; at
  16 bits L* and alpha equal and a*/b* within 1 code (the CIELab 16-bit
  write/read pair returns code + 1 when code mod 257 = 128).
- **L53 Photoshop's files** (`lab-files-547/take2`, read in place; prints SKIP
  with the path when it is absent): `inspect_psd` of `layers16.psd` answers
  `import_precision` "lab", `color_mode` "Lab" and `layered_available` true;
  `open_document psd_mode layers` of `layers8.psd`, `layers16.psb` and
  `layers8.tif` opens lab at the file's depth with the layer count of its
  PSD twin (`layers8.psd`, `layers16.psd`) and `sample_pixel` at all 12 cell
  centres equal to the twin's; `psd_mode composite` of `layers16.psd` gives
  one layer; in `styled8.psd` the "Overlay" layer keeps its color_overlay.

Phase 3b, L54–L57, fixed before the run (the writer shipped by
`lab-export-547/take1`'s PASS). The scene (`lab_scene` below), 16 × 12 at
the given depth: a Base filled with two sRGB halves, `Top` (a rectangle with
a Color Overlay #40a0c8), `Ranged` (Blend If on Lightness, `red`, underlying
0/0/96/160), `Masked` (a mask from a selection) in a group `Group`, and a
Levels adjustment layer `Levels` on top. Headers and tags are read here
(`psd_facts`, `tiff_facts`) from Adobe's framing and TIFF 6.0.

- **L54 the save refusal names Export as PSD.** On a Lab document
  `save_copy psd` answers exactly `SAVE_REFUSAL`.
- **L55 the 16-bit export.** `inspect_psd_export` (psd, layers) answers
  available true, depth 16, color_mode lab. `export_psd` psd, psb and tiff
  (layers) and psd (flattened) each write: a PSD/PSB with colour mode 9,
  depth 16, 3 header channels, a positive record count and no resource 1039;
  a TIFF with Photometric 8, 3 samples, BitsPerSample 16 and no tags 34675
  or 338. Each layered file reopens (`inspect_psd` import_precision lab,
  `open_document psd_mode layers`) as color_mode lab at 16 bits with the same
  layer names in the same order and the same kinds except `Levels`, which
  comes back a layer (a raster) — and the layers reply's `losses` name
  `Levels`; `render channel lightness`, `a` and `b` of every reopened file
  (the flattened one too) are within 1 code of the source's at every pixel.
- **L56 the 8-bit export.** The same at 8 bits for psd and tiff (layers):
  mode 9, depth 8, 3 channels, positive count, no 1039; TIFF Photometric 8,
  3 samples, no 34675/338; the reopened files' names, order and kinds as
  L55, and `render channel lightness|a|b` EQUAL to the source's.
- **L57 the style travels.** `inspect_psd` of the 8-bit PSD export observes
  the `Top` layer's effects with `color_overlay` and the `Ranged` layer's
  Blend If ranges carried, and the reopened `Ranged` keeps Blend If on
  channel `red` with underlying [0, 0, 96, 160].

Phase 4a, L58–L61, fixed before the run. The fixture is a Python-written
CIELab TIFF (`write_lab_tiff`, Photometric 8, no alpha), 24 × 16, an L* ramp
across at a* = b* = 0, at 8 and 16 bits, opened by `open_document`. "One step"
(`geo_step`, Gray G42's) is: `list_history` `undo_count` + 1, the document
stays lab, every probe sample (`sample_pixel` at `PROBE58`) with alpha 65535
has `center_rgba16[1] == center_rgba16[2] == NEUTRAL16` (32896), and one
`undo` restores the canvas size and the probe samples exactly.

- **L58 geometry.** On the 16-bit file each is one step: `image_size` 30×20,
  `canvas_size` 30×20 center, `crop` 2,2,16×10, `crop` 2,2,16×10 with angle 7,
  `rotate` 90, `rotate` 17, `flip` horizontal, `perspective_crop` (16×10);
  and `image_size` 30×20 on the 8-bit file.
- **L59 transforms.** On the 16-bit file each is one step:
  `transform_layer` rotate 30 bilinear; `transform_layers` (a matrix);
  `transform_again`; `add_layer_mask` then `transform_mask`;
  `straighten_layers`; `transform_layer` translate_x 6 then `reveal_canvas`
  (the canvas becomes 30 wide); `canvas_size` 30×20 then `trim` on
  transparent pixels (back to 24×16); and `select_rect` then
  `transform_selection` angle 15 changes the selection's bounds and leaves
  the probe samples unchanged.
- **L60 crop past the canvas.** `new_document lab` 12×8 `#808080` at 16 and
  8: `crop` x −3, y −2, 18×12, `delete_cropped` true, `background_color
  #3366cc` is one step, answers `extended` true, `sample_pixel` (0, 0) `lab`
  is within `LAB_BOUND[depth]` of `srgb_to_lab((0x33, 0x66, 0xcc))` and the
  old centre's codes are unchanged; after its undo, `fill_selection #3366cc`
  (no selection) is one step and its centre's `lab` is within
  `LAB_BOUND[depth]` of the same colour (fix pass 1: at 16 bits it read the
  8-bit codes, 44.314/11/−59); the same crop with `fill content_aware`
  answers without error and stays lab; with `fill generative_expand` it
  clears the gate (`GATE_PARTS`, L86's rule) and the mode stays lab (updated
  for 6b-iii: it answered exactly "generative_expand " + `PHASE_6`).
- **L61 the wand runs.** After `select_rect`, `transform_again`
  is one step (`geo_step`: a* = b* = 0 kept); `select_magic_wand` (2, 2)
  tolerance 10 answers ok and the mode stays lab (updated before L99's run:
  it answered `AWAITING` for `select_magic_wand`; updated before
  4c's run: it and `copy` answered "<tool> " + phase 4's sentence; `copy` runs
  since 4c, L69). (Updated before 4a-ii's run: `warp_layer`, `liquify`,
  `align_layers` and `move_layers` stood in that list; they run since 4a-ii,
  L62–L63. Updated before 4b-i's run: `transform_again` over a selection
  answered "transform_selected_pixels " + phase 4's sentence; it runs since 4b-i.)

Phase 4a-ii, L62–L63, fixed before the run, on L58's fixture and `geo_step`.
Every earlier document is closed before L62, so L62 and L63 run beside no
other document (setup only; no gate changed). Runs 1 and 2 missed L62:
`warp_layer` on the 24 × 16 file was refused because the Lab view's reserve
was larger than the document's display plan, fixed in the core
(`PrecisionDocument::display_scratch_bytes`' Lab arm) before run 3.

- **L62 warps.** On a fresh 16-bit file each is one step: `warp_layer` set
  (layer 0, pixels, 3×3, the centre node [2, 1.5], bicubic; after: `changed`
  true and `get_warp`'s `field` null); `puppet_warp` pins [{from [8, 8], to
  [10, 9]}, {from [17, 8], to [17, 7]}]; `perspective_warp` quads [{from [3, 2,
  21, 2, 21, 14, 3, 14], to [4, 3, 22, 1, 20, 15, 2, 13]}]; `liquify` (L61's
  former strokes); and `liquify` on the 8-bit file. Then after `select_rect`
  2,2,8×6 each is one step through `geo_step`: `puppet_warp` with `pins62s`
  [{from [4, 4], to [5, 5]}, {from [8, 6], to [8, 5]}], `perspective_warp` with
  `quads62s` [{from [3, 3, 9, 3, 9, 7, 3, 7], to [4, 3, 9, 2, 10, 8, 3, 7]}]
  (each pin and corner inside the 2..10 × 2..8 selection; `pins62`'s (17, 8)
  lies outside a selection mesh, which Puppet Warp refuses), and
  `move_selected_pixels` dx 2 dy 1. (Updated before 4b-i's run: with the
  selection the three answered "transform_selected_pixels " or
  "move_selected_pixels " + phase 4's sentence.)
- **L63 align, distribute, move.** On a fresh 16-bit file, set up with plain
  calls: `canvas_size` 60×24 anchor `top-left`; `duplicate_layer` of layer 0
  twice (layers 0–2 hold the ramp at (0, 0)); `move_layers` [1] dx 14;
  `move_layers` [2] dx 36 dy 8. Each is one step: `align_layers` [0, 1, 2]
  edge `top` (after: every `moved` offset_y 0); `distribute_layers` [0, 1, 2]
  axis `horizontal` (after: layer 1's offset_x 18); `move_layers` [1] dx 2 dy 1
  (after: layer 1 at (16, 1)); `move_layers` [1] dx 3 dy 0 `duplicate` true
  (after: `get_document`'s layer count 4, the copy at (17, 0)). If layer 0
  refuses a move for a lock, a third duplicate takes its place (noted).

Phase 4b-i, L64–L65, fixed before the run, on L58's fixture and `geo_step`.
Every earlier document is closed before L64 and again before L65, so each
opens a fresh file (setup only; no gate changed). Run 1 missed L64: its
`geo_file` answered L63's open document, already 60 × 24.

- **L64 selected pixels.** On a fresh 16-bit file with `select_rect` 2,2,8×6,
  each is one step: `transform_selected_pixels` scale 1.25;
  `move_selected_pixels` dx 2 dy 1 `duplicate` true; `layer_via_copy` (after:
  `get_document`'s layer count + 1); `layer_via_cut`; and, after a plain
  `duplicate_layer` of layer 0, `stamp_selected` of [0, 1].
  `move_selected_pixels` target `lightness` is one step too (since 6b-i,
  updated before its run: it answered `SELECTED_PLANE_REFUSAL`).
- **L65 Save and Load Selection.** On a fresh 16-bit file with `select_rect`
  2,2,8×6, `save_selection` name `Sel65` is one step (after: `list_channels`
  holds one more channel); it is then saved again with a plain call. After
  `deselect`, `load_selection` from `channel` `Sel65` and `load_selection` from
  `plane` `lightness` each answer `ok` with `get_selection`'s `selection_empty`
  false and the mode still lab; `load_selection` plane `red` answers exactly
  `LOAD_PLANE_REFUSAL`; `modify_selection` operation `invert` answers `ok`.

Phase 5a, L66–L67, fixed before the run. Every earlier document is closed
before L66 (setup only, L64's `close_all`). `COLOUR66` is a mid colour inside
sRGB.

- **L66 Lab → CMYK.** A 16-bit `new_document color_mode lab` of `COLOUR66`
  `convert_mode cmyk` answers `color_mode` cmyk and `depth` 16 in one undo
  step: undo answers lab, redo cmyk. An 8-bit one answers `depth` 8. The
  16-bit document's `sample_color` `ink16` is within `INK_BOUND66` (2 · 257)
  per ink of the same colour taken `convert_mode rgb rgb_space srgb`, then
  `convert_mode cmyk`. A 16-bit Lab document with `add_adjustment_layer` levels
  named `Levels66` refuses cmyk with exactly `ADJUSTMENT_CMYK66`; `flatten`
  true converts it to a one-layer cmyk document.
- **L67 Bits/Channel.** On a 16-bit Lab document of `COLOUR66`
  `convert_precision mode lab8` is one undo step: `get_precision`'s
  `bits_per_channel` 8 and every `sample_pixel` code at `POINTS67` a multiple
  of 257; undo answers 16 and the codes before; redo 8. `lab16` then answers
  16 with the 8-bit codes kept. `lab32` answers exactly `LAB32_REFUSAL`;
  `dry_run` true (lab8, on the 16-bit document) answers `dry_run` true,
  `changed` true and a `working_memory_plan_bytes` > 0, the document still 16
  bits. `save_action` of [`convert_precision lab8`, `convert_mode cmyk`],
  replayed by `run_action` on a fresh 16-bit Lab document of `COLOUR66`, gives
  exactly the `ink16` at `POINTS67` of the same two calls made directly.

Phase 4b-ii, L68 (numbered after 5a's L66–L67), fixed before the run. take1 (`lab_select_take.py`) shipped
sampled Color Range (its C2 rule) and neither the Magic Wand nor Select
Similar, so L61's Magic Wand refusal stands unchanged. Every earlier document
is closed first (setup only).

- **L68 Color Range.** On a fresh 16-bit Lab `new_document` (12 × 8,
  background `#808080`), `select_rect` 3,2,5×4 is filled with `#3366cc` and
  its `get_selection` bounds kept, then `deselect`. `select_color_range`
  range `sampled` with `samples16` the rect's codes from `sample_pixel`
  (5, 3)'s `center_rgba16`, tolerance 5, softness 0 answers `ok` with
  `get_selection`'s bounds equal to the rect's and the mode still lab; range
  `highlights` answers `ok` (since 6b-i, updated before its run: it answered
  `COLOR_RANGE_REFUSAL`); and, after `deselect`, `select_magic_wand` (5, 3)
  tolerance 10 answers ok with `get_selection`'s bounds equal to the rect's,
  then `select_similar` tolerance 10 answers ok with the same bounds (updated
  before L99's run for the wand and Similar: they answered a sentence
  awaiting their take; updated before 4c's run, which closes phase 4:
  they answered "<tool> " + phase 4's sentence).

Phase 5a-ii, L69 (numbered after 4b-ii's L68), fixed before the run. Every earlier document is closed
before L69 (setup only, `close_all`).

- **L69 Lab → Grayscale.** A 16-bit Lab document of `COLOUR66` `convert_mode
  gray gray_space gray_gamma_22` answers `color_mode` gray and `depth` 16 in
  one undo step: undo answers lab, redo gray. Its `sample_pixel` code at each
  of `POINTS67` is R = G = B and within `GRAY_BOUND69` (257) of the drive's
  own oracle from the Lab document's codes read before the conversion (L* =
  100 c / 65535, Y = L*⁻¹, v = Y^(1/2.2), 65535 v). An 8-bit one answers
  `depth` 8. A 16-bit Lab document with `add_adjustment_layer` levels named
  `Levels69` refuses gray with exactly `ADJUSTMENT_GRAY69`, still lab;
  `flatten` true converts it to a one-layer gray document. `save_action` of
  [`convert_mode gray gray_space gray_gamma_22`], replayed by `run_action` on a
  fresh 16-bit Lab document of `COLOUR66`, gives exactly the direct call's
  codes at `POINTS67`.

Phase 4c, L70–L71 (numbered after 5a-ii's L69), fixed before the run. `main` sets `DR_PASTEBOARD_NAME` to
`com.kgalligan.doctored.verify-lab-mode.<pid>` before the app starts, so no
case reads or writes the user's pasteboard. Every earlier document is closed
first (setup only).

- **L70 the clipboard.** A 16-bit Lab `new_document` 8 × 6 of `#3366cc` with
  `select_rect` 2,1,4×3 filled `#c8285a` is copied whole (`select_all`, `copy`
  layer 0); `get_clipboard` answers `mode` "lab". Its paste (`in_place`) into a
  second 16-bit Lab document of `#808080` is one undo step, the mode stays lab,
  `color_conversion` is null, and `center_rgba16` at `POINTS70` equals the
  source's exactly. The same clip pasted into a 16-bit sRGB document of
  `#ffffff` answers `color_conversion` Lab → RGB, and at `POINTS70` each of
  R, G and B is within `RGB_BOUND70` (257) of the source's own `convert_mode
  rgb rgb_space srgb` (undone after). An sRGB `#c8285a` 8 × 6 16-bit
  document copied whole into a 16-bit Lab document of `#808080`: `profile
  preserve` answers exactly `CROSS_LAB`; Convert answers `color_conversion`
  RGB → Lab with `black_point_compensation` false, and `sample_pixel` (3, 3)'s
  `lab` is within `LAB_BOUND[16]` of `srgb_to_lab((0xc8, 0x28, 0x5a))`.
- **L71 Duplicate and copying layers.** `duplicate_to_new_document` layers [0]
  of L70's Lab source answers a document whose mode is lab, `depth` 16 and
  `center_rgba16` at `POINTS70` equal to the source's. `copy_layers_to_document`
  of that layer into a 16-bit sRGB document answers `color_conversion` from
  Lab to RGB with the destination still rgb, and of an sRGB layer into a
  16-bit Lab document `color_conversion` from RGB to Lab with the destination
  still lab. `copy_layers_to_document` of a 16-bit Grayscale `#808080` 4 × 3
  document's layer 0 (L6's construction) into a 16-bit Lab 4 × 3 document
  lands `center_rgba16` at every pixel equal, exactly, to that Gray document's
  own `convert_mode lab` (depth 16). On a 12 × 8 16-bit Lab document, a
  `new_layer` filled `#3366cc` inside `select_rect` 3,2,5×4 and copied as
  layers (`copy mode layers`, layers [1]) makes `new_from_clipboard` answer a
  lab document 5 × 4 (the layer's content box).

Phase 5b, L72–L73 (numbered after lane C's 4c, L70–L71), fixed before the run.
take1 (`lab_cmyk_take.py`) read Edit ▸ Assign Profile… disabled and Convert to
Profile…, Proof Setup, Proof Colors and Gamut Warning enabled in Lab; its Lab →
CMYK MISS ships nothing, so no case checks Photoshop's inks. Every
earlier document is closed before L72 and again before L73 (setup only,
`close_all`).

- **L72 proof.** On a 16-bit 12 × 8 Lab `new_document` of `COLOUR66` with
  `select_rect` 2,2,4×3 filled `#0000ff` (then `deselect`):
  `set_proof_setup profile working_cmyk proof_colors true` answers `ok`; a 5 s
  poll of `get_proof_setup` reads `proof_colors` true and `failure` null at
  every reading; `set_proof_setup preserve_numbers true` refuses exactly
  `PRESERVE_REFUSAL72`. `render_proof` (source display) and `render_proof
  exact true`, each with `gamut_warning` true, answer `space` display-p3, and
  each is within `PROOF_BOUND8` (1 code) per channel at every pixel of the same
  call after `convert_mode rgb rgb_space displayP3` (then undone, the mode lab
  again), with an equal `out_of_gamut` > 0. An 8-bit Lab `new_document`'s
  `render_proof` answers `source` display. `get_precision`'s `supported_tools`
  holds `PROOF_TOOLS72`.
- **L73 profile commands.** On a 16-bit Lab document of `COLOUR66`,
  `assign_profile profile srgb` refuses exactly `UNAVAILABLE_REFUSAL` of
  `assign_profile`. `convert_profile profile working_cmyk` answers
  `color_mode` cmyk and `depth` 16 in one undo step (undo answers lab, redo
  cmyk), its `ink16` at `POINTS67` exactly a fresh document's `convert_mode
  cmyk`'s; `convert_profile profile srgb` answers rgb with `sample_pixel`
  codes at `POINTS67` exactly `convert_mode rgb rgb_space srgb`'s. `save_action`
  of [`convert_profile profile working_cmyk`], replayed by `run_action` on a
  fresh 16-bit Lab document of `COLOUR66`, gives exactly the direct call's
  `ink16` at `POINTS67`.
- **L74 type and shape (6a-i).** On a 16-bit 40 × 16 Lab document
  `add_text_layer` #ff0000 and `edit_text_layer` #00ff00 are one undo step
  each, and the row's text colour stays the authored hex (#ff0000, then
  #00ff00). `add_shape_layer` rect #00ff00 (3, 3, 10 × 6) on a 16 × 12 Lab
  document at 16 and at 8 bits is one undo step whose interior (8, 6)
  `sample_color` `lab` is within `LAB_BOUND[depth]` of `srgb_to_lab` of green;
  at 8 bits every `PROBE74` code of the layer (`source layer`) is a multiple
  of 257. On the 16-bit one `transform_layer` scale 2 keeps `shape`;
  `rasterize_artwork` drops it and undo restores it. `add_text_layer` with
  `ink` answers `INK_REFUSAL` exactly.
- **L75 fill layers (6a-i).** On a 16-bit 12 × 8 Lab document
  `add_fill_layer` solid #4080c0 and `edit_fill_layer` #c04080 are one undo
  step each, (5, 4) `within` each colour at `LAB_BOUND[16]`. A gradient
  #ff0000 @0.25 → #0000ff @0.75 (angle 0, linear) on a 64 × 4 Lab document
  reads (2, 2) `within` red and (61, 2) `within` blue at `LAB_BOUND[16]`, and
  its row's stops report #ff0000 and #0000ff. A custom 2 × 1 red/blue pattern
  on a 6 × 2 Lab document: every pixel of row 0 is `within` red or blue at
  `LAB_BOUND[8]` (the tile converts texel by texel to 8-bit codes), and both
  appear.
- **L76 paths and vector masks (6a-i).** Gray's G61 on a 16-bit 24 × 16 Lab
  document with a second layer: `set_path` "Box" lists; `path_to_selection`
  selects; `fill_path` #ff0000 on layer 1 is one undo step, (12, 8) `within`
  red at `LAB_BOUND[16]`; `stroke_path` #0000ff size 2 runs; `set_vector_mask`
  from "Box" then `get_vector_mask` answers `vector_mask` true. `save_copy
  rz` of it reopens as lab with "Box" listed, the vector mask on layer 1 and
  `PROBE76` codes equal; `save_copy rz` of L74's 16-bit shape document
  reopens as lab with the same `shape` description and `PROBE74` codes equal.
- **L77 conversions (6a-i).** A 16-bit 16 × 12 Lab document with a #00ff00
  shape and a #ff0000 text layer: `convert_mode` rgb (srgb), cmyk and gray
  (Gray Gamma 2.2) each keep both rows described (`shape`, `text`), and each
  is undone back to lab with both rows still described. `convert_precision
  lab8` keeps `shape`, and the layer's interior codes at (8, 6) are multiples
  of 257. A 16-bit 16 × 12 sRGB document with a #00ff00 shape: `convert_mode
  lab` is one undo step that keeps `shape`, the interior (8, 6) `within`
  green at `LAB_BOUND[16]`.

Phase 6b-i, L78–L82, fixed before the run. Every earlier document is closed
before L78 (setup only, `close_all`). `pair78` is a 16-bit 6 × 4 Lab document of
`COLOUR66` with a layer `Top` filled `TOP78`; codes are `sample_pixel`'s
`mean_rgba_codes` at `POINTS67`, a layer's with `source` layer.

- **L78 Apply Image and Calculations.** On `pair78`, `apply_image` layer 0
  target `lightness` source merged `source_plane` `a` `blend_mode` Multiply is
  one undo step; layer 0's L* is within `CHANNEL_BOUND78` (1 code) of L·a/65535
  (the composite's a), its a and b exactly kept. Then `apply_image` layer 0
  target `layer` `source_plane` `lab` Screen puts each channel within 1 code of
  b + s − b·s per channel. `calculations` `source1_plane` `lightness`
  `source2_plane` `b` Multiply adds one channel. `blend_mode` Color answers
  exactly `CHANNEL_MATH_LIMITATION`; `source_plane` `red` exactly
  `SOURCE_PLANE_REFUSAL78`.
- **L79 Stack Modes.** On a 16-bit 6 × 4 Lab document of `COLOUR66` with layers
  `B` filled `TOP78` and `C` filled `THIRD79`, `stack_mode` layers [0, 1, 2]
  `median` is one undo step and its new layer 3 is the per-channel median of
  the three layers' codes exactly. On a 16-bit 24 × 16 Lab document with two
  brush strokes and two duplicates, `reduce_noise_from_stack` [0, 1, 2]
  `median` is one undo step and the document stays lab.
- **L80 Load Files into Stack and Merge to HDR.** A 16-bit 4 × 2 Lab
  `new_document` of `COLOUR66` saved as Lab TIFF, then an 8-bit 4 × 2 sRGB PNG
  of `PATCHES`: `load_files_into_stack` [tiff, png] is lab, and layer 1's codes
  / 257 are within `STACK_BOUND80` (1) of the same PNG opened and
  `convert_mode lab`. `merge_layers_to_hdr` layers [0, 1] `evs` [0, 1] `align`
  false answers `depth` rgba32_float and a `profile` naming ProPhoto, its
  document rgb, and the stack stays lab.
- **L81 Color Range.** On a 16-bit 12 × 8 Lab `new_document` of `BACK81` with
  `select_rect` 1,1,3×3 filled `#ffffff` and 7,3,4×3 filled `RED81`:
  `select_color_range` `highlights` and `reds` each answer `ok` with
  `get_selection`'s bounds the white and the red rect; `skin_tones` and
  `out_of_gamut` answer `ok`; the mode stays lab.
- **L82 a plane's selected pixels.** On a 16-bit 12 × 8 Lab `new_document` of
  `BACK82` with `select_rect` 3,2,4×3 filled `PATCH82` and kept selected,
  `move_selected_pixels` layer 0 dx 2 dy 1 target `lightness` is one undo step:
  at the landing (8, 5) L* is the patch's, a and b the background's, and a and
  b are unchanged at every point of `GRID82`. After undo, target `a` likewise
  moves a with L* and b unchanged.

Phase 6a-ii, L83–L85, fixed before the run. Every earlier document is closed
first (setup only, `close_all`).

- **L83 comps, links, snapshots.** On a 16-bit 12 × 8 Lab document of
  `COLOUR66` with a `new_layer` "Top" filled `#c8285a` inside `select_rect`
  2,2,7×4: `save_layer_comp` "A" is one undo step; after
  `set_layer_properties` index 1 visible false, opacity 0.5,
  `apply_layer_comp` "A" is one undo step that restores `get_document`'s rows
  (name, opacity, visible, blend mode, offset) and `render`'s pixels exactly,
  the mode still lab. `link_layers` [0, 1] is one undo step and both rows
  report the same `linked`; `unlink_layers` [0, 1] is one undo step and no row
  reports `linked`. `create_snapshot` "S", a `brush_stroke` that changes
  `render`, then `restore_snapshot` is one undo step whose `render` equals the
  pre-stroke pixels exactly, the mode still lab. A 16-bit sRGB document with
  `save_layer_comp` "RGB" keeps it in `list_layer_comps` after `convert_mode
  lab`.
- **L84 Clear and the planes.** On a 16-bit 12 × 8 Lab document of
  `COLOUR66`: `clear_selection` layer 0 over `select_rect` 3,2,5×4 is one undo
  step, `center_rgba16[3]` 0 at (4, 3) and the codes at (0, 0) unchanged.
  On a fresh one, `move_selected_pixels` target `a` dx 2 `background`
  `#ff0000` over `select_rect` 2,2,4×3 is one undo step; the vacated pixel
  (2, 3)'s a* code (`center_rgba16[1]`) is within 257 of
  round(`srgb_to_lab`(#ff0000).a + 128)·257, its L* and b* codes as before.
  `transform_selected_pixels` target `lightness` scale 1.25 over
  `select_rect` 2,2,6×4 on a fresh one, with `select_rect` 4,3,2×2 filled
  `#ffffff` first, is one undo step, the mode still lab. (The first run
  missed it on a flat document, whose scaled plane equals itself: the white
  square was added, setup only; no gate changed.)
- **L85 Convert to Profile → Gray.** On a 16-bit Lab document of `COLOUR66`,
  `convert_profile profile dot_gain_20` answers `color_mode` gray and `depth`
  16 in one undo step (undo answers lab, redo gray), and its `sample_pixel`
  codes at `POINTS67` are exactly a fresh document's `convert_mode gray
  gray_space dot_gain_20`'s.

Phase 6b-ii, L86–L89, fixed before the run. Every earlier document is closed
first (setup only, `close_all`). A tool CLEARS THE GATE when it answers ok, or
an error of its own whose text contains none of `GATE_PARTS` ('Lab Color',
'until #1', 'matrix/TRC'). The pair is `texture86` (a textured 8-bit
`TEX_W` × `TEX_H` PNG written here) opened, `convert_precision` rgba16,
`convert_mode lab`, its layer duplicated and layer 1 `move_layers` by (3, 2).

- **L86 Auto-Align.** `auto_align_layers` [0, 1] on the pair answers ok in one
  undo step and the mode stays lab; `vignette` true and `distortion` true, each
  on a fresh pair, clear the gate.
- **L87 Auto-Blend and Merge Panorama.** Since the close (`lab_close_take.py`
  take1) `auto_blend_layers` `stack_images` on the aligned pair answers
  exactly `UNAVAILABLE_REFUSAL` of `auto_blend_layers` and changes nothing (the
  layer count and the mode stay; updated before the close's run: it answered
  ok, `undo` restored the pair, and `panorama` answered ok).
  `merge_panorama` of two overlapping Lab TIFFs (`save_copy` of the texture's
  left and right crops, each made Lab as the pair) clears the gate; on ok its
  document is lab. (Setup widened for 6b-iii: the crops were columns 0–100 and
  60–`TEX_W`, where Merge Panorama answered its own Auto-Align refusal and the lab
  check never ran; 0–110 and 50–`TEX_W` still did; they are 0–120 and
  40–`TEX_W`.)
- **L88 the selections.** On a 16-bit 24 × 16 Lab document of `BACK88` with
  `select_rect` 6,4,8×6 filled `PATCH88`: `set_quick_mask` on,
  `fill_selection target quick_mask`, then off, each answer ok; `quick_select`
  seeded at (10, 7) gives `get_selection` bounds equal to the patch rect's;
  `refine_selection` (decontaminate, feather 2, output new_layer) answers ok
  with one more layer and the mode stays lab; `select_subject`, `select_sky`,
  `select_person_parts` (hair) and `object_finder_select` (a box) clear the
  gate (the models from `DR_MODELS_DIR`).
- **L89 nit (b).** On a Lab document `convert_profile profile bogus` answers
  text containing `working_gray` and `dot_gain_20`.

Phase 6b-iii, L90–L92, fixed before the run: the model or provider reads the
Display P3 view and its result lands as L*a*b* codes by the Mode rule. L90 and
L91 assert only when the model fixtures are present (`model_ready`; the drive
exits 2 otherwise).

- **L90 Super Zoom and JPEG Artifacts Removal.** On a 16-bit 16 × 12 Lab
  document filled `#808080`, `super_zoom` zoom 2 answers ok; its document is
  lab at depth 16, 32 × 24, and its centre's `lab` a* and b* are within
  `AB_BOUND90` (0.5) of 0 and its L* within `L_BOUND90` (1) of the source's.
  `jpeg_artifacts_removal` on the source is one undo step, the mode stays lab
  and the centre's a* and b* are within `AB_BOUND90` of 0. (Setup fixed before
  the drive's run, after a probe: Super Zoom runs with `enhance_details` false,
  the classical resample, which still needs the Real-ESRGAN download and runs on
  the Display P3 view. With enhance on, Real-ESRGAN lifts a flat #808080 field
  by 1.3 L* and tints it green on an RGB document too (L*a*b* 54.89, −0.39,
  0.29 there; 54.88, −0.49, 0.30 on Lab), which is the model's behaviour and
  not the Lab path's.)
- **L91 Colorize and Sky Replacement.** On the texture document made Lab
  (L87's `lab_open`), `neural_colorize` output `new_layer` answers ok with one
  more layer and the mode lab; output `new_document` answers a lab document.
  `sky_replacement` answers exactly `RGB_ONLY_REFUSAL.format('Sky
  Replacement')`.
- **L92 the generative tools.** On a 16-bit 24 × 16 Lab document with
  `select_rect` 4,4,8×6, `generative_fill` (prompt "a stone"), then (deselected)
  `generative_expand` and `generative_variations` each clear the gate (the
  dead-port providers and the private `DR_CLOUD_DIR` come from `App`).

The close, L93–L98, fixed before the run: every tool the host gate refused
naming a phase (`CLOSE_LIFTED` and `CLOSE_REFUSED`, 58) is lifted or refused
for good by name, and Auto-Blend Layers refuses for good in Lab
(`lab_close_take.py` take1). `LAB_TOOLS` (L18) gains
`CLOSE_LIFTED` and loses `auto_blend_layers` (updated before the close's run).
Every earlier document is closed first (setup only, `close_all`).

- **L93 the 58.** On a 16-bit 12 × 8 Lab document, each `CLOSE_LIFTED` tool
  called with `document_id` alone clears the gate (`GATE_PARTS`, L86's rule);
  each `CLOSE_REFUSED` tool answers exactly its sentence there; no reply of the
  58 contains "#1 phase"; `get_precision` has no `later_phases` and no
  `default_phase` key.
- **L94 structure, metadata, views.** On a 16-bit two-layer Lab document:
  `arrange_layer` (layer 1 to back), `find_layers` (raster),
  `get_mask_properties` of a reveal-all mask, `auto_select_layer` (3, 3),
  `merge_group` of a group of the top layer, `set_ruler_origin`,
  `set_history_budget`, `list_lens_profiles` and `detect_horizon` answer ok;
  `set_resolution` 150 is ONE undo step and `get_metadata` reads 150 × 150;
  `set_file_info` copyright then `get_file_info` reads it back;
  `open_second_view`, `set_view` view 2 zoom 1, `get_view`,
  `close_second_view`, `save_workspace` and `restore_workspace` answer ok;
  the mode stays lab throughout.
- **L95 Distort and the planes.** `distort_layer` on a textured Lab document
  is ONE undo step and the mode stays lab; `modify_scalar` grow radius 2 on a
  from-selection mask changes the mask's `render` and target red answers
  exactly `PLANE_TARGET_REFUSAL`; `invert_channel` of an added alpha channel
  answers ok in one undo step; `add_luminosity_masks` on a 64 × 1 Lab document
  made from a 16-bit grey ramp adds exactly the nine RGB names
  (`LUMINOSITY_NAMES`), and along the ramp `render` of "Lights 1" is
  non-decreasing and of "Darks 1" non-increasing.
- **L96 Place.** `place_image` of an 8-bit colour PNG (`PATCHES`, 8 × 1) into a
  16-bit 8 × 1 Lab document is ONE undo step and `render` is within
  `PLACE_BOUND8` (1) code of that PNG opened, made 16-bit and converted to lab;
  an empty `x.dng` answers text containing `RAW_PLACE_REFUSAL`;
  `add_live_photo_layer` of an authored movie answers ok and the mode stays
  lab, or is NOT RUN (said so) without ffmpeg.
- **L97 outputs.** On the two-layer Lab document `export_layers` (items all,
  png 8 srgb) writes two PNGs; `export_pdf` writes a file starting `%PDF`;
  `contact_sheet` output document opens one more document; `separate_color`
  rgb [128, 64, 200] with the Lab document equals the same call with no
  document open (taken first).
- **L98 the take's refusals.** `auto_blend_layers` answers exactly
  `UNAVAILABLE_REFUSAL` of it, and `showAutoBlendLayers:` is absent from
  `get_precision`'s `supported_menu_items` (Sky Replacement, the take's other
  refuse_for_good row, keeps L91's sentence, unchanged).

The Magic Wand, Similar and Grow, L99–L101, fixed before the run:
`lab_select_682_take.py`'s take fitted the wand at both depths and
`lab_select_5_take.py`'s takes Select ▸ Similar and Grow at both depths
(updated before that batch's run: they had run at 16 bits only, over the full
box). Every earlier document is closed first (setup only, `close_all`). The SCENE is
an 8 × 4 Lab `new_document` of `#808080` at the depth with `select_rect` +
`fill_selection` (then `deselect`) of `SCENE99` (x, y, w, h, colour); its
codes are `sample_pixel`'s `mean_rgba_codes` at every pixel. The ORACLE is
this file's own (`wand_key99`): each code a byte (8 bits: round(v / 257); 16
bits: round-half-up(v·32768/65535) for L, (v·32640/65535) for a and b, then
⌊(i·255 + 16384)/32768⌋), the key L, a, b and ⌊(a − L + 255)/2⌋, ⌊(b − a +
255)/2⌋, ⌊(L − b + 255)/2⌋; a pixel is admitted when L, a and b lie within ±
t of the references' range and the three halves within ± ⌊t/2⌋ (alpha is
opaque throughout), the range Similar's and Grow's each key's values over the
N references, sorted, with ⌊N/10⌋ dropped at each end; the wand floods
four-connected from its seed, Grow from
every reference, Similar is global. A selection is read by `get_selection`
`points` (every pixel), selected = `coverage16` 65535, unselected = 0.

- **L99 the wand.** At 8 and 16 bits, after `deselect`, `select_magic_wand`
  (0, 0) at each t of `TOLERANCES99`, contiguous true and false, is ONE undo
  step, the mode stays lab, and the selection equals the oracle exactly; at
  each depth some case selects some pixels and leaves others.
- **L100 Similar and Grow.** At 8 and 16 bits, after `select_rect` (0, 0, 5,
  4) (20 references, so each key drops 2 at each end), `select_similar` at
  each t of `TOLERANCES99` with contiguous false and true is ONE undo step
  (or, when the oracle admits nothing outside the rectangle, answers changed
  false and adds none) and the selection equals the oracle (the references the
  rectangle's pixels) added to the rectangle, which stays selected; at each depth some case's
  oracle differs from the full box's (updated before
  `lab_select_5_take.py`'s batch's run: a (0, 0, 2, 2) rectangle at 16 bits,
  and at 8 bits a refusal).
- **L101 the Paint Bucket.** At 8 and 16 bits `fill` (0, 0) `#0000ff`
  tolerance 12, `fill_mode` color_match, contiguous true and false, is ONE undo
  step and changes exactly the oracle's wand region at that t, every changed
  pixel one code (updated after the first run: without `fill_mode` a typed
  contiguous false is the precision fill's selection-wide form, which filled
  every pixel; the region and its bound are unchanged).
"""

import argparse
import base64
import json
import math
import os
from pathlib import Path
import struct
import sys
import time
import zlib

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

import model_store_paths
from phase5_support import App

# ---- the expectations, fixed here BEFORE anything is measured -------------
RENDER_BOUND8 = 2
SAMPLE_BOUND8 = 1
NEUTRAL_BOUND8 = 1
WHITE_FLOOR = 254
NEW_32_REFUSAL = 'A Lab Color document is 8 or 16 bits per channel.'
# L99–L101 (module doc): the scene's rectangles over #808080 and the tolerances.
SCENE99 = [(2, 0, 2, 4, '#848484'), (4, 0, 2, 2, '#8c8080'), (4, 2, 2, 2, '#808c80'),
           (6, 0, 2, 4, '#a0a0a0')]
TOLERANCES99 = (2, 6, 20)
W99, H99 = 8, 4
CROSS_LAB = ("Don't Convert keeps numbers within one colour mode; pixels pasted between Lab Color "
             'and another mode are always converted')
POINTS70 = [(0, 0), (3, 2), (7, 5)]
RGB_BOUND70 = 257
PASTEBOARD = f'com.kgalligan.doctored.verify-lab-mode.{os.getpid()}'
SAVE_REFUSAL = ('A Lab Color document saves as .doctored or Lab TIFF; Export as PSD (export_psd) '
                'writes a Lab PSD, PSB or layered TIFF. Export As writes an RGB copy, or convert to '
                'RGB Color for other formats.')
TAKE2 = Path.home() / 'devel/doctored-fixtures/photoshop/lab-files-547/take2/jobs'
LAB_TIFF_W, LAB_TIFF_H = 8, 4
LAB_POINTS = [(0, 0), (3, 1), (5, 2), (7, 3)]
LAB_LIMITATION_HEAD = 'Lab Color layer blending supports Normal, '
LAB_LIMITATION_TAIL = ('; every other mode is unavailable in a Lab Color document. Pass Through '
                       'requires a group.')
TIFF_DEPTH_REFUSAL_8 = "A Lab TIFF keeps the document's own depth (8 bits per channel)."
FILL_BOUND8 = 2
EXPORT_BOUND8 = 2
GRAY_AB_BOUND = {8: 1, 16: 128}
FLATTEN_TEXT = 'is a layer in the Darken blend mode, which a Lab Color document cannot hold'
FLOAT_TEXT = 'convert to 16 Bits/Channel first'
PATCHES = [(200, 40, 90), (30, 180, 220), (128, 128, 128), (250, 250, 10),
           (64, 0, 255), (0, 0, 0), (255, 255, 255), (90, 160, 40)]
# Phase 1c.
LAB_TOOLS = [
    'convert_mode', 'get_color_profile', 'get_document', 'render',
    'set_active_layer', 'flatten_image', 'select_rect', 'select_ellipse', 'select_polygon',
    'select_freehand', 'select_all', 'deselect', 'reselect', 'get_selection', 'undo', 'redo',
    'save_copy', 'set_layer_lock', 'set_selected_layers', 'duplicate_layer', 'delete_layer',
    'list_guides', 'add_guide', 'remove_guide', 'clear_guides', 'list_history', 'restore_history',
    'set_layer_label', 'reorder_layer', 'set_layer_properties', 'reorder_layers',
    'close_document', 'get_recovery_status', 'write_recovery',
    'new_layer', 'group_layers', 'ungroup_layers', 'add_layer_mask', 'remove_layer_mask',
    'set_layer_mask_enabled', 'set_mask_properties', 'set_layer_clipped', 'merge_down',
    'merge_visible', 'stamp_visible', 'set_layer_style', 'fill_selection', 'export_image',
    'export_preview', 'quick_export', 'list_channels', 'add_channel', 'duplicate_channel',
    'delete_channel', 'rename_channel', 'set_channel_options',
    'sample_color', 'sample_pixel', 'histogram', 'get_precision',
    'brush_stroke', 'eraser_stroke', 'fill', 'gradient', 'apply_filter', 'add_adjustment_layer',
    'edit_adjustment_layer', 'run_action',
    'fade', 'auto_tone', 'auto_contrast', 'equalize',
    'get_color_samplers', 'set_color_samplers',
    # Phase 2e.
    'dodge_burn', 'sponge_stroke', 'blur_stroke', 'sharpen_stroke', 'smudge_stroke',
    'history_brush_stroke', 'clone_stamp', 'heal_stroke', 'spot_heal_stroke', 'patch_region',
    'content_aware_fill', 'preview_content_aware_fill', 'discard_content_aware_fill_preview',
    'content_aware_move', 'set_global_light', 'apply_style_preset', 'save_style_preset',
    # Phase 2f, second part.
    'stroke_selection',
    # Phase 2f, third part.
    'remove_stroke', 'art_history_brush_stroke', 'red_eye', 'red_eye_auto',
    # Phase 3b.
    'export_psd', 'inspect_psd_export',
    # Phase 4a.
    'image_size', 'canvas_size', 'crop', 'perspective_crop', 'fit_crop', 'trim',
    'reveal_canvas', 'rotate', 'flip', 'straighten_layers', 'transform_layer',
    'transform_layers', 'transform_again', 'transform_mask', 'transform_selection',
    # Phase 4a-ii.
    'get_warp', 'warp_layer', 'puppet_warp', 'perspective_warp', 'liquify', 'align_layers',
    'distribute_layers', 'move_layers',
    # Phase 4b-i.
    'move_selected_pixels', 'transform_selected_pixels', 'layer_via_copy', 'layer_via_cut',
    'stamp_selected', 'save_selection', 'load_selection', 'modify_selection',
    # Phase 5a.
    'convert_precision',
    # Phase 4b-ii.
    'select_color_range',
    # Phase 4c.
    'copy', 'cut', 'copy_merged', 'paste', 'duplicate_to_new_document', 'copy_layers_to_document',
    # Phase 5b.
    'convert_profile', 'get_proof_setup', 'set_proof_setup', 'render_proof', 'save_proof_preset',
    # Phase 6a-i: Gray 6a-i's twenty.
    'add_text_layer', 'edit_text_layer', 'add_shape_layer', 'edit_shape_layer', 'add_fill_layer',
    'edit_fill_layer', 'rasterize_artwork', 'list_paths', 'set_path', 'rename_path',
    'duplicate_path', 'delete_path', 'selection_to_path', 'path_to_selection', 'fill_path',
    'stroke_path', 'get_vector_mask', 'set_vector_mask', 'set_vector_mask_properties',
    'remove_vector_mask', 'set_clipping_path',
    # Paths panel ▸ Combine with Path….
    'combine_paths',
    # Replace Missing Fonts (designs/replace-missing-fonts.md).
    'list_missing_fonts', 'replace_missing_fonts',
    # Phase 6b-i.
    'apply_image', 'calculations', 'stack_mode', 'reduce_noise_from_stack', 'merge_layers_to_hdr',
    # Phase 6a-ii: Gray 6a-ii's sixteen.
    'link_layers', 'unlink_layers', 'list_layer_comps', 'save_layer_comp', 'apply_layer_comp',
    'update_layer_comp', 'delete_layer_comp', 'reorder_layer_comp', 'list_snapshots',
    'create_snapshot', 'rename_snapshot', 'delete_snapshot', 'restore_snapshot', 'save_snapshot',
    'load_snapshot', 'clear_selection',
    # Phase 6b-ii (Auto-Blend Layers refuses for good since the close).
    'auto_align_layers',
    'quick_select', 'select_subject', 'select_subjects', 'inspect_subjects', 'select_people',
    'select_face_parts', 'inspect_faces', 'object_select', 'refine_selection',
    'set_quick_mask', 'quick_mask_stroke',
    'select_sky', 'select_person_parts', 'object_finder_select',
    # Phase 6b-iii.
    'super_zoom', 'jpeg_artifacts_removal', 'neural_colorize', 'generative_fill',
    'generative_expand', 'generative_variations', 'select_generative_variation',
    'generate_variations',
    # The Magic Wand and Select Similar (L99).
    'select_magic_wand', 'select_similar',
    # #1 spot channels (18b81bc6).
    'add_spot_channel', 'set_spot_channel', 'merge_spot_channel', 'set_channel_kind',
    # Edit ▸ Define Pattern… (d81a3c71).
    'define_pattern',
]
# The close (module doc, L93): the 34 tools it lifts and the 24 it refuses for good by name.
CLOSE_LIFTED = [
    'arrange_layer', 'auto_select_layer', 'find_layers', 'get_mask_properties', 'merge_group',
    'list_views', 'open_second_view', 'close_second_view', 'get_view', 'set_view',
    'save_workspace', 'restore_workspace', 'get_file_info', 'set_file_info', 'get_metadata',
    'set_resolution', 'set_ruler_origin', 'set_history_budget', 'recover_document',
    'discard_recovery', 'detect_horizon', 'list_lens_profiles', 'distort_layer', 'modify_scalar',
    'invert_channel', 'add_luminosity_masks', 'select_magnetic', 'separate_color',
    'export_layers', 'export_pdf', 'contact_sheet', 'contact_sheet_from_sources', 'place_image',
    'add_live_photo_layer',
]
LAB_TOOLS += CLOSE_LIFTED
RAW_TOOLS = ['apply_raw_preset', 'apply_raw_settings', 'copy_raw_local_settings',
             'edit_raw_development', 'get_raw_local_mask', 'get_raw_locals', 'get_raw_source',
             'inspect_raw_development', 'migrate_raw_decoder', 'preview_raw_decoder_migration',
             'redevelop_raw', 'save_raw_preset', 'sync_raw_settings', 'get_image_source',
             'rasterize_image_source']
RAW_REFUSAL = ('{} is unavailable in a Lab Color document: Doctored keeps camera RAW layers on '
               'RGB documents only.')
RAW_PLACE_REFUSAL = RAW_REFUSAL.format('Placing a camera RAW file')
EVALUATION_REFUSAL = ('{} is unavailable in a Lab Color document: Doctored offers linear-light '
                      'evaluation on RGB documents only.')
TONE_MAP_REFUSAL = ("{} is unavailable in a Lab Color document: the tone map is a 32-bit "
                    "document's, and Lab Color is 8 or 16 bits per channel.")
CLOSE_REFUSED = dict(
    [(tool, RAW_REFUSAL.format(tool)) for tool in RAW_TOOLS]
    + [('add_text', 'add_text is unavailable in a Lab Color document: it draws 8-bit RGB '
                    'pixels; add_text_layer adds editable type.'),
       ('put_pixels', 'put_pixels is unavailable in a Lab Color document: it writes 8-bit RGBA '
                      'bytes, which Doctored offers on 8-bit RGB documents only.')]
    + [(tool, EVALUATION_REFUSAL.format(tool)) for tool in ('get_evaluation', 'set_evaluation')]
    + [(tool, TONE_MAP_REFUSAL.format(tool)) for tool in ('get_tone_map', 'set_tone_map')]
    + [('depth_blur', 'Depth Blur is unavailable in a Lab Color document: Doctored offers it '
                      'on RGB documents only.'),
       ('auto_color', 'Auto Color is unavailable in a Lab Color document.'),
       ('hdr_toning', 'HDR Toning is unavailable in a Lab Color document.')])
LUMINOSITY_NAMES = [f'{band} {level}' for band in ('Lights', 'Darks', 'Midtones')
                    for level in (1, 2, 3)]
PLACE_BOUND8 = 1
LAB_BLEND_NAMES = ['Normal', 'Dissolve', 'Multiply', 'Linear Burn', 'Darker Color', 'Screen',
                   'Linear Dodge (Add)', 'Lighter Color', 'Overlay', 'Soft Light', 'Hard Light',
                   'Vivid Light', 'Linear Light', 'Pin Light', 'Hard Mix', 'Hue', 'Saturation',
                   'Color', 'Luminosity']
# Phase 2's take: the 27 names of `photoshop-probe/gray_mode_take.CONSTANTS`, Photoshop's menu order.
ALL_BLEND_NAMES = ['Normal', 'Dissolve', 'Darken', 'Multiply', 'Color Burn', 'Linear Burn',
                   'Darker Color', 'Lighten', 'Screen', 'Color Dodge', 'Linear Dodge (Add)',
                   'Lighter Color', 'Overlay', 'Soft Light', 'Hard Light', 'Vivid Light',
                   'Linear Light', 'Pin Light', 'Hard Mix', 'Difference', 'Exclusion',
                   'Subtract', 'Divide', 'Hue', 'Saturation', 'Color', 'Luminosity']
LAB_BOUND = {16: (0.1, 0.1), 8: (0.6, 1.0)}
LAB_EXACT = 0.001
WHITE_LAB_BOUND = 0.1
CHANNEL_BOUND8 = 1
RENDER_REFUSAL = ('A Lab Color document renders its composite with no channel, channel lightness, '
                  'a or b with no layer, channel mask with a layer, or an alpha channel by name '
                  'with no layer.')
# Phase 2a and 2b.
UNAVAILABLE_REFUSAL = '{} is unavailable in a Lab Color document.'
RGB_ONLY_REFUSAL = ('{} is unavailable in a Lab Color document: Doctored offers it on RGB '
                    'documents only.')
PLANE_TARGET_REFUSAL = ("A Lab Color document's targets are layer, mask, lightness, a, b, alpha "
                        'and channel:<name>.')
# Phase 2c.
DEVELOP_FILTER_REFUSAL = ('{} is unavailable in a Lab Color document: it is a Develop Filter '
                      'control, and the Develop Filter is unavailable in Lab.')
PIXELS_ONLY_REFUSAL = ('A {0} adjustment layer is unavailable in a Lab Color document: {0} '
                       'applies to pixels only.')
FADE_BOUND = 1
HALF_BOUND = 0.5
NEUTRAL16 = 0x8080
# Phase 4b-i.
LOAD_PLANE_REFUSAL = ("A Lab Color document's planes are lightness, a, b, luma (its Lightness) "
                      'and alpha.')
# Phase 5a.
COLOUR66 = '#6a8fb0'
INK_BOUND66 = 2 * 257
ADJUSTMENT_CMYK66 = ('“Levels66” is an adjustment layer, which a CMYK document cannot hold yet; '
                     'flatten the image to convert it')
POINTS67 = [(0, 0), (3, 1), (7, 3)]
# Phase 5a-ii.
GRAY_BOUND69 = 257
ADJUSTMENT_GRAY69 = ('“Levels69” is a Lab Color adjustment layer, which a Grayscale document '
                     'cannot hold; flatten the image to convert it')
LAB32_REFUSAL = ('A Lab Color document converts between mode lab8 and lab16 (Image ▸ Mode ▸ 8/16 '
                 'Bits/Channel); 32 Bits/Channel is unavailable in Lab Color.')
# Phase 5b.
PROOF_BOUND8 = 1
PRESERVE_REFUSAL72 = 'Preserve Numbers needs a CMYK document.'
PROOF_TOOLS72 = ['get_proof_setup', 'set_proof_setup', 'render_proof', 'save_proof_preset']
# Phase 6a-i.
INK_REFUSAL = "ink is a CMYK document's colour. Pass color."
PROBE74 = [(x, y) for y in range(0, 12, 2) for x in range(0, 16, 2)]
PROBE76 = [(x, y) for y in range(0, 16, 2) for x in range(0, 24, 2)]
# Phase 6b-i.
TOP78 = '#d04a20'
THIRD79 = '#30a050'
CHANNEL_BOUND78 = 1
CHANNEL_MATH_NAMES = [n for n in LAB_BLEND_NAMES if n not in (
    'Darker Color', 'Lighter Color', 'Hue', 'Saturation', 'Color', 'Luminosity')]
CHANNEL_MATH_LIMITATION = ("Apply Image and Calculations on a Lab Color document blend each "
                           "channel's codes alone and support " + ', '.join(CHANNEL_MATH_NAMES)
                           + '; every other mode is unavailable there.')
SOURCE_PLANE_REFUSAL78 = ('source_plane must be "lab", "lightness", "a", "b", "alpha" or an alpha '
                          'channel\'s name on a Lab Color document (got "red")')
STACK_BOUND80 = 1
BACK81 = '#2040a0'
RED81 = '#c00000'
BACK82 = '#406080'
PATCH82 = '#f0a0a0'
GRID82 = [(x, y) for y in range(0, 8, 2) for x in range(0, 12, 2)] + [(8, 5), (3, 2)]
# Phase 6b-ii.
GATE_PARTS = ('Lab Color', 'until #1', 'matrix/TRC')
TEX_W, TEX_H = 160, 120
BACK88 = '#2040a0'
PATCH88 = '#e0c020'
# Phase 6b-iii.
AB_BOUND90 = 0.5
L_BOUND90 = 1.0
# Phase 2d.
SCALAR_TARGET_REFUSAL = ("A Lab Color document's scalar histogram takes target lightness, a, b, "
                         'alpha, mask or channel:<name>.')
LAB_VALUE_RULE = 'lightness: L* = 100·code/65535; a, b: 255·code/65535 − 128'
CHANNEL_CARRY_TEXT = ('cannot convert to Lab Color: a Levels adjustment layer on the Red, Green or '
                      'Blue channel')
# Phase 2e.
OVERLAY_BOUND = (0.5, 1.0)
TILE = (48, 32)
HOLE = (20, 12, 6, 6)
# Phase 2f.
AB_BOUND_2F = 0.5
LAB_BLEND_IF_GRAY = ("Blend If on the Gray channel is unavailable in a Lab Color document: "
                     "its Blend If channels are Lightness, a and b.")
AUTO_PATCHES = [(200, 40, 90), (30, 180, 220), (128, 128, 128), (90, 160, 40)]
# Phase 2f, second part: the 2 × 2 tile in row order (red, blue / green, white), opaque.
TILE43 = [(255, 0, 0), (0, 0, 255), (0, 255, 0), (255, 255, 255)]
# Phase 2f, third part.
MODEL_STORE = Path.home() / 'devel/doctored-fixtures/models'
MIGAN = 'doctored/migan-512-places2'
REMOVE_FIELD_BOUND = 5.0
FIELD45 = (60, 140, 90)
ART_DARKEN = 'The Darken blend mode is unavailable in a Lab Color document.'
ART16_REFUSAL = 'The Art History Brush is unavailable in a 16-bit document.'
RED_EYE_A_DROP = 20
# Phase 4a.
GEO_W, GEO_H = 24, 16
PROBE58 = [(x, y) for y in range(0, 10, 3) for x in range(0, 16, 3)]
PAD58 = (0x33, 0x66, 0xcc)
# Phase 3.
BLEND_BOUND8 = 1
BLEND_TOP = '#4a9ad0'
REARMED = ['Multiply', 'Screen', 'Linear Burn', 'Linear Dodge (Add)', 'Vivid Light', 'Hard Mix']
JOINED = ['Darker Color', 'Lighter Color', 'Hue', 'Saturation', 'Color', 'Luminosity']
# The sRGB profile's Bradford-adapted RGB → XYZ (D50) matrix (s15Fixed16 rXYZ, gXYZ, bXYZ) and
# the ICC PCS white.
SRGB_TO_XYZ = [[0x6FA0 / 65536, 0x6297 / 65536, 0x249F / 65536],
               [0x38F5 / 65536, 0xB787 / 65536, 0x0F84 / 65536],
               [0x0390 / 65536, 0x18D9 / 65536, 0xB6C3 / 65536]]
D50 = (0.9642, 1.0, 0.8249)


def texture86(x, y, c):
    """L86's textured 8-bit sample at (x, y), channel c: smooth waves, a hash noise and a
    10-pixel checker, so a registration has structure at every scale."""
    h = (x * 7919 + y * 104729 + c * 1299709) & 0xffffffff
    h = ((h ^ (h >> 15)) * 0x2C1B3C6D) & 0xffffffff
    noise = ((h ^ (h >> 12)) & 0xff) / 255 - 0.5
    wave = math.sin(x / 7.0 + c) * math.cos(y / 5.0 - c)
    checker = 0.5 if (x // 10 + y // 10) % 2 else -0.5
    v = 0.5 + 0.22 * wave + 0.12 * noise + 0.12 * checker
    return max(0, min(255, round(v * 255)))


def srgb_to_lab(rgb8):
    """The CIE L*a*b* (D50) of an 8-bit sRGB colour: the IEC 61966-2-1 curve, the Bradford-adapted
    matrix, the CIE formula."""
    def decode(v):
        v /= 255
        return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4

    def f(t):
        return t ** (1 / 3) if t > 216 / 24389 else (24389 / 27 * t + 16) / 116

    linear = [decode(v) for v in rgb8]
    xyz = [sum(row[c] * linear[c] for c in range(3)) for row in SRGB_TO_XYZ]
    fx, fy, fz = (f(xyz[i] / D50[i]) for i in range(3))
    return (116 * fy - 16, 500 * (fx - fy), 200 * (fy - fz))


def lab_arm(name, cb, cs):
    """Phase 3's 16-bit Lab arm of `name` (module doc, L48–L50) on unit codes [L^, a^, b^]."""
    def star(u):
        return 255 * u - 128

    def unit(a):
        return (min(127.0, max(-128.0, a)) + 128) / 255

    def per(lightness, chroma):
        return [lightness(cb[0], cs[0]), chroma(cb[1], cs[1]), chroma(cb[2], cs[2])]

    def vivid(b, s):
        # Photoshop's halves, the source's ends first (`blend.rs`'s Vivid Light on L*).
        if s in (0.0, 1.0):
            return s
        if s <= 0.5:
            return 1.0 if b == 1 else 1 - min(1.0, (1 - b) / (2 * s))
        return 0.0 if b == 0 else min(1.0, b / (2 - 2 * s))

    def vivid_open(b, s):
        if s in (0.0, 1.0):
            return s
        return 1 - (1 - b) / (2 * s) if s <= 0.5 else b / (2 - 2 * s)

    def hard_mix(b, s):
        if abs(b + s - 1) <= 1 / 4194304:
            return 1.0 if b - s > 1 / 32768 else 0.0
        return 1.0 if b + s >= 1 else 0.0

    def c5(b, s):
        p, q = star(b), star(s)
        return unit(p + q - abs(p) * q / 128)

    def cheb(c):
        a, b = star(c[1]), star(c[2])
        m = max(abs(a), abs(b))
        return a, b, (0.0 if m < 1e-9 else m)

    if name == 'Multiply':
        return per(lambda b, s: b * s, c5)
    if name == 'Screen':
        return per(lambda b, s: b + s - b * s, c5)
    if name == 'Linear Burn':
        return per(lambda b, s: max(0.0, b + s - 1), lambda b, s: unit(star(b) + star(s)))
    if name == 'Linear Dodge (Add)':
        return per(lambda b, s: min(1.0, b + s), lambda b, s: unit(star(b) + star(s)))
    if name == 'Vivid Light':
        def y(u):
            return 0.5 - star(u) / 256
        return per(vivid, lambda b, s: unit(-256 * (vivid_open(y(b), y(s)) - 0.5)))
    if name == 'Hard Mix':
        def y2(u):
            return 0.5 + star(u) / 255
        return per(hard_mix, lambda b, s: unit(255 * (hard_mix(y2(b), y2(s)) - 0.5)))
    if name == 'Darker Color':
        return list(cs) if cs[0] < cb[0] else list(cb)
    if name == 'Lighter Color':
        return list(cs) if cs[0] > cb[0] else list(cb)
    if name == 'Color':
        return [cb[0], cs[1], cs[2]]
    if name == 'Luminosity':
        return [cs[0], cb[1], cb[2]]
    if name == 'Hue':
        _, _, mb = cheb(cb)
        a, b, ms = cheb(cs)
        k = mb / ms if ms > 0 else 0.0
        return [cb[0], unit(a * k), unit(b * k)]
    if name == 'Saturation':
        a, b, mb = cheb(cb)
        _, _, ms = cheb(cs)
        k = ms / mb if mb > 0 else 0.0
        return [cb[0], unit(a * k), unit(b * k)]
    raise ValueError(name)


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))


def png(path, w, h, depth, colour, samples, extra=()):
    """A PNG of `samples` (row-major, channels per colour type) at `depth`."""
    channels = {0: 1, 2: 3, 4: 2, 6: 4}[colour]
    rows = b''
    for y in range(h):
        rows += b'\0'
        for v in samples[y * w * channels:(y + 1) * w * channels]:
            rows += struct.pack('>H', v) if depth == 16 else bytes([v])
    body = chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, depth, colour, 0, 0, 0))
    for c in extra:
        body += c
    path.write_bytes(b'\x89PNG\r\n\x1a\n' + body + chunk(b'IDAT', zlib.compress(rows))
                     + chunk(b'IEND', b''))


def png_pixels(data):
    """(width, height, pixels as tuples) of an 8/16-bit PNG of colour type 0, 2, 4 or 6."""
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    at, packed = 8, b''
    width = height = depth = colour = 0
    while at + 8 <= len(data):
        length, kind = struct.unpack('>I4s', data[at:at + 8])
        body = data[at + 8:at + 8 + length]
        if kind == b'IHDR':
            width, height, depth, colour, _, _, interlace = struct.unpack('>IIBBBBB', body)
            assert depth in (8, 16) and colour in (0, 2, 4, 6) and interlace == 0
        elif kind == b'IDAT':
            packed += body
        at += 12 + length
    channels = {0: 1, 2: 3, 4: 2, 6: 4}[colour]
    unit = channels * depth // 8
    stride = width * unit
    raw = zlib.decompress(packed)
    previous = bytearray(stride)
    pixels = []
    for y in range(height):
        start = y * (stride + 1)
        kind = raw[start]
        row = bytearray(raw[start + 1:start + 1 + stride])
        for i in range(stride):
            left = row[i - unit] if i >= unit else 0
            up = previous[i]
            corner = previous[i - unit] if i >= unit else 0
            if kind == 1:
                row[i] = (row[i] + left) & 255
            elif kind == 2:
                row[i] = (row[i] + up) & 255
            elif kind == 3:
                row[i] = (row[i] + (left + up) // 2) & 255
            elif kind == 4:
                p = left + up - corner
                pa, pb, pc = abs(p - left), abs(p - up), abs(p - corner)
                row[i] = (row[i] + (left if pa <= pb and pa <= pc else up if pb <= pc else corner)) & 255
        previous = row
        for x in range(width):
            px = row[x * unit:(x + 1) * unit]
            if depth == 16:
                pixels.append(tuple(struct.unpack(f'>{channels}H', bytes(px))))
            else:
                pixels.append(tuple(px))
    return width, height, pixels


def png_header(data):
    """PNG: (bit depth, colour type, inflated iCCP or b'')."""
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    at, depth, colour, icc = 8, None, None, b''
    while at + 8 <= len(data):
        length, kind = struct.unpack('>I4s', data[at:at + 8])
        body = data[at + 8:at + 8 + length]
        if kind == b'IHDR':
            depth, colour = body[8], body[9]
        elif kind == b'iCCP':
            name_end = body.index(b'\0')
            icc = zlib.decompress(body[name_end + 2:])
        at += 12 + length
    return depth, colour, icc


def lab_tiff_samples(photometric, bits, alpha):
    """Per pixel of an 8 × 4 Lab TIFF: the stored samples (L, a, b[, alpha]) of a ramp, by the
    published encoding (module doc, L51)."""
    out = []
    for i in range(LAB_TIFF_W * LAB_TIFF_H):
        l_star = 3 + (i * 29) % 95 + (0.41 if bits == 16 else 0)
        a = (i * 47) % 250 - 125 + (0.41 if bits == 16 else 0)
        b = (i * 61 + 13) % 250 - 124 - (0.41 if bits == 16 else 0)
        if bits == 8:
            row = [round(l_star * 255 / 100)]
            if photometric == 8:
                row += [round(v) & 0xFF for v in (a, b)]
            else:
                row += [round(v + 128) for v in (a, b)]
        elif photometric == 8:
            row = [round(l_star * 65535 / 100)] + [round(v * 256) & 0xFFFF for v in (a, b)]
        else:
            row = [round(l_star * 65280 / 100)] + [round((v + 128) * 256) for v in (a, b)]
        if alpha:
            row.append(((i * 37) % 180 + 64) * (1 if bits == 8 else 257))
        out.append(row)
    return out


def lab_tiff_codes(photometric, bits, row):
    """The document's ICC v4 codes of one pixel's stored samples (module doc, L51)."""
    out = []
    for c, v in enumerate(row):
        if c == 3:
            out.append(v * 257 if bits == 8 else v)
        elif bits == 8:
            out.append(((v ^ 0x80) if photometric == 8 and c else v) * 257)
        elif c == 0:
            out.append(v if photometric == 8 else round(v * 65535 / 65280))
        elif photometric == 8:
            signed = v - 65536 if v >= 32768 else v
            out.append(round((signed / 256 + 128) / 255 * 65535))
        else:
            out.append(round(v / 256 / 255 * 65535))
    return out


def write_lab_tiff(path, photometric, bits, rows, big=False, sample_format=None, extra=None,
                   size=(LAB_TIFF_W, LAB_TIFF_H)):
    """An uncompressed chunky Lab TIFF of `rows` (one strip), II or MM, `size` pixels."""
    order = '>' if big else '<'
    spp = len(rows[0])
    code = 'B' if bits == 8 else 'H'
    data = b''.join(struct.pack(order + code * spp, *row) for row in rows)
    if len(data) % 2:
        data += b'\0'
    blobs = b''
    at = 8 + len(data)
    bits_at = at
    blobs += struct.pack(order + 'H' * spp, *([bits] * spp))
    formats_at = at + len(blobs)
    if sample_format:
        blobs += struct.pack(order + 'H' * len(sample_format), *sample_format)
    ifd_at = at + len(blobs)

    def short(v):
        return struct.pack(order + 'HH', v, 0)

    entries = [(256, 4, 1, struct.pack(order + 'I', size[0])),
               (257, 4, 1, struct.pack(order + 'I', size[1])),
               (258, 3, spp, struct.pack(order + 'I', bits_at)),
               (259, 3, 1, short(1)), (262, 3, 1, short(photometric)),
               (273, 4, 1, struct.pack(order + 'I', 8)), (277, 3, 1, short(spp)),
               (278, 4, 1, struct.pack(order + 'I', size[1])),
               (279, 4, 1, struct.pack(order + 'I', len(data))), (284, 3, 1, short(1))]
    if extra is not None:
        entries.append((338, 3, 1, short(extra)))
    if sample_format:
        entries.append((339, 3, len(sample_format), struct.pack(order + 'I', formats_at)))
    ifd = struct.pack(order + 'H', len(entries))
    for tag, kind, n, value in entries:
        ifd += struct.pack(order + 'HHI', tag, kind, n) + value
    ifd += struct.pack(order + 'I', 0)
    head = (b'MM' if big else b'II') + struct.pack(order + 'HI', 42, ifd_at)
    path.write_bytes(head + data + blobs + ifd)


def tiff_lab(data):
    """A baseline TIFF's (tags {number: first value}, every tag number, the first pixel's three
    samples as L and signed a, b)."""
    order = '<' if data[:2] == b'II' else '>'
    assert struct.unpack(order + 'H', data[2:4])[0] == 42
    ifd = struct.unpack(order + 'I', data[4:8])[0]
    count = struct.unpack(order + 'H', data[ifd:ifd + 2])[0]
    tags, seen = {}, set()
    for e in range(count):
        at = ifd + 2 + e * 12
        tag, kind, n = struct.unpack(order + 'HHI', data[at:at + 8])
        size = 2 if kind == 3 else 4
        if n * size <= 4:
            base = at + 8
        else:
            base = struct.unpack(order + 'I', data[at + 8:at + 12])[0]
        value = struct.unpack(order + ('H' if kind == 3 else 'I'), data[base:base + size])[0]
        tags[tag] = value
        seen.add(tag)
    start = tags[273]
    if tags[258] == 8:
        l, a, b = data[start], data[start + 1], data[start + 2]
        a, b = struct.unpack('bb', bytes([a, b]))
    else:
        l, a, b = struct.unpack(order + 'Hhh', data[start:start + 6])
    return tags, seen, (l, a, b)


def psd_facts(data):
    """A PSD's or PSB's header (Adobe's framing): colour mode, depth, header
    channels, the image-resource ids and the signed record count of its
    layer-information section (or of an `Lr16` block)."""
    large = struct.unpack('>H', data[4:6])[0] == 2
    channels, _h, _w, depth, colour = struct.unpack('>HIIHH', data[12:26])
    at = 26
    at += 4 + struct.unpack('>I', data[at:at + 4])[0]
    length = struct.unpack('>I', data[at:at + 4])[0]
    resources, cursor, end = [], at + 4, at + 4 + length
    while cursor < end:
        rid = struct.unpack('>H', data[cursor + 4:cursor + 6])[0]
        name = data[cursor + 6]
        cursor += 6 + ((name + 2) & ~1)
        size = struct.unpack('>I', data[cursor:cursor + 4])[0]
        cursor += 4 + size + size % 2
        resources.append(rid)
    at = end
    width = 8 if large else 4
    at += width
    info = int.from_bytes(data[at:at + width], 'big')
    at += width
    count = struct.unpack('>h', data[at:at + 2])[0] if info else 0
    return dict(mode=colour, depth=depth, channels=channels, resources=resources, count=count)


def tiff_facts(data):
    """A big-endian TIFF's first directory (TIFF 6.0): Photometric, samples,
    BitsPerSample, and whether tags 34675 (ICC) and 338 (ExtraSamples) are
    present."""
    order = '>' if data[:2] == b'MM' else '<'
    ifd = struct.unpack(order + 'I', data[4:8])[0]
    count = struct.unpack(order + 'H', data[ifd:ifd + 2])[0]
    tags = {}
    for i in range(count):
        at = ifd + 2 + 12 * i
        tag, kind, n = struct.unpack(order + 'HHI', data[at:at + 8])
        if kind == 3:
            where = at + 8 if n <= 2 else struct.unpack(order + 'I', data[at + 8:at + 12])[0]
            tags[tag] = list(struct.unpack(order + 'H' * n, data[where:where + 2 * n]))
        else:
            tags[tag] = n
    return dict(photometric=tags.get(262, [None])[0], samples=tags.get(277, [None])[0],
                bits=tags.get(258), icc=34675 in tags, extra=338 in tags)


def wand_key99(codes, depth):
    """L99's oracle key of one pixel's u16 codes (module doc)."""
    def byte(v, channel):
        if depth == 8:
            return (2 * v + 257) // 514
        steps = 32768 if channel == 0 else 32640
        fifteen = (2 * v * steps + 65535) // 131070
        return (fifteen * 255 + 16384) // 32768
    lightness, a, b = (byte(codes[c], c) for c in range(3))
    return (lightness, a, b, (a - lightness + 255) // 2, (b - a + 255) // 2,
            (lightness - b + 255) // 2)


def region99(codes, references, t, depth, contiguous, trim=0):
    """L99–L101's oracle region over row-major `codes` (W99 × H99): admitted when every key
    lies within the references' range ± t (channels) or ± t // 2 (the halves), each key's
    values sorted with `trim` dropped at each end (Similar and Grow: N // 10); flooded
    four-connected from `references` when `contiguous`."""
    keys = [wand_key99(p, depth) for p in codes]
    columns = [sorted(keys[i][k] for i in references) for k in range(6)]
    low = [column[trim] for column in columns]
    high = [column[len(column) - 1 - trim] for column in columns]
    bound = (t, t, t, t // 2, t // 2, t // 2)

    def admit(i):
        return all(low[k] - bound[k] <= keys[i][k] <= high[k] + bound[k] for k in range(6))
    if not contiguous:
        return [admit(i) for i in range(len(codes))]
    out = [False] * len(codes)
    queue = list(references)
    for i in references:
        out[i] = True
    while queue:
        i = queue.pop()
        x, y = i % W99, i // W99
        for nx, ny in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
            j = ny * W99 + nx
            if 0 <= nx < W99 and 0 <= ny < H99 and not out[j] and admit(j):
                out[j] = True
                queue.append(j)
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    output = Path(args.output).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = output / 'work'
    folder.mkdir(parents=True, exist_ok=False)
    receipts = {'expectations': 'fixed in scripts/verify_lab_mode.py before any measurement',
                'pasteboard': PASTEBOARD}
    # Phase 4c: a private pasteboard, before the app starts (module doc, L69).
    os.environ['DR_PASTEBOARD_NAME'] = PASTEBOARD
    # L45's model half: the read-only fixture store, when it holds a verified MI-GAN.
    model_ready = model_store_paths.is_ready(MODEL_STORE, MIGAN)
    if model_ready:
        os.environ['DR_MODELS_DIR'] = str(MODEL_STORE)

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def mode(doc):
            return next(d for d in call('list_documents')['documents'] if d['id'] == doc)['color_mode']

        def summary(doc):
            return next(d for d in call('list_documents')['documents'] if d['id'] == doc)

        def render(doc):
            """(width, height, sRGB pixels, the PNG's base64) of `render` with no channel."""
            result = app.client.rpc('tools/call', {'name': 'render',
                                                   'arguments': {'document_id': doc}})
            assert not result.get('isError'), result
            data = next(c['data'] for c in result['content'] if c['type'] == 'image')
            w, h, px = png_pixels(base64.b64decode(data))
            return w, h, px, data

        def sample(doc, x, y):
            """The pixel as 8-bit codes (a 16-bit document's codes / 257, rounded)."""
            s = call('sample_pixel', document_id=doc, x=x, y=y)
            if 'center_rgba16' in s:
                return [round(v / 257) for v in s['center_rgba16']]
            return [s[k] for k in ('r', 'g', 'b', 'a')]

        def layers(doc):
            return [(l.get('name'), l.get('opacity'), l.get('visible'))
                    for l in call('get_document', document_id=doc)['layers']]

        # ---- L1: New -------------------------------------------------------
        made = {}
        for depth in (8, 16):
            reply = call('new_document', width=6, height=4, color_mode='lab', depth=depth,
                         background='white')
            doc = reply['document']['id']
            assert reply['document']['color_mode'] == 'lab', reply
            w, h, px, _ = render(doc)
            centre = px[(h // 2) * w + w // 2]
            assert all(c >= WHITE_FLOOR for c in centre[:3]), (depth, centre)
            made[depth] = doc
            receipts[f'l1_{depth}'] = centre
        print('PASS l1: New Lab Color at 8 and 16 bits renders white', flush=True)

        # ---- L2: New 32 ----------------------------------------------------
        refusal = call('new_document', fail=True, width=6, height=4, color_mode='lab', depth=32)
        assert refusal == NEW_32_REFUSAL, refusal
        receipts['l2'] = refusal
        print('PASS l2: New Lab Color at 32 bits refuses', flush=True)

        # ---- L3: RGB → Lab -------------------------------------------------
        rgb16 = folder / 'patches16.png'
        png(rgb16, len(PATCHES), 1, 16, 2, [v * 257 for p in PATCHES for v in p])
        doc = call('open_document', path=str(rgb16), precision='preserve')['document']['id']
        reply = call('convert_mode', document_id=doc, mode='lab')
        assert reply['color_mode'] == 'lab' and reply['depth'] == 16, reply
        assert mode(doc) == 'lab'
        _, _, px, _ = render(doc)
        worst = 0
        for i, patch in enumerate(PATCHES):
            worst = max(worst, max(abs(px[i][c] - patch[c]) for c in range(3)))
        assert worst <= RENDER_BOUND8, (worst, px)
        call('undo', document_id=doc)
        assert mode(doc) == 'rgb'
        call('redo', document_id=doc)
        assert mode(doc) == 'lab'
        receipts['l3'] = {'worst8': worst}
        print('PASS l3: RGB → Lab Color renders the source through the view, one undo step', flush=True)

        # ---- L4: Lab → RGB -------------------------------------------------
        back = call('convert_mode', document_id=doc, mode='rgb', rgb_space='srgb')
        assert back['color_mode'] == 'rgb' and back['depth'] == 16, back
        worst = 0
        for i, patch in enumerate(PATCHES):
            got = sample(doc, i, 0)
            worst = max(worst, max(abs(got[c] - patch[c]) for c in range(3)))
        assert worst <= SAMPLE_BOUND8, worst
        receipts['l4'] = {'worst8': worst}
        print('PASS l4: Lab Color → RGB returns each patch', flush=True)

        # ---- L5: 8 bits ----------------------------------------------------
        rgb8 = folder / 'patches8.png'
        png(rgb8, len(PATCHES), 1, 8, 2, [v for p in PATCHES for v in p])
        doc8 = call('open_document', path=str(rgb8))['document']['id']
        assert summary(doc8)['precision'] == 'rgba8'
        reply = call('convert_mode', document_id=doc8, mode='lab')
        assert reply['color_mode'] == 'lab' and reply['depth'] == 8, reply
        reply = call('convert_mode', document_id=doc8, mode='rgb', rgb_space='srgb')
        assert reply['color_mode'] == 'rgb' and summary(doc8)['precision'] == 'rgba8', reply
        receipts['l5'] = summary(doc8)
        print('PASS l5: an 8-bit document keeps its depth through Lab Color', flush=True)

        # ---- L6: Gray → Lab ------------------------------------------------
        gdoc = call('new_document', width=4, height=3, color_mode='gray', depth=16,
                    background='#808080')['document']['id']
        reply = call('convert_mode', document_id=gdoc, mode='lab')
        assert reply['color_mode'] == 'lab', reply
        _, _, px, _ = render(gdoc)
        spread = max(max(abs(p[0] - p[1]), abs(p[1] - p[2])) for p in px)
        assert spread <= NEUTRAL_BOUND8, (spread, px[:3])
        receipts['l6'] = {'spread8': spread, 'pixel': px[0]}
        print('PASS l6: Grayscale → Lab Color stays neutral', flush=True)

        # ---- L7: CMYK → Lab ------------------------------------------------
        cdoc = call('new_document', width=4, height=3, color_mode='cmyk', depth=16,
                    background='#4080c0')['document']['id']
        reply = call('convert_mode', document_id=cdoc, mode='lab')
        assert reply['color_mode'] == 'lab' and reply['depth'] == 16, reply
        call('undo', document_id=cdoc)
        assert mode(cdoc) == 'cmyk'
        call('redo', document_id=cdoc)
        assert mode(cdoc) == 'lab'
        receipts['l7'] = reply['working_memory_plan_bytes']
        print('PASS l7: CMYK → Lab Color through RGB, one undo step', flush=True)

        # ---- L8: 32 bits ---------------------------------------------------
        fdoc = call('new_document', width=4, height=3, depth=32, background='#336699')['document']['id']
        refusal = call('convert_mode', fail=True, document_id=fdoc, mode='lab')
        assert FLOAT_TEXT in refusal and mode(fdoc) == 'rgb', refusal
        receipts['l8'] = refusal
        print('PASS l8: a 32-bit document refuses Lab Color', flush=True)

        # ---- L9: flatten ---------------------------------------------------
        mdoc = call('new_document', width=4, height=3, depth=16, background='#336699')['document']['id']
        call('new_layer', document_id=mdoc, name='Darken')
        call('set_layer_properties', document_id=mdoc, index=1, blend_mode='Darken')
        refusal = call('convert_mode', fail=True, document_id=mdoc, mode='lab')
        assert FLATTEN_TEXT in refusal and mode(mdoc) == 'rgb', refusal
        flattened = call('convert_mode', document_id=mdoc, mode='lab', flatten=True)
        assert flattened['document']['layer_count'] == 1 and mode(mdoc) == 'lab', flattened
        call('undo', document_id=mdoc)
        assert mode(mdoc) == 'rgb' and summary(mdoc)['layer_count'] == 2
        receipts['l9'] = refusal
        print('PASS l9: a Darken layer is named; flatten converts in one undo step', flush=True)

        # ---- L10: .rz ------------------------------------------------------
        rz_docs = {16: made[16], 8: doc8}
        call('convert_mode', document_id=doc8, mode='lab')
        for depth, source in rz_docs.items():
            path = folder / f'lab{depth}.rz'
            call('save_copy', document_id=source, path=str(path), format='rz')
            reopened = call('open_document', path=str(path))['document']['id']
            assert mode(reopened) == 'lab', depth
            assert summary(reopened)['layer_count'] == summary(source)['layer_count'], depth
            assert render(reopened)[2] == render(source)[2], depth
            receipts[f'l10_{depth}'] = str(path)
        print('PASS l10: the .rz round trip keeps a Lab Color document at 8 and 16 bits', flush=True)

        # ---- L11: refusals -------------------------------------------------
        ldoc = made[16]
        refusals = {
            'png': call('save_copy', fail=True, document_id=ldoc, path=str(folder / 'x.png'),
                        format='png'),
            'get_tone_map': call('get_tone_map', fail=True, document_id=ldoc),
        }
        assert refusals['png'] == SAVE_REFUSAL, refusals
        assert refusals['get_tone_map'] == TONE_MAP_REFUSAL.format('get_tone_map'), refusals
        receipts['l11'] = refusals
        print('PASS l11: the save and the 32-bit tone map refuse by name', flush=True)

        # ---- L12: structure ------------------------------------------------
        sdoc = call('new_document', width=4, height=3, color_mode='lab', depth=16,
                    background='#336699')['document']['id']
        steps = [
            ('duplicate_layer', {'index': 0}),
            ('set_layer_properties', {'index': 1, 'opacity': 0.5}),
            ('reorder_layer', {'from': 1, 'to': 0}),
            ('set_layer_properties', {'index': 0, 'name': 'Renamed'}),
            ('duplicate_layer', {'index': 0}),
            ('delete_layer', {'index': 2}),
            ('flatten_image', {}),
        ]
        done = []
        for tool, arguments in steps:
            before = layers(sdoc)
            call(tool, document_id=sdoc, **arguments)
            after = layers(sdoc)
            assert after != before, (tool, before, after)
            call('undo', document_id=sdoc)
            assert layers(sdoc) == before, (tool, 'undo', layers(sdoc), before)
            call('redo', document_id=sdoc)
            assert layers(sdoc) == after, (tool, 'redo', layers(sdoc), after)
            assert mode(sdoc) == 'lab', tool
            done.append(tool)
        receipts['l12'] = done
        print('PASS l12: duplicate, opacity, reorder, rename, delete and flatten are one undo step each',
              flush=True)

        # ---- L13: the 1b structure ----------------------------------------
        def state(doc):
            return (call('get_document', document_id=doc)['layers'],
                    call('list_channels', document_id=doc))

        def one_step(doc, tool, **arguments):
            """`tool` is ONE undo step and the document stays Lab."""
            before = state(doc)
            reply = call(tool, document_id=doc, **arguments)
            after = state(doc)
            assert after != before, (tool, 'changed nothing')
            assert mode(doc) == 'lab', tool
            call('undo', document_id=doc)
            assert state(doc) == before, (tool, 'undo')
            call('redo', document_id=doc)
            assert state(doc) == after, (tool, 'redo')
            assert mode(doc) == 'lab', tool
            return reply

        tdoc = call('new_document', width=8, height=4, color_mode='lab', depth=16,
                    background='#336699')['document']['id']
        one_step(tdoc, 'new_layer', name='Top')
        call('fill_selection', document_id=tdoc, layer=1, color='#ffffff')
        one_step(tdoc, 'add_layer_mask', layer=1, kind='reveal_all')
        one_step(tdoc, 'set_layer_properties', index=1, blend_mode='Multiply', opacity=0.5)
        one_step(tdoc, 'new_layer', name='Clip')
        call('fill_selection', document_id=tdoc, layer=2, color='#404040')
        one_step(tdoc, 'set_layer_clipped', layer=2, clipped=True)
        one_step(tdoc, 'set_layer_style', layer=1, style={'fill_opacity': 0.5})
        one_step(tdoc, 'stamp_visible', above=2, name='Stamp')
        one_step(tdoc, 'merge_down', index=3)
        one_step(tdoc, 'group_layers', layers=[1, 2], name='Group')
        one_step(tdoc, 'add_channel', name='Matte')
        one_step(tdoc, 'rename_channel', channel='Matte', name='Matte 2')
        one_step(tdoc, 'delete_channel', channel='Matte 2')
        limits = {}
        for name in ('Subtract', 'Darken'):
            text = call('set_layer_properties', fail=True, document_id=tdoc, index=0, blend_mode=name)
            assert text.startswith(LAB_LIMITATION_HEAD) and text.endswith(LAB_LIMITATION_TAIL), text
            assert 'Subtract' not in text and 'Darken' not in text, text
            limits[name] = text
        receipts['l13'] = {'limitation': limits}
        print('PASS l13: the 1b layer structure and alpha channels, one undo step each', flush=True)

        # ---- L14: Fill -----------------------------------------------------
        fdoc16 = call('new_document', width=4, height=4, color_mode='lab', depth=16,
                      background='white')['document']['id']

        def centre(doc):
            w, h, px, _ = render(doc)
            return px[(h // 2) * w + w // 2]

        backdrop = centre(fdoc16)
        call('new_layer', document_id=fdoc16, name='Red')
        call('fill_selection', document_id=fdoc16, layer=1, color='#ff0000')
        red = centre(fdoc16)
        assert all(abs(red[c] - (255, 0, 0)[c]) <= FILL_BOUND8 for c in range(3)), red
        call('add_layer_mask', document_id=fdoc16, layer=1, kind='reveal_all')
        call('fill_selection', document_id=fdoc16, layer=1, target='mask', color='#000000')
        hidden = centre(fdoc16)
        assert hidden == backdrop, (hidden, backdrop)
        receipts['l14'] = {'red': red, 'hidden': hidden, 'backdrop': backdrop}
        print('PASS l14: Fill paints sRGB red; a black mask fill hides the layer', flush=True)

        # ---- L15: Lab TIFF -------------------------------------------------
        tiffs = {}
        for depth in (8, 16):
            top = 255 if depth == 8 else 65535
            for background in ('white', '#808080'):
                bdoc = call('new_document', width=4, height=3, color_mode='lab', depth=depth,
                            background=background)['document']['id']
                path = folder / f'lab{depth}-{background.strip("#")}.tif'
                reply = call('save_copy', document_id=bdoc, path=str(path), format='tiff')
                assert reply['format'] == 'TIFF' and reply['bit_depth'] == depth, reply
                tags, seen, (l, a, b) = tiff_lab(path.read_bytes())
                assert tags[262] == 8 and tags[277] == 3 and tags[258] == depth, tags
                assert 34675 not in seen, sorted(seen)
                if background == 'white':
                    assert (l, a, b) == (top, 0, 0), (depth, l, a, b)
                else:
                    assert abs(a) <= GRAY_AB_BOUND[depth] and abs(b) <= GRAY_AB_BOUND[depth], (l, a, b)
                tiffs[f'{depth}-{background}'] = [l, a, b]
                if depth == 8 and background == 'white':
                    refusal = call('save_copy', fail=True, document_id=bdoc,
                                   path=str(folder / 'deep.tif'), format='tiff', bit_depth=16)
                    assert refusal == TIFF_DEPTH_REFUSAL_8, refusal
                    # A plain one-layer Lab document's TIFF reopens as the document, so Save
                    # keeps it in place; a guide, which the TIFF has no place for, takes it out.
                    types = call('get_document', document_id=bdoc)['save_preservation']['save_types']
                    assert 'public.tiff' in types, types
                    call('add_guide', document_id=bdoc, orientation='vertical', position=2)
                    guided = call('get_document', document_id=bdoc)['save_preservation']
                    assert 'public.tiff' not in guided['save_types'], guided['save_types']
                    assert 'guides and ruler origin' in guided['losses_by_format']['tiff'], guided
                    tiffs['save_types'] = types
                    tiffs['save_types_with_guide'] = guided['save_types']
        receipts['l15'] = tiffs
        print('PASS l15: save_copy tiff writes CIELab at 8 and 16 bits', flush=True)

        # ---- L16: exports through RGB --------------------------------------
        def image(tool, **arguments):
            result = app.client.rpc('tools/call', {'name': tool, 'arguments': arguments})
            assert not result.get('isError'), (tool, result)
            data = next(c['data'] for c in result['content'] if c['type'] == 'image')
            return png_pixels(base64.b64decode(data))

        edoc = call('open_document', path=str(rgb16), precision='preserve')['document']['id']
        call('convert_mode', document_id=edoc, mode='lab')
        assert mode(edoc) == 'lab'
        _, _, shown, _ = render(edoc)
        e8 = folder / 'e8.png'
        call('export_image', document_id=edoc, path=str(e8),
             settings=dict(format='png', bit_depth=8, profile='srgb'))
        bits8, colour8, _ = png_header(e8.read_bytes())
        assert bits8 == 8 and colour8 in (2, 6), (bits8, colour8)
        _, _, exported = png_pixels(e8.read_bytes())
        worst = max(abs(exported[i][c] - shown[i][c]) for i in range(len(PATCHES)) for c in range(3))
        assert worst <= EXPORT_BOUND8, (worst, exported, shown)
        e16 = folder / 'e16.png'
        call('export_image', document_id=edoc, path=str(e16),
             settings=dict(format='png', bit_depth=16, profile='document'))
        bits16, colour16, icc16 = png_header(e16.read_bytes())
        assert bits16 == 16 and colour16 in (2, 6) and b'sRGB' in icc16, (bits16, colour16, icc16[:64])
        pw, ph, _ = image('export_preview', document_id=edoc,
                          settings=dict(format='png', bit_depth=8, profile='srgb'))
        assert (pw, ph) == (len(PATCHES), 1), (pw, ph)
        destination = folder / 'quick'
        destination.mkdir()
        quick = call('quick_export', document_id=edoc, destination=str(destination),
                     pattern='{name}-q', settings=dict(format='png', bit_depth=8, profile='srgb'))
        assert Path(quick['path']).read_bytes()[:8] == b'\x89PNG\r\n\x1a\n', quick
        receipts['l16'] = {'worst8': worst, 'quick': quick['path']}
        print('PASS l16: export_image, export_preview and quick_export write RGB', flush=True)

        # ---- L17: .rz of the structure document ----------------------------
        rz17 = folder / 'structure.rz'
        call('save_copy', document_id=tdoc, path=str(rz17), format='rz')
        rdoc = call('open_document', path=str(rz17))['document']['id']
        assert mode(rdoc) == 'lab'
        assert summary(rdoc)['layer_count'] == summary(tdoc)['layer_count']
        assert render(rdoc)[2] == render(tdoc)[2]
        receipts['l17'] = str(rz17)
        print('PASS l17: the .rz round trip keeps the 1b structure document', flush=True)

        # ---- L18: get_precision (1c) ---------------------------------------
        precision, blend18 = {}, {}
        for depth in (8, 16):
            reply = call('get_precision', document_id=made[depth])
            assert reply['color_mode'] == 'lab', reply
            assert reply['mode'] == f'lab{depth}' and reply['bits_per_channel'] == depth, reply
            assert reply['supported_tools'] == sorted(LAB_TOOLS), reply['supported_tools']
            assert 'clone_stamp' in reply['supported_tools']
            assert reply['blend_modes'] == LAB_BLEND_NAMES + ['Pass Through (groups)'], reply['blend_modes']
            blend18[depth] = reply['blend_modes']
            support = call('get_document', document_id=made[depth])['precision_support']
            assert support['supported_tools'] == reply['supported_tools'], support
            precision[depth] = {k: reply[k] for k in ('mode', 'mode_description', 'bits_per_channel')}
        receipts['l18'] = precision
        print('PASS l18: get_precision answers the Lab gate\'s own lists at 8 and 16 bits', flush=True)

        # ---- L19: sample_color / sample_pixel (1c) -------------------------
        patch_docs = {}
        for depth, extra in ((16, {'precision': 'preserve'}), (8, {})):
            # Fresh copies of L3's and L5's PNGs: open_document answers an open path's document.
            source = folder / f'patches{depth}-1c.png'
            png(source, len(PATCHES), 1, depth, 2,
                [v * (257 if depth == 16 else 1) for p in PATCHES for v in p])
            pdoc = call('open_document', path=str(source), **extra)['document']['id']
            reply = call('convert_mode', document_id=pdoc, mode='lab')
            assert reply['color_mode'] == 'lab' and reply['depth'] == depth, reply
            patch_docs[depth] = pdoc
        worst = {}
        for depth, pdoc in patch_docs.items():
            bound_l, bound_ab = LAB_BOUND[depth]
            worst[depth] = [0.0, 0.0]
            for i, patch in enumerate(PATCHES):
                colour = call('sample_color', document_id=pdoc, x=i, y=0)
                lab = colour['lab']
                got = (lab['l'], lab['a'], lab['b'])
                oracle = srgb_to_lab(patch)
                dl = abs(got[0] - oracle[0])
                dab = max(abs(got[1] - oracle[1]), abs(got[2] - oracle[2]))
                assert dl <= bound_l and dab <= bound_ab, (depth, i, got, oracle)
                worst[depth] = [max(worst[depth][0], dl), max(worst[depth][1], dab)]
                n = colour['normalized_rgba']
                exact = (100 * n[0], 255 * n[1] - 128, 255 * n[2] - 128)
                assert all(abs(got[c] - exact[c]) <= LAB_EXACT for c in range(3)), (got, exact)
                assert 'srgb_rgba' not in colour, colour
                pixel = call('sample_pixel', document_id=pdoc, x=i, y=0)
                assert pixel['lab'] == lab, (pixel['lab'], lab)
        whites = {}
        for depth in (8, 16):
            lab = call('sample_color', document_id=made[depth], x=1, y=1)['lab']
            got = (lab['l'], lab['a'], lab['b'])
            assert all(abs(got[c] - (100, 0, 0)[c]) <= WHITE_LAB_BOUND for c in range(3)), (depth, got)
            whites[depth] = got
        receipts['l19'] = {'worst_l_ab': worst, 'white': whites}
        print('PASS l19: sample_color and sample_pixel read L*a*b* on the CIE formula', flush=True)

        # ---- L20: render's Lab channels (1c) -------------------------------
        def channel_pixels(doc, **arguments):
            result = app.client.rpc('tools/call', {'name': 'render',
                                                   'arguments': dict(document_id=doc, **arguments)})
            assert not result.get('isError'), (arguments, result)
            data = next(c['data'] for c in result['content'] if c['type'] == 'image')
            return png_pixels(base64.b64decode(data))[2]

        pdoc = patch_docs[8]
        codes = [call('sample_pixel', document_id=pdoc, x=i, y=0)['mean_rgba_codes']
                 for i in range(len(PATCHES))]
        spread = 0
        for c, name in enumerate(('lightness', 'a', 'b')):
            shown = channel_pixels(pdoc, channel=name)
            for i in range(len(PATCHES)):
                spread = max(spread, abs(shown[i][0] - round(codes[i][c] / 257)))
        assert spread <= CHANNEL_BOUND8, spread
        refused = {
            'red': call('render', fail=True, document_id=pdoc, channel='red'),
            'composite': call('render', fail=True, document_id=pdoc, channel='composite'),
            'lightness_layer': call('render', fail=True, document_id=pdoc, channel='lightness', layer=0),
        }
        assert all(text == RENDER_REFUSAL for text in refused.values()), refused
        call('add_layer_mask', document_id=pdoc, layer=0, kind='hide_all')
        assert all(p[0] == 0 for p in channel_pixels(pdoc, channel='mask', layer=0))
        call('add_channel', document_id=pdoc, name='Matte')
        assert all(p[0] == 0 for p in channel_pixels(pdoc, channel='Matte'))
        receipts['l20'] = {'spread8': spread, 'refused': refused}
        print('PASS l20: render draws lightness, a and b of the codes, a mask and an alpha channel',
              flush=True)

        # ---- L21: histogram (1c) -------------------------------------------
        hdoc = call('new_document', width=6, height=4, color_mode='lab', depth=16,
                    background='white')['document']['id']
        report = call('histogram', document_id=hdoc)
        assert report['channels'] == ['lightness', 'a', 'b'], report.get('channels')
        assert len(report['bins']) == 3 and len(report['bin_weight_units']) == 3, report
        for row, peak in zip(report['bin_weight_units'], (255, 128, 128)):
            assert sum(row) > 0 and row[peak] == sum(row), (peak, row)
        assert not [k for k in report if k.endswith('_luma')], sorted(report)
        refusal = call('histogram', fail=True, document_id=hdoc, source='scalar', target='red')
        assert refusal == SCALAR_TARGET_REFUSAL, refusal
        call('add_layer_mask', document_id=hdoc, layer=0, kind='hide_all')
        scalar = call('histogram', document_id=hdoc, source='scalar', target='mask', layer=0)
        assert 'lab_bin_rule' not in scalar, sorted(scalar)
        assert scalar.get('channels') != ['lightness', 'a', 'b'], scalar.get('channels')
        receipts['l21'] = {'keys': sorted(report), 'scalar_keys': sorted(scalar), 'refusal': refusal}
        print('PASS l21: histogram counts Lightness, a and b; a scalar mask report is untouched',
              flush=True)

        # ---- L22: the measured blend gate (phase 2's take) -----------------
        bdoc = call('new_document', width=4, height=4, color_mode='lab', depth=16,
                    background='#336699')['document']['id']
        call('new_layer', document_id=bdoc, name='Top')

        def undo_count(doc):
            return call('list_history', document_id=doc)['undo_count']

        before = undo_count(bdoc)
        call('set_layer_properties', document_id=bdoc, index=1, blend_mode='Multiply')
        assert undo_count(bdoc) == before + 1, 'Multiply is not one undo step'
        current, walked = 'Multiply', {}
        for name in ALL_BLEND_NAMES:
            before = undo_count(bdoc)
            if name in LAB_BLEND_NAMES:
                call('set_layer_properties', document_id=bdoc, index=1, blend_mode=name)
                steps = undo_count(bdoc) - before
                assert steps == (0 if name == current else 1), (name, current, steps)
                current = name
                walked[name] = f'admitted, {steps} undo step(s)'
            else:
                text = call('set_layer_properties', fail=True, document_id=bdoc, index=1,
                            blend_mode=name)
                assert text.startswith(LAB_LIMITATION_HEAD), (name, text)
                assert undo_count(bdoc) == before, (name, 'a refusal added an undo step')
                walked[name] = 'refused'
        assert mode(bdoc) == 'lab'
        assert call('get_precision', document_id=bdoc)['blend_modes'] == blend18[16]
        receipts['l22'] = walked
        print(f'PASS l22: {len(LAB_BLEND_NAMES)} measured Lab modes admitted, '
              f'{len(ALL_BLEND_NAMES) - len(LAB_BLEND_NAMES)} refused', flush=True)


        # ---- L23–L27: painting and adjustments (phase 2a and 2b) -------------------------------
        def new_lab(depth, w=8, h=4, background='white'):
            return call('new_document', width=w, height=h, color_mode='lab', depth=depth,
                        background=background)['document']['id']

        def codes_at(doc, points, **scope):
            """`sample_pixel`'s `mean_rgba_codes` (u16 codes) at each point."""
            return [[int(round(v)) for v in call('sample_pixel', document_id=doc, x=x, y=y,
                                                  **scope)['mean_rgba_codes']]
                    for x, y in points]

        def lab_at(doc, x, y):
            lab = call('sample_color', document_id=doc, x=x, y=y)['lab']
            return (lab['l'], lab['a'], lab['b'])

        def within(got, rgb8, depth, what):
            bound_l, bound_ab = LAB_BOUND[depth]
            want = srgb_to_lab(rgb8)
            assert abs(got[0] - want[0]) <= bound_l, (what, got, want)
            assert max(abs(got[1] - want[1]), abs(got[2] - want[2])) <= bound_ab, (what, got, want)

        def pixel_step(doc, probe, tool, **arguments):
            """`tool` changes `probe()` in ONE undo step; the document stays lab."""
            before, count = probe(), undo_count(doc)
            reply = call(tool, document_id=doc, **arguments)
            after = probe()
            assert after != before, (tool, 'changed nothing')
            assert undo_count(doc) == count + 1, (tool, 'not one undo step')
            assert mode(doc) == 'lab', tool
            call('undo', document_id=doc)
            assert probe() == before, (tool, 'undo')
            call('redo', document_id=doc)
            assert probe() == after, (tool, 'redo')
            return after, reply

        W23, H23 = 8, 4
        everywhere = [(x, y) for y in range(H23) for x in range(W23)]
        cover = dict(points=[[0, 2], [W23 - 1, 2]], size=40, hardness=100)

        # L23: Brush and Eraser at 16 bits.
        pdoc23 = new_lab(16)
        call('new_layer', document_id=pdoc23, name='Paint')
        pixel_step(pdoc23, lambda: codes_at(pdoc23, everywhere), 'brush_stroke', color='#ff0000',
                   **cover)
        red23 = lab_at(pdoc23, 4, 2)
        within(red23, (255, 0, 0), 16, 'brush 16')
        layer_codes = lambda: codes_at(pdoc23, everywhere, source='layer', layer=1)  # noqa: E731
        erased23, _ = pixel_step(pdoc23, layer_codes, 'eraser_stroke', **cover)
        assert all(p[3] == 0 for p in erased23), erased23
        darken = call('brush_stroke', fail=True, document_id=pdoc23, color='#ff0000',
                      blend_mode='Darken', **cover)
        assert darken.startswith(LAB_LIMITATION_HEAD), darken
        receipts['l23'] = {'red': red23, 'darken': darken}
        print('PASS l23: the Brush paints red\'s L*a*b* and the Eraser clears, one step each',
              flush=True)

        # L24: the bucket, Fill's paint and the Gradient at 16 bits.
        fills24 = {}
        for name, colour, rgb8, extra in (('bucket', '#0000ff', (0, 0, 255), {}),
                                          ('paint', '#00ff00', (0, 255, 0),
                                           {'contiguous': False})):
            fdoc24 = new_lab(16)
            pixel_step(fdoc24, lambda: codes_at(fdoc24, everywhere), 'fill', x=3, y=2,
                       color=colour, tolerance=0, **extra)
            got = lab_at(fdoc24, 4, 2)
            within(got, rgb8, 16, name)
            fills24[name] = got
        gdoc24 = new_lab(16)
        row24 = [(x, 2) for x in range(W23)]
        ramp24, _ = pixel_step(gdoc24, lambda: codes_at(gdoc24, row24), 'gradient', x0=1, y0=2,
                               x1=W23 - 1, y1=2, start_color='#000000', end_color='#ffffff')
        within(lab_at(gdoc24, 0, 2), (0, 0, 0), 16, 'gradient start')
        within(lab_at(gdoc24, W23 - 1, 2), (255, 255, 255), 16, 'gradient end')
        assert all(b[0] >= a[0] for a, b in zip(ramp24, ramp24[1:])), ramp24
        receipts['l24'] = {'fills': fills24, 'ramp_l': [p[0] for p in ramp24]}
        print('PASS l24: the bucket, Fill and the Gradient paint L*a*b*, one step each', flush=True)

        # L25: 8 bits and an Action.
        doc25 = new_lab(8)
        call('brush_stroke', document_id=doc25, color='#ff0000', **cover)
        red25 = lab_at(doc25, 4, 2)
        within(red25, (255, 0, 0), 8, 'brush 8')
        centre25 = codes_at(doc25, [(4, 2)])[0]
        assert all(v % 257 == 0 for v in centre25), centre25
        brush_args = dict(points=[[1, 1], [6, 2]], size=3, hardness=100, color='#ff0000')
        call('save_action', name='lab-paint-levels-547', overwrite=True, steps=[
            {'tool': 'brush_stroke', 'arguments': brush_args},
            {'tool': 'apply_filter', 'arguments': {'filter': 'levels', 'gamma': 1.4}},
        ])
        played25, direct25 = new_lab(8), new_lab(8)
        call('run_action', document_id=played25, name='lab-paint-levels-547')
        call('brush_stroke', document_id=direct25, **brush_args)
        call('apply_filter', document_id=direct25, layer=0, filter='levels', gamma=1.4)
        replayed = codes_at(played25, everywhere)
        assert replayed == codes_at(direct25, everywhere), 'the Action replays the two steps'
        assert replayed != codes_at(new_lab(8), everywhere), 'the Action changed nothing'
        assert mode(played25) == 'lab'
        call('delete_action', name='lab-paint-levels-547')
        receipts['l25'] = {'red8': red25, 'centre8': centre25}
        print('PASS l25: the Brush paints at 8 bits on the 257 grid; an Action replays paint and '
              'Levels', flush=True)

        # L26: the adjustments on the 16-bit patch document.
        adoc26 = patch_docs[16]
        patches26 = [(i, 0) for i in range(len(PATCHES))]
        before26 = codes_at(adoc26, patches26)
        levelled26, _ = pixel_step(adoc26, lambda: codes_at(adoc26, patches26), 'apply_filter',
                                   layer=0, filter='levels', gamma=1.4)
        assert all(a[1:] == b[1:] for a, b in zip(levelled26, before26)), (levelled26, before26)
        call('undo', document_id=adoc26)
        inverted26, _ = pixel_step(adoc26, lambda: codes_at(adoc26, patches26), 'apply_filter',
                                   layer=0, filter='invert')
        assert all(a[c] == 65535 - b[c] for a, b in zip(inverted26, before26) for c in range(3)), (
            inverted26, before26)
        assert [p[3] for p in inverted26] == [p[3] for p in before26], inverted26
        call('undo', document_id=adoc26)
        assert codes_at(adoc26, patches26) == before26
        threshold26, _ = pixel_step(adoc26, lambda: codes_at(adoc26, patches26),
                                    'add_adjustment_layer', op='threshold', params={'level': 0.5},
                                    name='Threshold')
        for p in threshold26:
            assert p[1] == NEUTRAL16 and p[2] == NEUTRAL16 and p[0] in (0, 65535), threshold26
        rz26 = folder / 'adjusted.rz'
        call('save_copy', document_id=adoc26, path=str(rz26), format='rz')
        rdoc26 = call('open_document', path=str(rz26))['document']['id']
        assert mode(rdoc26) == 'lab'
        assert summary(rdoc26)['layer_count'] == summary(adoc26)['layer_count']
        assert any(l.get('is_adjustment') for l in call('get_document', document_id=rdoc26)['layers'])
        back26 = call('convert_mode', document_id=adoc26, mode='rgb', rgb_space='srgb')
        assert back26['color_mode'] == 'rgb', back26
        assert any(l.get('is_adjustment') for l in call('get_document', document_id=adoc26)['layers'])
        receipts['l26'] = {'levels': levelled26, 'invert': inverted26, 'threshold': threshold26}
        print('PASS l26: Levels keeps a and b, Invert inverts L, a and b, a Threshold layer is '
              'black and white; .rz and Lab → RGB keep the layer', flush=True)

        # L27: exact refusals.
        rdoc27 = new_lab(16)
        refusals27 = {
            'exposure': call('apply_filter', fail=True, document_id=rdoc27, layer=0,
                             filter='exposure', exposure=0.5),
            'black_and_white': call('add_adjustment_layer', fail=True, document_id=rdoc27,
                                    op='black_and_white'),
            'auto_color': call('auto_color', fail=True, document_id=rdoc27, layer=0),
            'hdr_toning': call('hdr_toning', fail=True, document_id=rdoc27),
            'sepia': call('apply_filter', fail=True, document_id=rdoc27, layer=0, filter='sepia'),
            'target_red': call('brush_stroke', fail=True, document_id=rdoc27, color='#ff0000',
                               target='red', **cover),
        }
        want27 = {
            'exposure': UNAVAILABLE_REFUSAL.format('Exposure'),
            'black_and_white': UNAVAILABLE_REFUSAL.format('Black & White'),
            'auto_color': UNAVAILABLE_REFUSAL.format('Auto Color'),
            'hdr_toning': UNAVAILABLE_REFUSAL.format('HDR Toning'),
            'sepia': RGB_ONLY_REFUSAL.format('Sepia'),
            'target_red': PLANE_TARGET_REFUSAL,
        }
        assert refusals27 == want27, refusals27
        receipts['l27'] = refusals27
        print('PASS l27: refusals name the Lab sentence, RGB only and the Lab plane names',
              flush=True)

        # ---- L28–L31: filters, Fade, colour adjustments, Auto (phase 2c) ----------------------
        fresh_count = [0]

        def fresh_patches(patches=PATCHES):
            """A new 16-bit Lab document of `patches` (a new PNG path each time)."""
            fresh_count[0] += 1
            source = folder / f'patches16-2c-{fresh_count[0]}.png'
            png(source, len(patches), 1, 16, 2, [v * 257 for p in patches for v in p])
            pdoc = call('open_document', path=str(source), precision='preserve')['document']['id']
            reply = call('convert_mode', document_id=pdoc, mode='lab')
            assert reply['color_mode'] == 'lab' and reply['depth'] == 16, reply
            return pdoc

        patches2c = [(i, 0) for i in range(len(PATCHES))]

        def patch_codes(doc):
            return codes_at(doc, patches2c)

        def lab_of_code(c):
            return (100 * c[0] / 65535, 255 * c[1] / 65535 - 128, 255 * c[2] / 65535 - 128)

        # L28: filters.
        doc28 = fresh_patches()
        before28 = patch_codes(doc28)
        changed28 = {}
        for filt, extra in (('blur', {'sigma': 2}),
                            ('unsharp_mask', {'radius': 1.5, 'amount': 1.2, 'threshold': 0}),
                            ('add_noise', {'amount': 0.2, 'seed': 7}),
                            ('pixelate', {'block': 2})):
            after, _ = pixel_step(doc28, lambda: patch_codes(doc28), 'apply_filter', layer=0,
                                  filter=filt, **extra)
            changed28[filt] = sum(1 for a, b in zip(after, before28) if a != b)
            call('undo', document_id=doc28)
            assert patch_codes(doc28) == before28, filt
        call('apply_filter', document_id=doc28, layer=0, filter='clouds', seed=3,
             foreground_color='#ff0000', background_color='#0000ff')
        red28, blue28 = srgb_to_lab((255, 0, 0)), srgb_to_lab((0, 0, 255))
        bound_l, bound_ab = LAB_BOUND[16]
        clouds28 = []
        for x, y in patches2c:
            got = lab_at(doc28, x, y)
            clouds28.append(got)
            for c, bound in ((0, bound_l), (1, bound_ab), (2, bound_ab)):
                lo, hi = sorted((red28[c], blue28[c]))
                assert lo - bound <= got[c] <= hi + bound, ('clouds', c, got, red28, blue28)
        rdoc28 = fresh_patches()
        refusals28 = {
            'lens_correction': call('apply_filter', fail=True, document_id=rdoc28, layer=0,
                                    filter='lens_correction', distortion=20),
            'develop_filter': call('apply_filter', fail=True, document_id=rdoc28, layer=0,
                               filter='develop_filter'),
            'difference_clouds': call('apply_filter', fail=True, document_id=rdoc28, layer=0,
                                      filter='difference_clouds', seed=5),
            'presence': call('apply_filter', fail=True, document_id=rdoc28, layer=0,
                             filter='presence', clarity=40),
            'depth_blur': call('depth_blur', fail=True, document_id=rdoc28, layer=0),
        }
        want28 = {
            'lens_correction': UNAVAILABLE_REFUSAL.format('Lens Correction'),
            'develop_filter': UNAVAILABLE_REFUSAL.format('Develop Filter'),
            'difference_clouds': UNAVAILABLE_REFUSAL.format('Difference Clouds'),
            'presence': DEVELOP_FILTER_REFUSAL.format('Presence'),
            'depth_blur': RGB_ONLY_REFUSAL.format('Depth Blur'),
        }
        assert refusals28 == want28, refusals28
        receipts['l28'] = {'changed': changed28, 'clouds': clouds28, 'refusals': refusals28}
        print('PASS l28: blur, unsharp mask, noise and pixelate run on L*a*b*, clouds paint between '
              'their colours, and the disabled filters refuse', flush=True)

        # L29: Fade and Actions.
        doc29 = fresh_patches()
        before29 = patch_codes(doc29)
        # Not `pixel_step`: its undo and redo would leave nothing to fade.
        count29 = undo_count(doc29)
        call('apply_filter', document_id=doc29, layer=0, filter='blur', sigma=2)
        blurred29 = patch_codes(doc29)
        assert blurred29 != before29 and undo_count(doc29) == count29 + 1, blurred29
        call('fade', document_id=doc29, opacity=0.5)
        assert undo_count(doc29) == count29 + 1, 'the Fade replaces the blur step'
        faded29 = patch_codes(doc29)
        worst29 = 0.0
        for f, b, a in zip(faded29, before29, blurred29):
            for c in range(3):
                worst29 = max(worst29, abs(f[c] - (b[c] + a[c]) / 2))
        assert worst29 <= FADE_BOUND, (worst29, faded29, before29, blurred29)
        assert mode(doc29) == 'lab'
        call('undo', document_id=doc29)
        assert patch_codes(doc29) == before29, 'one undo restores the pre-blur codes'
        call('apply_filter', document_id=doc29, layer=0, filter='blur', sigma=2)
        darken29 = call('fade', fail=True, document_id=doc29, opacity=0.5, mode='Darken')
        assert darken29.startswith(LAB_LIMITATION_HEAD), darken29
        hue_args = {'filter': 'hue_saturation', 'params': {'hue': 30, 'saturation': 0.2}}
        call('save_action', name='lab-blur-hue-547', overwrite=True, steps=[
            {'tool': 'apply_filter', 'arguments': {'filter': 'blur', 'sigma': 2}},
            {'tool': 'apply_filter', 'arguments': hue_args},
        ])
        played29, direct29 = fresh_patches(), fresh_patches()
        call('run_action', document_id=played29, name='lab-blur-hue-547')
        call('apply_filter', document_id=direct29, layer=0, filter='blur', sigma=2)
        call('apply_filter', document_id=direct29, layer=0, **hue_args)
        assert patch_codes(played29) == patch_codes(direct29), 'the Action replays the two steps'
        assert patch_codes(played29) != before29, 'the Action changed nothing'
        assert mode(played29) == 'lab'
        call('delete_action', name='lab-blur-hue-547')
        receipts['l29'] = {'worst_fade': worst29, 'darken': darken29}
        print('PASS l29: Fade lerps the codes and undoes with its step; an Action replays a filter '
              'and Hue/Saturation', flush=True)

        # L30: the colour adjustments.
        doc30 = fresh_patches()
        before30 = patch_codes(doc30)
        desat30, _ = pixel_step(doc30, lambda: patch_codes(doc30), 'apply_filter', layer=0,
                                filter='hue_saturation', params={'saturation': -1})
        for d, b in zip(desat30, before30):
            assert d[0] == b[0] and d[1] == NEUTRAL16 and d[2] == NEUTRAL16, (d, b)
        call('undo', document_id=doc30)
        levels30, _ = pixel_step(doc30, lambda: patch_codes(doc30), 'apply_filter', layer=0,
                                 filter='levels', g={'gamma': 1.4})
        assert all(l[0] == b[0] and l[2] == b[2] for l, b in zip(levels30, before30)), levels30
        assert any(l[1] != b[1] for l, b in zip(levels30, before30)), levels30
        call('undo', document_id=doc30)
        half30, _ = pixel_step(doc30, lambda: patch_codes(doc30), 'apply_filter', layer=0,
                               filter='adjust', saturation=-0.5)
        worst30 = 0.0
        for h, b in zip(half30, before30):
            assert h[0] == b[0], ('saturation keeps L', h, b)
            got, was = lab_of_code(h), lab_of_code(b)
            for c in (1, 2):
                worst30 = max(worst30, abs(got[c] - 0.5 * was[c]))
        assert worst30 <= HALF_BOUND, (worst30, half30, before30)
        call('undo', document_id=doc30)
        hue30, _ = pixel_step(doc30, lambda: patch_codes(doc30), 'add_adjustment_layer',
                              op='hue_saturation', params={'hue': 180}, name='Hue')
        sh30 = call('add_adjustment_layer', fail=True, document_id=doc30, op='shadows_highlights',
                    params={'shadows': {'amount': 0.5}})
        assert sh30 == PIXELS_ONLY_REFUSAL.format('Shadows/Highlights'), sh30
        receipts['l30'] = {'desaturate': desat30, 'levels_g': levels30, 'half_worst': worst30,
                           'hue_layer': hue30, 'shadows_highlights': sh30}
        print('PASS l30: Desaturate keeps L, Levels g moves a alone, saturation halves a* and b*, '
              'a Hue/Saturation layer composites; Shadows/Highlights refuses as a layer', flush=True)

        # L31: Auto Tone, Auto Contrast and Equalize.
        # Without black and white, whose L* already spans the range (no stretch to make).
        mid31 = [p for p in PATCHES if p not in ((0, 0, 0), (255, 255, 255))]
        doc31 = fresh_patches(mid31)
        points31 = [(i, 0) for i in range(len(mid31))]
        before31 = codes_at(doc31, points31)
        auto31 = {}
        for tool in ('auto_tone', 'auto_contrast', 'equalize'):
            after, _ = pixel_step(doc31, lambda: codes_at(doc31, points31), tool, layer=0)
            assert all(a[1:] == b[1:] for a, b in zip(after, before31)), (tool, after, before31)
            assert any(a[0] != b[0] for a, b in zip(after, before31)), (tool, after)
            auto31[tool] = [p[0] for p in after]
            call('undo', document_id=doc31)
            assert codes_at(doc31, points31) == before31, tool
        receipts['l31'] = auto31
        print('PASS l31: Auto Tone, Auto Contrast and Equalize move Lightness alone, one step each',
              flush=True)

        # ---- L32–L35: the 2d half (phase 2d) ---------------------------------------------------
        # L32: the scalar histogram of Lightness, a and b.
        doc32 = fresh_patches()
        scalar32 = {}
        for name in ('lightness', 'a', 'b'):
            report = call('histogram', document_id=doc32, source='scalar', target=name, layer=0)
            assert report['target'] == name, (name, report.get('target'))
            assert report['lab_value_rule'] == LAB_VALUE_RULE, report.get('lab_value_rule')
            weight = sum(sum(row) for row in report['bin_weight_units'])
            assert weight > 0, (name, weight)
            scalar32[name] = weight
        red32 = call('histogram', fail=True, document_id=doc32, source='scalar', target='red',
                     layer=0)
        assert red32 == SCALAR_TARGET_REFUSAL, red32
        receipts['l32'] = {'weights': scalar32, 'red': red32}
        print('PASS l32: histogram source scalar reads Lightness, a and b by name; red refuses',
              flush=True)

        # L33: filters on an alpha channel and a layer mask.
        def plane_step(doc, probe, **arguments):
            """`apply_filter` changes `probe()` in ONE undo step; returns the change."""
            before, count = probe(), undo_count(doc)
            call('apply_filter', document_id=doc, layer=0, **arguments)
            after = probe()
            assert after != before, (arguments, 'changed nothing')
            assert undo_count(doc) == count + 1, (arguments, 'not one undo step')
            call('undo', document_id=doc)
            assert probe() == before, (arguments, 'undo')
            call('redo', document_id=doc)
            assert probe() == after, (arguments, 'redo')
            return [i for i, (a, b) in enumerate(zip(after, before)) if a != b]

        doc33 = fresh_patches()
        call('add_channel', document_id=doc33, name='Matte')
        call('select_rect', document_id=doc33, x=2, y=0, width=3, height=1)
        call('fill_selection', document_id=doc33, target='channel:Matte', color='#ffffff')
        call('deselect', document_id=doc33)
        matte33 = lambda: [p[0] for p in channel_pixels(doc33, channel='Matte')]  # noqa: E731
        moved33 = {'channel blur': plane_step(doc33, matte33, filter='blur', sigma=2,
                                              target='channel:Matte')}
        assert 1 in moved33['channel blur'] and 5 in moved33['channel blur'], moved33
        mdoc33 = fresh_patches()
        call('add_layer_mask', document_id=mdoc33, layer=0, kind='hide_all')
        call('select_rect', document_id=mdoc33, x=2, y=0, width=3, height=1)
        call('fill_selection', document_id=mdoc33, layer=0, target='mask', color='#ffffff')
        call('deselect', document_id=mdoc33)
        mask33 = lambda: [p[0] for p in channel_pixels(mdoc33, channel='mask', layer=0)]  # noqa: E731
        moved33['mask blur'] = plane_step(mdoc33, mask33, filter='blur', sigma=2, target='mask')
        assert 1 in moved33['mask blur'] and 5 in moved33['mask blur'], moved33
        moved33['mask emboss'] = plane_step(mdoc33, mask33, filter='emboss', target='mask')
        lens33 = call('apply_filter', fail=True, document_id=mdoc33, layer=0,
                      filter='lens_correction', distortion=20, target='mask')
        assert lens33 == UNAVAILABLE_REFUSAL.format('Lens Correction'), lens33
        assert mode(doc33) == 'lab' and mode(mdoc33) == 'lab'
        receipts['l33'] = {'moved': moved33, 'lens_correction': lens33}
        print('PASS l33: blur on an alpha channel, blur and emboss on a mask, one step each; '
              'Lens Correction refuses', flush=True)

        # L34: Color Samplers.
        doc34 = fresh_patches()
        set34 = call('set_color_samplers', document_id=doc34,
                     samplers=[{'x': 0, 'y': 0}, {'x': 3, 'y': 0}])
        assert set34['count'] == 2, set34
        for row in set34['samplers']:
            pixel = call('sample_pixel', document_id=doc34, x=row['x'], y=row['y'])
            assert row['lab'] == pixel['lab'], (row, pixel['lab'])
        got34 = call('get_color_samplers', document_id=doc34)
        assert got34['samplers'] == set34['samplers'], (got34, set34)
        cleared34 = call('set_color_samplers', document_id=doc34, samplers=[])
        assert cleared34['count'] == 0, cleared34
        receipts['l34'] = {'samplers': set34['samplers']}
        print('PASS l34: Color Samplers read sample_pixel\'s L*a*b*; [] clears', flush=True)

        # L35: RGB → Lab carries an adjustment layer.
        def rgb_patches():
            fresh_count[0] += 1
            source = folder / f'patches16-2d-{fresh_count[0]}.png'
            png(source, len(PATCHES), 1, 16, 2, [v * 257 for p in PATCHES for v in p])
            return call('open_document', path=str(source), precision='preserve')['document']['id']

        doc35 = rgb_patches()
        call('add_adjustment_layer', document_id=doc35, op='hue_saturation', params={'hue': 30},
             name='Hue')
        count35 = undo_count(doc35)
        reply35 = call('convert_mode', document_id=doc35, mode='lab')
        assert reply35['color_mode'] == 'lab', reply35
        layers35 = call('get_document', document_id=doc35)['layers']
        assert len(layers35) == 2, layers35
        assert sum(1 for l in layers35 if l.get('is_adjustment')) == 1, layers35
        assert undo_count(doc35) == count35 + 1, 'the conversion is not one undo step'
        call('undo', document_id=doc35)
        assert mode(doc35) == 'rgb'
        call('redo', document_id=doc35)
        assert mode(doc35) == 'lab'
        cdoc35 = rgb_patches()
        call('add_adjustment_layer', document_id=cdoc35, op='levels',
             params={'g': {'gamma': 1.4}}, name='Levels')
        levels35 = call('get_document', document_id=cdoc35)['layers'][1].get('adjustment')
        reply35c = call('convert_mode', document_id=cdoc35, mode='lab')
        assert reply35c['color_mode'] == 'lab', reply35c
        carried35 = call('get_document', document_id=cdoc35)['layers'][1].get('adjustment')
        assert carried35 == levels35 and levels35 is not None, (carried35, levels35)
        receipts['l35'] = {'carried': [l.get('name') for l in layers35], 'levels': carried35}
        print('PASS l35: RGB → Lab carries a Hue/Saturation layer in one step and a channel Levels '
              'layer unchanged', flush=True)

        # ---- L36–L38: retouch, effects and the planes (phase 2e) ------------------------------
        def ab_distance(c):
            return (abs(c[1] - NEUTRAL16), abs(c[2] - NEUTRAL16))

        # L36: the retouch tools.
        cover36 = dict(points=[[0, 0], [len(PATCHES) - 1, 0]], size=12, hardness=100)
        doc36 = fresh_patches()
        before36 = patch_codes(doc36)
        dodged36, _ = pixel_step(doc36, lambda: patch_codes(doc36), 'dodge_burn', layer=0,
                                 **cover36)
        assert all(d[1:3] == b[1:3] for d, b in zip(dodged36, before36)), (dodged36, before36)
        assert all(d[0] >= b[0] for d, b in zip(dodged36, before36)), (dodged36, before36)
        assert any(d[0] > b[0] for d, b in zip(dodged36, before36)), (dodged36, before36)
        call('undo', document_id=doc36)
        sponged36, _ = pixel_step(doc36, lambda: patch_codes(doc36), 'sponge_stroke', layer=0,
                                  mode='desaturate', **cover36)
        for d, b in zip(sponged36, before36):
            assert d[0] == b[0], ('the Sponge keeps L', d, b)
            assert all(x <= y for x, y in zip(ab_distance(d), ab_distance(b))), (d, b)
        assert any(ab_distance(d) != ab_distance(b) for d, b in zip(sponged36, before36))
        call('undo', document_id=doc36)
        blurred36, _ = pixel_step(doc36, lambda: patch_codes(doc36), 'blur_stroke', layer=0,
                                  **cover36)
        call('undo', document_id=doc36)
        tile_count = [0]

        def fresh_tiles():
            """A new 16-bit Lab document of L19's patches tiled over `TILE`."""
            tile_count[0] += 1
            w, h = TILE
            source = folder / f'tiles16-2e-{tile_count[0]}.png'
            hx, hy, hw, hh = HOLE

            def colour(x, y):
                if hx <= x < hx + hw and hy <= y < hy + hh:
                    return (255, 0, 255)
                return PATCHES[(x // 6 + y // 4) % len(PATCHES)]

            samples = [v * 257 for y in range(h) for x in range(w) for v in colour(x, y)]
            png(source, w, h, 16, 2, samples)
            tdoc = call('open_document', path=str(source), precision='preserve')['document']['id']
            reply = call('convert_mode', document_id=tdoc, mode='lab')
            assert reply['color_mode'] == 'lab' and reply['depth'] == 16, reply
            return tdoc

        cdoc36 = fresh_tiles()
        hole36 = [(x, y) for y in range(HOLE[1], HOLE[1] + HOLE[3])
                  for x in range(HOLE[0], HOLE[0] + HOLE[2])]
        call('select_rect', document_id=cdoc36, x=HOLE[0], y=HOLE[1], width=HOLE[2],
             height=HOLE[3])
        filled36, _ = pixel_step(cdoc36, lambda: codes_at(cdoc36, hole36), 'content_aware_fill',
                                 layer=0, seed=7)
        darken36 = call('clone_stamp', fail=True, document_id=doc36, layer=0, source_x=1,
                        source_y=0, blend_mode='Darken', **cover36)
        assert darken36.startswith(LAB_LIMITATION_HEAD), darken36
        receipts['l36'] = {'dodge': dodged36, 'sponge': sponged36, 'blur': blurred36,
                           'darken': darken36}
        print('PASS l36: Dodge raises L*, the Sponge desaturates a* and b*, Blur and Content-Aware '
              'Fill run, one step each; a Darken clone refuses', flush=True)

        # L37: layer effects.
        odoc37, fdoc37 = new_lab(16), new_lab(16)
        call('set_layer_style', document_id=odoc37, layer=0,
             style={'effects': [{'type': 'color_overlay', 'color': '#ff0000'}]})
        call('fill_selection', document_id=fdoc37, color='#ff0000')
        overlay37, fill37 = lab_at(odoc37, 4, 2), lab_at(fdoc37, 4, 2)
        assert abs(overlay37[0] - fill37[0]) <= OVERLAY_BOUND[0], (overlay37, fill37)
        assert max(abs(overlay37[1] - fill37[1]), abs(overlay37[2] - fill37[2])) \
            <= OVERLAY_BOUND[1], (overlay37, fill37)
        sdoc37 = rgb_patches()
        call('set_layer_style', document_id=sdoc37, layer=0,
             style={'effects': [{'type': 'drop_shadow', 'distance': 2, 'size': 2}]})
        count37 = undo_count(sdoc37)
        reply37 = call('convert_mode', document_id=sdoc37, mode='lab')
        assert reply37['color_mode'] == 'lab', reply37
        assert undo_count(sdoc37) == count37 + 1, 'the conversion is not one undo step'
        style37 = call('get_document', document_id=sdoc37)['layers'][0].get('style')
        assert style37 is not None, 'the drop shadow was not carried'
        receipts['l37'] = {'overlay': overlay37, 'fill': fill37, 'style': style37}
        print('PASS l37: a red Color Overlay composites as red Fill; RGB → Lab carries a drop '
              'shadow in one step', flush=True)

        # L38: the Lightness, a, b and alpha planes.
        doc38 = fresh_patches()
        before38 = patch_codes(doc38)
        call('fill_selection', document_id=doc38, target='a', color='#000000')
        filled38 = patch_codes(doc38)
        for f, b in zip(filled38, before38):
            assert f[1] == 0 and f[0] == b[0] and f[2] == b[2], (f, b)
        call('undo', document_id=doc38)
        assert patch_codes(doc38) == before38, 'fill_selection target a undo'
        blurred38, _ = pixel_step(doc38, lambda: patch_codes(doc38), 'apply_filter', layer=0,
                                  filter='blur', sigma=2, target='lightness')
        assert all(x[1:3] == b[1:3] for x, b in zip(blurred38, before38)), (blurred38, before38)
        assert any(x[0] != b[0] for x, b in zip(blurred38, before38)), blurred38
        call('undo', document_id=doc38)
        call('apply_filter', document_id=doc38, layer=0, filter='emboss', target='lightness')
        embossed38 = patch_codes(doc38)
        assert any(x[0] != b[0] for x, b in zip(embossed38, before38)), embossed38
        alpha38 = call('histogram', document_id=doc38, source='scalar', target='alpha', layer=0)
        assert sum(sum(row) for row in alpha38['bin_weight_units']) > 0, alpha38
        red38 = call('brush_stroke', fail=True, document_id=doc38, color='#ff0000', target='red',
                     **cover36)
        assert red38 == PLANE_TARGET_REFUSAL, red38
        assert mode(doc38) == 'lab'
        receipts['l38'] = {'fill_a': filled38, 'blur_lightness': blurred38,
                           'emboss_lightness': embossed38, 'red': red38}
        print('PASS l38: fill and filters run on a, Lightness; the alpha histogram answers; red '
              'refuses with the Lab plane names', flush=True)

        # ---- L39–L42: phase 2f ---------------------------------------------------------------
        # L39: the luma filters in passes.
        edge_count = [0]

        def edge_gray():
            """A new 16-bit Lab document of 16 × 8 grays: an L* edge at x = 8, a ±4 texture."""
            edge_count[0] += 1
            w, h = 16, 8
            source = folder / f'edge16-2f-{edge_count[0]}.png'
            samples = []
            for y in range(h):
                for x in range(w):
                    v = (40 if x < 8 else 210) + (x * 7 + y * 13) % 9 - 4
                    samples += [v * 257] * 3
            png(source, w, h, 16, 2, samples)
            edoc = call('open_document', path=str(source), precision='preserve')['document']['id']
            reply = call('convert_mode', document_id=edoc, mode='lab')
            assert reply['color_mode'] == 'lab' and reply['depth'] == 16, reply
            return edoc

        row39 = [(x, 3) for x in range(16)]
        doc39 = edge_gray()
        before39 = codes_at(doc39, row39)
        filters39 = (('emboss', {}), ('edge_detect', {}), ('lens_blur', {}),
                     ('reduce_noise', {'radius': 2, 'luminance': 0.25, 'detail': 0}),
                     ('smart_sharpen', {}))
        ran39 = {}
        for filt, extra in filters39:
            after, _ = pixel_step(doc39, lambda: codes_at(doc39, row39), 'apply_filter', layer=0,
                                  filter=filt, **extra)
            worst = max(max(abs(lab_of_code(c)[1]), abs(lab_of_code(c)[2])) for c in after)
            assert worst <= AB_BOUND_2F, (filt, worst, after)
            assert any(a[0] != b[0] for a, b in zip(after, before39)), (filt, 'no L* moved')
            ran39[filt] = {'worst_ab': worst,
                           'l_moved': sum(1 for a, b in zip(after, before39) if a[0] != b[0])}
            call('undo', document_id=doc39)
            assert codes_at(doc39, row39) == before39, (filt, 'undo')
        receipts['l39'] = ran39
        print('PASS l39: emboss, edge detect, lens blur, reduce noise and smart sharpen move L* '
              'and keep a* = b* = 0, one step each', flush=True)

        # L40: Offset's background fill on the a plane.
        doc40 = fresh_patches()
        before40 = patch_codes(doc40)
        offset40, _ = pixel_step(doc40, lambda: patch_codes(doc40), 'apply_filter', layer=0,
                                 filter='offset', horizontal=2, vertical=0, target='a',
                                 undefined_areas='background', background_color='#ffffff')
        for i, (got, was) in enumerate(zip(offset40, before40)):
            assert got[0] == was[0] and got[2] == was[2], (i, got, was)
        vacated40 = [lab_of_code(offset40[i])[1] for i in (0, 1)]
        assert all(abs(a) <= AB_BOUND_2F for a in vacated40), (vacated40, offset40)
        receipts['l40'] = {'vacated_a': vacated40, 'codes': offset40}
        print('PASS l40: Offset with the background fills the a plane with white\'s a* = 0',
              flush=True)

        # L41: Blend If on a*.
        doc41 = fresh_patches()
        call('duplicate_layer', document_id=doc41, index=0)
        call('fill_selection', document_id=doc41, layer=0, color='#808080')
        top41 = patch_codes(doc41)
        _, _, full41, _ = render(doc41)
        call('set_layer_style', document_id=doc41, layer=1,
             style={'blend_if': {'channel': 'green', 'this_layer': [128, 128, 255, 255]}})
        _, _, blended41, _ = render(doc41)
        shown41 = hidden41 = 0
        for i, code in enumerate(top41):
            got = blended41[i]
            if code[1] <= NEUTRAL16 - 257:
                hidden41 += 1
                assert all(abs(got[c] - 128) <= RENDER_BOUND8 for c in range(3)), (i, got, code)
            elif code[1] >= NEUTRAL16 + 257:
                shown41 += 1
                assert all(abs(got[c] - full41[i][c]) <= 1 for c in range(3)), \
                    (i, got, full41[i], code)
        assert shown41 > 0 and hidden41 > 0, (shown41, hidden41, top41)
        gray41 = call('set_layer_style', fail=True, document_id=doc41, layer=1,
                      style={'blend_if': {'channel': 'gray', 'this_layer': [0, 40, 255, 255]}})
        assert gray41 == LAB_BLEND_IF_GRAY, gray41
        receipts['l41'] = {'shown': shown41, 'hidden': hidden41, 'gray': gray41}
        print('PASS l41: Blend If on a* hides the patches with a* < 0; Gray refuses', flush=True)

        # L42: the channel Levels carry and Levels Auto on Lightness.
        doc42 = fresh_patches()
        call('add_adjustment_layer', document_id=doc42, op='levels',
             params={'g': {'gamma': 1.4}}, name='Levels')
        levels42 = call('get_document', document_id=doc42)['layers'][1].get('adjustment')
        back42 = call('convert_mode', document_id=doc42, mode='rgb', rgb_space='srgb')
        assert back42['color_mode'] == 'rgb', back42
        carried42 = call('get_document', document_id=doc42)['layers'][1].get('adjustment')
        assert carried42 == levels42 and levels42 is not None, (carried42, levels42)
        adoc42 = fresh_patches(AUTO_PATCHES)
        call('add_adjustment_layer', document_id=adoc42, op='levels', auto={'mode': 'tone'},
             name='Auto')
        auto42 = call('get_document', document_id=adoc42)['layers'][1].get('adjustment')
        stages42 = [k for k in sorted(auto42['params']) if k in ('r', 'g', 'b')] if auto42 else []
        assert stages42 == ['r'], auto42
        params42 = auto42['params']
        assert params42.get('black', 0) == 0 and params42.get('white', 1) == 1 \
            and params42.get('gamma', 1) == 1, auto42
        r42 = params42['r']
        assert r42.get('black', 0) != 0 or r42.get('white', 1) != 1 \
            or r42.get('gamma', 1) != 1, auto42
        color42 = call('add_adjustment_layer', fail=True, document_id=adoc42, op='levels',
                       auto={'mode': 'color'}, name='Auto Color')
        assert color42 == UNAVAILABLE_REFUSAL.format('Auto Color'), color42
        receipts['l42'] = {'levels': carried42, 'auto': auto42, 'color': color42}
        print('PASS l42: Lab → RGB carries a channel Levels layer; Levels Auto Tone sets '
              'Lightness alone; Auto Color refuses', flush=True)

        # ---- L43–L44: phase 2f, second part ---------------------------------------------------
        # L43: Pattern Overlay and Pattern strokes.
        tile43 = {'name': 'T', 'width': 2, 'height': 2,
                  'rgba': base64.b64encode(bytes(v for t in TILE43 for v in (*t, 255))).decode()}
        points43 = [(0, 0), (1, 0), (0, 1), (1, 1)]
        doc43 = new_lab(16, 8, 4)
        overlay43, _ = pixel_step(
            doc43, lambda: [lab_at(doc43, x, y) for x, y in points43], 'set_layer_style',
            layer=0, style={'effects': [{'type': 'pattern_overlay', 'pattern': tile43,
                                         'scale': 1}]})
        for (x, y), got in zip(points43, overlay43):
            within(got, TILE43[(y % 2) * 2 + x % 2], 8, ('l43 overlay', x, y))
        sdoc43 = new_lab(16, 8, 4)
        row43 = [(x, 0) for x in range(8)]
        stroke43, _ = pixel_step(
            sdoc43, lambda: [lab_at(sdoc43, x, y) for x, y in row43], 'set_layer_style',
            layer=0, style={'effects': [{'type': 'stroke', 'size': 2, 'position': 'inside',
                                         'fill_type': 'pattern',
                                         'pattern_fill': {'pattern': tile43}}]})
        cdoc43 = rgb_patches()
        call('set_layer_style', document_id=cdoc43, layer=0,
             style={'effects': [{'type': 'pattern_overlay', 'pattern': tile43, 'scale': 1}]})
        style43 = call('get_document', document_id=cdoc43)['layers'][0].get('style')
        count43 = undo_count(cdoc43)
        reply43 = call('convert_mode', document_id=cdoc43, mode='lab')
        assert reply43['color_mode'] == 'lab', reply43
        assert undo_count(cdoc43) == count43 + 1, 'the conversion is not one undo step'
        carried43 = call('get_document', document_id=cdoc43)['layers'][0].get('style')
        assert carried43 == style43 and style43 is not None, (carried43, style43)
        receipts['l43'] = {'overlay': overlay43, 'stroke': stroke43, 'style': carried43}
        print('PASS l43: a Pattern Overlay reads its sRGB texels as L*a*b*, a Pattern stroke '
              'runs, one step each; RGB → Lab carries a pattern style unchanged', flush=True)

        # L44: Edit ▸ Stroke….
        doc44 = new_lab(16, 16, 8)
        call('select_rect', document_id=doc44, x=2, y=0, width=12, height=7)
        band44, _ = pixel_step(
            doc44, lambda: [lab_at(doc44, 2, 3), lab_at(doc44, 8, 3)], 'stroke_selection',
            width=1, location='inside', color='#ff0000')
        white44 = lab_at(new_lab(16, 16, 8), 8, 3)
        assert band44[1] == white44, (band44, white44)
        assert abs(band44[0][0] - fill37[0]) <= OVERLAY_BOUND[0], (band44, fill37)
        assert max(abs(band44[0][1] - fill37[1]), abs(band44[0][2] - fill37[2])) \
            <= OVERLAY_BOUND[1], (band44, fill37)
        edoc44 = new_lab(16, 16, 8)
        call('new_layer', document_id=edoc44, name='Rect')
        call('select_rect', document_id=edoc44, x=4, y=2, width=6, height=4)
        call('fill_selection', document_id=edoc44, layer=1, color='#0000ff')
        call('deselect', document_id=edoc44)
        edge44, _ = pixel_step(
            edoc44, lambda: [lab_at(edoc44, 6, 1)], 'stroke_selection', layer=1, width=1,
            location='outside', color='#ff0000')
        assert abs(edge44[0][0] - fill37[0]) <= OVERLAY_BOUND[0], (edge44, fill37)
        assert max(abs(edge44[0][1] - fill37[1]), abs(edge44[0][2] - fill37[2])) \
            <= OVERLAY_BOUND[1], (edge44, fill37)
        darken44 = call('stroke_selection', fail=True, document_id=edoc44, layer=1, width=1,
                        location='outside', color='#ff0000', blend_mode='Darken')
        assert darken44.startswith(LAB_LIMITATION_HEAD), darken44
        receipts['l44'] = {'band': band44, 'edge': edge44, 'darken': darken44}
        print('PASS l44: Stroke… of a selection and of a layer edge paint red\'s L*a*b*, one step '
              'each; a Darken stroke refuses', flush=True)

        # ---- L45–L47: phase 2f, third part ----------------------------------------------------
        def converted(name, w, h, depth, pixel):
            """A `w` × `h` sRGB PNG of `pixel(x, y)` at `depth`, opened and converted to lab."""
            fresh_count[0] += 1
            source = folder / f'{name}-{depth}-{fresh_count[0]}.png'
            scale = 257 if depth == 16 else 1
            png(source, w, h, depth, 2,
                [v * scale for y in range(h) for x in range(w) for v in pixel(x, y)])
            extra = {'precision': 'preserve'} if depth == 16 else {}
            doc = call('open_document', path=str(source), **extra)['document']['id']
            reply = call('convert_mode', document_id=doc, mode='lab')
            assert reply['color_mode'] == 'lab' and reply['depth'] == depth, reply
            return doc

        # L45: Remove, Content-Aware and the model.
        def field45(x, y):
            if 23 <= y <= 25 and 22 <= x <= 42:
                return [255, 0, 255]
            step = 6 if (x // 4 + y // 4) % 2 else -6
            return [v + step for v in FIELD45]

        a45, b45 = (20, 24), (44, 24)

        def distance45(x, y):
            px, py = x + 0.5, y + 0.5
            dx = max(a45[0] - px, 0, px - b45[0])
            return (dx * dx + (py - a45[1]) ** 2) ** 0.5

        every45 = [(x, y) for y in range(48) for x in range(64)]
        l45 = {}
        for depth in (16, 8):
            for label, extra in (('content_aware', {'mode': 'content_aware'}), ('model', {})):
                if label == 'model' and not model_ready:
                    continue
                doc45 = converted('field45', 64, 48, depth, field45)
                before45 = codes_at(doc45, every45)
                count45 = undo_count(doc45)
                reply45 = call('remove_stroke', document_id=doc45, layer=0,
                               points=[list(a45), list(b45)], size=8, seed=7, **extra)
                assert undo_count(doc45) == count45 + 1, (label, depth, 'not one undo step')
                assert mode(doc45) == 'lab', (label, depth)
                after45 = codes_at(doc45, every45)
                near45 = sum(1 for (x, y), p, q in zip(every45, before45, after45)
                             if distance45(x, y) <= 2 and p != q)
                far45 = [(x, y) for (x, y), p, q in zip(every45, before45, after45)
                         if distance45(x, y) > 5 and p != q]
                assert near45 > 0, (label, depth, 'changed nothing near the stroke')
                assert not far45, (label, depth, far45[:8])
                row = {'reply': reply45, 'changed_near': near45}
                if label == 'model':
                    got45 = lab_at(doc45, 32, 24)
                    want45 = srgb_to_lab(FIELD45)
                    assert all(abs(got45[c] - want45[c]) <= REMOVE_FIELD_BOUND for c in range(3)), \
                        (depth, got45, want45)
                    row['lab'] = got45
                l45[f'{label}_{depth}'] = row
        receipts['l45'] = l45
        print('PASS l45: Remove runs on Lab at 16 and 8 bits (content_aware' +
              (' and the model' if model_ready else '; the model SKIPPED') +
              '), one step each, nothing far from the stroke moves', flush=True)

        # L46: the Art History Brush.
        doc46 = new_lab(8, 32, 24)
        state46 = call('list_history', document_id=doc46)['current_state_id']
        call('fill_selection', document_id=doc46, color='#3366cc')
        fill46 = lab_at(doc46, 16, 12)
        probe46 = [(x, y) for y in range(6, 19, 2) for x in range(8, 25, 2)]
        stroke46 = dict(layer=0, points=[[16, 12]], size=5, style='tight_long', area=30,
                        tolerance=0, seed=4, state_id=state46)
        count46 = undo_count(doc46)
        call('art_history_brush_stroke', document_id=doc46, **stroke46)
        assert undo_count(doc46) == count46 + 1, 'the Art History Brush is not one undo step'
        assert mode(doc46) == 'lab'
        lightness46 = [lab_at(doc46, x, y)[0] for x, y in probe46]
        assert max(lightness46) >= fill46[0] + 10, (fill46, lightness46)
        darken46 = call('art_history_brush_stroke', fail=True, document_id=doc46,
                        blend_mode='Darken', **stroke46)
        assert darken46 == ART_DARKEN, darken46
        count46 = undo_count(doc46)
        hue46 = call('art_history_brush_stroke', document_id=doc46, blend_mode='Hue', **stroke46)
        assert undo_count(doc46) == count46 + 1, 'the Hue stroke is not one undo step'
        sixteen46 = call('art_history_brush_stroke', fail=True, document_id=new_lab(16, 32, 24),
                         layer=0, points=[[16, 12]], size=5)
        assert sixteen46 == ART16_REFUSAL, sixteen46
        receipts['l46'] = {'fill': fill46, 'lightness': max(lightness46), 'darken': darken46,
                           'hue': hue46, 'sixteen': sixteen46}
        print('PASS l46: the Art History Brush paints an 8-bit Lab document from its New state, '
              'one step; Hue paints, Darken and 16 bits refuse', flush=True)

        # L47: Red Eye.
        def eye47(x, y):
            return (220, 30, 30) if (x - 12) ** 2 + (y - 8) ** 2 <= 9 else (128, 128, 128)

        l47 = {}
        for depth in (16, 8):
            doc47 = converted('eye47', 24, 16, depth, eye47)
            before47, keep47 = lab_at(doc47, 12, 8), codes_at(doc47, [(7, 3)])
            count47 = undo_count(doc47)
            call('red_eye', document_id=doc47, x=6, y=2, width=12, height=12)
            assert undo_count(doc47) == count47 + 1, (depth, 'red_eye is not one undo step')
            assert mode(doc47) == 'lab'
            after47 = lab_at(doc47, 12, 8)
            assert before47[1] - after47[1] >= RED_EYE_A_DROP, (depth, before47, after47)
            assert after47[0] < before47[0], (depth, before47, after47)
            assert codes_at(doc47, [(7, 3)]) == keep47, depth
            corrected47 = codes_at(doc47, [(12, 8)])
            auto47 = converted('eye47', 24, 16, depth, eye47)
            count47 = undo_count(auto47)
            call('red_eye_auto', document_id=auto47, eyes=[{'x': 12, 'y': 8, 'radius': 2}])
            assert undo_count(auto47) == count47 + 1, (depth, 'red_eye_auto is not one undo step')
            assert codes_at(auto47, [(12, 8)]) == corrected47, depth
            l47[depth] = {'before': before47, 'after': after47, 'codes': corrected47}
        receipts['l47'] = l47
        print('PASS l47: Red Eye and Remove Red Eye correct the red pupil of a Lab document at 16 '
              'and 8 bits, one step each, and leave the gray alone', flush=True)

        # ---- L48–L50: phase 3 --------------------------------------------------------
        row48 = [(x, 0) for x in range(len(PATCHES))]

        def assert_arm(doc, name, opacity, what):
            """Each composite pixel of `doc` within a code of `lab_arm` (module doc)."""
            base = codes_at(doc, row48, source='layer', layer=0)
            top = codes_at(doc, row48, source='layer', layer=1)
            got = codes_at(doc, row48)
            worst = 0.0
            for b, s, g in zip(base, top, got):
                cb, cs = [v / 65535 for v in b[:3]], [v / 65535 for v in s[:3]]
                arm = lab_arm(name, cb, cs)
                want = [cb[k] + opacity * (arm[k] - cb[k]) for k in range(3)]
                worst = max(worst, max(abs(g[k] / 65535 - want[k]) * 255 for k in range(3)))
            assert worst <= BLEND_BOUND8, (what, name, worst)
            return worst

        l48 = {}
        for name in REARMED:
            doc48 = rgb_patches()
            call('new_layer', document_id=doc48, name='Top')
            call('fill_selection', document_id=doc48, layer=1, color=BLEND_TOP)
            call('set_layer_properties', document_id=doc48, index=1, blend_mode=name, opacity=0.6)
            count48 = undo_count(doc48)
            reply48 = call('convert_mode', document_id=doc48, mode='lab')
            assert reply48['color_mode'] == 'lab' and reply48['depth'] == 16, (name, reply48)
            assert undo_count(doc48) == count48 + 1, (name, 'the conversion is not one undo step')
            call('undo', document_id=doc48)
            assert mode(doc48) == 'rgb', name
            call('redo', document_id=doc48)
            assert mode(doc48) == 'lab', name
            l48[name] = assert_arm(doc48, name, 0.6, 'l48')
        receipts['l48'] = l48
        print('PASS l48: RGB → Lab carries Multiply, Screen, Linear Burn, Linear Dodge (Add), '
              'Vivid Light and Hard Mix layers, composited by their Lab arms, one step each',
              flush=True)

        l49 = {}
        call('set_layer_properties', document_id=doc48, index=1, opacity=1.0)
        for name in JOINED:
            count49 = undo_count(doc48)
            call('set_layer_properties', document_id=doc48, index=1, blend_mode=name)
            assert undo_count(doc48) == count49 + 1, (name, 'not one undo step')
            assert mode(doc48) == 'lab', name
            l49[name] = assert_arm(doc48, name, 1.0, 'l49')
        receipts['l49'] = l49
        print('PASS l49: Darker Color, Lighter Color, Hue, Saturation, Color and Luminosity are '
              'set in one undo step each and composite by their Lab arms', flush=True)

        darken50 = call('set_layer_properties', fail=True, document_id=doc48, index=1,
                        blend_mode='Darken')
        assert darken50.startswith(LAB_LIMITATION_HEAD) and darken50.endswith(LAB_LIMITATION_TAIL), \
            darken50
        listed50 = app.client.rpc('tools/list', {})['tools']
        # The sentence before each "until phase 3": a blend note if it names a mode list.
        stale50 = [tool['name'] for tool in listed50
                   for before in tool.get('description', '').split('until phase 3')[:-1]
                   if any(word in before.rsplit('. ', 1)[-1]
                          for word in ('blend mode', 'blend_mode', 'mode takes'))]
        assert not stale50, stale50
        receipts['l50'] = {'darken': darken50}
        print('PASS l50: Darken refuses with the Lab limitation; no blend note waits for phase 3',
              flush=True)

        # ---- L51–L54: phase 3a ------------------------------------------------------------
        def lab_depth(doc):
            reply = call('get_precision', document_id=doc)
            assert reply['color_mode'] == 'lab', reply
            return reply['bits_per_channel']

        l51 = {}
        for name, photometric, bits, big, formats, extra in [
                ('cielab8_mm', 8, 8, True, None, None),
                ('cielab16_ii', 8, 16, False, [1, 2, 2], None),
                ('icclab8', 9, 8, False, None, None),
                ('icclab16', 9, 16, True, None, None),
                ('cielab16_alpha', 8, 16, False, None, 2)]:
            rows51 = lab_tiff_samples(photometric, bits, extra is not None)
            path51 = folder / f'{name}.tif'
            write_lab_tiff(path51, photometric, bits, rows51, big=big, sample_format=formats,
                           extra=extra)
            doc51 = call('open_document', path=str(path51))['document']['id']
            assert mode(doc51) == 'lab', name
            assert lab_depth(doc51) == bits, name
            got51 = codes_at(doc51, LAB_POINTS, source='layer', layer=0)
            worst = 0
            for (x, y), got in zip(LAB_POINTS, got51):
                want = lab_tiff_codes(photometric, bits, rows51[y * LAB_TIFF_W + x])
                if len(want) == 3:
                    want.append(65535)
                worst = max(worst, max(abs(g - w) for g, w in zip(got, want)))
            assert worst <= (0 if bits == 8 else 1), (name, worst, got51)
            l51[name] = worst
        associated = folder / 'cielab8_associated.tif'
        write_lab_tiff(associated, 8, 8, lab_tiff_samples(8, 8, True), extra=1)
        opened51 = app.client.rpc('tools/call', {'name': 'open_document',
                                                 'arguments': {'path': str(associated)}})
        if not opened51.get('isError'):
            text51 = next(c['text'] for c in opened51['content'] if c['type'] == 'text')
            assoc_doc = json.loads(text51)['document']['id']
            assert mode(assoc_doc) != 'lab', 'an associated-alpha Lab TIFF opened as lab'
            l51['associated'] = mode(assoc_doc)
        else:
            l51['associated'] = 'refused'
        receipts['l51'] = l51
        print('PASS l51: CIELab and ICCLab TIFFs at 8 and 16 bits open as Lab Color documents '
              'with the published codes; an associated alpha does not', flush=True)

        l52 = {}
        rgb52 = folder / 'patches8-52.png'
        png(rgb52, len(PATCHES), 1, 8, 2, [v for p in PATCHES for v in p])
        sources52 = {8: call('open_document', path=str(rgb52))['document']['id'],
                     16: rgb_patches()}
        points52 = [(x, 0) for x in (0, 2, 4, 6, len(PATCHES) - 1)]
        for bits, doc52 in sources52.items():
            reply52 = call('convert_mode', document_id=doc52, mode='lab')
            assert reply52['color_mode'] == 'lab' and reply52['depth'] == bits, reply52
            path52 = folder / f'roundtrip{bits}.tif'
            call('save_copy', document_id=doc52, path=str(path52), format='tiff')
            back52 = call('open_document', path=str(path52))['document']['id']
            assert mode(back52) == 'lab' and lab_depth(back52) == bits, bits
            mine, theirs = codes_at(doc52, points52), codes_at(back52, points52)
            for m, t in zip(mine, theirs):
                for c in range(4):
                    bound = 1 if bits == 16 and c in (1, 2) else 0
                    assert abs(m[c] - t[c]) <= bound, (bits, c, m, t)
            l52[bits] = theirs
        receipts['l52'] = l52
        print('PASS l52: a Lab TIFF save reopens as a Lab Color document at 8 and 16 bits with '
              'its codes', flush=True)

        layers53 = TAKE2 / 'layers2'
        if not (layers53 / 'layers16.psd').is_file():
            print(f'SKIP l53: the Photoshop take {TAKE2} is absent', flush=True)
            receipts['l53'] = 'SKIP'
        else:
            report53 = call('inspect_psd', path=str(layers53 / 'layers16.psd'))
            assert report53['import_precision'] == 'lab' and report53['color_mode'] == 'Lab', \
                report53
            assert report53['layered_available'] is True, report53

            def open_layers(path):
                report = call('inspect_psd', path=str(path))
                assert report['import_precision'] == 'lab', (path, report)
                return call('open_document', path=str(path), psd_mode='layers',
                            psd_fingerprint=report['source_fingerprint'])['document']['id']

            # The composite first, closed after, so the layered open below is a fresh document.
            composite53 = call('inspect_psd', path=str(layers53 / 'layers16.psd'))
            flat53 = call('open_document', path=str(layers53 / 'layers16.psd'),
                          psd_mode='composite',
                          psd_fingerprint=composite53['source_fingerprint'])['document']['id']
            assert mode(flat53) == 'lab' and summary(flat53)['layer_count'] == 1, \
                summary(flat53)
            call('close_document', document_id=flat53, discard_changes=True)
            cells53 = [(c * 8 + 4, r * 8 + 4) for r in range(3) for c in range(4)]
            l53 = {}
            twins = {'layers8.psd': 'layers8.psd', 'layers16.psb': 'layers16.psd',
                     'layers8.tif': 'layers8.psd'}
            references = {}
            for name, twin in twins.items():
                if twin not in references:
                    ref = open_layers(layers53 / twin)
                    references[twin] = (ref, summary(ref)['layer_count'], codes_at(ref, cells53))
                doc53 = open_layers(layers53 / name)
                bits = 16 if '16' in name else 8
                assert mode(doc53) == 'lab' and lab_depth(doc53) == bits, name
                _, count, codes = references[twin]
                assert summary(doc53)['layer_count'] == count, (name, count)
                assert codes_at(doc53, cells53) == codes, name
                l53[name] = {'layers': count}
            styled53 = open_layers(TAKE2 / 'styled2' / 'styled8.psd')
            overlay53 = next(row for row in call('get_document', document_id=styled53)['layers']
                             if row.get('name') == 'Overlay')
            effects53 = (overlay53.get('style') or {}).get('effects') or []
            assert any(e.get('type') == 'color_overlay' for e in effects53), overlay53
            l53['overlay'] = effects53
            receipts['l53'] = l53
            print('PASS l53: Photoshop\'s Lab PSD, PSB and layered TIFF open as Lab Color '
                  'documents with their twins\' layers and codes', flush=True)

        # ---- L54–L57: phase 3b ------------------------------------------------------------
        lab54 = call('new_document', width=4, height=3, color_mode='lab', depth=16,
                     background='white')['document']['id']
        refusal54 = call('save_copy', fail=True, document_id=lab54,
                         path=str(folder / 'lab54c.psd'), format='psd')
        assert refusal54 == SAVE_REFUSAL, refusal54
        receipts['l54'] = refusal54
        print('PASS l54: save_copy psd refuses, naming Export as PSD', flush=True)

        def lab_scene(depth):
            doc = call('new_document', width=16, height=12, color_mode='lab', depth=depth,
                       background='#ffffff')['document']['id']
            call('set_layer_properties', document_id=doc, index=0, name='Base')

            def index(name):
                return next(l['index'] for l in call('get_document', document_id=doc)['layers']
                            if l['name'] == name)

            def rect_fill(layer, x, y, w, h, colour):
                call('select_rect', document_id=doc, x=x, y=y, width=w, height=h)
                call('fill_selection', document_id=doc, layer=index(layer), color=colour)
                call('deselect', document_id=doc)

            rect_fill('Base', 0, 0, 8, 12, '#c04020')
            rect_fill('Base', 8, 0, 8, 12, '#2060a0')
            for name, (x, y, w, h), colour in [('Top', (2, 2, 6, 4), '#373737'),
                                               ('Ranged', (6, 5, 8, 5), '#d7c8b9'),
                                               ('Masked', (1, 7, 10, 4), '#5aa046')]:
                call('new_layer', document_id=doc, name=name)
                rect_fill(name, x, y, w, h, colour)
            call('set_layer_style', document_id=doc, layer=index('Top'),
                 style={'effects': [{'type': 'color_overlay', 'color': '#40a0c8'}]})
            call('set_layer_style', document_id=doc, layer=index('Ranged'),
                 style={'blend_if': {'channel': 'red', 'this_layer': [0, 0, 255, 255],
                                     'underlying': [0, 0, 96, 160]}})
            call('select_rect', document_id=doc, x=3, y=7, width=6, height=4)
            call('add_layer_mask', document_id=doc, layer=index('Masked'), kind='from_selection')
            call('deselect', document_id=doc)
            call('group_layers', document_id=doc, layers=[index('Masked')], name='Group')
            call('add_adjustment_layer', document_id=doc, op='levels', name='Levels',
                 position='top', params={'black': 10 / 255, 'white': 240 / 255, 'gamma': 1.2})
            return doc

        def kinds(doc):
            return [(l['name'], l.get('kind'), bool(l.get('is_adjustment')))
                    for l in call('get_document', document_id=doc)['layers']]

        def channel_pixels(doc, channel):
            result = app.client.rpc('tools/call', {'name': 'render', 'arguments': {
                'document_id': doc, 'channel': channel}})
            assert not result.get('isError'), result
            data = next(c['data'] for c in result['content'] if c['type'] == 'image')
            return png_pixels(base64.b64decode(data))[2]

        def same_channels(source, back, bound, what):
            worst = 0
            for channel in ('lightness', 'a', 'b'):
                for p, q in zip(channel_pixels(source, channel), channel_pixels(back, channel)):
                    worst = max(worst, abs(p[0] - q[0]))
            assert worst <= bound, (what, worst)
            return worst

        def reopen_layers(path):
            report = call('inspect_psd', path=str(path))
            assert report['import_precision'] == 'lab', (path, report)
            return call('open_document', path=str(path), psd_mode='layers',
                        psd_fingerprint=report['source_fingerprint'])['document']['id']

        def export_cases(depth, cases, bound, tag):
            source = lab_scene(depth)
            report = call('inspect_psd_export', document_id=source, format='psd', mode='layers')
            assert report['available'] is True and report['depth'] == depth \
                and report['color_mode'] == 'lab', report
            want = kinds(source)
            assert any(n == 'Levels' and a for n, _, a in want), want
            out = {}
            for container, mode_name in cases:
                suffix = {'psd': 'psd', 'psb': 'psb', 'tiff': 'tif'}[container]
                path = folder / f'{tag}_{mode_name}.{suffix}'
                reply = call('export_psd', document_id=source, path=str(path), format=container,
                             mode=mode_name)
                assert reply['bit_depth'] == depth, reply
                data = path.read_bytes()
                if container == 'tiff':
                    facts = tiff_facts(data)
                    assert facts['photometric'] == 8 and facts['samples'] == 3 \
                        and facts['bits'] == [depth] * 3 and not facts['icc'] \
                        and not facts['extra'], (path.name, facts)
                else:
                    facts = psd_facts(data)
                    assert (facts['mode'], facts['depth'], facts['channels']) == (9, depth, 3) \
                        and facts['count'] > 0 and 1039 not in facts['resources'], \
                        (path.name, facts)
                back = reopen_layers(path)
                assert mode(back) == 'lab' and lab_depth(back) == depth, path.name
                got = kinds(back)
                if mode_name == 'layers':
                    assert [n for n, _, _ in got] == [n for n, _, _ in want], (got, want)
                    for (n, k, a), (m, l, b) in zip(want, got):
                        if n == 'Levels':
                            assert a and not b, (n, a, b)
                        else:
                            assert (k, a) == (l, b), (n, k, a, l, b)
                    assert any('Levels' in loss for loss in reply['losses']), reply['losses']
                else:
                    assert len(got) == 1, got
                worst = same_channels(source, back, bound, path.name)
                out[path.name] = dict(facts=facts, worst=worst)
                call('close_document', document_id=back, discard_changes=True)
            return source, out

        _, l55 = export_cases(16, [('psd', 'layers'), ('psb', 'layers'), ('tiff', 'layers'),
                                   ('psd', 'flattened')], 1, 'lab55')
        receipts['l55'] = l55
        print('PASS l55: a 16-bit Lab document exports as colour-mode-9 PSD, PSB and layered '
              'TIFF that reopen as Lab with its layers and channels', flush=True)

        source56, l56 = export_cases(8, [('psd', 'layers'), ('tiff', 'layers')], 0, 'lab56')
        receipts['l56'] = l56
        print('PASS l56: an 8-bit Lab document exports as colour-mode-9 PSD and layered TIFF '
              'whose channels reopen equal', flush=True)

        path57 = folder / 'lab56_layers.psd'
        report57 = call('inspect_psd', path=str(path57))
        observed = [o for o in report57.get('observations') or [] if isinstance(o, str)]
        top57 = [o for o in observed if '\u201cTop\u201d' in o and 'color_overlay' in o]
        ranged57 = [o for o in observed if '\u201cRanged\u201d' in o and 'Blend If ranges on' in o]
        assert top57 and ranged57, observed
        back57 = reopen_layers(path57)
        row57 = next(l for l in call('get_document', document_id=back57)['layers']
                     if l['name'] == 'Ranged')
        style57 = row57.get('style') or {}
        if isinstance(style57, str):
            style57 = json.loads(style57)
        blend57 = row57.get('blend_if') or style57.get('blend_if')
        ranges57 = blend57 if isinstance(blend57, list) else [blend57]
        assert any(r and r.get('channel') == 'red' and list(r.get('underlying')) == [0, 0, 96, 160]
                   for r in ranges57), row57
        receipts['l57'] = dict(top=top57, ranged=ranged57, blend_if=blend57)
        print('PASS l57: the export carries the Color Overlay and Blend If on Lightness',
              flush=True)

        # ---- L58–L61: phase 4a ------------------------------------------------------------
        def geo_canvas(doc):
            info = call('get_document', document_id=doc)
            return info['width'], info['height']

        def geo_probe(doc):
            return [call('sample_pixel', document_id=doc, x=x, y=y)['center_rgba16']
                    for x, y in PROBE58]

        def geo_step(doc, tool, after=None, **arguments):
            """`tool` is one step (module doc, phase 4a); `after(doc, reply)` reads the result
            before the undo."""
            before, size, samples = undo_count(doc), geo_canvas(doc), geo_probe(doc)
            reply = call(tool, document_id=doc, **arguments)
            assert undo_count(doc) == before + 1, (tool, reply)
            assert mode(doc) == 'lab', tool
            for px in geo_probe(doc):
                if px[3] == 65535:
                    assert px[1] == px[2] == NEUTRAL16, (tool, px)
            if after:
                after(doc, reply)
            call('undo', document_id=doc)
            assert geo_canvas(doc) == size and geo_probe(doc) == samples, (tool, 'undo')
            return reply

        def geo_file(bits):
            rows = []
            for y in range(GEO_H):
                for x in range(GEO_W):
                    l_star = 5 + x * 90 / (GEO_W - 1)
                    rows.append([round(l_star * (255 if bits == 8 else 65535) / 100), 0, 0])
            path = folder / f'geo{bits}.tif'
            write_lab_tiff(path, 8, bits, rows, size=(GEO_W, GEO_H))
            doc = call('open_document', path=str(path))['document']['id']
            assert mode(doc) == 'lab' and lab_depth(doc) == bits, bits
            assert geo_canvas(doc) == (GEO_W, GEO_H), geo_canvas(doc)
            return doc

        l58 = {}
        gdoc58 = geo_file(16)
        for name, tool, arguments in [
                ('image_size', 'image_size', dict(width=30, height=20)),
                ('canvas_size', 'canvas_size', dict(width=30, height=20, anchor='center')),
                ('crop', 'crop', dict(x=2, y=2, width=16, height=10)),
                ('crop_angle', 'crop', dict(x=2, y=2, width=16, height=10, angle=7)),
                ('rotate90', 'rotate', dict(degrees=90)),
                ('rotate17', 'rotate', dict(degrees=17)),
                ('flip', 'flip', dict(axis='horizontal')),
                ('perspective_crop', 'perspective_crop',
                 dict(corners=[[1, 1], [22, 2], [21, 14], [2, 15]], width=16, height=10))]:
            reply = geo_step(gdoc58, tool, **arguments)
            l58[name] = {k: reply.get(k) for k in ('ok', 'changed', 'width', 'height')}
        bdoc58 = geo_file(8)
        geo_step(bdoc58, 'image_size', width=30, height=20)
        l58['image_size8'] = 'one step'
        receipts['l58'] = l58
        print('PASS l58: Image and Canvas Size, Crop (and straightened), Rotate, Flip and '
              'Perspective Crop are one step each on Lab, a* = b* = 0 kept', flush=True)

        l59 = {}
        tdoc59 = geo_file(16)

        def width30(doc, reply):
            assert geo_canvas(doc)[0] == 30, (geo_canvas(doc), reply)

        def back_to_size(doc, reply):
            assert geo_canvas(doc) == (GEO_W, GEO_H), (geo_canvas(doc), reply)

        for name, tool, arguments in [
                ('transform_layer', 'transform_layer', dict(layer=0, rotate=30,
                                                            sampler='bilinear')),
                ('transform_layers', 'transform_layers',
                 dict(layers=[0], matrix=[0.9, 0.1, -0.1, 0.9, 2, 1], sampler='bilinear')),
                ('transform_again', 'transform_again', {})]:
            reply = geo_step(tdoc59, tool, **arguments)
            l59[name] = {k: reply.get(k) for k in ('ok', 'changed')}
        call('add_layer_mask', document_id=tdoc59, layer=0)
        geo_step(tdoc59, 'transform_mask', layer=0, matrix=[0.8, 0.2, -0.2, 0.8, 3, 1],
                 filter='bilinear')
        call('undo', document_id=tdoc59)
        geo_step(tdoc59, 'straighten_layers', layers=[0], x0=0, y0=0, x1=23, y1=3)
        call('transform_layer', document_id=tdoc59, layer=0, translate_x=6)
        geo_step(tdoc59, 'reveal_canvas', after=width30)
        call('undo', document_id=tdoc59)
        call('canvas_size', document_id=tdoc59, width=30, height=20, anchor='center')
        geo_step(tdoc59, 'trim', after=back_to_size, based_on='transparent')
        call('undo', document_id=tdoc59)
        assert geo_canvas(tdoc59) == (GEO_W, GEO_H), geo_canvas(tdoc59)
        call('select_rect', document_id=tdoc59, x=2, y=2, width=8, height=6)
        bounds59 = call('get_selection', document_id=tdoc59)['bounds']
        samples59 = geo_probe(tdoc59)
        call('transform_selection', document_id=tdoc59, angle=15)
        turned59 = call('get_selection', document_id=tdoc59)['bounds']
        assert turned59 != bounds59, (bounds59, turned59)
        assert geo_probe(tdoc59) == samples59, 'transform_selection moved pixels'
        call('deselect', document_id=tdoc59)
        l59['transform_selection'] = dict(before=bounds59, after=turned59)
        receipts['l59'] = l59
        print('PASS l59: Free Transform, Transform Again, Transform Mask, Straighten, Reveal All, '
              'Trim and Transform Selection on Lab, one step each', flush=True)

        def attempt(tool, **arguments):
            """`tool` cleared the Lab gate (module doc, phase 6b-ii): (ok, reply or text)."""
            result = app.client.rpc('tools/call', {'name': tool, 'arguments': arguments})
            text = next((c['text'] for c in result.get('content', []) if c['type'] == 'text'), '')
            if result.get('isError'):
                assert not any(part in (text or '') for part in GATE_PARTS), (tool, text)
                return False, text
            try:
                return True, json.loads(text)
            except (TypeError, json.JSONDecodeError):
                return True, text

        l60 = {}
        for depth in (16, 8):
            pdoc60 = call('new_document', width=12, height=8, color_mode='lab', depth=depth,
                          background='#808080')['document']['id']
            centre = call('sample_pixel', document_id=pdoc60, x=6, y=4)['center_rgba16']
            before = undo_count(pdoc60)
            reply = call('crop', document_id=pdoc60, x=-3, y=-2, width=18, height=12,
                         delete_cropped=True, background_color='#3366cc')
            assert undo_count(pdoc60) == before + 1, reply
            assert reply.get('extended') is True, reply
            assert mode(pdoc60) == 'lab', depth
            pad = call('sample_pixel', document_id=pdoc60, x=0, y=0)['lab']
            within((pad['l'], pad['a'], pad['b']), PAD58, depth, f'pad {depth}')
            moved = call('sample_pixel', document_id=pdoc60, x=9, y=6)['center_rgba16']
            assert moved == centre, (depth, moved, centre)
            call('undo', document_id=pdoc60)
            before = undo_count(pdoc60)
            call('fill_selection', document_id=pdoc60, color='#3366cc')
            assert undo_count(pdoc60) == before + 1, ('fill_selection', depth)
            filled = call('sample_pixel', document_id=pdoc60, x=6, y=4)['lab']
            within((filled['l'], filled['a'], filled['b']), PAD58, depth, f'fill {depth}')
            call('undo', document_id=pdoc60)
            aware = call('crop', document_id=pdoc60, x=-3, y=-2, width=18, height=12,
                         delete_cropped=True, fill='content_aware')
            assert mode(pdoc60) == 'lab', aware
            # Updated for 6b-iii: Generative Expand clears the gate and the mode stays lab.
            ok60, expand = attempt('crop', document_id=pdoc60, x=-3, y=-2, width=18,
                                   height=12, fill='generative_expand')
            assert mode(pdoc60) == 'lab', (depth, expand)
            l60[str(depth)] = dict(pad=pad, fill=filled, centre=centre, aware=aware.get('fill'),
                                   expand='ok' if ok60 else expand)
        receipts['l60'] = l60
        print('PASS l60: a crop past the canvas pads the Background in Lab at 16 and 8 bits; '
              'content_aware runs, generative_expand clears the gate '
              f'{ {d: l60[d]["expand"] for d in l60} }', flush=True)

        sdoc61 = geo_file(16)
        call('select_rect', document_id=sdoc61, x=2, y=2, width=8, height=6)
        again61 = geo_step(sdoc61, 'transform_again')
        again61 = {k: again61.get(k) for k in ('ok', 'target', 'changed')}
        call('deselect', document_id=sdoc61)
        # Updated before L99's run: the wand runs.
        wand61 = call('select_magic_wand', document_id=sdoc61, x=2, y=2, tolerance=10)
        assert mode(sdoc61) == 'lab', wand61
        receipts['l61'] = dict(transform_again=again61, select_magic_wand=wand61.get('ok'))
        print('PASS l61: Transform Again over a selection is one step; the Magic Wand runs',
              flush=True)

        # ---- L62–L63: phase 4a-ii ---------------------------------------------------------
        # Every earlier document is closed first (setup only, module doc); L62 and L63 open
        # fresh files.
        for open_doc in call('list_documents')['documents']:
            call('close_document', document_id=open_doc['id'], discard_changes=True)
        l62 = {}
        wdoc62 = geo_file(16)
        displacements62 = [[0, 0] for _ in range(9)]
        displacements62[4] = [2, 1.5]
        pins62 = [{'from': [8, 8], 'to': [10, 9]}, {'from': [17, 8], 'to': [17, 7]}]
        quads62 = [{'from': [3, 2, 21, 2, 21, 14, 3, 14], 'to': [4, 3, 22, 1, 20, 15, 2, 13]}]
        strokes62 = [{'tool': 'forward_warp', 'size': 8, 'points': [[8, 6], [14, 7]]}]

        def no_field(doc, reply):
            assert reply.get('changed') is True, reply
            field = call('get_warp', document_id=doc, layer=0)
            assert field.get('field') is None, field

        reply = geo_step(wdoc62, 'warp_layer', after=no_field, layer=0, operation='set',
                         target='pixels', columns=3, rows=3, displacements=displacements62,
                         filter='bicubic')
        l62['warp_layer'] = {k: reply.get(k) for k in ('ok', 'changed')}
        for tool, arguments in [
                ('puppet_warp', dict(layer=0, pins=pins62)),
                ('perspective_warp', dict(layer=0, quads=quads62)),
                ('liquify', dict(layer=0, strokes=strokes62))]:
            reply = geo_step(wdoc62, tool, **arguments)
            l62[tool] = {k: reply.get(k) for k in ('ok', 'changed')}
        bdoc62 = geo_file(8)
        geo_step(bdoc62, 'liquify', layer=0, strokes=strokes62)
        l62['liquify8'] = 'one step'
        call('select_rect', document_id=wdoc62, x=2, y=2, width=8, height=6)
        pins62s = [{'from': [4, 4], 'to': [5, 5]}, {'from': [8, 6], 'to': [8, 5]}]
        quads62s = [{'from': [3, 3, 9, 3, 9, 7, 3, 7], 'to': [4, 3, 9, 2, 10, 8, 3, 7]}]
        for tool, arguments in [
                ('puppet_warp', dict(layer=0, pins=pins62s)),
                ('perspective_warp', dict(layer=0, quads=quads62s)),
                ('move_selected_pixels', dict(dx=2, dy=1))]:
            reply = geo_step(wdoc62, tool, **arguments)
            l62[tool + '_selected'] = {k: reply.get(k) for k in ('ok', 'changed', 'target')}
        call('deselect', document_id=wdoc62)
        receipts['l62'] = l62
        print('PASS l62: Warp (no field kept), Puppet Warp, Perspective Warp and Liquify are one '
              'step each on Lab, a* = b* = 0 kept, and with a selection Puppet Warp, Perspective '
              'Warp and Move Selected Pixels too', flush=True)

        l63 = {}
        mdoc63 = geo_file(16)
        call('canvas_size', document_id=mdoc63, width=60, height=24, anchor='top-left')
        call('duplicate_layer', document_id=mdoc63, layer=0)
        call('duplicate_layer', document_id=mdoc63, layer=0)
        call('move_layers', document_id=mdoc63, layers=[1], dx=14, dy=0)
        call('move_layers', document_id=mdoc63, layers=[2], dx=36, dy=8)

        def offsets(doc):
            return {layer['index']: (layer['offset_x'], layer['offset_y'])
                    for layer in call('get_document', document_id=doc)['layers']}

        l63['setup'] = offsets(mdoc63)

        def all_top(doc, reply):
            assert all(m['offset_y'] == 0 for m in reply['moved']), reply

        def spaced(doc, reply):
            assert offsets(doc)[1][0] == 18, (offsets(doc), reply)

        def nudged(doc, reply):
            assert offsets(doc)[1] == (16, 1), (offsets(doc), reply)

        def copied(doc, reply):
            info = call('get_document', document_id=doc)
            assert len(info['layers']) == 4, info['layers']
            copies = reply['copies']
            assert len(copies) == 1, reply
            where = offsets(doc)[copies[0]]
            assert where == (17, 0), (where, reply)

        for name, tool, after, arguments in [
                ('align_layers', 'align_layers', all_top, dict(layers=[0, 1, 2], edge='top')),
                ('distribute_layers', 'distribute_layers', spaced,
                 dict(layers=[0, 1, 2], axis='horizontal')),
                ('move_layers', 'move_layers', nudged, dict(layers=[1], dx=2, dy=1)),
                ('move_layers_duplicate', 'move_layers', copied,
                 dict(layers=[1], dx=3, dy=0, duplicate=True))]:
            reply = geo_step(mdoc63, tool, after=after, **arguments)
            l63[name] = {k: reply.get(k) for k in ('ok', 'moved', 'copies')}
        receipts['l63'] = l63
        print('PASS l63: Align, Distribute, the Move tool and its duplicate are one step each on '
              'Lab, a* = b* = 0 kept', flush=True)

        # ---- L64–L65: phase 4b-i ----------------------------------------------------------
        # Every earlier document is closed first, and again before L65 (setup only, module
        # doc): `open_document` of an open path answers that document.
        def close_all():
            for open_doc in call('list_documents')['documents']:
                call('close_document', document_id=open_doc['id'], discard_changes=True)

        def layer_count(doc):
            return len(call('get_document', document_id=doc)['layers'])

        l64 = {}
        close_all()
        pdoc64 = geo_file(16)
        call('select_rect', document_id=pdoc64, x=2, y=2, width=8, height=6)
        count64 = layer_count(pdoc64)

        def one_more(doc, reply):
            assert layer_count(doc) == count64 + 1, (layer_count(doc), reply)

        for name, tool, after, arguments in [
                ('transform', 'transform_selected_pixels', None, dict(scale=1.25)),
                ('move_duplicate', 'move_selected_pixels', None,
                 dict(dx=2, dy=1, duplicate=True)),
                ('layer_via_copy', 'layer_via_copy', one_more, {}),
                ('layer_via_cut', 'layer_via_cut', None, {})]:
            reply = geo_step(pdoc64, tool, after=after, **arguments)
            l64[name] = {k: reply.get(k) for k in ('ok', 'changed', 'layer')}
        call('duplicate_layer', document_id=pdoc64, layer=0)
        reply = geo_step(pdoc64, 'stamp_selected', layers=[0, 1])
        l64['stamp_selected'] = {k: reply.get(k) for k in ('ok', 'layer', 'members')}
        reply = geo_step(pdoc64, 'move_selected_pixels', dx=2, dy=1, target='lightness')
        l64['lightness'] = {k: reply.get(k) for k in ('ok', 'changed', 'layer')}
        receipts['l64'] = l64
        print('PASS l64: Transform and Move Selected Pixels, Layer via Copy and Cut, Stamp '
              'Selected and (6b-i) a Lightness move are one step each on Lab, a* = b* = 0 kept',
              flush=True)

        l65 = {}
        close_all()
        cdoc65 = geo_file(16)
        call('select_rect', document_id=cdoc65, x=2, y=2, width=8, height=6)
        channels65 = len(call('list_channels', document_id=cdoc65)['channels'])

        def one_more_channel(doc, reply):
            got = len(call('list_channels', document_id=doc)['channels'])
            assert got == channels65 + 1, (got, reply)

        reply = geo_step(cdoc65, 'save_selection', after=one_more_channel, name='Sel65')
        l65['save_selection'] = {k: reply.get(k) for k in ('ok', 'channel', 'name')}
        call('save_selection', document_id=cdoc65, name='Sel65')
        for name, arguments in [
                ('channel', {'from': 'channel', 'channel': 'Sel65'}),
                ('lightness', {'from': 'plane', 'plane': 'lightness'})]:
            call('deselect', document_id=cdoc65)
            reply = call('load_selection', document_id=cdoc65, **arguments)
            assert reply.get('ok') is True, (name, reply)
            selection = call('get_selection', document_id=cdoc65)
            assert selection['selection_empty'] is False, (name, selection)
            assert mode(cdoc65) == 'lab', name
            l65[name] = selection.get('bounds')
        red65 = call('load_selection', fail=True, document_id=cdoc65, plane='red',
                     **{'from': 'plane'})
        assert red65 == LOAD_PLANE_REFUSAL, red65
        l65['red'] = red65
        invert65 = call('modify_selection', document_id=cdoc65, operation='invert')
        assert invert65.get('ok') is True, invert65
        l65['invert'] = invert65.get('ok')
        receipts['l65'] = l65
        print('PASS l65: Save Selection is one step; Load Selection from a channel and from '
              'Lightness and Select Inverse run on Lab; red refuses', flush=True)

        # ---- L66–L67: phase 5a -------------------------------------------------------------
        close_all()

        def ink_at(doc, x, y):
            reply = call('sample_color', document_id=doc, x=x, y=y)
            assert reply.get('color_mode') == 'cmyk', reply
            return reply['ink16']

        l66 = {}
        doc66 = new_lab(16, background=COLOUR66)
        before66 = undo_count(doc66)
        reply = call('convert_mode', document_id=doc66, mode='cmyk')
        assert reply['color_mode'] == 'cmyk' and reply['depth'] == 16, reply
        assert undo_count(doc66) == before66 + 1, reply
        call('undo', document_id=doc66)
        assert mode(doc66) == 'lab'
        call('redo', document_id=doc66)
        assert mode(doc66) == 'cmyk'
        ink66 = ink_at(doc66, 3, 1)
        doc66_8 = new_lab(8, background=COLOUR66)
        reply = call('convert_mode', document_id=doc66_8, mode='cmyk')
        assert reply['color_mode'] == 'cmyk' and reply['depth'] == 8, reply
        via66 = new_lab(16, background=COLOUR66)
        call('convert_mode', document_id=via66, mode='rgb', rgb_space='srgb')
        call('convert_mode', document_id=via66, mode='cmyk')
        via_ink66 = ink_at(via66, 3, 1)
        worst66 = max(abs(a - b) for a, b in zip(ink66, via_ink66))
        assert worst66 <= INK_BOUND66, (ink66, via_ink66)
        adj66 = new_lab(16, background=COLOUR66)
        call('add_adjustment_layer', document_id=adj66, op='levels', params={'gamma': 1.2},
             name='Levels66')
        refusal66 = call('convert_mode', fail=True, document_id=adj66, mode='cmyk')
        assert refusal66 == ADJUSTMENT_CMYK66, refusal66
        assert mode(adj66) == 'lab'
        call('convert_mode', document_id=adj66, mode='cmyk', flatten=True)
        assert mode(adj66) == 'cmyk'
        assert len(call('get_document', document_id=adj66)['layers']) == 1
        l66.update(ink16=ink66, via_srgb=via_ink66, worst=worst66, refusal=refusal66)
        receipts['l66'] = l66
        print('PASS l66: Lab → CMYK is one step at 16 and 8 bits, within the sRGB route\'s ink; '
              'an adjustment layer refuses by name and flatten converts', flush=True)

        l67 = {}
        doc67 = new_lab(16, background=COLOUR66)
        codes16 = codes_at(doc67, POINTS67)
        dry67 = call('convert_precision', document_id=doc67, mode='lab8', dry_run=True)
        assert dry67.get('dry_run') is True and dry67.get('changed') is True, dry67
        assert dry67['working_memory_plan_bytes'] > 0, dry67
        assert lab_depth(doc67) == 16
        before67 = undo_count(doc67)
        reply = call('convert_precision', document_id=doc67, mode='lab8')
        assert reply.get('changed') is True and reply.get('bits_per_channel') == 8, reply
        assert undo_count(doc67) == before67 + 1, reply
        assert lab_depth(doc67) == 8
        codes8 = codes_at(doc67, POINTS67)
        assert all(v % 257 == 0 for px in codes8 for v in px), codes8
        call('undo', document_id=doc67)
        assert lab_depth(doc67) == 16
        assert codes_at(doc67, POINTS67) == codes16
        call('redo', document_id=doc67)
        assert lab_depth(doc67) == 8
        reply = call('convert_precision', document_id=doc67, mode='lab16')
        assert reply.get('bits_per_channel') == 16, reply
        assert lab_depth(doc67) == 16
        assert codes_at(doc67, POINTS67) == codes8
        refusal67 = call('convert_precision', fail=True, document_id=doc67, mode='lab32')
        assert refusal67 == LAB32_REFUSAL, refusal67
        call('save_action', name='lab-depth-cmyk-547', overwrite=True, steps=[
            {'tool': 'convert_precision', 'arguments': {'mode': 'lab8'}},
            {'tool': 'convert_mode', 'arguments': {'mode': 'cmyk'}},
        ])
        played67, direct67 = new_lab(16, background=COLOUR66), new_lab(16, background=COLOUR66)
        call('run_action', document_id=played67, name='lab-depth-cmyk-547')
        call('convert_precision', document_id=direct67, mode='lab8')
        call('convert_mode', document_id=direct67, mode='cmyk')
        played_ink = [ink_at(played67, x, y) for x, y in POINTS67]
        direct_ink = [ink_at(direct67, x, y) for x, y in POINTS67]
        assert played_ink == direct_ink, (played_ink, direct_ink)
        call('delete_action', name='lab-depth-cmyk-547')
        l67.update(codes16=codes16, codes8=codes8, plan=dry67['working_memory_plan_bytes'],
                   refusal=refusal67, ink=played_ink)
        receipts['l67'] = l67
        print('PASS l67: 8/16 Bits/Channel on Lab is one step each, on the 257 grid at 8; 32 '
              'refuses; an Action replays depth and CMYK', flush=True)

        # ---- L68: phase 4b-ii -------------------------------------------------------------
        l68 = {}
        close_all()
        rdoc68 = call('new_document', width=12, height=8, color_mode='lab', depth=16,
                      background='#808080')['document']['id']
        call('select_rect', document_id=rdoc68, x=3, y=2, width=5, height=4)
        rect68 = call('get_selection', document_id=rdoc68)['bounds']
        call('fill_selection', document_id=rdoc68, color='#3366cc')
        call('deselect', document_id=rdoc68)
        codes68 = call('sample_pixel', document_id=rdoc68, x=5, y=3)['center_rgba16'][:3]
        reply = call('select_color_range', document_id=rdoc68, range='sampled',
                     samples16=[codes68], tolerance=5, softness=0)
        assert reply.get('ok') is True, reply
        selection = call('get_selection', document_id=rdoc68)
        assert selection['bounds'] == rect68, (selection, rect68)
        assert mode(rdoc68) == 'lab', 'select_color_range'
        l68['sampled'] = dict(codes=codes68, bounds=selection['bounds'])
        got = call('select_color_range', document_id=rdoc68, range='highlights')
        assert got.get('ok') is True and mode(rdoc68) == 'lab', got
        l68['highlights'] = got
        # Updated before L99's run: the wand and Similar run.
        call('deselect', document_id=rdoc68)
        for tool, arguments in [('select_magic_wand', dict(x=5, y=3, tolerance=10)),
                                ('select_similar', dict(tolerance=10))]:
            call(tool, document_id=rdoc68, **arguments)
            bounds = call('get_selection', document_id=rdoc68)['bounds']
            assert bounds == rect68 and mode(rdoc68) == 'lab', (tool, bounds, rect68)
            l68[tool] = bounds
        receipts['l68'] = l68
        print('PASS l68: sampled Color Range selects the rect on Lab; highlights runs (6b-i); '
              'the Magic Wand and Select Similar select the rect', flush=True)

        # ---- L70–L71: phase 4c ------------------------------------------------------------
        close_all()

        def codes70(doc, points=POINTS70):
            return [call('sample_pixel', document_id=doc, x=x, y=y)['center_rgba16']
                    for x, y in points]

        l70 = {}
        src70 = call('new_document', width=8, height=6, color_mode='lab', depth=16,
                     background='#3366cc')['document']['id']
        call('select_rect', document_id=src70, x=2, y=1, width=4, height=3)
        call('fill_selection', document_id=src70, color='#c8285a')
        call('select_all', document_id=src70)
        call('copy', document_id=src70, layer=0)
        call('deselect', document_id=src70)
        clip70 = call('get_clipboard')
        assert clip70['mode'] == 'lab', clip70
        want70 = codes70(src70)
        dst70 = call('new_document', width=8, height=6, color_mode='lab', depth=16,
                     background='#808080')['document']['id']
        before = undo_count(dst70)
        pasted = call('paste', document_id=dst70, placement='in_place')
        assert undo_count(dst70) == before + 1 and mode(dst70) == 'lab', pasted
        assert pasted.get('color_conversion') is None, pasted
        assert codes70(dst70) == want70, (codes70(dst70), want70)
        l70['lab_to_lab'] = want70
        rgb70 = call('new_document', width=8, height=6, depth=16, profile='srgb',
                     background='#ffffff')['document']['id']
        into_rgb = call('paste', document_id=rgb70, placement='in_place')
        conversion = into_rgb['color_conversion']
        assert conversion and conversion['from'] == 'Lab' and conversion['to'] == 'RGB', into_rgb
        got_rgb = codes70(rgb70)
        call('convert_mode', document_id=src70, mode='rgb', rgb_space='srgb')
        ref_rgb = codes70(src70)
        call('undo', document_id=src70)
        assert mode(src70) == 'lab'
        for got, ref in zip(got_rgb, ref_rgb):
            assert max(abs(got[c] - ref[c]) for c in range(3)) <= RGB_BOUND70, (got, ref)
        l70['lab_to_rgb'] = dict(got=got_rgb, convert_mode=ref_rgb, conversion=conversion)
        srgb70 = call('new_document', width=8, height=6, depth=16, profile='srgb',
                      background='#c8285a')['document']['id']
        call('select_all', document_id=srgb70)
        call('copy', document_id=srgb70, layer=0)
        lab70 = call('new_document', width=8, height=6, color_mode='lab', depth=16,
                     background='#808080')['document']['id']
        refused = call('paste', fail=True, document_id=lab70, placement='in_place',
                       profile='preserve')
        assert refused == CROSS_LAB, refused
        into_lab = call('paste', document_id=lab70, placement='in_place')
        conversion = into_lab['color_conversion']
        assert conversion and conversion['from'] == 'RGB' and conversion['to'] == 'Lab', into_lab
        assert conversion['black_point_compensation'] is False, conversion
        lab = call('sample_pixel', document_id=lab70, x=3, y=3)['lab']
        within((lab['l'], lab['a'], lab['b']), (0xc8, 0x28, 0x5a), 16, 'sRGB into Lab')
        l70['rgb_to_lab'] = dict(lab=lab, refused=refused, conversion=conversion)
        receipts['l70'] = l70
        print('PASS l70: a Lab copy pastes its codes into Lab, converts into sRGB as Mode ▸ RGB '
              'Color does, and sRGB pastes into Lab by Mode ▸ Lab Color; Don\'t Convert refuses '
              'across modes', flush=True)

        l71 = {}
        dup71 = call('duplicate_to_new_document', document_id=src70, layers=[0])
        doc71 = dup71['document']['id']
        assert mode(doc71) == 'lab' and dup71['depth'] == 16, dup71
        assert codes70(doc71) == want70, (codes70(doc71), want70)
        l71['duplicate'] = dup71['depth']
        rgb71 = call('new_document', width=8, height=6, depth=16, profile='srgb',
                     background='#ffffff')['document']['id']
        out71 = call('copy_layers_to_document', document_id=src70, layers=[0],
                     to_document_id=rgb71)
        conversion = out71['color_conversion']
        assert conversion and conversion['from'] == 'Lab' and conversion['to'] == 'RGB', out71
        assert mode(rgb71) == 'rgb', out71
        lab71 = call('new_document', width=8, height=6, color_mode='lab', depth=16,
                     background='#808080')['document']['id']
        in71 = call('copy_layers_to_document', document_id=srgb70, layers=[0],
                    to_document_id=lab71)
        conversion = in71['color_conversion']
        assert conversion and conversion['from'] == 'RGB' and conversion['to'] == 'Lab', in71
        assert mode(lab71) == 'lab', in71
        l71['layers'] = dict(out=out71['color_conversion'], into=in71['color_conversion'])
        gray71 = call('new_document', width=4, height=3, color_mode='gray', depth=16,
                      background='#808080')['document']['id']
        glab71 = call('new_document', width=4, height=3, color_mode='lab', depth=16,
                      background='#ffffff')['document']['id']
        call('copy_layers_to_document', document_id=gray71, layers=[0], to_document_id=glab71)
        every = [(x, y) for y in range(3) for x in range(4)]
        landed = codes70(glab71, every)
        call('convert_mode', document_id=gray71, mode='lab')
        assert mode(gray71) == 'lab'
        oracle = codes70(gray71, every)
        assert landed == oracle, (landed, oracle)
        l71['gray_to_lab'] = landed[0]
        box71 = call('new_document', width=12, height=8, color_mode='lab', depth=16,
                     background='#808080')['document']['id']
        call('new_layer', document_id=box71, name='Box')
        call('select_rect', document_id=box71, x=3, y=2, width=5, height=4)
        call('fill_selection', document_id=box71, color='#3366cc')
        call('deselect', document_id=box71)
        call('copy', document_id=box71, mode='layers', layers=[1])
        made71 = call('new_from_clipboard')
        assert made71['document']['color_mode'] == 'lab', made71
        assert (made71['width'], made71['height']) == (5, 4), made71
        l71['new_from_clipboard'] = (made71['width'], made71['height'])
        receipts['l71'] = l71
        print('PASS l71: Duplicate to New Document keeps Lab; copying layers converts across '
              'modes, Gray → Lab as Mode ▸ Lab Color; New from Clipboard opens Lab at the '
              'content box', flush=True)

        # ---- L69: phase 5a-ii ----------------------------------------------------------
        close_all()

        def gray_oracle(code):
            l_star = 100.0 * code[0] / 65535.0
            fy = (l_star + 16.0) / 116.0
            y = fy ** 3 if fy ** 3 > 216.0 / 24389.0 else (116.0 * fy - 16.0) * 27.0 / 24389.0
            return 65535.0 * y ** (1.0 / 2.2)

        def to_gray69(doc):
            return call('convert_mode', document_id=doc, mode='gray', gray_space='gray_gamma_22')

        l69 = {}
        doc69 = new_lab(16, background=COLOUR66)
        lab69 = codes_at(doc69, POINTS67)
        before69 = undo_count(doc69)
        reply = to_gray69(doc69)
        assert reply['color_mode'] == 'gray' and reply['depth'] == 16, reply
        assert undo_count(doc69) == before69 + 1, reply
        call('undo', document_id=doc69)
        assert mode(doc69) == 'lab'
        call('redo', document_id=doc69)
        assert mode(doc69) == 'gray'
        gray69 = codes_at(doc69, POINTS67)
        worst69 = 0.0
        for got, lab in zip(gray69, lab69):
            assert got[0] == got[1] == got[2], gray69
            worst69 = max(worst69, abs(got[0] - gray_oracle(lab)))
        assert worst69 <= GRAY_BOUND69, (gray69, lab69, worst69)
        doc69_8 = new_lab(8, background=COLOUR66)
        reply = to_gray69(doc69_8)
        assert reply['color_mode'] == 'gray' and reply['depth'] == 8, reply
        adj69 = new_lab(16, background=COLOUR66)
        call('add_adjustment_layer', document_id=adj69, op='levels', params={'gamma': 1.2},
             name='Levels69')
        refusal69 = call('convert_mode', fail=True, document_id=adj69, mode='gray',
                         gray_space='gray_gamma_22')
        assert refusal69 == ADJUSTMENT_GRAY69, refusal69
        assert mode(adj69) == 'lab'
        call('convert_mode', document_id=adj69, mode='gray', gray_space='gray_gamma_22',
             flatten=True)
        assert mode(adj69) == 'gray'
        assert len(call('get_document', document_id=adj69)['layers']) == 1
        call('save_action', name='lab-gray-547', overwrite=True, steps=[
            {'tool': 'convert_mode', 'arguments': {'mode': 'gray', 'gray_space': 'gray_gamma_22'}},
        ])
        played69 = new_lab(16, background=COLOUR66)
        call('run_action', document_id=played69, name='lab-gray-547')
        assert mode(played69) == 'gray'
        played_codes = codes_at(played69, POINTS67)
        assert played_codes == gray69, (played_codes, gray69)
        call('delete_action', name='lab-gray-547')
        l69.update(lab=lab69, gray=gray69, worst=worst69, refusal=refusal69)
        receipts['l69'] = l69
        print('PASS l69: Lab → Grayscale is one step at 16 and 8 bits, L* through Gray Gamma 2.2; '
              'an adjustment layer refuses by name and flatten converts; an Action replays it',
              flush=True)

        # ---- L72–L73: phase 5b ------------------------------------------------------------
        close_all()

        def proof(doc, **arguments):
            """(pixels, the reply's facts) of `render_proof`."""
            result = app.client.rpc('tools/call', {'name': 'render_proof', 'arguments': dict(
                document_id=doc, **arguments)})
            assert not result.get('isError'), result
            data = next(c['data'] for c in result['content'] if c['type'] == 'image')
            facts = json.loads(next(c['text'] for c in result['content'] if c['type'] == 'text'))
            return png_pixels(base64.b64decode(data))[2], facts

        l72 = {}
        doc72 = call('new_document', width=12, height=8, color_mode='lab', depth=16,
                     background=COLOUR66)['document']['id']
        call('select_rect', document_id=doc72, x=2, y=2, width=4, height=3)
        call('fill_selection', document_id=doc72, color='#0000ff')
        call('deselect', document_id=doc72)
        reply = call('set_proof_setup', document_id=doc72, profile='working_cmyk', proof_colors=True)
        assert reply.get('ok') is True and reply.get('proof_colors') is True, reply
        readings, deadline = [], time.monotonic() + 5
        while time.monotonic() < deadline:
            state = call('get_proof_setup', document_id=doc72)
            assert state['proof_colors'] is True and state['failure'] is None, state
            readings.append(state['failure'])
            time.sleep(0.25)
        got = call('set_proof_setup', fail=True, document_id=doc72, preserve_numbers=True)
        assert got == PRESERVE_REFUSAL72, got
        lab72 = {}
        for name, extra in (('display', {}), ('exact', {'exact': True})):
            pixels, facts = proof(doc72, gamut_warning=True, **extra)
            assert facts['space'] == 'display-p3', (name, facts)
            assert facts['out_of_gamut'] > 0, (name, facts)
            if name == 'display':
                assert facts['source'] == 'display', facts
            lab72[name] = (pixels, facts)
        reply = call('convert_mode', document_id=doc72, mode='rgb', rgb_space='displayP3')
        assert reply['color_mode'] == 'rgb', reply
        worst72 = {}
        for name, extra in (('display', {}), ('exact', {'exact': True})):
            pixels, facts = proof(doc72, gamut_warning=True, **extra)
            ours, theirs = lab72[name]
            assert len(pixels) == len(ours), name
            worst = max(abs(a - b) for p, q in zip(ours, pixels) for a, b in zip(p[:3], q[:3]))
            assert worst <= PROOF_BOUND8, (name, worst)
            assert facts['out_of_gamut'] == theirs['out_of_gamut'], (name, facts, theirs)
            worst72[name] = dict(worst=worst, out_of_gamut=facts['out_of_gamut'])
        call('undo', document_id=doc72)
        assert mode(doc72) == 'lab'
        doc72_8 = new_lab(8, background=COLOUR66)
        call('set_proof_setup', document_id=doc72_8, profile='working_cmyk', proof_colors=True)
        _, facts8 = proof(doc72_8)
        assert facts8['source'] == 'display', facts8
        supported = call('get_precision', document_id=doc72)['supported_tools']
        assert all(tool in supported for tool in PROOF_TOOLS72), supported
        l72.update(readings=len(readings), refusal=got, worst=worst72, source8=facts8['source'])
        receipts['l72'] = l72
        print('PASS l72: Lab proofs its Display P3 view at 16 and 8 bits, display and exact, as '
              'the same document converted to Display P3 RGB; Preserve Numbers refuses',
              flush=True)

        close_all()
        l73 = {}
        doc73 = new_lab(16, background=COLOUR66)
        got = call('assign_profile', fail=True, document_id=doc73, profile='srgb')
        assert got == UNAVAILABLE_REFUSAL.format('assign_profile'), got
        before73 = undo_count(doc73)
        reply = call('convert_profile', document_id=doc73, profile='working_cmyk')
        assert reply['color_mode'] == 'cmyk' and reply['depth'] == 16, reply
        assert undo_count(doc73) == before73 + 1, reply
        call('undo', document_id=doc73)
        assert mode(doc73) == 'lab'
        call('redo', document_id=doc73)
        assert mode(doc73) == 'cmyk'
        ink73 = [ink_at(doc73, x, y) for x, y in POINTS67]
        via73 = new_lab(16, background=COLOUR66)
        call('convert_mode', document_id=via73, mode='cmyk')
        via_ink73 = [ink_at(via73, x, y) for x, y in POINTS67]
        assert ink73 == via_ink73, (ink73, via_ink73)
        rgb73, via_rgb73 = new_lab(16, background=COLOUR66), new_lab(16, background=COLOUR66)
        reply = call('convert_profile', document_id=rgb73, profile='srgb')
        assert reply['color_mode'] == 'rgb', reply
        call('convert_mode', document_id=via_rgb73, mode='rgb', rgb_space='srgb')
        codes73, via_codes73 = codes_at(rgb73, POINTS67), codes_at(via_rgb73, POINTS67)
        assert codes73 == via_codes73, (codes73, via_codes73)
        call('save_action', name='lab-profile-547', overwrite=True, steps=[
            {'tool': 'convert_profile', 'arguments': {'profile': 'working_cmyk'}},
        ])
        played73 = new_lab(16, background=COLOUR66)
        call('run_action', document_id=played73, name='lab-profile-547')
        assert mode(played73) == 'cmyk'
        played_ink73 = [ink_at(played73, x, y) for x, y in POINTS67]
        assert played_ink73 == ink73, (played_ink73, ink73)
        call('delete_action', name='lab-profile-547')
        l73.update(refusal=got, ink16=ink73, codes=codes73)
        receipts['l73'] = l73
        print('PASS l73: Assign Profile refuses; Convert to Profile is Image ▸ '
              'Mode\'s conversion to CMYK and RGB in one step, and an Action replays it',
              flush=True)

        # ---- L74–L77: phase 6a-i ---------------------------------------------------------
        close_all()

        def row_of(doc, index):
            return next(row for row in call('get_document', document_id=doc)['layers']
                        if row['index'] == index)

        def art_step(doc, tool, **arguments):
            """`tool` is one undo step and the document stays lab: its reply."""
            count = undo_count(doc)
            reply = call(tool, document_id=doc, **arguments)
            assert undo_count(doc) == count + 1, (tool, reply)
            assert mode(doc) == 'lab', tool
            return reply

        def unit8(text):
            digits = text.lstrip('#')
            return tuple(int(digits[i:i + 2], 16) for i in (0, 2, 4))

        def hex6(text):
            """A reported `#rrggbb[aa]` as its lower-case `#rrggbb`."""
            return text.lower()[:7]

        # L74: type and shape.
        l74 = {}
        tdoc74 = new_lab(16, w=40, h=16)
        text74 = art_step(tdoc74, 'add_text_layer', text='Lab', x=2, y=1, size=12,
                          font='Helvetica Neue', color='#ff0000')
        tidx74 = text74['layer']
        assert hex6(row_of(tdoc74, tidx74)['text']['color']) == '#ff0000', row_of(tdoc74, tidx74)
        art_step(tdoc74, 'edit_text_layer', layer=tidx74, color='#00ff00')
        assert hex6(row_of(tdoc74, tidx74)['text']['color']) == '#00ff00', row_of(tdoc74, tidx74)
        shapes74 = {}
        for depth in (16, 8):
            sdoc = new_lab(depth, w=16, h=12)
            shape = art_step(sdoc, 'add_shape_layer', kind='rect', x=3, y=3, w=10, h=6,
                             fill='#00ff00')
            interior = lab_at(sdoc, 8, 6)
            within(interior, (0, 255, 0), depth, f'shape {depth}')
            if depth == 8:
                codes = codes_at(sdoc, PROBE74, source='layer', layer=shape['layer'])
                assert all(v % 257 == 0 for code in codes for v in code), codes
            shapes74[depth] = {'doc': sdoc, 'layer': shape['layer'], 'interior': interior}
        sdoc74, sidx74 = shapes74[16]['doc'], shapes74[16]['layer']
        call('transform_layer', document_id=sdoc74, layer=sidx74, scale_x=2, scale_y=2)
        assert 'shape' in row_of(sdoc74, sidx74), row_of(sdoc74, sidx74)
        raster74 = call('rasterize_artwork', document_id=sdoc74, layer=sidx74)
        assert 'shape' not in row_of(sdoc74, sidx74), row_of(sdoc74, sidx74)
        call('undo', document_id=sdoc74)
        assert 'shape' in row_of(sdoc74, sidx74), row_of(sdoc74, sidx74)
        ink74 = call('add_text_layer', fail=True, document_id=sdoc74, text='Ink', x=1, y=1,
                     size=8, ink=[0, 0, 0, 100])
        assert ink74 == INK_REFUSAL, ink74
        l74.update(interior={str(d): v['interior'] for d, v in shapes74.items()},
                   rasterize=raster74, ink=ink74)
        receipts['l74'] = l74
        print('PASS l74: type and shape layers keep authored sRGB, render L*a*b* (8 bits on the '
              'grid), transform keeps and rasterize drops the description, ink refuses',
              flush=True)

        # L75: fill layers.
        l75 = {}
        fdoc75 = new_lab(16, w=12, h=8)
        solid75 = art_step(fdoc75, 'add_fill_layer', kind='solid', color='#4080c0')
        within(lab_at(fdoc75, 5, 4), unit8('#4080c0'), 16, 'solid fill')
        art_step(fdoc75, 'edit_fill_layer', layer=solid75['layer'], color='#c04080')
        edited75 = lab_at(fdoc75, 5, 4)
        within(edited75, unit8('#c04080'), 16, 'edited fill')
        gdoc75 = new_lab(16, w=64, h=4)
        gradient75 = call('add_fill_layer', document_id=gdoc75, kind='gradient', gradient={
            'stops': [{'position': 0.25, 'color': '#ff0000', 'opacity': 1},
                      {'position': 0.75, 'color': '#0000ff', 'opacity': 1}],
            'angle': 0, 'style': 'linear'})
        within(lab_at(gdoc75, 2, 2), (255, 0, 0), 16, 'gradient red end')
        within(lab_at(gdoc75, 61, 2), (0, 0, 255), 16, 'gradient blue end')
        stops75 = row_of(gdoc75, gradient75['layer'])['fill']['gradient']['stops']
        assert [hex6(s['color']) for s in stops75] == ['#ff0000', '#0000ff'], stops75
        tile75 = base64.b64encode(bytes([255, 0, 0, 255, 0, 0, 255, 255])).decode()
        cdoc75 = new_lab(16, w=6, h=2)
        call('add_fill_layer', document_id=cdoc75, kind='pattern',
             pattern={'pattern': {'name': 'Red Blue', 'width': 2, 'height': 1, 'rgba': tile75}})
        seen75 = set()
        for x in range(6):
            got = lab_at(cdoc75, x, 0)
            texels = []
            for name, rgb8 in (('red', (255, 0, 0)), ('blue', (0, 0, 255))):
                try:
                    within(got, rgb8, 8, f'pattern pixel {x}')
                    texels.append(name)
                except AssertionError:
                    pass
            assert len(texels) == 1, (x, got)
            seen75.add(texels[0])
        assert seen75 == {'red', 'blue'}, seen75
        l75.update(edited=edited75, stops=stops75)
        receipts['l75'] = l75
        print('PASS l75: solid, gradient and pattern fill layers render their sRGB colours as '
              'L*a*b*; the stops keep their authored hexes', flush=True)

        # L76: paths and vector masks.
        pdoc76 = new_lab(16, w=24, h=16)
        call('new_layer', document_id=pdoc76, name='Top')
        box76 = {'closed': True, 'anchors': [[4, 4], [20, 4], [20, 12], [4, 12]]}
        call('set_path', document_id=pdoc76, name='Box', subpaths=[box76])
        assert [p['name'] for p in call('list_paths', document_id=pdoc76)['paths']] == ['Box']
        selected76 = call('path_to_selection', document_id=pdoc76, path='Box')
        assert selected76['selection_empty'] is False, selected76
        call('deselect', document_id=pdoc76)
        art_step(pdoc76, 'fill_path', path='Box', color='#ff0000', layer=1)
        filled76 = lab_at(pdoc76, 12, 8)
        within(filled76, (255, 0, 0), 16, 'fill_path')
        call('stroke_path', document_id=pdoc76, path='Box', color='#0000ff', size=2, layer=1)
        call('set_vector_mask', document_id=pdoc76, layer=1, path='Box')
        assert call('get_vector_mask', document_id=pdoc76, layer=1)['vector_mask'] is True
        before76 = codes_at(pdoc76, PROBE76)
        rz76 = folder / 'paths-6a.rz'
        call('save_copy', document_id=pdoc76, path=str(rz76), format='rz')
        back76 = call('open_document', path=str(rz76))['document']['id']
        assert mode(back76) == 'lab'
        assert [p['name'] for p in call('list_paths', document_id=back76)['paths']] == ['Box']
        assert call('get_vector_mask', document_id=back76, layer=1)['vector_mask'] is True
        assert codes_at(back76, PROBE76) == before76, 'the paths .rz samples'
        shape76 = row_of(sdoc74, sidx74)['shape']
        shaped76 = codes_at(sdoc74, PROBE74)
        rz74s = folder / 'artwork-6a.rz'
        call('save_copy', document_id=sdoc74, path=str(rz74s), format='rz')
        backs76 = call('open_document', path=str(rz74s))['document']['id']
        assert mode(backs76) == 'lab'
        assert json.dumps(row_of(backs76, sidx74)['shape'], sort_keys=True) == json.dumps(
            shape76, sort_keys=True), (row_of(backs76, sidx74), shape76)
        assert codes_at(backs76, PROBE74) == shaped76, 'the shape .rz samples'
        receipts['l76'] = {'filled': filled76}
        print('PASS l76: paths, Fill and Stroke Path and a vector mask work on Lab and persist in '
              '.rz with a shape layer', flush=True)

        # L77: conversions.
        l77 = {}
        cdoc77 = new_lab(16, w=16, h=12)
        shape77 = call('add_shape_layer', document_id=cdoc77, kind='rect', x=3, y=3, w=10, h=6,
                       fill='#00ff00')['layer']
        text77 = call('add_text_layer', document_id=cdoc77, text='Mode', x=1, y=1, size=8,
                      font='Helvetica Neue', color='#ff0000')['layer']

        def described77(doc, what):
            assert 'shape' in row_of(doc, shape77), (what, row_of(doc, shape77))
            assert 'text' in row_of(doc, text77), (what, row_of(doc, text77))

        for target, extra in (('rgb', {'rgb_space': 'srgb'}), ('cmyk', {}),
                              ('gray', {'gray_space': 'gray_gamma_22'})):
            call('convert_mode', document_id=cdoc77, mode=target, **extra)
            assert mode(cdoc77) == target, target
            described77(cdoc77, target)
            l77[target] = row_of(cdoc77, text77)['text'].get('color')
            call('undo', document_id=cdoc77)
            assert mode(cdoc77) == 'lab', target
            described77(cdoc77, f'{target} undone')
        call('convert_precision', document_id=cdoc77, mode='lab8')
        assert mode(cdoc77) == 'lab'
        assert 'shape' in row_of(cdoc77, shape77), row_of(cdoc77, shape77)
        grid77 = codes_at(cdoc77, [(8, 6)], source='layer', layer=shape77)
        assert all(v % 257 == 0 for v in grid77[0]), grid77
        rdoc77 = call('new_document', width=16, height=12, depth=16, profile='srgb',
                      background='white')['document']['id']
        rshape77 = call('add_shape_layer', document_id=rdoc77, kind='rect', x=3, y=3, w=10, h=6,
                        fill='#00ff00')['layer']
        count77 = undo_count(rdoc77)
        call('convert_mode', document_id=rdoc77, mode='lab')
        assert mode(rdoc77) == 'lab' and undo_count(rdoc77) == count77 + 1
        assert 'shape' in row_of(rdoc77, rshape77), row_of(rdoc77, rshape77)
        interior77 = lab_at(rdoc77, 8, 6)
        within(interior77, (0, 255, 0), 16, 'RGB → Lab shape')
        l77.update(grid=grid77, interior=interior77)
        receipts['l77'] = l77
        print('PASS l77: RGB, CMYK and Grayscale conversions and 8 Bits/Channel keep type and '
              'shape layers editable on Lab; RGB → Lab re-renders a shape in one step',
              flush=True)

        # ---- L78–L82: phase 6b-i ----------------------------------------------------------
        close_all()
        points78 = [(0, 0), (3, 1), (5, 3)]

        def layer_codes(doc, index, points=points78):
            return codes_at(doc, points, source='layer', layer=index)

        def filled_layer(doc, name, colour):
            call('new_layer', document_id=doc, name=name)
            index = layer_count(doc) - 1
            call('select_all', document_id=doc)
            call('fill_selection', document_id=doc, layer=index, color=colour)
            call('deselect', document_id=doc)
            return index

        l78 = {}
        doc78 = new_lab(16, w=6, h=4, background=COLOUR66)
        filled_layer(doc78, 'Top', TOP78)
        base78, comp78 = layer_codes(doc78, 0), codes_at(doc78, points78)
        before78 = undo_count(doc78)
        call('apply_image', document_id=doc78, layer=0, target='lightness', source='merged',
             source_plane='a', blend_mode='Multiply')
        assert undo_count(doc78) == before78 + 1, 'apply_image is one undo step'
        assert mode(doc78) == 'lab'
        got78 = layer_codes(doc78, 0)
        for b, s, g in zip(base78, comp78, got78):
            assert abs(g[0] - b[0] * s[1] / 65535) <= CHANNEL_BOUND78, (b, s, g)
            assert g[1:] == b[1:], (b, g)
        screen_base = got78
        call('apply_image', document_id=doc78, layer=0, target='layer', source='merged',
             source_plane='lab', blend_mode='Screen')
        screened78 = layer_codes(doc78, 0)
        for b, s, g in zip(screen_base, comp78, screened78):
            for c in range(3):
                want = b[c] + s[c] - b[c] * s[c] / 65535
                assert abs(g[c] - want) <= CHANNEL_BOUND78, (c, b, s, g)
        channels78 = len(call('list_channels', document_id=doc78)['channels'])
        call('calculations', document_id=doc78, source1_plane='lightness', source2_plane='b',
             blend_mode='Multiply', name='Calc78')
        assert len(call('list_channels', document_id=doc78)['channels']) == channels78 + 1
        colour78 = call('apply_image', fail=True, document_id=doc78, layer=0, blend_mode='Color')
        assert colour78 == CHANNEL_MATH_LIMITATION, colour78
        red78 = call('apply_image', fail=True, document_id=doc78, layer=0, source_plane='red')
        assert red78 == SOURCE_PLANE_REFUSAL78, red78
        l78.update(multiply=got78, screen=screened78, refusals=[colour78, red78])
        receipts['l78'] = l78
        print('PASS l78: Apply Image blends each Lab channel alone in one step; Calculations adds '
              'a channel; Color and a red source plane refuse by name', flush=True)

        l79 = {}
        doc79 = new_lab(16, w=6, h=4, background=COLOUR66)
        filled_layer(doc79, 'B', TOP78)
        filled_layer(doc79, 'C', THIRD79)
        members79 = [layer_codes(doc79, i) for i in range(3)]
        before79 = undo_count(doc79)
        reply = call('stack_mode', document_id=doc79, layers=[0, 1, 2], mode='median',
                     name='Median')
        assert undo_count(doc79) == before79 + 1, reply
        assert mode(doc79) == 'lab'
        median79 = layer_codes(doc79, reply['layer'])
        for k, got in enumerate(median79):
            for c in range(3):
                want = sorted(m[k][c] for m in members79)[1]
                assert got[c] == want, (k, c, got, members79)
        ndoc79 = new_lab(16, w=24, h=16, background=COLOUR66)
        call('brush_stroke', document_id=ndoc79, layer=0, points=[[2, 2], [21, 13]], size=3,
             hardness=100, color='#000000')
        call('brush_stroke', document_id=ndoc79, layer=0, points=[[2, 13], [21, 2]], size=3,
             hardness=100, color='#ffffff')
        call('duplicate_layer', document_id=ndoc79, layer=0)
        call('duplicate_layer', document_id=ndoc79, layer=0)
        before79 = undo_count(ndoc79)
        noise79 = call('reduce_noise_from_stack', document_id=ndoc79, layers=[0, 1, 2],
                       mode='median')
        assert noise79.get('ok') and mode(ndoc79) == 'lab', noise79
        assert undo_count(ndoc79) == before79 + 1, noise79
        l79.update(median=median79, noise={k: noise79.get(k) for k in ('layer', 'mode')})
        receipts['l79'] = l79
        print('PASS l79: Stack Modes median is per channel on the Lab codes in one step; Reduce '
              'Noise from Stack is one step', flush=True)

        l80 = {}
        lab80 = new_lab(16, w=4, h=2, background=COLOUR66)
        tiff80 = folder / 'stack-lab16.tif'
        call('save_copy', document_id=lab80, path=str(tiff80), format='tiff')
        png80 = folder / 'stack-srgb8.png'
        png(png80, 4, 2, 8, 2, [v for p in PATCHES for v in p])
        stack80 = call('load_files_into_stack', paths=[str(tiff80), str(png80)])
        sdoc80 = stack80['document']['id']
        assert mode(sdoc80) == 'lab', stack80
        points80 = [(x, y) for y in range(2) for x in range(4)]
        odoc80 = call('open_document', path=str(png80))['document']['id']
        call('convert_mode', document_id=odoc80, mode='lab')
        assert mode(odoc80) == 'lab'
        stacked80 = layer_codes(sdoc80, 1, points80)
        direct80 = codes_at(odoc80, points80)
        worst80 = max(abs(round(a / 257) - round(b / 257))
                      for p, q in zip(stacked80, direct80) for a, b in zip(p[:3], q[:3]))
        assert worst80 <= STACK_BOUND80, (worst80, stacked80, direct80)
        hdr80 = call('merge_layers_to_hdr', document_id=sdoc80, layers=[0, 1], evs=[0, 1],
                     align=False)
        assert hdr80.get('depth') == 'rgba32_float', hdr80
        assert 'prophoto' in str(hdr80.get('profile')).lower(), hdr80
        assert mode(hdr80['document']['id']) == 'rgb', hdr80
        assert mode(sdoc80) == 'lab', 'the source stays Lab'
        l80.update(worst8=worst80, profile=hdr80.get('profile'))
        receipts['l80'] = l80
        print('PASS l80: Load Files into Stack keeps a Lab stack, converting the PNG by Mode ▸ Lab '
              'Color; Merge to HDR from Lab answers 32-bit ProPhoto RGB', flush=True)

        l81 = {}
        doc81 = call('new_document', width=12, height=8, color_mode='lab', depth=16,
                     background=BACK81)['document']['id']
        rects81 = {}
        for name, (x, y, w, h), colour in (('white', (1, 1, 3, 3), '#ffffff'),
                                            ('red', (7, 3, 4, 3), RED81)):
            call('select_rect', document_id=doc81, x=x, y=y, width=w, height=h)
            rects81[name] = call('get_selection', document_id=doc81)['bounds']
            call('fill_selection', document_id=doc81, color=colour)
        call('deselect', document_id=doc81)
        for name, rng in (('white', 'highlights'), ('red', 'reds')):
            reply = call('select_color_range', document_id=doc81, range=rng)
            assert reply.get('ok') is True, (rng, reply)
            bounds = call('get_selection', document_id=doc81)['bounds']
            assert bounds == rects81[name], (rng, bounds, rects81[name])
            l81[rng] = bounds
        for rng in ('skin_tones', 'out_of_gamut'):
            reply = call('select_color_range', document_id=doc81, range=rng)
            assert reply.get('ok') is True, (rng, reply)
            l81[rng] = reply.get('selection_empty')
        assert mode(doc81) == 'lab'
        receipts['l81'] = l81
        print('PASS l81: Color Range highlights and reds select their patches on Lab; Skin Tones '
              'and Out Of Gamut run', flush=True)

        l82 = {}
        doc82 = call('new_document', width=12, height=8, color_mode='lab', depth=16,
                     background=BACK82)['document']['id']
        call('select_rect', document_id=doc82, x=3, y=2, width=4, height=3)
        call('fill_selection', document_id=doc82, color=PATCH82)
        patch82 = codes_at(doc82, [(4, 3)])[0]
        before82 = codes_at(doc82, GRID82)
        for target, moved in (('lightness', 0), ('a', 1)):
            call('select_rect', document_id=doc82, x=3, y=2, width=4, height=3)
            count = undo_count(doc82)
            call('move_selected_pixels', document_id=doc82, layer=0, dx=2, dy=1, target=target)
            assert undo_count(doc82) == count + 1, target
            assert mode(doc82) == 'lab', target
            after = codes_at(doc82, GRID82)
            landing = after[GRID82.index((8, 5))]
            back = before82[GRID82.index((8, 5))]
            assert landing[moved] == patch82[moved], (target, landing, patch82)
            for c in range(3):
                if c != moved:
                    assert landing[c] == back[c], (target, landing, back)
                    assert [p[c] for p in after] == [p[c] for p in before82], (target, c)
            l82[target] = landing
            call('undo', document_id=doc82)
            assert codes_at(doc82, GRID82) == before82, (target, 'undo')
        receipts['l82'] = l82
        print('PASS l82: moving the selected pixels of Lightness or a moves that plane alone, '
              'one undo step', flush=True)
        # ---- L83–L85: phase 6a-ii --------------------------------------------------------
        close_all()

        def counted(doc, tool, **arguments):
            """`tool` adds exactly one undo step and the document stays as it was: its reply."""
            count, was = undo_count(doc), mode(doc)
            reply = call(tool, document_id=doc, **arguments)
            assert undo_count(doc) == count + 1, (tool, reply)
            assert mode(doc) == was, tool
            return reply

        def rows83(doc):
            return [{k: r.get(k) for k in ('name', 'opacity', 'visible', 'blend_mode',
                                           'offset_x', 'offset_y')}
                    for r in call('get_document', document_id=doc)['layers']]

        def comp_names(doc):
            return [c['name'] for c in call('list_layer_comps', document_id=doc)['comps']]

        def centre16(doc, x, y):
            return call('sample_pixel', document_id=doc, x=x, y=y)['center_rgba16']

        # L83: comps, links, snapshots.
        l83 = {}
        cdoc83 = new_lab(16, w=12, h=8, background=COLOUR66)
        call('new_layer', document_id=cdoc83, name='Top')
        call('select_rect', document_id=cdoc83, x=2, y=2, width=7, height=4)
        call('fill_selection', document_id=cdoc83, layer=1, color='#c8285a')
        call('deselect', document_id=cdoc83)
        counted(cdoc83, 'save_layer_comp', name='A')
        rows_before83, render_before83 = rows83(cdoc83), render(cdoc83)[2]
        call('set_layer_properties', document_id=cdoc83, index=1, visible=False, opacity=0.5)
        assert rows83(cdoc83) != rows_before83, 'the properties changed nothing'
        assert render(cdoc83)[2] != render_before83, 'the properties changed the render'
        applied83 = counted(cdoc83, 'apply_layer_comp', name='A')
        assert rows83(cdoc83) == rows_before83, (rows83(cdoc83), rows_before83)
        assert render(cdoc83)[2] == render_before83, 'apply restores the render'
        counted(cdoc83, 'link_layers', layers=[0, 1])
        linked83 = [r.get('linked') for r in call('get_document', document_id=cdoc83)['layers']]
        assert linked83[0] and linked83[0] == linked83[1], linked83
        counted(cdoc83, 'unlink_layers', layers=[0, 1])
        rows_unlinked83 = call('get_document', document_id=cdoc83)['layers']
        assert all('linked' not in r for r in rows_unlinked83), rows_unlinked83
        snap83 = call('create_snapshot', document_id=cdoc83, name='S')['snapshot_id']
        snapped83 = render(cdoc83)[2]
        call('brush_stroke', document_id=cdoc83, points=[[1, 1], [10, 6]], size=4, hardness=100,
             color='#ffffff')
        assert render(cdoc83)[2] != snapped83, 'the stroke changed nothing'
        counted(cdoc83, 'restore_snapshot', snapshot_id=snap83)
        assert render(cdoc83)[2] == snapped83, 'restore_snapshot restores the render'
        rdoc83 = call('new_document', width=12, height=8, depth=16, profile='srgb',
                      background='#808080')['document']['id']
        call('save_layer_comp', document_id=rdoc83, name='RGB')
        call('convert_mode', document_id=rdoc83, mode='lab')
        assert mode(rdoc83) == 'lab' and comp_names(rdoc83) == ['RGB'], comp_names(rdoc83)
        l83.update(applied=applied83, linked=linked83, snapshot=snap83)
        receipts['l83'] = l83
        print('PASS l83: layer comps, links and snapshots run on Lab in one undo step each; '
              'RGB → Lab carries a comp', flush=True)

        # L84: Clear and the planes.
        l84 = {}
        kdoc84 = new_lab(16, w=12, h=8, background=COLOUR66)
        corner84 = centre16(kdoc84, 0, 0)
        call('select_rect', document_id=kdoc84, x=3, y=2, width=5, height=4)
        counted(kdoc84, 'clear_selection', layer=0)
        cleared84 = centre16(kdoc84, 4, 3)
        assert cleared84[3] == 0, cleared84
        assert centre16(kdoc84, 0, 0) == corner84, (centre16(kdoc84, 0, 0), corner84)
        mdoc84 = new_lab(16, w=12, h=8, background=COLOUR66)
        before84 = centre16(mdoc84, 2, 3)
        call('select_rect', document_id=mdoc84, x=2, y=2, width=4, height=3)
        counted(mdoc84, 'move_selected_pixels', target='a', dx=2, dy=0, background='#ff0000')
        hole84 = centre16(mdoc84, 2, 3)
        want84 = round(srgb_to_lab((255, 0, 0))[1] + 128) * 257
        assert abs(hole84[1] - want84) <= 257, (hole84, want84)
        assert hole84[0] == before84[0] and hole84[2] == before84[2], (hole84, before84)
        tdoc84 = new_lab(16, w=12, h=8, background=COLOUR66)
        call('select_rect', document_id=tdoc84, x=4, y=3, width=2, height=2)
        call('fill_selection', document_id=tdoc84, color='#ffffff')
        call('select_rect', document_id=tdoc84, x=2, y=2, width=6, height=4)
        counted(tdoc84, 'transform_selected_pixels', target='lightness', scale=1.25)
        l84.update(cleared=cleared84, hole=hole84, want=want84)
        receipts['l84'] = l84
        print('PASS l84: Clear clears a Lab layer\'s selection; the a and Lightness planes\' '
              'selected pixels move and transform, the hole the background\'s a* code',
              flush=True)

        # L85: Convert to Profile → Gray.
        l85 = {}
        gdoc85 = new_lab(16, background=COLOUR66)
        before85 = undo_count(gdoc85)
        reply = call('convert_profile', document_id=gdoc85, profile='dot_gain_20')
        assert reply['color_mode'] == 'gray' and reply['depth'] == 16, reply
        assert undo_count(gdoc85) == before85 + 1, reply
        call('undo', document_id=gdoc85)
        assert mode(gdoc85) == 'lab'
        call('redo', document_id=gdoc85)
        assert mode(gdoc85) == 'gray'
        via85 = new_lab(16, background=COLOUR66)
        call('convert_mode', document_id=via85, mode='gray', gray_space='dot_gain_20')
        codes85, via_codes85 = codes_at(gdoc85, POINTS67), codes_at(via85, POINTS67)
        assert codes85 == via_codes85, (codes85, via_codes85)
        l85.update(reply={k: reply.get(k) for k in ('color_mode', 'depth', 'profile')},
                   codes=codes85)
        receipts['l85'] = l85
        print('PASS l85: Convert to Profile into Dot Gain 20% is Image ▸ Mode ▸ Grayscale\'s '
              'conversion in one step', flush=True)

        # ---- L86–L89: phase 6b-ii --------------------------------------------------------
        close_all()

        made86 = []

        def texture_png(x0, x1):
            """The texture's columns x0..x1 as a NEW 8-bit PNG (an open path answers its open
            document, so every pair is its own file)."""
            path = folder / f'texture86-{len(made86)}.png'
            made86.append(path)
            png(path, x1 - x0, TEX_H, 8, 2,
                [texture86(x, y, c) for y in range(TEX_H) for x in range(x0, x1) for c in range(3)])
            return path

        def lab_open(path):
            doc = call('open_document', path=str(path))['document']['id']
            call('convert_precision', document_id=doc, mode='rgba16', structure='preserve')
            call('convert_mode', document_id=doc, mode='lab')
            assert mode(doc) == 'lab', path
            return doc

        def pair86():
            doc = lab_open(texture_png(0, TEX_W))
            call('duplicate_layer', document_id=doc, index=0)
            call('move_layers', document_id=doc, layers=[1], dx=3, dy=2)
            return doc

        # L86: Auto-Align.
        l86 = {}
        adoc86 = pair86()
        count86 = undo_count(adoc86)
        aligned86 = call('auto_align_layers', document_id=adoc86, layers=[0, 1])
        assert aligned86.get('ok') is True, aligned86
        assert undo_count(adoc86) == count86 + 1, 'auto_align_layers is one undo step'
        assert mode(adoc86) == 'lab'
        for option in ('vignette', 'distortion'):
            ok86, reply86 = attempt('auto_align_layers', document_id=pair86(), layers=[0, 1],
                                    **{option: True})
            l86[option] = 'ok' if ok86 else reply86
        l86['aligned'] = sorted(aligned86)
        receipts['l86'] = l86
        print(f'PASS l86: Auto-Align runs on a Lab pair in one undo step; vignette and distortion '
              f'clear the gate {({k: l86[k] for k in ("vignette", "distortion")})}', flush=True)

        # L87: Auto-Blend refuses for good since the close; Merge Panorama.
        l87 = {}
        before87 = layer_count(adoc86)
        stacked87 = call('auto_blend_layers', fail=True, document_id=adoc86, layers=[0, 1],
                         method='stack_images')
        assert stacked87 == UNAVAILABLE_REFUSAL.format('auto_blend_layers'), stacked87
        assert layer_count(adoc86) == before87 and mode(adoc86) == 'lab', stacked87
        tiffs87 = []
        # Widened for 6b-iii (setup only): at (0, 100) and (60, TEX_W), and at the first widening
        # (0, 110) and (50, TEX_W), Merge Panorama stopped at its own Auto-Align refusal, so the lab
        # check never ran.
        for x0, x1 in ((0, 120), (40, TEX_W)):
            crop87 = lab_open(texture_png(x0, x1))
            path87 = folder / f'merge_panorama87-{x0}.tif'
            call('save_copy', document_id=crop87, path=str(path87), format='tiff')
            tiffs87.append(str(path87))
        ok87, merged87 = attempt('merge_panorama', paths=tiffs87)
        if ok87:
            assert mode(merged87['document']['id']) == 'lab', merged87
        l87.update(auto_blend=stacked87, merge_panorama='ok' if ok87 else merged87)
        receipts['l87'] = l87
        print(f'PASS l87: Auto-Blend refuses for good on Lab; Merge Panorama of Lab '
              f'TIFFs clears the gate ({l87["merge_panorama"]})', flush=True)

        # L88: the selections.
        l88 = {}
        sdoc88 = new_lab(16, w=24, h=16, background=BACK88)
        call('select_rect', document_id=sdoc88, x=6, y=4, width=8, height=6)
        rect88 = call('get_selection', document_id=sdoc88)['bounds']
        call('fill_selection', document_id=sdoc88, color=PATCH88)
        for step, arguments in (('set_quick_mask', {'active': True}),
                                ('fill_selection', {'color': '#808080', 'target': 'quick_mask'}),
                                ('set_quick_mask', {'active': False})):
            # `call` asserts the reply is not an error.
            l88[f'{step}_{len(l88)}'] = call(step, document_id=sdoc88, **arguments)
        call('deselect', document_id=sdoc88)
        quick88 = call('quick_select', document_id=sdoc88, seeds=[[10, 7]])
        bounds88 = call('get_selection', document_id=sdoc88)['bounds']
        assert bounds88 == rect88, (bounds88, rect88, quick88)
        layers88 = layer_count(sdoc88)
        refined88 = call('refine_selection', document_id=sdoc88, decontaminate=True, feather=2,
                         output='new_layer')
        assert layer_count(sdoc88) == layers88 + 1 and mode(sdoc88) == 'lab', refined88
        models88 = {
            'select_subject': attempt('select_subject', document_id=sdoc88),
            'select_sky': attempt('select_sky', document_id=sdoc88),
            'select_person_parts': attempt('select_person_parts', document_id=sdoc88,
                                           parts=['hair']),
            'object_finder_select': attempt('object_finder_select', document_id=sdoc88,
                                            box={'x': 2, 'y': 2, 'width': 12, 'height': 8}),
        }
        l88.update(rect=rect88, quick=bounds88, refined=refined88,
                   models={k: 'ok' if v[0] else v[1] for k, v in models88.items()})
        receipts['l88'] = l88
        print(f'PASS l88: Quick Mask, Quick Selection and Select and Mask run on Lab; the model '
              f'selections clear the gate {l88["models"]}', flush=True)

        # L89: nit (b), the Gray choices named.
        bogus89 = call('convert_profile', fail=True, document_id=new_lab(16), profile='bogus')
        assert 'working_gray' in bogus89 and 'dot_gain_20' in bogus89, bogus89
        receipts['l89'] = bogus89
        print('PASS l89: an unknown convert_profile profile names the Gray choices', flush=True)

        # ---- L90–L92: phase 6b-iii -------------------------------------------------------
        def lab_of(doc, x, y):
            got = call('sample_pixel', document_id=doc, x=x, y=y)['lab']
            return got['l'], got['a'], got['b']

        if model_ready:
            # L90: Super Zoom and JPEG Artifacts Removal on a neutral Lab document.
            zdoc90 = new_lab(16, w=16, h=12, background='#808080')
            source90 = lab_of(zdoc90, 8, 6)
            # Enhance off (setup, module doc): Real-ESRGAN's own tone shift on a flat field is not
            # the Lab path's; the stage still runs on the Display P3 view and returns by the Mode rule.
            zoom90 = call('super_zoom', document_id=zdoc90, layer=0, zoom=2, enhance_details=False)
            assert zoom90.get('ok') is True, zoom90
            zoomed90 = zoom90['document_id']
            assert mode(zoomed90) == 'lab', zoom90
            assert lab_depth(zoomed90) == 16, summary(zoomed90)
            size90 = call('get_document', document_id=zoomed90)
            assert (size90['width'], size90['height']) == (32, 24), size90
            centre90 = lab_of(zoomed90, 16, 12)
            assert abs(centre90[1]) <= AB_BOUND90 and abs(centre90[2]) <= AB_BOUND90, centre90
            assert abs(centre90[0] - source90[0]) <= L_BOUND90, (centre90, source90)
            before90 = undo_count(zdoc90)
            jpeg90 = call('jpeg_artifacts_removal', document_id=zdoc90, layer=0)
            assert undo_count(zdoc90) == before90 + 1, jpeg90
            assert mode(zdoc90) == 'lab', jpeg90
            restored90 = lab_of(zdoc90, 8, 6)
            assert abs(restored90[1]) <= AB_BOUND90 and abs(restored90[2]) <= AB_BOUND90, \
                restored90
            receipts['l90'] = dict(source=source90, zoom=zoom90, centre=centre90, jpeg=jpeg90,
                                   restored=restored90)
            print(f'PASS l90: Super Zoom makes a 16-bit Lab document (centre {centre90}) and JPEG '
                  f'Artifacts Removal is one step on Lab (centre {restored90})', flush=True)

            # L91: Colorize on Lab; Sky Replacement refuses for good.
            cdoc91 = lab_open(texture_png(0, TEX_W))
            layers91 = layer_count(cdoc91)
            colorized91 = call('neural_colorize', document_id=cdoc91, layer=0, output='new_layer')
            assert colorized91.get('ok') is True, colorized91
            assert layer_count(cdoc91) == layers91 + 1 and mode(cdoc91) == 'lab', colorized91
            fresh91 = call('neural_colorize', document_id=cdoc91, layer=0, output='new_document')
            assert fresh91.get('ok') is True and mode(fresh91['document_id']) == 'lab', fresh91
            sky91 = call('sky_replacement', fail=True, document_id=cdoc91, layer=0)
            assert sky91 == RGB_ONLY_REFUSAL.format('Sky Replacement'), sky91
            receipts['l91'] = dict(new_layer=colorized91, new_document=fresh91, sky=sky91)
            print('PASS l91: Colorize writes a new layer and a new Lab document on Lab; Sky '
                  'Replacement refuses for good', flush=True)
        else:
            print('SKIP l90–l91: the model fixtures are absent', flush=True)

        # L92: the generative tools clear the gate (dead-port providers, a private DR_CLOUD_DIR).
        gdoc92 = new_lab(16, w=24, h=16, background=BACK88)
        call('select_rect', document_id=gdoc92, x=4, y=4, width=8, height=6)
        fill92 = attempt('generative_fill', document_id=gdoc92, layer=0, prompt='a stone')
        call('deselect', document_id=gdoc92)
        expand92 = attempt('generative_expand', document_id=gdoc92, layer=0)
        variations92 = attempt('generative_variations', document_id=gdoc92, layer=0)
        assert mode(gdoc92) == 'lab'
        receipts['l92'] = {name: 'ok' if result[0] else result[1] for name, result in (
            ('generative_fill', fill92), ('generative_expand', expand92),
            ('generative_variations', variations92))}
        print(f'PASS l92: the generative tools clear the gate on Lab {receipts["l92"]}', flush=True)

        # ---- L93–L98: the close ----------------------------------------------------------
        close_all()

        def answer(tool, **arguments):
            """(is_error, text) of one call, whatever it answers."""
            result = app.client.rpc('tools/call', {'name': tool, 'arguments': arguments})
            text = next((c['text'] for c in result.get('content', []) if c['type'] == 'text'), '')
            return bool(result.get('isError')), text or ''

        def two_layer_lab():
            doc = new_lab(16, w=12, h=8, background=COLOUR66)
            call('new_layer', document_id=doc, name='Top')
            call('select_rect', document_id=doc, x=2, y=2, width=7, height=4)
            call('fill_selection', document_id=doc, layer=1, color='#c8285a')
            call('deselect', document_id=doc)
            return doc

        # L93: the 58 tools the gate refused naming a phase.
        rdoc93 = new_lab(16, w=12, h=8)
        l93, phased93 = {'lifted': {}, 'refused': {}}, []
        for tool in CLOSE_LIFTED:
            failed, text = answer(tool, document_id=rdoc93)
            if failed:
                assert not any(part in text for part in GATE_PARTS), (tool, text)
            if '#1 phase' in text:
                phased93.append((tool, text))
            l93['lifted'][tool] = text[:120] if failed else 'ok'
        for tool, sentence in CLOSE_REFUSED.items():
            failed, text = answer(tool, document_id=rdoc93)
            assert failed and text == sentence, (tool, text, sentence)
            if '#1 phase' in text:
                phased93.append((tool, text))
            l93['refused'][tool] = text
        assert phased93 == [], phased93
        assert len(CLOSE_LIFTED) + len(CLOSE_REFUSED) == 58
        precision93 = call('get_precision', document_id=rdoc93)
        assert 'later_phases' not in precision93 and 'default_phase' not in precision93, \
            sorted(precision93)
        receipts['l93'] = l93
        print('PASS l93: the 34 lifted tools clear the gate, the 24 others refuse for good by '
              'name, and no reply names a #1 phase', flush=True)
        close_all()

        # L94: structure, metadata, views and workspaces.
        doc94 = two_layer_lab()
        l94 = {}
        l94['arrange_layer'] = call('arrange_layer', document_id=doc94, layer=1, to='back')
        l94['find_layers'] = call('find_layers', document_id=doc94, kind='raster')
        call('add_layer_mask', document_id=doc94, layer=1, kind='reveal_all')
        l94['get_mask_properties'] = call('get_mask_properties', document_id=doc94, layer=1)
        l94['auto_select_layer'] = call('auto_select_layer', document_id=doc94, x=3, y=3)
        call('group_layers', document_id=doc94, layers=[1], name='Group94')
        group94 = call('find_layers', document_id=doc94, kind='group')
        rows94 = group94.get('layers') or group94.get('matches') or group94.get('results') or []
        assert len(rows94) == 1, group94
        l94['merge_group'] = call('merge_group', document_id=doc94, layer=rows94[0]['index'])
        count94 = undo_count(doc94)
        call('set_resolution', document_id=doc94, ppi_x=150)
        assert undo_count(doc94) == count94 + 1, 'set_resolution is one undo step'
        meta94 = call('get_metadata', document_id=doc94)
        assert meta94['ppi_x'] == 150 and meta94['ppi_y'] == 150, meta94
        call('set_file_info', document_id=doc94, copyright='Lab close 547')
        info94 = call('get_file_info', document_id=doc94)
        assert info94['copyright'] == 'Lab close 547', info94
        l94['set_ruler_origin'] = call('set_ruler_origin', document_id=doc94, x=2, y=3)
        l94['set_history_budget'] = call('set_history_budget', document_id=doc94,
                                         bytes=256 * 1024 * 1024)
        l94['list_lens_profiles'] = call('list_lens_profiles', document_id=doc94)
        l94['detect_horizon'] = call('detect_horizon', document_id=doc94)
        opened94 = call('open_second_view', document_id=doc94)
        assert opened94['view']['view'] == 2 and len(opened94['views']) == 2, opened94
        set94 = call('set_view', document_id=doc94, view=2, zoom=1)
        assert set94['zoom'] == 1.0, set94
        assert call('get_view', document_id=doc94, view=2)['zoom'] == 1.0
        closed94 = call('close_second_view', document_id=doc94, view=2)
        assert len(closed94['views']) == 1, closed94
        call('save_workspace', document_id=doc94, name='Lab close')
        l94['restore_workspace'] = call('restore_workspace', document_id=doc94, name='Lab close')
        assert mode(doc94) == 'lab'
        receipts['l94'] = sorted(l94)
        print('PASS l94: the lifted layer, metadata, ruler, history, lens, horizon, view and '
              'workspace tools answer on a Lab document; set_resolution is one undo step',
              flush=True)

        # L95: Distort, Modify Native Plane, Invert Channel and the luminosity masks.
        l95 = {}
        ddoc95 = lab_open(texture_png(0, 48))
        counted(ddoc95, 'distort_layer', layer=0,
                corners=[[1, 1], [46, 0], [44, TEX_H - 1], [2, TEX_H - 2]])
        mdoc95 = new_lab(16, w=12, h=8)
        call('select_rect', document_id=mdoc95, x=5, y=3, width=2, height=2)
        call('add_layer_mask', document_id=mdoc95, layer=0, kind='from_selection')
        call('deselect', document_id=mdoc95)
        _, _, mask95 = image('render', document_id=mdoc95, channel='mask', layer=0)
        call('modify_scalar', document_id=mdoc95, layer=0, target='mask', operation='grow',
             radius=2)
        _, _, grown95 = image('render', document_id=mdoc95, channel='mask', layer=0)
        assert grown95 != mask95, 'modify_scalar grew the mask'
        red95 = call('modify_scalar', fail=True, document_id=mdoc95, layer=0, target='red',
                     operation='grow')
        assert red95 == PLANE_TARGET_REFUSAL, red95
        call('add_channel', document_id=mdoc95, name='Matte')
        l95['invert_channel'] = counted(mdoc95, 'invert_channel', channel='Matte')
        ramp95 = folder / 'ramp95.png'
        png(ramp95, 64, 1, 16, 2, [round(x * 65535 / 63) for x in range(64) for _ in range(3)])
        rdoc95 = call('open_document', path=str(ramp95), precision='preserve')['document']['id']
        call('convert_mode', document_id=rdoc95, mode='lab')
        assert mode(rdoc95) == 'lab'
        before95 = [c['name'] for c in call('list_channels', document_id=rdoc95)['channels']]
        call('add_luminosity_masks', document_id=rdoc95)
        after95 = [c['name'] for c in call('list_channels', document_id=rdoc95)['channels']]
        assert after95[:len(before95)] == before95, (before95, after95)
        assert after95[len(before95):] == LUMINOSITY_NAMES, after95
        _, _, lights95 = image('render', document_id=rdoc95, channel='Lights 1')
        _, _, darks95 = image('render', document_id=rdoc95, channel='Darks 1')
        lights95, darks95 = [p[0] for p in lights95], [p[0] for p in darks95]
        assert all(b >= a for a, b in zip(lights95, lights95[1:])), lights95
        assert all(b <= a for a, b in zip(darks95, darks95[1:])), darks95
        assert lights95[-1] > lights95[0] and darks95[0] > darks95[-1], (lights95, darks95)
        l95.update(plane=red95, lights=lights95[::9], darks=darks95[::9])
        receipts['l95'] = l95
        print('PASS l95: Distort is one undo step; Modify Native Plane grows a mask and refuses '
              'red; Invert Channel runs; the luminosity masks follow L*', flush=True)

        # L96: Place and Live Photo.
        place96 = folder / 'place96.png'
        png(place96, len(PATCHES), 1, 8, 2, [v for p in PATCHES for v in p])
        pdoc96 = new_lab(16, w=len(PATCHES), h=1)
        counted(pdoc96, 'place_image', path=str(place96))
        odoc96 = call('open_document', path=str(place96))['document']['id']
        call('convert_precision', document_id=odoc96, mode='rgba16', structure='preserve')
        call('convert_mode', document_id=odoc96, mode='lab')
        _, _, placed96, _ = render(pdoc96)
        _, _, oracle96, _ = render(odoc96)
        worst96 = max(abs(placed96[i][c] - oracle96[i][c]) for i in range(len(PATCHES))
                      for c in range(3))
        assert worst96 <= PLACE_BOUND8, (worst96, placed96, oracle96)
        dng96 = folder / 'x.dng'
        dng96.write_bytes(b'')
        raw96 = call('place_image', fail=True, document_id=pdoc96, path=str(dng96))
        assert RAW_PLACE_REFUSAL in raw96, raw96
        from verify_phase12e2b_placement import author_movie
        movie96, reason96 = author_movie(folder)
        if movie96 is None:
            live96 = f'NOT RUN: {reason96}'
            print(f'NOT RUN l96 add_live_photo_layer: {reason96}', flush=True)
        else:
            ldoc96 = new_lab(16, w=64, h=48)
            call('add_live_photo_layer', document_id=ldoc96, path=str(movie96))
            assert mode(ldoc96) == 'lab'
            live96 = 'ok'
        receipts['l96'] = {'worst8': worst96, 'raw': raw96, 'live_photo': live96}
        print(f'PASS l96: a placed colour PNG lands within {worst96} code of its Mode ▸ Lab '
              'Color oracle; a camera RAW file refuses by name', flush=True)

        # L97: Export Layers, PDF, the contact sheet and Separate Color.
        close_all()
        colour97 = [128, 64, 200]
        bare97 = call('separate_color', rgb=colour97)
        edoc97 = two_layer_lab()
        lab97 = call('separate_color', document_id=edoc97, rgb=colour97)
        keys97 = ('ink_percent', 'out_of_gamut', 'delta_e', 'in_gamut', 'in_gamut_hex')
        assert {k: lab97.get(k) for k in keys97} == {k: bare97.get(k) for k in keys97}, \
            (lab97, bare97)
        layers97 = folder / 'layers97'
        layers97.mkdir()
        exported97 = call('export_layers', document_id=edoc97, items='all',
                          settings={'format': 'png', 'bit_depth': 8, 'profile': 'srgb'},
                          destination=str(layers97), pattern='{index}-{layer}')
        paths97 = [Path(item['path']) for item in exported97['items'] if item.get('path')]
        assert len(paths97) == 2, exported97
        assert all(p.read_bytes()[:8] == b'\x89PNG\r\n\x1a\n' for p in paths97), paths97
        pdf97 = folder / 'lab97.pdf'
        call('export_pdf', document_id=edoc97, path=str(pdf97))
        assert pdf97.read_bytes()[:4] == b'%PDF', pdf97.read_bytes()[:8]
        before97 = len(call('list_documents')['documents'])
        call('contact_sheet', document_id=edoc97, items='all', output={'kind': 'document'})
        assert len(call('list_documents')['documents']) == before97 + 1
        receipts['l97'] = {'separate_color': {k: lab97.get(k) for k in keys97},
                           'pngs': [str(p) for p in paths97], 'pdf_bytes': pdf97.stat().st_size}
        print('PASS l97: Export Layers writes PNGs, PDF and the contact sheet run, and Separate '
              'Color reads rgb as sRGB on Lab', flush=True)

        # L98: the take's refuse_for_good rows.
        bdoc98 = two_layer_lab()
        blend98 = call('auto_blend_layers', fail=True, document_id=bdoc98, layers=[0, 1],
                       method='stack_images')
        assert blend98 == UNAVAILABLE_REFUSAL.format('auto_blend_layers'), blend98
        items98 = call('get_precision', document_id=bdoc98)['supported_menu_items']
        assert 'showAutoBlendLayers:' not in items98, items98
        receipts['l98'] = {'auto_blend_layers': blend98}
        print('PASS l98: Auto-Blend Layers refuses for good, and its menu item '
              'is disabled', flush=True)

        # ---- L99–L101: the Magic Wand, Similar and Grow ---------------------------------
        close_all()
        everywhere99 = [(x, y) for y in range(H99) for x in range(W99)]

        def scene99(depth):
            """(document, row-major u16 codes) of the module doc's SCENE at `depth`."""
            doc = new_lab(depth, w=W99, h=H99, background='#808080')
            for x, y, w, h, colour in SCENE99:
                call('select_rect', document_id=doc, x=x, y=y, width=w, height=h)
                call('fill_selection', document_id=doc, color=colour)
            call('deselect', document_id=doc)
            return doc, [p[:3] for p in codes_at(doc, everywhere99)]

        def selected99(doc):
            points = [{'x': x, 'y': y} for x, y in everywhere99]
            samples = call('get_selection', document_id=doc, points=points)['samples']
            coverage = [s['coverage16'] for s in samples]
            assert all(v in (0, 65535) for v in coverage), coverage
            return [v == 65535 for v in coverage]

        l99, l100, l101 = {}, {}, {}
        for depth in (8, 16):
            doc99, codes99 = scene99(depth)
            sizes99 = []
            for t in TOLERANCES99:
                for contiguous in (True, False):
                    call('deselect', document_id=doc99)
                    counted(doc99, 'select_magic_wand', x=0, y=0, tolerance=t,
                            contiguous=contiguous)
                    want = region99(codes99, [0], t, depth, contiguous)
                    got = selected99(doc99)
                    assert got == want, (depth, t, contiguous, got, want, codes99)
                    sizes99.append(sum(want))
            assert any(0 < s < W99 * H99 for s in sizes99), (depth, sizes99)
            l99[str(depth)] = sizes99
            references100 = [y * W99 + x for y in range(4) for x in range(5)]
            trim100 = len(references100) // 10

            def added100(region):
                # The admitted region is ADDED to the selection (the catalog's rule), so a
                # selected pixel the trimmed range leaves out stays selected.
                return [w or i in references100 for i, w in enumerate(region)]
            differs100 = False
            rectangle100 = added100([False] * (W99 * H99))
            for t in TOLERANCES99:
                for contiguous in (False, True):
                    call('select_rect', document_id=doc99, x=0, y=0, width=5, height=4)
                    want = added100(region99(codes99, references100, t, depth, contiguous,
                                             trim100))
                    differs100 |= want != added100(region99(codes99, references100, t, depth,
                                                            contiguous))
                    if want == rectangle100:
                        # Nothing admitted outside the rectangle: unchanged, no undo step.
                        steps = undo_count(doc99)
                        reply = call('select_similar', document_id=doc99, tolerance=t,
                                     contiguous=contiguous)
                        assert reply['changed'] is False, (depth, t, contiguous, reply)
                        assert undo_count(doc99) == steps, (depth, t, contiguous)
                    else:
                        counted(doc99, 'select_similar', tolerance=t, contiguous=contiguous)
                    got = selected99(doc99)
                    assert got == want, (depth, t, contiguous, got, want, codes99)
                    l100[f'{depth}/{t}/{contiguous}'] = sum(want)
            assert differs100, (depth, 'some case trims')
            call('deselect', document_id=doc99)
            for contiguous in (True, False):
                before101 = codes_at(doc99, everywhere99)
                # `fill_mode` color_match: without it a typed contiguous false is the
                # selection-wide fill (the catalog's precision note), not the bucket.
                counted(doc99, 'fill', x=0, y=0, color='#0000ff', tolerance=12,
                        contiguous=contiguous, fill_mode='color_match')
                after101 = codes_at(doc99, everywhere99)
                changed = [a != b for a, b in zip(after101, before101)]
                want = region99(codes99, [0], 12, depth, contiguous)
                assert changed == want, (depth, contiguous, changed, want)
                filled = {tuple(after101[i]) for i in range(len(want)) if want[i]}
                assert len(filled) == 1, (depth, contiguous, filled)
                l101[f'{depth}/{contiguous}'] = sum(want)
                call('undo', document_id=doc99)
                assert codes_at(doc99, everywhere99) == before101, (depth, contiguous)
        receipts['l99'], receipts['l100'], receipts['l101'] = l99, l100, l101
        print(f'PASS l99: the Magic Wand selects the Lab key oracle at 8 and 16 bits {l99}',
              flush=True)
        print(f'PASS l100: Similar and Grow select the trimmed oracle at 8 and 16 bits {l100}',
              flush=True)
        print(f'PASS l101: the Paint Bucket fills the wand oracle\'s region {l101}', flush=True)

    (output / 'receipts.json').write_text(json.dumps(receipts, indent=2, default=str))
    if not model_ready:
        print(f'SKIP l45 model: no verified {MIGAN} in {MODEL_STORE}', flush=True)
        sys.exit(2)
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
