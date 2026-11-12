#!/usr/bin/env python3
"""The #1 phase-1a, 1b, 1c, 2a-ii, 2b, 2c, 2d, 2e, 3a, 3b, 3c, 4, 5a, 5b, 6a-i, 6a-ii and 6b and close MCP drive: Grayscale
documents are made, opened, converted both ways, saved as `.rz` and reopened
over MCP on the Debug app; phase 1b's layer structure, Fill and one-channel
PNG, TIFF and JPEG saves and exports work on them; phase 1c's `get_precision`,
`render` gray channel, Export As / Quick Export and K % answer; phase 2a-ii's
Brush, Eraser, Paint Bucket and Gradient paint gray, phase 2b's adjustments
run and phase 2c's filters, Fade, Actions and retouch tools run; phase 2d's
layer effects, Blend If and alpha-channel targets work; phase 2e's Stroke…,
content-aware tools, Remove, Art History Brush, adjustment-layer carry and HDR
Toning work; phase 3a's Grayscale
PSD, PSB and layered TIFF open works; phase 3b's Grayscale PSD, PSB and
layered TIFF export works; phase 3c's 32-bit Grayscale PSD, PSB and float TIFF
export and Photoshop's own 32-bit Grayscale PSD open work; phase 4's geometry,
selections and copy and paste across modes work; phase 5a's Assign and
Convert to Profile, 8/16/32 Bits/Channel and 32-bit HDR Toning work; phase
5b's Proof Setup / Proof Colors and Mode ▸ CMYK Color from Gray work; phase
6a-i's type, shape and fill layers, paths and vector masks work; phase
6a-ii's layer comps, links, snapshots, Edit ▸ Clear and the RGB/CMYK → Gray
artwork and comp carry work; phase 6b's Histogram, Apply Image, Calculations,
Stack Modes, the stack and HDR tools, the model selections, the neural and
generative tools and the layers clip of a text layer work, and Neural Colorize
and Sky Replacement refuse for good; the close's lifted tools (Arrange, Distort,
the views and workspaces, the Color Samplers, file info and resolution, Modify
Native Plane, Place and Live Photo layers, Export Layers, PDF, the contact sheet
and the 32-bit tone map) work, and the camera RAW, byte and evaluation tools
refuse for good by name; no reply names a #1 phase.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. The instance is owned by this file
(`phase5_support.App` on its own scratch MCP socket: a dummy API key and a
PRIVATE directory for every `DR_*_DIR`) and is terminated by its own context
manager; no Doctored process this file did not start is signalled. It never
launches Photoshop.

Every fixture is written HERE in pure Python under `--output`. The oracle is
written from the definitions: sRGB's published D50 luminance row (the sRGB
profile's s15Fixed16 `rXYZ`/`gXYZ`/`bXYZ` Y values) over the IEC 61966-2-1
decode, then the Gray space's own curve (`Y^e`, Dot Gain d %: e = ln 0.5 /
ln(0.5 − d/100); Gray Gamma γ: 1/γ; sGray the IEC encode). Bounds (the spec's):
≤ 4 codes of 65535 at 16 bits, ≤ 1 code of 255 at 8 bits.

- **G1 New.** `new_document color_mode gray` at 8, 16 and 32 bits answers a
  Grayscale document (`summary.color_mode` "gray"); the 16-bit #ff0000 fill in
  Gray Gamma 2.2 is the oracle's luminance within 4 codes, R = G = B.
- **G2 open.** A gray PNG 8 (untagged), a GrayA PNG 16, a PNG 16 with a Gray
  `iCCP`, a PNG 8 with an `sRGB` chunk and a BlackIsZero TIFF 16 open as
  Grayscale documents holding the file's codes exactly in R = G = B (8-bit
  ×257), alpha kept; the `sRGB` chunk is sGray, the `iCCP` its own name.
- **G3 RGB → Gray.** A 16-bit sRGB PNG of patches converts (`convert_mode
  gray`, Dot Gain 20 % and Gray Gamma 2.2) onto the oracle within 4 codes;
  an 8-bit RGB PNG converts at depth 8 within 1 code, on the ×257 grid; each is
  ONE undo step (undo answers rgb, redo gray).
- **G4 CMYK → Gray.** A CMYK document (New, working CMYK, #4080c0) converts to
  Grayscale whose code is, within 2 8-bit codes, the oracle's gray of the same
  document's RGB Color (sRGB) sample.
- **G5 Gray → RGB.** `convert_mode rgb` answers an RGB document with R = G = B
  and is one undo step.
- **G6 32 bits.** A 32-bit RGB document converts to 32-bit Grayscale and back
  to RGB, each one undo step.
- **G7 flatten.** (Rewritten for phase 6a-ii, whose type layer converts.) An
  8-bit RGB document with a layer in Hue refuses Grayscale with text
  containing "is a layer in the Hue blend mode, which a Grayscale document
  cannot hold" (the close drops "yet"); `flatten: true` converts in one undo
  step.
- **G8 .rz.** `save_copy rz` of a Grayscale document reopens as Grayscale with
  the same samples.
- **G9 refusals.** (The close's rewrite.) `set_resolution` 150 runs and
  keeps the document Grayscale (undone), `get_raw_source` answers
  `RAW_REFUSAL`, `image_size` runs since phase 4 (2 × 2, still Grayscale; undone),
  `get_proof_setup` answers since phase 5b (`proof_colors` false),
  `assign_profile srgb` the RGB-profile sentence
  (phase 5a: text containing `RGB_PROFILE_PART`), and `save_copy webp` the
  save sentence (`GrayDocument.saveRefusal`), each exactly.
- **G10 structure.** On a 16-bit Grayscale document `new_layer`,
  `add_layer_mask`, `set_layer_properties` Multiply + opacity 0.5,
  `set_layer_clipped`, `set_layer_style {"fill_opacity": 0.5}`,
  `stamp_visible`, `merge_down` and `group_layers` each make ONE undo step
  (undo restores the layer list as it was, redo as it became) and the document
  stays `color_mode` gray with R = G = B; `set_layer_properties` Hue and
  Darker Color (not offered in Grayscale, phase 2's take) refuse
  with the host's Grayscale blend sentence (`NativeLayerBlending.
  grayLimitation`: the Grayscale list, no Hue, Luminosity, Darker Color or
  Lighter Color); since phase 2d a drop-shadow style on layer 0 is one
  more undo step and the document stays R = G = B.
- **G11 Fill.** `fill_selection color #ff0000` on a 16-bit Gray Gamma 2.2
  document samples the oracle's luminance code within 4 codes, R = G = B;
  `target mask` `#000000` hides the masked layer (the sample is the backdrop's
  exactly).
- **G12 saves.** `save_copy` png, tiff and jpeg of an 8-bit and a 16-bit
  document (a gray PNG with the drive's Gray `iCCP`): each header holds ONE
  channel (PNG colour type 0 at the document's depth, TIFF SamplesPerPixel 1
  BlackIsZero, JPEG SOF Nf 1 at 8 bits); the embedded ICC (`iCCP` inflated,
  tag 34675, APP2) equals the drive's Gray profile bytes with data colour
  space `GRAY` at offset 16; `open_document` reopens each as Grayscale with
  the same profile name and samples, exact for PNG and TIFF and within 2 of
  255 for JPEG at quality 100. `save_preservation.save_types` of the one-layer
  16-bit document lists public.png and public.tiff but not public.jpeg; after
  `add_layer_mask` on its layer it lists neither, with `losses_by_format`'s
  keys unchanged; the one-layer 8-bit document lists public.jpeg.
- **G13 exports.** `export_image` png 16 and tiff 8 with profile document are
  one channel; profile srgb writes an RGB PNG (colour type 2 or 6).
- **G14 .rz.** `save_copy rz` of the G10 document reopens as Grayscale with
  every sample equal.
- **G15 get_precision.** On New Grayscale (Dot Gain 20 %) at 8, 16 and 32
  bits: `color_mode` gray, `bits_per_channel` 8 / 16 / 32, `mode` gray8 /
  gray16 / gray32; `supported_tools` equals `GRAY_TOOLS` (this file's fixed
  copy of `GrayDocument.tools`, phase 2's ten, phase 2c's thirteen and the
  close's lifted tools included) and contains
  `brush_stroke`; `blend_modes` is
  `GRAY_MODES` (32: `GRAY_MODES_32`, the float intersection) + "Pass Through
  (groups)"; `get_document`'s `precision_support.supported_tools` is equal.
- **G16 render.** `render channel gray` on 8- and 16-bit documents of known
  codes reads each pixel's code (8: c, 16: round(c / 257)) within 1 on R, G
  and B, and its text contains "8-bit display of native gray composite"; at
  32 bits the text contains "8-bit tone-mapped display of gray composite" (no
  code check). `channel red`, `channel composite` and `channel gray layer 0`
  each refuse with `RENDER_REFUSAL` exactly. After `new_layer` and
  `add_layer_mask hide_all` on layer 1, `channel mask layer 1` reads 0 at
  every pixel. Since phase 2d, after `add_channel` "Matte" (empty) on the
  8-bit document, `render channel Matte` (no layer) reads 0 at every pixel
  and its text contains "native channel Matte".
- **G17 Export As / Quick Export.** `export_preview` png 16 profile document
  is within 2 codes of `render` (no channel) on R, G and B at every pixel;
  `quick_export` with settings {png, 16, document}, a scratch destination and
  pattern `{name}-q` writes a PNG of colour type 0, depth 16, whose `iCCP` is
  the document's Gray profile (the drive's own Gray ICC the source PNG
  carries); `quick_export` webp refuses with text containing "has no
  one-channel export in Doctored" (phase 3a's sentence).
- **G18 K %.** `sample_color`'s `gray_k_percent` is 100 (1 − c / 255) at 8
  bits and 100 (1 − c16 / 65535) at 16 within 1e-6, and the key is absent at
  32 bits.

Phase 2a-ii and 2b (G19–G24), fixed before the run. "One undo step" means the
sampled pixels change, undo restores them exactly and redo brings back the
changed ones exactly, and the document stays Grayscale.

- **G19 Brush and Eraser.** On a 16-bit Gray Gamma 2.2 document of #808080,
  `brush_stroke` #ff0000 (size 4, hardness 100) samples the oracle's luminance
  of red within 4 codes on its stroke, R = G = B; `eraser_stroke` (size 4)
  leaves alpha 0 on its stroke; each is one undo step; `brush_stroke`
  `blend_mode` Hue refuses with the host's Grayscale blend sentence.
- **G20 Bucket, Fill and Gradient.** On 16-bit documents of #808080: `fill`
  #ff0000 (the bucket) and `fill` #ff0000 with `contiguous: false` (the
  core's `paint`) sample red's luminance within 4, R = G = B; `gradient`
  #000000 → #ffffff from x 1 to x W − 1 is R = G = B at every pixel with the
  first pixel within 4 of 0 and the last within 4 of 65535; each is one undo
  step.
- **G21 8 and 32 bits.** `brush_stroke` #ffffff on an 8-bit document of
  #808080 samples within 1 of 255 (in 8-bit codes), R = G = B; on a 32-bit
  document it is R = G = B.
- **G22 apply_filter.** On a 16-bit document holding the G20 ramp: `invert`
  turns every sampled code c into 65535 − c exactly; `levels` gamma 1.4 and
  `gradient_map` #ff0000 → #00ffff keep R = G = B; each is one undo step.
- **G23 adjustment layers.** `add_adjustment_layer` levels (gamma 1.4) and
  threshold (level 0.5) each add one layer in one undo step with the
  composite R = G = B; `edit_adjustment_layer` sets the Levels layer's gamma
  to 0.7 in one undo step; the document stays Grayscale; `save_copy rz`
  reopens Grayscale with the same layer count and every sampled pixel equal;
  `add_adjustment_layer shadows_highlights` refuses with `SH_LAYER_REFUSAL`
  exactly.
- **G24 refusals and Auto.** Exactly: `apply_filter hue_saturation` and
  `add_adjustment_layer black_and_white` and `auto_color` with the Grayscale
  sentence (`UNAVAILABLE_REFUSAL`), `apply_filter sepia` with the RGB-only
  sentence, `apply_filter levels` with only `r` with the Levels channel
  sentence, and `apply_filter levels` `target: "red"` with the host's plane
  sentence (`TARGET_REFUSAL`: the Gray channel is the layer, red refuses for
  good). `auto_tone`, `auto_contrast` and `equalize`
  on the G20 ramp each are one undo step and keep R = G = B.

Phase 2c (G25–G27), fixed before the run; "one undo step" as above. The
pattern document is a 24 × 16 untagged 16-bit gray PNG of varied codes,
written here.

- **G25 filters.** On the pattern document, `apply_filter` blur, unsharp_mask,
  emboss, add_noise, lens_correction (`chromatic_red_cyan` 50, `distortion`
  20) each change the sampled pixels, keep R = G = B and are one undo step;
  on a 16-bit Gray Gamma 2.2 document, clouds `#ff0000` / `#0000ff` does the
  same and every sample lies between the oracle greys of blue and red within
  4 codes (each grey the 8-bit code `round(255·Y^(1/2.2))` of its luminance,
  ×257: the colour Clouds is given is an `#rrggbb` hex); `blur` on 8- and
  32-bit Gray Gamma 2.2 documents holding a gradient is R = G = B; `liquify`
  runs since phase 4 (the document stays Grayscale).
- **G26 Fade and Actions.** After `apply_filter blur` on the pattern
  document, `fade` opacity 0.5 samples within 1 code of (before + after) / 2,
  R = G = B, and one undo returns the pre-blur samples exactly; `fade` mode
  Hue refuses with the host's Grayscale blend sentence (HEAD … TAIL);
  `save_action` of one `apply_filter blur` step, then `run_action` on a fresh
  copy of the pattern document, equals a direct `apply_filter blur` on
  another copy exactly.
- **G27 retouch.** On copies of the pattern document `clone_stamp`,
  `heal_stroke`, `spot_heal_stroke`, `patch_region`, `dodge_burn` (dodge and
  burn), `blur_stroke`, `sharpen_stroke`, `smudge_stroke` and
  `history_brush_stroke` (painting back the state before an `invert`) each
  change the sampled pixels, keep R = G = B and are one undo step;
  `sponge_stroke` saturate on a uniform 16-bit document of about code 0x4000
  lowers the stroke-centre code, desaturate raises it to at most 0x8000;
  `clone_stamp` `blend_mode` Hue refuses with the Grayscale blend sentence;
  `red_eye` refuses with `RED_EYE_REFUSAL` exactly.

Phase 2d (G28–G31), fixed before the run; "one undo step" as above. The
effects document is a 12 × 10 Gray Gamma 2.2 document of #808080 whose layer
1 holds an opaque #c0c0c0 square from (3, 3) to (8, 7), made here; its probe
is every pixel.

- **G28 effects.** On the 16-bit effects document `set_layer_style` on layer
  1 with drop_shadow #ff0000, color_overlay #00ff00, stroke #0000ff,
  outer_glow, bevel_emboss, satin, gradient_overlay #ff0000 → #0000ff and
  pattern_overlay (the stripes preset) is one undo step and keeps R = G = B at
  every pixel; color_overlay #ff0000 alone reports (`get_document`) a gray hex
  `#vvvvvv` with v within 1 of `round(255·srgb_encode(Y))`, Y red's published
  row, and samples within 4 codes of the spec's oracle grey
  `257·round(255·srgb_decode(v/255)^(1/2.2))` inside the square (drive-1 used
  the unquantized `round(65535·…)` here, against the spec, and missed it by
  6 codes: the colour enters the document as an 8-bit colour); a drop_shadow `blend` hue refuses with text containing
  the core's Grayscale blend sentence (`HUE_EFFECT`); a drop_shadow #ff0000
  on the 8- and the 32-bit effects document is R = G = B at every pixel.
- **G29 Blend If.** On a 16-bit document of #ffffff whose layer 1 holds a
  #202020 pixel row and a #e0e0e0 one, `set_layer_style` Blend If gray
  `this_layer [128, 128, 255, 255]` is one undo step, the dark row samples
  the backdrop exactly and the light row its own code exactly; Blend If on
  `red` (an identity ramp) refuses with `RED_BLEND_IF` exactly.
- **G30 Actions, presets, light, .rz.** `save_action` of one
  `set_layer_style` step (drop_shadow #ff0000 + color_overlay #00ff00 at
  opacity 0.5), run on a fresh 16-bit effects document, samples exactly as
  the direct call on another; `apply_style_preset` of a preset saved from a
  16-bit RGB document's color_overlay #ff0000 + drop_shadow #00ff00 layer is
  one undo step and R = G = B; `set_global_light` angle 30 altitude 40 over
  the drop shadow is one undo step and R = G = B; `save_copy rz` reopens
  Grayscale with every sample equal and `get_document`'s layer-1 style
  equal.
- **G31 channels.** At 16 and 32 bits on the effects document, `add_channel`
  "Alpha 1" (empty) adds one channel; `brush_stroke` and `fill_selection`
  (after `select_rect` 1, 1, 4, 4) with `target: "channel:Alpha 1"`, and at
  16 bits `apply_filter blur` sigma 2 on it, each change `render channel
  Alpha 1` and leave every composite sample exactly as it was;
  `delete_channel` is one undo step (one channel → none → one → none); with
  a selection `add_channel from: "selection"` and `from: "plane"` (luma) each
  add a channel since phase 4, and `from: "plane"` with plane red refuses
  with `ADD_CHANNEL_PLANE`, exactly.

Phase 3a (G32), fixed before the run. The files are Photoshop's own, take1's
read fixtures in `core/tests/fixtures/psd_photoshop/gray/`, each copied into
the scratch folder first (`open_document` answers an already-open path as it
is, so the composite open reads a second copy).

- **G32 gray open.** For `layers8.psd`, `layers8.tif`, `layers16.psd`,
  `layers16.psb` and `styled8.psd`: `inspect_psd` answers import_precision
  "gray"; `open_document psd_mode layers` is color_mode gray at the file's
  bits_per_channel (8 or 16) with more than one layer; for each PSD and PSB,
  `psd_mode composite` of the second copy is color_mode gray with one layer.
  `styled8.psd`'s report observes the “Overlay” layer's effects carried
  (its sentence names color_overlay).

Phase 3b (G33–G35), fixed before the run. The PSD document is a 16 × 12 Gray
document (Dot Gain 20 %) of #808080: layer "Shape" an opaque #c0c0c0
rectangle (2, 2)–(10, 8) with a mask revealing (3, 3)–(9, 7), Multiply at
50 %, inside group "Group"; layer "Styled" an opaque #404040 rectangle
(8, 4)–(14, 10) with a Color Overlay #c08040 and Blend If Gray (underlying
0/0/96/160); a Levels layer (black 10/255, white 240/255, gamma 1.2) on top.

- **G33 16 bits.** `inspect_psd_export` psd layers is available with depth 16
  and color_mode gray; `export_psd` psd, psb and tiff (layers) and psd
  flattened each write a file whose header is colour mode 1, depth 16, ONE
  channel (take2's layout; the TIFF: Photometric 1, SamplesPerPixel 1 and
  BitsPerSample ONE inline SHORT, 16); `inspect_psd` of the psd answers
  import_precision "gray" and observes the "Styled" style carried (its
  sentence names color_overlay and Blend If on gray); `open_document
  psd_mode layers` of the psd, psb and tiff each is color_mode gray,
  bits_per_channel 16, with the source's layer names, kinds and order, and
  `render` within 1 code of the source's at every pixel; `psd_mode
  composite` of the psd, and the flattened psd's layered open, are one layer.
- **G34 8 bits.** The same document at 8 bits: `export_psd` psd and tiff
  (layers) reopen color_mode gray, bits_per_channel 8, with the same names,
  kinds and order, and `render channel gray` equal to the source's at every
  pixel.
- **G35 32 bits and formats** (rewritten for phase 3c, fixed before its run).
  On a 32-bit Gray document (Dot Gain 20 %, a #808080 background and layer
  "Shape" an opaque #c0c0c0 rectangle (2, 2)–(10, 8), both at opacity 1):
  `inspect_psd_export` psd layers is available with depth 32 and color_mode
  gray; `export_psd` psd and psb (layers) and psd flattened each write a file
  whose header is colour mode 1, depth 32, ONE channel; `export_psd` tiff
  (layers) refuses with text containing `TIFF32_REFUSAL_PART`; the psd and
  psb reopen (`psd_mode layers`) color_mode gray, bits_per_channel 32, with
  the source's layer names, kinds and order, and `render channel gray` equal
  to the source's at every pixel. `export_image` tiff bit_depth 32 profile
  document writes a TIFF with SampleFormat 3, BitsPerSample 32 and
  SamplesPerPixel 1 that reopens (`precision preserve`) color_mode gray,
  bits_per_channel 32, with `render channel gray` equal to the source's at
  every pixel; `quick_export` webp refuses with `FORMAT_REFUSAL_PART`.

Phase 3c (G36), fixed before the run.

- **G36 32-bit gray open.** Photoshop's own `layers32.psd` (take2's read
  fixture, copied into the scratch folder): `inspect_psd` answers
  import_precision "gray" and depth 32; `open_document psd_mode layers` is
  color_mode gray, bits_per_channel 32, with exactly the layers "Base" and
  "Ramp"; `psd_mode composite` of a second copy is color_mode gray with one
  layer.

Phase 2e (G37–G41), fixed before the run. "One undo step" here means the
sampled pixels (codes, or the linear values `center_rgba32` at 32 bits)
change, are R = G = B, undo restores them exactly, redo brings back the
changed ones exactly, and the document stays Grayscale. The textured document
is a 24 × 16 Gray Gamma 2.2 document of #808080 at the gate's depth with a
size-3 hard `brush_stroke` #000000 from (2, 2) to (21, 13) and one #ffffff
from (2, 13) to (21, 2), made here.

- **G37 Stroke….** At 16 and 32 bits on the effects document:
  `stroke_selection` #ff0000 width 1 inside after `select_rect` (1, 1, 10, 8)
  on layer 0 is one undo step at (1, 1); with no selection, `stroke_selection`
  #ff0000 width 2 outside on layer 1 (its layer edge) is one undo step at
  (2, 5) and (9, 5); each of the three samples lies on red's luminance oracle
  (16 bits: within BOUND16 of `round(65535·Y^(1/2.2))`; 32 bits: within
  STROKE_BOUND32 of linear `Y`); `stroke_selection` `blend_mode` Hue refuses
  with the host's Grayscale blend sentence (HEAD … TAIL).
- **G38 content-aware tools.** At 8, 16 and 32 bits on the textured document,
  with `select_rect` (10, 6, 5, 4): `preview_content_aware_fill` (layer 0,
  seed 7, polled while running) changes no sample and
  `discard_content_aware_fill_preview` of its preview_id changes none either;
  `content_aware_fill` (layer 0, seed 7) is one undo step; on fresh textured
  documents `content_aware_move` of the square (9, 5)–(14, 10) by (4, 0) and
  `remove_stroke` mode content_aware (points (6, 8)–(12, 8), size 4, seed 7)
  are each one undo step. The probe is every pixel of x 7–17, y 4–11.
- **G39 Art History Brush.** On an 8-bit textured document filled #ffffff
  (`fill_selection`), `art_history_brush_stroke` from the opened state (layer
  0, a click at (12, 8), size 5, tight_long, area 30, seed 4) is one undo
  step over every second pixel; `blend_mode` Hue refuses with text containing
  `HUE_EFFECT`; on a 16-bit Gray document the stroke refuses with text
  containing `ART16_REFUSAL`.
- **G40 adjustment layers carry.** A 16-bit sRGB PNG of the patches with
  `add_adjustment_layer` levels (gamma 1.4) and gradient_map #ff0000 →
  #00ffff converts (`convert_mode gray`, Gray Gamma 2.2) to a Grayscale
  document of 3 layers in one undo step (undo answers rgb, redo gray), whose
  layer ops (get_document's `adjustment.op`) are levels and gradient_map, and
  whose map stops are each `#vvvvvv` with v within STOP_BOUND8 of
  `round(255·Y^(1/2.2))` of the original stop's colour; the composite is
  R = G = B. The same PNG with a hue_saturation layer "Hue" refuses with text
  containing `HUE_SAT_BLOCKER`, and `flatten: true` converts it to one layer.
- **G41 HDR Toning.** At 8 and 16 bits on the textured document,
  `hdr_toning` exposure_gamma (exposure 0.5, gamma 1.2) is one undo step over
  every second pixel, its reply's bit_depth and `get_precision`'s
  bits_per_channel are the document's depth; a 32-bit textured document's
  `get_precision` lists each of `PHASE_2E_TOOLS` and (the close's rewrite) has
  no `default_phase` key, and since phase 5a `hdr_toning` runs on it (G52
  holds its gates).

Phase 4 (G42–G47), fixed before the run. "One undo step" means `list_history`'s
undo_count grows by one, the document stays Grayscale, every probe sample is
R = G = B, and one undo restores the canvas size and the probe samples
exactly. The clipboard is the drive's own private pasteboard
(`DR_PASTEBOARD_NAME=com.kgalligan.doctored.verify-gray-mode.<pid>`).

- **G42 geometry.** On a 16-bit textured document: `image_size` 30 × 20,
  `canvas_size` 30 × 20, `crop` 2, 2, 16 × 10, `rotate` 90, `flip`
  horizontal, `transform_layer` rotate 30, `warp_layer` set 3 × 3 (and
  `get_warp`'s field null after it), `puppet_warp`, `perspective_warp` and
  `liquify` are each one undo step; `image_size` is one step at 8 and 32
  bits too.
- **G43 selections.** On the 16-bit textured document `select_magic_wand`,
  `select_color_range` sampled, `select_similar` and `select_subject` (a
  result or a refusal that does not name Grayscale) are admitted;
  `set_quick_mask` on, `fill_selection` with `target: "quick_mask"` and
  `set_quick_mask` off are admitted; `save_selection` and `load_selection`
  of that channel round-trip the selection's coverage16 exactly. On an
  8-bit Gray Gamma 2.2 document of known codes (the working Gray is Color
  Settings' default, not Gray Gamma 2.2), `load_selection from: "plane"
  plane: "gray"` gives coverage16 equal to each pixel's own code exactly;
  `load_selection` plane red refuses with `LOAD_PLANE_RED`.
- **G44 within Gray.** `copy` of a 10 × 6 selection of a 16-bit textured
  document, then `paste` in_place into a 16-bit Gray Gamma 2.2 document, is
  one undo step; the pasted layer's samples equal the source's exactly;
  `get_clipboard` mode is "gray" and `color_conversion` is null.
- **G45 across modes.** An sRGB #c8285a copy pasted into a 16-bit Gray Gamma
  2.2 document lands within BOUND16 of the oracle's gray, R = G = B, with
  `color_conversion` RGB → Gray, black_point_compensation false; profile
  preserve refuses with `CROSS_GRAY`. A Gray copy pasted into a 16-bit sRGB
  document is R = G = B within 1 code, `color_conversion` Gray → RGB; a
  CMYK #4080c0 copy pasted into the Gray document is R = G = B,
  `color_conversion` CMYK → Gray.
- **G46 the pasteboard TIFF.** After `copy` of an opaque 8-bit Gray
  document, the private pasteboard's TIFF has one sample a pixel at 8 bits,
  BlackIsZero, and an embedded ICC whose colour space is GRAY.
- **G47 no later phases.** (The close's rewrite.) `get_precision` on a Gray
  document has no `later_phases` or `default_phase` key, and
  `supported_tools` holds every `PHASE_4_TOOLS` tool.

Phase 5a (G48–G53), fixed before the run. "One undo step" means
`list_history`'s undo_count grows by one, the document stays Grayscale, and
one undo restores the prior profile name, bit depth and probe samples
exactly (redo the edited ones). The curve oracle is the drive's own `gray_of`
curves tabulated as the built-in Gray profiles hold them (`gray_curve`: a
4096-entry u16 table of the space's forward curve, sGray the 1024-entry IEC
table, read by linear interpolation and inverted by bisection; the drive's
gamma-1.8 ICC its u8Fixed8 gamma 461/256).

- **G48 Assign.** `assign_profile dot_gain_20` on a 16-bit Gray Gamma 2.2
  textured document is one undo step that keeps every probe sample's codes
  and names Dot Gain 20%.
- **G49 Convert.** `convert_profile dot_gain_20` of the 16-bit textured
  document lands within `CONVERT_BOUND16` (1 code of 65535) of the curve
  oracle, R = G = B, one undo step; of the 8-bit one on the ×257 grid within
  `BOUND8` of the oracle's 8-bit code; G2's 16-bit PNG with the drive's
  gamma-1.8 `iCCP` converts to `sgray` within `CONVERT_BOUND16`.
- **G50 non-Gray profiles.** `assign_profile srgb` refuses with text
  containing `RGB_PROFILE_PART` and the profile's quoted name;
  `convert_profile file` of the Generic CMYK profile refuses with text
  containing `CMYK_PROFILE_PART` (skipped, and said so, when the file is
  absent).
- **G51 Bits/Channel.** On the 16-bit textured document `convert_precision
  gray8` is one undo step whose samples are each `round(v / 257) · 257` of
  the 16-bit ones; `gray16` from there is one undo step keeping every sample;
  `gray32` is one undo step after which `get_precision` answers 32 bits; from
  32 `gray16` refuses with text containing `FLOAT_DEPTH_REFUSAL`.
- **G52 32-bit HDR Toning.** On a 32-bit textured document `hdr_toning`
  exposure_gamma (0.5, 1.2) at bit_depth 16 and at 8 answers a Grayscale
  document at that depth (`get_precision`), R = G = B, one undo step each; at
  8 every probe pixel's stored sample (`sample_pixel` source layer) is on the
  ×257 grid (the composite of the toned document evaluates in linear light,
  `float_to_u16`'s rule, so its readout is not an 8-bit code).
- **G53 the gate and the catalog.** (The close's rewrite.) `get_precision`
  has no `later_phases` key, and `supported_tools` holds
  `PHASE_5A_TOOLS`; `tools/list` shows `working_gray`, `installed` and every
  Gray space in `assign_profile`'s and `convert_profile`'s `profile` enum
  (`srgb`, `display_p3` and `file` still there) and `gray8`, `gray16`,
  `gray32` in `convert_precision`'s `mode` enum.

Phase 5b (G54–G56), fixed before the run. "One undo step" means
`list_history`'s undo_count grows by one and one `undo` restores the mode and
the probe samples exactly. An sGray document is the textured document at the
gate's depth with `assign_profile sgray`: its twin is sRGB's columns and
1024-entry table, so `convert_mode rgb rgb_space srgb` keeps every sample.
Bounds: `PROOF_BOUND8` (1 code of 255 per channel), `CMYK_TWIN_BOUND16` (2
codes of 65535 per ink).

- **G54 proof.** On a 16-bit sGray document `set_proof_setup profile
  working_cmyk proof_colors true` answers; after a 5 s poll `get_proof_setup`
  has `proof_colors` true and `failure` null; `set_proof_setup
  preserve_numbers true` refuses `PRESERVE_REFUSAL` exactly; `render_proof`
  (source "display") and `render_proof exact true` each equal, within
  `PROOF_BOUND8` per channel at every pixel (`png_pixels`) and with equal
  `out_of_gamut`, the same call after `convert_mode rgb rgb_space srgb` (then
  undone); an 8-bit sGray document's `render_proof` answers source
  "display"; on a 32-bit document `get_proof_setup` and `render_proof` refuse
  with text starting `<tool> is unavailable in a 32-bit float document.`;
  `get_precision`'s `supported_tools` holds `PROOF_TOOLS`.
- **G55 Gray → CMYK.** `convert_mode cmyk` of a 16-bit sGray document is one
  undo step answering `color_mode` cmyk, depth 16; each probe's
  `sample_color` `ink16` is within `CMYK_TWIN_BOUND16` of the same document
  converted `rgb` (srgb) then `cmyk`; an 8-bit one answers depth 8 with every
  probe `ink16` on the ×257 grid; `save_action` of one `convert_mode
  {"mode":"cmyk"}` step then `run_action` on a fresh 16-bit sGray document
  gives the direct conversion's probe `ink16` exactly.
- **G56 refusals.** A 32-bit document's `convert_mode cmyk` refuses exactly
  `CMYK_FLOAT_REFUSAL`; a 16-bit one with a Levels adjustment layer refuses
  with text containing `ADJUSTMENT_BLOCKER`, and `flatten true` converts in
  one undo step to a one-layer CMYK document.

Phase 6a-i (G57–G62), fixed before the run. The documents are New Grayscale in
Gray Gamma 2.2 at the gate's depth (#808080); "one undo step" means
`list_history`'s undo_count grows by one, the document stays Grayscale, one
`undo` restores the probe samples exactly and `redo` the edited ones. A
colour's gray oracle is `gray_of(space, luminance(rgb))` (16 bits: within
BOUND16 of 65535 times it; 8 bits: within BOUND8 of 255 times it, every sample
on the ×257 grid); a reported hex's gray is within 1 of
`round(255·srgb_encode(luminance(rgb)))`. "Neutral" is R = G = B at every
pixel of the `render` PNG (of the layer, or of the composite).

- **G57 type.** `add_text_layer` #ff0000 on a 16-bit document is one undo
  step; `get_document`'s text `color` is a gray hex on red's oracle and the
  rendered layer is neutral; `edit_text_layer` #00ff00 is one undo step that
  re-greys it (green's hex) and keeps the layer neutral; on a 32-bit document
  `add_text_layer` `rgba [1, 0, 0, 1]` reports `rgba [Y, Y, Y, 1]`, `Y` within
  `LINEAR_BOUND` of red's published row.
- **G58 shape.** `add_shape_layer` rect fill #00ff00 is one undo step whose
  interior is on green's oracle (16 and 8 bits), the layer neutral and the
  reported fill a gray hex.
- **G59 fill layers.** A solid #4080c0 fill layer's interior is on its oracle,
  neutral; `edit_fill_layer` #c04080 is one undo step on its oracle; a
  gradient #ff0000 → #0000ff is neutral with each reported stop a gray hex on
  its colour's oracle; a pattern fill of the stripes preset and one of a red
  and blue custom tile are each neutral.
- **G60 life.** On G58's shape layer `transform_layer` `scale_x`/`scale_y` 2
  keeps the shape description and the layer neutral; `rasterize_artwork`
  answers `editable_description` false and drops the description, and undo
  restores it; `save_copy rz` reopens Grayscale with the shape description
  equal and every probe sample equal; `add_text_layer` with `ink` refuses
  exactly `INK_REFUSAL`.
- **G61 paths.** On a 16-bit two-layer document `set_path` "Box" lists in
  `list_paths`; `path_to_selection` selects; `fill_path` #ff0000 on layer 1
  is one undo step on red's oracle inside the box, composite neutral;
  `stroke_path` #0000ff size 2 keeps it neutral; `set_vector_mask` from "Box"
  then `get_vector_mask` answers a vector mask, composite neutral; `save_copy
  rz` reopens Grayscale with the path "Box" and the vector mask.
- **G62 conversions.** `convert_profile dot_gain_20` of a 16-bit document with
  a #00ff00 shape layer is one undo step keeping the shape description, its
  interior within BOUND16 of `gray_of('dot_gain_20', …)`; with a #ff0000 text
  layer added, `convert_mode rgb` (srgb) and `convert_mode cmyk` each keep the
  text description (each undone, back to Grayscale).

Phase 6a-ii (G63–G68), fixed before the run, with 6a-i's definitions ("one
undo step", the oracle `gray_of`, BOUND16, BOUND8, "neutral"); a Mode
conversion's one undo step is undo_count + 1, undo answering the source mode
and redo Grayscale.

- **G63 comps.** On a 16-bit two-layer document `save_layer_comp` "A";
  `set_layer_properties` layer 1 visible false, opacity 0.5, Multiply;
  `apply_layer_comp` "A" is one undo step and `render` equals the pre-change
  render at every pixel; `update_layer_comp` (new name "A2"),
  `reorder_layer_comp` and `delete_layer_comp` are one undo step each;
  `save_copy rz` reopens Grayscale with `list_layer_comps` names equal.
- **G64 links.** `link_layers` [1, 2] is one undo step and both rows report
  the same `linked`; `transform_layer` translating layer 1 by 2 moves both
  layers' `offset_x` by 2 (transform_layer has no `linked_layers` reply key:
  the partner's move is read off get_document); `unlink_layers` is one undo
  step, then the same translation moves layer 1 only.
- **G65 snapshots.** `create_snapshot` "S"; `brush_stroke`;
  `restore_snapshot` restores the probe samples exactly (one undo step, still
  Grayscale); `save_snapshot` to `<output>/s.drsnapshot`, `delete_snapshot`,
  `load_snapshot`, another `brush_stroke`, `restore_snapshot` → Grayscale with
  the samples equal.
- **G66 Clear.** `select_rect` then `clear_selection` on a 16-bit document:
  alpha 0 inside, every sample outside unchanged, one undo step;
  `clearSelection:` is listed in `get_precision`'s `supported_menu_items`.
- **G67 carry.** A 16-bit sRGB document with `add_shape_layer` rect #00ff00
  and `add_text_layer` #ff0000 → `convert_mode gray` (Gray Gamma 2.2, no
  flatten) is one undo step; both descriptions are kept, the reported colours
  are gray hexes on their oracles, the shape interior is within BOUND16 and
  both layers are neutral; an 8-bit RGB document with a text layer converts
  keeping the description, every probe sample of the text layer alone (the
  background hidden) on the ×257 grid; a 16-bit
  CMYK document (New) with a text layer converts to Grayscale (sGray, whose
  curve is the hex's) keeping the description, its reported colour within 1
  (8-bit) of the converted sample at the centre of a `brush_stroke` (size 8,
  hardness 100) of the same hex on a raster layer beside it.
- **G68 comp carry.** A 16-bit sRGB document, layer 1 with a drop shadow
  #ff0000, `save_layer_comp` (appearance); `set_layer_style` null;
  `convert_mode gray`; `list_layer_comps` lists it; `apply_layer_comp` sets a
  shadow whose reported colour is a gray hex within 1 of red's.

Phase 6b (G69–G77), fixed before the run, with the definitions above ("one
undo step" as G19's: the samples change, undo restores them, redo brings them
back, and the undo count grows by one). The instance reads its models from the
read-only fixture store (`DR_MODELS_DIR` = `MODELS`); the cloud folder and the
providers' base URLs stay `App`'s private, dead ones, so no call reaches the
network. A tool "clears the gate" when its reply is `ok`, or an error of its
own (a model missing, no sky, no key, no consent, an align stage) that holds
none of `GATE_PARTS` (the phase-6 sentence, a Grayscale refusal, the 16-bit
gate).

- **G69 Histogram.** On uniform 8- and 16-bit documents `histogram` answers
  `channels` ["gray"], one `bins` and one `bin_weight_units` row whose only
  filled bin is round(c·255/max), and no `*_luma` key; `source scalar` target
  red answers `PLANE_REFUSAL` and target gray `SCALAR_TARGET_REFUSAL`, exactly.
- **G70 Apply Image.** `apply_image` Multiply, source merged onto layer 0 of a
  uniform 16-bit document, is within `APPLY_BOUND16` of c·c/65535 at every
  pixel, R = G = B, one undo step; target `channel:M` answers ok; Hue answers
  the Grayscale blend sentence, `source_plane` red `SOURCE_PLANE_REFUSAL`
  exactly, and a 32-bit document text containing `FLOAT_TOOL_PART`.
- **G71 Calculations.** gray × gray Multiply into a new channel "Calc" renders
  within `CALC_BOUND8` of c·c/65535/257 at every pixel; `result: selection`
  answers ok.
- **G72 Stack Modes.** On a two-layer document (#404040, then #c0c0c0)
  `stack_mode` mean is one undo step (G10's layer-list rule) and the composite
  is R = G = B at (a + b + 1) // 2 at 16 bits and the f32 of (a + b) / 2 at
  32, exactly; `reduce_noise_from_stack` median (it aligns its frames first, so
  on the textured document with its layer duplicated) answers ok and the
  render is R = G = B.
- **G73 model selections.** `select_sky`, `select_person_parts` (hair) and
  `object_finder_select` (a box) on the textured document each clear the gate.
- **G74 layers clip.** A #ff0000 `add_text_layer` on a 16-bit RGB document,
  `copy_layers_to_document` into a 16-bit Grayscale document, is a text layer
  whose colour is a gray hex within `HEX_BOUND` of red's, the composite R = G =
  B; copied back into a 16-bit RGB document it is still a text layer.
- **G75 Auto-Align and Auto-Blend.** On the textured document with its layer
  duplicated, `auto_align_layers` [0, 1] and `auto_blend_layers` [0, 1]
  stack_images answer ok, the document stays Grayscale and its render R = G =
  B.
- **G76 stacks and HDR.** A 16-bit gray PNG and an 8-bit RGB PNG written here:
  `load_files_into_stack` [gray, rgb] is Grayscale, and layer 1 (the
  composite) is R = G = B within `STACK_LOAD_BOUND16` of the RGB file opened,
  taken to 16 bits and through `convert_mode gray` (both the working Gray);
  `merge_to_hdr` of two gray PNGs (evs [0, 1], align false) and
  `merge_layers_to_hdr` of their stack each give a 32-bit Grayscale (`gray32`)
  document; `merge_panorama` of the two clears the gate.
- **G77 neural and generative.** On the textured document `super_zoom` clears
  the gate (on ok the new document is Grayscale), `jpeg_artifacts_removal`
  clears it (on ok the render is R = G = B), `neural_colorize` answers
  `COLORIZE_REFUSAL` and `sky_replacement` `SKY_REFUSAL` exactly, and
  `generative_fill` (a rectangle selected), `generative_expand` and
  `generative_variations` each clear the gate.

The close (G78–G85), fixed before the run. "One undo step" means
`list_history`'s undo_count grows by one and the document stays Grayscale.
Bound: `PLACE_BOUND16` (257 codes of 65535, 1/255).

- **G78 permanent refusals.** On a 16-bit Grayscale document every tool of
  `RAW_TOOLS` answers `RAW_REFUSAL`, `add_text` `ADD_TEXT_REFUSAL`,
  `put_pixels` `PUT_PIXELS_REFUSAL`, `get_evaluation` and `set_evaluation`
  `EVALUATION_REFUSAL`, each exactly; `place_image` of an empty `x.dng`
  answers text containing `RAW_PART`; and no `tools/call` reply of the whole
  run contains "#1 phase".
- **G79 lifted reads and edits.** On a 16-bit two-layer Grayscale document
  `arrange_layer` back, `find_layers`, `get_mask_properties` (after
  `add_layer_mask`), `auto_select_layer`, `set_resolution` 150 (one undo step;
  `get_metadata` then reports 150 ppi; a new document is at 300), `set_file_info` / `get_file_info`
  (the copyright comes back), `set_ruler_origin`, `set_history_budget`,
  `separate_color`, `list_lens_profiles` and `detect_horizon` each answer
  without an error.
- **G80 views and workspaces.** `open_second_view` answers view 2,
  `set_view` / `get_view` on view 2 agree on zoom 1, `close_second_view` 2
  leaves one view; `save_workspace` then `restore_workspace` of the same name
  answer without an error.
- **G81 samplers.** `set_color_samplers` at two points of the 16-bit textured
  document; each row's `gray_k_percent` equals `sample_color`'s at the same
  point exactly.
- **G82 Distort and Modify Native Plane.** `distort_layer` of the textured
  document's layer is one undo step and the render stays R = G = B;
  `modify_scalar` grow 2 of a mask made from a small selection changes the
  mask's render; target red answers `PLANE_REFUSAL` for `modify_scalar`.
- **G83 Place.** `place_image` of an 8-bit RGB PNG of `PATCHES` onto a
  16-bit Gray Gamma 2.2 document of its size is one undo step whose pixels
  are R = G = B within `PLACE_BOUND16` of the same file opened, taken to 16
  bits and through `convert_mode gray` (gray_gamma_22); `add_live_photo_layer`
  of `verify_phase12e2b_placement.author_movie`'s movie renders R = G = B
  (NOT RUN, and said so, without ffmpeg).
- **G84 outputs.** `export_layers` (items all, png 16, profile document) of
  a two-layer document writes two PNGs whose IHDR colour type is 0 or 4;
  `export_pdf` writes a file starting `%PDF`; `contact_sheet` output document
  opens one new document.
- **G85 tone map.** On a 32-bit textured document `set_tone_map` reinhard
  (white 4) changes the render and one `undo` restores it; on a 16-bit
  document `set_tone_map` fails with the handler's float sentence (text
  starting `TONE_MAP_FLOAT_PART`), as on an RGB document.
"""
import argparse
import base64
import json
import math
import os
from pathlib import Path
import struct
import subprocess
import sys
import time
import zlib

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

from phase5_support import App

# ---- the expectations, fixed here BEFORE anything is measured -------------
BOUND16 = 4
BOUND8 = 1
CMYK_BOUND8 = 2
SRGB_Y = (0x38F5 / 65536, 0xB787 / 65536, 0x0F84 / 65536)
RAW_PART = 'is unavailable in a Grayscale document: Doctored keeps camera RAW layers on RGB documents only.'
RAW_REFUSAL = '{} ' + RAW_PART
RGB_PROFILE_PART = ('is an RGB profile: a Grayscale document is assigned and converted to Gray '
                    'profiles only.')
SAVE_REFUSAL = ('A Grayscale document saves as .doctored, PNG, TIFF or JPEG (one channel, its Gray '
                'profile embedded); Export as PSD (export_psd) writes a PSD, PSB or layered '
                'TIFF. Convert to RGB Color for other formats.')
GRAY_LIMITATION_HEAD = 'Grayscale layer blending supports Normal, Dissolve, Darken, Multiply'
GRAY_LIMITATION_TAIL = ('; every other mode is unavailable in a Grayscale document. '
                        'Pass Through requires a group.')
JPEG_BOUND8 = 2
# Phase 1c (G15–G18), fixed before the run.
GRAY_TOOLS = sorted([
    'add_guide', 'add_layer_mask', 'clear_guides', 'close_document', 'convert_mode', 'delete_layer',
    'deselect', 'duplicate_layer', 'export_image', 'export_preview', 'fill_selection',
    'flatten_image', 'get_color_profile', 'get_document', 'get_precision', 'get_recovery_status',
    'get_selection', 'group_layers', 'list_guides', 'list_history', 'merge_down', 'merge_visible',
    'new_layer', 'quick_export', 'redo', 'remove_guide', 'remove_layer_mask', 'render',
    'reorder_layer', 'reorder_layers', 'reselect', 'restore_history', 'sample_color',
    'sample_pixel', 'save_copy', 'select_all', 'select_ellipse', 'select_freehand',
    'select_polygon', 'select_rect', 'set_active_layer', 'set_layer_clipped', 'set_layer_label',
    'set_layer_lock', 'set_layer_mask_enabled', 'set_layer_properties', 'set_layer_style',
    'set_mask_properties', 'set_selected_layers', 'stamp_visible', 'undo', 'ungroup_layers',
    'write_recovery',
    # Phase 2a-ii and 2b.
    'brush_stroke', 'eraser_stroke', 'fill', 'gradient', 'apply_filter', 'add_adjustment_layer',
    'edit_adjustment_layer', 'auto_tone', 'auto_contrast', 'equalize',
    # Phase 2c.
    'dodge_burn', 'sponge_stroke', 'blur_stroke', 'sharpen_stroke', 'smudge_stroke',
    'history_brush_stroke', 'clone_stamp', 'heal_stroke', 'spot_heal_stroke', 'patch_region',
    'fade', 'depth_blur', 'run_action',
    # Phase 2d.
    'set_global_light', 'apply_style_preset', 'save_style_preset', 'list_channels', 'add_channel',
    'duplicate_channel', 'delete_channel', 'rename_channel', 'set_channel_options',
    'invert_channel',
    # Phase 3b.
    'export_psd', 'inspect_psd_export',
    # Phase 2e.
    'stroke_selection', 'content_aware_fill', 'preview_content_aware_fill',
    'discard_content_aware_fill_preview', 'content_aware_move', 'remove_stroke',
    'art_history_brush_stroke', 'hdr_toning',
    # Phase 4.
    'image_size', 'canvas_size', 'crop', 'perspective_crop', 'fit_crop', 'trim', 'reveal_canvas',
    'rotate', 'flip', 'straighten_layers', 'transform_layer', 'transform_layers', 'transform_again',
    'transform_mask', 'transform_selection', 'get_warp', 'warp_layer', 'puppet_warp',
    'perspective_warp', 'liquify', 'align_layers', 'distribute_layers', 'move_layers',
    'move_selected_pixels', 'transform_selected_pixels', 'copy', 'cut', 'copy_merged', 'paste',
    'layer_via_copy', 'layer_via_cut', 'duplicate_to_new_document', 'copy_layers_to_document',
    'stamp_selected', 'merge_group', 'select_magic_wand', 'select_color_range', 'select_similar',
    'quick_select', 'select_subject', 'select_subjects', 'inspect_subjects', 'select_people',
    'select_face_parts', 'inspect_faces', 'object_select', 'refine_selection', 'select_magnetic',
    'modify_selection', 'save_selection', 'load_selection', 'set_quick_mask', 'quick_mask_stroke',
    'add_luminosity_masks',
    # Phase 5a.
    'assign_profile', 'convert_profile', 'convert_precision',
    # Phase 5b.
    'get_proof_setup', 'set_proof_setup', 'render_proof',
    # Phase 6a-i.
    'add_text_layer', 'edit_text_layer', 'add_shape_layer', 'edit_shape_layer', 'add_fill_layer',
    'edit_fill_layer', 'rasterize_artwork', 'list_paths', 'set_path', 'rename_path',
    'duplicate_path', 'delete_path', 'selection_to_path', 'path_to_selection', 'fill_path',
    'stroke_path', 'get_vector_mask', 'set_vector_mask', 'set_vector_mask_properties',
    'remove_vector_mask', 'set_clipping_path',
    # Phase 6a-ii.
    'link_layers', 'unlink_layers', 'list_layer_comps', 'save_layer_comp', 'apply_layer_comp',
    'update_layer_comp', 'delete_layer_comp', 'reorder_layer_comp', 'list_snapshots',
    'create_snapshot', 'rename_snapshot', 'delete_snapshot', 'restore_snapshot', 'save_snapshot',
    'load_snapshot', 'clear_selection',
    # Phase 6b.
    'histogram', 'apply_image', 'calculations', 'stack_mode', 'reduce_noise_from_stack',
    'auto_align_layers', 'auto_blend_layers', 'merge_layers_to_hdr', 'select_sky',
    'select_person_parts', 'object_finder_select', 'super_zoom', 'jpeg_artifacts_removal',
    'generative_fill', 'generative_expand', 'generative_variations',
    'select_generative_variation', 'generate_variations',
    # The close.
    'arrange_layer', 'distort_layer', 'auto_select_layer', 'find_layers', 'get_mask_properties',
    'list_views', 'open_second_view', 'close_second_view', 'get_view', 'set_view',
    'save_workspace', 'restore_workspace', 'get_color_samplers', 'set_color_samplers',
    'get_file_info', 'set_file_info', 'get_metadata', 'set_resolution', 'set_ruler_origin',
    'set_history_budget', 'recover_document', 'discard_recovery', 'detect_horizon',
    'list_lens_profiles', 'modify_scalar', 'save_proof_preset', 'separate_color', 'export_layers',
    'export_pdf', 'contact_sheet', 'contact_sheet_from_sources', 'get_tone_map', 'set_tone_map',
    'place_image', 'add_live_photo_layer',
    # #1 spot channels (18b81bc6).
    'add_spot_channel', 'set_spot_channel', 'merge_spot_channel', 'set_channel_kind'])
GRAY_MODES = ['Normal', 'Dissolve', 'Darken', 'Multiply', 'Color Burn', 'Linear Burn', 'Lighten',
              'Screen', 'Color Dodge', 'Linear Dodge (Add)', 'Overlay', 'Soft Light', 'Hard Light',
              'Vivid Light', 'Linear Light', 'Pin Light', 'Hard Mix', 'Difference', 'Exclusion',
              'Subtract', 'Divide']
GRAY_MODES_32 = ['Normal', 'Dissolve', 'Darken', 'Multiply', 'Linear Burn', 'Lighten',
                 'Linear Dodge (Add)', 'Linear Light', 'Pin Light', 'Difference', 'Subtract',
                 'Divide']
RENDER_REFUSAL = ('A Grayscale document renders its composite with no channel, channel gray '
                  'with no layer, channel mask with a layer, or an alpha channel by name with no '
                  'layer.')
RENDER_8_16 = '8-bit display of native gray composite'
RENDER_32 = '8-bit tone-mapped display of gray composite'
EXPORT_BOUND8 = 2
RENDER_BOUND8 = 1
K_BOUND = 1e-6
# Phase 2a-ii and 2b (G19–G24), fixed before the run.
SH_LAYER_REFUSAL = ('A Shadows/Highlights adjustment layer is unavailable in a Grayscale document: '
                    'Shadows/Highlights applies to pixels only.')
UNAVAILABLE_REFUSAL = '{} is unavailable in a Grayscale document.'
RGB_ONLY_REFUSAL = ('{} is unavailable in a Grayscale document: Doctored offers it on RGB '
                    'documents only.')
LEVELS_CHANNELS = ('Levels in a Grayscale document has one channel, Gray: set the composite, '
                   'or r, g and b alike.')
TARGET_REFUSAL = ('apply_filter target "red" is unavailable in a Grayscale document: its one '
                  'colour channel, Gray, is the layer, and Doctored offers the Red, Green, Blue '
                  'and Alpha targets on RGB documents only.')
# Phase 2c (G25–G27), fixed before the run.
RED_EYE_REFUSAL = ('Remove Red Eye is unavailable in a Grayscale document: Doctored offers it on '
                   'RGB documents only.')
FADE_BOUND16 = 1
CLOUDS_BOUND16 = 4
PATTERN_W, PATTERN_H = 24, 16
# Phase 2d (G28–G31), fixed before the run.
HUE_EFFECT = 'The Hue blend mode is unavailable in a Grayscale document.'
RED_BLEND_IF = ('Blend If on the Red channel is unavailable in a Grayscale document.')
ADD_CHANNEL_PLANE = ('add_channel target "red" is unavailable in a Grayscale document: its one '
                     'colour channel, Gray, is the layer, and Doctored offers the Red, Green, Blue '
                     'and Alpha targets on RGB documents only.')
OVERLAY_HEX_BOUND = 1
# Phase 3a (G32), fixed before the run.
FORMAT_REFUSAL_PART = 'has no one-channel export in Doctored'
GRAY_FIXTURES = ROOT / 'core/tests/fixtures/psd_photoshop/gray'
GRAY_FIXTURE_FILES = [('layers8.psd', 8), ('layers8.tif', 8), ('layers16.psd', 16),
                      ('layers16.psb', 16), ('styled8.psd', 8)]
# Phase 3b (G33–G35), fixed before the run; G35 rewritten for phase 3c before its run.
TIFF32_REFUSAL_PART = 'A TIFF with layers is not written'
# Phase 3c (G36), fixed before the run.
LAYERS32 = 'layers32.psd'
LAYERS32_NAMES = ['Base', 'Ramp']
PSD_RENDER_BOUND8 = 1
# Phase 2e (G37–G41), fixed before the run.
STROKE_BOUND32 = 1e-4
STOP_BOUND8 = 1
ART16_REFUSAL = 'The Art History Brush is unavailable in a 16-bit document.'
HUE_SAT_BLOCKER = ('“Hue” cannot become Grayscale: Hue/Saturation is unavailable in a Grayscale '
                   'document. Flatten the image to convert it')
PHASE_2E_TOOLS = ['stroke_selection', 'content_aware_fill', 'preview_content_aware_fill',
                  'discard_content_aware_fill_preview', 'content_aware_move', 'remove_stroke',
                  'art_history_brush_stroke', 'hdr_toning']
# Phase 4 (G42–G47), fixed before the run.
PHASE_4_TOOLS = [
    'image_size', 'canvas_size', 'crop', 'perspective_crop', 'fit_crop', 'trim', 'reveal_canvas',
    'rotate', 'flip', 'straighten_layers', 'transform_layer', 'transform_layers', 'transform_again',
    'transform_mask', 'transform_selection', 'get_warp', 'warp_layer', 'puppet_warp',
    'perspective_warp', 'liquify', 'align_layers', 'distribute_layers', 'move_layers',
    'move_selected_pixels', 'transform_selected_pixels', 'copy', 'cut', 'copy_merged', 'paste',
    'layer_via_copy', 'layer_via_cut', 'duplicate_to_new_document', 'copy_layers_to_document',
    'stamp_selected', 'merge_group', 'select_magic_wand', 'select_color_range', 'select_similar',
    'quick_select', 'select_subject', 'select_subjects', 'inspect_subjects', 'select_people',
    'select_face_parts', 'inspect_faces', 'object_select', 'refine_selection', 'select_magnetic',
    'modify_selection', 'save_selection', 'load_selection', 'set_quick_mask', 'quick_mask_stroke',
    'add_luminosity_masks']
CROSS_GRAY = ("Don't Convert keeps numbers within one colour mode; pixels pasted between "
              'Grayscale and another mode are always converted')
LOAD_PLANE_RED = ('load_selection target "red" is unavailable in a Grayscale document: its one '
                  'colour channel, Gray, is the layer, and Doctored offers the Red, Green, Blue '
                  'and Alpha targets on RGB documents only.')
GRAY_RGB_BOUND16 = 1
PASTEBOARD_READER = r'''
import AppKit
let arguments = CommandLine.arguments
let pasteboard = NSPasteboard(name: NSPasteboard.Name(arguments[1]))
guard let data = pasteboard.data(forType: .tiff) else { exit(2) }
try! data.write(to: URL(fileURLWithPath: arguments[2]))
'''
# Phase 5a (G48–G53), fixed before the run.
CONVERT_BOUND16 = 1
CMYK_PROFILE_PART = ('is a CMYK profile: a Grayscale document is assigned and converted to Gray '
                     'profiles only.')
FLOAT_DEPTH_REFUSAL = ('A 32-bit Grayscale document converts to 8 or 16 Bits/Channel through HDR '
                       'Toning (hdr_toning with bit_depth 8 or 16).')
GENERIC_CMYK = Path('/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc')
PHASE_5A_TOOLS = ['assign_profile', 'convert_profile', 'convert_precision']
GRAY_SPACES = ['dot_gain_10', 'dot_gain_15', 'dot_gain_20', 'dot_gain_25', 'dot_gain_30',
               'gray_gamma_18', 'gray_gamma_22', 'sgray']
ICC_GAMMA = 461 / 256
# Phase 5b (G54–G56), fixed before the run.
PROOF_BOUND8 = 1
CMYK_TWIN_BOUND16 = 2
PROOF_TOOLS = ['get_proof_setup', 'set_proof_setup', 'render_proof']
PRESERVE_REFUSAL = 'Preserve Numbers needs a CMYK document.'
FLOAT_TOOL_PART = 'is unavailable in a 32-bit float document.'
CMYK_FLOAT_REFUSAL = ('CMYK Color is unavailable in a 32-bit float document; convert to 16 '
                      'Bits/Channel first.')
ADJUSTMENT_BLOCKER = 'is an adjustment layer, which a CMYK document cannot hold yet'
# Phase 6a-i (G57–G62), fixed before the run.
LINEAR_BOUND = 1e-4
HEX_BOUND = 1
INK_REFUSAL = "ink is a CMYK document's colour. Pass color."
# Phase 6a-ii (G63–G68), fixed before the run.
HUE_BLOCKER = 'is a layer in the Hue blend mode, which a Grayscale document cannot hold'
# The close (G78–G85), fixed before the run.
RAW_TOOLS = ['apply_raw_preset', 'apply_raw_settings', 'copy_raw_local_settings',
             'edit_raw_development', 'get_raw_local_mask', 'get_raw_locals', 'get_raw_source',
             'inspect_raw_development', 'migrate_raw_decoder', 'preview_raw_decoder_migration',
             'redevelop_raw', 'save_raw_preset', 'sync_raw_settings', 'get_image_source',
             'rasterize_image_source']
ADD_TEXT_REFUSAL = ('add_text is unavailable in a Grayscale document: it draws 8-bit RGB pixels; '
                    'add_text_layer adds editable type.')
PUT_PIXELS_REFUSAL = ('put_pixels is unavailable in a Grayscale document: it writes 8-bit RGBA '
                      'bytes, which Doctored offers on 8-bit RGB documents only.')
EVALUATION_REFUSAL = ('{} is unavailable in a Grayscale document: Doctored offers linear-light '
                      'evaluation on RGB documents only.')
PLACE_BOUND16 = 257
TONE_MAP_FLOAT_PART = 'A tone map applies only to a 32-bit float document.'
# Phase 6b (G69–G77), fixed before the run.
APPLY_BOUND16 = 2
CALC_BOUND8 = 2
STACK_LOAD_BOUND16 = 1
PLANE_REFUSAL = ('{} target "{}" is unavailable in a Grayscale document: its one colour channel, '
                 'Gray, is the layer, and Doctored offers the Red, Green, Blue and Alpha targets '
                 'on RGB documents only.')
SCALAR_TARGET_REFUSAL = "A Grayscale document's scalar histogram takes target mask or channel:<name>."
SOURCE_PLANE_REFUSAL = ('source_plane must be "gray", "alpha" or an alpha channel\'s name on a '
                        'Grayscale document (got "red")')
COLORIZE_REFUSAL = ('Neural Colorize is unavailable in a Grayscale document: a Grayscale layer '
                    'holds no colour. Convert to RGB Color first.')
SKY_REFUSAL = ('Sky Replacement is unavailable in a Grayscale document: Doctored offers it on RGB '
               'documents only.')
# A reply carrying any of these did not clear the Grayscale gate (module doc, phase 6b).
GATE_PARTS = ('Grayscale', 'until #1', 'unavailable in 16-bit mode')
MODELS = Path('~/devel/doctored-fixtures/models').expanduser()
PSD_W, PSD_H = 16, 12
EFFECTS_W, EFFECTS_H = 12, 10
PATCHES = [(200, 40, 90), (30, 180, 220), (128, 128, 128), (250, 250, 10),
           (64, 0, 255), (0, 0, 0), (255, 255, 255), (90, 160, 40)]


def srgb_decode(v):
    return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4


def srgb_encode(y):
    return 12.92 * y if y <= 0.0031308 else 1.055 * y ** (1 / 2.4) - 0.055


def gray_of(space, y):
    y = min(max(y, 0.0), 1.0)
    if space == 'sgray':
        return srgb_encode(y)
    if space.startswith('dot_gain_'):
        d = int(space.rsplit('_', 1)[1])
        return y ** (math.log(0.5) / math.log(0.5 - d / 100))
    gamma = {'gray_gamma_18': 1.8, 'gray_gamma_22': 2.2}[space]
    return y ** (1 / gamma)


def gray_curve(space):
    """The built-in Gray profile's kTRC as a table (module doc, phase 5a): the
    space's forward curve, the inverse of `gray_of`, at 4096 entries rounded to
    u16 (sGray: the IEC decode at 1024); the drive's ICC its gamma."""
    if space == 'icc':
        return None
    if space == 'sgray':
        n, forward = 1024, srgb_decode
    else:
        n = 4096
        if space.startswith('dot_gain_'):
            d = int(space.rsplit('_', 1)[1])
            exponent = math.log(0.5) / math.log(0.5 - d / 100)
        else:
            exponent = 1 / {'gray_gamma_18': 1.8, 'gray_gamma_22': 2.2}[space]
        forward = lambda v: v ** (1 / exponent)  # noqa: E731
    return [round(forward(i / (n - 1)) * 65535) / 65535 for i in range(n)]


def curve_decode(table, v):
    if table is None:
        return v ** ICC_GAMMA
    x = min(max(v, 0.0), 1.0) * (len(table) - 1)
    i = min(int(x), len(table) - 2)
    return table[i] + (table[i + 1] - table[i]) * (x - i)


def curve_encode(table, y):
    lo, hi = 0.0, 1.0
    for _ in range(60):
        mid = 0.5 * (lo + hi)
        if curve_decode(table, mid) < y:
            lo = mid
        else:
            hi = mid
    return 0.5 * (lo + hi)


def luminance(rgb_unit):
    return sum(k * srgb_decode(v) for k, v in zip(SRGB_Y, rgb_unit))


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


def gray_icc(name, gamma):
    """A v2.1 mntr GRAY profile: desc, wtpt D50, a gamma kTRC (ICC.1:2001-04)."""
    desc = b'desc' + b'\0' * 4 + struct.pack('>I', len(name) + 1) + name.encode() + b'\0' + b'\0' * 78
    wtpt = b'XYZ ' + b'\0' * 4 + struct.pack('>iii', 0xF6D6, 0x10000, 0xD32D)
    ktrc = b'curv' + b'\0' * 4 + struct.pack('>IH', 1, round(gamma * 256))
    tags = [(b'desc', desc), (b'wtpt', wtpt), (b'kTRC', ktrc)]
    start = 128 + 4 + 12 * len(tags)
    table, payload = b'', b''
    for sig, data in tags:
        table += sig + struct.pack('>II', start + len(payload), len(data))
        payload += data + b'\0' * (-len(data) % 4)
    header = (struct.pack('>I', start + len(payload)) + b'\0' * 4 + bytes([2, 0x10, 0, 0])
              + b'mntrGRAYXYZ ' + b'\0' * 12 + b'acsp' + b'\0' * 28
              + struct.pack('>iii', 0xF6D6, 0x10000, 0xD32D) + b'\0' * 48)
    assert len(header) == 128
    return header + struct.pack('>I', len(tags)) + table + payload


def iccp(profile):
    return chunk(b'iCCP', b'ICC Profile\0\0' + zlib.compress(profile))


def gray_tiff16(w, h, values):
    """A little-endian classic BlackIsZero TIFF, one 16-bit sample, one strip."""
    data = struct.pack(f'<{len(values)}H', *values)

    def short(v):
        return struct.pack('<HH', v, 0)

    entries = [(256, 4, 1, struct.pack('<I', w)), (257, 4, 1, struct.pack('<I', h)),
               (258, 3, 1, short(16)), (259, 3, 1, short(1)), (262, 3, 1, short(1)),
               (273, 4, 1, struct.pack('<I', 8)), (277, 3, 1, short(1)),
               (278, 4, 1, struct.pack('<I', h)), (279, 4, 1, struct.pack('<I', len(data))),
               (284, 3, 1, short(1))]
    out = b'II*\x00' + struct.pack('<I', 8 + len(data)) + data + struct.pack('<H', len(entries))
    for tag, kind, count, value in entries:
        out += struct.pack('<HHI', tag, kind, count) + value
    return out + struct.pack('<I', 0)


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


def tiff_header(data):
    """TIFF: (SamplesPerPixel, Photometric, first BitsPerSample, ICC tag 34675)."""
    order = '<' if data[:2] == b'II' else '>'
    assert struct.unpack(order + 'H', data[2:4])[0] == 42
    ifd = struct.unpack(order + 'I', data[4:8])[0]
    count = struct.unpack(order + 'H', data[ifd:ifd + 2])[0]
    spp, photometric, bits, icc = 1, None, None, b''
    for e in range(count):
        at = ifd + 2 + e * 12
        tag, _kind, n = struct.unpack(order + 'HHI', data[at:at + 8])
        short = struct.unpack(order + 'H', data[at + 8:at + 10])[0]
        offset = struct.unpack(order + 'I', data[at + 8:at + 12])[0]
        if tag == 277:
            spp = short
        elif tag == 262:
            photometric = short
        elif tag == 258:
            bits = short if n <= 2 else struct.unpack(order + 'H', data[offset:offset + 2])[0]
        elif tag == 34675:
            icc = data[at + 8:at + 8 + n] if n <= 4 else data[offset:offset + n]
    return spp, photometric, bits, icc


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


def jpeg_header(data):
    """JPEG: (SOF component count, the APP2 ICC_PROFILE chunks joined)."""
    assert data[:2] == b'\xff\xd8'
    at, nf, icc = 2, None, b''
    while at + 4 <= len(data):
        assert data[at] == 0xFF, at
        marker = data[at + 1]
        if marker == 0xDA:
            break
        length = struct.unpack('>H', data[at + 2:at + 4])[0]
        body = data[at + 4:at + 2 + length]
        if 0xC0 <= marker <= 0xC2:
            nf = body[5]
        elif marker == 0xE2 and body.startswith(b'ICC_PROFILE\0'):
            icc += body[14:]
        at += 2 + length
    return nf, icc


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    output = Path(args.output).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = output / 'work'
    folder.mkdir(parents=True, exist_ok=False)
    receipts = {'expectations': 'fixed in scripts/verify_gray_mode.py before any measurement'}
    # Phase 4: copy and paste use the drive's own pasteboard, never the person's.
    os.environ['DR_PASTEBOARD_NAME'] = f'com.kgalligan.doctored.verify-gray-mode.{os.getpid()}'
    # Phase 6b: the models come from the read-only fixture store; the cloud folder and the
    # providers' base URLs stay `App`'s private, dead ones.
    os.environ['DR_MODELS_DIR'] = str(MODELS)

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call
        # The close (G78): every tools/call reply of the run is read for "#1 phase".
        phase_replies = []
        plain_rpc = app.client.rpc

        def recording_rpc(method, params=None):
            result = plain_rpc(method, params)
            if method == 'tools/call' and isinstance(result, dict):
                for content in result.get('content', []) or []:
                    text = content.get('text') if isinstance(content, dict) else None
                    if text and '#1 phase' in text:
                        phase_replies.append((params.get('name'), text[:300]))
            return result
        app.client.rpc = recording_rpc

        def center(doc, x, y):
            """The pixel as 16-bit codes (an 8-bit document's bytes ×257)."""
            sample = call('sample_color', document_id=doc, x=x, y=y)
            if 'center_rgba16' in sample:
                return sample['center_rgba16']
            return [sample[k] * 257 for k in ('r', 'g', 'b', 'a')]

        def mode(doc):
            return next(d for d in call('list_documents')['documents'] if d['id'] == doc)['color_mode']

        # ---- G1: New -------------------------------------------------------
        made = {}
        for depth in (8, 16, 32):
            reply = call('new_document', width=6, height=4, color_mode='gray', depth=depth,
                         background='#ff0000', profile='gray_gamma_22')
            doc = reply['document']['id']
            assert reply['document']['color_mode'] == 'gray', reply
            made[depth] = doc
        expected = round(gray_of('gray_gamma_22', luminance((1, 0, 0))) * 65535)
        got = center(made[16], 2, 2)
        assert got[0] == got[1] == got[2] and abs(got[0] - expected) <= BOUND16, (got, expected)
        assert next(d for d in call('list_documents')['documents']
                    if d['id'] == made[32])['precision'] == 'gray32'
        receipts['g1'] = {'fill16': got, 'oracle16': expected}
        print('PASS g1: New Grayscale at 8, 16 and 32 bits', flush=True)

        # ---- G2: opens -------------------------------------------------------
        codes8 = [0, 17, 128, 255, 64, 200]
        codes16 = [0, 1000, 30000, 65535, 12345, 50000]
        alpha16 = [65535, 40000, 0, 20000, 65535, 1]
        icc = gray_icc('Drive Gray 1.8', 1.8)
        files = {
            'u8.png': (lambda p: png(p, 3, 2, 8, 0, codes8), [c * 257 for c in codes8], None, None),
            'a16.png': (lambda p: png(p, 3, 2, 16, 4, [v for pair in zip(codes16, alpha16) for v in pair]),
                        codes16, alpha16, None),
            'icc16.png': (lambda p: png(p, 3, 2, 16, 0, codes16, [iccp(icc)]), codes16, None,
                          'Drive Gray 1.8'),
            'srgb8.png': (lambda p: png(p, 3, 2, 8, 0, codes8, [chunk(b'sRGB', b'\0')]),
                          [c * 257 for c in codes8], None, 'sGray'),
            'g16.tif': (lambda p: p.write_bytes(gray_tiff16(3, 2, codes16)), codes16, None, None),
        }
        opened = {}
        for name, (write, want, alphas, profile) in files.items():
            path = folder / name
            write(path)
            doc = call('open_document', path=str(path))['document']['id']
            assert mode(doc) == 'gray', name
            for i, code in enumerate(want):
                px = center(doc, i % 3, i // 3)
                if alphas is not None and alphas[i] == 0:
                    continue
                assert px[0] == px[1] == px[2] == code, (name, i, px, code)
                assert px[3] == (alphas[i] if alphas else 65535), (name, i, px)
            if profile:
                assert call('get_color_profile', document_id=doc)['name'] == profile, name
            opened[name] = doc
        receipts['g2'] = sorted(files)
        print('PASS g2: gray PNG/TIFF open as Grayscale with their codes and profiles', flush=True)

        # ---- G3: RGB → Gray ----------------------------------------------------
        # One file per opened document: open_document answers an already open path's document.
        rgb8 = folder / 'patches8.png'
        png(rgb8, len(PATCHES), 1, 8, 2, [v for p in PATCHES for v in p])
        conversions = {}
        for space in ('dot_gain_20', 'gray_gamma_22'):
            rgb16 = folder / f'patches16-{space}.png'
            png(rgb16, len(PATCHES), 1, 16, 2, [v * 257 for p in PATCHES for v in p])
            doc = call('open_document', path=str(rgb16), precision='preserve')['document']['id']
            reply = call('convert_mode', document_id=doc, mode='gray', gray_space=space)
            assert reply['color_mode'] == 'gray' and reply['depth'] == 16, reply
            worst = 0
            for i, patch in enumerate(PATCHES):
                px = center(doc, i, 0)
                want = round(gray_of(space, luminance([v / 255 for v in patch])) * 65535)
                assert px[0] == px[1] == px[2], px
                worst = max(worst, abs(px[0] - want))
            assert worst <= BOUND16, (space, worst)
            call('undo', document_id=doc)
            assert mode(doc) == 'rgb'
            call('redo', document_id=doc)
            assert mode(doc) == 'gray'
            conversions[space] = worst
        doc8 = call('open_document', path=str(rgb8))['document']['id']
        reply = call('convert_mode', document_id=doc8, mode='gray', gray_space='dot_gain_20')
        assert reply['depth'] == 8, reply
        worst8 = 0
        for i, patch in enumerate(PATCHES):
            px = center(doc8, i, 0)
            assert px[0] % 257 == 0 and px[0] == px[1] == px[2], px
            want = round(gray_of('dot_gain_20', luminance([v / 255 for v in patch])) * 255)
            worst8 = max(worst8, abs(px[0] // 257 - want))
        assert worst8 <= BOUND8, worst8
        receipts['g3'] = {'worst16': conversions, 'worst8': worst8}
        print('PASS g3: RGB → Grayscale lands on the oracle, one undo step', flush=True)

        # ---- G4: CMYK → Gray ---------------------------------------------------
        cdoc = call('new_document', width=4, height=4, color_mode='cmyk', depth=16,
                    background='#4080c0')['document']['id']
        call('convert_mode', document_id=cdoc, mode='rgb', rgb_space='srgb')
        rgb = center(cdoc, 1, 1)
        call('undo', document_id=cdoc)
        assert mode(cdoc) == 'cmyk'
        reply = call('convert_mode', document_id=cdoc, mode='gray', gray_space='dot_gain_20')
        assert reply['color_mode'] == 'gray', reply
        gray = center(cdoc, 1, 1)
        want = gray_of('dot_gain_20', luminance([v / 65535 for v in rgb[:3]])) * 255
        assert gray[0] == gray[1] == gray[2] and abs(gray[0] / 257 - want) <= CMYK_BOUND8, (gray, want)
        receipts['g4'] = {'rgb': rgb, 'gray': gray, 'oracle8': want}
        print('PASS g4: CMYK → Grayscale through RGB', flush=True)

        # ---- G5: Gray → RGB ------------------------------------------------------
        back = call('convert_mode', document_id=opened['u8.png'], mode='rgb', rgb_space='srgb')
        assert back['color_mode'] == 'rgb' and back['depth'] == 8, back
        px = center(opened['u8.png'], 2, 0)
        assert px[0] == px[1] == px[2], px
        call('undo', document_id=opened['u8.png'])
        assert mode(opened['u8.png']) == 'gray'
        receipts['g5'] = back
        print('PASS g5: Grayscale → RGB Color, one undo step', flush=True)

        # ---- G6: 32 bits ------------------------------------------------------------
        fdoc = call('new_document', width=4, height=4, depth=32, background='#336699')['document']['id']
        call('convert_mode', document_id=fdoc, mode='gray')
        assert mode(fdoc) == 'gray'
        call('undo', document_id=fdoc)
        assert mode(fdoc) == 'rgb'
        call('redo', document_id=fdoc)
        call('convert_mode', document_id=fdoc, mode='rgb', rgb_space='srgb')
        assert mode(fdoc) == 'rgb'
        call('undo', document_id=fdoc)
        assert mode(fdoc) == 'gray'
        receipts['g6'] = 'rgb32 → gray32 → rgb32'
        print('PASS g6: 32-bit RGB ⇄ Grayscale, one undo step each', flush=True)

        # ---- G7: a Hue layer needs a flatten -------------------------------------
        typed8 = folder / 'typed8.png'
        png(typed8, len(PATCHES), 1, 8, 2, [v for p in PATCHES for v in p])
        tdoc = call('open_document', path=str(typed8))['document']['id']
        call('new_layer', document_id=tdoc, name='Hue')
        call('set_layer_properties', document_id=tdoc, index=1, blend_mode='Hue')
        refusal = call('convert_mode', fail=True, document_id=tdoc, mode='gray')
        assert HUE_BLOCKER in refusal, refusal
        assert mode(tdoc) == 'rgb'
        flattened = call('convert_mode', document_id=tdoc, mode='gray', flatten=True)
        assert flattened['document']['layer_count'] == 1 and mode(tdoc) == 'gray', flattened
        call('undo', document_id=tdoc)
        assert mode(tdoc) == 'rgb' and next(
            d for d in call('list_documents')['documents'] if d['id'] == tdoc)['layer_count'] == 2
        receipts['g7'] = refusal
        print('PASS g7: a Hue layer is named; flatten converts in one undo step', flush=True)

        # ---- G8: .rz ------------------------------------------------------------------
        rz = folder / 'gray.rz'
        call('save_copy', document_id=opened['a16.png'], path=str(rz), format='rz')
        rdoc = call('open_document', path=str(rz))['document']['id']
        assert mode(rdoc) == 'gray'
        for i in range(6):
            assert center(rdoc, i % 3, i // 3) == center(opened['a16.png'], i % 3, i // 3), i
        receipts['g8'] = str(rz)
        print('PASS g8: the .rz round trip keeps a Grayscale document', flush=True)

        # ---- G9: refusals -----------------------------------------------------------------
        gdoc = opened['g16.tif']
        refusals = {
            'get_raw_source': call('get_raw_source', fail=True, document_id=gdoc),
            'assign_profile': call('assign_profile', fail=True, document_id=gdoc, profile='srgb'),
            'webp': call('save_copy', fail=True, document_id=gdoc, path=str(folder / 'x.webp'),
                         format='webp'),
        }
        assert refusals['get_raw_source'] == RAW_REFUSAL.format('get_raw_source'), refusals
        # The close: Image Size's resolution runs on a Grayscale document.
        call('set_resolution', document_id=gdoc, ppi_x=150)
        assert mode(gdoc) == 'gray'
        call('undo', document_id=gdoc)
        refusals['set_resolution'] = 'admitted (the close)'
        # Phase 4: Image Size runs on a Grayscale document.
        resized = call('image_size', document_id=gdoc, width=2, height=2)
        assert mode(gdoc) == 'gray', resized
        call('undo', document_id=gdoc)
        refusals['image_size'] = 'admitted (phase 4)'
        assert RGB_PROFILE_PART in refusals['assign_profile'], refusals
        # Phase 5b: Proof Setup answers on a Grayscale document.
        proof9 = call('get_proof_setup', document_id=gdoc)
        assert proof9['proof_colors'] is False, proof9
        refusals['get_proof_setup'] = 'admitted (phase 5b)'
        assert refusals['webp'] == SAVE_REFUSAL, refusals
        receipts['g9'] = refusals
        print('PASS g9: a RAW tool, an RGB profile and the save sentence refuse; set_resolution, '
              'image_size and get_proof_setup run',
              flush=True)

        # ---- G10: layer structure ---------------------------------------------------------
        def layers(doc):
            return call('get_document', document_id=doc)['layers']

        def one_step(doc, tool, **arguments):
            """`tool` is ONE undo step and the document stays Grayscale."""
            before = layers(doc)
            reply = call(tool, document_id=doc, **arguments)
            after = layers(doc)
            assert after != before, (tool, 'changed nothing')
            assert mode(doc) == 'gray', tool
            call('undo', document_id=doc)
            assert layers(doc) == before, (tool, 'undo')
            call('redo', document_id=doc)
            assert layers(doc) == after, (tool, 'redo')
            assert mode(doc) == 'gray', tool
            return reply

        def neutral(doc, w, h):
            for y in range(h):
                for x in range(w):
                    px = center(doc, x, y)
                    assert px[0] == px[1] == px[2], (x, y, px)

        sdoc = call('new_document', width=8, height=4, color_mode='gray', depth=16,
                    background='#808080', profile='gray_gamma_22')['document']['id']
        one_step(sdoc, 'new_layer', name='Top')
        call('fill_selection', document_id=sdoc, layer=1, color='#ffffff')
        one_step(sdoc, 'add_layer_mask', layer=1, kind='reveal_all')
        one_step(sdoc, 'set_layer_properties', index=1, blend_mode='Multiply', opacity=0.5)
        one_step(sdoc, 'new_layer', name='Clip')
        call('fill_selection', document_id=sdoc, layer=2, color='#404040')
        one_step(sdoc, 'set_layer_clipped', layer=2, clipped=True)
        one_step(sdoc, 'set_layer_style', layer=1, style={'fill_opacity': 0.5})
        one_step(sdoc, 'stamp_visible', above=2, name='Stamp')
        one_step(sdoc, 'merge_down', index=3)
        one_step(sdoc, 'group_layers', layers=[1, 2], name='Group')
        steps = ['new_layer', 'add_layer_mask', 'set_layer_properties', 'new_layer',
                 'set_layer_clipped', 'set_layer_style', 'stamp_visible', 'merge_down',
                 'group_layers']
        neutral(sdoc, 8, 4)
        hue = call('set_layer_properties', fail=True, document_id=sdoc, index=0, blend_mode='Hue')
        assert hue.startswith(GRAY_LIMITATION_HEAD) and hue.endswith(GRAY_LIMITATION_TAIL), hue
        assert 'Hue' not in hue and 'Luminosity' not in hue, hue
        darker = call('set_layer_properties', fail=True, document_id=sdoc, index=0,
                      blend_mode='Darker Color')
        assert darker == hue and 'Darker Color' not in hue and 'Lighter Color' not in hue, darker
        # Phase 2d: an effect is one more undo step and the document stays gray.
        shadow = one_step(sdoc, 'set_layer_style', layer=0,
                          style={'effects': [{'type': 'drop_shadow'}]})
        neutral(sdoc, 8, 4)
        receipts['g10'] = {'steps': steps, 'hue': hue, 'shadow': shadow}
        print('PASS g10: the layer structure, one undo step each, R = G = B', flush=True)

        # ---- G11: Fill ----------------------------------------------------------------------
        fdoc16 = call('new_document', width=4, height=4, color_mode='gray', depth=16,
                      background='#808080', profile='gray_gamma_22')['document']['id']
        backdrop = center(fdoc16, 1, 1)
        call('new_layer', document_id=fdoc16, name='Red')
        call('fill_selection', document_id=fdoc16, layer=1, color='#ff0000')
        red = center(fdoc16, 1, 1)
        want = round(gray_of('gray_gamma_22', luminance((1, 0, 0))) * 65535)
        assert red[0] == red[1] == red[2] and abs(red[0] - want) <= BOUND16, (red, want)
        call('add_layer_mask', document_id=fdoc16, layer=1, kind='reveal_all')
        call('fill_selection', document_id=fdoc16, layer=1, target='mask', color='#000000')
        hidden = center(fdoc16, 1, 1)
        assert hidden == backdrop, (hidden, backdrop)
        receipts['g11'] = {'red': red, 'oracle16': want, 'hidden': hidden, 'backdrop': backdrop}
        print('PASS g11: Fill paints the luminance; a black mask fill hides the layer', flush=True)

        # ---- G12: one-channel saves -----------------------------------------------------------
        icc8 = folder / 'icc8.png'
        png(icc8, 3, 2, 8, 0, codes8, [iccp(icc)])
        doc8 = call('open_document', path=str(icc8))['document']['id']
        doc16 = opened['icc16.png']
        assert call('get_color_profile', document_id=doc8)['name'] == 'Drive Gray 1.8'
        saves = {}
        for depth, doc in ((8, doc8), (16, doc16)):
            originals = [center(doc, i % 3, i // 3) for i in range(6)]
            for fmt in ('png', 'tiff', 'jpeg'):
                path = folder / f'save{depth}.{fmt}'
                extra = {'jpeg_quality': 100} if fmt == 'jpeg' else {}
                call('save_copy', document_id=doc, path=str(path), format=fmt, **extra)
                data = path.read_bytes()
                bits = 8 if fmt == 'jpeg' else depth
                if fmt == 'png':
                    got_bits, colour, embedded = png_header(data)
                    assert (got_bits, colour) == (bits, 0), (path, got_bits, colour)
                elif fmt == 'tiff':
                    spp, photometric, got_bits, embedded = tiff_header(data)
                    assert (spp, photometric, got_bits) == (1, 1, bits), (path, spp, photometric)
                else:
                    nf, embedded = jpeg_header(data)
                    assert nf == 1, (path, nf)
                assert embedded == icc and embedded[16:20] == b'GRAY', path
                again = call('open_document', path=str(path))['document']['id']
                assert mode(again) == 'gray', path
                assert call('get_color_profile', document_id=again)['name'] == 'Drive Gray 1.8', path
                for i, want_px in enumerate(originals):
                    px = center(again, i % 3, i // 3)
                    assert px[0] == px[1] == px[2], (path, i, px)
                    if fmt == 'jpeg':
                        assert abs(px[0] // 257 - round(want_px[0] / 257)) <= JPEG_BOUND8, (path, i, px)
                    else:
                        assert px == want_px, (path, i, px, want_px)
                saves[f'{depth}.{fmt}'] = len(data)

        def preservation(doc):
            return call('get_document', document_id=doc)['save_preservation']

        kept16 = preservation(doc16)
        assert 'public.png' in kept16['save_types'] and 'public.tiff' in kept16['save_types'], kept16
        assert 'public.jpeg' not in kept16['save_types'], kept16
        call('add_layer_mask', document_id=doc16, layer=0, kind='reveal_all')
        masked16 = preservation(doc16)
        assert 'public.png' not in masked16['save_types'], masked16
        assert 'public.tiff' not in masked16['save_types'], masked16
        assert sorted(masked16['losses_by_format']) == sorted(kept16['losses_by_format']), masked16
        kept8 = preservation(doc8)
        assert 'public.jpeg' in kept8['save_types'], kept8
        receipts['g12'] = {'bytes': saves, 'save_types16': kept16['save_types'],
                           'save_types16_masked': masked16['save_types'],
                           'save_types8': kept8['save_types']}
        print('PASS g12: one-channel PNG, TIFF and JPEG saves with the Gray ICC reopen', flush=True)

        # ---- G13: exports ---------------------------------------------------------------------
        exports = {}
        for name, settings in (('e16.png', dict(format='png', bit_depth=16, profile='document')),
                               ('e8.tif', dict(format='tiff', bit_depth=8, profile='document')),
                               ('rgb.png', dict(format='png', bit_depth=8, profile='srgb'))):
            path = folder / name
            call('export_image', document_id=doc8, path=str(path), settings=settings)
            data = path.read_bytes()
            if name == 'e16.png':
                got_bits, colour, embedded = png_header(data)
                assert (got_bits, colour) == (16, 0) and embedded == icc, (name, got_bits, colour)
            elif name == 'e8.tif':
                spp, photometric, got_bits, embedded = tiff_header(data)
                assert (spp, photometric, got_bits) == (1, 1, 8) and embedded == icc, name
            else:
                _bits, colour, _icc = png_header(data)
                assert colour in (2, 6), (name, colour)
            exports[name] = len(data)
        receipts['g13'] = exports
        print('PASS g13: export_image keeps one channel; srgb writes RGB', flush=True)

        # ---- G14: .rz of the structure document ---------------------------------------------------
        rz10 = folder / 'structure.rz'
        call('save_copy', document_id=sdoc, path=str(rz10), format='rz')
        rdoc10 = call('open_document', path=str(rz10))['document']['id']
        assert mode(rdoc10) == 'gray'
        for y in range(4):
            for x in range(8):
                assert center(rdoc10, x, y) == center(sdoc, x, y), (x, y)
        receipts['g14'] = str(rz10)
        print('PASS g14: the .rz round trip keeps the structure document', flush=True)

        # ---- G15: get_precision -------------------------------------------------------------------
        precision = {}
        for depth in (8, 16, 32):
            pdoc = call('new_document', width=4, height=2, color_mode='gray', depth=depth,
                        background='#808080', profile='dot_gain_20')['document']['id']
            reply = call('get_precision', document_id=pdoc)
            assert reply['color_mode'] == 'gray', reply
            assert reply['bits_per_channel'] == depth and reply['mode'] == f'gray{depth}', reply
            assert reply['supported_tools'] == GRAY_TOOLS, reply['supported_tools']
            assert 'brush_stroke' in reply['supported_tools']
            modes = GRAY_MODES_32 if depth == 32 else GRAY_MODES
            assert reply['blend_modes'] == modes + ['Pass Through (groups)'], reply['blend_modes']
            support = call('get_document', document_id=pdoc)['precision_support']
            assert support['supported_tools'] == reply['supported_tools'], support
            precision[depth] = {k: reply[k] for k in ('mode', 'bits_per_channel', 'mode_description',
                                                      'gray_profile')}
        receipts['g15'] = precision
        print('PASS g15: get_precision answers the Grayscale lists at 8, 16 and 32 bits', flush=True)

        # ---- G16: render's gray channel ---------------------------------------------------------
        def image(tool, **arguments):
            result = app.client.rpc('tools/call', {'name': tool, 'arguments': arguments})
            assert not result.get('isError'), (tool, result)
            data = next(c['data'] for c in result['content'] if c['type'] == 'image')
            text = next((c['text'] for c in result['content'] if c['type'] == 'text'), '')
            return png_pixels(base64.b64decode(data)), text

        k8 = [0, 17, 64, 128, 200, 255]
        k16 = [0, 1000, 12345, 30000, 50000, 65535]
        k8_path, k16_path = folder / 'k8.png', folder / 'k16.png'
        png(k8_path, 6, 1, 8, 0, k8)
        png(k16_path, 6, 1, 16, 0, k16, [iccp(icc)])
        kdoc8 = call('open_document', path=str(k8_path))['document']['id']
        kdoc16 = call('open_document', path=str(k16_path))['document']['id']
        kdoc32 = call('new_document', width=6, height=1, color_mode='gray', depth=32,
                      background='#808080', profile='dot_gain_20')['document']['id']
        rendered = {}
        for doc, want in ((kdoc8, k8), (kdoc16, [round(c / 257) for c in k16])):
            (w, h, px), text = image('render', document_id=doc, channel='gray')
            assert (w, h) == (6, 1) and RENDER_8_16 in text, (w, h, text)
            for i, code in enumerate(want):
                assert all(abs(v - code) <= RENDER_BOUND8 for v in px[i][:3]), (doc, i, px[i], code)
            rendered[doc] = [list(p) for p in px]
        _, text32 = image('render', document_id=kdoc32, channel='gray')
        assert RENDER_32 in text32, text32
        render_refusals = {
            'red': call('render', fail=True, document_id=kdoc16, channel='red'),
            'composite': call('render', fail=True, document_id=kdoc16, channel='composite'),
            'gray_layer': call('render', fail=True, document_id=kdoc16, channel='gray', layer=0),
        }
        assert all(r == RENDER_REFUSAL for r in render_refusals.values()), render_refusals
        call('new_layer', document_id=kdoc8, name='Masked')
        call('add_layer_mask', document_id=kdoc8, layer=1, kind='hide_all')
        (w, h, mask), _ = image('render', document_id=kdoc8, channel='mask', layer=1)
        assert all(v == 0 for p in mask for v in p[:3]), mask
        # Phase 2d: an alpha channel by name.
        call('add_channel', document_id=kdoc8, name='Matte')
        (w, h, matte), matte_text = image('render', document_id=kdoc8, channel='Matte')
        assert (w, h) == (6, 1) and 'native channel Matte' in matte_text, (w, h, matte_text)
        assert all(v == 0 for p in matte for v in p[:3]), matte
        receipts['g16'] = {'rendered': rendered, 'text32': text32, 'refusals': render_refusals,
                           'matte': matte_text}
        print('PASS g16: render channel gray reads the codes; other channels refuse', flush=True)

        # ---- G17: Export As and Quick Export -----------------------------------------------------
        (pw, ph, preview), _ = image('export_preview', document_id=kdoc16,
                                     settings=dict(format='png', bit_depth=16, profile='document'))
        (rw, rh, composite), _ = image('render', document_id=kdoc16)
        assert (pw, ph) == (rw, rh) == (6, 1), (pw, ph, rw, rh)
        worst_export = max(abs(a - b) for p, q in zip(preview, composite) for a, b in zip(p[:3], q[:3]))
        assert worst_export <= EXPORT_BOUND8, (preview, composite)
        destination = folder / 'quick'
        destination.mkdir()
        quick = call('quick_export', document_id=kdoc16, destination=str(destination),
                     pattern='{name}-q', settings=dict(format='png', bit_depth=16, profile='document'))
        got_bits, colour, embedded = png_header(Path(quick['path']).read_bytes())
        assert (got_bits, colour) == (16, 0) and embedded == icc, (quick, got_bits, colour)
        webp = call('quick_export', fail=True, document_id=kdoc16, destination=str(destination),
                    pattern='{name}-w', settings=dict(format='webp', bit_depth=8, profile='document'))
        assert FORMAT_REFUSAL_PART in webp, webp
        receipts['g17'] = {'export_worst': worst_export, 'quick': quick['path'], 'webp': webp}
        print('PASS g17: Export As previews and Quick Export writes one channel', flush=True)

        # ---- G18: K % ---------------------------------------------------------------------------
        ks = {}
        for doc, codes, top in ((kdoc8, k8, 255), (kdoc16, k16, 65535)):
            for i, code in enumerate(codes):
                sample = call('sample_color', document_id=doc, x=i, y=0)
                want = 100 * (1 - code / top)
                assert abs(sample['gray_k_percent'] - want) <= K_BOUND, (doc, i, sample, want)
                ks.setdefault(top, []).append(sample['gray_k_percent'])
        assert 'gray_k_percent' not in call('sample_color', document_id=kdoc32, x=1, y=0)
        receipts['g18'] = ks
        print('PASS g18: sample_color reports K % at 8 and 16 bits, none at 32', flush=True)

        # ---- G19–G24: painting and adjustments (phase 2a-ii and 2b) ---------------------------
        def new_gray(depth, w=12, h=8, background='#808080'):
            return call('new_document', width=w, height=h, color_mode='gray', depth=depth,
                        background=background, profile='gray_gamma_22')['document']['id']

        def samples(doc, points):
            return [center(doc, x, y) for x, y in points]

        def pixel_step(doc, probe, tool, **arguments):
            """`tool` changes the sampled pixels in ONE undo step; the document stays gray."""
            before = samples(doc, probe)
            reply = call(tool, document_id=doc, **arguments)
            after = samples(doc, probe)
            assert after != before, (tool, 'changed nothing')
            assert mode(doc) == 'gray', tool
            call('undo', document_id=doc)
            assert samples(doc, probe) == before, (tool, 'undo')
            call('redo', document_id=doc)
            assert samples(doc, probe) == after, (tool, 'redo')
            assert mode(doc) == 'gray', tool
            return after, reply

        def assert_neutral(pixels, what):
            for px in pixels:
                assert px[0] == px[1] == px[2], (what, px)

        red16 = round(gray_of('gray_gamma_22', luminance((1, 0, 0))) * 65535)
        W = 12
        row = [(x, 4) for x in range(W)]

        # G19: Brush and Eraser at 16 bits.
        bdoc = new_gray(16)
        (brushed,), _ = pixel_step(bdoc, [(5, 2)], 'brush_stroke', points=[[1, 2], [10, 2]],
                                   size=4, hardness=100, color='#ff0000')
        assert brushed[0] == brushed[1] == brushed[2] and abs(brushed[0] - red16) <= BOUND16, (
            brushed, red16)
        (erased,), _ = pixel_step(bdoc, [(5, 6)], 'eraser_stroke', points=[[1, 6], [10, 6]],
                                  size=4, hardness=100)
        assert erased[3] == 0, erased
        hue_brush = call('brush_stroke', fail=True, document_id=bdoc, points=[[1, 2], [10, 2]],
                         size=4, color='#ff0000', blend_mode='Hue')
        assert hue_brush.startswith(GRAY_LIMITATION_HEAD), hue_brush
        assert hue_brush.endswith(GRAY_LIMITATION_TAIL), hue_brush
        receipts['g19'] = {'brush': brushed, 'oracle16': red16, 'erased': erased, 'hue': hue_brush}
        print('PASS g19: the Brush paints red\'s luminance and the Eraser clears, one step each',
              flush=True)

        # G20: the bucket, Fill's paint and the Gradient at 16 bits.
        fills = {}
        for name, extra in (('bucket', {}), ('paint', {'contiguous': False})):
            fdoc20 = new_gray(16)
            (px,), _ = pixel_step(fdoc20, [(3, 3)], 'fill', x=3, y=3, color='#ff0000',
                                  tolerance=0, **extra)
            assert px[0] == px[1] == px[2] and abs(px[0] - red16) <= BOUND16, (name, px, red16)
            fills[name] = px
        ramp = new_gray(16)
        ramped, _ = pixel_step(ramp, row, 'gradient', x0=1, y0=4, x1=W - 1, y1=4,
                               start_color='#000000', end_color='#ffffff')
        assert_neutral(ramped, 'gradient')
        assert ramped[0][0] <= BOUND16 and ramped[-1][0] >= 65535 - BOUND16, ramped
        receipts['g20'] = {'fills': fills, 'ramp': ramped}
        print('PASS g20: the bucket, Fill and the Gradient paint gray, one step each', flush=True)

        # G21: 8 and 32 bits.
        doc8b = new_gray(8)
        call('brush_stroke', document_id=doc8b, points=[[1, 2], [10, 2]], size=4, hardness=100,
             color='#ffffff')
        white8 = center(doc8b, 5, 2)
        assert white8[0] == white8[1] == white8[2] and abs(white8[0] / 257 - 255) <= BOUND8, white8
        doc32b = new_gray(32)
        call('brush_stroke', document_id=doc32b, points=[[1, 2], [10, 2]], size=4, hardness=100,
             color='#ff0000')
        red32 = call('sample_color', document_id=doc32b, x=5, y=2)['center_rgba32']
        assert red32[0] == red32[1] == red32[2], red32
        assert mode(doc8b) == 'gray' and mode(doc32b) == 'gray'
        receipts['g21'] = {'white8': white8, 'red32': red32}
        print('PASS g21: the Brush paints gray at 8 and 32 bits', flush=True)

        # G22: apply_filter on the ramp.
        before = samples(ramp, row)
        inverted, _ = pixel_step(ramp, row, 'apply_filter', layer=0, filter='invert')
        assert all(a[i] == 65535 - b[i] for a, b in zip(inverted, before) for i in range(3)), (
            inverted, before)
        assert [p[3] for p in inverted] == [p[3] for p in before], inverted
        call('undo', document_id=ramp)
        levelled, _ = pixel_step(ramp, row, 'apply_filter', layer=0, filter='levels', gamma=1.4)
        assert_neutral(levelled, 'levels')
        call('undo', document_id=ramp)
        mapped, _ = pixel_step(ramp, row, 'apply_filter', layer=0, filter='gradient_map',
                               params={'gradient': {'stops': [
                                   {'position': 0, 'color': '#ff0000'},
                                   {'position': 1, 'color': '#00ffff'}]}})
        assert_neutral(mapped, 'gradient_map')
        # The coloured stops ran, not the default black-to-white map (identity on this ramp):
        # red's gray lifts the black end and cyan's gray lowers the white end.
        assert mapped[0][0] >= before[0][0] + 0.1 * 65535, (mapped, before)
        assert mapped[-1][0] <= before[-1][0] - 0.05 * 65535, (mapped, before)
        call('undo', document_id=ramp)
        assert samples(ramp, row) == before
        receipts['g22'] = {'invert': inverted, 'levels': levelled, 'gradient_map': mapped}
        print('PASS g22: apply_filter invert, levels and gradient_map run on gray, one step each',
              flush=True)

        # G23: adjustment layers.
        def layer_step(doc, tool, **arguments):
            before_layers = layers(doc)
            before_px = samples(doc, row)
            reply = call(tool, document_id=doc, **arguments)
            after_layers, after_px = layers(doc), samples(doc, row)
            assert (after_layers, after_px) != (before_layers, before_px), (tool, 'changed nothing')
            assert mode(doc) == 'gray', tool
            assert_neutral(after_px, tool)
            call('undo', document_id=doc)
            assert (layers(doc), samples(doc, row)) == (before_layers, before_px), (tool, 'undo')
            call('redo', document_id=doc)
            assert (layers(doc), samples(doc, row)) == (after_layers, after_px), (tool, 'redo')
            return reply

        adoc = new_gray(16)
        call('gradient', document_id=adoc, x0=1, y0=4, x1=W - 1, y1=4, start_color='#000000',
             end_color='#ffffff')
        levels_layer = layer_step(adoc, 'add_adjustment_layer', op='levels', params={'gamma': 1.4},
                                  name='Levels')
        layer_step(adoc, 'add_adjustment_layer', op='threshold', params={'level': 0.5},
                   name='Threshold')
        call('set_layer_properties', document_id=adoc, index=2, visible=False)
        layer_step(adoc, 'edit_adjustment_layer', layer=levels_layer['layer'],
                   params={'gamma': 0.7})
        count = len(layers(adoc))
        rz23 = folder / 'adjusted.rz'
        call('save_copy', document_id=adoc, path=str(rz23), format='rz')
        rdoc23 = call('open_document', path=str(rz23))['document']['id']
        assert mode(rdoc23) == 'gray' and len(layers(rdoc23)) == count, layers(rdoc23)
        assert samples(rdoc23, row) == samples(adoc, row)
        sh = call('add_adjustment_layer', fail=True, document_id=adoc, op='shadows_highlights')
        assert sh == SH_LAYER_REFUSAL, sh
        receipts['g23'] = {'layers': count, 'shadows_highlights': sh}
        print('PASS g23: Levels and Threshold layers, their edit and .rz; S/H refuses as a layer',
              flush=True)

        # G24: exact refusals, then Auto Tone, Auto Contrast and Equalize.
        refusals24 = {
            'hue_saturation': call('apply_filter', fail=True, document_id=ramp, layer=0,
                                   filter='hue_saturation', params={'hue': 10}),
            'black_and_white': call('add_adjustment_layer', fail=True, document_id=ramp,
                                    op='black_and_white'),
            'auto_color': call('auto_color', fail=True, document_id=ramp, layer=0),
            'sepia': call('apply_filter', fail=True, document_id=ramp, layer=0, filter='sepia'),
            'levels_r': call('apply_filter', fail=True, document_id=ramp, layer=0,
                             filter='levels', r={'gamma': 1.2}),
            'target_red': call('apply_filter', fail=True, document_id=ramp, layer=0,
                               filter='levels', gamma=1.2, target='red'),
        }
        want24 = {
            'hue_saturation': UNAVAILABLE_REFUSAL.format('Hue/Saturation'),
            'black_and_white': UNAVAILABLE_REFUSAL.format('Black & White'),
            'auto_color': UNAVAILABLE_REFUSAL.format('Auto Color'),
            'sepia': RGB_ONLY_REFUSAL.format('Sepia'),
            'levels_r': LEVELS_CHANNELS,
            'target_red': TARGET_REFUSAL,
        }
        assert refusals24 == want24, refusals24
        autos = {}
        for tool, extra in (('auto_tone', {}), ('auto_contrast', {}),
                            ('equalize', {'scope': 'entire'})):
            adoc24 = new_gray(16)
            call('gradient', document_id=adoc24, x0=1, y0=4, x1=W - 1, y1=4,
                 start_color='#404040', end_color='#c0c0c0')
            changed, _ = pixel_step(adoc24, row, tool, layer=0, **extra)
            assert_neutral(changed, tool)
            autos[tool] = changed
        receipts['g24'] = {'refusals': refusals24, 'autos': autos}
        print('PASS g24: refusals name the Grayscale sentence, RGB only, the plane rule and the '
              'channel rule; '
              'Auto runs',
              flush=True)

        # ---- G25–G27: filters, Fade, Actions and retouch (phase 2c) -----------------------------
        pattern_codes = [((x * 7919 + y * 104729 + 1013) % 251) * 257 + (x * y) % 200
                         for y in range(PATTERN_H) for x in range(PATTERN_W)]
        copies = [0]

        def pattern():
            """A fresh 16-bit pattern document (one file per document: open_document answers an
            already open path's document)."""
            copies[0] += 1
            path = folder / f'pattern-{copies[0]}.png'
            png(path, PATTERN_W, PATTERN_H, 16, 0, pattern_codes)
            doc = call('open_document', path=str(path))['document']['id']
            assert mode(doc) == 'gray', path
            return doc

        stroke_row = [(x, 8) for x in range(3, 22, 2)]
        grid = [(x, y) for y in range(1, PATTERN_H, 3) for x in range(1, PATTERN_W, 3)]

        # G25: filters at 16 bits, Clouds, 8 and 32 bits, Liquify.
        g25 = {}
        for name, args in (('blur', dict(sigma=2)), ('unsharp_mask', dict(radius=2, amount=1.5)),
                           ('emboss', {}), ('add_noise', dict(amount=0.2, seed=5)),
                           ('lens_correction', dict(chromatic_red_cyan=50, distortion=20))):
            pdoc = pattern()
            changed, _ = pixel_step(pdoc, grid, 'apply_filter', layer=0, filter=name, **args)
            assert_neutral(changed, name)
            g25[name] = changed[:4]
        red_grey = round(gray_of('gray_gamma_22', luminance((1, 0, 0))) * 255) * 257
        blue_grey = round(gray_of('gray_gamma_22', luminance((0, 0, 1))) * 255) * 257
        cdoc25 = new_gray(16, w=PATTERN_W, h=PATTERN_H)
        clouded, _ = pixel_step(cdoc25, grid, 'apply_filter', layer=0, filter='clouds', seed=3,
                                foreground_color='#ff0000', background_color='#0000ff')
        assert_neutral(clouded, 'clouds')
        low, high = min(red_grey, blue_grey), max(red_grey, blue_grey)
        for px in clouded:
            assert low - CLOUDS_BOUND16 <= px[0] <= high + CLOUDS_BOUND16, (px, low, high)
        g25['clouds'] = {'samples': clouded[:4], 'red_grey': red_grey, 'blue_grey': blue_grey}
        for depth in (8, 32):
            bdoc25 = new_gray(depth, w=PATTERN_W, h=PATTERN_H)
            call('gradient', document_id=bdoc25, x0=1, y0=4, x1=PATTERN_W - 1, y1=12,
                 start_color='#000000', end_color='#ffffff')
            call('apply_filter', document_id=bdoc25, layer=0, filter='blur', sigma=2)
            assert mode(bdoc25) == 'gray', depth
            for x, y in grid:
                sample = call('sample_color', document_id=bdoc25, x=x, y=y)
                px = sample.get('center_rgba32') or center(bdoc25, x, y)
                assert px[0] == px[1] == px[2], (depth, x, y, px)
        ldoc25 = new_gray(16)
        liquify = call('liquify', document_id=ldoc25,
                       strokes=[dict(tool='forward_warp', points=[[3, 3], [8, 4]], size=6,
                                     strength=100, density=100)])
        assert mode(ldoc25) == 'gray', liquify
        g25['liquify'] = liquify
        receipts['g25'] = g25
        print('PASS g25: filters run on gray at 8, 16 and 32 bits, one step each; Clouds is greyed;'
              ' Liquify runs (phase 4)', flush=True)

        # G26: Fade and Actions.
        fdoc26 = pattern()
        before26 = samples(fdoc26, grid)
        call('apply_filter', document_id=fdoc26, layer=0, filter='blur', sigma=2)
        blurred26 = samples(fdoc26, grid)
        assert blurred26 != before26
        hue_fade = call('fade', fail=True, document_id=fdoc26, opacity=0.5, mode='Hue')
        assert hue_fade.startswith(GRAY_LIMITATION_HEAD), hue_fade
        assert hue_fade.endswith(GRAY_LIMITATION_TAIL), hue_fade
        faded = call('fade', document_id=fdoc26, opacity=0.5)
        assert faded.get('changed') is True, faded
        faded26 = samples(fdoc26, grid)
        assert_neutral(faded26, 'fade')
        for b, a, f in zip(before26, blurred26, faded26):
            assert abs(f[0] - (a[0] + b[0]) / 2) <= FADE_BOUND16, (b, a, f)
        assert mode(fdoc26) == 'gray'
        call('undo', document_id=fdoc26)
        assert samples(fdoc26, grid) == before26, 'one undo returns the pre-blur samples'
        call('save_action', name='gray-blur-547', overwrite=True,
             steps=[{'tool': 'apply_filter', 'arguments': {'filter': 'blur', 'sigma': 2}}])
        played, direct = pattern(), pattern()
        call('run_action', document_id=played, name='gray-blur-547')
        call('apply_filter', document_id=direct, layer=0, filter='blur', sigma=2)
        assert samples(played, grid) == samples(direct, grid), 'the Action replays the blur'
        assert samples(played, grid) != before26
        assert mode(played) == 'gray'
        call('delete_action', name='gray-blur-547')
        receipts['g26'] = {'before': before26[:4], 'blurred': blurred26[:4], 'faded': faded26[:4],
                           'hue': hue_fade}
        print('PASS g26: Fade halves a blur and undoes it whole; an Action replays the blur',
              flush=True)

        # G27: retouch on 16 bits.
        g27 = {}
        stroke = [[3, 8], [21, 8]]
        source = dict(source_x=12, source_y=3)
        cases = [
            ('clone_stamp', dict(points=stroke, size=6, sample='current', **source)),
            ('heal_stroke', dict(points=stroke, size=6, sample='current', **source)),
            ('spot_heal_stroke', dict(points=[[12, 8]], size=6)),
            ('patch_region', dict(points=[[4, 5], [11, 5], [11, 11], [4, 11]], offset_x=10,
                                  offset_y=0)),
            ('dodge_burn', dict(points=stroke, size=6, exposure=60, range='midtones')),
            ('dodge_burn', dict(points=stroke, size=6, exposure=60, range='midtones', burn=True)),
            ('blur_stroke', dict(points=stroke, size=6, strength=100)),
            ('sharpen_stroke', dict(points=stroke, size=6, strength=100)),
            ('smudge_stroke', dict(points=stroke, size=6, strength=90)),
        ]
        probes = stroke_row + [(x, y) for y in range(5, 12) for x in range(4, 12)]
        for tool, args in cases:
            rdoc27 = pattern()
            changed, _ = pixel_step(rdoc27, probes, tool, layer=0, **args)
            assert_neutral(changed, tool)
            g27[tool + ('_burn' if args.get('burn') else '')] = changed[:4]
        hdoc27 = pattern()
        state = call('list_history', document_id=hdoc27)['current_state_id']
        call('apply_filter', document_id=hdoc27, layer=0, filter='invert')
        changed, _ = pixel_step(hdoc27, stroke_row, 'history_brush_stroke', layer=0, points=stroke,
                                size=6, opacity=100, state_id=state)
        assert_neutral(changed, 'history_brush_stroke')
        g27['history_brush_stroke'] = changed[:4]
        sponged = {}
        for sponge_mode in ('saturate', 'desaturate'):
            udoc27 = new_gray(16, w=PATTERN_W, h=PATTERN_H, background='#3d3d3d')
            start = center(udoc27, 12, 8)
            call('sponge_stroke', document_id=udoc27, layer=0, points=stroke, size=6,
                 mode=sponge_mode, strength=80)
            end = center(udoc27, 12, 8)
            assert end[0] == end[1] == end[2], (sponge_mode, end)
            if sponge_mode == 'saturate':
                assert end[0] < start[0], (sponge_mode, start, end)
            else:
                assert start[0] < end[0] <= 0x8000, (sponge_mode, start, end)
            sponged[sponge_mode] = {'start': start, 'end': end}
        g27['sponge'] = sponged
        clone_hue = call('clone_stamp', fail=True, document_id=pattern(), layer=0, points=stroke,
                         size=6, blend_mode='Hue', **source)
        assert clone_hue.startswith(GRAY_LIMITATION_HEAD), clone_hue
        assert clone_hue.endswith(GRAY_LIMITATION_TAIL), clone_hue
        red_eye = call('red_eye', fail=True, document_id=pattern(), layer=0, x=4, y=4, width=8,
                       height=8)
        assert red_eye == RED_EYE_REFUSAL, red_eye
        g27['refusals'] = {'clone_hue': clone_hue, 'red_eye': red_eye}
        receipts['g27'] = g27
        print('PASS g27: the retouch tools run on gray, one step each; the Sponge works around '
              'the middle gray; Hue and Red Eye refuse', flush=True)

        # ---- G28–G31: effects, Blend If, presets and channels (phase 2d) -----------------------
        every = [(x, y) for y in range(EFFECTS_H) for x in range(EFFECTS_W)]
        inside = [(x, y) for y in range(4, 7) for x in range(4, 8)]

        def effects_doc(depth):
            """The effects document (module doc): #808080, layer 1 an opaque #c0c0c0 square."""
            doc = new_gray(depth, w=EFFECTS_W, h=EFFECTS_H)
            call('new_layer', document_id=doc, name='Square')
            call('select_rect', document_id=doc, x=3, y=3, width=6, height=5)
            call('fill_selection', document_id=doc, layer=1, color='#c0c0c0')
            call('deselect', document_id=doc)
            return doc

        def any_depth(doc, points):
            """The samples as codes, or the linear values at 32 bits."""
            out = []
            for x, y in points:
                sample = call('sample_color', document_id=doc, x=x, y=y)
                out.append(sample.get('center_rgba32') or center(doc, x, y))
            return out

        def square_style(doc):
            return next(row for row in layers(doc) if row.get('index') == 1).get('style')

        def stops(a, b):
            return {'stops': [{'position': 0, 'color': a, 'opacity': 1},
                              {'position': 1, 'color': b, 'opacity': 1}]}

        # G28: every effect at 16 bits; Color Overlay red's gray; Hue; 8 and 32 bits.
        edoc = effects_doc(16)
        full_style = {'effects': [
            {'type': 'drop_shadow', 'color': '#ff0000', 'distance': 2, 'size': 2},
            {'type': 'color_overlay', 'color': '#00ff00', 'opacity': 0.5},
            {'type': 'stroke', 'color': '#0000ff', 'size': 1},
            {'type': 'outer_glow', 'size': 2},
            {'type': 'bevel_emboss', 'size': 2},
            {'type': 'satin', 'distance': 2, 'size': 2},
            {'type': 'gradient_overlay', 'opacity': 0.5, 'gradient': stops('#ff0000', '#0000ff')},
            {'type': 'pattern_overlay', 'opacity': 0.4, 'pattern': {'preset': 'stripes'}},
        ]}
        styled28, _ = pixel_step(edoc, every, 'set_layer_style', layer=1, style=full_style)
        assert_neutral(styled28, 'every effect')
        odoc = effects_doc(16)
        call('set_layer_style', document_id=odoc, layer=1,
             style={'effects': [{'type': 'color_overlay', 'color': '#ff0000'}]})
        overlay = next(e for e in square_style(odoc)['effects'] if e['type'] == 'color_overlay')
        hexa = overlay['color']
        v = int(hexa[1:3], 16)
        assert hexa == '#%02x%02x%02x' % (v, v, v), hexa
        v_oracle = round(255 * srgb_encode(SRGB_Y[0]))
        assert abs(v - v_oracle) <= OVERLAY_HEX_BOUND, (v, v_oracle)
        want28 = 257 * round(255 * srgb_decode(v / 255) ** (1 / 2.2))
        for px in samples(odoc, inside):
            assert px[0] == px[1] == px[2] and abs(px[0] - want28) <= BOUND16, (px, want28)
        hue28 = call('set_layer_style', fail=True, document_id=odoc, layer=1,
                     style={'effects': [{'type': 'drop_shadow', 'blend': 'hue'}]})
        assert HUE_EFFECT in hue28, hue28
        for depth in (8, 32):
            ddoc = effects_doc(depth)
            call('set_layer_style', document_id=ddoc, layer=1, style={'effects': [
                {'type': 'drop_shadow', 'color': '#ff0000', 'distance': 2, 'size': 2}]})
            assert mode(ddoc) == 'gray', depth
            assert_neutral(any_depth(ddoc, every), f'drop shadow at {depth}')
        receipts['g28'] = {'overlay_hex': hexa, 'oracle_v': v_oracle, 'oracle16': want28,
                           'hue': hue28}
        print('PASS g28: every layer effect composites gray in one undo step; red overlays its '
              'gray; Hue refuses', flush=True)

        # G29: Blend If on Gray hides the dark row and keeps the light one; Red refuses.
        bdoc29 = new_gray(16, w=EFFECTS_W, h=EFFECTS_H, background='#ffffff')
        call('new_layer', document_id=bdoc29, name='Rows')
        for y, colour in ((2, '#202020'), (6, '#e0e0e0')):
            call('select_rect', document_id=bdoc29, x=0, y=y, width=EFFECTS_W, height=1)
            call('fill_selection', document_id=bdoc29, layer=1, color=colour)
        call('deselect', document_id=bdoc29)
        dark_row = [(x, 2) for x in range(EFFECTS_W)]
        light_row = [(x, 6) for x in range(EFFECTS_W)]
        backdrop29 = center(bdoc29, 0, 0)
        light29 = samples(bdoc29, light_row)
        blended, _ = pixel_step(bdoc29, dark_row + light_row, 'set_layer_style', layer=1,
                                style={'blend_if': {'channel': 'gray',
                                                    'this_layer': [128, 128, 255, 255],
                                                    'underlying': [0, 0, 255, 255]}})
        assert all(px == backdrop29 for px in blended[:EFFECTS_W]), (blended, backdrop29)
        assert blended[EFFECTS_W:] == light29, (blended, light29)
        red29 = call('set_layer_style', fail=True, document_id=bdoc29, layer=1,
                     style={'blend_if': {'channel': 'red', 'this_layer': [0, 0, 255, 255],
                                         'underlying': [0, 0, 255, 255]}})
        assert red29 == RED_BLEND_IF, red29
        receipts['g29'] = {'dark': blended[0], 'light': blended[EFFECTS_W], 'red': red29}
        print('PASS g29: Blend If on Gray hides by the layer\'s code in one undo step; Red refuses',
              flush=True)

        # G30: an Action, a style preset, the global light and the .rz round trip.
        action_style = {'effects': [
            {'type': 'drop_shadow', 'color': '#ff0000', 'distance': 2, 'size': 2},
            {'type': 'color_overlay', 'color': '#00ff00', 'opacity': 0.5}]}
        call('save_action', name='gray-style-547', overwrite=True,
             steps=[{'tool': 'set_layer_style', 'arguments': {'layer': 1, 'style': action_style}}])
        played30, direct30 = effects_doc(16), effects_doc(16)
        call('run_action', document_id=played30, name='gray-style-547')
        call('set_layer_style', document_id=direct30, layer=1, style=action_style)
        played_px = samples(played30, every)
        assert played_px == samples(direct30, every), 'the Action replays the style'
        assert played_px != samples(effects_doc(16), every), 'the Action changed nothing'
        assert_neutral(played_px, 'action')
        call('delete_action', name='gray-style-547')
        rgb30 = call('new_document', width=EFFECTS_W, height=EFFECTS_H, depth=16,
                     background='#808080')['document']['id']
        call('new_layer', document_id=rgb30, name='Square')
        call('select_rect', document_id=rgb30, x=3, y=3, width=6, height=5)
        call('fill_selection', document_id=rgb30, layer=1, color='#c0c0c0')
        call('deselect', document_id=rgb30)
        call('set_layer_style', document_id=rgb30, layer=1, style={'effects': [
            {'type': 'color_overlay', 'color': '#ff0000'},
            {'type': 'drop_shadow', 'color': '#00ff00', 'distance': 2, 'size': 2}]})
        call('save_style_preset', document_id=rgb30, layer=1, name='gray-547-preset', replace=True)
        pdoc30 = effects_doc(16)
        preset30, _ = pixel_step(pdoc30, every, 'apply_style_preset', layers=[1],
                                 name='gray-547-preset')
        assert_neutral(preset30, 'apply_style_preset')
        lit30, _ = pixel_step(pdoc30, every, 'set_global_light', angle=30, altitude=40)
        assert_neutral(lit30, 'set_global_light')
        rz30 = folder / 'styled.rz'
        call('save_copy', document_id=pdoc30, path=str(rz30), format='rz')
        back30 = call('open_document', path=str(rz30))['document']['id']
        assert mode(back30) == 'gray'
        assert samples(back30, every) == samples(pdoc30, every), 'the .rz samples'
        assert square_style(back30) == square_style(pdoc30), 'the .rz style'
        receipts['g30'] = {'style': square_style(pdoc30)}
        print('PASS g30: an Action, a style preset and the global light each make gray in one '
              'step; the .rz keeps the style', flush=True)

        # G31: alpha channels as targets at 16 and 32 bits.
        g31 = {}
        for depth in (16, 32):
            cdoc = effects_doc(depth)
            composite = any_depth(cdoc, every)

            def channel_render():
                (_, _, px), _ = image('render', document_id=cdoc, channel='Alpha 1')
                return px

            def channel_count():
                return len(call('list_channels', document_id=cdoc)['channels'])

            call('add_channel', document_id=cdoc, name='Alpha 1')
            assert channel_count() == 1, depth
            edits = [
                ('brush_stroke', dict(layer=0, points=[[1, 8], [10, 8]], size=3, hardness=100,
                                      target='channel:Alpha 1')),
                ('fill_selection', dict(layer=0, color='#ffffff', target='channel:Alpha 1')),
            ]
            if depth == 16:
                edits.append(('apply_filter', dict(layer=0, filter='blur', sigma=2,
                                                   target='channel:Alpha 1')))
            for tool, arguments in edits:
                if tool == 'fill_selection':
                    call('select_rect', document_id=cdoc, x=1, y=1, width=4, height=4)
                before = channel_render()
                call(tool, document_id=cdoc, **arguments)
                call('deselect', document_id=cdoc)
                assert channel_render() != before, (depth, tool, 'the channel did not change')
                assert any_depth(cdoc, every) == composite, (depth, tool, 'the composite changed')
                assert mode(cdoc) == 'gray', (depth, tool)
            call('delete_channel', document_id=cdoc, channel='Alpha 1')
            assert channel_count() == 0, depth
            call('undo', document_id=cdoc)
            assert channel_count() == 1, depth
            call('redo', document_id=cdoc)
            assert channel_count() == 0, depth
            call('select_rect', document_id=cdoc, x=1, y=1, width=4, height=4)
            # Phase 4: Save Selection as a channel and the plane sources run; a colour plane
            # refuses for good.
            from_selection = call('add_channel', document_id=cdoc, name='S',
                                  **{'from': 'selection'})
            from_plane = call('add_channel', document_id=cdoc, name='P', **{'from': 'plane'})
            assert channel_count() == 2, depth
            red_plane = call('add_channel', fail=True, document_id=cdoc, name='R',
                             **{'from': 'plane', 'plane': 'red'})
            assert red_plane == ADD_CHANNEL_PLANE, red_plane
            call('deselect', document_id=cdoc)
            g31[depth] = {'selection': from_selection, 'plane': from_plane}
        receipts['g31'] = g31
        print('PASS g31: an alpha channel takes the Brush, Fill and a filter without touching the '
              'composite; Delete is one step; Save Selection and plane sources run (phase 4)',
              flush=True)

        # ---- G32: Photoshop's Grayscale PSD, PSB and layered TIFF open (phase 3a) -------------
        def bits_of(doc):
            return call('get_precision', document_id=doc)['bits_per_channel']

        g32 = {}
        gray_files = folder / 'gray-files'
        gray_files.mkdir()
        for name, depth in GRAY_FIXTURE_FILES:
            data = (GRAY_FIXTURES / name).read_bytes()
            path = gray_files / name
            path.write_bytes(data)
            report = call('inspect_psd', path=str(path))
            assert report['import_precision'] == 'gray', (name, report)
            back = call('open_document', path=str(path), psd_mode='layers',
                        psd_fingerprint=report['source_fingerprint'])['document']['id']
            assert mode(back) == 'gray' and bits_of(back) == depth, (name, mode(back), bits_of(back))
            count = len(layers(back))
            assert count > 1, (name, layers(back))
            entry = {'layers': count}
            if name == 'styled8.psd':
                carried = [o for o in report['observations']
                           if '(“Overlay”) has Photoshop layer effects (' in o]
                assert carried and 'color_overlay' in carried[0], report['observations']
                entry['carried'] = carried[0]
            if not name.endswith('.tif'):
                second = gray_files / ('composite-' + name)
                second.write_bytes(data)
                again = call('inspect_psd', path=str(second))
                flat = call('open_document', path=str(second), psd_mode='composite',
                            psd_fingerprint=again['source_fingerprint'])['document']['id']
                assert mode(flat) == 'gray' and len(layers(flat)) == 1, (name, layers(flat))
                entry['composite'] = 1
            g32[name] = entry
        receipts['g32'] = g32
        print("PASS g32: Photoshop's Grayscale PSD, PSB and layered TIFF open as Grayscale "
              'documents with their layers', flush=True)

        # ---- G33–G35: Grayscale PSD, PSB and layered TIFF export (phase 3b) -------------------
        def psd_doc(depth):
            """The PSD document (module doc) at `depth`."""
            doc = call('new_document', width=PSD_W, height=PSD_H, color_mode='gray', depth=depth,
                       background='#808080', profile='dot_gain_20')['document']['id']
            call('new_layer', document_id=doc, name='Shape')
            call('select_rect', document_id=doc, x=2, y=2, width=8, height=6)
            call('fill_selection', document_id=doc, layer=1, color='#c0c0c0')
            call('select_rect', document_id=doc, x=3, y=3, width=6, height=4)
            call('add_layer_mask', document_id=doc, layer=1, kind='from_selection')
            call('deselect', document_id=doc)
            call('set_layer_properties', document_id=doc, index=1, blend_mode='Multiply',
                 opacity=0.5)
            call('group_layers', document_id=doc, layers=[1], name='Group')
            call('new_layer', document_id=doc, name='Styled')
            styled = next(l['index'] for l in layers(doc) if l['name'] == 'Styled')
            call('select_rect', document_id=doc, x=8, y=4, width=6, height=6)
            call('fill_selection', document_id=doc, layer=styled, color='#404040')
            call('deselect', document_id=doc)
            call('set_layer_style', document_id=doc, layer=styled, style={
                'effects': [{'type': 'color_overlay', 'color': '#c08040'}],
                'blend_if': {'channel': 'gray', 'this_layer': [0, 0, 255, 255],
                             'underlying': [0, 0, 96, 160]}})
            call('add_adjustment_layer', document_id=doc, op='levels', name='Levels',
                 position='top', params={'black': 10 / 255, 'white': 240 / 255, 'gamma': 1.2})
            return doc

        def shape_of(doc):
            return [(l['name'], l.get('kind'), l['index']) for l in layers(doc)]

        def psd_header(data):
            """(colour mode, depth, channels) of a PSD or PSB."""
            assert data[:4] == b'8BPS', data[:4]
            channels, _, _, depth, colour = struct.unpack('>HIIHH', data[12:26])
            return colour, depth, channels

        def tiff_bits(data):
            """TIFF tag 258's (type, count, four value bytes) and its first value, and tags
            262 and 277."""
            order = '<' if data[:2] == b'II' else '>'
            ifd = struct.unpack(order + 'I', data[4:8])[0]
            count = struct.unpack(order + 'H', data[ifd:ifd + 2])[0]
            out = {}
            for e in range(count):
                at = ifd + 2 + e * 12
                tag, kind, n = struct.unpack(order + 'HHI', data[at:at + 8])
                out[tag] = (kind, n, data[at + 8:at + 12])
            short = lambda tag: struct.unpack(order + 'H', out[tag][2][:2])[0]
            return out[258], short(258), short(262), short(277)

        def reopen(path, psd_mode):
            report = call('inspect_psd', path=str(path))
            doc = call('open_document', path=str(path), psd_mode=psd_mode,
                       psd_fingerprint=report['source_fingerprint'])['document']['id']
            return report, doc

        g33 = {}
        pdoc = psd_doc(16)
        report33 = call('inspect_psd_export', document_id=pdoc, format='psd', mode='layers')
        assert report33['available'] and report33['depth'] == 16, report33
        assert report33['color_mode'] == 'gray', report33
        (_, _, source16), _ = image('render', document_id=pdoc)
        for container in ('psd', 'psb', 'tiff'):
            ext = {'psd': 'psd', 'psb': 'psb', 'tiff': 'tif'}[container]
            path = folder / f'g33.{ext}'
            reply = call('export_psd', document_id=pdoc, path=str(path), format=container,
                         mode='layers')
            assert reply['bit_depth'] == 16, reply
            data = path.read_bytes()
            if container == 'tiff':
                bits, first, photometric, spp = tiff_bits(data)
                assert photometric == 1 and spp == 1, (photometric, spp)
                assert bits[0] == 3 and bits[1] == 1 and first == 16, (bits, first)
            else:
                assert psd_header(data) == (1, 16, 1), psd_header(data)
            report, back = reopen(path, 'layers')
            if container == 'psd':
                assert report['import_precision'] == 'gray', report
                carried = [o for o in report['observations'] if '(“Styled”) has Photoshop layer '
                           'effects (' in o]
                assert carried and 'color_overlay' in carried[0] \
                    and 'Blend If ranges on gray' in carried[0], report['observations']
            assert mode(back) == 'gray' and bits_of(back) == 16, (container, mode(back))
            assert shape_of(back) == shape_of(pdoc), (container, shape_of(back), shape_of(pdoc))
            (_, _, rendered), _ = image('render', document_id=back)
            worst = max(abs(a - b) for p, q in zip(rendered, source16) for a, b in zip(p, q))
            assert worst <= PSD_RENDER_BOUND8, (container, worst)
            g33[container] = {'worst': worst, 'layers': len(layers(back))}
        # A second path: open_document answers an already-open file as it is.
        composite_path = folder / 'g33c.psd'
        composite_path.write_bytes((folder / 'g33.psd').read_bytes())
        _, composite = reopen(composite_path, 'composite')
        assert len(layers(composite)) == 1 and mode(composite) == 'gray', layers(composite)
        flat = folder / 'g33flat.psd'
        call('export_psd', document_id=pdoc, path=str(flat), format='psd', mode='flattened')
        assert psd_header(flat.read_bytes()) == (1, 16, 1), psd_header(flat.read_bytes())
        _, flat_doc = reopen(flat, 'layers')
        assert len(layers(flat_doc)) == 1 and mode(flat_doc) == 'gray', layers(flat_doc)
        receipts['g33'] = g33
        print('PASS g33: a 16-bit Grayscale document exports PSD, PSB and layered TIFF that reopen '
              'Grayscale with its layers', flush=True)

        g34 = {}
        pdoc8 = psd_doc(8)
        (_, _, gray8), _ = image('render', document_id=pdoc8, channel='gray')
        for container in ('psd', 'tiff'):
            ext = {'psd': 'psd', 'tiff': 'tif'}[container]
            path = folder / f'g34.{ext}'
            reply = call('export_psd', document_id=pdoc8, path=str(path), format=container,
                         mode='layers')
            assert reply['bit_depth'] == 8, reply
            _, back = reopen(path, 'layers')
            assert mode(back) == 'gray' and bits_of(back) == 8, (container, mode(back))
            assert shape_of(back) == shape_of(pdoc8), (container, shape_of(back))
            (_, _, back8), _ = image('render', document_id=back, channel='gray')
            assert back8 == gray8, container
            g34[container] = 'equal'
        receipts['g34'] = g34
        print('PASS g34: an 8-bit Grayscale document round-trips PSD and layered TIFF exactly',
              flush=True)

        # ---- G35: 32 bits (phase 3c) ------------------------------------------------------
        def tiff_short(data, tag):
            """IFD0's first SHORT value of `tag`."""
            order = '<' if data[:2] == b'II' else '>'
            ifd = struct.unpack(order + 'I', data[4:8])[0]
            count = struct.unpack(order + 'H', data[ifd:ifd + 2])[0]
            for e in range(count):
                at = ifd + 2 + e * 12
                if struct.unpack(order + 'H', data[at:at + 2])[0] == tag:
                    return struct.unpack(order + 'H', data[at + 8:at + 10])[0]
            raise AssertionError(f'no TIFF tag {tag}')

        fdoc32 = call('new_document', width=PSD_W, height=PSD_H, color_mode='gray', depth=32,
                      background='#808080', profile='dot_gain_20')['document']['id']
        call('new_layer', document_id=fdoc32, name='Shape')
        call('select_rect', document_id=fdoc32, x=2, y=2, width=8, height=6)
        call('fill_selection', document_id=fdoc32, layer=1, color='#c0c0c0')
        call('deselect', document_id=fdoc32)
        inspect32 = call('inspect_psd_export', document_id=fdoc32, format='psd', mode='layers')
        assert inspect32['available'] and inspect32['depth'] == 32, inspect32
        assert inspect32['color_mode'] == 'gray', inspect32
        (_, _, gray32), _ = image('render', document_id=fdoc32, channel='gray')
        g35 = {}
        for container in ('psd', 'psb'):
            path = folder / f'g35.{container}'
            call('export_psd', document_id=fdoc32, path=str(path), format=container, mode='layers')
            assert psd_header(path.read_bytes()) == (1, 32, 1), psd_header(path.read_bytes())
            _, back = reopen(path, 'layers')
            assert mode(back) == 'gray' and bits_of(back) == 32, (container, mode(back))
            assert shape_of(back) == shape_of(fdoc32), (container, shape_of(back))
            (_, _, back32), _ = image('render', document_id=back, channel='gray')
            assert back32 == gray32, container
            g35[container] = 'equal'
        flat32 = folder / 'g35flat.psd'
        call('export_psd', document_id=fdoc32, path=str(flat32), format='psd', mode='flattened')
        assert psd_header(flat32.read_bytes()) == (1, 32, 1), psd_header(flat32.read_bytes())
        tiff_layers = call('export_psd', fail=True, document_id=fdoc32,
                           path=str(folder / 'g35.tif'), format='tiff', mode='layers')
        assert TIFF32_REFUSAL_PART in tiff_layers, tiff_layers
        assert not (folder / 'g35.tif').exists()
        flat_tiff = folder / 'g35flat.tif'
        call('export_image', document_id=fdoc32, path=str(flat_tiff),
             settings=dict(format='tiff', bit_depth=32, profile='document'))
        data = flat_tiff.read_bytes()
        fields = (tiff_short(data, 339), tiff_short(data, 258), tiff_short(data, 277))
        assert fields == (3, 32, 1), fields
        tiff_doc = call('open_document', path=str(flat_tiff),
                        precision='preserve')['document']['id']
        assert mode(tiff_doc) == 'gray' and bits_of(tiff_doc) == 32, (mode(tiff_doc),
                                                                       bits_of(tiff_doc))
        (_, _, tiff32), _ = image('render', document_id=tiff_doc, channel='gray')
        assert tiff32 == gray32, 'the float TIFF render'
        g35['tiff'] = {'sample_format': fields[0], 'bits': fields[1], 'samples': fields[2]}
        destination35 = folder / 'quick35'
        destination35.mkdir()
        webp35 = call('quick_export', fail=True, document_id=kdoc16, destination=str(destination35),
                      pattern='{name}-w', settings=dict(format='webp', bit_depth=8,
                                                        profile='document'))
        assert FORMAT_REFUSAL_PART in webp35, webp35
        g35['tiff_layers'] = tiff_layers
        g35['webp'] = webp35
        receipts['g35'] = g35
        print('PASS g35: a 32-bit Grayscale document exports 32-bit PSD, PSB and one-channel float '
              'TIFF that reopen Grayscale 32-bit; a layered TIFF and webp refuse', flush=True)

        # ---- G36: Photoshop's 32-bit Grayscale PSD open (phase 3c) --------------------------
        data36 = (GRAY_FIXTURES / LAYERS32).read_bytes()
        path36 = gray_files / LAYERS32
        path36.write_bytes(data36)
        report36 = call('inspect_psd', path=str(path36))
        assert report36['import_precision'] == 'gray' and report36['depth'] == 32, report36
        doc36 = call('open_document', path=str(path36), psd_mode='layers',
                     psd_fingerprint=report36['source_fingerprint'])['document']['id']
        assert mode(doc36) == 'gray' and bits_of(doc36) == 32, (mode(doc36), bits_of(doc36))
        names36 = sorted(l['name'] for l in layers(doc36))
        assert names36 == LAYERS32_NAMES, names36
        second36 = gray_files / ('composite-' + LAYERS32)
        second36.write_bytes(data36)
        again36 = call('inspect_psd', path=str(second36))
        flat36 = call('open_document', path=str(second36), psd_mode='composite',
                      psd_fingerprint=again36['source_fingerprint'])['document']['id']
        assert mode(flat36) == 'gray' and len(layers(flat36)) == 1, layers(flat36)
        receipts['g36'] = {'layers': names36, 'composite': 1}
        print("PASS g36: Photoshop's 32-bit Grayscale PSD opens as a 32-bit Grayscale document "
              'with its layers, and its composite as one layer', flush=True)

        # ---- G37–G41: the rest of phase 2 (phase 2e) ------------------------------------------
        def depth_step(doc, probe, tool, **arguments):
            """`tool` is one undo step over `probe` (module doc, phase 2e), R = G = B."""
            before = any_depth(doc, probe)
            reply = call(tool, document_id=doc, **arguments)
            after = any_depth(doc, probe)
            assert after != before, (tool, 'changed nothing')
            assert mode(doc) == 'gray', tool
            for px in after:
                assert px[0] == px[1] == px[2], (tool, px)
            call('undo', document_id=doc)
            assert any_depth(doc, probe) == before, (tool, 'undo')
            call('redo', document_id=doc)
            assert any_depth(doc, probe) == after, (tool, 'redo')
            assert mode(doc) == 'gray', tool
            return after, reply

        def textured(depth):
            """The textured document (module doc, phase 2e)."""
            doc = new_gray(depth, w=PATTERN_W, h=PATTERN_H)
            call('brush_stroke', document_id=doc, layer=0, points=[[2, 2], [21, 13]], size=3,
                 hardness=100, color='#000000')
            call('brush_stroke', document_id=doc, layer=0, points=[[2, 13], [21, 2]], size=3,
                 hardness=100, color='#ffffff')
            return doc

        # G37: Stroke… with a selection and on the layer edge.
        red_y = luminance((1, 0, 0))
        g37 = {}
        for depth in (16, 32):
            sdoc37 = effects_doc(depth)
            call('select_rect', document_id=sdoc37, x=1, y=1, width=10, height=8)
            inside37, _ = depth_step(sdoc37, [(1, 1)], 'stroke_selection', layer=0,
                                     color='#ff0000', width=1, location='inside')
            call('deselect', document_id=sdoc37)
            edge37, _ = depth_step(sdoc37, [(2, 5), (9, 5)], 'stroke_selection', layer=1,
                                   color='#ff0000', width=2, location='outside')
            for px in inside37 + edge37:
                if depth == 32:
                    assert abs(px[0] - red_y) <= STROKE_BOUND32, (depth, px, red_y)
                else:
                    assert abs(px[0] - red16) <= BOUND16, (depth, px, red16)
            hue37 = call('stroke_selection', fail=True, document_id=sdoc37, layer=1,
                         color='#ff0000', width=2, blend_mode='Hue')
            assert hue37.startswith(GRAY_LIMITATION_HEAD), hue37
            assert hue37.endswith(GRAY_LIMITATION_TAIL), hue37
            g37[depth] = {'inside': inside37, 'edge': edge37, 'hue': hue37}
        receipts['g37'] = g37
        print('PASS g37: Stroke… with a selection and on the layer edge paints red\'s gray at 16 '
              'and 32 bits, one step each; Hue refuses', flush=True)

        # G38: Content-Aware Fill (and its preview), Move and Remove.
        probe38 = [(x, y) for y in range(4, 12) for x in range(7, 18)]
        g38 = {}
        for depth in (8, 16, 32):
            row38 = {}
            fdoc38 = textured(depth)
            call('select_rect', document_id=fdoc38, x=10, y=6, width=5, height=4)
            before38 = any_depth(fdoc38, probe38)
            preview = call('preview_content_aware_fill', document_id=fdoc38, layer=0, seed=7)
            deadline = time.monotonic() + 120
            while preview.get('state') == 'running':
                assert time.monotonic() < deadline, preview
                time.sleep(0.05)
                preview = call('preview_content_aware_fill', document_id=fdoc38,
                               preview_id=preview['preview_id'])
            assert any_depth(fdoc38, probe38) == before38, (depth, 'a preview changed pixels')
            call('discard_content_aware_fill_preview', document_id=fdoc38,
                 preview_id=preview['preview_id'])
            assert any_depth(fdoc38, probe38) == before38, (depth, 'a discard changed pixels')
            row38['fill'] = depth_step(fdoc38, probe38, 'content_aware_fill', layer=0,
                                       seed=7)[0][:3]
            call('deselect', document_id=fdoc38)
            mdoc38 = textured(depth)
            row38['move'] = depth_step(mdoc38, probe38, 'content_aware_move', layer=0, seed=7,
                                       points=[[9, 5], [14, 5], [14, 10], [9, 10]], offset_x=4,
                                       offset_y=0)[0][:3]
            rdoc38 = textured(depth)
            row38['remove'] = depth_step(rdoc38, probe38, 'remove_stroke', layer=0, seed=7,
                                         mode='content_aware', points=[[6, 8], [12, 8]],
                                         size=4)[0][:3]
            g38[depth] = row38
        receipts['g38'] = g38
        print('PASS g38: Content-Aware Fill (and its preview), Move and Remove run on gray at 8, '
              '16 and 32 bits, one step each', flush=True)

        # G39: the Art History Brush at 8 bits.
        probe39 = [(x, y) for y in range(0, PATTERN_H, 2) for x in range(0, PATTERN_W, 2)]
        adoc39 = textured(8)
        call('fill_selection', document_id=adoc39, layer=0, color='#ffffff')
        art39, _ = depth_step(adoc39, probe39, 'art_history_brush_stroke', layer=0,
                              points=[[12, 8]], size=5, style='tight_long', area=30, seed=4)
        hue39 = call('art_history_brush_stroke', fail=True, document_id=adoc39, layer=0,
                     points=[[12, 8]], size=5, blend_mode='Hue')
        assert HUE_EFFECT in hue39, hue39
        sixteen39 = call('art_history_brush_stroke', fail=True, document_id=new_gray(16),
                         layer=0, points=[[6, 4]], size=5)
        assert ART16_REFUSAL in sixteen39, sixteen39
        receipts['g39'] = {'painted': art39[:4], 'hue': hue39, 'sixteen': sixteen39}
        print('PASS g39: the Art History Brush paints an 8-bit Grayscale document, one step; Hue '
              'and 16 bits refuse', flush=True)

        # G40: RGB → Gray carries the adjustment layers it can hold.
        rgb40 = folder / 'patches16-adjustments.png'
        png(rgb40, len(PATCHES), 1, 16, 2, [v * 257 for p in PATCHES for v in p])
        doc40 = call('open_document', path=str(rgb40), precision='preserve')['document']['id']
        call('add_adjustment_layer', document_id=doc40, op='levels', params={'gamma': 1.4},
             name='Levels')
        stops40 = [{'position': 0, 'color': '#ff0000'}, {'position': 1, 'color': '#00ffff'}]
        call('add_adjustment_layer', document_id=doc40, op='gradient_map',
             params={'gradient': {'stops': stops40}}, name='Map')
        reply40 = call('convert_mode', document_id=doc40, mode='gray', gray_space='gray_gamma_22')
        assert reply40['color_mode'] == 'gray', reply40
        assert reply40['document']['layer_count'] == 3, reply40
        rows40 = layers(doc40)
        ops40 = sorted(row['adjustment']['op'] for row in rows40 if row.get('adjustment'))
        assert ops40 == ['gradient_map', 'levels'], rows40
        map40 = next(row for row in rows40 if (row.get('adjustment') or {}).get('op')
                     == 'gradient_map')
        greys40 = []
        for stop, was in zip(map40['adjustment']['params']['gradient']['stops'], stops40):
            hex40 = stop['color'].lstrip('#').lower()
            assert hex40[0:2] == hex40[2:4] == hex40[4:6], stop
            rgb = [int(was['color'][1 + 2 * c:3 + 2 * c], 16) / 255 for c in range(3)]
            want = round(gray_of('gray_gamma_22', luminance(rgb)) * 255)
            assert abs(int(hex40[0:2], 16) - want) <= STOP_BOUND8, (stop, want)
            greys40.append((stop['color'], want))
        neutral(doc40, len(PATCHES), 1)
        call('undo', document_id=doc40)
        assert mode(doc40) == 'rgb'
        call('redo', document_id=doc40)
        assert mode(doc40) == 'gray'
        hue40 = folder / 'patches16-hue.png'
        png(hue40, len(PATCHES), 1, 16, 2, [v * 257 for p in PATCHES for v in p])
        hdoc40 = call('open_document', path=str(hue40), precision='preserve')['document']['id']
        call('add_adjustment_layer', document_id=hdoc40, op='hue_saturation', params={'hue': 20},
             name='Hue')
        blocker40 = call('convert_mode', fail=True, document_id=hdoc40, mode='gray',
                         gray_space='gray_gamma_22')
        assert HUE_SAT_BLOCKER in blocker40, blocker40
        flat40 = call('convert_mode', document_id=hdoc40, mode='gray', gray_space='gray_gamma_22',
                      flatten=True)
        assert flat40['document']['layer_count'] == 1 and mode(hdoc40) == 'gray', flat40
        receipts['g40'] = {'ops': ops40, 'stops': greys40, 'blocker': blocker40}
        print('PASS g40: RGB → Grayscale carries Levels and a greyed Gradient Map; a Hue/Saturation '
              'layer is named and flatten converts', flush=True)

        # G41: HDR Toning at 8 and 16 bits; 32 refuses; get_precision.
        g41 = {}
        for depth in (8, 16):
            hdoc41 = textured(depth)
            toned41, reply41 = depth_step(hdoc41, probe39, 'hdr_toning', method='exposure_gamma',
                                          exposure=0.5, gamma=1.2)
            assert reply41['bit_depth'] == depth, reply41
            assert bits_of(hdoc41) == depth, bits_of(hdoc41)
            g41[str(depth)] = toned41[:4]
        fdoc41 = textured(32)
        precision41 = call('get_precision', document_id=fdoc41)
        for tool in PHASE_2E_TOOLS:
            assert tool in precision41['supported_tools'], tool
        assert 'default_phase' not in precision41, precision41['default_phase']
        # Phase 5a: a 32-bit Grayscale document tones too (G52 holds its gates).
        float41 = call('hdr_toning', document_id=fdoc41, method='exposure_gamma', exposure=0.5,
                       gamma=1.2)
        assert mode(fdoc41) == 'gray' and bits_of(fdoc41) == 16, float41
        g41['float'] = 'admitted (phase 5a)'
        receipts['g41'] = g41
        print('PASS g41: HDR Toning keeps an 8- or 16-bit Grayscale document at its depth, one '
              'step; a 32-bit one tones (phase 5a); get_precision lists phase 2e', flush=True)

        # ---- G42–G47: geometry, selections and the clipboard (phase 4) -------------------------
        def undo_count(doc):
            return int(call('list_history', document_id=doc)['undo_count'])

        def canvas(doc):
            info = call('get_document', document_id=doc)
            return info['width'], info['height']

        probe42 = [(x, y) for y in range(0, 8, 3) for x in range(0, 14, 3)]

        def geo_step(doc, tool, **arguments):
            """`tool` is one undo step (module doc, phase 4)."""
            before, size, samples_before = undo_count(doc), canvas(doc), any_depth(doc, probe42)
            reply = call(tool, document_id=doc, **arguments)
            assert undo_count(doc) == before + 1, (tool, reply)
            assert mode(doc) == 'gray', tool
            for px in any_depth(doc, probe42):
                assert px[0] == px[1] == px[2], (tool, px)
            call('undo', document_id=doc)
            assert canvas(doc) == size and any_depth(doc, probe42) == samples_before, (tool, 'undo')
            return reply

        # G42: geometry, transforms, warps and Liquify.
        g42 = {}
        gdoc42 = textured(16)
        displacements = [[0, 0] for _ in range(9)]
        displacements[4] = [2, 1]
        for tool, arguments in [
            ('image_size', dict(width=30, height=20)),
            ('canvas_size', dict(width=30, height=20, anchor='center')),
            ('crop', dict(x=2, y=2, width=16, height=10)),
            ('rotate', dict(degrees=90)),
            ('flip', dict(axis='horizontal')),
            ('transform_layer', dict(layer=0, rotate=30, sampler='bilinear')),
            ('puppet_warp', dict(layer=0, pins=[{'from': [6, 6], 'to': [8, 7]},
                                                {'from': [18, 9], 'to': [18, 9]}])),
            ('perspective_warp', dict(layer=0, quads=[{'from': [2, 2, 21, 2, 21, 13, 2, 13],
                                                       'to': [3, 3, 22, 1, 20, 14, 1, 12]}])),
            ('liquify', dict(layer=0, strokes=[{'tool': 'forward_warp', 'size': 8,
                                                'points': [[8, 6], [14, 7]]}])),
        ]:
            reply = geo_step(gdoc42, tool, **arguments)
            g42[tool] = {k: reply.get(k) for k in ('ok', 'changed', 'width', 'height')}
        before = undo_count(gdoc42)
        warped = call('warp_layer', document_id=gdoc42, layer=0, operation='set', target='pixels',
                      columns=3, rows=3, displacements=displacements, filter='bicubic')
        assert undo_count(gdoc42) == before + 1 and mode(gdoc42) == 'gray', warped
        field = call('get_warp', document_id=gdoc42, layer=0)
        assert field.get('field') is None, field
        for px in any_depth(gdoc42, probe42):
            assert px[0] == px[1] == px[2], ('warp_layer', px)
        call('undo', document_id=gdoc42)
        g42['warp_layer'] = {'field': None}
        for depth in (8, 32):
            ddoc42 = textured(depth)
            geo_step(ddoc42, 'image_size', width=30, height=20)
        receipts['g42'] = g42
        print('PASS g42: Image and Canvas Size, Crop, rotate, flip, Free Transform, the warps and '
              'Liquify are one step each on Grayscale, R = G = B', flush=True)

        # G43: selections, Quick Mask, Save/Load Selection and the Gray row.
        g43 = {}
        sdoc43 = textured(16)
        wand = call('select_magic_wand', document_id=sdoc43, x=2, y=2, tolerance=10,
                    contiguous=True)
        assert not call('get_selection', document_id=sdoc43)['selection_empty'], wand
        view = call('sample_color', document_id=sdoc43, x=12, y=7)['center_rgba16'][:3]
        call('select_color_range', document_id=sdoc43, range='sampled', samples16=[view])
        assert not call('get_selection', document_id=sdoc43)['selection_empty']
        call('select_rect', document_id=sdoc43, x=4, y=4, width=6, height=4)
        call('select_similar', document_id=sdoc43, tolerance=8)
        assert not call('get_selection', document_id=sdoc43)['selection_empty']
        subject = app.client.rpc('tools/call', {'name': 'select_subject',
                                                'arguments': {'document_id': sdoc43}})
        subject_text = ' '.join(c.get('text', '') for c in subject.get('content', []))
        assert not subject.get('isError') or 'Grayscale' not in subject_text, subject_text
        g43['select_subject'] = 'error' if subject.get('isError') else 'ok'
        call('select_rect', document_id=sdoc43, x=3, y=3, width=8, height=5)
        call('set_quick_mask', document_id=sdoc43, active=True)
        quick = call('fill_selection', document_id=sdoc43, color='#808080', target='quick_mask')
        call('set_quick_mask', document_id=sdoc43, active=False)
        g43['quick_mask_fill'] = {k: quick.get(k) for k in ('ok', 'target')}
        points43 = [{'x': x, 'y': y} for y in range(PATTERN_H) for x in range(PATTERN_W)]
        held = [s['coverage16'] for s in
                call('get_selection', document_id=sdoc43, points=points43)['samples']]
        call('save_selection', document_id=sdoc43, name='Held')
        call('deselect', document_id=sdoc43)
        call('load_selection', document_id=sdoc43, channel='Held', **{'from': 'channel'})
        loaded = [s['coverage16'] for s in
                  call('get_selection', document_id=sdoc43, points=points43)['samples']]
        assert loaded == held, 'save/load selection round trip'
        red = call('load_selection', fail=True, document_id=sdoc43,
                   **{'from': 'plane', 'plane': 'red'})
        assert red == LOAD_PLANE_RED, red
        codes43 = [0, 17, 64, 128, 200, 255]
        row43 = folder / 'g43-codes.png'
        png(row43, 6, 1, 8, 0, codes43, [iccp(gray_icc('Gray Gamma 2.2 (drive)', 2.2))])
        kdoc43 = call('open_document', path=str(row43))['document']['id']
        assert mode(kdoc43) == 'gray'
        call('load_selection', document_id=kdoc43, **{'from': 'plane', 'plane': 'gray'})
        own = [s['coverage16'] for s in call('get_selection', document_id=kdoc43,
                                             points=[{'x': x, 'y': 0} for x in range(6)])['samples']]
        assert own == [center(kdoc43, x, 0)[0] for x in range(6)], own
        assert own == [c * 257 for c in codes43], own
        g43['gray_row'] = own
        receipts['g43'] = g43
        print('PASS g43: the wand, Color Range, Similar, Subject, Quick Mask, Save/Load Selection '
              'and the Gray row (its own codes) on Grayscale', flush=True)

        # G44: copy and paste within Grayscale.
        src44 = textured(16)
        dst44 = new_gray(16, w=PATTERN_W, h=PATTERN_H)
        call('select_rect', document_id=src44, x=3, y=2, width=10, height=6)
        call('copy', document_id=src44, layer=0)
        call('deselect', document_id=src44)
        clip44 = call('get_clipboard')
        assert clip44['mode'] == 'gray', clip44
        before = undo_count(dst44)
        pasted44 = call('paste', document_id=dst44, placement='in_place')
        assert undo_count(dst44) == before + 1 and mode(dst44) == 'gray', pasted44
        assert pasted44.get('color_conversion') is None, pasted44
        top44 = len(call('get_document', document_id=dst44)['layers']) - 1
        for x, y in [(3, 2), (7, 4), (12, 7)]:
            got = call('sample_pixel', document_id=dst44, x=x, y=y, source='layer',
                       layer=top44)['mean_rgba16']
            want = call('sample_pixel', document_id=src44, x=x, y=y, source='layer',
                        layer=0)['mean_rgba16']
            assert got == want, (x, y, got, want)
        receipts['g44'] = {'clipboard_mode': clip44['mode']}
        print('PASS g44: Gray copy and paste keep the codes exactly, one step', flush=True)

        # G45: across modes.
        g45 = {}
        rgb45 = call('new_document', width=8, height=6, depth=16, profile='srgb',
                     background='#c8285a')['document']['id']
        call('select_all', document_id=rgb45)
        call('copy', document_id=rgb45, layer=0)
        gray45 = new_gray(16, w=8, h=6)
        refused45 = call('paste', fail=True, document_id=gray45, placement='in_place',
                         profile='preserve')
        assert refused45 == CROSS_GRAY, refused45
        into_gray = call('paste', document_id=gray45, placement='in_place')
        conversion = into_gray['color_conversion']
        assert conversion and conversion['from'] == 'RGB' and conversion['to'] == 'Gray', into_gray
        assert conversion['black_point_compensation'] is False, conversion
        want = round(gray_of('gray_gamma_22', luminance((0xc8 / 255, 0x28 / 255, 0x5a / 255)))
                     * 65535)
        px = center(gray45, 3, 3)
        assert px[0] == px[1] == px[2] and abs(px[0] - want) <= BOUND16, (px, want)
        g45['rgb_to_gray'] = {'pixel': px, 'oracle': want}
        call('select_all', document_id=gray45)
        call('copy', document_id=gray45, layer=1)
        rgb_target = call('new_document', width=8, height=6, depth=16, profile='srgb',
                          background='#ffffff')['document']['id']
        out_of_gray = call('paste', document_id=rgb_target, placement='in_place')
        conversion = out_of_gray['color_conversion']
        assert conversion and conversion['from'] == 'Gray' and conversion['to'] == 'RGB', out_of_gray
        px = center(rgb_target, 3, 3)
        assert max(px[:3]) - min(px[:3]) <= GRAY_RGB_BOUND16, px
        g45['gray_to_rgb'] = px
        cmyk45 = call('new_document', width=8, height=6, color_mode='cmyk', depth=16,
                      background='#4080c0')['document']['id']
        call('select_all', document_id=cmyk45)
        call('copy', document_id=cmyk45, layer=0)
        from_cmyk = call('paste', document_id=gray45, placement='in_place')
        conversion = from_cmyk['color_conversion']
        assert conversion and conversion['from'] == 'CMYK' and conversion['to'] == 'Gray', from_cmyk
        px = center(gray45, 3, 3)
        assert px[0] == px[1] == px[2] and mode(gray45) == 'gray', px
        g45['cmyk_to_gray'] = px
        receipts['g45'] = g45
        print('PASS g45: RGB → Gray lands on the luminance, Gray → RGB is neutral, CMYK → Gray '
              'converts; Don\'t Convert refuses across modes', flush=True)

        # G46: the pasteboard's one-channel TIFF.
        opaque46 = call('new_document', width=7, height=5, color_mode='gray', depth=8,
                        background='#606060', profile='gray_gamma_22')['document']['id']
        call('select_all', document_id=opaque46)
        call('copy', document_id=opaque46, layer=0)
        reader = folder / 'pasteboard-reader.swift'
        reader.write_text(PASTEBOARD_READER)
        board = folder / 'g46-board.tif'
        subprocess.run(['xcrun', 'swift', str(reader), os.environ['DR_PASTEBOARD_NAME'],
                        str(board)], check=True, capture_output=True, timeout=300)
        spp, photometric, bits, board_icc = tiff_header(board.read_bytes())
        assert (spp, photometric, bits) == (1, 1, 8), (spp, photometric, bits)
        assert len(board_icc) > 128 and board_icc[16:20] == b'GRAY', board_icc[:24]
        receipts['g46'] = {'samples': spp, 'bits': bits, 'icc_bytes': len(board_icc)}
        print('PASS g46: the pasteboard TIFF of a Gray copy is one gray channel with its Gray '
              'profile', flush=True)

        # G47: no later phases (the close's rewrite).
        precision47 = call('get_precision', document_id=opaque46)
        later = [k for k in ('later_phases', 'default_phase') if k in precision47]
        assert later == [], later
        for tool in PHASE_4_TOOLS:
            assert tool in precision47['supported_tools'], tool
        receipts['g47'] = later
        print('PASS g47: get_precision lists phase 4 and names no later phase', flush=True)

        # ---- G48–G53: colour (phase 5a) -------------------------------------------------------
        def profile_of(doc):
            return call('get_color_profile', document_id=doc)['name']

        def colour_step(doc, probe, tool, **arguments):
            """`tool` is one undo step (module doc, phase 5a): its reply and the samples after."""
            count, name, bits = undo_count(doc), profile_of(doc), bits_of(doc)
            before = any_depth(doc, probe)
            reply = call(tool, document_id=doc, **arguments)
            assert undo_count(doc) == count + 1, (tool, reply)
            assert mode(doc) == 'gray', tool
            after = any_depth(doc, probe)
            for px in after:
                assert px[0] == px[1] == px[2], (tool, px)
            changed = (profile_of(doc), bits_of(doc))
            call('undo', document_id=doc)
            assert (profile_of(doc), bits_of(doc)) == (name, bits), (tool, 'undo')
            assert any_depth(doc, probe) == before, (tool, 'undo')
            call('redo', document_id=doc)
            assert any_depth(doc, probe) == after and (profile_of(doc), bits_of(doc)) == changed, \
                (tool, 'redo')
            return reply, before, after

        # G48: Assign keeps every code.
        adoc48 = textured(16)
        reply48, before48, after48 = colour_step(adoc48, probe39, 'assign_profile',
                                                 profile='dot_gain_20')
        assert after48 == before48, 'assign changed samples'
        assert profile_of(adoc48) == 'Dot Gain 20%', profile_of(adoc48)
        assert reply48['changed'] is True and reply48['color_mode'] == 'gray', reply48
        receipts['g48'] = {'profile': profile_of(adoc48), 'previous': reply48['previous_profile']}
        print('PASS g48: Assign Profile keeps every gray code and names Dot Gain 20%, one step',
              flush=True)

        # G49: Convert onto the curve oracle at 16 and 8 bits, and from an embedded ICC.
        source49, target49 = gray_curve('gray_gamma_22'), gray_curve('dot_gain_20')
        worst49 = {}
        for depth in (16, 8):
            cdoc49 = textured(depth)
            _, before49, after49 = colour_step(cdoc49, probe39, 'convert_profile',
                                               profile='dot_gain_20')
            assert profile_of(cdoc49) == 'Dot Gain 20%', profile_of(cdoc49)
            worst = 0.0
            for was, got in zip(before49, after49):
                want = curve_encode(target49, curve_decode(source49, was[0] / 65535))
                if depth == 16:
                    worst = max(worst, abs(got[0] - want * 65535))
                    assert abs(got[0] - want * 65535) <= CONVERT_BOUND16, (was, got, want * 65535)
                else:
                    assert got[0] % 257 == 0, got
                    worst = max(worst, abs(got[0] / 257 - want * 255))
                    assert abs(got[0] / 257 - round(want * 255)) <= BOUND8, (was, got, want * 255)
            worst49[str(depth)] = worst
        idoc49 = opened['icc16.png']
        probe49 = [(x, y) for y in range(2) for x in range(3)]
        _, before49, after49 = colour_step(idoc49, probe49, 'convert_profile', profile='sgray')
        assert profile_of(idoc49) == 'sGray', profile_of(idoc49)
        sgray49 = gray_curve('sgray')
        for was, got in zip(before49, after49):
            want = curve_encode(sgray49, curve_decode(None, was[0] / 65535)) * 65535
            assert abs(got[0] - want) <= CONVERT_BOUND16, (was, got, want)
        receipts['g49'] = {'worst': worst49, 'icc_to_sgray': after49}
        print('PASS g49: Convert to Profile lands on the curves at 16 and 8 bits and from an '
              'embedded Gray ICC, one step each', flush=True)

        # G50: an RGB or CMYK profile refuses by name.
        ndoc50 = textured(16)
        srgb50 = call('assign_profile', fail=True, document_id=ndoc50, profile='srgb')
        assert RGB_PROFILE_PART in srgb50 and srgb50.startswith('“'), srgb50
        g50 = {'srgb': srgb50}
        if GENERIC_CMYK.is_file():
            cmyk50 = call('convert_profile', fail=True, document_id=ndoc50, profile='file',
                          path=str(GENERIC_CMYK))
            assert CMYK_PROFILE_PART in cmyk50, cmyk50
            g50['cmyk'] = cmyk50
        else:
            g50['cmyk'] = f'skipped: {GENERIC_CMYK} is absent'
            print(f'g50: skipped the CMYK case, {GENERIC_CMYK} is absent', flush=True)
        receipts['g50'] = g50
        print('PASS g50: an RGB or CMYK profile refuses by name', flush=True)

        # G51: 8/16/32 Bits/Channel.
        bdoc51 = textured(16)
        _, sixteen51, eight51 = colour_step(bdoc51, probe39, 'convert_precision', mode='gray8')
        assert bits_of(bdoc51) == 8
        for was, got in zip(sixteen51, eight51):
            assert got == [(v + 128) // 257 * 257 for v in was], (was, got)
        _, _, back51 = colour_step(bdoc51, probe39, 'convert_precision', mode='gray16')
        assert bits_of(bdoc51) == 16 and back51 == eight51, back51
        colour_step(bdoc51, probe39, 'convert_precision', mode='gray32')
        assert bits_of(bdoc51) == 32
        refused51 = call('convert_precision', fail=True, document_id=bdoc51, mode='gray16')
        assert FLOAT_DEPTH_REFUSAL in refused51, refused51
        receipts['g51'] = {'refusal': refused51}
        print('PASS g51: 8/16/32 Bits/Channel, one step each; 32 → 16 is HDR Toning\'s',
              flush=True)

        # G52: HDR Toning of a 32-bit Grayscale document into 16 and 8 bits.
        g52 = {}
        for depth in (16, 8):
            hdoc52 = textured(32)
            reply52, _, toned52 = colour_step(hdoc52, probe39, 'hdr_toning', method='exposure_gamma',
                                              exposure=0.5, gamma=1.2, bit_depth=depth)
            assert bits_of(hdoc52) == depth, (depth, bits_of(hdoc52))
            if depth == 8:
                for x, y in probe39:
                    stored = call('sample_pixel', document_id=hdoc52, x=x, y=y, source='layer',
                                  layer=0)['center_rgba_codes']
                    assert stored[0] == stored[1] == stored[2] and stored[0] % 257 == 0, stored
            g52[str(depth)] = toned52[:4]
        receipts['g52'] = g52
        print('PASS g52: HDR Toning turns a 32-bit Grayscale document into 16 or 8 bits, one step',
              flush=True)

        # G53: the gate and the catalog's enums.
        precision53 = call('get_precision', document_id=bdoc51)
        later53 = 'later_phases' in precision53
        assert not later53, precision53['later_phases']
        for tool in PHASE_5A_TOOLS:
            assert tool in precision53['supported_tools'], tool
        listed = {t['name']: t for t in app.client.rpc('tools/list', {})['tools']}
        for tool in ('assign_profile', 'convert_profile'):
            options = listed[tool]['inputSchema']['properties']['profile']['enum']
            for value in ['working_gray', 'installed', 'srgb', 'display_p3', 'file'] + GRAY_SPACES:
                assert value in options, (tool, value, options)
        modes53 = listed['convert_precision']['inputSchema']['properties']['mode']['enum']
        for value in ('gray8', 'gray16', 'gray32'):
            assert value in modes53, (value, modes53)
        receipts['g53'] = {'later_phases': later53, 'convert_precision_modes': modes53}
        print('PASS g53: no later phase; the catalog lists the Gray profiles and depths',
              flush=True)

        # ---- G54–G56: proof and Gray → CMYK (phase 5b) ----------------------------------------
        def sgray(depth):
            """The sGray document (module doc, phase 5b)."""
            doc = textured(depth)
            call('assign_profile', document_id=doc, profile='sgray')
            assert profile_of(doc) == 'sGray', profile_of(doc)
            return doc

        def proof_image(doc, **arguments):
            pixels, text = image('render_proof', document_id=doc, profile='working_cmyk',
                                 **arguments)
            return pixels, json.loads(text)

        def proof_close(gray, rgb, what):
            (gw, gh, gp), ginfo = gray
            (rw, rh, rp), rinfo = rgb
            assert (gw, gh) == (rw, rh), (what, (gw, gh), (rw, rh))
            worst = 0
            for a, b in zip(gp, rp):
                for c in range(min(len(a), len(b))):
                    worst = max(worst, abs(a[c] - b[c]))
            assert worst <= PROOF_BOUND8, (what, worst)
            assert ginfo['out_of_gamut'] == rinfo['out_of_gamut'], (what, ginfo, rinfo)
            return worst

        # G54: Proof Setup and Proof Colors through the RGB proof of the twin.
        pdoc54 = sgray(16)
        setup54 = call('set_proof_setup', document_id=pdoc54, profile='working_cmyk',
                       proof_colors=True)
        assert setup54.get('proof_colors') is True, setup54
        state54 = None
        deadline54 = time.monotonic() + 5
        while time.monotonic() < deadline54:
            state54 = call('get_proof_setup', document_id=pdoc54)
            if state54.get('proof_colors') is True and state54.get('failure') is None:
                break
            time.sleep(0.1)
        assert state54['proof_colors'] is True and state54['failure'] is None, state54
        preserve54 = call('set_proof_setup', fail=True, document_id=pdoc54, preserve_numbers=True)
        assert preserve54 == PRESERVE_REFUSAL, preserve54
        display54 = proof_image(pdoc54)
        exact54 = proof_image(pdoc54, exact=True)
        assert display54[1]['source'] == 'display', display54[1]
        call('convert_mode', document_id=pdoc54, mode='rgb', rgb_space='srgb')
        assert mode(pdoc54) == 'rgb'
        worst54 = {
            'display': proof_close(display54, proof_image(pdoc54), 'display'),
            'exact': proof_close(exact54, proof_image(pdoc54, exact=True), 'exact'),
        }
        call('undo', document_id=pdoc54)
        assert mode(pdoc54) == 'gray'
        eight54 = proof_image(sgray(8))[1]
        assert eight54['source'] == 'display', eight54
        float54 = new_gray(32)
        for tool in ('get_proof_setup', 'render_proof'):
            refused = call(tool, fail=True, document_id=float54)
            assert refused.startswith(f'{tool} {FLOAT_TOOL_PART}'), (tool, refused)
        supported54 = call('get_precision', document_id=pdoc54)['supported_tools']
        for tool in PROOF_TOOLS:
            assert tool in supported54, tool
        receipts['g54'] = {'worst': worst54, 'out_of_gamut': display54[1]['out_of_gamut'],
                           'eight': eight54}
        print('PASS g54: a Grayscale document proofs as the RGB proof of its twin; 32 bits refuses',
              flush=True)

        # G55: Gray → CMYK equals Gray → RGB → CMYK; 8 bits; an Action replays it.
        def inks(doc, probe):
            return [call('sample_color', document_id=doc, x=x, y=y)['ink16'] for x, y in probe]

        via55 = sgray(16)
        call('convert_mode', document_id=via55, mode='rgb', rgb_space='srgb')
        call('convert_mode', document_id=via55, mode='cmyk')
        assert mode(via55) == 'cmyk'
        reference55 = inks(via55, probe39)
        cdoc55 = sgray(16)
        count55, before55 = undo_count(cdoc55), any_depth(cdoc55, probe39)
        reply55 = call('convert_mode', document_id=cdoc55, mode='cmyk')
        assert reply55['color_mode'] == 'cmyk' and reply55['depth'] == 16, reply55
        assert undo_count(cdoc55) == count55 + 1, reply55
        direct55 = inks(cdoc55, probe39)
        worst55 = 0
        for got, want in zip(direct55, reference55):
            for c in range(4):
                worst55 = max(worst55, abs(got[c] - want[c]))
        assert worst55 <= CMYK_TWIN_BOUND16, worst55
        call('undo', document_id=cdoc55)
        assert mode(cdoc55) == 'gray' and any_depth(cdoc55, probe39) == before55, 'undo'
        call('redo', document_id=cdoc55)
        assert mode(cdoc55) == 'cmyk' and inks(cdoc55, probe39) == direct55, 'redo'
        eight55 = sgray(8)
        reply8 = call('convert_mode', document_id=eight55, mode='cmyk')
        assert reply8['color_mode'] == 'cmyk' and reply8['depth'] == 8, reply8
        for ink in inks(eight55, probe39):
            assert all(v % 257 == 0 for v in ink[:4]), ink
        call('save_action', name='gray-cmyk-547', overwrite=True,
             steps=[{'tool': 'convert_mode', 'arguments': {'mode': 'cmyk'}}])
        played55 = sgray(16)
        call('run_action', document_id=played55, name='gray-cmyk-547')
        assert mode(played55) == 'cmyk'
        assert inks(played55, probe39) == direct55, 'the Action replays the conversion'
        call('delete_action', name='gray-cmyk-547')
        receipts['g55'] = {'worst16': worst55, 'reply16': {k: reply55[k] for k in
                                                           ('color_mode', 'depth', 'profile')}}
        print('PASS g55: Grayscale → CMYK Color equals Gray → RGB → CMYK, keeps 8 bits, one step, '
              'and an Action replays it', flush=True)

        # G56: refusals and the Flatten answer.
        float56 = call('convert_mode', fail=True, document_id=new_gray(32), mode='cmyk')
        assert float56 == CMYK_FLOAT_REFUSAL, float56
        adoc56 = sgray(16)
        call('add_adjustment_layer', document_id=adoc56, op='levels', params={'gamma': 1.4},
             name='Levels')
        blocked56 = call('convert_mode', fail=True, document_id=adoc56, mode='cmyk')
        assert ADJUSTMENT_BLOCKER in blocked56, blocked56
        assert mode(adoc56) == 'gray'
        count56 = undo_count(adoc56)
        flat56 = call('convert_mode', document_id=adoc56, mode='cmyk', flatten=True)
        assert flat56['color_mode'] == 'cmyk' and flat56['document']['layer_count'] == 1, flat56
        assert undo_count(adoc56) == count56 + 1, flat56
        call('undo', document_id=adoc56)
        assert mode(adoc56) == 'gray' and len(layers(adoc56)) == 2, layers(adoc56)
        receipts['g56'] = {'float': float56, 'blocked': blocked56}
        print('PASS g56: 32 bits and an adjustment layer refuse by name; flatten converts in one '
              'step', flush=True)

        # ---- G57–G62: type, shape and fill layers, paths and vector masks (phase 6a-i) ---------
        def row_of(doc, index):
            return next(row for row in layers(doc) if row['index'] == index)

        def hex_gray(text, rgb, what):
            """A reported hex is gray and within HEX_BOUND of `rgb`'s sRGB gray byte."""
            digits = text.lstrip('#').lower()
            assert digits[0:2] == digits[2:4] == digits[4:6], (what, text)
            want = round(255 * srgb_encode(luminance(rgb)))
            assert abs(int(digits[0:2], 16) - want) <= HEX_BOUND, (what, text, want)
            return text

        def unit(hex_text):
            digits = hex_text.lstrip('#')
            return tuple(int(digits[i:i + 2], 16) / 255 for i in (0, 2, 4))

        def on_oracle(doc, x, y, rgb, depth, what, space='gray_gamma_22'):
            """The composite at (x, y) is R = G = B on `rgb`'s gray (module doc, phase 6a-i)."""
            px = center(doc, x, y)
            assert px[0] == px[1] == px[2], (what, px)
            y_rgb = luminance(rgb)
            if depth == 8:
                assert px[0] % 257 == 0, (what, px)
                want = round(255 * gray_of(space, y_rgb))
                assert abs(px[0] // 257 - want) <= BOUND8, (what, px, want)
            else:
                want = round(65535 * gray_of(space, y_rgb))
                assert abs(px[0] - want) <= BOUND16, (what, px, want)
            return px

        def rendered_neutral(doc, what, **arguments):
            (w, h, pixels), _ = image('render', document_id=doc, **arguments)
            assert w > 0 and h > 0 and len(pixels) == w * h, what
            for px in pixels:
                assert px[0] == px[1] == px[2], (what, px)

        def art_step(doc, probe, tool, **arguments):
            """`tool` is one undo step (module doc, phase 6a-i): its reply."""
            count, before = undo_count(doc), samples(doc, probe)
            reply = call(tool, document_id=doc, **arguments)
            assert undo_count(doc) == count + 1, (tool, reply)
            assert mode(doc) == 'gray', tool
            after = samples(doc, probe)
            call('undo', document_id=doc)
            assert samples(doc, probe) == before, (tool, 'undo')
            call('redo', document_id=doc)
            assert samples(doc, probe) == after, (tool, 'redo')
            assert mode(doc) == 'gray', tool
            return reply

        red, green, blue = (1, 0, 0), (0, 1, 0), (0, 0, 1)
        probe57 = [(x, y) for y in range(0, 16, 3) for x in range(0, 40, 3)]

        # G57: type.
        tdoc57 = new_gray(16, w=40, h=16)
        text57 = art_step(tdoc57, probe57, 'add_text_layer', text='Gray', x=2, y=1, size=12,
                          font='Helvetica Neue', color='#ff0000')
        tidx57 = text57['layer']
        red57 = hex_gray(row_of(tdoc57, tidx57)['text']['color'], red, 'text red')
        rendered_neutral(tdoc57, 'text layer', layer=tidx57)
        art_step(tdoc57, probe57, 'edit_text_layer', layer=tidx57, color='#00ff00')
        green57 = hex_gray(row_of(tdoc57, tidx57)['text']['color'], green, 'text green')
        rendered_neutral(tdoc57, 'edited text layer', layer=tidx57)
        fdoc57 = new_gray(32, w=40, h=16)
        text57f = call('add_text_layer', document_id=fdoc57, text='Gray', x=2, y=1, size=12,
                       font='Helvetica Neue', rgba=[1, 0, 0, 1])
        rgba57 = row_of(fdoc57, text57f['layer'])['text']['rgba']
        assert rgba57[0] == rgba57[1] == rgba57[2] and rgba57[3] == 1, rgba57
        assert abs(rgba57[0] - SRGB_Y[0]) <= LINEAR_BOUND, rgba57
        receipts['g57'] = {'red': red57, 'green': green57, 'rgba32': rgba57}
        print('PASS g57: a type layer stores its colour as its gray, renders R = G = B and '
              're-greys on edit; 32 bits stores linear Y', flush=True)

        # G58: shape.
        shapes58 = {}
        probe58 = [(x, y) for y in range(0, 12, 2) for x in range(0, 16, 2)]
        for depth in (16, 8):
            sdoc = new_gray(depth, w=16, h=12)
            shape = art_step(sdoc, probe58, 'add_shape_layer', kind='rect', x=3, y=3, w=10, h=6,
                             fill='#00ff00')
            px = on_oracle(sdoc, 8, 6, green, depth, f'shape {depth}')
            rendered_neutral(sdoc, f'shape layer {depth}', layer=shape['layer'])
            hex_gray(row_of(sdoc, shape['layer'])['shape']['fill'], green, 'shape fill')
            shapes58[depth] = {'doc': sdoc, 'layer': shape['layer'], 'interior': px}
        receipts['g58'] = {str(d): v['interior'] for d, v in shapes58.items()}
        print('PASS g58: a shape layer renders its gray at 16 and 8 bits, R = G = B', flush=True)

        # G59: fill layers.
        probe59 = [(x, y) for y in range(0, 8, 2) for x in range(0, 12, 2)]
        fdoc59 = new_gray(16)
        solid59 = art_step(fdoc59, probe59, 'add_fill_layer', kind='solid', color='#4080c0')
        on_oracle(fdoc59, 5, 4, unit('#4080c0'), 16, 'solid fill')
        rendered_neutral(fdoc59, 'solid fill')
        art_step(fdoc59, probe59, 'edit_fill_layer', layer=solid59['layer'], color='#c04080')
        edited59 = on_oracle(fdoc59, 5, 4, unit('#c04080'), 16, 'edited fill')
        gdoc59 = new_gray(16)
        gradient59 = call('add_fill_layer', document_id=gdoc59, kind='gradient', gradient={
            'stops': [{'position': 0, 'color': '#ff0000', 'opacity': 1},
                      {'position': 1, 'color': '#0000ff', 'opacity': 1}],
            'angle': 0, 'style': 'linear'})
        stops59 = row_of(gdoc59, gradient59['layer'])['fill']['gradient']['stops']
        assert len(stops59) == 2, stops59
        hex_gray(stops59[0]['color'], red, 'stop red')
        hex_gray(stops59[1]['color'], blue, 'stop blue')
        rendered_neutral(gdoc59, 'gradient fill')
        pdoc59 = new_gray(16)
        call('add_fill_layer', document_id=pdoc59, kind='pattern',
             pattern={'pattern': {'preset': 'stripes'}})
        rendered_neutral(pdoc59, 'preset pattern fill')
        tile59 = base64.b64encode(bytes([255, 0, 0, 255, 0, 0, 255, 255])).decode()
        cdoc59 = new_gray(16)
        call('add_fill_layer', document_id=cdoc59, kind='pattern',
             pattern={'pattern': {'name': 'Red Blue', 'width': 2, 'height': 1, 'rgba': tile59}})
        rendered_neutral(cdoc59, 'custom pattern fill')
        receipts['g59'] = {'edited': edited59, 'stops': stops59}
        print('PASS g59: solid, gradient and pattern fill layers store gray and render R = G = B',
              flush=True)

        # G60: transform, rasterize, .rz and the ink refusal.
        sdoc60, sidx60 = shapes58[16]['doc'], shapes58[16]['layer']
        call('transform_layer', document_id=sdoc60, layer=sidx60, scale_x=2, scale_y=2)
        assert 'shape' in row_of(sdoc60, sidx60), row_of(sdoc60, sidx60)
        rendered_neutral(sdoc60, 'transformed shape', layer=sidx60)
        raster60 = call('rasterize_artwork', document_id=sdoc60, layer=sidx60)
        assert raster60['editable_description'] is False, raster60
        assert 'shape' not in row_of(sdoc60, sidx60), row_of(sdoc60, sidx60)
        call('undo', document_id=sdoc60)
        shape60 = row_of(sdoc60, sidx60)['shape']
        before60 = samples(sdoc60, probe58)
        rz60 = folder / 'artwork-6a.rz'
        call('save_copy', document_id=sdoc60, path=str(rz60), format='rz')
        back60 = call('open_document', path=str(rz60))['document']['id']
        assert mode(back60) == 'gray'
        assert json.dumps(row_of(back60, sidx60)['shape'], sort_keys=True) == json.dumps(
            shape60, sort_keys=True), (row_of(back60, sidx60), shape60)
        assert samples(back60, probe58) == before60, 'the .rz samples'
        ink60 = call('add_text_layer', fail=True, document_id=sdoc60, text='Ink', x=1, y=1,
                     size=8, ink=[0, 0, 0, 100])
        assert ink60 == INK_REFUSAL, ink60
        receipts['g60'] = {'rasterize': raster60, 'ink': ink60}
        print('PASS g60: transform keeps the description, rasterize drops it (undone), .rz keeps '
              'it, ink refuses', flush=True)

        # G61: paths and vector masks.
        pdoc61 = new_gray(16, w=24, h=16)
        call('new_layer', document_id=pdoc61, name='Top')
        box61 = {'closed': True, 'anchors': [[4, 4], [20, 4], [20, 12], [4, 12]]}
        call('set_path', document_id=pdoc61, name='Box', subpaths=[box61])
        assert [p['name'] for p in call('list_paths', document_id=pdoc61)['paths']] == ['Box']
        selected61 = call('path_to_selection', document_id=pdoc61, path='Box')
        assert selected61['selection_empty'] is False, selected61
        call('deselect', document_id=pdoc61)
        probe61 = [(x, y) for y in range(0, 16, 2) for x in range(0, 24, 2)]
        art_step(pdoc61, probe61, 'fill_path', path='Box', color='#ff0000', layer=1)
        filled61 = on_oracle(pdoc61, 12, 8, red, 16, 'fill_path')
        rendered_neutral(pdoc61, 'filled path')
        call('stroke_path', document_id=pdoc61, path='Box', color='#0000ff', size=2, layer=1)
        rendered_neutral(pdoc61, 'stroked path')
        call('set_vector_mask', document_id=pdoc61, layer=1, path='Box')
        assert call('get_vector_mask', document_id=pdoc61, layer=1)['vector_mask'] is True
        rendered_neutral(pdoc61, 'vector-masked composite')
        rz61 = folder / 'paths-6a.rz'
        call('save_copy', document_id=pdoc61, path=str(rz61), format='rz')
        back61 = call('open_document', path=str(rz61))['document']['id']
        assert mode(back61) == 'gray'
        assert [p['name'] for p in call('list_paths', document_id=back61)['paths']] == ['Box']
        assert call('get_vector_mask', document_id=back61, layer=1)['vector_mask'] is True
        receipts['g61'] = {'filled': filled61}
        print('PASS g61: paths, Fill and Stroke Path and a vector mask work and persist in .rz',
              flush=True)

        # G62: the profile and mode conversions keep the layers described.
        cdoc62 = new_gray(16, w=16, h=12)
        shape62 = call('add_shape_layer', document_id=cdoc62, kind='rect', x=3, y=3, w=10, h=6,
                       fill='#00ff00')['layer']
        count62, before62 = undo_count(cdoc62), center(cdoc62, 8, 6)
        call('convert_profile', document_id=cdoc62, profile='dot_gain_20')
        assert undo_count(cdoc62) == count62 + 1 and mode(cdoc62) == 'gray'
        assert profile_of(cdoc62) == 'Dot Gain 20%', profile_of(cdoc62)
        assert 'shape' in row_of(cdoc62, shape62), row_of(cdoc62, shape62)
        converted62 = on_oracle(cdoc62, 8, 6, green, 16, 'converted shape', space='dot_gain_20')
        call('undo', document_id=cdoc62)
        assert center(cdoc62, 8, 6) == before62 and profile_of(cdoc62) != 'Dot Gain 20%'
        text62 = call('add_text_layer', document_id=cdoc62, text='Mode', x=1, y=1, size=8,
                      font='Helvetica Neue', color='#ff0000')['layer']
        modes62 = {}
        for target, extra in (('rgb', {'rgb_space': 'srgb'}), ('cmyk', {})):
            call('convert_mode', document_id=cdoc62, mode=target, **extra)
            assert mode(cdoc62) == target, target
            row = row_of(cdoc62, text62)
            assert 'text' in row, (target, row)
            modes62[target] = row['text'].get('color')
            call('undo', document_id=cdoc62)
            assert mode(cdoc62) == 'gray', target
        receipts['g62'] = {'converted': converted62, 'modes': modes62}
        print('PASS g62: Convert to Profile, RGB Color and CMYK Color keep type and shape layers '
              'editable', flush=True)

        # ---- G63–G68: comps, links, snapshots, Clear and the carry (phase 6a-ii) ---------------
        def comp_names(doc):
            return [c['name'] for c in call('list_layer_comps', document_id=doc)['comps']]

        def counted(doc, tool, **arguments):
            """`tool` adds exactly one undo step: its reply."""
            count = undo_count(doc)
            reply = call(tool, document_id=doc, **arguments)
            assert undo_count(doc) == count + 1, (tool, reply)
            return reply

        def rendered(doc):
            (w, h, pixels), _ = image('render', document_id=doc)
            return w, h, pixels

        def two_layer(depth=16, w=12, h=8):
            """New Grayscale #808080 with layer 1 an opaque #303030 rectangle."""
            doc = new_gray(depth, w=w, h=h)
            call('new_layer', document_id=doc, name='Top')
            call('select_rect', document_id=doc, x=2, y=2, width=7, height=4)
            call('fill_selection', document_id=doc, layer=1, color='#303030')
            call('deselect', document_id=doc)
            return doc

        # G63: comps.
        cdoc63 = two_layer()
        counted(cdoc63, 'save_layer_comp', name='A')
        before63 = rendered(cdoc63)
        call('set_layer_properties', document_id=cdoc63, index=1, visible=False, opacity=0.5,
             blend_mode='Multiply')
        assert rendered(cdoc63) != before63, 'the properties changed the render'
        applied63 = counted(cdoc63, 'apply_layer_comp', name='A')
        assert mode(cdoc63) == 'gray', applied63
        assert rendered(cdoc63) == before63, 'apply restores the render'
        counted(cdoc63, 'save_layer_comp', name='B')
        counted(cdoc63, 'update_layer_comp', name='A', new_name='A2')
        assert comp_names(cdoc63) == ['A2', 'B'], comp_names(cdoc63)
        counted(cdoc63, 'reorder_layer_comp', name='B', to=0)
        assert comp_names(cdoc63) == ['B', 'A2'], comp_names(cdoc63)
        counted(cdoc63, 'delete_layer_comp', name='B')
        names63 = comp_names(cdoc63)
        assert names63 == ['A2'], names63
        rz63 = folder / 'comps-6a2.rz'
        call('save_copy', document_id=cdoc63, path=str(rz63), format='rz')
        back63 = call('open_document', path=str(rz63))['document']['id']
        assert mode(back63) == 'gray' and comp_names(back63) == names63, comp_names(back63)
        receipts['g63'] = {'applied': applied63, 'names': names63}
        print('PASS g63: layer comps save, apply (render restored), update, reorder, delete and '
              'persist in .rz, one undo step each', flush=True)

        # G64: links.
        ldoc64 = two_layer()
        call('new_layer', document_id=ldoc64, name='Partner')
        call('select_rect', document_id=ldoc64, x=1, y=1, width=4, height=3)
        call('fill_selection', document_id=ldoc64, layer=2, color='#a0a0a0')
        call('deselect', document_id=ldoc64)
        counted(ldoc64, 'link_layers', layers=[1, 2])
        rows64 = {r['index']: r for r in layers(ldoc64)}
        assert rows64[1].get('linked') and rows64[1].get('linked') == rows64[2].get('linked'), rows64
        x64 = {i: rows64[i]['offset_x'] for i in (1, 2)}
        call('transform_layer', document_id=ldoc64, layer=1, translate_x=2, sampler='nearest')
        moved64 = {r['index']: r['offset_x'] for r in layers(ldoc64) if r['index'] in (1, 2)}
        assert moved64 == {1: x64[1] + 2, 2: x64[2] + 2}, (x64, moved64)
        assert mode(ldoc64) == 'gray'
        counted(ldoc64, 'unlink_layers', layers=[1, 2])
        assert all('linked' not in r for r in layers(ldoc64)), layers(ldoc64)
        call('transform_layer', document_id=ldoc64, layer=1, translate_x=2, sampler='nearest')
        alone64 = {r['index']: r['offset_x'] for r in layers(ldoc64) if r['index'] in (1, 2)}
        assert alone64 == {1: moved64[1] + 2, 2: moved64[2]}, (moved64, alone64)
        receipts['g64'] = {'before': x64, 'linked': moved64, 'unlinked': alone64}
        print('PASS g64: link_layers moves the partner with the layer; unlink_layers stops it, one '
              'undo step each', flush=True)

        # G65: snapshots.
        probe65 = [(x, y) for y in range(0, 8, 2) for x in range(0, 12, 2)]
        sdoc65 = two_layer()
        before65 = samples(sdoc65, probe65)
        snap65 = call('create_snapshot', document_id=sdoc65, name='S')['snapshot_id']
        call('brush_stroke', document_id=sdoc65, points=[[1, 1], [10, 6]], size=4, hardness=100,
             color='#ffffff')
        assert samples(sdoc65, probe65) != before65, 'the stroke changed nothing'
        counted(sdoc65, 'restore_snapshot', snapshot_id=snap65)
        assert mode(sdoc65) == 'gray' and samples(sdoc65, probe65) == before65, 'restore'
        package65 = (output / 's.drsnapshot').resolve()
        call('save_snapshot', document_id=sdoc65, snapshot_id=snap65, path=str(package65))
        call('delete_snapshot', document_id=sdoc65, snapshot_id=snap65)
        loaded65 = call('load_snapshot', document_id=sdoc65, path=str(package65))['snapshot_id']
        call('brush_stroke', document_id=sdoc65, points=[[1, 6], [10, 1]], size=4, hardness=100,
             color='#000000')
        assert samples(sdoc65, probe65) != before65, 'the second stroke changed nothing'
        call('restore_snapshot', document_id=sdoc65, snapshot_id=loaded65)
        assert mode(sdoc65) == 'gray' and samples(sdoc65, probe65) == before65, 'loaded restore'
        receipts['g65'] = {'package': str(package65)}
        print('PASS g65: snapshots restore a Grayscale document, saved and loaded included',
              flush=True)

        # G66: Edit ▸ Clear.
        probe66 = [(x, y) for y in range(8) for x in range(12)]
        kdoc66 = new_gray(16)
        before66 = samples(kdoc66, probe66)
        call('select_rect', document_id=kdoc66, x=3, y=2, width=5, height=4)
        counted(kdoc66, 'clear_selection', layer=0)
        assert mode(kdoc66) == 'gray'
        for (x, y), was in zip(probe66, before66):
            px = center(kdoc66, x, y)
            if 3 <= x < 8 and 2 <= y < 6:
                assert px[3] == 0, ((x, y), px)
            else:
                assert px == was, ((x, y), px, was)
        menu66 = call('get_precision', document_id=kdoc66)['supported_menu_items']
        assert 'clearSelection:' in menu66, menu66
        receipts['g66'] = 'cleared'
        print('PASS g66: clear_selection erases the selection, one undo step; Edit ▸ Clear is '
              'listed', flush=True)

        # G67: RGB and CMYK → Gray keep type and shape layers editable.
        rdoc67 = call('new_document', width=40, height=16, depth=16,
                      background='#808080')['document']['id']
        shape67 = call('add_shape_layer', document_id=rdoc67, kind='rect', x=3, y=3, w=10, h=8,
                       fill='#00ff00')['layer']
        text67 = call('add_text_layer', document_id=rdoc67, text='Gray', x=16, y=1, size=12,
                      font='Helvetica Neue', color='#ff0000')['layer']
        counted(rdoc67, 'convert_mode', mode='gray', gray_space='gray_gamma_22')
        assert mode(rdoc67) == 'gray'
        call('undo', document_id=rdoc67)
        assert mode(rdoc67) == 'rgb'
        call('redo', document_id=rdoc67)
        assert mode(rdoc67) == 'gray'
        shape_row67, text_row67 = row_of(rdoc67, shape67), row_of(rdoc67, text67)
        assert 'shape' in shape_row67 and 'text' in text_row67, (shape_row67, text_row67)
        fill67 = hex_gray(shape_row67['shape']['fill'], green, 'carried shape fill')
        color67 = hex_gray(text_row67['text']['color'], red, 'carried text colour')
        interior67 = on_oracle(rdoc67, 8, 7, green, 16, 'carried shape')
        rendered_neutral(rdoc67, 'carried shape layer', layer=shape67)
        rendered_neutral(rdoc67, 'carried text layer', layer=text67)
        edoc67 = call('new_document', width=40, height=16, depth=8,
                      background='#808080')['document']['id']
        etext67 = call('add_text_layer', document_id=edoc67, text='Gray', x=2, y=1, size=12,
                       font='Helvetica Neue', color='#ff0000')['layer']
        call('convert_mode', document_id=edoc67, mode='gray', gray_space='gray_gamma_22')
        assert mode(edoc67) == 'gray' and 'text' in row_of(edoc67, etext67), row_of(edoc67, etext67)
        # The text layer's own samples: the background hidden, so the composite is the layer (a
        # composite over the background blends off the grid, as an 8-bit Gray document paints).
        call('set_layer_properties', document_id=edoc67, index=0, visible=False)
        for px in samples(edoc67, probe57):
            assert px[0] % 257 == 0 and px[0] == px[1] == px[2], px
        kdoc67 = call('new_document', width=40, height=16, color_mode='cmyk', depth=16,
                      background='#ffffff')['document']['id']
        call('new_layer', document_id=kdoc67, name='Stroke')
        call('brush_stroke', document_id=kdoc67, layer=1, points=[[30, 8], [34, 8]], size=8,
             hardness=100, color='#ff0000')
        ktext67 = call('add_text_layer', document_id=kdoc67, text='Gr', x=1, y=1, size=12,
                       font='Helvetica Neue', color='#ff0000')['layer']
        call('convert_mode', document_id=kdoc67, mode='gray', gray_space='sgray')
        assert mode(kdoc67) == 'gray'
        krow67 = row_of(kdoc67, ktext67)
        assert 'text' in krow67, krow67
        kdigits = krow67['text']['color'].lstrip('#').lower()
        assert kdigits[0:2] == kdigits[2:4] == kdigits[4:6], kdigits
        stroke67 = center(kdoc67, 32, 8)
        assert stroke67[0] == stroke67[1] == stroke67[2], stroke67
        assert abs(int(kdigits[0:2], 16) - stroke67[0] / 257) <= 1, (kdigits, stroke67)
        receipts['g67'] = {'fill': fill67, 'color': color67, 'interior': interior67,
                           'cmyk_text': kdigits, 'cmyk_stroke': stroke67}
        print('PASS g67: RGB and CMYK → Grayscale keep type and shape layers editable, greyed',
              flush=True)

        # G68: RGB → Gray carries a comp, its style greyed.
        mdoc68 = call('new_document', width=12, height=8, depth=16,
                      background='#808080')['document']['id']
        call('new_layer', document_id=mdoc68, name='Top')
        call('select_rect', document_id=mdoc68, x=3, y=2, width=5, height=4)
        call('fill_selection', document_id=mdoc68, layer=1, color='#c0c0c0')
        call('deselect', document_id=mdoc68)
        call('set_layer_style', document_id=mdoc68, layer=1, style={'effects': [
            {'type': 'drop_shadow', 'color': '#ff0000', 'distance': 2, 'size': 1}]})
        call('save_layer_comp', document_id=mdoc68, name='Red', visibility=False, position=False,
             appearance=True)
        call('set_layer_style', document_id=mdoc68, layer=1, style=None)
        assert not square_style(mdoc68), square_style(mdoc68)
        call('convert_mode', document_id=mdoc68, mode='gray', gray_space='gray_gamma_22')
        assert mode(mdoc68) == 'gray' and comp_names(mdoc68) == ['Red'], comp_names(mdoc68)
        counted(mdoc68, 'apply_layer_comp', name='Red')
        shadow68 = next(e for e in square_style(mdoc68)['effects'] if e['type'] == 'drop_shadow')
        hex68 = hex_gray(shadow68['color'], red, 'carried comp shadow')
        receipts['g68'] = {'shadow': hex68}
        print('PASS g68: RGB → Grayscale carries a layer comp, its shadow greyed', flush=True)

        # ---- G69–G77: analysis, multi-image, neural and generative tools (phase 6b) ------------
        def attempt(tool, **arguments):
            """`tool` cleared the Grayscale gate (module doc, phase 6b): (ok, reply or text)."""
            result = app.client.rpc('tools/call', {'name': tool, 'arguments': arguments})
            text = next((c['text'] for c in result.get('content', []) if c['type'] == 'text'), '')
            if result.get('isError'):
                assert not any(part in (text or '') for part in GATE_PARTS), (tool, text)
                return False, text
            try:
                return True, json.loads(text)
            except (TypeError, json.JSONDecodeError):
                return True, text

        def doc_info(doc):
            return next(d for d in call('list_documents')['documents'] if d['id'] == doc)

        # G69: the Histogram's one Gray row.
        hist69 = {}
        for depth, top in ((8, 255), (16, 65535)):
            hdoc = new_gray(depth, w=6, h=4)
            code = center(hdoc, 1, 1)[0]
            code = code // 257 if depth == 8 else code
            report = call('histogram', document_id=hdoc)
            assert report['channels'] == ['gray'], report.get('channels')
            assert not [k for k in report if k.endswith('_luma')], sorted(report)
            assert len(report['bins']) == 1 and len(report['bin_weight_units']) == 1, report
            filled = [i for i, v in enumerate(report['bins'][0]) if v]
            assert filled == [round(code * 255 / top)], (depth, code, filled)
            hist69[depth] = {'code': code, 'bins': filled}
            if depth == 16:
                red69 = call('histogram', fail=True, document_id=hdoc, source='scalar',
                             target='red')
                assert red69 == PLANE_REFUSAL.format('histogram', 'red'), red69
                gray69 = call('histogram', fail=True, document_id=hdoc, source='scalar',
                              target='gray')
                assert gray69 == SCALAR_TARGET_REFUSAL, gray69
        receipts['g69'] = hist69
        print('PASS g69: the histogram is one Gray row; colour scalar targets refuse', flush=True)

        # G70: Apply Image onto the layer, a channel, and its refusals.
        adoc70 = new_gray(16, w=6, h=4)
        c70 = center(adoc70, 1, 1)[0]
        probe70 = [(x, y) for y in range(4) for x in range(6)]
        count70 = undo_count(adoc70)
        after70, _ = pixel_step(adoc70, probe70, 'apply_image', layer=0, source='merged',
                                blend_mode='Multiply')
        assert undo_count(adoc70) == count70 + 1, 'apply_image is one undo step'
        want70 = c70 * c70 / 65535
        assert_neutral(after70, 'apply_image')
        assert all(abs(px[0] - want70) <= APPLY_BOUND16 for px in after70), (after70, want70)
        call('add_channel', document_id=adoc70, name='M')
        channel70 = call('apply_image', document_id=adoc70, target='channel:M', source='merged',
                         blend_mode='Screen')
        hue70 = call('apply_image', fail=True, document_id=adoc70, blend_mode='Hue')
        assert GRAY_LIMITATION_HEAD in hue70 and hue70.endswith(GRAY_LIMITATION_TAIL), hue70
        plane70 = call('apply_image', fail=True, document_id=adoc70, source_plane='red')
        assert plane70 == SOURCE_PLANE_REFUSAL, plane70
        fdoc70 = new_gray(32, w=6, h=4)
        float70 = call('apply_image', fail=True, document_id=fdoc70, source='merged')
        assert FLOAT_TOOL_PART in float70, float70
        receipts['g70'] = {'code': c70, 'multiply': after70[0], 'oracle': want70,
                           'channel': channel70.get('action'), 'float': float70}
        print('PASS g70: Apply Image multiplies the Gray layer in one undo step; a channel target '
              'works; Hue, a colour plane and 32 bits refuse', flush=True)

        # G71: Calculations into a new channel and as the selection.
        cdoc71 = new_gray(16, w=6, h=4)
        c71 = center(cdoc71, 1, 1)[0]
        call('calculations', document_id=cdoc71, source1_plane='gray', source2_plane='gray',
             blend_mode='Multiply', name='Calc')
        (w71, h71, calc71), _ = image('render', document_id=cdoc71, channel='Calc')
        want71 = c71 * c71 / 65535 / 257
        assert all(abs(p[0] - want71) <= CALC_BOUND8 for p in calc71), (calc71[:3], want71)
        selected71 = call('calculations', document_id=cdoc71, source1_plane='gray',
                          source2_plane='alpha', blend_mode='Multiply', result='selection')
        assert selected71.get('ok'), selected71
        receipts['g71'] = {'channel': calc71[0], 'oracle8': want71}
        print('PASS g71: Calculations writes a new channel and a selection', flush=True)

        # G72: Stack Modes' mean at 16 and 32 bits; Reduce Noise from Stack.
        stack72 = {}
        for depth in (16, 32):
            sdoc72 = new_gray(depth, w=6, h=4, background='#404040')
            call('new_layer', document_id=sdoc72, name='B')
            call('select_all', document_id=sdoc72)
            call('fill_selection', document_id=sdoc72, layer=1, color='#c0c0c0')
            call('deselect', document_id=sdoc72)
            call('set_layer_properties', document_id=sdoc72, index=1, visible=False)
            a72 = any_depth(sdoc72, [(2, 2)])[0]
            call('set_layer_properties', document_id=sdoc72, index=1, visible=True)
            b72 = any_depth(sdoc72, [(2, 2)])[0]
            one_step(sdoc72, 'stack_mode', layers=[0, 1], mode='mean', name='Mean')
            got72 = any_depth(sdoc72, [(2, 2)])[0]
            assert got72[0] == got72[1] == got72[2], got72
            if depth == 16:
                want72 = (a72[0] + b72[0] + 1) // 2
                assert got72[0] == want72, (a72, b72, got72)
            else:
                f32 = lambda v: struct.unpack('<f', struct.pack('<f', v))[0]
                want72 = f32((a72[0] + b72[0]) / 2)
                assert f32(got72[0]) == want72, (a72, b72, got72)
            stack72[depth] = {'a': a72[0], 'b': b72[0], 'mean': got72[0]}
        # Reduce Noise from Stack aligns its frames first: two copies of the textured layer.
        ndoc72 = textured(16)
        call('duplicate_layer', document_id=ndoc72, index=0)
        noise72 = call('reduce_noise_from_stack', document_id=ndoc72, layers=[0, 1],
                       mode='median')
        assert noise72.get('ok') and mode(ndoc72) == 'gray', noise72
        rendered_neutral(ndoc72, 'reduce noise from stack')
        receipts['g72'] = stack72
        print('PASS g72: Stack Modes mean at 16 and 32 bits, R = G = B, one undo step; Reduce '
              'Noise from Stack runs', flush=True)

        # G73: the model selections clear the gate.
        mdoc73 = textured(16)
        select73 = {
            'select_sky': attempt('select_sky', document_id=mdoc73),
            'select_person_parts': attempt('select_person_parts', document_id=mdoc73,
                                           parts=['hair']),
            'object_finder_select': attempt('object_finder_select', document_id=mdoc73,
                                            box={'x': 2, 'y': 2, 'width': 12, 'height': 8}),
        }
        receipts['g73'] = {k: v[1] if not v[0] else 'ok' for k, v in select73.items()}
        print(f'PASS g73: the model selections clear the gate {receipts["g73"]}', flush=True)

        # G74: a text layer copied into and out of Grayscale stays a text layer.
        rdoc74 = call('new_document', width=40, height=16, depth=16,
                      background='#808080')['document']['id']
        text74 = call('add_text_layer', document_id=rdoc74, text='Gray', x=2, y=1, size=12,
                      font='Helvetica Neue', color='#ff0000')['layer']
        gdoc74 = new_gray(16, w=40, h=16)
        call('copy_layers_to_document', document_id=rdoc74, layers=[text74],
             to_document_id=gdoc74)
        top74 = max(r['index'] for r in layers(gdoc74))
        row74 = row_of(gdoc74, top74)
        assert 'text' in row74, row74
        color74 = hex_gray(row74['text']['color'], (1, 0, 0), 'copied text colour')
        rendered_neutral(gdoc74, 'copied text composite')
        back74 = call('new_document', width=40, height=16, depth=16,
                      background='#808080')['document']['id']
        call('copy_layers_to_document', document_id=gdoc74, layers=[top74],
             to_document_id=back74)
        out74 = row_of(back74, max(r['index'] for r in layers(back74)))
        assert 'text' in out74 and mode(back74) == 'rgb', out74
        receipts['g74'] = {'color': color74, 'out': out74['text'].get('color')}
        print('PASS g74: copy_layers_to_document keeps a text layer editable into and out of '
              'Grayscale', flush=True)

        # G75: Auto-Align and Auto-Blend on a two-layer Grayscale document.
        adoc75 = textured(16)
        call('duplicate_layer', document_id=adoc75, index=0)
        aligned75 = call('auto_align_layers', document_id=adoc75, layers=[0, 1])
        assert aligned75.get('ok') and mode(adoc75) == 'gray', aligned75
        blended75 = call('auto_blend_layers', document_id=adoc75, layers=[0, 1],
                         method='stack_images')
        assert blended75.get('ok') and mode(adoc75) == 'gray', blended75
        rendered_neutral(adoc75, 'auto-blended composite')
        receipts['g75'] = {'align': sorted(aligned75), 'blend': sorted(blended75)}
        print('PASS g75: Auto-Align and Auto-Blend run on a Grayscale document, R = G = B',
              flush=True)

        # G76: Load Files into Stack, Merge to HDR and Merge Panorama from Grayscale files.
        gray76 = folder / 'stack-gray16.png'
        gray76b = folder / 'stack-gray16b.png'
        rgb76 = folder / 'stack-rgb8.png'
        png(gray76, 4, 2, 16, 0, [9000, 20000, 30000, 40000, 50000, 60000, 12345, 54321])
        png(gray76b, 4, 2, 16, 0, [18000, 40000, 60000, 65535, 65535, 65535, 24690, 65535])
        png(rgb76, 4, 2, 8, 2, [v for p in PATCHES[:8] for v in p])
        stack76 = call('load_files_into_stack', paths=[str(gray76), str(rgb76)])
        sdoc76 = stack76['document']['id']
        assert mode(sdoc76) == 'gray', stack76
        odoc76 = call('open_document', path=str(rgb76))['document']['id']
        call('convert_precision', document_id=odoc76, mode='rgba16', structure='preserve')
        call('convert_mode', document_id=odoc76, mode='gray')
        worst76 = 0
        for y in range(2):
            for x in range(4):
                got, want = center(sdoc76, x, y), center(odoc76, x, y)
                assert got[0] == got[1] == got[2], got
                worst76 = max(worst76, abs(got[0] - want[0]))
        assert worst76 <= STACK_LOAD_BOUND16, worst76
        hdr76 = call('merge_to_hdr', paths=[str(gray76), str(gray76b)], evs=[0, 1], align=False)
        hdoc76 = hdr76['document']['id']
        assert mode(hdoc76) == 'gray' and doc_info(hdoc76)['precision'] == 'gray32', hdr76
        pair76 = call('load_files_into_stack', paths=[str(gray76), str(gray76b)])['document']['id']
        hdr76b = call('merge_layers_to_hdr', document_id=pair76, layers=[0, 1], evs=[0, 1],
                      align=False)
        hdoc76b = hdr76b['document']['id']
        assert mode(hdoc76b) == 'gray' and doc_info(hdoc76b)['precision'] == 'gray32', hdr76b
        photo76 = attempt('merge_panorama', paths=[str(gray76), str(gray76b)])
        receipts['g76'] = {'worst16': worst76, 'hdr_profile': hdr76.get('profile'),
                           'merge_panorama': 'ok' if photo76[0] else photo76[1]}
        print('PASS g76: Load Files into Stack, Merge to HDR (files and layers) and Merge Panorama '
              'from Grayscale files', flush=True)

        # G77: the neural and generative tools.
        ndoc77 = textured(16)
        zoom77 = attempt('super_zoom', document_id=ndoc77, layer=0)
        if zoom77[0]:
            assert mode(zoom77[1]['document_id']) == 'gray', zoom77
        jpeg77 = attempt('jpeg_artifacts_removal', document_id=ndoc77, layer=0)
        if jpeg77[0]:
            rendered_neutral(ndoc77, 'jpeg artifacts removal')
        colorize77 = call('neural_colorize', fail=True, document_id=ndoc77, layer=0)
        assert colorize77 == COLORIZE_REFUSAL, colorize77
        sky77 = call('sky_replacement', fail=True, document_id=ndoc77, layer=0)
        assert sky77 == SKY_REFUSAL, sky77
        call('select_rect', document_id=ndoc77, x=4, y=4, width=8, height=6)
        fill77 = attempt('generative_fill', document_id=ndoc77, layer=0, prompt='a stone')
        call('deselect', document_id=ndoc77)
        expand77 = attempt('generative_expand', document_id=ndoc77, layer=0)
        variations77 = attempt('generative_variations', document_id=ndoc77, layer=0)
        receipts['g77'] = {name: 'ok' if result[0] else result[1] for name, result in (
            ('super_zoom', zoom77), ('jpeg_artifacts_removal', jpeg77),
            ('generative_fill', fill77), ('generative_expand', expand77),
            ('generative_variations', variations77))}
        receipts['g77']['refusals'] = [colorize77, sky77]
        print(f'PASS g77: the neural and generative tools clear the gate {receipts["g77"]}; '
              'Colorize and Sky Replacement refuse for good', flush=True)

        # ---- G78–G85: the close ---------------------------------------------------------------
        # G78: the permanent refusals, by name.
        rdoc78 = new_gray(16, w=6, h=4)
        refused78 = {}
        for tool in RAW_TOOLS:
            refused78[tool] = call(tool, fail=True, document_id=rdoc78)
            assert refused78[tool] == RAW_REFUSAL.format(tool), (tool, refused78[tool])
        refused78['add_text'] = call('add_text', fail=True, document_id=rdoc78, text='A')
        assert refused78['add_text'] == ADD_TEXT_REFUSAL, refused78['add_text']
        refused78['put_pixels'] = call('put_pixels', fail=True, document_id=rdoc78)
        assert refused78['put_pixels'] == PUT_PIXELS_REFUSAL, refused78['put_pixels']
        for tool in ('get_evaluation', 'set_evaluation'):
            refused78[tool] = call(tool, fail=True, document_id=rdoc78)
            assert refused78[tool] == EVALUATION_REFUSAL.format(tool), (tool, refused78[tool])
        dng78 = folder / 'x.dng'
        dng78.write_bytes(b'')
        refused78['place_image dng'] = call('place_image', fail=True, document_id=rdoc78,
                                            path=str(dng78))
        assert RAW_PART in refused78['place_image dng'], refused78['place_image dng']
        receipts['g78'] = refused78

        # G79: the lifted reads and edits answer.
        doc79 = two_layer(16)
        g79 = {}
        g79['arrange_layer'] = call('arrange_layer', document_id=doc79, layer=1, to='back')
        g79['find_layers'] = call('find_layers', document_id=doc79, kind='raster')
        call('add_layer_mask', document_id=doc79, layer=1, kind='reveal_all')
        g79['get_mask_properties'] = call('get_mask_properties', document_id=doc79, layer=1)
        g79['auto_select_layer'] = call('auto_select_layer', document_id=doc79, x=3, y=3)
        count79 = undo_count(doc79)
        call('set_resolution', document_id=doc79, ppi_x=150)
        assert undo_count(doc79) == count79 + 1, 'set_resolution is one undo step'
        assert mode(doc79) == 'gray'
        meta79 = call('get_metadata', document_id=doc79)
        assert meta79['ppi_x'] == 150 and meta79['ppi_y'] == 150, meta79
        call('set_file_info', document_id=doc79, copyright='Close 547')
        info79 = call('get_file_info', document_id=doc79)
        assert info79['copyright'] == 'Close 547', info79
        g79['set_ruler_origin'] = call('set_ruler_origin', document_id=doc79, x=2, y=3)
        g79['set_history_budget'] = call('set_history_budget', document_id=doc79,
                                         bytes=256 * 1024 * 1024)
        g79['separate_color'] = call('separate_color', document_id=doc79, rgb=[128, 128, 128])
        g79['list_lens_profiles'] = call('list_lens_profiles', document_id=doc79)
        g79['detect_horizon'] = call('detect_horizon', document_id=doc79)
        assert mode(doc79) == 'gray'
        receipts['g79'] = sorted(g79)
        print('PASS g79: the lifted layer, metadata, ruler, history, colour and lens tools answer '
              'on a Grayscale document', flush=True)

        # G80: views and workspaces.
        vdoc80 = new_gray(16, w=24, h=16)
        opened80 = call('open_second_view', document_id=vdoc80)
        assert opened80['view']['view'] == 2 and len(opened80['views']) == 2, opened80
        set80 = call('set_view', document_id=vdoc80, view=2, zoom=1)
        assert set80['zoom'] == 1.0, set80
        assert call('get_view', document_id=vdoc80, view=2)['zoom'] == 1.0
        closed80 = call('close_second_view', document_id=vdoc80, view=2)
        assert len(closed80['views']) == 1, closed80
        call('save_workspace', document_id=vdoc80, name='Gray close')
        restored80 = call('restore_workspace', document_id=vdoc80, name='Gray close')
        receipts['g80'] = {'views': len(closed80['views']), 'restored': sorted(restored80)}
        print('PASS g80: second views and workspaces round-trip on a Grayscale document',
              flush=True)

        # G81: the samplers' K %.
        sdoc81 = textured(16)
        points81 = [(3, 3), (10, 5)]
        call('set_color_samplers', document_id=sdoc81,
             samplers=[{'x': x, 'y': y} for x, y in points81])
        rows81 = call('get_color_samplers', document_id=sdoc81)['samplers']
        k81 = []
        for row, (x, y) in zip(rows81, points81):
            want = call('sample_color', document_id=sdoc81, x=x, y=y)['gray_k_percent']
            assert row['gray_k_percent'] == want, (row, want)
            k81.append(want)
        receipts['g81'] = k81
        print('PASS g81: each Color Sampler reports sample_color\'s K %', flush=True)

        # G82: Distort and Modify Native Plane.
        ddoc82 = textured(16)
        count82 = undo_count(ddoc82)
        call('distort_layer', document_id=ddoc82, layer=0,
             corners=[[1, 1], [PATTERN_W - 2, 0], [PATTERN_W - 4, PATTERN_H - 1], [2, PATTERN_H - 2]])
        assert undo_count(ddoc82) == count82 + 1, 'distort_layer is one undo step'
        assert mode(ddoc82) == 'gray'
        rendered_neutral(ddoc82, 'distorted composite')
        mdoc82 = new_gray(16, w=12, h=8)
        call('select_rect', document_id=mdoc82, x=5, y=3, width=2, height=2)
        call('add_layer_mask', document_id=mdoc82, layer=0, kind='from_selection')
        call('deselect', document_id=mdoc82)
        (_, _, mask82), _ = image('render', document_id=mdoc82, channel='mask', layer=0)
        call('modify_scalar', document_id=mdoc82, layer=0, target='mask', operation='grow',
             radius=2)
        (_, _, grown82), _ = image('render', document_id=mdoc82, channel='mask', layer=0)
        assert grown82 != mask82, 'modify_scalar grew the mask'
        red82 = call('modify_scalar', fail=True, document_id=mdoc82, layer=0, target='red',
                     operation='grow')
        assert red82 == PLANE_REFUSAL.format('modify_scalar', 'red'), red82
        receipts['g82'] = {'plane': red82}
        print('PASS g82: Distort keeps R = G = B in one undo step; Modify Native Plane grows a '
              'mask and refuses a colour plane', flush=True)

        # G83: Place and Live Photo layers land folded to gray.
        place83 = folder / 'place-rgb8.png'
        png(place83, len(PATCHES), 1, 8, 2, [v for p in PATCHES for v in p])
        pdoc83 = new_gray(16, w=len(PATCHES), h=1)
        count83 = undo_count(pdoc83)
        call('place_image', document_id=pdoc83, path=str(place83))
        assert undo_count(pdoc83) == count83 + 1, 'place_image is one undo step'
        assert mode(pdoc83) == 'gray'
        odoc83 = call('open_document', path=str(place83))['document']['id']
        call('convert_precision', document_id=odoc83, mode='rgba16', structure='preserve')
        call('convert_mode', document_id=odoc83, mode='gray', gray_space='gray_gamma_22')
        worst83 = 0
        for x in range(len(PATCHES)):
            got, want = center(pdoc83, x, 0), center(odoc83, x, 0)
            assert got[0] == got[1] == got[2], got
            worst83 = max(worst83, abs(got[0] - want[0]))
        assert worst83 <= PLACE_BOUND16, worst83
        from verify_phase12e2b_placement import author_movie
        movie83, reason83 = author_movie(folder)
        if movie83 is None:
            live83 = f'NOT RUN: {reason83}'
            print(f'NOT RUN g83 add_live_photo_layer: {reason83}', flush=True)
        else:
            ldoc83 = new_gray(16, w=64, h=48)
            call('add_live_photo_layer', document_id=ldoc83, path=str(movie83))
            assert mode(ldoc83) == 'gray'
            rendered_neutral(ldoc83, 'live photo layer')
            live83 = 'ok'
        receipts['g83'] = {'worst16': worst83, 'live_photo': live83}
        print('PASS g83: a placed colour image lands R = G = B on its Mode ▸ Grayscale oracle',
              flush=True)

        # G84: Export Layers, PDF and the contact sheet.
        edoc84 = two_layer(16)
        layers84 = folder / 'layers84'
        layers84.mkdir()
        exported84 = call('export_layers', document_id=edoc84, items='all',
                          settings={'format': 'png', 'bit_depth': 16, 'profile': 'document'},
                          destination=str(layers84), pattern='{index}-{layer}')
        paths84 = [Path(item['path']) for item in exported84['items'] if item.get('path')]
        assert len(paths84) == 2, exported84
        types84 = [p.read_bytes()[25] for p in paths84]
        assert all(t in (0, 4) for t in types84), types84
        pdf84 = folder / 'gray84.pdf'
        call('export_pdf', document_id=edoc84, path=str(pdf84))
        assert pdf84.read_bytes()[:4] == b'%PDF', pdf84.read_bytes()[:8]
        before84 = len(call('list_documents')['documents'])
        call('contact_sheet', document_id=edoc84, items='all', output={'kind': 'document'})
        assert len(call('list_documents')['documents']) == before84 + 1
        receipts['g84'] = {'png_colour_types': types84, 'pdf_bytes': pdf84.stat().st_size}
        print('PASS g84: Export Layers writes one-channel PNGs; PDF and the contact sheet run',
              flush=True)

        # G85: the 32-bit tone map.
        tdoc85 = textured(32)
        before85 = rendered(tdoc85)
        call('set_tone_map', document_id=tdoc85, operator='reinhard', white=4)
        assert rendered(tdoc85) != before85, 'the tone map changed the render'
        assert mode(tdoc85) == 'gray'
        call('undo', document_id=tdoc85)
        assert rendered(tdoc85) == before85, 'undo restored the render'
        u16_85 = call('set_tone_map', fail=True, document_id=new_gray(16, w=6, h=4),
                      operator='reinhard')
        assert u16_85.startswith(TONE_MAP_FLOAT_PART), u16_85
        receipts['g85'] = {'u16': u16_85}
        print('PASS g85: the 32-bit tone map changes the render in one undo step; 16 bits refuse '
              'as on RGB', flush=True)

        # G78's last check: no reply of the run named a #1 phase.
        assert phase_replies == [], phase_replies
        print('PASS g78: the RAW, byte and evaluation tools refuse for good by name; no reply '
              'names a #1 phase', flush=True)

    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print(f'receipts: {output / "receipt.json"}', flush=True)
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
