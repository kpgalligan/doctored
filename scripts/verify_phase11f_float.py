#!/usr/bin/env python3
"""The Phase 11F HDR / 32-bit float MCP drive, on a built Release app.

Nine gates, FIXED before the run; a failed gate IS the receipt and no
tolerance is widened after the fact.

- **F1 open.** `core/tests/fixtures/float_11f/scene.tiff` — a 48x32 RGBA f32
  Deflate TIFF with linear ramps, an 8.0 above-white block, a -0.25 block and
  a half-alpha band, authored by `--author-fixture` below and verified by
  `tiffinfo` / `magick identify` — opens as a 32-bit float document. Its
  samples come back BIT-EXACT through a 32-bit export, and the import
  warnings are empty.
- **F2 editing.** An Exposure +1 EV adjustment layer is exactly a gain of two
  (8.0 becomes 16.0, -0.25 becomes -0.5); adding a Multiply copy underneath
  it reproduces an f64 oracle written in this file. **T_REL** 1e-5 relative.
- **F3 persistence.** `save_copy` to `.rz` and reopen: samples bit-exact, the
  file's first word 40 and its second 39.
- **F4 snapshots and recovery.** `create_snapshot` / `restore_snapshot`, and
  `write_recovery` + a forced crash + relaunch + `recover_document`, both
  bit-exact.
- **F5 exact delivery.** `export_image bit_depth:32` read by `tiffinfo`
  (SampleFormat 3, 32 bits, 4 samples, ExtraSamples 2, ICC present), by
  `magick identify` and by the stdlib parser below; samples bit-exact.
- **F6 tone-mapped delivery.** An 8-bit PNG under both maps, within **T_CODE**
  1 code of the Python oracle (map, then the sRGB transfer function, then
  round).
- **F7 conversion.** `convert_precision mode:"rgba16"` matches
  `round(65535 * oracle)` within 1 code of 65535; the converted document's
  `.rz` starts at 39 and `get_evaluation` reports `linear_light`.
- **F8 refusals.** `set_layer_properties blend_mode:Screen`,
  `add_adjustment_layer op:invert`, `set_evaluation encoded`,
  `set_tone_map operator:clip white:2`, `open_document precision:rgba8` and
  `export_image bit_depth:32` on a u16 document each refuse by name, and
  `get_document` is unchanged afterwards. `image_size` is ADMITTED at float
  (filtered geometry), so it runs after that check and must resize.
- **F9 the u16 mode is untouched.** A 16-bit document's `.rz` and its PNG16
  export are byte-identical before and after the whole run.
- **F11 float save_preservation.** `get_document` on a float
  document reports `save_preservation`, whose `losses_by_format.png` carries
  the 16-bit float sentence; `save_copy format:png` (no `bit_depth`) succeeds
  with `bit_depth` 16 and `master_bit_depth` 32, and its samples equal
  `export_image {format:png, bit_depth:16}`'s.
- **F12 layer and plane renders.** On a fresh float document,
  `render layer:` returns the layer tone-mapped like the composite (within
  **T_CODE8**, under `clip` and `reinhard`), ignores the layer's opacity,
  renders a group alone, and `render channel:` red/green/blue/alpha/luma —
  of the composite and of the raster record — matches F6's oracle (alpha
  `round(a * 255)`, luma Rec.709 over the oracle codes) within **T_CODE8**.
  An out-of-range layer refuses by name, no reply carries the u16 selection
  refusal, the composite reply says "32-bit float", and on the u16 document a
  layer render equals its composite. The group `group_layers` makes is Pass
  Through, as at every other depth.
- **F13 Load Selection from a plane or layer transparency.** On a fresh
  float document, `load_selection` from the red/green/blue/luma/alpha planes
  and from layer 0's transparency gives `get_selection` `coverage16` within
  **T_COV16** of `round(65535 * clamp(v, 0, 1))` (luma
  `round(65535 * clamp(sum y_i * v_i, 0, 1))` over the UNclamped samples, `y`
  the fixture profile's Y row through `icc_basis`) at seven points
  covering 8.0, -0.25, ramps and half alpha; a
  `reinhard` tone map leaves the red plane's coverage unchanged; and no reply
  carries the u16 selection refusal (`select_magic_wand` is admitted, see
  F46).
- **F14 the 32-bit blend modes measured against Photoshop.** On a fresh float
  document, `duplicate_layer` then `set_layer_properties` moves the copy by
  (6, 4) and sets each built mode in turn — Linear Dodge (Add), Subtract,
  Darker Color, Lighter Color, Hue, Saturation, Color, Luminosity — and the
  `export32` composite matches this file's f64 oracle of Photoshop's measured
  32-bit formulas (unclamped Linear Dodge and Subtract; W3C weights; SetLum
  with no ClipColor) within **T_REL**; Dissolve shows the copy at alpha 1 or
  keeps the backdrop on every pixel, both at least once. `fill_selection`
  with white ink in Subtract at 0.5 on another fresh copy matches its oracle.
  `save_copy` `.rz` and reopen: samples bit-exact and `get_document` reports
  the mode. Screen and Overlay still refuse with the float layer-blending
  sentence (Divide is admitted, see F46).
- **F15 a layer style, Blend If and the global light.** On a
  fresh float document, `duplicate_layer` moves the copy by (6, 4) and
  `set_layer_style` gives it a hard drop shadow (Normal, size 0, distance 4,
  angle 0 — cast left, onto the backdrop the copy leaves uncovered), a
  colour overlay at 0.5 and a red Blend If ramp [0, 0, 60, 140]; the
  `export32` composite matches this file's f64 oracle of Photoshop's measured
  32-bit model (the effect colour decoded to linear light — through the
  fixture profile's own `curv` tables, since that profile is sRGB's space and
  its numbers are the authored ones — the W3C step in linear light, the
  stored red value unclamped on the ramp) within **T_REL**
  at every pixel the shadow or the overlay covers fully. `set_global_light`
  is accepted and reported; `save_copy` `.rz` and reopen: samples bit-exact
  and `get_document` reports the style and the light. An Outer Glow in Screen
  refuses with the core's sentence.
- **F16 the 32-bit adjustments measured against Photoshop.** On a
  fresh copy of the fixture each, `add_adjustment_layer` with Levels, Black &
  White, Channel Mixer, Photo Filter, `bcs`, Exposure, Curves and
  Hue/Saturation (Colorize, then master Lightness alone) — the core
  tests' oracle sets —
  and the `export32` composite matches this file's f64 oracle of the form
  fitted to Photoshop 26.10.0's 32-bit renders (Levels unclamped with a
  signed power; Black & White floored at 0; the mixer in 1/1024 steps with
  half its constant; Photo Filter's density multiply in the profile's native
  XYZ, its basis read from the fixture profile's own `rXYZ/gXYZ/bXYZ/wtpt/
  chad` tags and its colour decoded through the profile's `curv` tables;
  `bcs` unclamped; Exposure per stored channel `sgn(u)·|u|^(1/gamma)`,
  `u = v·2^e + o`; Curves as the natural cubic spline, continued along
  its end slope above the last point and below a first point at 0, flat
  below any other first point, channel before master, slice 3d;
  Hue/Saturation's Colorize as `C32`, `Y` the two-sided lightness of
  `(max + min)/2` held past its target and chroma
  `S·(1 − |2Y − 1|)·(1 − |a|)` unclamped, its Lightness per component with
  a component past the target held; Color Balance as take6 pinned it,
  per-channel Levels in whole units continued beyond 0 and 1 along the
  4096-stage end slopes) within
  **T_REL**, alpha untouched. `edit_adjustment_layer`
  to a second set re-renders to that set's oracle; `save_copy` `.rz` and
  reopen: samples bit-exact and `get_document` reports the params. A
  Hue/Saturation layer with master Hue -55 alone renders the hexcone
  rotation by a shift on the 1/24576-turn grid, truncated toward zero
  (take4), within **T_REL**; a Curves layer in map (pencil)
  form (take3's M3: `rgb_map` M1 over an `r_map` square root) renders each
  map as straight lines between its entries, its first and last segments
  continued beyond 0…255, channel before master (`float_adjust_take3.py`),
  within **T_REL** (`curves_map`); a Color Lookup layer loading a
  3D `.cube` this drive writes (take3's `cross_5`, `params: {file}`) renders
  Photoshop's device-link form (the table on the stored values, tetrahedral,
  an input above 1 read as 1 and one below 0 as 0; `float_adjust_take3.py`)
  within **T_REL**, `edit_adjustment_layer` to `strength` 0.5 keeps the
  table and re-renders to the lerp, and `save_copy` `.rz` and reopen are
  bit-exact (`color_lookup`); a 1D `.cube` is admitted
  (`color_lookup_1d_admitted`: the table on the stored values, here
  `0.5·clamp(v, 0, 1)` within **T_REL**); `op:hue_saturation` with a Reds
  band hue edited is admitted (the display-scaled form), finite and
  changes the composite.
- **F17 destructive Image ▸ Adjustments at 32 bits.** On a
  fresh copy of the fixture each, `apply_filter` with Levels (flat), Black &
  White, Channel Mixer, Photo Filter, Exposure, Curves and Color Balance
  (F16's first sets) and the
  `export32` composite matches F16's oracle within **T_REL**, alpha
  untouched, the reply reporting `rgba32_float`; `undo` restores the
  composite bit-exact. The same holds for `apply_filter` `curves` with F16's
  map-form set (`curves_map`) and for `apply_filter` `color_lookup`
  with F16's `.cube` (`color_lookup`), and for `apply_filter` `adjust`
  (`bcs`: destructive equals layer), flat, with F16's first `bcs` set
  (`adjust_legacy`, F16's oracle) and with `legacy: false` (`adjust_modern`:
  per stored channel inside [0, 1], `x^(2^(-1.5·b))` then the S-curve
  `x < 0.5 ? 0.5·(2x)^k : 1 − 0.5·(2(1−x))^k` with `k = 2^(1.5·c)`, a channel
  outside [0, 1] unchanged, then the luma-anchored saturation scale,
  unclamped). `apply_filter` `invert` refuses by name and changes nothing
  (Vibrance applies, see F46).
- **F18 a Pass Through group at 32 bits.** On a fresh copy of
  the fixture, two `duplicate_layer`s, the upper copy set to Multiply,
  `group_layers` over both (Pass Through) and
  `set_layer_properties opacity 0.6` on the group: the `export32`
  composite matches this file's f64 oracle of the form Photoshop 26.10.0 was
  measured to use (the children over a COPY of the backdrop, then that result
  interpolated back towards the backdrop by the group's opacity,
  premultiplied; `scripts/photoshop-probe/float_pass_through.py`) within
  **T_REL**. The group switched to Normal (isolated) and back to
  `blend_mode: "Pass Through"` is accepted and renders bit-exact as before;
  `save_copy` `.rz` and reopen: samples bit-exact and `get_document` reports
  the group's kind `group` and blend mode `Pass Through`.
- **F20 text and shape layers at 32 bits.** Photoshop 26.10.0's
  measured 32-bit type and shape colour (`scripts/photoshop-probe/
  float_artwork.py`, receipt `doctored-fixtures/photoshop/float-artwork-473/
  take2`): a stored colour number c IS linear light (H-raw), antialias alpha
  is the 16-bit coverage (C-same), and a layer pasted into or converted out
  of 32 bits has its numbers rewritten so it looks the same (PT/PSH, NR).
  The oracle is this file's: c/255 through the matrix `inv(fixture.to_pcs) ·
  srgb.to_pcs` (the fixture's colorant tags, and those of the sRGB profile a
  16-bit sRGB document exports), at **T_REL**; the IEC sRGB curve written
  out here. F = (46, 128, 200), S = (188, 64, 16). (a) `add_shape_layer`
  `fill_rgba` F/255 is admitted on a fresh fixture copy, its interior is the
  oracle, `get_document` reports `fill_rgba` F/255 exactly and `ink`
  `linear_srgb`, and a second shape with `fill` "#2E80C8FF" has the same
  interior; (b) with the backdrop hidden, a fractional box's alpha equals a
  16-bit twin's alpha/65535 within 1/65535 at every pixel; (c)
  `add_text_layer` and `edit_shape_layer` are admitted and reported,
  `transform_layer` ×2 Bilinear keeps `shape`, `save_copy` `.rz` and reopen
  is bit-exact with `shape`, and `rasterize_artwork` keeps the pixels
  bit-exact and drops `shape`; (d) a text layer at `rgba` S/255 and a shape
  at F/255 in a 16-bit sRGB document, each copied whole and pasted in place,
  land as `text` / `shape` with `ink` `linear_srgb`, their numbers
  sRGBdecode(c/255) within 1e-12 (255 × the fill 6.9679, 55.0442, 147.2818
  within 2e-3, PSH: Photoshop's curve is 1.2e-3 codes from the IEC one), their interiors the H-dec oracle within the u16 step,
  and again after an `edit_shape_layer` of `stroke_width` and an
  `edit_text_layer` of `size` at **T_REL**; (e) `convert_precision`
  `structure: "preserve"` to 16 bits keeps both with `ink` `srgb`, `fill_rgba`
  sRGBencode(F/255) within 1e-12 (255 × 117.7647, 187.8444, 229.1000 within
  2e-3, NR), and after a `stroke_width` edit the 16-bit interior is the float
  appearance within 1 code; (f) `fill_rgba` with a component above 1 refuses
  in-band; (g) in the 16-bit document of (d) both report `ink` `srgb` and
  still report `text` / `shape` after `save_copy` `.rz` and reopen.
- **F28 Move and Free Transform of a layer's selected pixels.** On a
  fresh copy of F1's `scene.tiff` with `select_rect` 4,4,16,8: (a)
  `move_selected_pixels` dx 9 dy 5, and the `export32` composite equals this
  file's f64 move oracle (the vacated pixels keep RGB at alpha a·(1−c), the
  float lands Normal source-over) within **T_REL**, alpha 0 where the oracle's
  is; (b) `undo` gives F1's samples bit-exact; (c)
  `transform_selected_pixels` rotate 90 sampler nearest (the box's centre,
  (12, 8); positive is clockwise) lands the permutation (destination (X, Y)
  reads source (Y + 4, 19 − X)) over the transparent hole, within **T_REL**;
  (d) `transform_again` with that selection succeeds; (e)
  `move_selected_pixels` `target` "red" is admitted (F30 checks its
  pixels).
- **F30 Move and Free Transform of one colour channel.**
  Photoshop 26.10.0 leaves the background swatch's 0.30 / 0.59 / 0.11 luma
  rounded to a code, over 255, in the vacated area, unconverted at 32 bits
  (receipts `channel-move-633/take2` and `plane-gray-431-633/take1`: `hole8`,
  `hole16` and `hole32` `3059r`, `form32` `raw`). On a fresh copy of F1's
  `scene.tiff` with `select_rect` 4,4,16,8 and `background` #0000ff (28 under
  that rule, 29 under the Rec. 601 floor it replaced; a grey cannot tell the
  two apart): (a) `move_selected_pixels`
  dx 9 dy 5 `target` "red" equals this file's f64 oracle (the hole toward
  28/255 and the landing source-over, each at the selection × the stored
  alpha) within **T_REL**, green, blue and alpha bit-exact; (b) `undo` is
  bit-exact; (c) `transform_selected_pixels` rotate 90 nearest `target` "red"
  lands the quarter-turn permutation by the same oracle; (d) after
  `convert_precision` to 16 bits the same red move leaves 7196 in the hole
  and copies the source's red code, within **T_CODE16**.
- **F32 Fill and Stroke on one colour channel, and above-white ink.**
  Photoshop 26.10.0 writes 0.30 r + 0.59 g + 0.11 b of the swatch's
  components into the one targeted channel, unconverted at 32 bits, whatever
  the pixel's transparency, and the linear numbers' same sum for a float
  foreground (receipt `plane-fill-431/take1`: `3059x` / `raw`, SOFT
  `written`, HDR `lin`); a float foreground fills a layer as its own numbers
  (`hdr-colour-317-608/take2`). On a fresh copy of F1's `scene.tiff`, each
  against this file's f64 oracle: (a) `select_rect` 4,4,16,8 then
  `fill_selection` `target` "red" `color` #dc1eaa `color_space` "document":
  Red inside reads (0.30·220 + 0.59·30 + 0.11·170)/255 within **T_REL**,
  every other sample is bit-exact and the reply's `storage` is
  "rgba32_float"; (b) `undo` is bit-exact, then the same fill over 4,20,16,8,
  whose rows 24–27 are the fixture's half-alpha band: every row reads the
  same value (the `written` row), and `undo` is bit-exact; (c) `target`
  "green" Multiply at opacity 0.5 reads g·(1 + (V − 1)/2); (d)
  `stroke_selection` `target` "blue" width 2 inside: the rect's 2-pixel ring
  reads V, and pixels inset 3 or more and outside are bit-exact; (e) a layer
  `fill_selection` with `rgba32` [4, 0.5, 0.25, 1] reads exactly those
  numbers inside the rect, `undo` bit-exact; (f) `stroke_selection` with that
  `rgba32` writes them on the same ring; (g) `rgba32` with `color`, `rgba32`
  [70000, 0, 0, 1], and `rgba32` after `convert_precision` to 16 bits each
  refuse in band by name; (h) `rgba32` with `target` "red" reads
  0.30·4 + 0.59·0.5 + 0.11·0.25 = 1.5225 (the H-LIN row). At 16 and 8 bits
  the same channel takes the same luma, unrounded at 16 and rounded to a code
  at 8 (receipt `plane-gray-431-633/take1`: `fill16` `3059x`, `fill8`
  `3059r`, both strokes `band`), after (g)'s conversion to 16 bits, over the
  opaque rect 4,4,16,8 and read from 16- and 8-bit PNG exports: (i) the (a)
  fill answers `changed` true, Red inside reads round(65535·V) = 26317 within
  **T_CODE16** (the Rec. 709 gray it replaced is 20689) and every other
  sample equals the export taken before the fill; (i-undo) `undo` gives that
  export back exactly, so (j) cannot pass on (i)'s value converted (26317
  narrows to 102 too); (j) after `convert_precision` to 8 bits the same fill
  answers `changed` true, Red inside reads code 102 exactly (Rec. 709: 80)
  and every other sample equals the 8-bit export taken before it.
- **F34 RAW Develop edits of a placed source at 32 bits.**
  Photoshop 26.10.0 places a camera RAW file in a 32-bit document as its
  SDR development: the 32-bit rows read the 16-bit rows' sRGB decode on
  every compared sample and never pass luminance 1.002 (receipt
  `float-raw-602/take2`, `scripts/photoshop-probe/float_raw.py`: R-SDR by
  the controller's ruling). The camera file is `phase3_fixtures.synthetic_dng`
  (512 × 384, orientation 6) opened with `raw: {exposure: 0.5}` at 16 bits:
  the TWIN, of canvas 384 × 512. The float target is this gate's own, a
  transparent 384 × 512 float TIFF in the fixture's profile, and the twin's
  layer reaches it by `copy` / `paste` `mode: 'layers'`, `placement:
  'in_place'`, asserted at (0, 0) covering the canvas. Every float export
  is compared over the whole canvas with the twin's own 16-bit PNG export,
  each code's sRGB decode taken through `inv(fixture colorants) · the
  export's colorants`, within **2e-4**; the
  twin's export must be opaque and not all zero. `pasted`: `get_raw_source`
  keeps exposure 0.5 and `captured_implementation_available`,
  `get_image_source` `fallback_depth` 32 and `master_depth` 16.
  `redeveloped`: `redevelop_raw` exposure 1.0 in both documents is one
  history state, matches the twin's new export and differs from before; the
  same call again adds no state. `undo` is bit-exact at exposure 0.5 and
  `redo` restores. `settings`: a recorded `apply_raw_preset` is one
  `apply_raw_settings` step that `run_action` replays after `undo`;
  `sync_raw_settings` float → twin and twin → float each change one layer.
  `inspected`: a patched `inspect_raw_development` answers two images,
  `committed` false, the revision and history unchanged. `locals`: one
  `create` radial (`scripts/raw-local-probe/mcp.py`'s shape) with an
  exposure offset in both documents lists, has the twin's mask SHA-256 and
  matches the twin's export; after the twin's local takes another offset,
  `copy_raw_local_settings` answers `unchanged` false and matches again.
  `transformed`: after a 150 % Bilinear scale, `redevelop_raw` and
  `edit_raw_development` each add a state. `migrated`: the twin's `.rz`
  rewritten by `scripts/raw-probe/native.py` `incompatible_decoder`,
  reopened and pasted in place into a fresh target, reads `unavailable`,
  refuses `redevelop_raw` in band with the export unchanged, gives a
  `preview_raw_decoder_migration` review and migrates in one state; `undo`
  is bit-exact. `reopened`: `save_copy` `.rz` reopens bit-exact with the
  same `raw` and the local's mask SHA-256.
- **F35 Black & White with Tint at 32 bits.** Photoshop 26.10.0 sets
  the tint colour's NUMBERS (over 255) to the grey's luminosity with
  ClipColor, the grey `max(0, gray)` capped at 16 and ClipColor's ceiling
  at 16, plus 1/510 on every component when either branch fired (receipt
  `float-adjust-473/take6`, `scripts/photoshop-probe/float_adjust_take6.py`:
  BT32-low `clip/e510/num` 0.003 tolerance units; the cap is take6's fit,
  take7 stalled). `tint_layer`: on a fresh copy of the fixture,
  `add_adjustment_layer` `black_and_white` with take6's B3 weights (reds
  1.2, blues -0.5) and B2's tint `#2878dc` matches this file's f64 oracle
  within **T_REL**, alpha untouched; `edit_adjustment_layer` to `#998a66`
  re-renders to that colour's oracle; `save_copy` `.rz` and reopen are
  bit-exact and `get_document` reports the params. `tint_apply`: on a fresh
  copy, `apply_filter` `black_and_white` with the first params matches the
  same oracle within **T_REL**, and `undo` is bit-exact.
- **F36 a camera RAW file placed at every depth.** Photoshop places a
  camera RAW file as a smart object of its development at the file's print
  size (`float-raw-602` takes 1–2: 300 ppi at 100 %, R-SDR at 32 bits, the
  `.xmp` sidecar's exposure honoured; `float-host-602/take2`: a 72 ppi file
  at 24 %). The camera file is F34's `synthetic_dng` (`f36.dng`, developed
  384 × 512); the TWIN is `open_document` of it with `raw: {exposure: 0.5}`,
  `precision: preserve`, and P is the twin's reported `ppi_x` (the open sets
  it from the file's own resolution, as the place reads it); every target
  is made at P. `placed32`: `place_image` `raw: {exposure: 0.5,
  output_encoding: sdr16}` (Photoshop's R-SDR, named because a float
  target's default is the float development) into a
  transparent 384 × 512 float TIFF at P in the fixture's profile is one
  history state, the layer at (0, 0, 384, 512), `get_raw_source` exposure
  0.5, `captured_implementation_available` and `original_filename`
  `f36.dng`, `get_image_source` depths 32 / 16 and no link; the float
  export is within **2e-4** of the twin's 16-bit PNG export in F34's form.
  `placed16` / `placed8`: `new_document` 16- / 8-bit sRGB transparent at
  P; the PNG exports are within **T_CODE16** of the twin's codes / within
  **T_CODE8** of `round(code · 255 / 65535)`. `sidecar`: `f36s.dng` beside
  `f36s.xmp` (`float_host_raw.py`'s SIDECAR text at `Exposure2012="+1.00"`)
  places with `raw: {output_encoding: sdr16}` alone at exposure 1.0 with no
  `disclosures`, within 2e-4 of
  a twin opened from a sidecar-free copy at `raw: {exposure: 1.0}`; and
  `open_document` of `f36s.dng` reads exposure 1.0. `scaled`: a float target
  at P / 2 lands the layer at (0, 0, 192, 256) with `transform` [0.5, 0, 0,
  0.5, 0, 0] and, the encoding unnamed, depths 32 / 32, and at
  `x: 10, y: 20` at (10, 20, 192, 256) with [0.5, 0, 0,
  0.5, 10, 20]. `linked`: `storage: linked` keeps `link.path` with
  `link_status` `current`. `redeveloped`: on a second linked place,
  `redevelop_raw` exposure 1 and an `edit_raw_development` `create` (F34's
  radial) each add one state and leave `link.path`, `link.sha256` and
  `link_status` `current` unchanged. `refused`: `raw: {exposure:
  9}` and `raw` with a PNG refuse in band, history unchanged. `undo` gives
  the empty target back bit-exact and `redo` the place. `reopened`:
  `save_copy` `.rz` reopens bit-exact with the same `raw`.
- **F37 the Brush, Eraser and Gradient at 32-bit float.** On a fresh
  copy of the fixture, each row read through a 32-bit export against an
  oracle written in this file, within **T_REL** where a value is computed
  and bit for bit everywhere a row leaves alone. A stroke is checked in its
  CORE (pixel centres within radius − 2 of the stroked segment, where a hard
  tip's coverage is whole) and FAR from it (radius + 2 and beyond, bit for
  bit); the ring between is the tip's edge and is not read.
  (a) `brush_stroke` with a `document` hex, hardness 100, opacity 1: the core
  reads the ink decoded through the fixture profile's own `curv` tables at
  alpha 1, the reply's storage is `rgba32_float`, and `undo` is bit-exact.
  (b) `rgba32` (4, 0.5, 0.25, 1) reads those numbers, and at opacity 0.5 over
  the 8.0 block its red reads 6.0 (no clamp). (c) `eraser_stroke` at opacity
  0.5 halves alpha and leaves RGB bit-exact. (d) `gradient` `start_rgba32`
  black to `end_rgba32` (8, 8, 8, 1) across the canvas width reads `8t` in
  column `x`, `t = x / 47`, and `document` hex black to white reads `t`
  (the ramp interpolates in linear light). (e) `add_layer_mask`, then
  `eraser_stroke target: mask`, zeroes the exported alpha in the stroke's
  core with reply storage `u16`. (f) the refusals, each by name (`fill` and
  `dodge_burn` are admitted, see F46): `brush_stroke blend_mode: Screen`, `target: red`, `rgba32`
  beside `color`, `start_rgba32` alone, and the coverage-ink sentence from
  `brush_stroke rgba32 target: mask`, from `brush_stroke rgba32` on a fresh
  Exposure adjustment layer with a mask (no `target`; the export is bit-exact
  with before the call, so its mask was not painted) and from `gradient
  start_rgba32 + end_rgba32 target: mask`. (g) `background: true` returns a
  `job_id` that `job_status` sees `completed`, with the stroke applied.
  (h) `save_copy` `.rz` starts 46, 40, 39 and reopens bit-exact. (i) with a
  fresh reveal-all mask, `gradient target: mask` from `#000000` to `#ffffff`
  across the canvas width: the exported alpha reads `alpha ×
  round(65535 t) / 65535` within 2/65535, reply storage `u16`, and one
  `undo` restores the export bit-exact. (j) `select_rect`, `save_selection
  name: F37`, `deselect`, then `brush_stroke target: "channel:F37"` (hardness
  100, opacity 1) outside the saved rectangle: `render channel: F37` reads 255
  in the stroke's core and in the saved rectangle and 0 far from both, the
  32-bit export is bit-exact with before (a channel is not document pixels),
  the reply says storage `u16` and target `channel:F37`, and one `undo`
  restores the render's sha256.
- **F38 OpenEXR, Radiance and the view exposure.** Three
  fixtures written by this file: an uncompressed f32 RGBA OpenEXR (12 × 6:
  an opaque block, a block at alpha 0.3 and a block at alpha 0 whose stored
  colour is not zero, the samples ASSOCIATED as the format stores them), a
  half RGB OpenEXR with Display P3 chromaticities, and a flat RGBE `.hdr`.
  `open_*`: each opens as `rgba32_float` with the authored values, read back
  through `export_image`'s 32-bit TIFF and `read_float_tiff` — the f32 file
  divided by its alpha (the canonical zero at alpha 0), the half file through
  `struct 'e'`, the RGBE file as `m · 2^(e − 136)`. `roundtrip`: the f32
  document exported to f32 OpenEXR, read by `f38_read_exr` (its own ZIP
  predictor and interleave), is bit-exact where alpha is 1, has alpha
  bit-exact and RGB within **2 ulp** (the distance between bit patterns)
  where alpha is 0.3, and RGB 0 where alpha is 0; it declares the Rec.709
  chromaticities and carries nothing. `half`: `float_samples: f16` to OpenEXR
  and to TIFF equals `struct 'e'` of each written sample. `radiance_out`:
  the `.hdr` export is within one RGBE mantissa step of `max(rgb, 0) · alpha`
  and carries `PRIMARIES=`. `save_copy`: `.exr` writes `export_image`'s
  bytes. `render_exposure`: `render exposure:-3` and `:2` are within
  **T_CODE8** of gain, clip, the sRGB function and round; `exposure: 0` is
  the plain render. `view_exposure`: a fresh float window's `get_view` reads
  `exposure: 0, exposure_displayed: false`; `set_view exposure:-3` replies
  `exposure: -3` and `get_view`, polled for at most 10 s, reports
  `exposure_displayed: true`; `set_view exposure:0` replies
  `exposure_displayed: false`; `get_document` and `unsaved_changes` are the
  same before, during and after, and a `render` with no argument is still the
  plain one. `refusals`, each by name: `precision: rgba8` on the `.exr`,
  `exr` from the 16-bit document, `exr` at `bit_depth` 16, `hdr` with `f16`,
  `render` and `set_view` `exposure` on the 16-bit document, `exposure: 25`
  on both, and `render` `exposure` with `layer`.
- **F39 eleven filters on a float layer.** On a fresh copy of
  the fixture, read through 32-bit exports. `get_precision` lists the eleven
  among its `filters` (F45 holds the whole list). Each of the ten `apply_filter` filters (`blur`,
  `box_blur`, `motion_blur`, `radial_blur`, `lens_blur`, `tilt_shift`,
  `offset`, `lens_correction`, `unsharp_mask`, `high_pass`) replies `changed:
  true` and `storage: rgba32_float`, leaves every sample finite, and one
  `undo` restores the export bit for bit; `liquify` with one full-strength
  push does the same. Two have oracles written here: `offset` (7, −5, wrap)
  equals the index map `(x − 7 mod W, y + 5 mod H)` bit for bit, and
  `box_blur` radius 1 equals the premultiplied 3 × 3 mean with clamped edges
  within **T_REL**. `above_white`: after `blur` sigma 2 the 8.0 block still
  holds a red above 1. `selection`: with a rectangle selected `blur` replies
  `write_scope: selection`, changes samples inside it and leaves every sample
  outside it bit-identical. `refusals`: `blur` with `target: mask` refuses
  naming the target, and the export is bit-identical afterwards.
- **F43 Vision selections on a float document.** The picture is
  `F43_PORTRAIT` (512 × 512; an absent file FAILS) decoded to linear light by
  IEC 61966-2.1 written out here, with `F43_HOT` (the suit's chest) scaled by
  **F43_GAIN** 4 so part of the subject is above white; the float document is
  that picture through `write_float_tiff`, and its 16-bit TWIN is
  `round(65535 · srgb_encode(v))` of the same samples (the encode's clamp is
  the Clip tone map), so the twin is the float picture's own display computed
  here. Coverage is `get_selection`'s `coverage16` on a stride-**F43_STRIDE**
  lattice; "covers" is at least **F43_INSIDE** (half), "clear" at most
  **F43_OUTSIDE** (1 %), and two selections agree when the IoU of their
  lattice points at or above F43_INSIDE is at least **F43_IOU** 0.95. Vision's
  output differs between macOS versions, so no mask byte or hash is asserted.
  `fixture`: at least **F43_HOT_MIN** colour samples above 1.0 and
  `sample_pixel` reads a linear red above 1.0 at `F43_CHEST`. `tools`:
  `get_precision` lists `select_subject`, `inspect_subjects`,
  `select_subjects`, `select_people` and `object_select`. `subject`:
  `select_subject` finds an instance, covers `F43_FACE` and `F43_CHEST`, is
  clear at each `F43_BACKGROUND` point, adds one history step, and at least
  **F43_MIN_CLASS** lattice points read 0 and as many 65535. `twin`: it
  agrees with the twin's `select_subject`. `confines_filter`,
  `confines_paint`, `confines_retouch`: under that selection `apply_filter
  blur` sigma 3 (replying `write_scope: selection`), `brush_stroke rgba32 [4,
  0.5, 0.25, 1]` and `clone_stamp` along row 40 leave every lattice pixel
  with coverage 0 bit-identical between two 32-bit exports, change at least
  one with coverage 65535, and one `undo` is bit-exact. `mask`:
  `add_layer_mask kind: from_selection`, `deselect` and `load_selection from:
  layer_mask` give the lattice back within **T_COV16**. `exposure`: with
  `set_view exposure: -6` displayed, `select_subject` agrees with `subject`.
  `people`: `select_people` covers the face, is clear at
  `F43_PEOPLE_CORNERS` and agrees with the twin's. `objects`:
  `inspect_subjects` lists an instance whose bounds hold the face,
  `select_subjects` with it covers the face, and `object_select` over those
  bounds with `refine` omitted replies `refined: false`, names that instance
  and reads 0 at every lattice point outside the region; on the twin the same
  call replies `refined: true`, and with `refine: false` it agrees with the
  float result. `refine_admitted`: `object_select refine: true` on the float
  document runs the edge stage on the display codes (rule E): it
  replies `refined: true` and adds one history step. `reinhard`: under
  `set_tone_map reinhard white 4` `select_subject` still covers the face and
  is clear at the background points.
- **F40 retouching and Stroke Path at 32-bit float.** The target is
  this gate's own: 240 × 160, opaque, red a ramp from 0 to 8 across the width
  (`8 x / 239`), green 0.5, blue 0.25, the same in every row, written by
  `write_float_tiff` in the fixture's profile. Every row opens a fresh copy
  and reads a 32-bit export. A stroke's or a rectangle's CORE is judged and
  everything FAR from it must be bit for bit (F37's regions). Tolerances,
  relative and floored at 1.0: **T_REL** for the clone and the stroked path,
  **F40_HEAL** 2 % for `heal_stroke` and `patch_region`, **F40_SYNTH** 5 %
  for spot heal, Content-Aware Fill, Remove and a moved blob, **F40_VACATED**
  10 % for the place a move leaves.
  (a) `clone_stamp` (hardness 100, opacity 1) from 120 px to the right: the
  core reads the source column's values, all above white; the reply's storage
  is `rgba32_float`; `undo` is bit-exact. (b) opacity 0.5 reads the mean of
  the two columns; `blend_mode: Multiply` reads their product, unclamped
  (above 8); `Screen` refuses by name and changes nothing; `output:
  new_layer` adds a layer and exports as (a). (c) a dark `brush_stroke
  rgba32` blob, then `heal_stroke` from 40 rows up: the core reads the scene
  within F40_HEAL, above white wherever the scene is. (d) `spot_heal_stroke`
  over the blob reads the scene within F40_SYNTH, and after `undo` the same
  seed gives the same export bit for bit. (e) `select_rect` around the blob
  and `content_aware_fill`: the selected pixels read the scene within
  F40_SYNTH and the rest is bit-exact; after `undo`,
  `preview_content_aware_fill` changes nothing and `content_aware_fill
  preview_id` gives the direct call's export bit for bit. (f) `remove_stroke
  background: true` returns a `job_id` that `job_status` sees `completed`,
  and the core reads the scene within F40_SYNTH. (g) `patch_region` over a
  square around the blob with offset (0, −40) reads the scene within
  F40_HEAL. (h) a (20, 10, 5) blob and `content_aware_move` by (0, −50): the
  blob's core at its new place reads (20, 10, 5) within F40_SYNTH and at its
  old place the scene within F40_VACATED; `undo` is bit-exact; `mode:
  extend` leaves the blob at both places. (i) `set_path` and `stroke_path
  rgba32: [4, 0.5, 0.25, 1]` read the ink in the line's core; one `undo` is
  bit-exact; `tool: eraser` at opacity 0.5 halves alpha there with RGB
  bit-exact, and at opacity 1 zeroes it (a composite export carries no colour
  under alpha 0, so RGB is not read in that row). (j) the
  refusals, each by name, the export bit-exact afterwards (`dodge_burn`,
  `fill` and `sponge_stroke` are admitted, see F46; `red_eye` is too, see
  F45): `clone_stamp target: mask`; `eraser_stroke rgba32`, `brush_stroke
  start_rgba32` and `gradient rgba32` (each "takes no" that argument);
  `gradient start_rgba32 + end_rgba32 target: quick_mask` (the depth
  sentence, naming `start_rgba32` and `start_color and end_color`); and
  `heal_stroke` on an Exposure adjustment layer. (k) a cloned and
  spot-healed document's `save_copy` `.rz` reopens bit-exact.
- **F42 the Blur, Sharpen, Smudge and History brushes at 32-bit float.**
  F40's builder (240 × 160, a fresh copy and a 32-bit export per row) on
  two targets: F40's ramp, and this gate's EDGE (red 0.1 left of
  column 120 and 8 from it, green 0.5, blue 0.25, opaque). Regions are F37's:
  a hard stroke's CORE is judged and everything FAR from it must be bit for
  bit. Bounds, fixed here before the run: **F42_BLUR** — every colour sample
  of a blurred core pixel is within 2 % of the reference's magnitude;
  **F42_SHARPEN** — within 2 % of `max(|reference|, 0.1)` (the result crosses
  zero); **T_REL** for the History Brush's mix and the smudged centre. The
  reference is written here in binary64: the Gaussian of the export read
  before the stroke, summed out to `ceil(8σ)` with clamped edges (the layer
  is opaque, so alpha has no weight), σ 2 for Blur and
  `rgb + (rgb − Gaussian at σ 1)` for Sharpen, with no clamp.
  (a) a `brush_stroke rgba32: [5000, 5000, 5000, 1]` blob on the ramp, then
  `blur_stroke strength: 100` (hardness 100, size 40) across it: the core is
  within F42_BLUR, a core pixel that was off the blob reads a green above 1
  (the highlight spread), alpha and everything far are bit-exact, and one
  `undo` restores the blobbed export bit for bit. (b) on the edge,
  `sharpen_stroke strength: 100` across it: the core is within F42_SHARPEN and
  a core pixel on the dark side reads a red below zero. (c) on the edge, a
  hard `smudge_stroke strength: 100` from (150.5, 80.5), on the 8 side, 40 px
  left: pixel (110, 80), 0.1 before, reads the start centre's 8.0 within
  T_REL, above 1. (d) on the ramp, `list_history`'s `current_state_id`, a
  bright `brush_stroke`, then `history_brush_stroke opacity: 100 state_id:` that
  id: the core equals the ORIGINAL scene bit for bit and everything far the
  painted export; after `undo`, `opacity: 50` reads the mean of the two
  within T_REL. (e) the refusals, each by name, the export bit-exact
  afterwards (`sponge_stroke` is admitted, see F46): `blur_stroke target: mask`
  (layer RGB only); `smudge_stroke` in Quick Mask; `sharpen_stroke
  blend_mode: Multiply` ("has no blend_mode"); and `blur_stroke` on an
  Exposure adjustment layer. (f) `blur_stroke background: true` returns a
  `job_id` that `job_status` sees `completed` with `storage: rgba32_float`;
  the synchronous replies of (a), (b), (c) and (d) carry `storage:
  rgba32_float` and `state: completed`; and a blurred, smudged document's
  `save_copy` `.rz` reopens bit-exact.
- **F44 New Document at 32 bits and the conversion INTO float.** MCP
  only. The oracle is the sRGB EOTF (IEC 61966-2.1) written out here,
  `srgb_decode`, applied to `code / 65535`; **F44_TOL** 1e-5 absolute (the
  built-in TRC is a 1024-entry 16-bit `curv`). (a) `new_document depth: 32`
  sRGB white: `precision` is `rgba32_float` and every export32 sample is
  exactly 1.0. (b) background `#808080`: RGB within F44_TOL of
  `srgb_decode(32896 / 65535)`, alpha exactly 1.0. (c) transparent: every
  sample exactly 0 and the one layer is named "Layer 1". (d) `profile:
  display_p3` white: every sample exactly 1.0. (e) the drive writes, with the
  standard library, an uncompressed 256 × 256 RGBA16 TIFF with no profile:
  pixel `i` is (i, 65535 − i, 7·i mod 65536, 65535), so R, G and B each hold
  every code; it opens with `precision` `rgba16`, `get_metadata`'s profile
  naming sRGB, and a 16-bit PNG export equal to the authored codes. (f)
  `convert_precision mode: rgba32 structure: preserve`: the plan's direction
  is `rgba16_to_rgba32_float` with no blocked row, `precision` becomes
  `rgba32_float`, `undo` gives `rgba16` and `redo` `rgba32_float`. (g) its
  export32 is within F44_TOL of `srgb_decode` per sample, alpha exactly 1.0.
  (h) `save_copy` `.rz`, reopened: `rgba32_float`, export32 bit-exact with
  (g). (i) `convert_precision rgba16 preserve` on it: the 16-bit export equals
  the authored codes. (j) an 8-bit RGBA PNG of every byte code in R, G and B
  (16 × 16, opaque) → `rgba32` preserve → `rgba8` preserve `dither: none`:
  the 8-bit export equals the authored bytes. (k) the TIFF with its layer
  duplicated and the copy set to Screen: `rgba32` preserve refuses naming the
  copy and "screen", `dry_run` lists it in `blocked` (index 1), and the
  document stays `rgba16`. (l) `rgba32` with `flatten: true` refuses with
  "A conversion to 32-bit float keeps every layer", and `rgba32` on a float
  document with "already uses the requested precision".
- **F45 the display-referred filters, Presence, Develop Filter and Red
  Eye on a float layer.** On a fresh copy of the fixture,
  read through 32-bit exports. Bounds, fixed here before the run: **T_REL**;
  **F45_DECODE** 1e-4 (absolute, one decoded display value); **F45_KEPT**
  1e-3 (relative, a value a filter's window never leaves). `listed`:
  `get_precision`'s `filters` are exactly the twenty-four. Each of the
  thirteen `apply_filter` filters (`surface_blur`, `dust_and_scratches`,
  `reduce_noise`, `add_noise`, `add_grain`, `clouds`, `difference_clouds`,
  `emboss`, `edge_detect`, `pixelate`, `sharpen`, `presence`, `develop_filter`)
  replies `changed: true` and `storage: rgba32_float`, leaves every sample
  finite, and one `undo` restores the export bit for bit. Oracles written
  here: `pixelate` block 4 is the premultiplied cell mean within T_REL;
  `add_noise` amount 0.2 leaves every zero sample zero and moves every
  other by `|out/base − 1| ≤ 0.2`; `develop_filter` `{exposure: 1}` is twice the
  base bit for bit; `emboss` at (5, 3), inside the flat (8.0, 4.0, 2.0)
  block: the kernel's weights sum to 1, so a flat region embosses to its
  encoded luma plus 0.5, which here passes every channel's encoded maximum,
  and the finish holds each channel there, the block's own value, within
  F45_DECODE relative (a flat BLACK region reading sRGB EOTF(0.5) = 0.214041
  is `core/tests/float_filters_display.rs`'s). The 8.0
  block's (2, 2) red stays above 1.0 after `surface_blur`,
  `dust_and_scratches`, `reduce_noise`, `sharpen` and `presence`, and is 8.0
  within F45_KEPT after the first two and `sharpen` (their windows lie
  inside the block). `selection`: `surface_blur` under `F39_RECT` changes
  samples inside it and leaves every sample outside bit-identical.
  `runs`: `presence` `dehaze` 30, `develop_filter` `highlights` 30 and
  `develop_filter` `vibrance` 30 each reply `changed: true` and `storage:
  rgba32_float`, leave every sample finite, and one `undo` restores the
  export bit for bit (F47 holds their rules). `refusals`:
  `surface_blur` with `target: mask` refuses naming the target, and the
  export is bit-identical afterwards; `get_precision`'s `operators` no
  longer hold the twelve-filter row.
  `red_eye`: a #d01818 disc of radius 3 filled at (6, 16), then `red_eye`
  over (1, 11, 11, 11): samples change inside the rect only, the reply's
  storage is `rgba32_float`, and one `undo` restores the filled export bit
  for bit.
- **F46 the display-defined tools, blend modes and adjustments at 32 bits.**
  MCP only; fixed here before the run. The F46 picture is a 48 × 32
  RGBA f32 TIFF this drive writes with the fixture's profile: six 8-px-wide
  grey bands, left to right 0.25, 0.999, 1, 2, 8 and −0.25, alpha 1 (band
  centres x = 4, 12, …, 44; the seed is (36, 16), the 8.0 band). E is the
  profile's forward curve searched over its own `curv` tables
  (`encode_through`) and D the table itself (`curve_eval`), the rule the
  core states (`display_encoding`). Tolerances: **T_REL** for every float
  sample, 1/65535 (+1e-6) for an alpha read off a u16 mask, and coverage
  codes exact. (a) `fill` at the seed, tolerance 0, `rgba32` [4, 4, 4, 1]:
  the 1, 2 and 8 bands read exactly 4.0 (contiguous: the display copy calls
  them all white), 0.25, 0.999 and −0.25 stay bit for bit, reply storage
  `rgba32_float`. (b) `add_layer_mask`, then `fill target: mask` with
  document `#808080` at the seed: the export's alpha is 32896/65535
  everywhere, its RGB the bands' within T_REL (the composite divides the
  partial alpha back out), and the reply's storage `u16`. (c) `fill sample_all_layers:
  true` with (a)'s arguments: (a)'s result, reply storage `rgba32_float`.
  (d) `select_rect` (0, 0, 24, 32), then `fill contiguous: false rgba32`
  [4, 4, 4, 1]: the selection reads 4.0 exactly and the rest is bit for
  bit. (e) `dodge_burn` along y = 16.5 across the canvas, size 24, hardness
  100: midtones dodge at 50, highlights burn at 80 and shadows dodge at 60,
  each against this file's R4 (v in 0…1: D(shaped(E(v))); above 1:
  v·D(shaped(1)); below 0: v + D(shaped(0))) in the stroke's core, the rest
  bit for bit. (f) (b)'s mask, `dodge_burn target: mask` midtones dodge at
  50: the alpha in the core is round(65535·shaped(s/65535))/65535, s =
  32896, RGB the bands' within T_REL, the rest bit for bit with (b), reply
  storage `u16`. (g) on a fixture copy,
  `sponge_stroke mode: saturate strength: 100` along y = 4.5, size 16:
  the core reads Y + 2·(rgb − Y) (Rec. 709 Y over the linear values),
  unclamped, held to ±65504. (h) `select_magic_wand` at the seed, tolerance
  0, (i) `select_color_range range: highlights`, (j) `quick_select` from the
  seed, tolerance 0, edge 0, (k) `select_rect` (34, 14, 4, 4) then
  `select_similar tolerance: 0`: `get_selection` at the six band centres on
  y = 16 reads [0, 0, 65535, 65535, 65535, 0] for (h), (j) and (k) and
  [0, ≥ 65534 × 4, 0] for (i). (l) F14's shifted copy of the fixture in
  Linear Burn, Linear Light, Pin Light and Divide against this file's W3C
  oracle (unclamped; Divide `clamp(cb / max(cs, 1/65504), ±65504)`) within
  T_REL. (m) Hue Rotate 30, Sepia and Grayscale adjustment layers on a
  fixture copy against their closed forms (the W3C hue matrix, the sepia
  matrix, Rec. 709 luma; no clamp) within T_REL; and on an 8 × 3 picture
  whose row 0 holds colours of maximum 1 and rows 1 and 2 the same times 2
  and 8, Vibrance, Selective Color, Replace Color and Hue/Saturation
  saturation layers read out(k·p) = k·out(p) within T_REL. (n) `apply_filter
  threshold level: 0.5` and `apply_filter white_balance` (temperature 5000,
  document_v2) on fixture copies equal their adjustment layers bit for bit,
  reply storage `rgba32_float`. (o) `quick_select enhance_edge: true` from
  the seed, tolerance 0, edge 0, runs (rule E: the edge stage reads the
  display codes): the 8.0 band's centre reads at least 32768 and the 0.25
  band's 0; then refusals by name: `set_layer_properties blend_mode: Screen`, `fill
  rgba32 target: mask`, `dodge_burn` on an Exposure adjustment layer,
  `sponge_stroke target: mask`, `add_adjustment_layer op: invert` and `op:
  shadows_highlights`. (p) every pixel edit of (a)–(g) and (n) undoes in one
  `undo`, bit for bit.
- **F47 Select and Mask, Enhance Edge, the Magnetic Lasso, Dehaze and the
  Develop Filter's Highlights, Shadows, Vibrance and Saturation at 32
  bits.** MCP only; fixed here before the run. The SQUARE picture
  is a 64 × 48 RGBA f32 TIFF this drive writes with the system sRGB profile:
  black, an 8.0 grey square at (16, 12, 32, 24) and a 2.0 inset at (24, 18,
  16, 12); its TWIN is a 16-bit PNG holding the square's R1 codes (65535 on
  the whole square, 0 elsewhere): a value above white compares as white, so
  the inset vanishes. Coverage is `get_selection`'s `coverage16` at every
  pixel. `magnetic`: `select_magnetic` along `F47_TRACE` gives the twin's
  outline points and coverage exactly (rule M: the trace image is R1 at 8
  bits). `enhance_edge`: `quick_select enhance_edge: true` from (20, 16),
  tolerance 0, edge 0, gives the twin's coverage exactly. `refine_selection`:
  over `select_rect` `F47_RECT`, `edge_radius 4`, `smart_radius` and an `add`
  brush stroke give the twin's coverage exactly (rule E), unlike the rect's,
  and reply `coverage_storage: u16`; `output: mask` loads back within
  **T_COV16**; `new_layer` and `new_layer_with_mask` add one layer, keep
  `precision: rgba32_float`, read (8, 8, 8, 1) and (2, 2, 2, 1) bit for bit
  where the refined coverage is full and alpha 0 at (0, 0); `decontaminate:
  true` refuses by its operator row and adds no history step. The FILTER
  picture is 96 × 48: on top six 16-px grey bands 0.25, 0.999, 1, 2, 8 and
  −0.25, below a dark colour ramp (green 0.02, so its left end is a shadow)
  whose red passes 1. `apply_filter` `presence
  dehaze` ±40 and `develop_filter` `highlights` 40, `shadows` 40, `dehaze` 40,
  `vibrance` 40 and `saturation` −40, each alone: `changed: true`, `storage:
  rgba32_float`, every sample finite, one `undo` bit for bit; a positive
  Dehaze leaves every component above 1 bit for bit and a negative one moves
  none above `max(input, 1)` (rule D); Highlights and Shadows leave the 2.0
  and 8.0 bands bit for bit (rule H); Vibrance and Saturation leave them
  within T_REL (rule V: a grey is the master's fixed point, scaled). `get_precision`'s
  `operators.refused` names are exactly `F47_REFUSED`, nine rows, and the reasons of the
  blend, Invert, Shadows/Highlights and legacy White Balance rows each say
  "above 1.0". `auto_refuses`: `auto_tone` on the filter picture fails with
  text holding the automatic tone row's reason; `levels_auto_refuses`:
  `add_adjustment_layer op: levels, auto: {mode: tone}` fails with "automatic
  tone correction"; the samples are bit for bit after both. `object_select
  refine: true` is F43's `refine_admitted`.

`--gates F28[,F30...]` runs a subset, in the order above, with each named
gate's prerequisites added (F2 needs F1, F3 needs F2, F4 and F5 need F3, F6 and
F7 need F5); the last line then reads `SELECTED GATES PASS: ...` instead of
`ALL GATES PASS`. `--output` is resolved to an absolute path before any gate
runs, because the app answers absolute paths and a gate compares them with the
ones it sent.

Nothing here derives an expectation from Doctored: the fixture is written by
this file, the colour arithmetic is written out below, the TIFF and PNG
readers are this file's, and the digests come from `hashlib`.
"""
import argparse
import base64
import hashlib
import json
import math
import struct
import subprocess
import sys
import time
import zlib
from pathlib import Path

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

import phase5_support
from phase5_support import App
from verify_phase11e_linear import chunk, png16, read_png

# ---- the tolerances, fixed here BEFORE anything is measured ----------------
T_REL = 1e-5          # F2: relative agreement with the f64 composite oracle
T_CODE8 = 1           # F6: codes of 255
T_CODE16 = 1          # F7: codes of 65535
T_COV16 = 1           # F13: coverage codes of 65535

WIDTH, HEIGHT = 48, 32
FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'
SYSTEM_SRGB = Path('/System/Library/ColorSync/Profiles/sRGB Profile.icc')


# ------------------------------------------------------------ the fixture --

def scene():
    """The authored scene, as f32-rounded Python floats.

    Linear ramps, one genuinely above-white block (8.0, 4.0, 2.0), one block
    with a supported negative component (-0.25) and a half-alpha band. Every
    value is representable exactly in binary32, so "bit-exact" below is a
    statement about the pipeline and not about rounding.
    """
    out = []
    for y in range(HEIGHT):
        for x in range(WIDTH):
            r = x / (WIDTH - 1)
            g = y / (HEIGHT - 1)
            b = 0.25
            a = 1.0
            if y < 8 and x < 12:
                r, g, b = 8.0, 4.0, 2.0
            elif y < 8 and x < 24:
                r, g, b = -0.25, 0.5, 0.75
            if y >= 24:
                a = 0.5
            out.extend(struct.unpack('<4f', struct.pack('<4f', r, g, b, a)))
    return out


def author_fixture(path):
    """Writes the fixture: 48x32 RGBA f32, Deflate, ExtraSamples 2, ICC.

    Hand-assembled rather than produced by `magick`, because the gate needs
    EXACT 8.0 and -0.25 samples and ImageMagick's Q16-HDRI pipeline scales
    float output by its own quantum range. The file is then verified by
    `tiffinfo` and `magick identify`, which is what makes it a real float
    TIFF rather than one this build agrees with itself about.
    """
    return write_float_tiff(path, WIDTH, HEIGHT, scene(), SYSTEM_SRGB.read_bytes())


def write_float_tiff(path, width, height, samples, icc, ppi=72.0):
    """A `width` x `height` RGBA f32 Deflate TIFF of `samples` (straight
    alpha, ExtraSamples 2) carrying `icc` at `ppi` (both axes, in
    ten-thousandths), one strip: the fixture's writer, shared with F36's
    targets."""
    raw = struct.pack('<%df' % (width * height * 4), *samples)
    strip = zlib.compress(raw, 6)
    header = 8
    count = 16
    ifd_size = 2 + count * 12 + 4
    at = header + ifd_size
    bits_off = at; at += 8
    fmt_off = at; at += 8
    xres_off = at; at += 8
    yres_off = at; at += 8
    icc_off = at; at += len(icc)
    if at % 2:
        at += 1
    strip_off = at
    entries = sorted([
        (256, 3, 1, width), (257, 3, 1, height), (258, 3, 4, bits_off),
        (259, 3, 1, 8), (262, 3, 1, 2), (273, 4, 1, strip_off), (277, 3, 1, 4),
        (278, 3, 1, height), (279, 4, 1, len(strip)), (282, 5, 1, xres_off),
        (283, 5, 1, yres_off), (284, 3, 1, 1), (296, 3, 1, 2), (338, 3, 1, 2),
        (339, 3, 4, fmt_off), (34675, 7, len(icc), icc_off),
    ])
    body = b'II' + struct.pack('<HI', 42, header)
    ifd = struct.pack('<H', len(entries))
    for tag, kind, n, value in entries:
        if kind == 3 and n == 1:
            ifd += struct.pack('<HHIHH', tag, kind, n, value, 0)
        else:
            ifd += struct.pack('<HHII', tag, kind, n, value)
    ifd += struct.pack('<I', 0)
    assert len(ifd) == ifd_size, (len(ifd), ifd_size)
    tail = struct.pack('<4H', 32, 32, 32, 32) + struct.pack('<4H', 3, 3, 3, 3)
    tail += struct.pack('<II', round(ppi * 10000), 10000) * 2
    tail += icc
    if (header + ifd_size + len(tail)) % 2:
        tail += b'\0'
    Path(path).write_bytes(body + ifd + tail + strip)
    return path


# ------------------------------------------------------ the stdlib readers --

def read_float_tiff(path):
    """A classic little-endian float TIFF, parsed here rather than by a
    library: tags, strips, Deflate and the f32 samples — or, for a 16-bit
    file (F38), the half samples through `struct`'s own `'e'`."""
    data = Path(path).read_bytes()
    assert data[:2] == b'II' and struct.unpack_from('<H', data, 2)[0] == 42, 'classic little TIFF'
    offset = struct.unpack_from('<I', data, 4)[0]
    n = struct.unpack_from('<H', data, offset)[0]
    tags = {}
    for i in range(n):
        at = offset + 2 + i * 12
        tag, kind, cnt = struct.unpack_from('<HHI', data, at)
        size = {1: 1, 2: 1, 3: 2, 4: 4, 5: 8, 7: 1}.get(kind, 1) * cnt
        if size <= 4:
            payload = data[at + 8:at + 8 + size]
        else:
            where = struct.unpack_from('<I', data, at + 8)[0]
            payload = data[where:where + size]
        if kind == 3:
            tags[tag] = list(struct.unpack('<%dH' % cnt, payload))
        elif kind == 4:
            tags[tag] = list(struct.unpack('<%dI' % cnt, payload))
        elif kind == 5:
            tags[tag] = list(struct.unpack('<%dI' % (cnt * 2), payload))
        else:
            tags[tag] = payload
    width, height = tags[256][0], tags[257][0]
    samples_per_pixel = tags[277][0]
    compression = tags[259][0]
    assert set(tags[258]) in ({32}, {16}), tags[258]
    assert set(tags[339]) == {3}, tags[339]
    assert tags.get(284, [1])[0] == 1, 'chunky only'
    body = b''
    for start, length in zip(tags[273], tags[279]):
        piece = data[start:start + length]
        body += zlib.decompress(piece) if compression == 8 else piece
    total = width * height * samples_per_pixel
    if tags[258][0] == 16:
        values = list(struct.unpack('<%de' % total, body[:total * 2]))
    else:
        values = list(struct.unpack('<%df' % total, body[:total * 4]))
    return dict(width=width, height=height, channels=samples_per_pixel,
                extra_samples=tags.get(338), sample_format=tags[339],
                bits=tags[258], compression=compression,
                icc=tags.get(34675, b''), icc_bytes=len(tags.get(34675, b'')),
                samples=values)


def bits_equal(a, b):
    """f32 equality by BITS, so a sign or a NaN cannot pass as equal."""
    if len(a) != len(b):
        return False
    pack = lambda v: struct.pack('<f', v)
    return all(pack(x) == pack(y) for x, y in zip(a, b))


# ------------------------------------------------------------ the oracles --

def srgb_encode(linear):
    """The analytic sRGB transfer function, from the specification."""
    linear = min(max(linear, 0.0), 1.0)
    if linear <= 0.0031308:
        return 12.92 * linear
    return 1.055 * linear ** (1.0 / 2.4) - 0.055


def icc_trc_tables(icc):
    """The three `curv` tone curves out of an ICC profile, as u16 tables.

    Parsed here from the FILE's own bytes: the tag table, then each of
    `rTRC`/`gTRC`/`bTRC` as a `curv` element. Only the table form is handled,
    which is what the fixture (and every v2 matrix/TRC profile this build
    writes) carries.
    """
    count = struct.unpack_from('>I', icc, 128)[0]
    tags = {}
    for i in range(count):
        signature, offset, size = struct.unpack_from('>4sII', icc, 132 + i * 12)
        tags[signature] = (offset, size)
    out = []
    for signature in (b'rTRC', b'gTRC', b'bTRC'):
        offset, _ = tags[signature]
        assert icc[offset:offset + 4] == b'curv', icc[offset:offset + 4]
        entries = struct.unpack_from('>I', icc, offset + 8)[0]
        assert entries > 1, entries
        out.append(list(struct.unpack_from('>%dH' % entries, icc, offset + 12)))
    return out


def icc_tags(icc):
    """The tag table of an ICC profile: signature -> its data bytes."""
    count = struct.unpack_from('>I', icc, 128)[0]
    tags = {}
    for i in range(count):
        signature, offset, size = struct.unpack_from('>4sII', icc, 132 + i * 12)
        tags[signature] = icc[offset:offset + size]
    return tags


def icc_basis(icc):
    """F16's Photo Filter basis from the profile's own tags: `m` the D50
    matrix (row-major, columns `rXYZ`/`gXYZ`/`bXYZ`) and `d` its native->D50
    adaptation — the `chad` tag (row-major s15Fixed16), else Bradford from a
    `wtpt` more than 1e-4 from D50, else the identity."""
    tags = icc_tags(icc)

    def s15(data, at):
        return struct.unpack_from('>i', data, at)[0] / 65536.0

    def xyz(signature):
        data = tags[signature]
        assert data[:4] == b'XYZ ', data[:4]
        return [s15(data, 8 + 4 * k) for k in range(3)]

    columns = [xyz(b'rXYZ'), xyz(b'gXYZ'), xyz(b'bXYZ')]
    m = [[columns[c][i] for c in range(3)] for i in range(3)]
    if b'chad' in tags:
        data = tags[b'chad']
        assert data[:4] == b'sf32', data[:4]
        d = [[s15(data, 8 + 4 * (3 * i + j)) for j in range(3)] for i in range(3)]
    elif b'wtpt' in tags and any(abs(w - z) > 1e-4 for w, z in zip(xyz(b'wtpt'), F16_D50)):
        d = f16_bradford(xyz(b'wtpt'), F16_D50)
    else:
        d = [[1.0 if i == j else 0.0 for j in range(3)] for i in range(3)]
    return m, d


def curve_eval(x, table):
    """One `curv` table, evaluated the way the ICC specification defines it:
    uniform samples over [0, 1], linearly interpolated over `n - 1` intervals."""
    x = min(max(x, 0.0), 1.0)
    n = len(table)
    position = x * (n - 1)
    i = int(position)
    if i >= n - 1:
        return table[n - 1] / 65535.0
    f = position - i
    return (table[i] * (1.0 - f) + table[i + 1] * f) / 65535.0


ENCODE_INTERVALS = 65536


def forward_tables(tables):
    """`values[c][i] = curve_c(i / 65536)` — the forward sampling a FORWARD
    threshold search needs. There is no curve inverter here either: the
    encode below searches this table exactly as the core's does.

    That sharing is deliberate and it bounds what F7 proves. The CURVE comes
    from the fixture's own ICC bytes and the export round trip is modelled
    independently, but the ENCODE RULE (forward search over this table, then
    one round) is the core's documented rule restated here, not an
    independent derivation of it. F7 is therefore not a fully independent
    oracle the way F2 (f64 algebra) and F6 (analytic sRGB) are: a change of
    encode rule in the core would have to be mirrored here rather than being
    caught by this gate."""
    return [[curve_eval(i / ENCODE_INTERVALS, table) for i in range(ENCODE_INTERVALS + 1)]
            for table in tables]


def encode_through(values, linear):
    """One linear value as its ENCODED coordinate, by bracketing the forward
    table and interpolating — the documented rule, written out here."""
    import bisect
    v = min(max(linear, 0.0), 1.0)
    if v <= values[0]:
        return 0.0
    if v > values[ENCODE_INTERVALS]:
        return 1.0
    upper = bisect.bisect_left(values, v)
    if upper == 0:
        return 0.0
    low = upper - 1
    span = values[upper] - values[low]
    frac = 0.0 if span == 0 else (v - values[low]) / span
    return (low + frac) / ENCODE_INTERVALS


def tone_map(value, operator, white=4.0):
    if operator == 'clip':
        return min(max(value, 0.0), 1.0)
    u = max(value, 0.0)
    return min(u * (1.0 + u / (white * white)) / (1.0 + u), 1.0)


def exposure_oracle(base, stops=1.0):
    """+N EV in linear light is a gain of exactly 2**N on each stored channel
    (no reference-basis matrices at 32 bits). Alpha is untouched."""
    gain = 2.0 ** stops
    out = []
    for i in range(0, len(base), 4):
        a = base[i + 3]
        out.extend([base[i] * gain, base[i + 1] * gain, base[i + 2] * gain, a]
                   if a > 0 else [base[i] * gain, base[i + 1] * gain, base[i + 2] * gain, a])
    return out


def multiply_over(base):
    """One layer composited onto an identical copy of itself in MULTIPLY,
    through the W3C source-over formula with an unclamped kernel:

        ao = as + ab(1 - as)
        co = (as(1 - ab)cs + as*ab*B(cb, cs) + (1 - as)ab*cb) / ao,  B = cb*cs
    """
    out = []
    for i in range(0, len(base), 4):
        cb = base[i:i + 3]
        ab = base[i + 3]
        cs, sa = cb, ab
        ao = sa + ab * (1.0 - sa)
        if ao == 0.0:
            out.extend([0.0, 0.0, 0.0, 0.0])
            continue
        row = [(sa * (1.0 - ab) * cs[c] + sa * ab * (cb[c] * cs[c])
                + (1.0 - sa) * ab * cb[c]) / ao for c in range(3)]
        out.extend(row + [ao])
    return out


def close(actual, expected, relative):
    worst = 0.0
    for a, e in zip(actual, expected):
        scale = max(abs(e), 1e-3)
        worst = max(worst, abs(a - e) / scale)
    return worst <= relative, worst


# ------------------------------------------------------------------ tools --

def tool(name):
    for folder in ('/opt/homebrew/bin', '/usr/local/bin', '/usr/bin'):
        if Path(folder, name).exists():
            return str(Path(folder, name))
    return None


def run(command):
    return subprocess.run(command, capture_output=True, text=True, check=False).stdout


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def first_words(path, count=2):
    data = Path(path).read_bytes()
    assert data[:4] in (b'RZDC', b'DRDC'), data[:4]
    return [struct.unpack_from('<I', data, 4 + 4 * i)[0] for i in range(count)]


def record(report, name, passed, detail):
    report['gates'][name] = dict(passed=bool(passed), detail=detail)
    print(('PASS ' if passed else 'FAIL ') + name, flush=True)
    assert passed, (name, detail)


def export32(app, document, path):
    reply = app.call('export_image', document_id=document, path=str(path),
                     settings={'format': 'tiff', 'bit_depth': 32}, overwrite=True)
    assert reply['ok'], reply
    return read_float_tiff(path)


# ------------------------------------------------------------------ gates --

def gate_f1(app, folder, report):
    document = app.open(FIXTURE)
    state = app.call('get_document', document_id=document)
    exported = export32(app, document, folder / 'f1.tiff')
    base = scene()
    detail = dict(
        precision=state.get('precision'),
        float_block=state.get('float'),
        evaluation=state.get('evaluation'),
        import_warnings=state.get('import_warnings'),
        dimensions=[state.get('width'), state.get('height')],
        samples_bit_exact=bits_equal(exported['samples'], base),
        above_white=exported['samples'][0:3],
        below_zero=exported['samples'][48:51],
        half_alpha=exported['samples'][24 * WIDTH * 4 + 3],
    )
    passed = (detail['precision'] == 'rgba32_float'
              and (detail['float_block'] or {}).get('range') == 'extended'
              and (detail['float_block'] or {}).get('tone_map', {}).get('operator') == 'clip'
              and (detail['evaluation'] or {}).get('policy') == 'linear_light'
              and not (detail['import_warnings'] or [])
              and detail['samples_bit_exact']
              and detail['above_white'] == [8.0, 4.0, 2.0]
              and detail['below_zero'][0] == -0.25
              and detail['half_alpha'] == 0.5)
    record(report, 'F1 a genuine float TIFF opens with exact samples', passed, detail)
    return document


def gate_f2(app, folder, document, report):
    app.call('add_adjustment_layer', document_id=document, op='exposure',
             params={'exposure': 1.0})
    only = export32(app, document, folder / 'f2-exposure.tiff')
    base = scene()
    expected = exposure_oracle(base)
    ok_exposure, worst_exposure = close(only['samples'], expected, T_REL)

    app.call('duplicate_layer', document_id=document, index=0)
    app.call('set_layer_properties', document_id=document, index=1, blend_mode='Multiply')
    composed = export32(app, document, folder / 'f2-composite.tiff')
    expected_composite = exposure_oracle(multiply_over(base))
    ok_composite, worst_composite = close(composed['samples'], expected_composite, T_REL)

    detail = dict(tolerance=T_REL, exposure_worst_relative=worst_exposure,
                  composite_worst_relative=worst_composite,
                  above_white_after_exposure=only['samples'][0],
                  below_zero_after_exposure=only['samples'][48],
                  layers=[row['name'] for row in
                          app.call('get_document', document_id=document)['layers']])
    passed = (ok_exposure and ok_composite
              and abs(only['samples'][0] - 16.0) <= 1e-4
              and abs(only['samples'][48] + 0.5) <= 1e-6)
    record(report, 'F2 exposure and a Multiply composite match an f64 oracle', passed, detail)
    return composed['samples']


def gate_f3(app, folder, document, samples, report):
    native = folder / 'f3.rz'
    app.call('save_copy', document_id=document, path=str(native), format='rz')
    # Since 12E-1 every .rz leads with the identity word (46 typed), with
    # the float wrapper 40 and then 39 behind it.
    words = first_words(native, 3)
    reopened = app.open(native)
    state = app.call('get_document', document_id=reopened)
    round_trip = export32(app, reopened, folder / 'f3.tiff')
    detail = dict(first_words=words, precision=state.get('precision'),
                  tone_map=(state.get('float') or {}).get('tone_map'),
                  samples_bit_exact=bits_equal(round_trip['samples'], samples),
                  sha256=digest(native))
    passed = (words == [46, 40, 39] and detail['precision'] == 'rgba32_float'
              and detail['samples_bit_exact'])
    record(report, 'F3 a float .rz reopens bit-exact behind 46, 40 then 39', passed, detail)
    return reopened


def gate_f4(app, folder, document, samples, report):
    app.call('create_snapshot', document_id=document, name='float')
    app.call('add_adjustment_layer', document_id=document, op='exposure',
             params={'exposure': -2.0})
    entry = next(row for row in app.call('list_snapshots', document_id=document)['snapshots']
                 if row['name'] == 'float')
    app.call('restore_snapshot', document_id=document, snapshot_id=entry['snapshot_id'])
    restored = export32(app, document, folder / 'f4-snapshot.tiff')
    detail = dict(snapshot_bit_exact=bits_equal(restored['samples'], samples))
    record(report, 'F4a a snapshot restores the float samples exactly',
           detail['snapshot_bit_exact'], detail)


def gate_f4_recovery(folder, report, executable, samples):
    """A forced crash and a relaunched recovery, on their own owned instance."""
    recovery = folder / 'recovery'
    previous = phase5_support.APP
    phase5_support.APP = executable
    try:
        with App(folder / 'app-a', recovery, interval=600) as app:
            document = app.open(FIXTURE)
            app.call('add_adjustment_layer', document_id=document, op='exposure',
                     params={'exposure': 1.0})
            app.call('duplicate_layer', document_id=document, index=0)
            app.call('set_layer_properties', document_id=document, index=1,
                     blend_mode='Multiply')
            app.call('write_recovery', document_id=document, wait=True)
            app.crash()
        with App(folder / 'app-b', recovery, interval=600) as app:
            available = app.call('get_recovery_status')['available']
            assert available, available
            reply = app.call('recover_document', recovery_id=available[0]['recovery_id'],
                             show=True)
            recovered = reply['document']['id']
            state = app.call('get_document', document_id=recovered)
            exported = export32(app, recovered, folder / 'f4-recovered.tiff')
            detail = dict(precision=state.get('precision'),
                          tone_map=(state.get('float') or {}).get('tone_map'),
                          samples_bit_exact=bits_equal(exported['samples'], samples))
    finally:
        phase5_support.APP = previous
    passed = detail['precision'] == 'rgba32_float' and detail['samples_bit_exact']
    record(report, 'F4b a crash and recover_document keep the float samples', passed, detail)


def gate_f5(app, folder, document, report):
    path = folder / 'f5.tiff'
    exported = export32(app, document, path)
    info = run([tool('tiffinfo'), str(path)]) if tool('tiffinfo') else ''
    identify = run([tool('magick'), 'identify', '-format',
                    '%[depth] %[type] %[channels]', str(path)]) if tool('magick') else ''
    detail = dict(
        tiffinfo=info.strip().splitlines(),
        magick=identify.strip(),
        parser=dict(channels=exported['channels'], bits=exported['bits'],
                    sample_format=exported['sample_format'],
                    extra_samples=exported['extra_samples'],
                    icc_bytes=exported['icc_bytes'], compression=exported['compression']),
        samples_bit_exact=bits_equal(exported['samples'],
                                     export32(app, document, folder / 'f5-again.tiff')['samples']),
        tools=dict(tiffinfo=tool('tiffinfo'), magick=tool('magick')))
    passed = (exported['channels'] == 4 and exported['bits'] == [32] * 4
              and exported['sample_format'] == [3] * 4
              and exported['extra_samples'] == [2] and exported['icc_bytes'] > 0
              and 'IEEE floating point' in info and 'unassoc-alpha' in info
              and 'ICC Profile: <present>' in info
              and identify.startswith('32 TrueColorAlpha')
              and detail['samples_bit_exact'])
    record(report, 'F5 the 32-bit export reads back exactly in three readers', passed, detail)
    return exported['samples']


def gate_f6(app, folder, document, linear, report):
    cases = []
    worst = 0
    for name, settings in [('clip', {'operator': 'clip'}),
                           ('reinhard', {'operator': 'reinhard', 'white': 4.0})]:
        app.call('set_tone_map', document_id=document, **settings)
        path = folder / f'f6-{name}.png'
        app.call('export_image', document_id=document, path=str(path),
                 settings={'format': 'png', 'bit_depth': 8}, overwrite=True)
        _, _, pixels = read_png(Path(path).read_bytes())
        case_worst = 0
        for i in range(WIDTH * HEIGHT):
            for c in range(3):
                mapped = tone_map(linear[i * 4 + c], settings['operator'],
                                  settings.get('white', 4.0))
                expected = round(srgb_encode(mapped) * 255.0)
                case_worst = max(case_worst, abs(pixels[i][c] - expected))
        worst = max(worst, case_worst)
        cases.append(dict(map=settings, worst_codes=case_worst, sha256=digest(path)))
    detail = dict(tolerance=T_CODE8, cases=cases, worst_codes=worst,
                  maps_differ=cases[0]['sha256'] != cases[1]['sha256'])
    record(report, 'F6 both tone maps match the Python oracle',
           worst <= T_CODE8 and detail['maps_differ'], detail)


def gate_f7(app, folder, document, linear, report):
    """The deliberate 16-bit conversion, against the FILE's own tone curve.

    The oracle here is written out in full rather than reusing `srgb_encode`,
    because two documented stages sit between the linear sample and the
    exported code and neither is the analytic sRGB function:

    1. The conversion maps the sample and encodes it through the DOCUMENT
       PROFILE's own 1024-entry `curv` table (a piecewise-linear
       approximation of the sRGB curve, which differs from the analytic one
       by several codes of 65535 in the toe), then rounds once.
    2. The converted document is LINEAR LIGHT (11E), so exporting it decodes
       each stored code through that same curve, composites the single raster
       over transparency — an identity — and re-encodes once.

    Both stages are reproduced below from the ICC bytes in the fixture and
    from the forward-search rule, so the comparison is exact rather than
    approximate.
    """
    tables = icc_trc_tables(read_float_tiff(FIXTURE)['icc'])
    values = forward_tables(tables)
    app.call('set_tone_map', document_id=document, operator='reinhard', white=4.0)
    app.call('convert_precision', document_id=document, mode='rgba16', flatten=True)
    state = app.call('get_document', document_id=document)
    path = folder / 'f7.png'
    app.call('export_image', document_id=document, path=str(path),
             settings={'format': 'png', 'bit_depth': 16}, overwrite=True)
    _, _, pixels = read_png(Path(path).read_bytes())
    worst = 0
    for i in range(WIDTH * HEIGHT):
        for c in range(3):
            mapped = tone_map(linear[i * 4 + c], 'reinhard', 4.0)
            stored = round(encode_through(values[c], mapped) * 65535.0)
            decoded = curve_eval(stored / 65535.0, tables[c])
            expected = round(encode_through(values[c], decoded) * 65535.0)
            worst = max(worst, abs(pixels[i][c] - expected))
    native = folder / 'f7.rz'
    app.call('save_copy', document_id=document, path=str(native), format='rz')
    evaluation = app.call('get_evaluation', document_id=document)
    detail = dict(tolerance=T_CODE16, worst_codes=worst, precision=state.get('precision'),
                  first_words=first_words(native, 2), evaluation=evaluation.get('policy'),
                  oracle='curve from the fixture ICC and the export round trip modelled '
                         'independently; the forward-search encode rule is shared with the core')
    passed = (worst <= T_CODE16 and state.get('precision') == 'rgba16'
              and detail['first_words'] == [46, 39]
              and evaluation.get('policy') == 'linear_light')
    record(report, 'F7 the deliberate 16-bit conversion carries the map', passed, detail)


def gate_f8(app, folder, report, u16_document):
    # A COPY of the fixture, so this runs on a FRESH float document: opening
    # a path that is already open returns the document already on screen.
    clean = folder / 'f8-clean.tiff'
    clean.write_bytes(FIXTURE.read_bytes())
    document = app.open(clean)
    before = app.call('get_document', document_id=document)
    cases = {}

    def refuses(name, tool_name, **arguments):
        # `fail=True` asserts the tool reported an in-band error and returns
        # its sentence, so the REFUSAL is the measurement.
        cases[name] = dict(refused=True,
                           message=app.call(tool_name, fail=True, document_id=document,
                                            **arguments))

    # The Brush, Eraser and Gradient (F37), the bucket (F46) and the Magnetic
    # Lasso (F47) are admitted at float.
    refuses('screen_blend', 'set_layer_properties', index=0, blend_mode='Screen')
    refuses('invert_adjustment', 'add_adjustment_layer', op='invert', params={})
    refuses('encoded_evaluation', 'set_evaluation', policy='encoded')
    refuses('clip_with_white', 'set_tone_map', operator='clip', white=2.0)
    # A COPY, because opening a path that is already open returns the open
    # document before any import choice is resolved.
    copy = folder / 'f8-precision.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    assert copy != clean
    cases['open_as_rgba8'] = dict(
        refused=True,
        message=app.call('open_document', fail=True, path=str(copy), precision='rgba8'))
    cases['u16_32bit_export'] = dict(
        refused=True,
        message=app.call('export_image', fail=True, document_id=u16_document,
                         path=str(folder / 'f8-refused.tiff'),
                         settings={'format': 'tiff', 'bit_depth': 32}, overwrite=True))

    after = app.call('get_document', document_id=document)
    # Image Size resamples at float. It runs AFTER the unchanged
    # check, because it changes the document.
    app.call('image_size', document_id=document, width=24, height=16)
    resized = app.call('get_document', document_id=document)
    detail = dict(cases=cases, document_unchanged=before == after,
                  image_size_admitted=(resized.get('width'), resized.get('height')) == (24, 16))
    named = all(row['message'] for row in cases.values())
    record(report, 'F8 every unadmitted operator refuses by name',
           named and detail['document_unchanged'] and detail['image_size_admitted'], detail)


def gate_f10(app, folder, report):
    """Agent parity: the selection work the float UI enables must answer over
    MCP too. A selection is canvas coverage, not document pixels, so a float
    document takes the marquee, Save Selection (a `channel_set`) and Load
    Selection exactly as the menus do."""
    copy = folder / 'f10-parity.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    document = app.open(copy)
    app.call('select_rect', document_id=document, x=4, y=4, width=16, height=8)
    selection = app.call('get_selection', document_id=document)
    saved = app.call('save_selection', document_id=document, name='Parity')
    channels = app.call('list_channels', document_id=document)
    loaded = app.call('load_selection',
                      **{'document_id': document, 'from': 'channel',
                         'channel': saved.get('channel')})
    state = app.call('get_document', document_id=document)
    names = [row.get('name') for row in (channels.get('channels') or [])]
    detail = dict(selection_bounds=selection.get('bounds'), saved=saved.get('ok'),
                  channels=names, loaded=loaded.get('ok'),
                  precision=state.get('precision'))
    passed = (selection.get('bounds') is not None and saved.get('ok') is True
              and 'Parity' in names and loaded.get('ok') is True
              and state.get('precision') == 'rgba32_float')
    record(report, 'F10 selection and save_selection answer the agent on a float document',
           passed, detail)


def gate_f11(app, folder, report):
    # A COPY of the fixture, so this runs on a FRESH float document: opening
    # a path that is already open returns the document already on screen.
    copy = folder / 'f11-preservation.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    document = app.open(copy)
    state = app.call('get_document', document_id=document)
    preservation = state.get('save_preservation') or {}
    png_losses = (preservation.get('losses_by_format') or {}).get('png') or []
    sentence = ('32-bit float values and the range above white '
                '(tone-mapped to 16-bit codes through the document\'s tone map)')
    flat = folder / 'f11-flat.png'
    saved = app.call('save_copy', document_id=document, path=str(flat), format='png')
    exported = folder / 'f11-export.png'
    app.call('export_image', document_id=document, path=str(exported),
             settings={'format': 'png', 'bit_depth': 16}, overwrite=True)
    flat_bytes = flat.read_bytes()
    _, _, flat_pixels = read_png(flat_bytes)
    _, _, export_pixels = read_png(exported.read_bytes())
    detail = dict(precision=state.get('precision'),
                  has_save_preservation='save_preservation' in state,
                  png_losses=png_losses,
                  save_copy=dict(bit_depth=saved.get('bit_depth'),
                                 master_bit_depth=saved.get('master_bit_depth'),
                                 ihdr_bit_depth=flat_bytes[24], ihdr_color_type=flat_bytes[25],
                                 sha256=digest(flat)),
                  export_sha256=digest(exported),
                  samples_equal=flat_pixels == export_pixels)
    passed = (state.get('precision') == 'rgba32_float'
              and sentence in png_losses
              and saved.get('bit_depth') == 16 and saved.get('master_bit_depth') == 32
              and flat_bytes[24] == 16
              and flat_pixels == export_pixels)
    record(report, 'F11 a float document reports save_preservation and save_copy writes PNG16',
           passed, detail)


def gate_f12(app, folder, report, u16_document):
    """A float layer, group or plane render is a tone-mapped display
    like the composite, never the u16 native-selection refusal."""
    copy = folder / 'f12-layers.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    document = app.open(copy)
    texts = []

    def render(doc, **arguments):
        reply = app.client.rpc('tools/call', {'name': 'render', 'arguments': dict(
            document_id=doc, max_side=4096, **arguments)})
        text = ' '.join(item.get('text', '') for item in reply.get('content', [])
                        if item.get('type') == 'text')
        texts.append(text)
        if reply.get('isError'):
            return dict(error=True, text=text)
        data = next(item['data'] for item in reply['content'] if item['type'] == 'image')
        raw = base64.b64decode(data)
        width, height, pixels = read_png(raw)
        return dict(error=False, text=text, width=width, height=height, pixels=pixels,
                    sha256=hashlib.sha256(raw).hexdigest())

    def worst(a, b):
        if a['error'] or b['error'] or len(a['pixels']) != len(b['pixels']):
            return None
        return max(abs(x - y) for p, q in zip(a['pixels'], b['pixels']) for x, y in zip(p, q))

    base = scene()
    codes = [[round(srgb_encode(tone_map(base[i * 4 + c], 'clip')) * 255.0) for c in range(3)]
             for i in range(WIDTH * HEIGHT)]
    oracle = {
        'red': [p[0] for p in codes], 'green': [p[1] for p in codes],
        'blue': [p[2] for p in codes],
        'alpha': [round(base[i * 4 + 3] * 255.0) for i in range(WIDTH * HEIGHT)],
        'luma': [round(0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]) for p in codes],
    }

    def plane_worst(result, channel):
        if result['error'] or len(result['pixels']) != WIDTH * HEIGHT:
            return None
        return max(abs(p[0] - e) for p, e in zip(result['pixels'], oracle[channel]))

    detail = {}
    app.call('set_tone_map', document_id=document, operator='clip')
    composite_a = render(document)
    layer_a = render(document, layer=0)
    detail['a'] = dict(error=layer_a['error'], text=layer_a['text'],
                       size=(layer_a.get('width'), layer_a.get('height')),
                       worst=worst(layer_a, composite_a))
    ok_a = (not layer_a['error'] and (layer_a['width'], layer_a['height']) == (WIDTH, HEIGHT)
            and detail['a']['worst'] is not None and detail['a']['worst'] <= T_CODE8)

    app.call('set_tone_map', document_id=document, operator='reinhard', white=4.0)
    composite_b = render(document)
    layer_b = render(document, layer=0)
    detail['b'] = dict(error=layer_b['error'], worst=worst(layer_b, composite_b),
                       differs_from_a=layer_b.get('sha256') != layer_a.get('sha256'))
    ok_b = (not layer_b['error'] and detail['b']['differs_from_a']
            and detail['b']['worst'] is not None and detail['b']['worst'] <= T_CODE8)

    app.call('set_layer_properties', document_id=document, index=0, opacity=0.5)
    composite_c = render(document)
    layer_c = render(document, layer=0)
    detail['c'] = dict(composite_changed=composite_c.get('sha256') != composite_b.get('sha256'),
                       layer_identical=(not layer_c['error']
                                        and layer_c['pixels'] == layer_b['pixels']))
    ok_c = detail['c']['composite_changed'] and detail['c']['layer_identical']

    app.call('set_layer_properties', document_id=document, index=0, opacity=1.0)
    app.call('group_layers', document_id=document, layers=[0], name='Group')
    rows = app.call('get_document', document_id=document).get('layers') or []
    groups = [row.get('index') for row in rows if row.get('kind') == 'group']
    group_d = render(document, layer=groups[0]) if groups else dict(error=True, text='no group')
    detail['d'] = dict(group=groups, error=group_d['error'], text=group_d['text'],
                       worst=worst(group_d, layer_b))
    ok_d = (len(groups) == 1 and not group_d['error']
            and detail['d']['worst'] is not None and detail['d']['worst'] <= T_CODE8)

    app.call('set_tone_map', document_id=document, operator='clip')
    detail['e'] = {}
    detail['f'] = {}
    for channel in ('red', 'green', 'blue', 'alpha', 'luma'):
        composite = render(document, channel=channel)
        layer = render(document, channel=channel, layer=0)
        detail['e'][channel] = dict(error=composite['error'], worst=plane_worst(composite, channel))
        detail['f'][channel] = dict(error=layer['error'], worst=plane_worst(layer, channel))
    ok_e = all(row['worst'] is not None and row['worst'] <= T_CODE8
               for row in detail['e'].values())
    ok_f = all(row['worst'] is not None and row['worst'] <= T_CODE8
               for row in detail['f'].values())

    refused = render(document, channel='red', layer=99)
    detail['g'] = dict(error=refused['error'], text=refused['text'])
    ok_g = (refused['error'] and 'out of range' in refused['text']
            and 'Could not render' not in refused['text'])

    detail['h'] = not any('native selection requires u16 source' in text for text in texts)
    detail['i'] = composite_a['text']
    ok_i = '32-bit float' in composite_a['text'] and '16-bit' not in composite_a['text']

    u16_composite = render(u16_document)
    u16_layer = render(u16_document, layer=0)
    u16_red = render(u16_document, channel='red')
    detail['j'] = dict(layer_equals_composite=(not u16_layer['error']
                                               and u16_layer['pixels'] == u16_composite['pixels']),
                       red_error=u16_red['error'], red_text=u16_red['text'])
    ok_j = detail['j']['layer_equals_composite'] and not u16_red['error']

    detail['rows'] = dict(a=ok_a, b=ok_b, c=ok_c, d=ok_d, e=ok_e, f=ok_f, g=ok_g,
                          h=detail['h'], i=ok_i, j=ok_j)
    record(report, 'F12 float layer, group and plane renders are tone-mapped displays',
           all(detail['rows'].values()), detail)


def gate_f13(app, folder, report):
    """Load Selection from a colour plane or a layer's transparency
    answers on a float document with each scene-linear sample clamped to
    0...1 (Photoshop 2025's measured mapping), whatever the tone map; the
    luminosity plane is the fixture profile's Y row (`icc_basis`) over the
    UNclamped samples, only the sum clamped (Photoshop 2025 measured);
    Magic Wand still refuses by name."""
    copy = folder / 'f13-selection.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    document = app.open(copy)
    base = scene()
    points = [(5, 3), (17, 3), (0, 12), (10, 12), (24, 16), (47, 20), (10, 28)]
    texts = []
    # Row 1 of the D50 matrix: the rXYZ/gXYZ/bXYZ Y entries.
    y_row = icc_basis(read_float_tiff(FIXTURE)['icc'])[0][1]

    def code(v):
        return int(min(1.0, max(0.0, v)) * 65535.0 + 0.5)

    def oracle(plane, x, y):
        p = base[(y * WIDTH + x) * 4:(y * WIDTH + x) * 4 + 4]
        if plane == 'luma':
            return code(sum(w * c for w, c in zip(y_row, p[:3])))
        return code(p[{'red': 0, 'green': 1, 'blue': 2, 'alpha': 3}[plane]])

    def load(**source):
        reply = app.client.rpc('tools/call', {'name': 'load_selection', 'arguments': dict(
            document_id=document, **source)})
        text = ' '.join(item.get('text', '') for item in reply.get('content', [])
                        if item.get('type') == 'text')
        texts.append(text)
        if reply.get('isError'):
            return dict(error=True, text=text)
        state = app.call('get_selection', document_id=document,
                         points=[{'x': x, 'y': y} for x, y in points])
        return dict(error=False, coverage=[row.get('coverage16') for row in
                                           state.get('samples') or []])

    def check(result, plane):
        if result['error'] or len(result['coverage']) != len(points):
            return dict(error=True, text=result.get('text')), False
        want = [oracle(plane, x, y) for x, y in points]
        worst = max(abs(g - w) for g, w in zip(result['coverage'], want))
        return dict(got=result['coverage'], want=want, worst=worst), worst <= T_COV16

    detail = {}
    rows = {}
    for plane in ('red', 'green', 'blue', 'luma', 'alpha'):
        detail[plane], rows[plane] = check(load(**{'from': 'plane', 'plane': plane}), plane)
    detail['layer_alpha'], rows['layer_alpha'] = check(
        load(**{'from': 'layer_alpha', 'layer': 0}), 'alpha')

    app.call('set_tone_map', document_id=document, operator='reinhard', white=4.0)
    detail['reinhard_red'], rows['reinhard_red'] = check(
        load(**{'from': 'plane', 'plane': 'red'}), 'red')

    # The Magic Wand is admitted at float (F46 measures it).
    rows['no_u16_refusal'] = not any('requires u16 source' in text for text in texts)
    detail['rows'] = rows
    record(report, 'F13 float Load Selection from a plane or layer transparency clamps to 0...1',
           all(rows.values()), detail)


# F14's oracle: Photoshop 26.10.0's measured 32-bit formulas
# (`scripts/photoshop-probe/float_blend.py`), written out here.
F14_SHIFT = (6, 4)
F14_MODES = ['Linear Dodge (Add)', 'Subtract', 'Darker Color', 'Lighter Color', 'Hue',
             'Saturation', 'Color', 'Luminosity']


def f14_lum(c):
    return 0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]


def f14_set_sat(c, s):
    lo, mid, hi = sorted(range(3), key=lambda k: c[k])
    out = [0.0, 0.0, 0.0]
    if c[hi] > c[lo]:
        out[mid] = (c[mid] - c[lo]) * s / (c[hi] - c[lo])
        out[hi] = s
    return out


def f14_set_lum(c, l):
    d = l - f14_lum(c)
    return [v + d for v in c]


def f14_blend(mode, cb, cs):
    """`B(Cb, Cs)`, or None for a Darker/Lighter Color pick too close for
    f32 and f64 to agree on (either triple is then accepted)."""
    sat = lambda c: max(c) - min(c)
    if mode == 'Linear Dodge (Add)':
        return [b + s for b, s in zip(cb, cs)]
    if mode == 'Subtract':
        return [b - s for b, s in zip(cb, cs)]
    if mode in ('Darker Color', 'Lighter Color'):
        ls, lb = f14_lum(cs), f14_lum(cb)
        if abs(ls - lb) <= 1e-5 * max(1.0, abs(lb)) and list(cs) != list(cb):
            return None
        return list(cs) if (ls < lb if mode == 'Darker Color' else ls > lb) else list(cb)
    if mode == 'Hue':
        return f14_set_lum(f14_set_sat(cs, sat(cb)), f14_lum(cb))
    if mode == 'Saturation':
        return f14_set_lum(f14_set_sat(cb, sat(cs)), f14_lum(cb))
    if mode == 'Color':
        return f14_set_lum(cs, f14_lum(cb))
    return f14_set_lum(cb, f14_lum(cs))


def f14_composite(cb, ab, cs, sa, b):
    ao = sa + ab * (1.0 - sa)
    if ao == 0.0:
        return [0.0, 0.0, 0.0, 0.0]
    return [(sa * (1.0 - ab) * cs[c] + sa * ab * b[c] + (1.0 - sa) * ab * cb[c]) / ao
            for c in range(3)] + [ao]


def f14_close(got, want):
    return all(abs(g - w) <= T_REL * max(1.0, abs(w)) for g, w in zip(got, want))


def gate_f14(app, folder, report):
    copy = folder / 'f14-blend.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    document = app.open(copy)
    base = scene()
    app.call('duplicate_layer', document_id=document, index=0)
    dx, dy = F14_SHIFT
    app.call('set_layer_properties', document_id=document, index=1, offset_x=dx, offset_y=dy)

    def pixel(x, y):
        return base[(y * WIDTH + x) * 4:(y * WIDTH + x) * 4 + 4]

    def source(x, y):
        sx, sy = x - dx, y - dy
        if 0 <= sx < WIDTH and 0 <= sy < HEIGHT:
            return pixel(sx, sy)
        return [0.0, 0.0, 0.0, 0.0]

    detail = dict(tolerance=T_REL, shift=list(F14_SHIFT), modes={})
    rows = {}
    for mode in F14_MODES:
        app.call('set_layer_properties', document_id=document, index=1, blend_mode=mode)
        got = export32(app, document, folder / f'f14-{mode.split()[0].lower()}.tiff')['samples']
        failures = []
        for y in range(HEIGHT):
            for x in range(WIDTH):
                bg, src = pixel(x, y), source(x, y)
                out = got[(y * WIDTH + x) * 4:(y * WIDTH + x) * 4 + 4]
                b = f14_blend(mode, bg[:3], src[:3])
                picks = [b] if b is not None else [src[:3], bg[:3]]
                wants = [f14_composite(bg[:3], bg[3], src[:3], src[3], p) for p in picks]
                if not any(f14_close(out, w) for w in wants):
                    failures.append(dict(x=x, y=y, got=out, want=wants, cb=bg, cs=src))
        detail['modes'][mode] = dict(failures=len(failures), first=failures[:2])
        rows[mode] = not failures

    app.call('set_layer_properties', document_id=document, index=1, blend_mode='Dissolve')
    dissolved = export32(app, document, folder / 'f14-dissolve.tiff')['samples']
    shown = kept = other = 0
    for y in range(HEIGHT):
        for x in range(WIDTH):
            bg, src = pixel(x, y), source(x, y)
            out = dissolved[(y * WIDTH + x) * 4:(y * WIDTH + x) * 4 + 4]
            if src[3] > 0 and out == src[:3] + [1.0]:
                shown += 1
            elif f14_close(out, bg):
                kept += 1
            else:
                other += 1
    detail['dissolve'] = dict(shown=shown, kept=kept, other=other)
    rows['dissolve'] = other == 0 and shown > 0 and kept > 0

    # Persistence: the last mode set (Dissolve) survives a native round trip.
    before = dissolved
    native = folder / 'f14.rz'
    app.call('save_copy', document_id=document, path=str(native), format='rz')
    reopened = app.open(native)
    state = app.call('get_document', document_id=reopened)
    after = export32(app, reopened, folder / 'f14-reopened.tiff')['samples']
    reopened_mode = [row.get('blend_mode') for row in state.get('layers') or []]
    detail['reopened'] = dict(modes=reopened_mode, bit_exact=bits_equal(after, before))
    rows['reopened'] = detail['reopened']['bit_exact'] and 'Dissolve' in reopened_mode

    # Fill… with a built paint mode: white ink in Subtract at 0.5.
    fill_copy = folder / 'f14-fill.tiff'
    fill_copy.write_bytes(FIXTURE.read_bytes())
    filled_doc = app.open(fill_copy)
    app.call('fill_selection', document_id=filled_doc, layer=0, color='#ffffff',
             color_space='document', blend_mode='Subtract', opacity=0.5)
    filled = export32(app, filled_doc, folder / 'f14-fill-out.tiff')['samples']
    fill_failures = []
    for y in range(HEIGHT):
        for x in range(WIDTH):
            bg = pixel(x, y)
            want = f14_composite(bg[:3], bg[3], [1.0, 1.0, 1.0], 0.5,
                                 [v - 1.0 for v in bg[:3]])
            out = filled[(y * WIDTH + x) * 4:(y * WIDTH + x) * 4 + 4]
            if not f14_close(out, want):
                fill_failures.append(dict(x=x, y=y, got=out, want=want))
    detail['fill_subtract'] = dict(failures=len(fill_failures), first=fill_failures[:2])
    rows['fill_subtract'] = not fill_failures

    refusals = {}
    # Divide is admitted (F46); Overlay inverts about white and refuses.
    for mode in ('Screen', 'Overlay'):
        message = app.call('set_layer_properties', fail=True, document_id=document, index=1,
                           blend_mode=mode)
        refusals[mode] = message
        rows[f'{mode.lower()}_refuses'] = ('32-bit float layer blending supports' in message
                                           and 'Linear Dodge (Add)' in message)
    detail['refusals'] = refusals
    detail['rows'] = rows
    record(report, 'F14 the 32-bit blend modes composite, fill and persist',
           all(rows.values()), detail)


# F15's oracle: Photoshop 26.10.0's measured 32-bit style model
# (`scripts/photoshop-probe/float_styles.py`), written out here.
F15_SHIFT = (6, 4)
F15_SHADOW = 4
F15_RAMP = (0, 0, 60, 140)


def f15_decode(code, table):
    """An authored 8-bit code as linear light through one `curv` table of
    the fixture's own profile (`curve_eval`)."""
    return curve_eval(code / 255.0, table)


def f15_weight(red):
    """The measured This Layer ramp on the STORED red value, unclamped, its
    zero end one 15-bit code out (the Underlying ramp is full: weight 1)."""
    lo0, lo1, hi0, hi1 = [k * 257.0 for k in F15_RAMP]
    lo0 -= 65535.0 / 32768.0
    hi1 += 65535.0 / 32768.0
    v = red * 65535.0
    if v < lo0:
        return 0.0
    if v < lo1:
        return (v - lo0) / (lo1 - lo0)
    if v <= hi0:
        return 1.0
    if v <= hi1:
        return (hi1 - v) / (hi1 - hi0)
    return 0.0


def f15_over(cb, cs, sa):
    return [sa * s + (1.0 - sa) * b for b, s in zip(cb, cs)]


def gate_f15(app, folder, report):
    copy = folder / 'f15-style.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    document = app.open(copy)
    base = scene()
    app.call('duplicate_layer', document_id=document, index=0)
    dx, dy = F15_SHIFT
    app.call('set_layer_properties', document_id=document, index=1, offset_x=dx, offset_y=dy)
    style = {
        'blend_if': {'channel': 'red', 'this_layer': list(F15_RAMP),
                     'underlying': [0, 0, 255, 255]},
        'effects': [
            {'type': 'drop_shadow', 'blend': 'normal', 'color': '#ff8000', 'opacity': 1.0,
             'use_global_light': False, 'angle': 0, 'distance': F15_SHADOW, 'spread': 0,
             'size': 0},
            {'type': 'color_overlay', 'blend': 'normal', 'color': '#0040ff', 'opacity': 0.5},
        ],
    }
    reply = app.call('set_layer_style', document_id=document, layer=1, style=style)
    rows = dict(style_set=bool(reply.get('ok')))
    tables = icc_trc_tables(SYSTEM_SRGB.read_bytes())
    shadow_ink = [f15_decode(v, t) for v, t in zip((255, 128, 0), tables)]
    overlay_ink = [f15_decode(v, t) for v, t in zip((0, 64, 255), tables)]

    def pixel(x, y):
        return base[(y * WIDTH + x) * 4:(y * WIDTH + x) * 4 + 4]

    got = export32(app, document, folder / 'f15-styled.tiff')['samples']
    failures, shadowed, overlaid = [], 0, 0
    for y in range(HEIGHT):
        for x in range(WIDTH):
            bg = pixel(x, y)
            sx, sy = x - dx, y - dy
            inside = 0 <= sx < WIDTH and 0 <= sy < HEIGHT
            out = got[(y * WIDTH + x) * 4:(y * WIDTH + x) * 4 + 4]
            # Pixels where an effect covers FULLY: an opaque source pixel
            # (the overlay), or — off the copy — an opaque source pixel the
            # shadow lands from (cast left by F15_SHADOW).
            if inside and pixel(sx, sy)[3] == 1.0 and bg[3] == 1.0:
                cs = pixel(sx, sy)
                acc = f15_over(bg[:3], cs[:3], f15_weight(cs[0]))
                want = f15_over(acc, overlay_ink, 0.5) + [1.0]
                overlaid += 1
            elif not inside and bg[3] == 1.0:
                lx, ly = x + F15_SHADOW - dx, y - dy
                if not (0 <= lx < WIDTH and 0 <= ly < HEIGHT) or pixel(lx, ly)[3] != 1.0:
                    continue
                want = shadow_ink + [1.0]
                shadowed += 1
            else:
                continue
            if not f14_close(out, want):
                failures.append(dict(x=x, y=y, got=out, want=want))
    rows['composite'] = not failures and shadowed > 0 and overlaid > 0
    detail = dict(tolerance=T_REL, shadowed=shadowed, overlaid=overlaid,
                  failures=len(failures), first=failures[:3])

    light = app.call('set_global_light', document_id=document, angle=45, altitude=60)
    rows['light_set'] = bool(light.get('ok'))
    state = app.call('get_document', document_id=document)
    detail['light'] = state.get('global_light')
    rows['light_reported'] = (state.get('global_light') or {}).get('angle') == 45

    native = folder / 'f15.rz'
    app.call('save_copy', document_id=document, path=str(native), format='rz')
    reopened = app.open(native)
    after = export32(app, reopened, folder / 'f15-reopened.tiff')['samples']
    reopened_state = app.call('get_document', document_id=reopened)
    styles = [row.get('style') for row in reopened_state.get('layers') or []]
    detail['reopened'] = dict(styles=styles, light=reopened_state.get('global_light'),
                              bit_exact=bits_equal(after, got))
    rows['reopened'] = (detail['reopened']['bit_exact']
                        and any(isinstance(s, dict) and s.get('effects') for s in styles)
                        and (reopened_state.get('global_light') or {}).get('angle') == 45)

    message = app.call('set_layer_style', fail=True, document_id=document, layer=1,
                       style={'effects': [{'type': 'outer_glow', 'blend': 'screen'}]})
    detail['refusal'] = message
    rows['screen_glow_refuses'] = ('the screen blend mode on the outer_glow layer effect is '
                                   'unavailable in a 32-bit float document') in message
    detail['rows'] = rows
    record(report, 'F15 a layer style, Blend If and the global light composite and persist',
           all(rows.values()), detail)


# F16's oracle: the per-op forms fitted to Photoshop 26.10.0's 32-bit
# renders (`scripts/photoshop-probe/float_adjust.py`, receipt
# `doctored-fixtures/photoshop/float-adjust-473/take2`), written out here.
F16_D50 = [0.9642, 1.0, 0.8249]
F16_BRADFORD = [[0.8951, 0.2664, -0.1614], [-0.7502, 1.7135, 0.0367],
                [0.0389, -0.0685, 1.0296]]


def f16_mul(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)] for i in range(3)]


def f16_apply(m, v):
    return [sum(m[i][k] * v[k] for k in range(3)) for i in range(3)]


def f16_inv(m):
    det = (m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
           - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
           + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]))

    def cof(r, c):
        r0, r1, c0, c1 = (r + 1) % 3, (r + 2) % 3, (c + 1) % 3, (c + 2) % 3
        return m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0]
    return [[cof(j, i) / det for j in range(3)] for i in range(3)]


def f16_bradford(source, dest):
    cs, cd = f16_apply(F16_BRADFORD, source), f16_apply(F16_BRADFORD, dest)
    scale = [[cd[i] / cs[i] if i == j else 0.0 for j in range(3)] for i in range(3)]
    return f16_mul(f16_inv(F16_BRADFORD), f16_mul(scale, F16_BRADFORD))


def f16_signed_pow(t, p):
    return (1.0 if t >= 0 else -1.0) * abs(t) ** p


def f16_levels(params):
    def stage(p):
        return [p.get('black', 0.0), p.get('white', 1.0), p.get('gamma', 1.0),
                p.get('output_black', 0.0), p.get('output_white', 1.0)]

    master = stage(params)
    channels = [stage(params.get(k, {})) for k in ('r', 'g', 'b')]

    def run(v, s):
        black, white, gamma, ob, ow = s
        return ob + (ow - ob) * f16_signed_pow((v - black) / (white - black), 1.0 / gamma)
    return lambda rgb: [run(run(rgb[c], channels[c]), master) for c in range(3)]


def f16_black_and_white(params):
    names = ['reds', 'yellows', 'greens', 'cyans', 'blues', 'magentas']
    defaults = [0.4, 0.6, 0.4, 0.6, 0.2, 0.8]
    weights = [params.get(n, d) for n, d in zip(names, defaults)]

    def run(rgb):
        hi, mid, lo = sorted(range(3), key=lambda k: -rgb[k])
        pair = {(0, 1): 1, (1, 2): 3}.get((min(hi, mid), max(hi, mid)), 5)
        gray = (rgb[lo] + (rgb[hi] - rgb[mid]) * weights[2 * hi]
                + (rgb[mid] - rgb[lo]) * weights[pair])
        return [max(gray, 0.0)] * 3
    return run


def f16_mixer(params):
    import math
    rows = [params.get(name, default) for name, default in
            (('red', {'r': 1}), ('green', {'g': 1}), ('blue', {'b': 1}))]
    if params.get('monochrome'):
        rows = [params.get('gray', {'r': 0.4, 'g': 0.4, 'b': 0.2})] * 3

    def q(c):
        return math.trunc(1024.0 * c) / 1024.0
    return lambda rgb: [sum(q(row.get(k, 0.0)) * rgb[j] for j, k in enumerate('rgb'))
                        + math.trunc(512.0 * row.get('constant', 0.0)) / 1024.0
                        for row in rows]


def f16_photo_filter(params, icc):
    m, d = icc_basis(icc)
    tables = icc_trc_tables(icc)
    hex_colour = params['color'].lstrip('#')
    codes = [int(hex_colour[k:k + 2], 16) for k in (0, 2, 4)]
    xyz = f16_apply(m, [curve_eval(c / 255.0, t) for c, t in zip(codes, tables)])
    ratio = [x / w for x, w in zip(xyz, F16_D50)]
    to_native, back = f16_mul(f16_inv(d), m), f16_mul(f16_inv(m), d)
    density, preserve = params['density'], params['preserve_luminosity']

    def run(rgb):
        x = f16_apply(to_native, rgb)
        y = [x[c] * (1.0 - density + density * ratio[c]) for c in range(3)]
        if preserve and y[1] != 0.0:
            y = [v * x[1] / y[1] for v in y]
        return f16_apply(back, y)
    return run


def f16_exposure(params):
    gain = 2.0 ** params.get('exposure', 0.0)
    offset = params.get('offset', 0.0)
    power = 1.0 / params.get('gamma', 1.0)
    return lambda rgb: [f16_signed_pow(v * gain + offset, power) for v in rgb]


def f16_bcs(params):
    b = params.get('brightness', 0.0)
    slope = 1.0 + params.get('contrast', 0.0)
    scale = 1.0 + params.get('saturation', 0.0)

    def run(rgb):
        o = [(v + b - 0.5) * slope + 0.5 for v in rgb]
        a = 0.2126 * o[0] + 0.7152 * o[1] + 0.0722 * o[2]
        return [a + (v - a) * scale for v in o]
    return run


def f16_natural(points):
    """The natural cubic spline (S'' = 0 at both ends) through `points` in
    0-255 units, with Photoshop's 32-bit ends; second derivatives by dense
    Gaussian elimination."""
    pts = sorted((float(a), float(b)) for a, b in points)
    n = len(pts)
    a = [[0.0] * (n + 1) for _ in range(n)]
    a[0][0] = a[n - 1][n - 1] = 1.0
    for i in range(1, n - 1):
        h0, h1 = pts[i][0] - pts[i - 1][0], pts[i + 1][0] - pts[i][0]
        a[i][i - 1], a[i][i], a[i][i + 1] = h0, 2.0 * (h0 + h1), h1
        a[i][n] = 6.0 * ((pts[i + 1][1] - pts[i][1]) / h1 - (pts[i][1] - pts[i - 1][1]) / h0)
    for col in range(n):
        for row in range(col + 1, n):
            f = a[row][col] / a[col][col]
            a[row] = [x - f * y for x, y in zip(a[row], a[col])]
    m = [0.0] * n
    for row in reversed(range(n)):
        m[row] = (a[row][n] - sum(a[row][k] * m[k] for k in range(row + 1, n))) / a[row][row]

    def slope(i, x):
        h = pts[i + 1][0] - pts[i][0]
        u, v = (pts[i + 1][0] - x) / h, (x - pts[i][0]) / h
        return ((pts[i + 1][1] - pts[i][1]) / h
                + (-(3 * u * u - 1) * m[i] + (3 * v * v - 1) * m[i + 1]) * h / 6.0)

    def run(x):
        (x0, y0), (xn, yn) = pts[0], pts[-1]
        if x > xn:
            return yn + slope(n - 2, xn) * (x - xn)
        if x < x0:
            return y0 + slope(0, x0) * x if x0 == 0.0 else y0
        i = max(k for k in range(n - 1) if pts[k][0] <= x)
        h = pts[i + 1][0] - pts[i][0]
        u, v = (pts[i + 1][0] - x) / h, (x - pts[i][0]) / h
        return u * pts[i][1] + v * pts[i + 1][1] + ((u ** 3 - u) * m[i] + (v ** 3 - v) * m[i + 1]) * h * h / 6.0
    return run


def f16_map_stage(table, x):
    """One drawn map at a continuous code: the straight line between the two
    entries that bracket `x`, the first and last segments continued."""
    i = min(max(int(math.floor(x)), 0), 254)
    return table[i] + (x - i) * (table[i + 1] - table[i])


def f16_curves_map(params):
    identity = list(range(256))
    master = params.get('rgb_map', identity)
    channels = [params.get(k, identity) for k in ('r_map', 'g_map', 'b_map')]
    return lambda rgb: [f16_map_stage(master, f16_map_stage(channels[c], rgb[c] * 255.0)) / 255.0
                        for c in range(3)]


# take3's M3: M1 (m0 = 30, m1 = 34, 30 + round(0.8 i), m254 = 233,
# m255 = 239) under a red square-root map.
F16_CURVES_MAP = {
    'rgb_map': [30, 34] + [30 + int(math.floor(0.8 * i + 0.5)) for i in range(2, 254)]
               + [233, 239],
    'r_map': [int(math.floor(255.0 * math.sqrt(i / 255.0) + 0.5)) for i in range(256)],
}


F16_LOOKUP_SIZE = 5


def f16_lookup_entry(r, g, b):
    """take3's `cross_5` (L4): the entry at a node's (r, g, b)
    fractions. Its red is bilinear, so only the interpolation's own shape
    decides it between nodes."""
    return [r * g, g, b]


def f16_lookup_cube(where):
    """The 3D `.cube` of `f16_lookup_entry`, red fastest."""
    n = F16_LOOKUP_SIZE
    lines = ['TITLE "cross_5"', f'LUT_3D_SIZE {n}', 'DOMAIN_MIN 0.0 0.0 0.0',
             'DOMAIN_MAX 1.0 1.0 1.0']
    for b in range(n):
        for g in range(n):
            for r in range(n):
                entry = f16_lookup_entry(r / (n - 1), g / (n - 1), b / (n - 1))
                lines.append(' '.join(f'{v:.6f}' for v in entry))
    where.write_text('\n'.join(lines) + '\n')
    return str(where)


def f16_color_lookup(params):
    """Photoshop's 32-bit device-link form (take3) of the `cross_5`
    table: an input above 1 read as 1 and one below 0 as 0, tetrahedral (the
    corner walks from the cell's (0, 0, 0) to its (1, 1, 1) along the axes in
    decreasing fraction order, ties in r, g, b order, adding each fraction
    times its step), then the `strength` lerp from the stored value."""
    n = F16_LOOKUP_SIZE
    strength = params.get('strength', 1.0)

    def run(rgb):
        cell = []
        for v in rgb:
            t = min(max(v, 0.0), 1.0) * (n - 1)
            i = min(int(math.floor(t)), n - 2)
            cell.append((i, t - i))
        at = [c[0] for c in cell]
        previous = f16_lookup_entry(*[i / (n - 1) for i in at])
        mapped = list(previous)
        for axis in sorted(range(3), key=lambda a: -cell[a][1]):
            at[axis] += 1
            here = f16_lookup_entry(*[i / (n - 1) for i in at])
            mapped = [m + cell[axis][1] * (h - p) for m, h, p in zip(mapped, here, previous)]
            previous = here
        return [v + (m - v) * strength for v, m in zip(rgb, mapped)]
    return run


def f16_curves(params):
    identity = [[0, 0], [255, 255]]
    master = f16_natural(params.get('rgb', identity))
    channels = [f16_natural(params.get(k, identity)) for k in ('r', 'g', 'b')]
    return lambda rgb: [master(channels[c](rgb[c] * 255.0)) / 255.0 for c in range(3)]


F16_SETS = {
    'levels': ({'black': 20 / 255, 'white': 230 / 255, 'gamma': 1.4,
                'output_black': 10 / 255, 'output_white': 245 / 255, 'r': {'white': 220 / 255}},
               {'black': 0.05, 'white': 0.8, 'gamma': 0.6}),
    'black_and_white': ({'reds': 1.2, 'blues': -0.5}, {}),
    'channel_mixer': ({'red': {'r': 1, 'g': 0, 'b': 0, 'constant': 0.1}},
                      {'monochrome': True, 'gray': {'r': 0.4, 'g': 0.4, 'b': 0.2}}),
    'photo_filter': ({'color': '#ec8a00', 'density': 1.0, 'preserve_luminosity': False},
                     {'color': '#ec8a00', 'density': 0.25, 'preserve_luminosity': True}),
    'bcs': ({'brightness': 20 / 255}, {'contrast': 3 / 7, 'saturation': 0.25}),
    'exposure': ({'exposure': 0.5, 'offset': 0.02, 'gamma': 0.8},
                 {'exposure': -1.0, 'offset': -0.05, 'gamma': 1.3}),
    'curves': ({'rgb': [[0, 0], [64, 48], [192, 214], [255, 255]]},
               {'rgb': [[0, 30], [192, 255]], 'r': [[64, 0], [192, 255]],
                'g': [[0, 10], [100, 160], [200, 180], [255, 250]]}),
    'hue_saturation': ({'colorize': True, 'colorize_hue': 310, 'colorize_saturation': 0.8,
                        'colorize_lightness': -0.65},
                       {'lightness': 0.33}),
    # take5's T1 (preserve off) and P7 (preserve on).
    'color_balance': ({'shadows': {'cyan_red': -0.4, 'magenta_green': 0.3, 'yellow_blue': 0.0},
                       'midtones': {'cyan_red': 0.2, 'magenta_green': -0.5, 'yellow_blue': 0.8},
                       'highlights': {'cyan_red': 0.6, 'magenta_green': 0.0,
                                      'yellow_blue': -0.7},
                       'preserve_luminosity': False},
                      {'shadows': {'cyan_red': -0.3, 'magenta_green': 0.25, 'yellow_blue': 0.1},
                       'midtones': {'cyan_red': -0.15, 'magenta_green': 0.4,
                                    'yellow_blue': -0.6},
                       'highlights': {'cyan_red': 0.2, 'magenta_green': -0.45,
                                      'yellow_blue': 0.35},
                       'preserve_luminosity': True}),
}


def f16_two_sided(c, a):
    """The 32-bit two-sided lightness: a value already past the target held."""
    if a > 0:
        return c if c > 1.0 else c + (1.0 - c) * a
    if a < 0:
        return c if c < 0.0 else c * (1.0 + a)
    return c


def f16_hexcone(big, small, h):
    c = big - small
    hp = (h % 360.0) / 60.0  # in [0, 6): Python's % takes the divisor's sign
    i = int(hp) % 6
    f = hp - int(hp)
    up, down = small + c * f, big - c * f
    return list([(big, up, small), (down, big, small), (small, big, up), (small, down, big),
                 (up, small, big), (big, small, down)][i])


def f16_hue_of(rgb):
    """The hexcone hue in degrees, None for a grey."""
    big, small = max(rgb), min(rgb)
    c = big - small
    if c <= 0.0:
        return None
    r, g, b = rgb
    if big == r:
        h = ((g - b) / c) % 6.0
    elif big == g:
        h = (b - r) / c + 2.0
    else:
        h = (r - g) / c + 4.0
    return (60.0 * h) % 360.0


def f16_hue_saturation(params):
    """The 32-bit forms: Colorize `C32`, master Lightness per component,
    or master Hue alone, the hexcone rotation by a shift on the
    1/24576-turn grid truncated toward zero (take4's `s4096t`)."""
    if params.get('hue'):
        d = params['hue']
        shift = int(d * 24576.0 / 360.0) * 360.0 / 24576.0  # int() truncates toward 0

        def rotate(rgb):
            h = f16_hue_of(rgb)
            if h is None:
                return list(rgb)
            return f16_hexcone(max(rgb), min(rgb), h + shift)
        return rotate
    if params.get('colorize'):
        h, s = params.get('colorize_hue', 0.0), params.get('colorize_saturation', 0.25)
        a = params.get('colorize_lightness', 0.0)

        def colorize(rgb):
            y = f16_two_sided((max(rgb) + min(rgb)) / 2.0, a)
            c = s * (1.0 - abs(2.0 * y - 1.0)) * (1.0 - abs(a))
            return f16_hexcone(y + c / 2.0, y - c / 2.0, h)
        return colorize
    a = params.get('lightness', 0.0)
    return lambda rgb: [f16_two_sided(v, a) for v in rgb]


def f16_color_balance(params):
    """The 32-bit form, as `scripts/photoshop-probe/color_balance_take6.py` pins
    it (take6): whole units u = round(100 v) half away from zero; per channel
    a Levels with b = max(0, -S), w = 255 - max(0, H), t = M + trunc((S +
    H)/2) (Preserve Luminosity off) or b = max(S) - S, w = 255 - (H -
    min(H)), t = M - trunc((max(M) + min(M))/2) (on); gamma r2(2^(t/100)),
    the cubic Hermite toe below x0 = 2 2^(gamma/(p - 1)) for gamma > 1; and
    beyond [0, 1] the 4096-stage end slopes `stages`: 1 + (x - 1) hi above,
    x lo below."""
    keys = ('cyan_red', 'magenta_green', 'yellow_blue')

    def units(tone):
        block = params.get(tone) or {}
        return [int(math.copysign(math.floor(abs(100.0 * block.get(k, 0.0)) + 0.5),
                                  block.get(k, 0.0))) for k in keys]
    s, m, h = units('shadows'), units('midtones'), units('highlights')
    if params.get('preserve_luminosity', True):
        mid = max(m) + min(m)
        bwt = [(max(s) - s[c], 255 - (h[c] - min(h)), m[c] - math.trunc(mid / 2))
               for c in range(3)]
    else:
        bwt = [(max(0, -s[c]), 255 - max(0, h[c]), m[c] + math.trunc((s[c] + h[c]) / 2))
               for c in range(3)]

    def rnd_ratio(n, d):
        return (2 * n + d) // (2 * d)

    def channel(b, w, t):
        gamma = math.floor(100.0 * 2.0 ** (t / 100.0) + 0.5) / 100.0
        p = 1.0 / gamma

        def g(y):
            if gamma > 1.0:
                x0 = 2.0 * 2.0 ** (gamma / (p - 1.0))
                if y < x0:
                    s0, p1, m1 = 2.0 ** gamma, x0 ** p, p * x0 ** (p - 1.0)
                    u = y / x0
                    return ((u ** 3 - 2 * u * u + u) * x0 * s0 + (-2 * u ** 3 + 3 * u * u) * p1
                            + (u ** 3 - u * u) * x0 * m1)
            return y ** p
        if gamma == 1.0:
            hi = rnd_ratio(2040, w - b) / 8.0 if w == 255 else 0.0
            lo = rnd_ratio(2040, w - b) / 8.0 if b == 0 else 0.0
        else:
            def stage(k):
                return min(max(rnd_ratio(255 * k - 4096 * b, w - b), 0), 4096)

            def level(n):
                return math.floor(32768.0 * g(n / 4096.0) + 0.5)
            hi = (level(4096) - level(stage(4095))) / 8.0
            lo = (level(stage(1)) - level(stage(0))) / 8.0

        def f(x):
            if x > 1.0:
                return 1.0 + (x - 1.0) * hi
            if x < 0.0:
                return x * lo
            return g(min(max((x - b / 255.0) * 255.0 / (w - b), 0.0), 1.0))
        return f
    curves = [channel(*v) for v in bwt]
    return lambda rgb: [curves[c](rgb[c]) for c in range(3)]


def f16_oracle(op, params, icc):
    if op == 'levels':
        return f16_levels(params)
    if op == 'black_and_white':
        return f16_black_and_white(params)
    if op == 'channel_mixer':
        return f16_mixer(params)
    if op == 'photo_filter':
        return f16_photo_filter(params, icc)
    if op == 'exposure':
        return f16_exposure(params)
    if op == 'curves' and any(k in params for k in ('rgb_map', 'r_map', 'g_map', 'b_map')):
        return f16_curves_map(params)
    if op == 'curves':
        return f16_curves(params)
    if op == 'hue_saturation':
        return f16_hue_saturation(params)
    if op == 'color_balance':
        return f16_color_balance(params)
    if op == 'color_lookup':
        return f16_color_lookup(params)
    return f16_bcs(params)


def f16_reports(want, got):
    """Every key `want` names is reported with its value (nested objects
    likewise; a reply may add the defaults it filled in)."""
    if isinstance(want, dict):
        return isinstance(got, dict) and all(f16_reports(v, got.get(k)) for k, v in want.items())
    if isinstance(want, bool) or not isinstance(want, (int, float)):
        return want == got
    return isinstance(got, (int, float)) and abs(want - got) <= 1e-9


def gate_f16(app, folder, report):
    base = scene()
    icc = read_float_tiff(FIXTURE)['icc']
    rows, detail = {}, dict(tolerance=T_REL, ops={})

    def check(samples, oracle):
        worst, failures = 0.0, []
        for i in range(0, len(base), 4):
            want = oracle(base[i:i + 3]) + [base[i + 3]]
            got = samples[i:i + 4]
            for g, w in zip(got, want):
                worst = max(worst, abs(g - w) / max(1.0, abs(w)))
            if not f14_close(got, want):
                failures.append(dict(pixel=i // 4, got=got, want=want))
        return not failures, worst, failures[:3]

    for op, (first, second) in F16_SETS.items():
        copy = folder / f'f16-{op}.tiff'
        copy.write_bytes(FIXTURE.read_bytes())
        document = app.open(copy)
        app.call('add_adjustment_layer', document_id=document, op=op, params=first)
        added = export32(app, document, folder / f'f16-{op}-added.tiff')['samples']
        ok_added, worst_added, bad_added = check(added, f16_oracle(op, first, icc))
        app.call('edit_adjustment_layer', document_id=document, layer=1, params=second)
        edited = export32(app, document, folder / f'f16-{op}-edited.tiff')['samples']
        ok_edited, worst_edited, bad_edited = check(edited, f16_oracle(op, second, icc))
        native = folder / f'f16-{op}.rz'
        app.call('save_copy', document_id=document, path=str(native), format='rz')
        reopened = app.open(native)
        after = export32(app, reopened, folder / f'f16-{op}-reopened.tiff')['samples']
        layers = app.call('get_document', document_id=reopened).get('layers') or []
        reported = [row.get('adjustment') for row in layers if row.get('adjustment')]
        params_reported = (len(reported) == 1 and reported[0].get('op') == op
                           and f16_reports(second, reported[0].get('params')))
        detail['ops'][op] = dict(added_worst=worst_added, added_failures=bad_added,
                                 edited_worst=worst_edited, edited_failures=bad_edited,
                                 reopened_bit_exact=bits_equal(after, edited),
                                 reported=reported)
        rows[op] = (ok_added and ok_edited and bits_equal(after, edited) and params_reported)

    hue = {'hue': -55}
    copy = folder / 'f16-hue_saturation-hue.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    document = app.open(copy)
    app.call('add_adjustment_layer', document_id=document, op='hue_saturation', params=hue)
    turned = export32(app, document, folder / 'f16-hue_saturation-hue-added.tiff')['samples']
    ok_hue, worst_hue, bad_hue = check(turned, f16_hue_saturation(hue))
    detail['hue_saturation_hue'] = dict(params=hue, worst=worst_hue, failures=bad_hue)
    rows['hue_saturation_hue'] = ok_hue

    copy = folder / 'f16-curves_map.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    document = app.open(copy)
    app.call('add_adjustment_layer', document_id=document, op='curves', params=F16_CURVES_MAP)
    mapped = export32(app, document, folder / 'f16-curves_map-added.tiff')['samples']
    ok_map, worst_map, bad_map = check(mapped, f16_oracle('curves', F16_CURVES_MAP, icc))
    detail['curves_map'] = dict(worst=worst_map, failures=bad_map)
    rows['curves_map'] = ok_map

    cube = f16_lookup_cube(folder / 'f16-cross_5.cube')
    copy = folder / 'f16-color_lookup.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    document = app.open(copy)
    app.call('add_adjustment_layer', document_id=document, op='color_lookup',
             params={'file': cube})
    looked = export32(app, document, folder / 'f16-color_lookup-added.tiff')['samples']
    ok_look, worst_look, bad_look = check(looked, f16_oracle('color_lookup', {}, icc))
    half = {'strength': 0.5}
    app.call('edit_adjustment_layer', document_id=document, layer=1, params=half)
    lerped = export32(app, document, folder / 'f16-color_lookup-edited.tiff')['samples']
    ok_half, worst_half, bad_half = check(lerped, f16_oracle('color_lookup', half, icc))
    native = folder / 'f16-color_lookup.rz'
    app.call('save_copy', document_id=document, path=str(native), format='rz')
    reopened = app.open(native)
    after = export32(app, reopened, folder / 'f16-color_lookup-reopened.tiff')['samples']
    layers = app.call('get_document', document_id=reopened).get('layers') or []
    reported = [row.get('adjustment') for row in layers if row.get('adjustment')]
    kept = (len(reported) == 1 and reported[0].get('op') == 'color_lookup'
            and f16_reports({'kind': '3d', 'size': F16_LOOKUP_SIZE, 'strength': 0.5},
                            reported[0].get('params')))
    detail['color_lookup'] = dict(added_worst=worst_look, added_failures=bad_look,
                                  edited_worst=worst_half, edited_failures=bad_half,
                                  reopened_bit_exact=bits_equal(after, lerped),
                                  reported=reported)
    rows['color_lookup'] = ok_look and ok_half and bits_equal(after, lerped) and kept

    one_d = folder / 'f16-gain_half_1d.cube'
    one_d.write_text('LUT_1D_SIZE 2\n0.0 0.0 0.0\n0.5 0.5 0.5\n')
    copy = folder / 'f16-color_lookup-1d.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    document = app.open(copy)
    # A 1D table runs on the stored values — this one maps
    # each channel to 0.5·clamp(v, 0, 1).
    reply = app.call('add_adjustment_layer', document_id=document,
                     op='color_lookup', params={'file': str(one_d)})
    one_d_out = export32(app, document, folder / 'f16-color_lookup-1d-added.tiff')['samples']
    want_1d = []
    for i in range(0, len(base), 4):
        want_1d.extend([0.5 * min(max(v, 0.0), 1.0) for v in base[i:i + 3]] + [base[i + 3]])
    ok_1d, worst_1d = close(one_d_out, want_1d, T_REL)
    detail['color_lookup_1d'] = dict(reply=reply, worst=worst_1d)
    rows['color_lookup_1d_admitted'] = ok_1d

    copy = folder / 'f16-hue-band.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    document = app.open(copy)
    # A hue band takes the display-scaled form (its oracle
    # is the core's `float_display_adjustments`); admitted, finite, changed.
    reply = app.call('add_adjustment_layer', document_id=document,
                     op='hue_saturation', params={'bands': {'reds': {'hue': 20}}})
    banded = export32(app, document, folder / 'f16-hue-band.tiff.out.tiff')['samples']
    detail['hue_band'] = dict(reply=reply)
    rows['hue_saturation_band_admitted'] = (all(math.isfinite(v) for v in banded)
                                            and not bits_equal(banded, base))
    detail['rows'] = rows
    record(report, 'F16 the 32-bit adjustments render, edit and persist',
           all(rows.values()), detail)


F17_OPS = ('levels', 'black_and_white', 'channel_mixer', 'photo_filter', 'exposure', 'curves',
           'color_balance')
# The modern Brightness/Contrast, fixed before the first measurement.
F17_MODERN = {'brightness': 0.3, 'contrast': 0.4, 'saturation': -0.2, 'legacy': False}


def f17_bcs_modern(params):
    """The modern curve on the stored values: a channel inside [0, 1]
    takes the brightness power then the contrast S-curve about mid-grey, a
    channel outside passes both unchanged, and the luma-anchored saturation
    scale that follows is unclamped."""
    gamma = 2.0 ** (-1.5 * params.get('brightness', 0.0))
    power = 2.0 ** (1.5 * params.get('contrast', 0.0))
    scale = 1.0 + params.get('saturation', 0.0)

    def run(rgb):
        o = []
        for v in rgb:
            if 0.0 <= v <= 1.0:
                v = v ** gamma
                v = 0.5 * (2 * v) ** power if v < 0.5 else 1 - 0.5 * (2 * (1 - v)) ** power
            o.append(v)
        a = 0.2126 * o[0] + 0.7152 * o[1] + 0.0722 * o[2]
        return [a + (v - a) * scale for v in o]
    return run


def gate_f17(app, folder, report):
    base = scene()
    icc = read_float_tiff(FIXTURE)['icc']
    rows, detail = {}, dict(tolerance=T_REL, ops={})

    def check(samples, oracle):
        worst, failures = 0.0, []
        for i in range(0, len(base), 4):
            want = oracle(base[i:i + 3]) + [base[i + 3]]
            got = samples[i:i + 4]
            for g, w in zip(got, want):
                worst = max(worst, abs(g - w) / max(1.0, abs(w)))
            if not f14_close(got, want) or not bits_equal(got[3:], want[3:]):
                failures.append(dict(pixel=i // 4, got=got, want=want))
        return not failures, worst, failures[:3]

    for op in F17_OPS:
        params = F16_SETS[op][0]
        copy = folder / f'f17-{op}.tiff'
        copy.write_bytes(FIXTURE.read_bytes())
        document = app.open(copy)
        before = export32(app, document, folder / f'f17-{op}-before.tiff')['samples']
        # Levels keeps its flat arguments (Image ▸ Adjustments ▸ Levels…'s
        # recorded shape); every other op takes a params object.
        arguments = dict(params) if op == 'levels' else dict(params=params)
        reply = app.call('apply_filter', document_id=document, layer=0, filter=op, **arguments)
        applied = export32(app, document, folder / f'f17-{op}-applied.tiff')['samples']
        ok, worst, bad = check(applied, f16_oracle(op, params, icc))
        app.call('undo', document_id=document)
        undone = export32(app, document, folder / f'f17-{op}-undone.tiff')['samples']
        detail['ops'][op] = dict(reply=reply, worst=worst, failures=bad,
                                 undo_bit_exact=bits_equal(undone, before))
        rows[op] = (ok and reply.get('changed') is True
                    and reply.get('storage') == 'rgba32_float' and bits_equal(undone, before))

    copy = folder / 'f17-curves_map.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    document = app.open(copy)
    before = export32(app, document, folder / 'f17-curves_map-before.tiff')['samples']
    reply = app.call('apply_filter', document_id=document, layer=0, filter='curves',
                     params=F16_CURVES_MAP)
    applied = export32(app, document, folder / 'f17-curves_map-applied.tiff')['samples']
    ok, worst, bad = check(applied, f16_oracle('curves', F16_CURVES_MAP, icc))
    app.call('undo', document_id=document)
    undone = export32(app, document, folder / 'f17-curves_map-undone.tiff')['samples']
    detail['curves_map'] = dict(reply=reply, worst=worst, failures=bad,
                                undo_bit_exact=bits_equal(undone, before))
    rows['curves_map'] = (ok and reply.get('changed') is True
                          and reply.get('storage') == 'rgba32_float'
                          and bits_equal(undone, before))

    cube = f16_lookup_cube(folder / 'f17-cross_5.cube')
    copy = folder / 'f17-color_lookup.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    document = app.open(copy)
    before = export32(app, document, folder / 'f17-color_lookup-before.tiff')['samples']
    reply = app.call('apply_filter', document_id=document, layer=0, filter='color_lookup',
                     params={'file': cube})
    applied = export32(app, document, folder / 'f17-color_lookup-applied.tiff')['samples']
    ok, worst, bad = check(applied, f16_oracle('color_lookup', {}, icc))
    app.call('undo', document_id=document)
    undone = export32(app, document, folder / 'f17-color_lookup-undone.tiff')['samples']
    detail['color_lookup'] = dict(reply=reply, worst=worst, failures=bad,
                                  undo_bit_exact=bits_equal(undone, before))
    rows['color_lookup'] = (ok and reply.get('changed') is True
                            and reply.get('storage') == 'rgba32_float'
                            and bits_equal(undone, before))

    # Brightness/Contrast applies in both forms, flat as `adjust`.
    for name, params, oracle in (
            ('adjust_legacy', F16_SETS['bcs'][0], f16_bcs(F16_SETS['bcs'][0])),
            ('adjust_modern', F17_MODERN, f17_bcs_modern(F17_MODERN))):
        copy = folder / f'f17-{name}.tiff'
        copy.write_bytes(FIXTURE.read_bytes())
        document = app.open(copy)
        before = export32(app, document, folder / f'f17-{name}-before.tiff')['samples']
        reply = app.call('apply_filter', document_id=document, layer=0, filter='adjust',
                         **params)
        applied = export32(app, document, folder / f'f17-{name}-applied.tiff')['samples']
        ok, worst, bad = check(applied, oracle)
        app.call('undo', document_id=document)
        undone = export32(app, document, folder / f'f17-{name}-undone.tiff')['samples']
        detail[name] = dict(reply=reply, worst=worst, failures=bad,
                            undo_bit_exact=bits_equal(undone, before))
        rows[name] = (ok and reply.get('changed') is True
                      and reply.get('storage') == 'rgba32_float' and bits_equal(undone, before))

    copy = folder / 'f17-refused.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    document = app.open(copy)
    before = export32(app, document, folder / 'f17-refused-before.tiff')['samples']
    # Vibrance applies (display-scaled); Invert stays refused.
    for name, filter_name, arguments in (
            ('invert', 'invert', dict()),):
        message = app.call('apply_filter', fail=True, document_id=document, layer=0,
                           filter=filter_name, **arguments)
        detail[f'{name}_refusal'] = message
        rows[f'{name}_refuses'] = (
            f'apply_filter "{filter_name}" is unavailable in a 32-bit float document' in message)
    after = export32(app, document, folder / 'f17-refused-after.tiff')['samples']
    rows['refusals_change_nothing'] = bits_equal(after, before)
    detail['rows'] = rows
    record(report, 'F17 destructive adjustments at 32 bits match the layer forms and undo',
           all(rows.values()), detail)


def f18_oracle(px, opacity=0.6):
    """The F18 group over its own backdrop pixel `px` (straight RGBA): copy 1
    (Normal, the pixel itself) then copy 2 (Multiply, the pixel itself) over
    a COPY of the backdrop by the W3C step, then the group's opacity
    interpolates that result back towards the backdrop, premultiplied."""
    cb, a = list(px[:3]), px[3]

    def over(bg, ab, cs, sa, blend):
        ao = sa + ab * (1.0 - sa)
        if ao == 0.0:
            return [0.0, 0.0, 0.0], 0.0
        return [(sa * (1.0 - ab) * cs[c] + sa * ab * blend(bg[c], cs[c])
                 + (1.0 - sa) * ab * bg[c]) / ao for c in range(3)], ao

    inner, alpha = over(cb, a, cb, a, lambda b, s: s)
    inner, alpha = over(inner, alpha, cb, a, lambda b, s: b * s)
    out_alpha = a + opacity * (alpha - a)
    if out_alpha <= 0.0:
        return [0.0, 0.0, 0.0, 0.0]
    return [(cb[c] * a + opacity * (inner[c] * alpha - cb[c] * a)) / out_alpha
            for c in range(3)] + [out_alpha]


def gate_f18(app, folder, report):
    copy = folder / 'f18-pass-through.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    document = app.open(copy)
    base = scene()
    app.call('duplicate_layer', document_id=document, index=0)
    app.call('duplicate_layer', document_id=document, index=1)
    app.call('set_layer_properties', document_id=document, index=2, blend_mode='Multiply')
    grouped = app.call('group_layers', document_id=document, layers=[1, 2], name='Pass')
    group = grouped.get('group')
    app.call('set_layer_properties', document_id=document, index=group, opacity=0.6)
    rows = {}
    detail = dict(tolerance=T_REL, group=group)

    def group_row(doc):
        state = app.call('get_document', document_id=doc)
        return next((row for row in state.get('layers') or [] if row.get('index') == group),
                    {})

    row = group_row(document)
    detail['row'] = dict(kind=row.get('kind'), blend_mode=row.get('blend_mode'),
                         opacity=row.get('opacity'))
    rows['minted_pass_through'] = (row.get('kind') == 'group'
                                   and row.get('blend_mode') == 'Pass Through')
    got = export32(app, document, folder / 'f18.tiff')['samples']
    failures = []
    for i in range(WIDTH * HEIGHT):
        want = f18_oracle(base[i * 4:i * 4 + 4])
        out = got[i * 4:i * 4 + 4]
        if not f14_close(out, want):
            failures.append(dict(i=i, got=out, want=want, base=base[i * 4:i * 4 + 4]))
    detail['composite'] = dict(failures=len(failures), first=failures[:2])
    rows['composite'] = not failures

    isolated = app.call('set_layer_properties', document_id=document, index=group,
                        blend_mode='Normal')
    switched = app.call('set_layer_properties', document_id=document, index=group,
                        blend_mode='Pass Through')
    again = export32(app, document, folder / 'f18-set.tiff')['samples']
    detail['set'] = dict(isolated=isolated, switched=switched,
                         blend_mode=group_row(document).get('blend_mode'),
                         bit_exact=bits_equal(again, got))
    rows['set'] = (detail['set']['blend_mode'] == 'Pass Through'
                   and detail['set']['bit_exact'])

    native = folder / 'f18.rz'
    app.call('save_copy', document_id=document, path=str(native), format='rz')
    reopened = app.open(native)
    after = export32(app, reopened, folder / 'f18-reopened.tiff')['samples']
    row = group_row(reopened)
    detail['reopened'] = dict(kind=row.get('kind'), blend_mode=row.get('blend_mode'),
                              bit_exact=bits_equal(after, got))
    rows['reopened'] = (detail['reopened']['bit_exact'] and row.get('kind') == 'group'
                        and row.get('blend_mode') == 'Pass Through')
    detail['rows'] = rows
    record(report, 'F18 a Pass Through group composites, sets and persists at 32 bits',
           all(rows.values()), detail)


F20_F, F20_S = (46, 128, 200), (188, 64, 16)
F20_U16 = 1.0 / 65535.0
# Photoshop's read-back codes (take2 PSH, NR) against 255 × this file's IEC
# curve: Photoshop's own sRGB curve sits up to 1.2e-3 codes from the analytic
# one (255·decode(46/255) = 6.96683 against its 6.9679), so the receipt
# anchors hold at 2e-3 codes; the 1e-12 checks are against the IEC curve.
F20_RECEIPT = 2e-3


def f20_decode(v):
    """The IEC 61966-2-1 sRGB decode, written out here."""
    return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4


def f20_encode(v):
    return 12.92 * v if v <= 0.0031308 else 1.055 * v ** (1.0 / 2.4) - 0.055


def f20_columns(icc):
    """The D50 colorant matrix (row-major, columns rXYZ/gXYZ/bXYZ) out of an
    ICC profile's own tags."""
    tags = icc_tags(icc)
    columns = []
    for signature in (b'rXYZ', b'gXYZ', b'bXYZ'):
        data = tags[signature]
        assert data[:4] == b'XYZ ', data[:4]
        columns.append([struct.unpack_from('>i', data, 8 + 4 * k)[0] / 65536.0
                        for k in range(3)])
    return [[columns[c][i] for c in range(3)] for i in range(3)]


def f20_png_icc(path):
    """The profile a PNG carries in its `iCCP` chunk."""
    data = Path(path).read_bytes()
    offset = 8
    while offset < len(data):
        size = struct.unpack_from('>I', data, offset)[0]
        kind = data[offset + 4:offset + 8]
        if kind == b'iCCP':
            body = data[offset + 8:offset + 8 + size]
            name_end = body.index(b'\0')
            return zlib.decompress(body[name_end + 2:])
        offset += size + 12
    raise AssertionError(f'{path} carries no iCCP')


def f20_rows(app, document):
    return {row.get('index'): row for row in
            app.call('get_document', document_id=document).get('layers') or []}


def f20_fresh(folder, label):
    copy = folder / f'f20-{label}.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    return copy


def f20_box(samples, x0, y0, x1, y1, width=WIDTH):
    """The RGBA samples of the half-open box, row by row."""
    out = []
    for y in range(y0, y1):
        for x in range(x0, x1):
            i = (y * width + x) * 4
            out.append(samples[i:i + 4])
    return out


def f20_interior(pixels, want, tolerance, alpha_floor=None):
    """The worst relative error of `pixels` (alpha at or above
    `alpha_floor` when given) against `want`, and how many were read."""
    worst, count = 0.0, 0
    for px in pixels:
        if alpha_floor is not None and px[3] < alpha_floor:
            continue
        count += 1
        for c in range(3):
            worst = max(worst, abs(px[c] - want[c]) / max(abs(want[c]), 1.0))
    return worst <= tolerance and count >= 4, dict(worst=worst, count=count)


def gate_f20(app, folder, report):
    rows = {}
    detail = dict(tolerance=T_REL, receipt='doctored-fixtures/photoshop/float-artwork-473/take2')
    fixture_icc = read_float_tiff(FIXTURE)['icc']
    tables = icc_trc_tables(fixture_icc)
    values = forward_tables(tables)
    f_raw = [v / 255.0 for v in F20_F]
    s_raw = [v / 255.0 for v in F20_S]

    # The sRGB profile a 16-bit sRGB document carries, read off its export.
    sixteen = app.call('new_document', width=WIDTH, height=HEIGHT, depth=16, profile='srgb',
                       background='transparent')['document']['id']
    probe = folder / 'f20-srgb.png'
    app.call('export_image', document_id=sixteen, path=str(probe),
             settings={'format': 'png', 'bit_depth': 16}, overwrite=True)
    matrix = f16_mul(f16_inv(f20_columns(fixture_icc)), f20_columns(f20_png_icc(probe)))
    detail['matrix'] = matrix

    def oracle(raw):
        return f16_apply(matrix, raw)

    # (a) a shape authored at 32 bits: its numbers ARE linear light.
    document = app.open(f20_fresh(folder, 'a'))
    shape = app.call('add_shape_layer', document_id=document, kind='rect', x=8, y=8, w=24,
                     h=12, fill_rgba=f_raw + [1.0])
    layer = shape['layer']
    row = f20_rows(app, document)[layer].get('shape') or {}
    detail['a_shape'] = row
    rows['a_reported'] = (row.get('fill_rgba') == f_raw + [1.0]
                          and row.get('ink') == 'linear_srgb')
    hex_shape = app.call('add_shape_layer', document_id=document, kind='rect', x=34, y=8,
                         w=10, h=12, fill='#2E80C8FF')
    composite = export32(app, document, folder / 'f20-a.tiff')['samples']
    want = oracle(f_raw)
    rows['a_interior'], detail['a_interior'] = f20_interior(
        f20_box(composite, 10, 10, 30, 18), want, T_REL)
    rows['a_hex'], detail['a_hex'] = f20_interior(
        f20_box(composite, 36, 10, 42, 18), want, T_REL)

    # (b) C-same: the alpha of a fractional box against a 16-bit twin.
    alone = app.open(f20_fresh(folder, 'b'))
    app.call('set_layer_properties', document_id=alone, index=0, visible=False)
    box = dict(kind='rect', x=8, y=8, w=20.5, h=10.25, fill_rgba=f_raw + [1.0])
    app.call('add_shape_layer', document_id=alone, **box)
    float_alpha = [px[3] for px in f20_box(export32(app, alone, folder / 'f20-b.tiff')['samples'],
                                           4, 4, 34, 22)]
    twin = app.call('new_document', width=WIDTH, height=HEIGHT, depth=16, profile='srgb',
                    background='transparent')['document']['id']
    app.call('add_shape_layer', document_id=twin, **box)
    twin_png = folder / 'f20-b16.png'
    app.call('export_image', document_id=twin, path=str(twin_png),
             settings={'format': 'png', 'bit_depth': 16}, overwrite=True)
    _, _, twin_pixels = read_png(twin_png.read_bytes())
    twin_alpha = [twin_pixels[y * WIDTH + x][3] / 65535.0
                  for y in range(4, 22) for x in range(4, 34)]
    worst = max(abs(a - b) for a, b in zip(float_alpha, twin_alpha))
    partial = sum(1 for a in float_alpha if 0.0 < a < 1.0)
    detail['b_alpha'] = dict(worst=worst, partial=partial)
    rows['b_coverage'] = worst <= F20_U16 and partial >= 2

    # (c) text, re-edit, transform, persistence and Rasterize Artwork.
    text = app.call('add_text_layer', document_id=document, text='HI', x=4, y=20, size=10,
                    rgba=s_raw + [1.0])
    edited = app.call('edit_shape_layer', document_id=document, layer=layer, stroke_width=3)
    state = f20_rows(app, document)
    detail['c'] = dict(text=state[text['layer']].get('text', {}).get('ink'),
                       edited=edited.get('ok'))
    rows['c_admitted'] = ('text' in state[text['layer']] and 'shape' in state[layer]
                          and 'shape' in state[hex_shape['layer']])
    app.call('transform_layer', document_id=document, layer=layer, scale=2, sampler='bilinear')
    rows['c_transform'] = 'shape' in f20_rows(app, document)[layer]
    before = export32(app, document, folder / 'f20-c.tiff')['samples']
    native = folder / 'f20-c.rz'
    app.call('save_copy', document_id=document, path=str(native), format='rz')
    reopened = app.open(native)
    again = export32(app, reopened, folder / 'f20-c-reopened.tiff')['samples']
    rows['c_reopened'] = bits_equal(again, before) and 'shape' in f20_rows(app, reopened)[layer]
    app.call('rasterize_artwork', document_id=reopened, layer=layer)
    flat = export32(app, reopened, folder / 'f20-c-rasterized.tiff')['samples']
    rows['c_rasterized'] = (bits_equal(flat, before)
                            and 'shape' not in f20_rows(app, reopened)[layer])

    # (d) 16-bit layers pasted in: rewritten so they look the same.
    source = app.call('new_document', width=WIDTH, height=HEIGHT, depth=16, profile='srgb',
                      background='transparent')['document']['id']
    text16 = app.call('add_text_layer', document_id=source, text='HI', x=2, y=4, size=16,
                      rgba=s_raw + [1.0])['layer']
    shape16 = app.call('add_shape_layer', document_id=source, kind='rect', x=30, y=6, w=14,
                       h=10, fill_rgba=f_raw + [1.0])['layer']
    target = app.open(f20_fresh(folder, 'd'))
    app.call('set_layer_properties', document_id=target, index=0, visible=False)
    landed = {}
    for kind, index in (('text', text16), ('shape', shape16)):
        copied = app.call('copy', document_id=source, layer=index)
        pasted = app.call('paste', document_id=target, placement='in_place')
        detail['d_' + kind] = dict(copied=copied.get('editable'), pasted=pasted.get('editable'))
        landed[kind] = pasted['layer']
    state = f20_rows(app, target)
    text_row = state[landed['text']].get('text') or {}
    shape_row = state[landed['shape']].get('shape') or {}
    f_dec = [f20_decode(v) for v in f_raw]
    s_dec = [f20_decode(v) for v in s_raw]
    near = lambda got, want, bound: (got is not None and len(got) == len(want)
                                     and all(abs(g - w) <= bound for g, w in zip(got, want)))
    rows['d_kinds'] = (text_row.get('ink') == 'linear_srgb'
                       and shape_row.get('ink') == 'linear_srgb')
    rows['d_numbers'] = (near(text_row.get('rgba'), s_dec + [1.0], 1e-12)
                         and near(shape_row.get('fill_rgba'), f_dec + [1.0], 1e-12)
                         and near([255 * v for v in (shape_row.get('fill_rgba') or [])[:3]],
                                  [6.9679, 55.0442, 147.2818], F20_RECEIPT))
    detail['d_numbers'] = dict(text=text_row.get('rgba'), shape=shape_row.get('fill_rgba'))

    def interiors(path, tolerance):
        samples = export32(app, target, folder / path)['samples']
        shape_ok, shape_detail = f20_interior(f20_box(samples, 32, 8, 42, 14),
                                              oracle(f_dec), tolerance)
        text_ok, text_detail = f20_interior(f20_box(samples, 0, 0, 30, HEIGHT),
                                            oracle(s_dec), tolerance, alpha_floor=0.999)
        return shape_ok and text_ok, dict(shape=shape_detail, text=text_detail)

    rows['d_interiors'], detail['d_interiors'] = interiors('f20-d.tiff', F20_U16)
    app.call('edit_shape_layer', document_id=target, layer=landed['shape'], stroke_width=3)
    app.call('edit_text_layer', document_id=target, layer=landed['text'], size=18)
    rows['d_rerendered'], detail['d_rerendered'] = interiors('f20-d-edited.tiff', T_REL)

    # (e) Preserve narrowing to 16 bits: the inverse rewrite. The shape's
    # interior after the ×2 transform about its centre (20, 14) covers
    # x 10…30, y 4…18 with nothing above it.
    float_shape = f20_box(before, 10, 4, 30, 18)
    app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')
    state = f20_rows(app, document)
    narrowed = state[layer].get('shape') or {}
    narrowed_text = state[text['layer']].get('text') or {}
    f_enc = [f20_encode(v) for v in f_raw]
    detail['e'] = dict(shape=narrowed, text_ink=narrowed_text.get('ink'))
    rows['e_kept'] = (narrowed.get('ink') == 'srgb' and narrowed_text.get('ink') == 'srgb'
                      and near(narrowed.get('fill_rgba'), f_enc + [1.0], 1e-12)
                      and near([255 * v for v in (narrowed.get('fill_rgba') or [])[:3]],
                               [117.7647, 187.8444, 229.1000], F20_RECEIPT))
    app.call('edit_shape_layer', document_id=document, layer=layer, stroke_width=4)
    png = folder / 'f20-e.png'
    app.call('export_image', document_id=document, path=str(png),
             settings={'format': 'png', 'bit_depth': 16}, overwrite=True)
    _, _, coded = read_png(png.read_bytes())
    # F7's model of a linear-light 16-bit export: the stored code (the float
    # appearance encoded through the fixture's curve, rounded once) decoded
    # through that curve and re-encoded once.
    worst = 0
    for k, (y, x) in enumerate((y, x) for y in range(4, 18) for x in range(10, 30)):
        for c in range(3):
            stored = round(encode_through(values[c], float_shape[k][c]) * 65535.0)
            decoded = curve_eval(stored / 65535.0, tables[c])
            expected = round(encode_through(values[c], decoded) * 65535.0)
            worst = max(worst, abs(coded[y * WIDTH + x][c] - expected))
    detail['e_codes'] = worst
    rows['e_appearance'] = worst <= T_CODE16

    # (f) above the float range refuses in-band (above white up to 65504 is
    # admitted at 32 bits).
    message = app.call('add_shape_layer', fail=True, document_id=reopened, kind='rect', x=2,
                       y=2, w=4, h=4, fill_rgba=[65505.0, 0.2, 0.2, 1.0])
    detail['f'] = message
    rows['f_above_white'] = 'requires four finite components' in message

    # (g) the 16-bit document keeps sRGB numbers and survives a reopen.
    state = f20_rows(app, source)
    rows['g_ink'] = ((state[text16].get('text') or {}).get('ink') == 'srgb'
                     and (state[shape16].get('shape') or {}).get('ink') == 'srgb')
    native16 = folder / 'f20-g.rz'
    app.call('save_copy', document_id=source, path=str(native16), format='rz')
    back = f20_rows(app, app.open(native16))
    rows['g_reopened'] = 'text' in back[text16] and 'shape' in back[shape16]
    detail['rows'] = rows
    record(report, 'F20 text and shape layers render, paste, convert and persist at 32 bits',
           all(rows.values()), detail)


def u16_fixture(folder):
    path = folder / 'u16.png'
    samples = []
    for y in range(HEIGHT):
        for x in range(WIDTH):
            samples.extend([(x * 1301) % 65536, (y * 2113) % 65536,
                            ((x + y) * 907) % 65536, 65535])
    png16(path, WIDTH, HEIGHT, samples)
    return path


def u16_state(app, folder, tag):
    path = u16_fixture(folder)
    document = app.open(path)
    native = folder / f'u16-{tag}.rz'
    export = folder / f'u16-{tag}.png'
    app.call('save_copy', document_id=document, path=str(native), format='rz')
    app.call('export_image', document_id=document, path=str(export),
             settings={'format': 'png', 'bit_depth': 16}, overwrite=True)
    return document, dict(native=digest(native), export=digest(export),
                          first_words=first_words(native, 2))


F28_RECT = (4, 4, 16, 8)


def f28_pixel(samples, x, y):
    if 0 <= x < WIDTH and 0 <= y < HEIGHT:
        return samples[(y * WIDTH + x) * 4:(y * WIDTH + x) * 4 + 4]
    return None


def f28_over(orig, c, source):
    """W3C Normal source-over of the float (straight rgba, coverage) onto the
    pixel holed to alpha a·(1−c), its RGB kept; f64."""
    ab = orig[3] * (1.0 - c)
    out = list(orig[:3]) + [ab]
    if source is not None and source[0][3] * source[1] > 0.0:
        s, fc = source
        sa = s[3] * fc
        ao = sa + ab * (1.0 - sa)
        out = [(s[k] * sa + orig[k] * ab * (1.0 - sa)) / ao for k in range(3)] + [ao]
    return out


def f28_expected(base, landed):
    """The scene with the selection rect holed and `landed(x, y)` (the float
    sample arriving there, or None) composited."""
    x0, y0, w, h = F28_RECT
    out = []
    for y in range(HEIGHT):
        for x in range(WIDTH):
            c = 1.0 if x0 <= x < x0 + w and y0 <= y < y0 + h else 0.0
            out.extend(f28_over(f28_pixel(base, x, y), c, landed(x, y)))
    return out


def f28_close(got, want):
    """Alpha within T_REL; RGB within T_REL wherever the expected alpha is
    visible (a composite export has no colour under alpha 0)."""
    worst = 0.0
    for i in range(0, len(want), 4):
        if want[i + 3] == 0.0:
            if got[i + 3] != 0.0:
                return False, float('inf')
            continue
        for k in range(4):
            worst = max(worst, abs(got[i + k] - want[i + k]) / max(1.0, abs(want[i + k])))
    return worst <= T_REL, worst


def gate_f28(app, folder, report):
    copy = folder / 'f28-scene.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    doc = app.open(copy)
    base = scene()
    x0, y0, w, h = F28_RECT
    inside = lambda x, y: x0 <= x < x0 + w and y0 <= y < y0 + h
    rows, detail = {}, dict(tolerance=T_REL, rect=list(F28_RECT))

    def select():
        app.call('select_rect', document_id=doc, x=x0, y=y0, width=w, height=h)

    # (a) the move, against the f64 oracle.
    select()
    moved = app.call('move_selected_pixels', document_id=doc, dx=9, dy=5)
    got = export32(app, doc, folder / 'f28-a.tiff')['samples']
    want = f28_expected(base, lambda x, y: (f28_pixel(base, x - 9, y - 5), 1.0)
                        if inside(x - 9, y - 5) else None)
    rows['a_move'], detail['a_worst'] = f28_close(got, want)
    detail['a_reply'] = moved

    # (b) undo is bit-exact.
    app.call('undo', document_id=doc)
    rows['b_undo'] = bits_equal(export32(app, doc, folder / 'f28-b.tiff')['samples'], base)

    # (c) a Nearest quarter turn about the box's centre is a permutation.
    select()
    turned = app.call('transform_selected_pixels', document_id=doc, rotate=90,
                      sampler='nearest')
    got = export32(app, doc, folder / 'f28-c.tiff')['samples']

    def quarter(x, y):
        sx, sy = y + 4, 19 - x
        return (f28_pixel(base, sx, sy), 1.0) if inside(sx, sy) else None
    rows['c_turn'], detail['c_worst'] = f28_close(got, f28_expected(base, quarter))
    detail['c_reply'] = turned

    # (d) Transform Again with that selection.
    again = app.call('transform_again', document_id=doc)
    rows['d_again'] = isinstance(again, dict)
    detail['d_reply'] = again

    # (e) a colour-plane target is admitted (F30 checks its pixels).
    reply = app.call('move_selected_pixels', document_id=doc, dx=1, dy=0, target='red')
    rows['e_red'] = isinstance(reply, dict) and reply.get('ok') is True
    detail['e_reply'] = reply
    detail['rows'] = rows
    record(report, "F28 Move and Free Transform of a float layer's selected pixels",
           all(rows.values()), detail)


# One colour channel of the float layer's selected pixels. The
# vacated red is the background's 0.30 / 0.59 / 0.11 luma rounded to a code,
# over 255, unconverted at 32 bits (`plane-gray-431-633/take1`: `hole32`
# `3059r`, `form32` `raw`). The fixture's profile has sRGB's primaries, so
# #0000ff is the document codes (0, 0, 255): floor(0.11 * 255 + 1/2) = 28, so
# the hole is 28/255 here and 28 * 257 = 7196 at 16 bits. The Rec. 601 floor
# this replaced reads 29 (7453), which T_REL and T_CODE16 both tell apart.
F30_RECT = (4, 4, 16, 8)
F30_BACKGROUND = '#0000ff'
F30_HOLE = 28 / 255
F30_HOLE16 = 7196


def f30_cov(base, x, y):
    """The coverage the red walk receives: the selection × the stored
    alpha, as u16."""
    x0, y0, w, h = F30_RECT
    if not (x0 <= x < x0 + w and y0 <= y < y0 + h):
        return 0.0
    return round(65535 * f28_pixel(base, x, y)[3]) / 65535


def f30_expected(base, source):
    """The scene with ONLY red moved: holed toward F30_HOLE by the scaled
    coverage, then the float sample from `source(x, y)` (a canvas point or
    None) landed source-over at its own scaled coverage; f64."""
    out = []
    for y in range(HEIGHT):
        for x in range(WIDTH):
            p = list(f28_pixel(base, x, y))
            c = f30_cov(base, x, y)
            red = p[0] + (F30_HOLE - p[0]) * c
            at = source(x, y)
            if at is not None and f30_cov(base, *at) > 0.0:
                fa = f30_cov(base, *at)
                red = red + (f28_pixel(base, *at)[0] - red) * fa
            p[0] = red
            out.extend(p)
    return out


def gate_f30(app, folder, report):
    copy = folder / 'f30-scene.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    doc = app.open(copy)
    base = scene()
    x0, y0, w, h = F30_RECT
    inside = lambda x, y: x0 <= x < x0 + w and y0 <= y < y0 + h
    rows, detail = {}, dict(tolerance=T_REL, rect=list(F30_RECT), background=F30_BACKGROUND)

    def select():
        app.call('select_rect', document_id=doc, x=x0, y=y0, width=w, height=h)

    # (a) a red move, against the f64 oracle; green, blue and alpha bit for bit.
    select()
    moved = app.call('move_selected_pixels', document_id=doc, dx=9, dy=5, target='red',
                     background=F30_BACKGROUND)
    got = export32(app, doc, folder / 'f30-a.tiff')['samples']
    want = f30_expected(base, lambda x, y: (x - 9, y - 5) if inside(x - 9, y - 5) else None)
    rows['a_move'], detail['a_worst'] = f28_close(got, want)
    rows['a_others'] = all(bits_equal(got[i + 1:i + 4], base[i + 1:i + 4])
                           for i in range(0, len(base), 4))
    rows['a_reply'] = moved.get('target') == 'red' and 'layer_bounds' in moved
    detail['a_reply'] = moved

    # (b) undo is bit-exact.
    app.call('undo', document_id=doc)
    rows['b_undo'] = bits_equal(export32(app, doc, folder / 'f30-b.tiff')['samples'], base)

    # (c) a Nearest quarter turn of the red channel about the box's centre.
    select()
    turned = app.call('transform_selected_pixels', document_id=doc, rotate=90,
                      sampler='nearest', target='red', background=F30_BACKGROUND)
    got = export32(app, doc, folder / 'f30-c.tiff')['samples']

    def quarter(x, y):
        sx, sy = y + 4, 19 - x
        return (sx, sy) if inside(sx, sy) else None
    rows['c_turn'], detail['c_worst'] = f28_close(got, f30_expected(base, quarter))
    detail['c_reply'] = turned
    app.call('undo', document_id=doc)

    # (d) after a conversion to 16 bits, one red move: the hole is 7196 and the
    # landing copies the source's red code; nothing else changes.
    app.call('convert_precision', document_id=doc, mode='rgba16', structure='preserve')

    def png16(label):
        path = folder / f'f30-{label}.png'
        app.call('export_image', document_id=doc, path=str(path),
                 settings={'format': 'png', 'bit_depth': 16}, overwrite=True)
        return read_png(path.read_bytes())[2]
    before = png16('d-before')
    select()
    app.call('move_selected_pixels', document_id=doc, dx=9, dy=5, target='red',
             background=F30_BACKGROUND)
    after = png16('d-after')
    worst = 0
    for y in range(HEIGHT):
        for x in range(WIDTH):
            want = list(before[y * WIDTH + x])
            if inside(x - 9, y - 5):
                want[0] = before[(y - 5) * WIDTH + x - 9][0]
            elif inside(x, y):
                want[0] = F30_HOLE16
            got16 = after[y * WIDTH + x]
            worst = max(worst, max(abs(a - b) for a, b in zip(got16, want)))
    rows['d_u16'] = worst <= T_CODE16
    detail['d_worst_codes'] = worst
    detail['rows'] = rows
    record(report, "F30 Move and Free Transform of one colour channel of a float layer",
           all(rows.values()), detail)


# ------------------------------------------------------------------- F32 --

# F32's oracle: Photoshop 26.10.0's measured rule for Fill / Stroke on ONE
# colour channel at 32 bits (`scripts/photoshop-probe/plane_fill.py`,
# receipt `plane-fill-431/take1`), written out here: the value is the swatch's
# 30/59/11 luma over its components, raw, at opacity × coverage whatever the
# pixel's alpha; a float foreground's is the same sum over its linear numbers.
F32_RECT = (4, 4, 16, 8)
F32_SOFT_RECT = (4, 20, 16, 8)
F32_COLOR = '#dc1eaa'
F32_V = (0.30 * 220 + 0.59 * 30 + 0.11 * 170) / 255
F32_HDR = [4.0, 0.5, 0.25, 1.0]
F32_HDR_V = 0.30 * 4.0 + 0.59 * 0.5 + 0.11 * 0.25
# The same swatch at 16 and 8 bits (`scripts/photoshop-probe/plane_gray.py`,
# receipt `plane-gray-431-633/take1`): the luma unrounded as a 16-bit sample,
# and rounded to a code over the swatch's 8-bit codes.
F32_V16 = round(65535 * F32_V)
F32_V8 = math.floor(0.30 * 220 + 0.59 * 30 + 0.11 * 170 + 0.5)
F32_BOTH = 'Pass rgba32 or color, not both; rgba32 takes no color_space.'
F32_RANGE = 'rgba32 requires four finite numbers'
F32_DEPTH = "rgba32 is a 32-bit float document's colour. Pass color."


def f32_inside(rect):
    x0, y0, w, h = rect
    return lambda x, y: 'in' if x0 <= x < x0 + w and y0 <= y < y0 + h else None


def f32_ring(rect, width):
    """The inside stroke's band: the `width` rings read the ink, the ring
    after them is the band's soft edge (unchecked), the rest is untouched."""
    x0, y0, w, h = rect

    def where(x, y):
        if not (x0 <= x < x0 + w and y0 <= y < y0 + h):
            return None
        inset = min(x - x0, x0 + w - 1 - x, y - y0, y0 + h - 1 - y)
        return 'in' if inset < width else 'skip' if inset == width else None
    return where


def f32_check(got, base, region, want):
    """`got` against `base`: where `region(x, y)` is 'in', each component
    `want(pixel)` names within T_REL; everywhere but a 'skip' pixel, every
    other sample bit for bit."""
    worst, exact = 0.0, True
    for y in range(HEIGHT):
        for x in range(WIDTH):
            i = (y * WIDTH + x) * 4
            where = region(x, y)
            if where == 'skip':
                continue
            wanted = want(base[i:i + 4]) if where == 'in' else {}
            for k in range(4):
                if k in wanted:
                    worst = max(worst, abs(got[i + k] - wanted[k]) / max(1.0, abs(wanted[k])))
                elif not bits_equal(got[i + k:i + k + 1], base[i + k:i + k + 1]):
                    exact = False
    return exact and worst <= T_REL, dict(worst=worst, others_bit_exact=exact)


def gate_f32(app, folder, report):
    copy = folder / 'f32-scene.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    doc = app.open(copy)
    base = scene()
    rows, detail = {}, dict(tolerance=T_REL, rect=list(F32_RECT), value=F32_V,
                            hdr_value=F32_HDR_V)

    def select(rect=F32_RECT):
        app.call('select_rect', document_id=doc, x=rect[0], y=rect[1], width=rect[2],
                 height=rect[3])

    def after(label):
        return export32(app, doc, folder / f'f32-{label}.tiff')['samples']

    def undone(label):
        app.call('undo', document_id=doc)
        return bits_equal(after(label + '-undo'), base)

    document_ink = dict(color=F32_COLOR, color_space='document')

    # (a) Fill on the Red channel.
    select()
    reply = app.call('fill_selection', document_id=doc, target='red', **document_ink)
    rows['a_red'], detail['a'] = f32_check(after('a'), base, f32_inside(F32_RECT),
                                           lambda p: {0: F32_V})
    rows['a_reply'] = reply.get('storage') == 'rgba32_float' and reply.get('target') == 'red' \
        and reply.get('changed') is True
    detail['a_reply'] = reply

    # (b) undo, then the same fill across the half-alpha band: Photoshop's
    # `written` row, the full value whatever the pixel's alpha.
    rows['b_undo'] = undone('a')
    select(F32_SOFT_RECT)
    app.call('fill_selection', document_id=doc, target='red', **document_ink)
    rows['b_soft'], detail['b'] = f32_check(after('b'), base, f32_inside(F32_SOFT_RECT),
                                            lambda p: {0: F32_V})
    rows['b_soft_undo'] = undone('b')

    # (c) Multiply at half opacity on the Green channel.
    select()
    app.call('fill_selection', document_id=doc, target='green', blend_mode='Multiply',
             opacity=0.5, **document_ink)
    rows['c_multiply'], detail['c'] = f32_check(
        after('c'), base, f32_inside(F32_RECT),
        lambda p: {1: p[1] * (1.0 + (F32_V - 1.0) * 0.5)})
    rows['c_undo'] = undone('c')

    # (d) Stroke on the Blue channel: the inside band.
    select()
    stroked = app.call('stroke_selection', document_id=doc, target='blue', width=2,
                       location='inside', **document_ink)
    rows['d_stroke'], detail['d'] = f32_check(after('d'), base, f32_ring(F32_RECT, 2),
                                              lambda p: {2: F32_V})
    rows['d_reply'] = stroked.get('storage') == 'rgba32_float'
    rows['d_undo'] = undone('d')

    # (e) above-white ink on the layer: the numbers themselves.
    ink = {0: F32_HDR[0], 1: F32_HDR[1], 2: F32_HDR[2], 3: F32_HDR[3]}
    select()
    app.call('fill_selection', document_id=doc, layer=0, rgba32=F32_HDR)
    rows['e_fill'], detail['e'] = f32_check(after('e'), base, f32_inside(F32_RECT),
                                            lambda p: ink)
    rows['e_undo'] = undone('e')

    # (f) the same ink stroked.
    select()
    app.call('stroke_selection', document_id=doc, layer=0, width=2, location='inside',
             rgba32=F32_HDR)
    rows['f_stroke'], detail['f'] = f32_check(after('f'), base, f32_ring(F32_RECT, 2),
                                              lambda p: ink)
    rows['f_undo'] = undone('f')

    # (h) linear ink on a colour channel: the luma of the linear numbers (H-LIN).
    select()
    app.call('fill_selection', document_id=doc, target='red', rgba32=F32_HDR)
    rows['h_linear_plane'], detail['h'] = f32_check(after('h'), base, f32_inside(F32_RECT),
                                                    lambda p: {0: F32_HDR_V})
    rows['h_undo'] = undone('h')

    # (g) the in-band refusals; the conversion to 16 bits comes last.
    select()
    refusals = dict(
        both=app.call('fill_selection', fail=True, document_id=doc, layer=0, color='#ffffff',
                      rgba32=F32_HDR),
        range=app.call('fill_selection', fail=True, document_id=doc, layer=0,
                       rgba32=[70000, 0, 0, 1]))
    app.call('convert_precision', document_id=doc, mode='rgba16', structure='preserve')
    refusals['depth'] = app.call('stroke_selection', fail=True, document_id=doc, layer=0,
                                 width=2, rgba32=F32_HDR)
    rows['g_both'] = F32_BOTH in refusals['both']
    rows['g_range'] = F32_RANGE in refusals['range']
    rows['g_depth'] = F32_DEPTH in refusals['depth']
    detail['g_refusals'] = refusals

    def png(label, bits):
        path = folder / f'f32-{label}.png'
        app.call('export_image', document_id=doc, path=str(path),
                 settings={'format': 'png', 'bit_depth': bits}, overwrite=True)
        return read_png(path.read_bytes())[2]

    def plane_fill(before, got, value, slack):
        """Red inside F32_RECT reads `value` within `slack` codes; every other
        sample equals `before`."""
        inside = f32_inside(F32_RECT)
        worst, others = 0, len(got) == len(before)
        for y in range(HEIGHT):
            for x in range(WIDTH):
                was, now = tuple(before[y * WIDTH + x]), tuple(got[y * WIDTH + x])
                if inside(x, y):
                    worst = max(worst, abs(now[0] - value))
                    others = others and now[1:] == was[1:]
                else:
                    others = others and now == was
        return others and worst <= slack, dict(worst_codes=worst, others_equal=others)

    # (i) the Red fill at 16 bits: the luma unrounded, as a 16-bit sample.
    before16 = png('i-before', 16)
    select()
    filled = app.call('fill_selection', document_id=doc, target='red', **document_ink)
    rows['i_red16'], detail['i'] = plane_fill(before16, png('i', 16), F32_V16, T_CODE16)
    rows['i_reply'] = filled.get('changed') is True
    detail['i_reply'] = filled

    # (i-undo) the 16-bit document is as it was before (j) converts it.
    app.call('undo', document_id=doc)
    rows['i_undo'] = png('i-undo', 16) == before16

    # (j) the same fill at 8 bits: the luma rounded to a code.
    app.call('convert_precision', document_id=doc, mode='rgba8', structure='preserve')
    before8 = png('j-before', 8)
    select()
    filled = app.call('fill_selection', document_id=doc, target='red', **document_ink)
    rows['j_red8'], detail['j'] = plane_fill(before8, png('j', 8), F32_V8, 0)
    rows['j_reply'] = filled.get('changed') is True
    detail['j_reply'] = filled
    detail.update(value16=F32_V16, value8=F32_V8)
    detail['rows'] = rows
    record(report, 'F32 Fill and Stroke on one colour channel of a float layer, and '
                   'above-white ink', all(rows.values()), detail)


# ------------------------------------------------------------------- F33 --

F33_STILLS = ROOT / 'core/tests/fixtures/host_stills_opaque'


def f33_png(path):
    """`scripts/pixels.py`'s `png_rgba` of a file (that reader is written
    from the PNG standard and reads no Doctored code)."""
    from pixels import png_rgba
    return png_rgba(Path(path).read_bytes())


# ------------------------------------------------------------------- F34 --

F34_SIZE = (384, 512)     # synthetic_dng's 512 x 384 at orientation 6
F34_RAW = 2e-4            # a float export against the 16-bit twin's decoded codes
F34_PRESET = 'F34 exposure'
F34_ACTION = 'F34 apply a RAW preset at 32 bits'
# One radial local: `scripts/raw-local-probe/mcp.py`'s `radial()` shape.
F34_RADIAL = dict(op='create', id='radial', name='Radial',
                  mask=dict(op='radial', center=[384 * .47, 512 * .51],
                            radius=[384 * .39, 512 * .36], hardness=.137))


def f34_native():
    """`scripts/raw-probe/native.py`, loaded by path: an independent reader
    and writer of the `.rz` source records (it calls no Doctored loader)."""
    import importlib.util
    spec = importlib.util.spec_from_file_location('f34_native',
                                                  ROOT / 'scripts/raw-probe/native.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def gate_f34(app, folder, report):
    import time
    from phase3_fixtures import synthetic_dng
    rows = {}
    detail = dict(tolerance=F34_RAW,
                  receipt='doctored-fixtures/photoshop/float-raw-602/take2')
    width, height = F34_SIZE
    fixture_icc = read_float_tiff(FIXTURE)['icc']
    fixture_inverse = f16_inv(f20_columns(fixture_icc))
    decode = [f20_decode(code / 65535.0) for code in range(65536)]

    def states(document):
        return len(app.call('list_history', document_id=document)['states'])

    def raw(document, layer):
        return app.call('get_raw_source', document_id=document, layer=layer)['source']

    def exposure(document, layer):
        return raw(document, layer)['raw']['recipe']['values']['exposure']

    def depths(document, layer):
        source = app.call('get_image_source', document_id=document, layer=layer)['source'] or {}
        return dict(fallback_depth=source.get('fallback_depth'),
                    master_depth=source.get('master_depth'))

    def float_export(document, label):
        tiff = export32(app, document, folder / f'f34-{label}.tiff')
        assert (tiff['width'], tiff['height'], tiff['channels']) == (width, height, 4), label
        return tiff['samples']

    def twin_export(document, label):
        """`(samples, sound)`: the twin's 16-bit PNG export as linear light in
        the fixture's profile (each code's sRGB decode through `inv(fixture
        colorants) · the export's own colorants`, alpha its code / 65535),
        and whether the export is opaque and not all zero."""
        path = folder / f'f34-{label}.png'
        app.call('export_image', document_id=document, path=str(path),
                 settings={'format': 'png', 'bit_depth': 16}, overwrite=True)
        codes = f33_png(path)
        assert (codes['width'], codes['height'], codes['depth']) == (width, height, 16), label
        matrix = f16_mul(fixture_inverse, f20_columns(f20_png_icc(path)))
        samples = codes['samples']
        out = []
        for i in range(0, len(samples), 4):
            out.extend(f16_apply(matrix, [decode[samples[i + c]] for c in range(3)]))
            out.append(samples[i + 3] / 65535.0)
        opaque = all(samples[i] == 65535 for i in range(3, len(samples), 4))
        lit = any(samples[i] or samples[i + 1] or samples[i + 2]
                  for i in range(0, len(samples), 4))
        return out, opaque and lit

    def worst(got, want):
        assert len(got) == len(want), (len(got), len(want))
        return max(abs(a - b) for a, b in zip(got, want))

    def float_target(label):
        """The gate's own target: a transparent float document of the
        developed size in the fixture's profile."""
        path = folder / f'f34-{label}.tiff'
        write_float_tiff(path, width, height, [0.0] * (width * height * 4), fixture_icc)
        return app.open(path)

    def pasted_into(target, source, source_layer):
        """Copy `source`'s layer and paste it in place as layers; the pasted
        layer's index, asserted at (0, 0) covering the canvas."""
        app.call('copy', document_id=source, mode='layers', layers=[source_layer])
        app.call('paste', document_id=target, mode='layers', placement='in_place')
        layers = app.call('get_document', document_id=target)['layers']
        found = [row for row in layers
                 if (app.call('get_image_source', document_id=target,
                              layer=row['index']).get('source') or {}).get('raw')]
        assert len(found) == 1, layers
        row = found[0]
        assert (row['offset_x'], row['offset_y'], row['width'], row['height']) \
            == (0, 0, width, height), row
        return row['index']

    def with_images(tool, **arguments):
        """A tool whose reply carries images: `(report, images)`."""
        deadline = time.monotonic() + 30
        while True:
            result = app.client.rpc('tools/call', {'name': tool, 'arguments': arguments})
            text = next(block['text'] for block in result['content']
                        if block['type'] == 'text')
            if result.get('isError') and 'Another background operation is running.' in text \
                    and time.monotonic() < deadline:
                time.sleep(0.05)
                continue
            assert not result.get('isError'), text
            return json.loads(text), [block for block in result['content']
                                      if block['type'] == 'image']

    # The twin: the camera file developed at 16 bits.
    dng = folder / 'f34.dng'
    synthetic_dng(dng)
    twin = app.call('open_document', path=str(dng), raw={'exposure': 0.5},
                    precision='preserve')['document']['id']
    state = app.call('get_document', document_id=twin)
    assert (state['width'], state['height']) == F34_SIZE, (state['width'], state['height'])
    assert app.call('get_precision', document_id=twin)['mode'] == 'rgba16'

    # pasted: the twin's layer pasted in place into the float target keeps
    # its original and reads the development's sRGB decode.
    document = float_target('target')
    layer = pasted_into(document, twin, 0)
    source = raw(document, layer)
    first = float_export(document, 'pasted')
    want, sound = twin_export(twin, 'twin-pasted')
    detail['pasted'] = dict(depths(document, layer), layer=layer,
                            exposure=source['raw']['recipe']['values']['exposure'],
                            decoder_status=source.get('decoder_status'),
                            twin_sound=sound, worst=worst(first, want),
                            float=app.call('get_precision', document_id=document)['mode'])
    rows['pasted'] = (detail['pasted']['fallback_depth'] == 32
                      and detail['pasted']['master_depth'] == 16
                      and detail['pasted']['exposure'] == 0.5
                      and detail['pasted']['decoder_status'] == 'captured_implementation_available'
                      and sound and detail['pasted']['worst'] <= F34_RAW)
    twin_native = folder / 'f34-twin.rz'
    app.call('save_copy', document_id=twin, path=str(twin_native), format='rz')

    # redeveloped: one history state, the twin's new development, no state
    # for the same recipe again.
    before = states(document)
    app.call('redevelop_raw', document_id=document, layer=layer, raw={'exposure': 1.0})
    app.call('redevelop_raw', document_id=twin, layer=0, raw={'exposure': 1.0})
    after = states(document)
    second = float_export(document, 'redeveloped')
    want, sound = twin_export(twin, 'twin-redeveloped')
    app.call('redevelop_raw', document_id=document, layer=layer, raw={'exposure': 1.0})
    detail['redeveloped'] = dict(states=(before, after, states(document)),
                                 exposure=exposure(document, layer), twin_sound=sound,
                                 worst=worst(second, want),
                                 differs=not bits_equal(second, first),
                                 depths=depths(document, layer))
    rows['redeveloped'] = (after == before + 1 and states(document) == after
                           and detail['redeveloped']['exposure'] == 1.0 and sound
                           and detail['redeveloped']['worst'] <= F34_RAW
                           and detail['redeveloped']['differs']
                           and detail['redeveloped']['depths'] == dict(fallback_depth=32,
                                                                       master_depth=16))

    # undo: the first development comes back bit-exact; redo restores.
    app.call('undo', document_id=document)
    undone = float_export(document, 'undone')
    undone_exposure = exposure(document, layer)
    app.call('redo', document_id=document)
    redone = float_export(document, 'redone')
    detail['undo'] = dict(bit_exact=bits_equal(undone, first), exposure=undone_exposure,
                          redo_bit_exact=bits_equal(redone, second),
                          redo_exposure=exposure(document, layer))
    rows['undo'] = detail['undo'] == dict(bit_exact=True, exposure=0.5, redo_bit_exact=True,
                                          redo_exposure=1.0)

    # settings: a preset applied under recording is one apply_raw_settings
    # step that replays; synchronization runs in both directions.
    app.call('save_raw_preset', name=F34_PRESET, raw={'exposure': 0.25})
    app.call('start_recording')
    applied = app.call('apply_raw_preset', document_id=document, name=F34_PRESET, layer=layer)
    action = app.call('stop_recording', name=F34_ACTION)
    preset_exposure = exposure(document, layer)
    app.call('undo', document_id=document)
    undone_exposure = exposure(document, layer)
    app.call('run_action', document_id=document, name=F34_ACTION)
    replayed_exposure = exposure(document, layer)
    out = app.call('sync_raw_settings', document_id=document, layer=layer, fields=['exposure'],
                   targets=[{'document_id': twin, 'layer': 0}])
    twin_exposure = exposure(twin, 0)
    app.call('redevelop_raw', document_id=twin, layer=0, raw={'exposure': 0.75})
    back = app.call('sync_raw_settings', document_id=twin, layer=0, fields=['exposure'],
                    targets=[{'document_id': document, 'layer': layer}])
    third = float_export(document, 'synchronized')
    want, sound = twin_export(twin, 'twin-synchronized')
    detail['settings'] = dict(applied=applied.get('result'),
                              steps=[step.get('tool') for step in action.get('steps') or []],
                              preset_exposure=preset_exposure, undone_exposure=undone_exposure,
                              replayed_exposure=replayed_exposure, out=out.get('result'),
                              twin_exposure=twin_exposure, back=back.get('result'),
                              exposure=exposure(document, layer), twin_sound=sound,
                              worst=worst(third, want))
    # Each settings result also lists the targets it skipped.
    changed = dict(changed_documents=1, changed_layers=1, skipped=[])
    rows['settings'] = (detail['settings']['applied'] == changed
                        and detail['settings']['steps'] == ['apply_raw_settings']
                        and (preset_exposure, undone_exposure, replayed_exposure)
                        == (0.25, 1.0, 0.25)
                        and detail['settings']['out'] == changed
                        and twin_exposure == 0.25
                        and detail['settings']['back'] == changed
                        and detail['settings']['exposure'] == 0.75 and sound
                        and detail['settings']['worst'] <= F34_RAW)

    # inspected: a patched inspection previews and commits nothing.
    revision, before = raw(document, layer)['revision_id'], states(document)
    inspection, images = with_images('inspect_raw_development', document_id=document,
                                     layer=layer, raw={'exposure': 1.25})
    detail['inspected'] = dict(images=len(images), committed=inspection.get('committed'),
                               revision_kept=raw(document, layer)['revision_id'] == revision,
                               states=(before, states(document)),
                               proposed=((inspection.get('proposed_descriptor') or {})
                                         .get('recipe') or {}).get('values', {}).get('exposure'))
    rows['inspected'] = (len(images) == 2 and inspection.get('committed') is False
                         and detail['inspected']['revision_kept']
                         and states(document) == before
                         and detail['inspected']['proposed'] == 1.25)

    # locals: one radial local with an exposure offset in both documents,
    # then the twin's local changed and its settings copied across.
    def mask_sha(target, target_layer):
        return app.call('get_raw_local_mask', document_id=target, layer=target_layer,
                        id='radial').get('sha256')

    create = dict(F34_RADIAL, exposure_offset=.75)
    edited = app.call('edit_raw_development', document_id=document, layer=layer,
                      operations=[create])
    app.call('edit_raw_development', document_id=twin, layer=0, operations=[create])
    nodes = app.call('get_raw_locals', document_id=document, layer=layer)['nodes']
    fourth = float_export(document, 'local')
    want, sound = twin_export(twin, 'twin-local')
    local_worst = worst(fourth, want)
    app.call('edit_raw_development', document_id=twin, layer=0,
             operations=[dict(op='update', id='radial', exposure_offset=.25)])
    copied = app.call('copy_raw_local_settings', document_id=document, layer=layer, id='radial',
                      source_document_id=twin, source_layer=0, source_id='radial')
    fifth = float_export(document, 'local-copied')
    want, copied_sound = twin_export(twin, 'twin-local-copied')
    copied_nodes = app.call('get_raw_locals', document_id=document, layer=layer)['nodes']
    detail['locals'] = dict(unchanged=edited.get('unchanged'),
                            nodes=[(node.get('id'), node.get('exposure_offset'))
                                   for node in nodes],
                            mask=mask_sha(document, layer), twin_mask=mask_sha(twin, 0),
                            worst=local_worst, differs=not bits_equal(fourth, third),
                            copied_unchanged=copied.get('unchanged'),
                            copied_nodes=[(node.get('id'), node.get('exposure_offset'))
                                          for node in copied_nodes],
                            copied_worst=worst(fifth, want),
                            copied_differs=not bits_equal(fifth, fourth),
                            depths=depths(document, layer))
    rows['locals'] = (edited.get('unchanged') is False
                      and detail['locals']['nodes'] == [('radial', .75)]
                      and detail['locals']['mask'] is not None
                      and detail['locals']['mask'] == detail['locals']['twin_mask']
                      and sound and local_worst <= F34_RAW and detail['locals']['differs']
                      and copied.get('unchanged') is False
                      and detail['locals']['copied_nodes'] == [('radial', .25)]
                      and copied_sound and detail['locals']['copied_worst'] <= F34_RAW
                      and detail['locals']['copied_differs']
                      and detail['locals']['depths'] == dict(fallback_depth=32,
                                                             master_depth=16))

    # transformed: a scaled layer still redevelops and takes a local edit.
    app.call('transform_layer', document_id=document, layer=layer, scale=1.5,
             around='top_left', sampler='bilinear')
    before = states(document)
    app.call('redevelop_raw', document_id=document, layer=layer, raw={'exposure': 0.625})
    local = app.call('edit_raw_development', document_id=document, layer=layer,
                     operations=[dict(op='update', id='radial', exposure_offset=.5)])
    row = next(r for r in app.call('get_document', document_id=document)['layers']
               if r['index'] == layer)
    bound = app.call('get_image_source', document_id=document, layer=layer)['source'] or {}
    detail['transformed'] = dict(states=(before, states(document)),
                                 exposure=exposure(document, layer),
                                 unchanged=local.get('unchanged'),
                                 size=(row['width'], row['height']),
                                 valid=bound.get('valid', True),
                                 depths=depths(document, layer))
    rows['transformed'] = (states(document) == before + 2
                           and detail['transformed']['exposure'] == 0.625
                           and local.get('unchanged') is False
                           and detail['transformed']['size'] == (width * 3 // 2,
                                                                 height * 3 // 2)
                           and detail['transformed']['valid'] is not False
                           and detail['transformed']['depths'] == dict(fallback_depth=32,
                                                                       master_depth=16))

    # migrated: a record whose captured decoder is unavailable keeps its
    # rendering, refuses to redevelop, and migrates under review in one state.
    native = f34_native()
    old_native = folder / 'f34-old.rz'
    native.incompatible_decoder(twin_native, old_native)
    old = app.open(old_native)
    stale = float_target('target-old')
    stale_layer = pasted_into(stale, old, 0)
    kept = float_export(stale, 'old')
    status = raw(stale, stale_layer).get('decoder_status')
    before = states(stale)
    refusal = app.call('redevelop_raw', fail=True, document_id=stale, layer=stale_layer,
                       raw={'exposure': 1.0})
    refused_kept = bits_equal(float_export(stale, 'old-refused'), kept) \
        and states(stale) == before
    review, images = with_images('preview_raw_decoder_migration', document_id=stale,
                                 layer=stale_layer)
    app.call('migrate_raw_decoder', document_id=stale, layer=stale_layer,
             preview_id=review.get('preview_id'))
    migrated_states = states(stale)
    migrated_status = raw(stale, stale_layer).get('decoder_status')
    app.call('undo', document_id=stale)
    detail['migrated'] = dict(status=status, refusal=refusal, refused_kept=refused_kept,
                              preview_id=bool(review.get('preview_id')), images=len(images),
                              states=(before, migrated_states),
                              migrated_status=migrated_status,
                              migrated_depths=depths(stale, stale_layer),
                              undo_bit_exact=bits_equal(float_export(stale, 'old-undone'),
                                                        kept),
                              undo_status=raw(stale, stale_layer).get('decoder_status'))
    rows['migrated'] = (status == 'unavailable' and refused_kept
                        and detail['migrated']['preview_id'] and len(images) == 2
                        and migrated_states == before + 1
                        and migrated_status == 'captured_implementation_available'
                        and detail['migrated']['migrated_depths'] == dict(fallback_depth=32,
                                                                          master_depth=16)
                        and detail['migrated']['undo_bit_exact']
                        and detail['migrated']['undo_status'] == 'unavailable')

    # reopened: the .rz carries the original, the recipe, the local's mask
    # and the pixels bit-exact.
    last = float_export(document, 'last')
    saved = folder / 'f34.rz'
    app.call('save_copy', document_id=document, path=str(saved), format='rz')
    reopened = app.open(saved)
    again = float_export(reopened, 'reopened')
    detail['reopened'] = dict(depths(reopened, layer), bit_exact=bits_equal(again, last),
                              raw_equal=raw(reopened, layer)['raw'] == raw(document,
                                                                         layer)['raw'],
                              mask_equal=mask_sha(reopened, layer) == mask_sha(document,
                                                                              layer))
    rows['reopened'] = detail['reopened'] == dict(fallback_depth=32, master_depth=16,
                                                  bit_exact=True, raw_equal=True,
                                                  mask_equal=True)
    detail['rows'] = rows
    record(report, 'F34 RAW Develop edits of a placed source at 32 bits',
           all(rows.values()), detail)


F35_FIRST = {'reds': 1.2, 'blues': -0.5, 'tint': True, 'tint_color': '#2878dc'}
F35_SECOND = {'reds': 1.2, 'blues': -0.5, 'tint': True, 'tint_color': '#998a66'}
F35_CAP = 16.0
F35_FLOOR = 1.0 / 510.0


def f35_oracle(params):
    """Black & White with Tint at 32 bits (module doc, F35): the grey of
    `f16_black_and_white`, capped at 16, then SetLum of the tint's numbers
    over 255 to it with ClipColor at 16, plus 1/510 when a branch fired."""
    gray = f16_black_and_white(params)
    code = params['tint_color'].lstrip('#')
    tint = [int(code[i:i + 2], 16) / 255.0 for i in (0, 2, 4)]

    def lum(c):
        return 0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]

    def run(rgb):
        big = min(gray(rgb)[0], F35_CAP)
        c = [t + (big - lum(tint)) for t in tint]
        n, x = min(c), max(c)
        low, high = n < 0.0, x > F35_CAP and x > big
        if low:
            c = [big + (k - big) * big / (big - n) for k in c]
        if high:
            c = [big + (k - big) * (F35_CAP - big) / (x - big) for k in c]
        return [k + F35_FLOOR for k in c] if low or high else c
    return run


def gate_f35(app, folder, report):
    base = scene()
    rows, detail = {}, dict(tolerance=T_REL)

    def check(samples, oracle):
        worst, failures = 0.0, []
        for i in range(0, len(base), 4):
            want = oracle(base[i:i + 3]) + [base[i + 3]]
            got = samples[i:i + 4]
            for g, w in zip(got, want):
                worst = max(worst, abs(g - w) / max(1.0, abs(w)))
            if not f14_close(got, want) or not bits_equal(got[3:], want[3:]):
                failures.append(dict(pixel=i // 4, got=got, want=want))
        return not failures, worst, failures[:3]

    def fresh(label):
        copy = folder / f'f35-{label}.tiff'
        copy.write_bytes(FIXTURE.read_bytes())
        return app.open(copy)

    # tint_layer: add, edit, save and reopen.
    document = fresh('layer')
    app.call('add_adjustment_layer', document_id=document, op='black_and_white',
             params=F35_FIRST)
    added = export32(app, document, folder / 'f35-added.tiff')['samples']
    ok_added, worst_added, bad_added = check(added, f35_oracle(F35_FIRST))
    app.call('edit_adjustment_layer', document_id=document, layer=1, params=F35_SECOND)
    edited = export32(app, document, folder / 'f35-edited.tiff')['samples']
    ok_edited, worst_edited, bad_edited = check(edited, f35_oracle(F35_SECOND))
    native = folder / 'f35.rz'
    app.call('save_copy', document_id=document, path=str(native), format='rz')
    reopened = app.open(native)
    after = export32(app, reopened, folder / 'f35-reopened.tiff')['samples']
    layers = app.call('get_document', document_id=reopened).get('layers') or []
    reported = [row.get('adjustment') for row in layers if row.get('adjustment')]
    kept = (len(reported) == 1 and reported[0].get('op') == 'black_and_white'
            and f16_reports(F35_SECOND, reported[0].get('params')))
    detail['tint_layer'] = dict(added_worst=worst_added, added_failures=bad_added,
                                edited_worst=worst_edited, edited_failures=bad_edited,
                                reopened_bit_exact=bits_equal(after, edited), reported=reported)
    rows['tint_layer'] = ok_added and ok_edited and bits_equal(after, edited) and kept

    # tint_apply: the destructive twin, then undo.
    document = fresh('apply')
    before = export32(app, document, folder / 'f35-before.tiff')['samples']
    reply = app.call('apply_filter', document_id=document, layer=0, filter='black_and_white',
                     params=F35_FIRST)
    applied = export32(app, document, folder / 'f35-applied.tiff')['samples']
    ok, worst, bad = check(applied, f35_oracle(F35_FIRST))
    app.call('undo', document_id=document)
    undone = export32(app, document, folder / 'f35-undone.tiff')['samples']
    detail['tint_apply'] = dict(reply=reply, worst=worst, failures=bad,
                                undo_bit_exact=bits_equal(undone, before))
    rows['tint_apply'] = (ok and reply.get('changed') is True
                          and reply.get('storage') == 'rgba32_float'
                          and bits_equal(undone, before))
    detail['rows'] = rows
    record(report, 'F35 Black & White with Tint renders, edits, persists and applies at 32 bits',
           all(rows.values()), detail)


# ------------------------------------------------------------------- F36 --

F36_SIDECAR = ('<x:xmpmeta xmlns:x="adobe:ns:meta/">\n'
               ' <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">\n'
               '  <rdf:Description rdf:about=""\n'
               '    xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"\n'
               '   crs:Version="15.0"\n'
               '   crs:ProcessVersion="11.0"\n'
               '   crs:Exposure2012="+1.00"\n'
               '   crs:HasSettings="True"/>\n'
               ' </rdf:RDF>\n'
               '</x:xmpmeta>\n')


def gate_f36(app, folder, report):
    from phase3_fixtures import synthetic_dng
    rows = {}
    detail = dict(tolerance=dict(float=F34_RAW, codes16=T_CODE16, codes8=T_CODE8),
                  receipts=['doctored-fixtures/photoshop/float-raw-602/take1',
                            'doctored-fixtures/photoshop/float-raw-602/take2',
                            'doctored-fixtures/photoshop/float-host-602/take2'])
    width, height = F34_SIZE
    fixture_icc = read_float_tiff(FIXTURE)['icc']
    fixture_inverse = f16_inv(f20_columns(fixture_icc))
    decode = [f20_decode(code / 65535.0) for code in range(65536)]

    def states(document):
        return len(app.call('list_history', document_id=document)['states'])

    def raw(document, layer):
        return app.call('get_raw_source', document_id=document, layer=layer)['source']

    def exposure(document, layer):
        return raw(document, layer)['raw']['recipe']['values']['exposure']

    def image_source(document, layer):
        return app.call('get_image_source', document_id=document, layer=layer)['source'] or {}

    def bounds(document, layer):
        row = next(r for r in app.call('get_document', document_id=document)['layers']
                   if r['index'] == layer)
        return (row['offset_x'], row['offset_y'], row['width'], row['height'])

    def float_export(document, label):
        tiff = export32(app, document, folder / f'f36-{label}.tiff')
        assert (tiff['width'], tiff['height'], tiff['channels']) == (width, height, 4), label
        return tiff['samples']

    def png_codes(document, label, depth):
        path = folder / f'f36-{label}.png'
        app.call('export_image', document_id=document, path=str(path),
                 settings={'format': 'png', 'bit_depth': depth}, overwrite=True)
        codes = f33_png(path)
        assert (codes['width'], codes['height'], codes['depth']) == (width, height, depth), label
        return path, codes['samples']

    def twin_export(document, label):
        """F34's form: the twin's 16-bit PNG as linear light in the fixture's
        profile, its raw codes, and whether it is opaque and not all zero."""
        path, samples = png_codes(document, label, 16)
        matrix = f16_mul(fixture_inverse, f20_columns(f20_png_icc(path)))
        out = []
        for i in range(0, len(samples), 4):
            out.extend(f16_apply(matrix, [decode[samples[i + c]] for c in range(3)]))
            out.append(samples[i + 3] / 65535.0)
        opaque = all(samples[i] == 65535 for i in range(3, len(samples), 4))
        lit = any(samples[i] or samples[i + 1] or samples[i + 2]
                  for i in range(0, len(samples), 4))
        return out, samples, opaque and lit

    def worst(got, want):
        assert len(got) == len(want), (len(got), len(want))
        return max(abs(a - b) for a, b in zip(got, want))

    def float_target(label, ppi):
        """A transparent float document of the developed size in the
        fixture's profile, at `ppi`."""
        path = folder / f'f36-{label}.tiff'
        write_float_tiff(path, width, height, [0.0] * (width * height * 4), fixture_icc,
                         ppi=ppi)
        return app.open(path)

    def place(document, path, **arguments):
        return app.call('place_image', document_id=document, path=str(path), **arguments)

    # The twin: the camera file developed at 16 bits; P is its resolution.
    dng = folder / 'f36.dng'
    synthetic_dng(dng)
    twin = app.call('open_document', path=str(dng), raw={'exposure': 0.5},
                    precision='preserve')['document']['id']
    state = app.call('get_document', document_id=twin)
    assert (state['width'], state['height']) == F34_SIZE, (state['width'], state['height'])
    ppi = state['resolution']['ppi_x']
    detail['ppi'] = ppi
    want, twin_codes, sound = twin_export(twin, 'twin')
    detail['twin_sound'] = sound

    # placed32: a float target at P; one state, the development at 100 %.
    document = float_target('target', ppi)
    target_ppi = app.call('get_document', document_id=document)['resolution']['ppi_x']
    empty = float_export(document, 'empty')
    before = states(document)
    placed = place(document, dng, raw={'exposure': 0.5, 'output_encoding': 'sdr16'})
    layer = placed['layer']
    source = raw(document, layer)
    bound = image_source(document, layer)
    first = float_export(document, 'placed32')
    detail['placed32'] = dict(target_ppi=target_ppi, states=(before, states(document)),
                              bounds=bounds(document, layer),
                              exposure=source['raw']['recipe']['values']['exposure'],
                              decoder_status=source.get('decoder_status'),
                              filename=source['raw'].get('original_filename'),
                              depths=(bound.get('fallback_depth'), bound.get('master_depth')),
                              link=bound.get('link'), disclosures=placed.get('disclosures'),
                              worst=worst(first, want))
    rows['placed32'] = (target_ppi == ppi and states(document) == before + 1
                        and detail['placed32']['bounds'] == (0, 0, width, height)
                        and detail['placed32']['exposure'] == 0.5
                        and detail['placed32']['decoder_status']
                        == 'captured_implementation_available'
                        and detail['placed32']['filename'] == 'f36.dng'
                        and detail['placed32']['depths'] == (32, 16)
                        and bound.get('link') is None and sound
                        and detail['placed32']['worst'] <= F34_RAW)

    # placed16 / placed8: new documents at P read the twin's own codes.
    for depth, tolerance in ((16, T_CODE16), (8, T_CODE8)):
        target = app.call('new_document', width=width, height=height, depth=depth,
                          profile='srgb', background='transparent', ppi=ppi)['document']['id']
        out = place(target, dng, raw={'exposure': 0.5})
        _, codes = png_codes(target, f'placed{depth}', depth)
        scale = 1.0 if depth == 16 else 255.0 / 65535.0
        gap = max(abs(got - round(code * scale)) for got, code in zip(codes, twin_codes))
        found = image_source(target, out['layer'])
        detail[f'placed{depth}'] = dict(worst_codes=gap, bounds=bounds(target, out['layer']),
                                        depths=(found.get('fallback_depth'),
                                                found.get('master_depth')),
                                        exposure=exposure(target, out['layer']))
        rows[f'placed{depth}'] = (gap <= tolerance
                                  and detail[f'placed{depth}']['bounds']
                                  == (0, 0, width, height)
                                  and detail[f'placed{depth}']['depths'] == (depth, 16)
                                  and detail[f'placed{depth}']['exposure'] == 0.5)

    # sidecar: an .xmp beside the file sets the exposure, with no disclosure.
    sidecar_dng = folder / 'f36s.dng'
    sidecar_dng.write_bytes(dng.read_bytes())
    (folder / 'f36s.xmp').write_text(F36_SIDECAR)
    exposed = folder / 'f36e.dng'
    exposed.write_bytes(dng.read_bytes())
    twin_one = app.call('open_document', path=str(exposed), raw={'exposure': 1.0},
                        precision='preserve')['document']['id']
    want_one, _, sound_one = twin_export(twin_one, 'twin-one')
    side = float_target('sidecar', ppi)
    out = place(side, sidecar_dng, raw={'output_encoding': 'sdr16'})
    sidecar_export = float_export(side, 'sidecar')
    opened = app.call('open_document', path=str(sidecar_dng),
                      precision='preserve')['document']['id']
    detail['sidecar'] = dict(exposure=exposure(side, out['layer']),
                             disclosures=out.get('disclosures'), twin_sound=sound_one,
                             worst=worst(sidecar_export, want_one),
                             opened_exposure=exposure(opened, 0))
    rows['sidecar'] = (detail['sidecar']['exposure'] == 1.0 and 'disclosures' not in out
                       and sound_one and detail['sidecar']['worst'] <= F34_RAW
                       and detail['sidecar']['opened_exposure'] == 1.0)

    # scaled: a target at P/2 places the file at 50 %.
    half = float_target('half', ppi / 2)
    out = place(half, dng, raw={'exposure': 0.5})
    found = image_source(half, out['layer'])
    moved = place(half, dng, raw={'exposure': 0.5}, x=10, y=20)
    offset = image_source(half, moved['layer'])
    detail['scaled'] = dict(bounds=bounds(half, out['layer']), transform=found.get('transform'),
                            offset_bounds=bounds(half, moved['layer']),
                            offset_transform=offset.get('transform'),
                            depths=(found.get('fallback_depth'), found.get('master_depth')))
    rows['scaled'] = (detail['scaled']['bounds'] == (0, 0, width // 2, height // 2)
                      and found.get('transform') == [0.5, 0, 0, 0.5, 0, 0]
                      and detail['scaled']['depths'] == (32, 32)
                      and detail['scaled']['offset_bounds'] == (10, 20, width // 2, height // 2)
                      and offset.get('transform') == [0.5, 0, 0, 0.5, 10, 20])

    # linked: the link and its status.
    linked = float_target('linked', ppi)
    out = place(linked, dng, raw={'exposure': 0.5}, storage='linked')
    found = image_source(linked, out['layer'])
    detail['linked'] = dict(link=found.get('link'), status=found.get('link_status'))
    rows['linked'] = ((found.get('link') or {}).get('path') == str(dng)
                      and found.get('link_status') == 'current')

    # redeveloped: Redevelop RAW and a RAW local keep a linked layer linked.
    def link_state(document, layer):
        found = image_source(document, layer)
        link = found.get('link') or {}
        return (link.get('path'), link.get('sha256'), found.get('link_status'))

    again = float_target('redeveloped', ppi)
    out = place(again, dng, raw={'exposure': 0.5}, storage='linked')
    at = out['layer']
    linked_state = link_state(again, at)
    counts = [states(again)]
    app.call('redevelop_raw', document_id=again, layer=at, raw={'exposure': 1.0})
    counts.append(states(again))
    after_redevelop = link_state(again, at)
    app.call('edit_raw_development', document_id=again, layer=at,
             operations=[dict(F34_RADIAL, exposure_offset=.75)])
    counts.append(states(again))
    after_local = link_state(again, at)
    detail['redeveloped'] = dict(link=linked_state, after_redevelop=after_redevelop,
                                 after_local=after_local, states=counts,
                                 exposure=exposure(again, at))
    rows['redeveloped'] = (linked_state[0] == str(dng) and linked_state[2] == 'current'
                           and after_redevelop == linked_state and after_local == linked_state
                           and counts == [counts[0], counts[0] + 1, counts[0] + 2]
                           and detail['redeveloped']['exposure'] == 1.0)

    # refused: a bad raw setting and raw for a PNG refuse in band.
    before = states(document)
    over = app.call('place_image', fail=True, document_id=document, path=str(dng),
                    raw={'exposure': 9})
    png = app.call('place_image', fail=True, document_id=document,
                   path=str(F33_STILLS / 'source8.png'), raw={'exposure': 0.5})
    detail['refused'] = dict(over=over, png=png, states=(before, states(document)))
    rows['refused'] = (states(document) == before and 'exposure' in over
                       and 'raw applies only to a camera RAW file.' in png)

    # undo / redo: the place comes and goes bit-exact.
    app.call('undo', document_id=document)
    undone = float_export(document, 'undone')
    app.call('redo', document_id=document)
    redone = float_export(document, 'redone')
    detail['undo'] = dict(undo_bit_exact=bits_equal(undone, empty),
                          redo_bit_exact=bits_equal(redone, first))
    rows['undo'] = detail['undo']['undo_bit_exact']
    rows['redo'] = detail['undo']['redo_bit_exact']

    # reopened: the .rz carries the original and the recipe, bit-exact.
    saved = folder / 'f36.rz'
    app.call('save_copy', document_id=document, path=str(saved), format='rz')
    reopened = app.open(saved)
    detail['reopened'] = dict(bit_exact=bits_equal(float_export(reopened, 'reopened'), first),
                              raw_equal=raw(reopened, layer)['raw'] == raw(document,
                                                                         layer)['raw'])
    rows['reopened'] = detail['reopened'] == dict(bit_exact=True, raw_equal=True)
    detail['rows'] = rows
    record(report, 'F36 a camera RAW file placed at every depth',
           all(rows.values()), detail)


# ------------------------------------------------------------------- F37 --

F37_INK = [4.0, 0.5, 0.25, 1.0]
F37_GRAY = 128
# A stroke along row 16 of the ramp, and one inside the 8.0 block.
F37_LINE = ((10.5, 16.5), (30.5, 16.5))
F37_BLOCK_LINE = ((3.5, 4.5), (8.5, 4.5))
# The saved selection of row (j): clear of F37_LINE's stroke.
F37_SAVED = (38, 8, 8, 6)
F37_PLANE = 'Painting one colour channel is unavailable in a 32-bit float document.'
F37_COVERAGE_INK = "rgba32, start_rgba32 and end_rgba32 colour a float layer's pixels."
F37_SCREEN = 'The Screen blend mode is unavailable in a 32-bit float document.'
F37_PAIR = 'Pass start_rgba32 and end_rgba32 together'
F37_UNAVAILABLE = ' is unavailable in a 32-bit float document'


def f37_points(line):
    return [{'x': x, 'y': y} for x, y in line]


def f37_distance(line, x, y):
    """The distance from pixel (x, y)'s centre to the segment `line`."""
    (ax, ay), (bx, by) = line
    px, py = x + 0.5, y + 0.5
    dx, dy = bx - ax, by - ay
    t = min(max(((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy), 0.0), 1.0)
    return math.hypot(px - (ax + t * dx), py - (ay + t * dy))


def f37_stroke(line, size):
    """A hard stroke's regions: 'in' its core, None far from it, 'skip' the
    tip's edge between the two."""
    radius = size / 2.0

    def where(x, y):
        d = f37_distance(line, x, y)
        return 'in' if d <= radius - 2.0 else None if d >= radius + 2.0 else 'skip'
    return where


def f37_check(got, base, region, want, tolerance=T_REL):
    """`f32_check` with the pixel's position passed to `want`, and `None` as
    a wanted value for a component the row does not read."""
    worst, exact, read = 0.0, True, 0
    for y in range(HEIGHT):
        for x in range(WIDTH):
            i = (y * WIDTH + x) * 4
            where = region(x, y)
            if where == 'skip':
                continue
            wanted = want(x, y, base[i:i + 4]) if where == 'in' else {}
            for k in range(4):
                if k in wanted:
                    if wanted[k] is None:
                        continue
                    read += 1
                    worst = max(worst, abs(got[i + k] - wanted[k]) / max(1.0, abs(wanted[k])))
                elif not bits_equal(got[i + k:i + k + 1], base[i + k:i + k + 1]):
                    exact = False
    return (exact and read > 0 and worst <= tolerance,
            dict(worst=worst, others_bit_exact=exact, samples_read=read))


def f37_render(app, document, **arguments):
    """F12's `render` call: the reply's PNG as pixels, with its sha256."""
    reply = app.client.rpc('tools/call', {'name': 'render', 'arguments': dict(
        document_id=document, max_side=4096, **arguments)})
    text = ' '.join(item.get('text', '') for item in reply.get('content', [])
                    if item.get('type') == 'text')
    assert not reply.get('isError'), text
    raw = base64.b64decode(next(item['data'] for item in reply['content']
                                if item['type'] == 'image'))
    width, height, pixels = read_png(raw)
    return dict(width=width, height=height, pixels=pixels,
                sha256=hashlib.sha256(raw).hexdigest())


def gate_f37(app, folder, report):
    """The Brush, Eraser and Gradient on a float layer, its mask and an
    alpha channel."""
    copy = folder / 'f37-scene.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    doc = app.open(copy)
    base = scene()
    rows, detail = {}, dict(tolerance=T_REL, ink=F37_INK)
    wide, block = f37_stroke(F37_LINE, 8), f37_stroke(F37_BLOCK_LINE, 6)
    everywhere = lambda x, y: 'in'
    across = dict(x0=0.5, y0=16, x1=WIDTH - 0.5, y1=16)

    def after(label):
        return export32(app, doc, folder / f'f37-{label}.tiff')['samples']

    def undone(label, to=None):
        app.call('undo', document_id=doc)
        return bits_equal(after(label + '-undo'), base if to is None else to)

    def stroke(tool_name, line=F37_LINE, size=8, **arguments):
        return app.call(tool_name, document_id=doc, points=f37_points(line), size=size,
                        hardness=100, spacing=10, **arguments)

    # (a) a document hex: the ink decoded through the profile's own curves.
    tables = icc_trc_tables(read_float_tiff(FIXTURE)['icc'])
    decoded = [curve_eval(F37_GRAY / 255.0, table) for table in tables]
    reply = stroke('brush_stroke', layer=0, opacity=1, color='#808080', color_space='document')
    rows['a_ink'], detail['a'] = f32_check(
        after('a'), base, wide, lambda p: {0: decoded[0], 1: decoded[1], 2: decoded[2], 3: 1.0})
    rows['a_reply'] = reply.get('storage') == 'rgba32_float' and reply.get('changed') is True
    detail['a_reply'], detail['a_decoded'] = reply, decoded
    rows['a_undo'] = undone('a')

    # (b) above-white ink is its own numbers, and nothing clamps the blend.
    ink = {k: F37_INK[k] for k in range(4)}
    stroke('brush_stroke', layer=0, opacity=1, rgba32=F37_INK)
    rows['b_ink'], detail['b'] = f32_check(after('b'), base, wide, lambda p: ink)
    rows['b_undo'] = undone('b')
    stroke('brush_stroke', line=F37_BLOCK_LINE, size=6, layer=0, opacity=0.5, rgba32=F37_INK)
    got = after('b-half')
    rows['b_no_clamp'], detail['b_half'] = f32_check(
        got, base, block,
        lambda p: {0: 0.5 * F37_INK[0] + 0.5 * p[0], 1: 0.5 * F37_INK[1] + 0.5 * p[1],
                   2: 0.5 * F37_INK[2] + 0.5 * p[2], 3: 1.0})
    detail['b_half_red'] = got[(4 * WIDTH + 5) * 4]
    rows['b_red_reads_6'] = abs(detail['b_half_red'] - 6.0) <= 6.0 * T_REL
    rows['b_half_undo'] = undone('b-half')

    # (c) the Eraser halves alpha and reads no colour.
    reply = stroke('eraser_stroke', layer=0, opacity=0.5)
    rows['c_erase'], detail['c'] = f32_check(after('c'), base, wide, lambda p: {3: p[3] * 0.5})
    rows['c_reply'] = reply.get('storage') == 'rgba32_float'
    rows['c_undo'] = undone('c')

    # (d) the Gradient interpolates in linear light, above white included.
    # Smoothness 0 is the linear ramp; the tool's default is 100 %.
    column = lambda x: x / (WIDTH - 1.0)
    reply = app.call('gradient', document_id=doc, layer=0, start_rgba32=[0, 0, 0, 1],
                     end_rgba32=[8, 8, 8, 1], smoothness=0, **across)
    rows['d_hdr'], detail['d_hdr'] = f37_check(
        after('d-hdr'), base, everywhere,
        lambda x, y, p: {0: 8.0 * column(x), 1: 8.0 * column(x), 2: 8.0 * column(x), 3: 1.0})
    rows['d_reply'] = reply.get('storage') == 'rgba32_float' and reply.get('changed') is True
    rows['d_hdr_undo'] = undone('d-hdr')
    app.call('gradient', document_id=doc, layer=0, start_color='#000000', end_color='#ffffff',
             color_space='document', smoothness=0, **across)
    rows['d_linear'], detail['d_linear'] = f37_check(
        after('d-linear'), base, everywhere,
        lambda x, y, p: {0: column(x), 1: column(x), 2: column(x), 3: 1.0})
    rows['d_linear_undo'] = undone('d-linear')

    # (e) a mask is u16 coverage: the Eraser on it hides the stroke's core.
    app.call('add_layer_mask', document_id=doc, layer=0)
    masked = after('e-mask')
    detail['e_mask_keeps_export'] = bits_equal(masked, base)
    reply = stroke('eraser_stroke', layer=0, target='mask', opacity=1)
    rows['e_mask'], detail['e'] = f37_check(
        after('e'), masked, wide, lambda x, y, p: {0: None, 1: None, 2: None, 3: 0.0})
    rows['e_reply'] = reply.get('storage') == 'u16' and reply.get('target') == 'mask'
    detail['e_reply'] = reply
    rows['e_undo'] = undone('e', masked)

    # (i) a Gradient on the same fresh mask: the ramp as coverage.
    reply = app.call('gradient', document_id=doc, layer=0, target='mask',
                     start_color='#000000', end_color='#ffffff', smoothness=0, **across)
    rows['i_mask'], detail['i'] = f37_check(
        after('i'), masked, everywhere,
        lambda x, y, p: {0: None, 1: None, 2: None,
                         3: p[3] * round(65535 * column(x)) / 65535.0},
        tolerance=2.0 / 65535.0)
    rows['i_reply'] = reply.get('storage') == 'u16' and reply.get('changed') is True
    detail['i_reply'] = reply
    rows['i_undo'] = undone('i', masked)
    rows['mask_undo'] = undone('mask')

    # (f) the refusals, each by name, and nothing painted.
    hdr_ends = dict(start_rgba32=[0, 0, 0, 1], end_rgba32=[8, 8, 8, 1])
    # `fill` and `dodge_burn` are admitted (F46).
    refusals = dict(
        screen=stroke('brush_stroke', fail=True, layer=0, color='#ffffff', blend_mode='Screen'),
        plane=stroke('brush_stroke', fail=True, layer=0, color='#ffffff', target='red'),
        both=stroke('brush_stroke', fail=True, layer=0, color='#ffffff', rgba32=F37_INK),
        one_endpoint=app.call('gradient', fail=True, document_id=doc, layer=0,
                              start_rgba32=[0, 0, 0, 1], **across),
        mask_ink=stroke('brush_stroke', fail=True, layer=0, target='mask', rgba32=F37_INK),
        gradient_mask_ink=app.call('gradient', fail=True, document_id=doc, layer=0,
                                   target='mask', **hdr_ends, **across))
    app.call('add_adjustment_layer', document_id=doc, op='exposure', params={'exposure': 1.0})
    layers = app.call('get_document', document_id=doc)['layers']
    adjustment = next(row for row in layers if row.get('is_adjustment'))
    steps = 1
    if not adjustment.get('has_mask'):
        app.call('add_layer_mask', document_id=doc, layer=adjustment['index'])
        steps = 2
    adjusted = after('f-adjustment')
    refusals['adjustment_ink'] = stroke('brush_stroke', fail=True, layer=adjustment['index'],
                                        rgba32=F37_INK)
    rows['f_adjustment_mask_unpainted'] = bits_equal(after('f-adjustment-after'), adjusted)
    for _ in range(steps):
        app.call('undo', document_id=doc)
    rows['f_screen'] = F37_SCREEN in refusals['screen']
    rows['f_plane'] = F37_PLANE in refusals['plane']
    rows['f_both'] = F32_BOTH in refusals['both']
    rows['f_one_endpoint'] = F37_PAIR in refusals['one_endpoint']
    for name in ('mask_ink', 'gradient_mask_ink', 'adjustment_ink'):
        rows['f_' + name] = F37_COVERAGE_INK in refusals[name]
    rows['f_unchanged'] = bits_equal(after('f'), base)
    detail['f_refusals'] = refusals
    detail['f_adjustment'] = dict(index=adjustment['index'], mask_added_here=steps == 2)

    # (j) an alpha channel is u16 coverage and no document pixel.
    app.call('select_rect', document_id=doc, x=F37_SAVED[0], y=F37_SAVED[1],
             width=F37_SAVED[2], height=F37_SAVED[3])
    app.call('save_selection', document_id=doc, name='F37')
    app.call('deselect', document_id=doc)
    plain = f37_render(app, doc, channel='F37')
    reply = stroke('brush_stroke', target='channel:F37', opacity=1, color='#ffffff')
    painted = f37_render(app, doc, channel='F37')
    saved = f32_inside(F37_SAVED)
    worst_in = worst_out = 0
    for y in range(HEIGHT):
        for x in range(WIDTH):
            value = painted['pixels'][y * WIDTH + x][0]
            where = wide(x, y)
            if where == 'in' or saved(x, y):
                worst_in = max(worst_in, 255 - value)
            elif where is None:
                worst_out = max(worst_out, value)
    rows['j_channel'] = ((painted['width'], painted['height']) == (WIDTH, HEIGHT)
                         and worst_in == 0 and worst_out == 0)
    rows['j_pixels_untouched'] = bits_equal(after('j'), base)
    rows['j_reply'] = reply.get('storage') == 'u16' and reply.get('target') == 'channel:F37'
    app.call('undo', document_id=doc)
    rows['j_undo'] = (f37_render(app, doc, channel='F37')['sha256'] == plain['sha256']
                      and painted['sha256'] != plain['sha256'])
    detail['j'] = dict(worst_in=worst_in, worst_out=worst_out, reply=reply)

    # (g) a background stroke is a job the caller polls.
    job = stroke('brush_stroke', layer=0, opacity=1, rgba32=F37_INK, background=True)
    status, deadline = dict(state='running'), time.monotonic() + 60
    while job.get('job_id') and status.get('state') == 'running':
        status = app.call('job_status', job_id=job['job_id'])
        assert time.monotonic() < deadline, status
        time.sleep(0.02)
    current = after('g')
    rows['g_job'] = bool(job.get('job_id')) and status.get('state') == 'completed'
    rows['g_applied'], detail['g'] = f32_check(current, base, wide, lambda p: ink)
    detail['g_job'] = dict(first=job, last=status)

    # (h) the painted document persists bit for bit.
    native = folder / 'f37.rz'
    app.call('save_copy', document_id=doc, path=str(native), format='rz')
    words = first_words(native, 3)
    reopened = app.open(native)
    rows['h_reopened'] = (words == [46, 40, 39] and bits_equal(
        export32(app, reopened, folder / 'f37-reopened.tiff')['samples'], current))
    detail['h_first_words'] = words

    detail['rows'] = rows
    record(report, 'F37 the Brush, Eraser and Gradient paint a float layer in linear light, '
                   'and its mask and an alpha channel as coverage', all(rows.values()), detail)
# ------------------------------------------ F38: OpenEXR, Radiance, exposure --

def f38_f32(value):
    """`value` rounded to binary32, as a Python float."""
    return struct.unpack('<f', struct.pack('<f', value))[0]


def f38_ulps(a, b):
    """The distance between two f32 values in representable values: each
    bit pattern placed on one number line (negatives mirrored)."""
    def ordered(value):
        bits = struct.unpack('<I', struct.pack('<f', value))[0]
        return -(bits & 0x7fffffff) if bits & 0x80000000 else bits
    return abs(ordered(a) - ordered(b))


def f38_write_exr(path, width, height, channels, chromaticities=None):
    """An uncompressed scanline OpenEXR file, written from the file layout
    in the OpenEXR specification: magic, version 2, the attribute list, one
    offset per scanline, then each scanline's channels in name order.
    `channels` maps a name to `(kind, values)`: kind `'f'` is FLOAT and
    `'e'` HALF, both packed by `struct`."""
    def text(value):
        return value.encode() + b'\0'

    def attribute(name, kind, value):
        return text(name) + text(kind) + struct.pack('<i', len(value)) + value

    names = sorted(channels)
    chlist = b''
    for name in names:
        chlist += text(name) + struct.pack('<iB3xii', {'e': 1, 'f': 2}[channels[name][0]],
                                            0, 1, 1)
    chlist += b'\0'
    window = struct.pack('<4i', 0, 0, width - 1, height - 1)
    header = attribute('channels', 'chlist', chlist)
    if chromaticities is not None:
        header += attribute('chromaticities', 'chromaticities',
                            struct.pack('<8f', *chromaticities))
    header += attribute('compression', 'compression', b'\0')
    header += attribute('dataWindow', 'box2i', window)
    header += attribute('displayWindow', 'box2i', window)
    header += attribute('lineOrder', 'lineOrder', b'\0')
    header += attribute('pixelAspectRatio', 'float', struct.pack('<f', 1.0))
    header += attribute('screenWindowCenter', 'v2f', struct.pack('<2f', 0.0, 0.0))
    header += attribute('screenWindowWidth', 'float', struct.pack('<f', 1.0))
    header += b'\0'
    head = b'\x76\x2f\x31\x01' + struct.pack('<I', 2) + header
    lines = []
    for y in range(height):
        row = b''
        for name in names:
            kind, values = channels[name]
            row += struct.pack('<%d%s' % (width, kind), *values[y * width:(y + 1) * width])
        lines.append(struct.pack('<ii', y, len(row)) + row)
    offsets, at = [], len(head) + 8 * height
    for line in lines:
        offsets.append(at)
        at += len(line)
    Path(path).write_bytes(head + struct.pack('<%dQ' % height, *offsets) + b''.join(lines))
    return path


def f38_read_exr(path):
    """A single-part scanline OpenEXR file as this build writes one, read
    here rather than by a library: the attributes, the offset table and
    each chunk — stored, or ZIP (zlib, then the format's byte predictor and
    its two-halves interleave undone). Returns the channels as
    `{name: (kind, values)}` plus the chromaticities."""
    data = Path(path).read_bytes()
    assert data[:4] == b'\x76\x2f\x31\x01', data[:4]
    assert struct.unpack_from('<I', data, 4)[0] == 2, 'a single-part scanline file'
    at = 8
    attributes = {}

    def text():
        nonlocal at
        end = data.index(b'\0', at)
        value = data[at:end].decode()
        at = end + 1
        return value

    while data[at] != 0:
        name, kind = text(), text()
        size = struct.unpack_from('<i', data, at)[0]
        attributes[name] = (kind, data[at + 4:at + 4 + size])
        at += 4 + size
    at += 1
    chlist = attributes['channels'][1]
    channels, cursor = [], 0
    while chlist[cursor] != 0:
        end = chlist.index(b'\0', cursor)
        name = chlist[cursor:end].decode()
        kind, _, xs, ys = struct.unpack_from('<iB3xii', chlist, end + 1)
        assert (xs, ys) == (1, 1), (xs, ys)
        channels.append((name, {1: 'e', 2: 'f'}[kind]))
        cursor = end + 17
    x0, y0, x1, y1 = struct.unpack('<4i', attributes['dataWindow'][1])
    width, height = x1 - x0 + 1, y1 - y0 + 1
    compression = attributes['compression'][1][0]
    per_block = {0: 1, 2: 1, 3: 16}[compression]
    blocks = -(-height // per_block)
    offsets = struct.unpack_from('<%dQ' % blocks, data, at)
    line_bytes = sum(width * (2 if kind == 'e' else 4) for _, kind in channels)
    values = {name: (kind, [0.0] * (width * height)) for name, kind in channels}
    for offset in offsets:
        y, size = struct.unpack_from('<ii', data, offset)
        rows = min(per_block, y1 - y + 1)
        raw = data[offset + 8:offset + 8 + size]
        if size != rows * line_bytes:
            packed = bytearray(zlib.decompress(raw))
            for i in range(1, len(packed)):
                packed[i] = (packed[i - 1] + packed[i] - 128) & 0xff
            half = (len(packed) + 1) // 2
            raw = bytearray(len(packed))
            raw[0::2] = packed[:half]
            raw[1::2] = packed[half:]
        assert len(raw) == rows * line_bytes, (len(raw), rows, line_bytes)
        cursor = 0
        for row in range(rows):
            for name, kind in channels:
                step = 2 if kind == 'e' else 4
                line = struct.unpack_from('<%d%s' % (width, kind), raw, cursor)
                cursor += width * step
                start = (y - y0 + row) * width
                values[name][1][start:start + width] = line
    chroma = attributes.get('chromaticities')
    return dict(width=width, height=height, channels=values, compression=compression,
                chromaticities=list(struct.unpack('<8f', chroma[1])) if chroma else None,
                has_icc=any('icc' in name.lower() for name in attributes))


def f38_rgbe_value(pixel):
    """One RGBE pixel's three values: `m * 2**(e - 136)`, zero when `e` is 0."""
    if pixel[3] == 0:
        return [0.0, 0.0, 0.0]
    return [f38_f32(pixel[c] * 2.0 ** (pixel[3] - 136)) for c in range(3)]


def f38_write_rgbe(path, width, height, pixels):
    """A flat (not run-length encoded) Radiance file: the text header, the
    resolution line and four bytes a pixel."""
    head = b'#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n' + ('-Y %d +X %d\n' % (height, width)).encode()
    assert len(pixels) == width * height
    Path(path).write_bytes(head + b''.join(bytes(pixel) for pixel in pixels))
    return path


def f38_read_rgbe(path):
    """A Radiance file as this build writes one, read here: the header (the
    `PRIMARIES=` line with it), the `-Y H +X W` line, then scanlines in the
    new run-length scheme (marker 2 2 hi lo, four components of runs and
    literals) or flat pixels."""
    data = Path(path).read_bytes()
    assert data.startswith(b'#?RADIANCE'), data[:12]
    end = data.index(b'\n\n')
    header = data[:end].decode('latin-1').split('\n')
    at = data.index(b'\n', end + 2)
    tokens = data[end + 2:at].decode().split()
    assert tokens[0] == '-Y' and tokens[2] == '+X', tokens
    height, width = int(tokens[1]), int(tokens[3])
    at += 1
    pixels = []
    for _ in range(height):
        if 8 <= width <= 32767 and data[at] == 2 and data[at + 1] == 2 and data[at + 2] < 128:
            assert data[at + 2] * 256 + data[at + 3] == width, 'scanline width'
            at += 4
            planes = []
            for _ in range(4):
                plane = bytearray()
                while len(plane) < width:
                    count = data[at]
                    if count > 128:
                        plane += bytes([data[at + 1]]) * (count - 128)
                        at += 2
                    else:
                        plane += data[at + 1:at + 1 + count]
                        at += 1 + count
                assert len(plane) == width, len(plane)
                planes.append(plane)
            pixels.extend(zip(*planes))
        else:
            for x in range(width):
                pixels.append(tuple(data[at + x * 4:at + x * 4 + 4]))
            at += width * 4
    primaries = next((line.split('=', 1)[1].split() for line in header
                      if line.startswith('PRIMARIES=')), None)
    return dict(width=width, height=height, pixels=pixels, header=header,
                primaries=[float(v) for v in primaries] if primaries else None)


F38_W, F38_H = 12, 6
F38_P3 = [0.680, 0.320, 0.265, 0.690, 0.150, 0.060, 0.3127, 0.3290]
F38_REC709 = [0.64, 0.33, 0.30, 0.60, 0.15, 0.06, 0.3127, 0.3290]


def f38_alpha_fixture():
    """The f32 OpenEXR fixture's STORED (associated) samples: an opaque block
    with values above white and one negative, a block at alpha 0.3, and a
    block at alpha 0 whose stored colour is not zero."""
    stored = []
    for y in range(F38_H):
        for x in range(F38_W):
            alpha = f38_f32([1.0, 0.3, 0.0][x // 4])
            v = ((x * 7 + y * 13) % 17) / 17.0
            scale = alpha if alpha > 0 else 1.0
            r, g, b = v * 6.0, 0.05 + v * 0.5, (-0.25 if (x + y) % 5 == 0 else 2.0 + v)
            stored.append([f38_f32(r * scale), f38_f32(g * scale), f38_f32(b * scale), alpha])
    return stored


def f38_straight(stored):
    """What the open holds for one stored pixel: divided by alpha, the
    canonical zero at alpha 0; alpha itself untouched."""
    r, g, b, a = stored
    if a == 1.0:
        return [r, g, b, a]
    if a > 0.0:
        return [f38_f32(r / a), f38_f32(g / a), f38_f32(b / a), a]
    return [0.0, 0.0, 0.0, a]


def gate_f38(app, folder, report, u16_document):
    """OpenEXR and Radiance in, the three float containers out,
    and the view exposure."""
    rows, detail = {}, {}

    def export(document, name, **settings):
        path = folder / name
        reply = app.call('export_image', document_id=document, path=str(path),
                         settings=dict(bit_depth=32, **settings), overwrite=True)
        assert reply['ok'], reply
        return path, reply

    def flat(document, name):
        return read_float_tiff(export(document, name, format='tiff')[0])['samples']

    # ---- the three fixtures open as float documents with their values ----
    stored = f38_alpha_fixture()
    exr32 = f38_write_exr(folder / 'f38-f32.exr', F38_W, F38_H, {
        'R': ('f', [p[0] for p in stored]), 'G': ('f', [p[1] for p in stored]),
        'B': ('f', [p[2] for p in stored]), 'A': ('f', [p[3] for p in stored])})
    alpha_doc = app.open(exr32)
    alpha_state = app.call('get_document', document_id=alpha_doc)
    straight = [f38_straight(p) for p in stored]
    alpha_flat = flat(alpha_doc, 'f38-f32-open.tiff')
    rows['open_exr_f32'] = (
        alpha_state.get('precision') == 'rgba32_float'
        and bits_equal(alpha_flat, [v for p in straight for v in p]))
    detail['open_exr_f32'] = dict(precision=alpha_state.get('precision'),
                                  import_warnings=alpha_state.get('import_warnings'))

    halves = [[struct.unpack('<e', struct.pack('<e', ((x * 5 + y * 3 + c) % 64) / 8.0 - 1.0))[0]
               for c in range(3)] for y in range(4) for x in range(8)]
    exr16 = f38_write_exr(folder / 'f38-half-p3.exr', 8, 4, {
        'R': ('e', [p[0] for p in halves]), 'G': ('e', [p[1] for p in halves]),
        'B': ('e', [p[2] for p in halves])}, chromaticities=F38_P3)
    p3_doc = app.open(exr16)
    p3_state = app.call('get_document', document_id=p3_doc)
    rows['open_exr_half_p3'] = (
        p3_state.get('precision') == 'rgba32_float'
        and 'P3' in (p3_state.get('color_profile') or {}).get('name', '')
        and bits_equal(flat(p3_doc, 'f38-half-open.tiff'),
                       [v for p in halves for v in p + [1.0]]))
    detail['open_exr_half_p3'] = dict(profile=(p3_state.get('color_profile') or {}).get('name'),
                                      import_warnings=p3_state.get('import_warnings'))

    rgbe = [(((x * 37 + y * 11) % 200) + 30, ((x * 53 + y * 7) % 220) + 20,
             ((x * 17 + y * 29) % 240) + 10, 124 + (x + 2 * y) % 10)
            for y in range(4) for x in range(10)]
    hdr = f38_write_rgbe(folder / 'f38.hdr', 10, 4, rgbe)
    hdr_doc = app.open(hdr)
    hdr_state = app.call('get_document', document_id=hdr_doc)
    hdr_linear = [f38_rgbe_value(p) for p in rgbe]
    rows['open_hdr'] = (
        hdr_state.get('precision') == 'rgba32_float'
        and bits_equal(flat(hdr_doc, 'f38-hdr-open.tiff'),
                       [v for p in hdr_linear for v in p + [1.0]]))
    detail['open_hdr'] = dict(precision=hdr_state.get('precision'),
                              import_warnings=hdr_state.get('import_warnings'),
                              above_white=max(max(p) for p in hdr_linear))

    # ---- the f32 OpenEXR round trip, per alpha block ----
    path, reply = export(alpha_doc, 'f38-roundtrip.exr', format='exr')
    back = f38_read_exr(path)
    worst = dict(opaque=0, fractional=0, transparent=0.0, alpha_exact=True)
    for i, pixel in enumerate(stored):
        got = [back['channels'][name][1][i] for name in 'RGBA']
        worst['alpha_exact'] &= bits_equal([got[3]], [pixel[3]])
        if pixel[3] == 1.0:
            worst['opaque'] = max(worst['opaque'], 0 if bits_equal(got[:3], pixel[:3]) else 1)
        elif pixel[3] > 0.0:
            worst['fractional'] = max(worst['fractional'],
                                      max(f38_ulps(got[c], pixel[c]) for c in range(3)))
        else:
            worst['transparent'] = max(worst['transparent'], max(abs(v) for v in got[:3]))
    rows['roundtrip'] = (
        worst['opaque'] == 0 and worst['transparent'] == 0.0 and worst['alpha_exact']
        and worst['fractional'] <= 2
        and all(kind == 'f' for kind, _ in back['channels'].values())
        and sorted(back['channels']) == ['A', 'B', 'G', 'R']
        and bits_equal(back['chromaticities'], F38_REC709) and not back['has_icc']
        and reply.get('float_samples') == 'f32' and reply.get('bit_depth') == 32
        and reply.get('carried') == [])
    detail['roundtrip'] = dict(worst=worst, compression=back['compression'],
                               chromaticities=back['chromaticities'],
                               float_samples=reply.get('float_samples'),
                               carried=reply.get('carried'))

    # ---- half samples equal struct's own 'e' rounding, in both containers ----
    def half(value):
        return struct.unpack('<e', struct.pack('<e', value))[0]

    path, reply = export(alpha_doc, 'f38-half.exr', format='exr', float_samples='f16')
    back = f38_read_exr(path)
    half_exr = all(
        struct.pack('<e', back['channels'][name][1][i])
        == struct.pack('<e', half(f38_f32(straight[i][c] * straight[i][3]) if c < 3
                                  else straight[i][3]))
        for i in range(len(stored)) for c, name in enumerate('RGBA'))
    path, tiff_reply = export(alpha_doc, 'f38-half.tiff', format='tiff', float_samples='f16')
    half_tiff = read_float_tiff(path)
    rows['half'] = (
        half_exr and all(kind == 'e' for kind, _ in back['channels'].values())
        and reply.get('float_samples') == 'f16'
        and half_tiff['bits'] == [16] * 4 and half_tiff['sample_format'] == [3] * 4
        and half_tiff['extra_samples'] == [2] and half_tiff['icc_bytes'] > 0
        and all(struct.pack('<e', got) == struct.pack('<e', half(want))
                for got, want in zip(half_tiff['samples'], [v for p in straight for v in p]))
        and tiff_reply.get('float_samples') == 'f16')
    detail['half'] = dict(exr=half_exr, tiff_bits=half_tiff['bits'],
                          tiff_extra_samples=half_tiff['extra_samples'])

    # ---- Radiance out: within the format's own mantissa step ----
    path, reply = export(alpha_doc, 'f38-out.hdr', format='hdr')
    written = f38_read_rgbe(path)
    worst_steps = 0.0
    for i, pixel in enumerate(straight):
        want = [f38_f32(max(pixel[c], 0.0) * pixel[3]) for c in range(3)]
        largest = max(want)
        got = f38_rgbe_value(written['pixels'][i])
        if largest <= 0.0:
            worst_steps = max(worst_steps, max(got))
            continue
        step = 2.0 ** (math.floor(math.log2(largest)) + 1) / 256.0
        worst_steps = max(worst_steps, max(abs(got[c] - want[c]) for c in range(3)) / step)
    rows['radiance_out'] = (
        worst_steps <= 1.001 and (written['width'], written['height']) == (F38_W, F38_H)
        and written['primaries'] is not None
        and all(abs(a - b) < 1e-6 for a, b in zip(written['primaries'], F38_REC709))
        and reply.get('float_samples') == 'rgbe' and reply.get('carried') == [])
    detail['radiance_out'] = dict(worst_steps=worst_steps, primaries=written['primaries'],
                                  float_samples=reply.get('float_samples'))

    # ---- save_copy writes export_image's bytes ----
    path, _ = export(alpha_doc, 'f38-export.exr', format='exr')
    copy = folder / 'f38-copy.exr'
    saved = app.call('save_copy', document_id=alpha_doc, path=str(copy))
    rows['save_copy'] = (digest(copy) == digest(path) and saved.get('bit_depth') == 32
                         and saved.get('float_samples') == 'f32'
                         and saved.get('master_bit_depth') == 32)
    detail['save_copy'] = dict(sha256=digest(copy), export_sha256=digest(path),
                               bit_depth=saved.get('bit_depth'),
                               losses=saved.get('flat_save_losses'))

    # ---- render's exposure, against the gain, the clip and the sRGB curve ----
    def render(document, **arguments):
        result = app.client.rpc('tools/call', {'name': 'render', 'arguments': dict(
            document_id=document, max_side=4096, **arguments)})
        text = ' '.join(item.get('text', '') for item in result.get('content', [])
                        if item.get('type') == 'text')
        if result.get('isError'):
            return dict(error=True, text=text)
        raw = base64.b64decode(next(item['data'] for item in result['content']
                                    if item['type'] == 'image'))
        width, height, pixels = read_png(raw)
        return dict(error=False, text=text, pixels=pixels,
                    sha256=hashlib.sha256(raw).hexdigest())

    worst_codes = {}
    for stops in (-3.0, 2.0):
        shown = render(hdr_doc, exposure=stops)
        gain = 2.0 ** stops
        worst_codes[stops] = max(
            abs(shown['pixels'][i][c] - round(srgb_encode(tone_map(v * gain, 'clip')) * 255.0))
            for i, pixel in enumerate(hdr_linear) for c, v in enumerate(pixel))
    plain, zero, dark = render(hdr_doc), render(hdr_doc, exposure=0), render(hdr_doc, exposure=-3)
    rows['render_exposure'] = (
        max(worst_codes.values()) <= T_CODE8 and plain['sha256'] == zero['sha256']
        and plain['sha256'] != dark['sha256'] and 'exposure' in dark['text'])
    detail['render_exposure'] = dict(tolerance=T_CODE8, worst_codes=worst_codes,
                                     caption=dark['text'])

    # ---- the window's own exposure: view state, drawn asynchronously ----
    def dirty(document):
        return next(row for row in app.call('list_documents')['documents']
                    if row['id'] == document)['unsaved_changes']

    before = (app.call('get_document', document_id=hdr_doc), dirty(hdr_doc))
    fresh = app.call('get_view', document_id=hdr_doc)
    set_reply = app.call('set_view', document_id=hdr_doc, exposure=-3)
    shown = phase5_support.wait_for(
        lambda: app.call('get_view', document_id=hdr_doc),
        lambda state: state.get('exposure_displayed') is True, timeout=10,
        detail='the exposed image to reach the canvas')
    during = (app.call('get_document', document_id=hdr_doc), dirty(hdr_doc))
    # The window's exposure is not the render's: a render with no argument
    # is still the document's own display.
    unexposed = render(hdr_doc)
    reset = app.call('set_view', document_id=hdr_doc, exposure=0)
    after = (app.call('get_document', document_id=hdr_doc), dirty(hdr_doc))
    rows['view_exposure'] = (
        (fresh.get('exposure'), fresh.get('exposure_displayed')) == (0, False)
        and set_reply.get('exposure') == -3 and shown.get('exposure') == -3
        and (reset.get('exposure'), reset.get('exposure_displayed')) == (0, False)
        and before == during == after
        and unexposed['sha256'] == plain['sha256'])
    detail['view_exposure'] = dict(fresh=[fresh.get('exposure'), fresh.get('exposure_displayed')],
                                   set_reply=[set_reply.get('exposure'),
                                              set_reply.get('exposure_displayed')],
                                   displayed=shown.get('exposure_displayed'),
                                   reset=[reset.get('exposure'), reset.get('exposure_displayed')],
                                   document_unchanged=before == during == after,
                                   unsaved_changes=before[1])

    # ---- the refusals, each by name ----
    refused = {}
    copy = folder / 'f38-rgba8.exr'
    copy.write_bytes(Path(exr32).read_bytes())
    refused['open_exr_as_rgba8'] = (
        app.call('open_document', fail=True, path=str(copy), precision='rgba8'), 'OpenEXR')
    refused['exr_from_u16'] = (
        app.call('export_image', fail=True, document_id=u16_document,
                 path=str(folder / 'f38-refused.exr'),
                 settings={'format': 'exr', 'bit_depth': 32}, overwrite=True),
        '32-bit float document')
    refused['exr_at_16'] = (
        app.call('export_image', fail=True, document_id=alpha_doc,
                 path=str(folder / 'f38-refused16.exr'),
                 settings={'format': 'exr', 'bit_depth': 16}, overwrite=True),
        '32-bit float only')
    refused['hdr_half'] = (
        app.call('export_image', fail=True, document_id=alpha_doc,
                 path=str(folder / 'f38-refused.hdr'),
                 settings={'format': 'hdr', 'bit_depth': 32, 'float_samples': 'f16'},
                 overwrite=True), 'tiff or exr')
    refused['render_exposure_u16'] = (
        app.call('render', fail=True, document_id=u16_document, exposure=-3), '32-bit float')
    refused['set_view_exposure_u16'] = (
        app.call('set_view', fail=True, document_id=u16_document, exposure=-3), '32-bit float')
    refused['set_view_exposure_25'] = (
        app.call('set_view', fail=True, document_id=hdr_doc, exposure=25), '20')
    refused['render_exposure_25'] = (
        app.call('render', fail=True, document_id=hdr_doc, exposure=25), '20')
    refused['render_exposure_layer'] = (
        app.call('render', fail=True, document_id=hdr_doc, exposure=-1, layer=0), 'layer')
    rows['refusals'] = (
        all(named in message for message, named in refused.values())
        and not (folder / 'f38-refused.exr').exists()
        and app.call('get_view', document_id=u16_document).get('exposure') == 0)
    detail['refusals'] = {name: message for name, (message, _) in refused.items()}

    detail['rows'] = rows
    record(report, 'F38 OpenEXR and Radiance in and out, and the view exposure',
           all(rows.values()), detail)


# ------------------------------------------------------------------- F39 --

F39_FILTERS = [
    ('blur', dict(sigma=2)),
    ('box_blur', dict(radius=1)),
    ('motion_blur', dict(angle=30, distance=6)),
    ('radial_blur', dict(amount=20)),
    ('lens_blur', dict(radius=3, brightness=50, threshold=128)),
    ('tilt_shift', dict(radius=4, focus=0.05, transition=0.3)),
    ('offset', dict(horizontal=7, vertical=-5, undefined_areas='wrap')),
    ('lens_correction', dict(distortion=40, vignette_amount=-30)),
    ('unsharp_mask', dict(radius=2, amount=1.5)),
    ('high_pass', dict(radius=3)),
]
F39_RECT = (12, 8, 20, 14)
F39_PUSH = dict(tool='forward_warp', points=[[20, 16], [26, 16]], size=16, strength=100,
                density=100)


def f39_offset(base, horizontal, vertical):
    """Offset with Wrap Around: pixel (x, y) takes (x − h, y − v) modulo the
    plane, so a positive value moves content right and down."""
    out = []
    for y in range(HEIGHT):
        for x in range(WIDTH):
            i = (((y - vertical) % HEIGHT) * WIDTH + (x - horizontal) % WIDTH) * 4
            out.extend(base[i:i + 4])
    return out


def f39_box(base, radius):
    """Box Blur: the premultiplied mean over the (2r + 1)² square with clamped
    edges, turned back to straight colour; below 2^-24 of mean alpha the
    pixel is transparent black."""
    out, count = [], (2 * radius + 1) ** 2
    for y in range(HEIGHT):
        for x in range(WIDTH):
            total = [0.0, 0.0, 0.0, 0.0]
            for dy in range(-radius, radius + 1):
                for dx in range(-radius, radius + 1):
                    sx = min(max(x + dx, 0), WIDTH - 1)
                    sy = min(max(y + dy, 0), HEIGHT - 1)
                    r, g, b, a = base[(sy * WIDTH + sx) * 4:(sy * WIDTH + sx) * 4 + 4]
                    total = [total[0] + r * a, total[1] + g * a, total[2] + b * a, total[3] + a]
            alpha = total[3] / count
            if alpha < 2.0 ** -24:
                out.extend([0.0, 0.0, 0.0, 0.0])
            else:
                out.extend([total[0] / total[3], total[1] / total[3], total[2] / total[3],
                            min(alpha, 1.0)])
    return out


def f39_close(got, want):
    worst = max(abs(g - w) / max(1.0, abs(w)) for g, w in zip(got, want))
    return worst <= T_REL, worst


def gate_f39(app, folder, report):
    """The eleven filters a float layer runs, over MCP."""
    copy = folder / 'f39-scene.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    doc = app.open(copy)
    base = scene()
    rows, detail = {}, dict(tolerance=T_REL, filters={})

    def samples(label):
        return export32(app, doc, folder / f'f39-{label}.tiff')['samples']

    rows['base'] = bits_equal(samples('base'), base)
    names = [name for name, _ in F39_FILTERS]
    listed = app.call('get_precision', document_id=doc).get('filters')
    rows['listed'] = set(names + ['liquify']) <= set(listed or [])
    detail['listed'] = listed

    applied = {}
    for name, arguments in F39_FILTERS:
        reply = app.call('apply_filter', document_id=doc, layer=0, filter=name, **arguments)
        applied[name] = samples(name)
        app.call('undo', document_id=doc)
        undone = bits_equal(samples(name + '-undo'), base)
        rows[name] = (reply.get('changed') is True and reply.get('storage') == 'rgba32_float'
                      and all(math.isfinite(v) for v in applied[name])
                      and not bits_equal(applied[name], base) and undone)
        detail['filters'][name] = dict(reply=reply, undo_bit_exact=undone)

    rows['offset_map'] = bits_equal(applied['offset'], f39_offset(base, 7, -5))
    rows['box_mean'], detail['box_worst'] = f39_close(applied['box_blur'], f39_box(base, 1))
    reds = applied['blur'][0::4]
    rows['above_white'] = max(base[0::4]) > 1.0 and max(reds) > 1.0
    detail['above_white'] = dict(before=max(base[0::4]), after=max(reds))

    # A selection confines the write; the filter still reads the whole layer.
    x0, y0, w, h = F39_RECT
    app.call('select_rect', document_id=doc, x=x0, y=y0, width=w, height=h)
    reply = app.call('apply_filter', document_id=doc, layer=0, filter='blur', sigma=2)
    selected = samples('selected')
    outside, inside = True, False
    for y in range(HEIGHT):
        for x in range(WIDTH):
            i = (y * WIDTH + x) * 4
            same = bits_equal(selected[i:i + 4], base[i:i + 4])
            if x0 <= x < x0 + w and y0 <= y < y0 + h:
                inside = inside or not same
            else:
                outside = outside and same
    rows['selection'] = outside and inside and reply.get('write_scope') == 'selection'
    detail['selection'] = dict(reply=reply, outside_bit_exact=outside, inside_changed=inside)
    app.call('undo', document_id=doc)
    app.call('deselect', document_id=doc)
    rows['selection_undo'] = bits_equal(samples('selected-undo'), base)

    # Liquify: one full-strength push inside the picture.
    reply = app.call('liquify', document_id=doc, strokes=[F39_PUSH])
    pushed = samples('liquify')
    app.call('undo', document_id=doc)
    rows['liquify'] = (reply.get('changed') is True and reply.get('storage') == 'rgba32_float'
                       and all(math.isfinite(v) for v in pushed)
                       and not bits_equal(pushed, base)
                       and bits_equal(samples('liquify-undo'), base))
    detail['liquify'] = reply

    refused = {}
    refused['mask'] = app.call('apply_filter', fail=True, document_id=doc, layer=0,
                               filter='blur', sigma=2, target='mask')
    # Filters run on a float mask, so a maskless layer refuses for
    # the missing mask.
    rows['mask_refuses'] = refused['mask'] == 'The target layer has no mask.'
    rows['refusals_change_nothing'] = bits_equal(samples('refused'), base)
    detail['refusals'] = refused

    detail['rows'] = rows
    record(report, 'F39 eleven filters run on a float layer in linear light and undo',
           all(rows.values()), detail)


# ------------------------------------------------------------------- F43 --

# Fixed before anything is measured, never widened.
F43_PORTRAIT = Path.home() / 'devel/doctored-fixtures/phase6/photos/portrait.png'   # 512 x 512, absent = FAIL
F43_HOT, F43_GAIN, F43_HOT_MIN = (110, 300, 180, 130), 4.0, 5000   # x, y, w, h of the suit's chest; samples > 1.0
F43_FACE, F43_CHEST = (225, 113), (150, 340)
F43_BACKGROUND = [(501, 10), (115, 40), (335, 40)]   # plain backdrop (top-right corner, beside the head)
F43_PEOPLE_CORNERS = [(10, 10), (501, 10)]           # the flag is no person; the bottom corners hold the suit and helmet
F43_INSIDE, F43_OUTSIDE = 32768, 655                 # coverage16: at least half; at most 1 %
F43_STRIDE, F43_IOU, F43_MIN_CLASS = 4, 0.95, 1000   # lattice; IoU of coverage16 >= 32768; lattice points per class
F43_TOOLS = ['select_subject', 'inspect_subjects', 'select_subjects', 'select_people',
             'object_select']
# Row 40 crosses the backdrop on both sides of the head and the hair between.
F43_LINE = ((100.5, 40.5), (350.5, 40.5))
F43_INK = [4.0, 0.5, 0.25, 1.0]
F43_CLONE_DOWN = 200


def f43_picture() -> tuple[int, int, list[float]]:
    """The portrait as linear f32 RGBA: each RGB code decoded by IEC
    61966-2.1 (written out here), alpha 1.0, and RGB times `F43_GAIN` inside
    `F43_HOT`; every value rounded through f32, as the file stores it."""
    data = F43_PORTRAIT.read_bytes()
    width, height, pixels = read_png(data)
    full = 255.0 if data[24] == 8 else 65535.0

    def decode(code):
        c = code / full
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4

    x0, y0, w, h = F43_HOT
    out = []
    for i, (r, g, b, _) in enumerate(pixels):
        x, y = i % width, i // width
        gain = F43_GAIN if x0 <= x < x0 + w and y0 <= y < y0 + h else 1.0
        out.extend((decode(r) * gain, decode(g) * gain, decode(b) * gain, 1.0))
    layout = '<%df' % len(out)
    return width, height, list(struct.unpack(layout, struct.pack(layout, *out)))


def f43_coverage(app, document, points):
    """`get_selection`'s `coverage16` at `points`, 4096 a call (its limit)."""
    out = []
    for start in range(0, len(points), 4096):
        reply = app.call('get_selection', document_id=document,
                         points=[{'x': x, 'y': y} for x, y in points[start:start + 4096]])
        out.extend(row['coverage16'] for row in reply.get('samples') or [])
    assert len(out) == len(points), (len(out), len(points))
    return out


def f43_iou(a, b):
    """Intersection over union of two lattices' points at or above
    `F43_INSIDE`; 0 when neither holds one."""
    both = sum(1 for p, q in zip(a, b) if p >= F43_INSIDE and q >= F43_INSIDE)
    either = sum(1 for p, q in zip(a, b) if p >= F43_INSIDE or q >= F43_INSIDE)
    return both / either if either else 0.0


def gate_f43(app, folder, report) -> None:
    """Select Subject, Select Objects, All People and Object Selection
    on a float document, against the same picture at 16 bits."""
    name = ('F43 Select Subject, Select Objects, All People and Object Selection select in a '
            'float document')
    if not F43_PORTRAIT.is_file():
        record(report, name, False, dict(missing=str(F43_PORTRAIT)))
    width, height, linear = f43_picture()
    doc = app.open(write_float_tiff(folder / 'f43-portrait.tiff', width, height, linear,
                                    SYSTEM_SRGB.read_bytes()))
    codes = []
    for i in range(0, len(linear), 4):
        codes.extend([round(65535 * srgb_encode(v)) for v in linear[i:i + 3]] + [65535])
    png16(folder / 'f43-twin.png', width, height, codes)
    twin = app.open(folder / 'f43-twin.png')

    rows = {}
    detail = dict(bounds=dict(inside=F43_INSIDE, outside=F43_OUTSIDE, iou=F43_IOU,
                              min_class=F43_MIN_CLASS, stride=F43_STRIDE, coverage16=T_COV16))
    lattice = [(x, y) for y in range(0, height, F43_STRIDE) for x in range(0, width, F43_STRIDE)]

    def cover(document, points=lattice):
        return f43_coverage(app, document, points)

    def history(document):
        return app.call('list_history', document_id=document)

    def export(label):
        return export32(app, doc, folder / f'f43-{label}.tiff')['samples']

    def pixel(samples, index):
        x, y = lattice[index]
        return samples[(y * width + x) * 4:(y * width + x) * 4 + 4]

    def point_rule(document, inside, outside):
        got_in, got_out = cover(document, inside), cover(document, outside)
        return (all(v >= F43_INSIDE for v in got_in) and all(v <= F43_OUTSIDE for v in got_out),
                dict(inside=got_in, outside=got_out))

    # ---- fixture, tools ----
    hot = sum(1 for i, v in enumerate(linear) if i % 4 != 3 and v > 1.0)
    chest = app.call('sample_pixel', document_id=doc, x=F43_CHEST[0], y=F43_CHEST[1])
    precisions = [app.call('get_document', document_id=d).get('precision') for d in (doc, twin)]
    red = (chest.get('center_rgba32') or [0.0])[0]
    rows['fixture'] = ((width, height) == (512, 512) and hot >= F43_HOT_MIN and red > 1.0
                       and precisions == ['rgba32_float', 'rgba16'])
    detail['fixture'] = dict(size=[width, height], above_white=hot, chest_red=red,
                             precisions=precisions)
    tools = app.call('get_precision', document_id=doc).get('supported_tools') or []
    rows['tools'] = all(tool_name in tools for tool_name in F43_TOOLS)
    detail['tools'] = [tool_name for tool_name in F43_TOOLS if tool_name not in tools]

    # ---- subject, twin ----
    before = history(doc)['cursor']
    reply = app.call('select_subject', document_id=doc)
    subject = cover(doc)
    ok, points = point_rule(doc, [F43_FACE, F43_CHEST], F43_BACKGROUND)
    chosen = history(doc)
    classes = [sum(1 for v in subject if v == 0), sum(1 for v in subject if v == 65535)]
    rows['subject'] = (reply.get('instances', 0) >= 1 and ok and chosen['cursor'] == before + 1
                       and min(classes) >= F43_MIN_CLASS)
    detail['subject'] = dict(reply=reply, points=points, classes=classes,
                             cursor=[before, chosen['cursor']])
    twin_reply = app.call('select_subject', document_id=twin)
    twin_subject = cover(twin)
    detail['twin'] = dict(iou=f43_iou(subject, twin_subject),
                          instances=twin_reply.get('instances'))
    rows['twin'] = detail['twin']['iou'] >= F43_IOU

    # ---- the selection confines a filter, paint and a retouch ----
    base = export('base')
    clear = [i for i, v in enumerate(subject) if v == 0]
    full = [i for i, v in enumerate(subject) if v == 65535]

    def confined(label, call):
        reply = call()
        got = export(label)
        outside = all(bits_equal(pixel(got, i), pixel(base, i)) for i in clear)
        inside = sum(1 for i in full if not bits_equal(pixel(got, i), pixel(base, i)))
        app.call('undo', document_id=doc)
        undone = bits_equal(export(label + '-undo'), base)
        detail[label] = dict(reply=reply, outside_bit_identical=outside, inside_changed=inside,
                             undo_bit_exact=undone)
        return reply, outside and inside >= 1 and undone

    reply, ok = confined('confines_filter', lambda: app.call(
        'apply_filter', document_id=doc, layer=0, filter='blur', sigma=3))
    rows['confines_filter'] = ok and reply.get('write_scope') == 'selection'
    line = f37_points(F43_LINE)
    _, rows['confines_paint'] = confined('confines_paint', lambda: app.call(
        'brush_stroke', document_id=doc, layer=0, points=line, size=8, hardness=100, opacity=1,
        rgba32=F43_INK))
    _, rows['confines_retouch'] = confined('confines_retouch', lambda: app.call(
        'clone_stamp', document_id=doc, layer=0, points=line, size=12, hardness=100, opacity=1,
        source_x=F43_LINE[0][0], source_y=F43_LINE[0][1] + F43_CLONE_DOWN))

    # ---- the selection makes a layer mask, and the mask gives it back ----
    app.call('add_layer_mask', document_id=doc, layer=0, kind='from_selection')
    app.call('deselect', document_id=doc)
    emptied = app.call('get_selection', document_id=doc).get('selection_empty')
    app.call('load_selection', **{'document_id': doc, 'from': 'layer_mask', 'layer': 0})
    loaded = cover(doc)
    detail['mask'] = dict(worst=max(abs(a - b) for a, b in zip(loaded, subject)),
                          deselected=emptied)
    rows['mask'] = emptied is True and detail['mask']['worst'] <= T_COV16
    # Back to the state `subject` left, so the rows below see the unmasked picture.
    app.call('restore_history', document_id=doc, state_id=chosen['current_state_id'])
    assert bits_equal(export('mask-restored'), base), 'the mask was not undone'

    # ---- the window's exposure is view state ----
    app.call('set_view', document_id=doc, exposure=-6)
    shown = phase5_support.wait_for(
        lambda: app.call('get_view', document_id=doc),
        lambda state: state.get('exposure_displayed') is True, timeout=10,
        detail='the exposed image to reach the canvas')
    app.call('select_subject', document_id=doc)
    exposed = cover(doc)
    app.call('set_view', document_id=doc, exposure=0)
    detail['exposure'] = dict(iou=f43_iou(subject, exposed), exposure=shown.get('exposure'))
    rows['exposure'] = shown.get('exposure') == -6 and detail['exposure']['iou'] >= F43_IOU

    # ---- people ----
    app.call('select_people', document_id=doc)
    people = cover(doc)
    ok, points = point_rule(doc, [F43_FACE], F43_PEOPLE_CORNERS)
    app.call('select_people', document_id=twin)
    detail['people'] = dict(points=points, iou=f43_iou(people, cover(twin)))
    rows['people'] = ok and detail['people']['iou'] >= F43_IOU

    # ---- objects: the picker's instances and the Object Selection region ----
    listed = app.call('inspect_subjects', document_id=doc)
    holding = [row for row in listed.get('instances') or []
               if row['bounds']['x'] <= F43_FACE[0] < row['bounds']['x'] + row['bounds']['width']
               and row['bounds']['y'] <= F43_FACE[1] < row['bounds']['y'] + row['bounds']['height']]
    region = dict(x=0, y=0, width=width, height=height)
    detail['objects'] = dict(listed=listed.get('instances'), holding=holding)
    rows['objects'] = False
    if holding:
        instance, region = holding[0]['instance'], holding[0]['bounds']
        app.call('select_subjects', document_id=doc, instances=[instance])
        face = cover(doc, [F43_FACE])[0]
        found = app.call('object_select', document_id=doc, region=region)
        objects = cover(doc)
        beyond = [v for (x, y), v in zip(lattice, objects)
                  if not (region['x'] <= x < region['x'] + region['width']
                          and region['y'] <= y < region['y'] + region['height'])]
        # The twin, in this order: the default the other depths keep, then
        # the unrefined form the float result is compared with.
        twin_default = app.call('object_select', document_id=twin, region=region)
        twin_plain = app.call('object_select', document_id=twin, region=region, refine=False)
        iou = f43_iou(objects, cover(twin))
        rows['objects'] = (face >= F43_INSIDE and found.get('refined') is False
                           and instance in (found.get('instances') or [])
                           and all(v == 0 for v in beyond)
                           and twin_default.get('refined') is True
                           and twin_plain.get('refined') is False and iou >= F43_IOU)
        detail['objects'].update(instance=instance, region=region, face=face, found=found,
                                 lattice_points_outside=len(beyond),
                                 outside_nonzero=sum(1 for v in beyond if v != 0),
                                 twin_default=twin_default, twin_plain=twin_plain, iou=iou)
    at = history(doc)['cursor']
    refined = app.call('object_select', document_id=doc, region=region, refine=True)
    rows['refine_admitted'] = (refined.get('refined') is True
                               and history(doc)['cursor'] == at + 1)
    detail['refine_admitted'] = dict(reply=refined, cursor=[at, history(doc)['cursor']])

    # ---- the document's own tone map is what Vision sees ----
    app.call('set_tone_map', document_id=doc, operator='reinhard', white=4.0)
    app.call('select_subject', document_id=doc)
    rows['reinhard'], detail['reinhard'] = point_rule(doc, [F43_FACE], F43_BACKGROUND)

    detail['rows'] = rows
    record(report, name, all(rows.values()), detail)


# ------------------------------------------------------------------- F40 --

F40_SIZE = (240, 160)
# The tolerances, fixed here before the run (relative, floored at 1.0).
F40_HEAL = 0.02       # heal_stroke and patch_region against the scene
F40_SYNTH = 0.05      # spot heal, Content-Aware Fill, Remove and the moved blob
F40_VACATED = 0.10    # the place a Content-Aware Move leaves
F40_SHIFT = 120       # the clone's source lies this far to the right
F40_CLONE_LINE = ((30.5, 80.5), (90.5, 80.5))
F40_DARK = [0.01, 0.01, 0.01, 1.0]
F40_BLOB_LINE = ((100.5, 100.5), (104.5, 100.5))
F40_HEAL_LINE = ((96.5, 100.5), (108.5, 100.5))
F40_REMOVE_LINE = ((98.5, 100.5), (106.5, 100.5))
F40_FILL_RECT = (88, 90, 30, 22)
F40_PATCH_RECT = (92, 92, 22, 18)
F40_BRIGHT = [20.0, 10.0, 5.0, 1.0]
F40_MOVE_BLOB = ((100.5, 110.5), (104.5, 110.5))
F40_MOVE_RECT = (90, 100, 25, 21)
F40_MOVE = (0, -50)
F40_PATH = ((40.0, 120.0), (200.0, 120.0))
F40_INK = [4.0, 0.5, 0.25, 1.0]
F40_SCALAR = 'clone_stamp requires layer RGB; this operation does not edit scalar targets.'
F40_FOREIGN = {
    'eraser_ink': 'eraser_stroke takes no rgba32. It has no colour.',
    'brush_ramp_ink': 'brush_stroke takes no start_rgba32. Its linear ink is rgba32.',
    'gradient_ink': 'gradient takes no rgba32. Its linear ink is start_rgba32 and end_rgba32.',
}
F40_QUICK_MASK = ("start_rgba32 is a 32-bit float document's colour. "
                  'Pass start_color and end_color.')


def f40_red(x):
    """The scene's red in column `x`, as the f32 the file stores: 0 to 8."""
    return struct.unpack('<f', struct.pack('<f', 8.0 * x / (F40_SIZE[0] - 1)))[0]


def f40_scene():
    """F40's own target: opaque, red a ramp from 0 to 8 across the width (so
    most of it is above white), green 0.5, blue 0.25; the same in every row,
    so a row 40 or 50 above a pixel holds that pixel's own values."""
    width, height = F40_SIZE
    row = []
    for x in range(width):
        row.extend((f40_red(x), 0.5, 0.25, 1.0))
    return row * height


def f40_box(rect, margin=2):
    """A rectangle's regions: 'in' its interior inset by `margin`, None
    outside it grown by one pixel, 'skip' the band between."""
    x0, y0, w, h = rect

    def where(x, y):
        if x0 + margin <= x < x0 + w - margin and y0 + margin <= y < y0 + h - margin:
            return 'in'
        if x0 - 1 <= x < x0 + w + 1 and y0 - 1 <= y < y0 + h + 1:
            return 'skip'
        return None
    return where


def f40_check(got, base, region, want, tolerance=T_REL):
    """`f37_check` over F40's canvas: where `region(x, y)` is 'in', each
    component `want(x, y, pixel)` names within `tolerance` (relative, floored
    at 1.0; `None` for a component the row does not read); everywhere but a
    'skip' pixel, every other sample bit for bit."""
    width, height = F40_SIZE
    worst, exact, read = 0.0, True, 0
    for y in range(height):
        for x in range(width):
            i = (y * width + x) * 4
            where = region(x, y)
            if where == 'skip':
                continue
            wanted = want(x, y, base[i:i + 4]) if where == 'in' else {}
            for k in range(4):
                if k in wanted:
                    if wanted[k] is None:
                        continue
                    read += 1
                    worst = max(worst, abs(got[i + k] - wanted[k]) / max(1.0, abs(wanted[k])))
                elif not bits_equal(got[i + k:i + k + 1], base[i + k:i + k + 1]):
                    exact = False
    return (exact and read > 0 and worst <= tolerance,
            dict(worst=worst, others_bit_exact=exact, samples_read=read))


def f40_core(line, size):
    """The pixels of a hard stroke's core: centres within radius − 2."""
    width, height = F40_SIZE
    where = f37_stroke(line, size)
    return [(x, y) for y in range(height) for x in range(width) if where(x, y) == 'in']


def f40_reads(got, pixels, want, tolerance):
    """Whether every pixel of `pixels` reads `want(x)` (RGBA) within
    `tolerance`, relative and floored at 1.0; and the worst miss."""
    width = F40_SIZE[0]
    worst = 0.0
    for x, y in pixels:
        i = (y * width + x) * 4
        for k, wanted in enumerate(want(x)):
            worst = max(worst, abs(got[i + k] - wanted) / max(1.0, abs(wanted)))
    return bool(pixels) and worst <= tolerance, worst


def f40_wait(app, reply):
    """A background call's job, polled until it leaves `running`."""
    status, deadline = dict(state='running'), time.monotonic() + 120
    while reply.get('job_id') and status.get('state') == 'running':
        status = app.call('job_status', job_id=reply['job_id'])
        assert time.monotonic() < deadline, status
        time.sleep(0.02)
    return status


def gate_f40(app, folder, report):
    """Retouching and Stroke Path on a float layer."""
    width, height = F40_SIZE
    base = f40_scene()
    icc = read_float_tiff(FIXTURE)['icc']
    rows, detail = {}, dict(tolerances=dict(clone=T_REL, heal=F40_HEAL, synthesis=F40_SYNTH,
                                            vacated=F40_VACATED))
    scene_at = lambda x, y, p: {0: f40_red(x), 1: 0.5, 2: 0.25, 3: 1.0}

    def fresh(label):
        """A new document on its own copy of the target, one per row."""
        path = folder / f'f40-{label}.tiff'
        write_float_tiff(path, width, height, base, icc)
        return app.open(path)

    def after(doc, label):
        return export32(app, doc, folder / f'f40-{label}-export.tiff')['samples']

    def stroke(doc, tool_name, line, size, **arguments):
        return app.call(tool_name, document_id=doc, points=f37_points(line), size=size,
                        hardness=100, spacing=10, **arguments)

    def blob(doc, line=F40_BLOB_LINE, ink=F40_DARK):
        """A 10 px blob of `ink` painted with the float Brush; the export."""
        stroke(doc, 'brush_stroke', line, 10, layer=0, opacity=1, rgba32=ink)
        return after(doc, f'blob-{doc}')

    doc = fresh('a')
    rows['opened'] = bits_equal(after(doc, 'opened'), base)

    # (a) the Clone Stamp copies linear light: the source column's values.
    wide = f37_stroke(F40_CLONE_LINE, 12)
    source = dict(source_x=F40_CLONE_LINE[0][0] + F40_SHIFT, source_y=F40_CLONE_LINE[0][1])
    cloned = lambda x, y, p: {0: f40_red(x + F40_SHIFT), 1: 0.5, 2: 0.25, 3: 1.0}
    reply = stroke(doc, 'clone_stamp', F40_CLONE_LINE, 12, layer=0, opacity=1, **source)
    first = after(doc, 'a')
    rows['a_core'], detail['a'] = f40_check(first, base, wide, cloned)
    rows['a_above_white'] = min(f40_red(x + F40_SHIFT)
                                for x, _ in f40_core(F40_CLONE_LINE, 12)) > 1.0
    rows['a_reply'] = reply.get('storage') == 'rgba32_float' and reply.get('changed') is True
    detail['a_reply'] = reply
    app.call('undo', document_id=doc)
    rows['a_undo'] = bits_equal(after(doc, 'a-undo'), base)

    # (b) opacity, a float blend mode, a refused one and a new layer.
    doc = fresh('b')
    stroke(doc, 'clone_stamp', F40_CLONE_LINE, 12, layer=0, opacity=0.5, **source)
    rows['b_half'], detail['b_half'] = f40_check(
        after(doc, 'b-half'), base, wide,
        lambda x, y, p: {0: 0.5 * p[0] + 0.5 * f40_red(x + F40_SHIFT), 1: 0.5, 2: 0.25, 3: 1.0})
    app.call('undo', document_id=doc)
    stroke(doc, 'clone_stamp', F40_CLONE_LINE, 12, layer=0, opacity=1, blend_mode='Multiply',
           **source)
    got = after(doc, 'b-multiply')
    rows['b_multiply'], detail['b_multiply'] = f40_check(
        got, base, wide,
        lambda x, y, p: {0: p[0] * f40_red(x + F40_SHIFT), 1: 0.25, 2: 0.0625, 3: 1.0})
    detail['b_product'] = got[(80 * width + 90) * 4]
    rows['b_unclamped'] = detail['b_product'] > 8.0
    app.call('undo', document_id=doc)
    screen = stroke(doc, 'clone_stamp', F40_CLONE_LINE, 12, fail=True, layer=0, opacity=1,
                    blend_mode='Screen', **source)
    rows['b_screen'] = F37_SCREEN in screen
    rows['b_screen_unchanged'] = bits_equal(after(doc, 'b-screen'), base)
    stroke(doc, 'clone_stamp', F40_CLONE_LINE, 12, layer=0, opacity=1, output='new_layer',
           **source)
    layers = app.call('get_document', document_id=doc)['layers']
    rows['b_new_layer'], detail['b_new_layer'] = f40_check(
        after(doc, 'b-new-layer'), base, wide, cloned)
    rows['b_new_layer_added'] = len(layers) == 2
    detail['b_screen'] = screen

    # (c) the Healing Brush from 40 rows up, over a dark blob.
    doc = fresh('c')
    blobbed = blob(doc)
    rows['c_blob'] = not bits_equal(blobbed, base)
    heal = f37_stroke(F40_HEAL_LINE, 24)
    reply = stroke(doc, 'heal_stroke', F40_HEAL_LINE, 24, layer=0,
                   source_x=F40_HEAL_LINE[0][0], source_y=F40_HEAL_LINE[0][1] - 40)
    got = after(doc, 'c')
    rows['c_heal'], detail['c'] = f40_check(got, blobbed, heal, scene_at, F40_HEAL)
    core = f40_core(F40_HEAL_LINE, 24)
    rows['c_above_white'] = all(got[(y * width + x) * 4] > 1.0
                                for x, y in core if f40_red(x) > 1.0)
    rows['c_reply'] = reply.get('storage') == 'rgba32_float'
    detail['c_reply'] = reply

    # (d) Spot Healing over the blob, and the same seed twice.
    doc = fresh('d')
    blobbed = blob(doc)
    spot = dict(layer=0, seed=7)
    stroke(doc, 'spot_heal_stroke', F40_HEAL_LINE, 24, **spot)
    got = after(doc, 'd')
    rows['d_spot'], detail['d'] = f40_check(got, blobbed, heal, scene_at, F40_SYNTH)
    app.call('undo', document_id=doc)
    rows['d_undo'] = bits_equal(after(doc, 'd-undo'), blobbed)
    stroke(doc, 'spot_heal_stroke', F40_HEAL_LINE, 24, **spot)
    rows['d_repeat'] = bits_equal(after(doc, 'd-repeat'), got)

    # (e) Content-Aware Fill of a selection, direct and through a preview.
    doc = fresh('e')
    blobbed = blob(doc)
    x, y, w, h = F40_FILL_RECT
    app.call('select_rect', document_id=doc, x=x, y=y, width=w, height=h)
    app.call('content_aware_fill', document_id=doc, layer=0, seed=7)
    got = after(doc, 'e')
    rows['e_fill'], detail['e'] = f40_check(got, blobbed, f32_inside(F40_FILL_RECT), scene_at,
                                            F40_SYNTH)
    app.call('undo', document_id=doc)
    preview = app.call('preview_content_aware_fill', document_id=doc, layer=0, seed=7)
    deadline = time.monotonic() + 120
    while preview.get('state') == 'running':
        assert time.monotonic() < deadline, preview
        time.sleep(0.05)
        preview = app.call('preview_content_aware_fill', document_id=doc,
                           preview_id=preview['preview_id'])
    detail['e_preview'] = {k: v for k, v in preview.items() if k in ('state', 'message')}
    rows['e_preview_unpublished'] = bits_equal(after(doc, 'e-unpublished'), blobbed)
    app.call('content_aware_fill', document_id=doc, preview_id=preview['preview_id'])
    rows['e_preview'] = bits_equal(after(doc, 'e-preview'), got)
    app.call('deselect', document_id=doc)

    # (f) Remove, as a background job the caller polls.
    doc = fresh('f')
    blobbed = blob(doc)
    job = app.call('remove_stroke', document_id=doc, points=f37_points(F40_REMOVE_LINE),
                   size=20, layer=0, seed=7, background=True, mode='content_aware')
    status = f40_wait(app, job)
    rows['f_job'] = bool(job.get('job_id')) and status.get('state') == 'completed'
    rows['f_remove'], detail['f'] = f40_check(
        after(doc, 'f'), blobbed, f37_stroke(F40_REMOVE_LINE, 20), scene_at, F40_SYNTH)
    detail['f_job'] = dict(first=job, last=status)

    # (g) Patch: the outlined square healed from 40 rows up.
    doc = fresh('g')
    blobbed = blob(doc)
    x, y, w, h = F40_PATCH_RECT
    app.call('patch_region', document_id=doc, layer=0, offset_x=0, offset_y=-40,
             points=[[x, y], [x + w, y], [x + w, y + h], [x, y + h]])
    rows['g_patch'], detail['g'] = f40_check(
        after(doc, 'g'), blobbed, f40_box(F40_PATCH_RECT), scene_at, F40_HEAL)

    # (h) Content-Aware Move of a bright blob, then Extend.
    doc = fresh('h')
    bright = blob(doc, line=F40_MOVE_BLOB, ink=F40_BRIGHT)
    x, y, w, h = F40_MOVE_RECT
    region = dict(points=[[x, y], [x + w, y], [x + w, y + h], [x, y + h]],
                  offset_x=F40_MOVE[0], offset_y=F40_MOVE[1], layer=0, seed=7)
    old = f40_core(F40_MOVE_BLOB, 10)
    new = [(px + F40_MOVE[0], py + F40_MOVE[1]) for px, py in old]
    blob_ink = lambda px: F40_BRIGHT
    scene_ink = lambda px: [f40_red(px), 0.5, 0.25, 1.0]
    rows['h_blob'], _ = f40_reads(bright, old, blob_ink, T_REL)
    app.call('content_aware_move', document_id=doc, **region)
    got = after(doc, 'h')
    rows['h_move_new'], new_worst = f40_reads(got, new, blob_ink, F40_SYNTH)
    rows['h_move_old'], old_worst = f40_reads(got, old, scene_ink, F40_VACATED)
    app.call('undo', document_id=doc)
    rows['h_undo'] = bits_equal(after(doc, 'h-undo'), bright)
    app.call('content_aware_move', document_id=doc, mode='extend', **region)
    got = after(doc, 'h-extend')
    kept_new, extend_new = f40_reads(got, new, blob_ink, F40_SYNTH)
    kept_old, extend_old = f40_reads(got, old, blob_ink, F40_SYNTH)
    rows['h_extend'] = kept_new and kept_old
    detail['h'] = dict(new=new_worst, old=old_worst, extend_new=extend_new,
                       extend_old=extend_old, pixels=len(old))

    # (i) Stroke Path with above-white ink, one undo, then the Eraser.
    doc = fresh('i')
    app.call('set_path', document_id=doc,
             subpaths=[{'anchors': [list(point) for point in F40_PATH]}])
    line = f37_stroke(F40_PATH, 8)
    ink = {k: F40_INK[k] for k in range(4)}
    reply = app.call('stroke_path', document_id=doc, path='Work Path', rgba32=F40_INK, size=8,
                     hardness=100, layer=0)
    rows['i_stroke'], detail['i'] = f40_check(after(doc, 'i'), base, line, lambda x, y, p: ink)
    rows['i_reply'] = reply.get('changed') is True and reply.get('tool') == 'brush'
    detail['i_reply'] = reply
    app.call('undo', document_id=doc)
    rows['i_undo'] = bits_equal(after(doc, 'i-undo'), base)
    # The Eraser at half strength halves alpha and leaves RGB bit for bit;
    # at full strength alpha is 0, where a composite export carries no colour.
    app.call('stroke_path', document_id=doc, path='Work Path', tool='eraser', size=8,
             hardness=100, opacity=0.5, layer=0)
    rows['i_erase_half'], detail['i_erase_half'] = f40_check(
        after(doc, 'i-erase-half'), base, line, lambda x, y, p: {3: 0.5})
    app.call('undo', document_id=doc)
    app.call('stroke_path', document_id=doc, path='Work Path', tool='eraser', size=8,
             hardness=100, layer=0)
    rows['i_erase'], detail['i_erase'] = f40_check(
        after(doc, 'i-erase'), base, line,
        lambda x, y, p: {0: None, 1: None, 2: None, 3: 0.0})

    # (j) the refusals, each by name, and nothing changed.
    doc = fresh('j')
    pair = dict(start_rgba32=[0, 0, 0, 1], end_rgba32=[8, 8, 8, 1])
    across = dict(x0=0.5, y0=80, x1=width - 0.5, y1=80)
    # Red Eye runs at float (F45 has it).
    refusals = dict(
        # `dodge_burn`, `fill` and `sponge_stroke` are admitted (F46).
        scalar=stroke(doc, 'clone_stamp', F40_CLONE_LINE, 12, fail=True, layer=0, target='mask',
                      **source),
        eraser_ink=stroke(doc, 'eraser_stroke', F40_HEAL_LINE, 8, fail=True, layer=0,
                          rgba32=F40_INK),
        brush_ramp_ink=stroke(doc, 'brush_stroke', F40_HEAL_LINE, 8, fail=True, layer=0,
                              start_rgba32=F40_INK),
        gradient_ink=app.call('gradient', fail=True, document_id=doc, layer=0, rgba32=F40_INK,
                              **across),
        quick_mask=app.call('gradient', fail=True, document_id=doc, layer=0,
                            target='quick_mask', **pair, **across))
    rows['j_scalar'] = F40_SCALAR in refusals['scalar']
    for name, sentence in F40_FOREIGN.items():
        rows['j_' + name] = sentence in refusals[name]
    rows['j_quick_mask'] = F40_QUICK_MASK in refusals['quick_mask']
    rows['j_unchanged'] = bits_equal(after(doc, 'j'), base)
    app.call('add_adjustment_layer', document_id=doc, op='exposure', params={'exposure': 1.0})
    layers = app.call('get_document', document_id=doc)['layers']
    adjustment = next(row for row in layers if row.get('is_adjustment'))
    adjusted = after(doc, 'j-adjustment')
    refusals['adjustment'] = stroke(
        doc, 'heal_stroke', F40_HEAL_LINE, 24, fail=True, layer=adjustment['index'],
        source_x=F40_HEAL_LINE[0][0], source_y=F40_HEAL_LINE[0][1] - 40)
    rows['j_adjustment'] = ('adjustment' in refusals['adjustment']
                            and bits_equal(after(doc, 'j-adjustment-after'), adjusted))
    detail['j_refusals'] = refusals

    # (k) a retouched document persists bit for bit.
    doc = fresh('k')
    stroke(doc, 'clone_stamp', F40_CLONE_LINE, 12, layer=0, opacity=1, **source)
    blob(doc)
    stroke(doc, 'spot_heal_stroke', F40_HEAL_LINE, 24, layer=0, seed=7)
    current = after(doc, 'k')
    native = folder / 'f40.rz'
    app.call('save_copy', document_id=doc, path=str(native), format='rz')
    reopened = app.open(native)
    rows['k_reopened'] = (not bits_equal(current, base)
                          and bits_equal(after(reopened, 'k-reopened'), current))

    detail['rows'] = rows
    record(report, 'F40 the Clone Stamp, the healing tools, Content-Aware Fill and Move and '
                   'Stroke Path retouch a float layer', all(rows.values()), detail)


# ------------------------------------------------------------------- F42 --

# The bounds, fixed here before the run.
F42_BLUR = 0.02            # of the reference's magnitude
F42_SHARPEN = 0.02         # of max(|reference|, F42_SHARPEN_FLOOR)
F42_SHARPEN_FLOOR = 0.1
F42_EDGE = 120             # the edge target's first bright column
F42_BLOB = [5000.0, 5000.0, 5000.0, 1.0]
F42_BLUR_LINE = ((90.5, 100.5), (114.5, 100.5))
F42_SHARPEN_LINE = ((104.5, 80.5), (136.5, 80.5))
F42_SMUDGE_LINE = ((150.5, 80.5), (110.5, 80.5))
F42_PAINT_LINE = ((60.5, 40.5), (100.5, 40.5))
F42_HISTORY_LINE = ((70.5, 40.5), (120.5, 40.5))
F42_SCALAR = 'blur_stroke requires layer RGB; this operation does not edit scalar targets.'
F42_QUICK_MASK = 'Exit Quick Mask before editing layer pixels, masks or channels.'
F42_BLEND = 'sharpen_stroke has no blend_mode'


def f42_edge_scene():
    """F42's second target: opaque, red 0.1 left of column `F42_EDGE` and 8.0
    from it, green 0.5, blue 0.25; the same in every row."""
    width, height = F40_SIZE
    row = []
    for x in range(width):
        row.extend((f38_f32(0.1) if x < F42_EDGE else 8.0, 0.5, 0.25, 1.0))
    return row * height


def f42_gaussian(samples, sigma, x, y):
    """The alpha-weighted Gaussian of pixel (x, y)'s straight colour in an
    F40-sized export, in binary64: the full two-dimensional sum out to
    ceil(8 sigma), reads clamped to the canvas."""
    width, height = F40_SIZE
    half = math.ceil(8 * sigma)
    weights = [math.exp(-0.5 * (d / sigma) ** 2) for d in range(-half, half + 1)]
    acc, total = [0.0, 0.0, 0.0], 0.0
    for dy in range(-half, half + 1):
        row = min(max(y + dy, 0), height - 1) * width
        for dx in range(-half, half + 1):
            i = (row + min(max(x + dx, 0), width - 1)) * 4
            weight = weights[dy + half] * weights[dx + half] * samples[i + 3]
            acc[0] += weight * samples[i]
            acc[1] += weight * samples[i + 1]
            acc[2] += weight * samples[i + 2]
            total += weight
    return [value / total for value in acc]


def f42_check(got, base, region, want, floor):
    """`f40_check` with a bound relative to the reference itself: where
    `region(x, y)` is 'in', each of the three colour samples is within 2 % of
    `max(|want|, floor)`; everywhere but a 'skip' pixel every other sample
    (alpha in the core, all four far from it) is `base`'s bit for bit."""
    width, height = F40_SIZE
    worst, exact, read = 0.0, True, 0
    for y in range(height):
        for x in range(width):
            i = (y * width + x) * 4
            where = region(x, y)
            if where == 'skip':
                continue
            wanted = want(x, y) if where == 'in' else []
            for k in range(4):
                if k < len(wanted):
                    read += 1
                    scale = max(abs(wanted[k]), floor)
                    worst = max(worst, abs(got[i + k] - wanted[k]) / scale if scale else math.inf)
                elif not bits_equal(got[i + k:i + k + 1], base[i + k:i + k + 1]):
                    exact = False
    return worst, exact, read


def gate_f42(app, folder, report):
    """The Blur, Sharpen and Smudge brushes and the History Brush on a
    float layer."""
    width, height = F40_SIZE
    ramp, edge = f40_scene(), f42_edge_scene()
    icc = read_float_tiff(FIXTURE)['icc']
    rows, detail = {}, dict(bounds=dict(blur=F42_BLUR, sharpen=F42_SHARPEN,
                                        sharpen_floor=F42_SHARPEN_FLOOR, relative=T_REL))

    def fresh(label, samples):
        """A new document on its own copy of a target, one per row."""
        path = folder / f'f42-{label}.tiff'
        write_float_tiff(path, width, height, samples, icc)
        return app.open(path)

    def after(doc, label):
        return export32(app, doc, folder / f'f42-{label}-export.tiff')['samples']

    def stroke(doc, tool_name, line, size, **arguments):
        return app.call(tool_name, document_id=doc, points=f37_points(line), size=size,
                        hardness=100, spacing=10, **arguments)

    def ran(reply):
        return reply.get('storage') == 'rgba32_float' and reply.get('state') == 'completed'

    def at(samples, x, y):
        i = (y * width + x) * 4
        return samples[i:i + 4]

    # (a) Blur spreads a 5000 highlight, within 2 % of the f64 Gaussian.
    doc = fresh('a', ramp)
    rows['opened'] = bits_equal(after(doc, 'opened'), ramp)
    stroke(doc, 'brush_stroke', F40_BLOB_LINE, 10, layer=0, opacity=1, rgba32=F42_BLOB)
    blobbed = after(doc, 'a-blob')
    rows['a_blob'] = at(blobbed, 102, 100)[:3] == F42_BLOB[:3]
    region = f37_stroke(F42_BLUR_LINE, 40)
    reply = stroke(doc, 'blur_stroke', F42_BLUR_LINE, 40, layer=0, strength=100)
    got = after(doc, 'a')
    worst, exact, read = f42_check(got, blobbed, region,
                                   lambda x, y: f42_gaussian(blobbed, 2.0, x, y), 0.0)
    rows['a_core'] = read > 0 and worst <= F42_BLUR
    rows['a_alpha_and_far_bit_exact'] = exact
    core = f40_core(F42_BLUR_LINE, 40)
    spread = [(x, y) for x, y in core
              if at(blobbed, x, y)[1] == 0.5 and at(got, x, y)[1] > 1.0]
    rows['a_spread_above_white'] = len(spread) > 0
    rows['a_reply'] = ran(reply)
    app.call('undo', document_id=doc)
    rows['a_undo'] = bits_equal(after(doc, 'a-undo'), blobbed)
    detail['a'] = dict(worst=worst, samples_read=read, lifted_pixels=len(spread), reply=reply)

    # (b) Sharpen across the 0.1 / 8 edge: the dark side goes below zero.
    doc = fresh('b', edge)
    rows['b_opened'] = bits_equal(after(doc, 'b-opened'), edge)
    region = f37_stroke(F42_SHARPEN_LINE, 24)

    def sharpened(x, y):
        low = f42_gaussian(edge, 1.0, x, y)
        return [2.0 * value - low[k] for k, value in enumerate(at(edge, x, y)[:3])]

    reply = stroke(doc, 'sharpen_stroke', F42_SHARPEN_LINE, 24, layer=0, strength=100)
    got = after(doc, 'b')
    worst, exact, read = f42_check(got, edge, region, sharpened, F42_SHARPEN_FLOOR)
    rows['b_core'] = read > 0 and worst <= F42_SHARPEN
    rows['b_alpha_and_far_bit_exact'] = exact
    dark = at(got, F42_EDGE - 1, 80)[0]
    rows['b_below_zero'] = dark < 0.0 and sharpened(F42_EDGE - 1, 80)[0] < 0.0
    rows['b_reply'] = ran(reply)
    detail['b'] = dict(worst=worst, samples_read=read, dark_side=dark, reply=reply)

    # (c) Smudge carries the 8.0 side 40 px into the dark one.
    doc = fresh('c', edge)
    start, end = (150, 80), (110, 80)
    reply = stroke(doc, 'smudge_stroke', F42_SMUDGE_LINE, 20, layer=0, strength=100)
    got = after(doc, 'c')
    carried, source = at(got, *end), at(edge, *start)
    rows['c_start_is_bright'] = source[0] == 8.0 and at(edge, *end)[0] < 1.0
    rows['c_end_reads_start'], miss = close(carried, source, T_REL)
    rows['c_above_white'] = carried[0] > 1.0
    rows['c_reply'] = ran(reply)
    detail['c'] = dict(end=carried, start=source, worst=miss, reply=reply)

    # (d) the History Brush restores the pre-edit state's exact samples.
    doc = fresh('d', ramp)
    state = app.call('list_history', document_id=doc)['current_state_id']
    stroke(doc, 'brush_stroke', F42_PAINT_LINE, 12, layer=0, opacity=1, rgba32=F40_BRIGHT)
    painted = after(doc, 'd-painted')
    rows['d_painted'] = not bits_equal(painted, ramp)
    region = f37_stroke(F42_HISTORY_LINE, 16)
    reply = stroke(doc, 'history_brush_stroke', F42_HISTORY_LINE, 16, layer=0, opacity=100,
                   state_id=state)
    got = after(doc, 'd')
    restored, far, changed = True, True, 0
    for y in range(height):
        for x in range(width):
            where = region(x, y)
            if where == 'in':
                restored &= bits_equal(at(got, x, y), at(ramp, x, y))
                changed += not bits_equal(at(painted, x, y), at(ramp, x, y))
            elif where is None:
                far &= bits_equal(at(got, x, y), at(painted, x, y))
    rows['d_core_is_the_state'] = restored and changed > 0
    rows['d_far_bit_exact'] = far
    rows['d_reply'] = ran(reply)
    app.call('undo', document_id=doc)
    rows['d_undo'] = bits_equal(after(doc, 'd-undo'), painted)
    stroke(doc, 'history_brush_stroke', F42_HISTORY_LINE, 16, layer=0, opacity=50,
           state_id=state)
    rows['d_half'], detail['d_half'] = f40_check(
        after(doc, 'd-half'), painted, region,
        lambda x, y, p: {k: 0.5 * p[k] + 0.5 * at(ramp, x, y)[k] for k in range(4)})
    detail['d'] = dict(state_id=state, restored_pixels_that_differed=changed, reply=reply)

    # (e) the refusals, each by name, and nothing changed.
    doc = fresh('e', ramp)
    # `sponge_stroke` is admitted (F46).
    refusals = dict(
        scalar=stroke(doc, 'blur_stroke', F42_BLUR_LINE, 24, fail=True, layer=0, target='mask'),
        blend=stroke(doc, 'sharpen_stroke', F42_BLUR_LINE, 24, fail=True, layer=0,
                     blend_mode='Multiply'))
    app.call('set_quick_mask', document_id=doc, active=True)
    refusals['quick_mask'] = stroke(doc, 'smudge_stroke', F42_SMUDGE_LINE, 20, fail=True,
                                    layer=0)
    app.call('set_quick_mask', document_id=doc, active=False)
    rows['e_scalar'] = F42_SCALAR in refusals['scalar']
    rows['e_blend'] = F42_BLEND in refusals['blend']
    rows['e_quick_mask'] = F42_QUICK_MASK in refusals['quick_mask']
    rows['e_unchanged'] = bits_equal(after(doc, 'e'), ramp)
    app.call('add_adjustment_layer', document_id=doc, op='exposure', params={'exposure': 1.0})
    layers = app.call('get_document', document_id=doc)['layers']
    adjustment = next(row for row in layers if row.get('is_adjustment'))
    adjusted = after(doc, 'e-adjustment')
    refusals['adjustment'] = stroke(doc, 'blur_stroke', F42_BLUR_LINE, 24, fail=True,
                                    layer=adjustment['index'])
    rows['e_adjustment'] = ('adjustment' in refusals['adjustment']
                            and bits_equal(after(doc, 'e-adjustment-after'), adjusted))
    detail['e_refusals'] = refusals

    # (f) a background job, and a brushed document persists bit for bit.
    doc = fresh('f', edge)
    job = stroke(doc, 'blur_stroke', F42_SHARPEN_LINE, 24, layer=0, strength=100,
                 background=True)
    status = f40_wait(app, job)
    rows['f_job'] = (bool(job.get('job_id')) and status.get('state') == 'completed'
                     and status.get('storage') == 'rgba32_float')
    stroke(doc, 'smudge_stroke', F42_SMUDGE_LINE, 20, layer=0, strength=60)
    current = after(doc, 'f')
    native = folder / 'f42.rz'
    app.call('save_copy', document_id=doc, path=str(native), format='rz')
    reopened = app.open(native)
    rows['f_reopened'] = (not bits_equal(current, edge)
                          and bits_equal(after(reopened, 'f-reopened'), current))
    detail['f_job'] = dict(first=job, last=status)

    detail['rows'] = rows
    record(report, 'F42 the Blur, Sharpen and Smudge brushes and the History Brush rework a '
                   'float layer in linear light', all(rows.values()), detail)


# ------------------------------------------------------------------- F44 --

# Fixed before anything is measured, never widened.
F44_TOL = 1e-5      # the sRGB EOTF, absolute
F44_SIDE = 256      # the 16-bit TIFF: 65536 pixels, every code
F44_BYTES = 16      # the 8-bit PNG: 256 pixels, every byte


def srgb_decode(encoded):
    """IEC 61966-2.1's EOTF, written out here."""
    if encoded <= 0.04045:
        return encoded / 12.92
    return ((encoded + 0.055) / 1.055) ** 2.4


def f44_codes():
    return [(i, 65535 - i, (7 * i) % 65536, 65535) for i in range(F44_SIDE * F44_SIDE)]


def f44_tiff16(path, pixels, side):
    """An uncompressed little-endian RGBA16 TIFF with no profile: one strip,
    unassociated alpha (ExtraSamples 2), unsigned samples."""
    body = struct.pack('<%dH' % (side * side * 4), *[v for p in pixels for v in p])
    count = 11
    ifd_size = 2 + count * 12 + 4
    bits_off = 8 + ifd_size
    fmt_off = bits_off + 8
    strip_off = fmt_off + 8
    entries = [
        (256, 3, 1, side), (257, 3, 1, side), (258, 3, 4, bits_off), (259, 3, 1, 1),
        (262, 3, 1, 2), (273, 4, 1, strip_off), (277, 3, 1, 4), (278, 3, 1, side),
        (279, 4, 1, len(body)), (338, 3, 1, 2), (339, 3, 4, fmt_off),
    ]
    ifd = struct.pack('<H', len(entries))
    for tag, kind, n, value in entries:
        if kind == 3 and n == 1:
            ifd += struct.pack('<HHIHH', tag, kind, n, value, 0)
        else:
            ifd += struct.pack('<HHII', tag, kind, n, value)
    ifd += struct.pack('<I', 0)
    tail = struct.pack('<4H', 16, 16, 16, 16) + struct.pack('<4H', 1, 1, 1, 1)
    Path(path).write_bytes(b'II' + struct.pack('<HI', 42, 8) + ifd + tail + body)


def f44_png8(path, pixels, side):
    rows = b''.join(b'\0' + bytes(v for p in pixels[y * side:(y + 1) * side] for v in p)
                    for y in range(side))
    Path(path).write_bytes(
        b'\x89PNG\r\n\x1a\n'
        + chunk(b'IHDR', struct.pack('>IIBBBBB', side, side, 8, 6, 0, 0, 0))
        + chunk(b'sRGB', b'\0')
        + chunk(b'IDAT', zlib.compress(rows, 6))
        + chunk(b'IEND', b''))


def f44_png(app, document, path, depth):
    reply = app.call('export_image', document_id=document, path=str(path),
                     settings={'format': 'png', 'bit_depth': depth}, overwrite=True)
    assert reply['ok'], reply
    return read_png(Path(path).read_bytes())[2]


def f44_precision(app, document):
    return app.call('get_document', document_id=document).get('precision')


def gate_f44(app, folder, report):
    """New Document at 32 bits and the conversion into float, over MCP."""
    rows, detail = {}, dict(tolerance=F44_TOL)

    def fresh(**arguments):
        return app.call('new_document', width=8, height=6, depth=32, **arguments)['document']['id']

    # (a)–(d) New Document at 32 bits.
    white = fresh(profile='srgb', background='white')
    rows['a_white'] = (f44_precision(app, white) == 'rgba32_float'
                       and all(v == 1.0 for v in export32(app, white, folder / 'f44-a.tiff')['samples']))
    grey = export32(app, fresh(profile='srgb', background='#808080'), folder / 'f44-b.tiff')['samples']
    want = srgb_decode(32896 / 65535)
    rows['b_grey'] = all(abs(grey[i] - want) <= F44_TOL if i % 4 < 3 else grey[i] == 1.0
                         for i in range(len(grey)))
    detail['b_grey'] = dict(got=grey[:4], want=want)
    clear = fresh(profile='srgb', background='transparent')
    names = [row['name'] for row in app.call('get_document', document_id=clear)['layers']]
    rows['c_transparent'] = (names == ['Layer 1'] and all(
        v == 0.0 for v in export32(app, clear, folder / 'f44-c.tiff')['samples']))
    detail['c_layers'] = names
    p3 = fresh(profile='display_p3', background='white')
    rows['d_p3_white'] = all(v == 1.0 for v in export32(app, p3, folder / 'f44-d.tiff')['samples'])

    # (e) a real 16-bit TIFF holding every code.
    codes = f44_codes()
    tiff = folder / 'f44-every-code.tiff'
    f44_tiff16(tiff, codes, F44_SIDE)
    sixteen = app.open(tiff)
    profile = app.call('get_metadata', document_id=sixteen).get('profile')
    exported = f44_png(app, sixteen, folder / 'f44-e.png', 16)
    rows['e_open16'] = (f44_precision(app, sixteen) == 'rgba16' and 'sRGB' in str(profile)
                        and exported == codes)
    detail['e_profile'] = profile

    # (f) the conversion, its plan, undo and redo.
    reply = app.call('convert_precision', document_id=sixteen, mode='rgba32', structure='preserve')
    plan = reply.get('conversion', {})
    converted = f44_precision(app, sixteen)
    app.call('undo', document_id=sixteen)
    undone = f44_precision(app, sixteen)
    app.call('redo', document_id=sixteen)
    redone = f44_precision(app, sixteen)
    rows['f_convert'] = (plan.get('direction') == 'rgba16_to_rgba32_float'
                         and plan.get('blocked') == [] and converted == 'rgba32_float'
                         and undone == 'rgba16' and redone == 'rgba32_float')
    detail['f_convert'] = dict(direction=plan.get('direction'), blocked=plan.get('blocked'),
                               differences=plan.get('rendering_differences'),
                               converted=converted, undone=undone, redone=redone)

    # (g) the decoded samples against the oracle.
    floats = export32(app, sixteen, folder / 'f44-g.tiff')['samples']
    worst = 0.0
    alpha = True
    for i, pixel in enumerate(codes):
        for c in range(3):
            worst = max(worst, abs(floats[i * 4 + c] - srgb_decode(pixel[c] / 65535)))
        alpha = alpha and floats[i * 4 + 3] == 1.0
    rows['g_decoded'] = worst <= F44_TOL and alpha
    detail['g_worst'] = worst

    # (h) the .rz round trip.
    native = folder / 'f44-h.rz'
    app.call('save_copy', document_id=sixteen, path=str(native), format='rz')
    reopened = app.open(native)
    rows['h_reopen'] = (f44_precision(app, reopened) == 'rgba32_float'
                        and bits_equal(export32(app, reopened, folder / 'f44-h.tiff')['samples'],
                                       floats))

    # (i) back to 16 bits: every code returns.
    app.call('convert_precision', document_id=sixteen, mode='rgba16', structure='preserve')
    rows['i_back16'] = (f44_precision(app, sixteen) == 'rgba16'
                        and f44_png(app, sixteen, folder / 'f44-i.png', 16) == codes)

    # (j) 8 → 32 → 8 returns every byte.
    bytes8 = [(i, 255 - i, (7 * i) % 256, 255) for i in range(F44_BYTES * F44_BYTES)]
    png8 = folder / 'f44-every-byte.png'
    f44_png8(png8, bytes8, F44_BYTES)
    eight = app.open(png8)
    app.call('convert_precision', document_id=eight, mode='rgba32', structure='preserve')
    float8 = f44_precision(app, eight)
    app.call('convert_precision', document_id=eight, mode='rgba8', structure='preserve',
             dither='none')
    rows['j_back8'] = (float8 == 'rgba32_float' and f44_precision(app, eight) == 'rgba8'
                       and f44_png(app, eight, folder / 'f44-j.png', 8) == bytes8)

    # (k) a Screen layer blocks by name.
    copy = folder / 'f44-screen.tiff'
    copy.write_bytes(tiff.read_bytes())
    screened = app.open(copy)
    app.call('duplicate_layer', document_id=screened, index=0)
    app.call('set_layer_properties', document_id=screened, index=1, blend_mode='Screen')
    name = app.call('get_document', document_id=screened)['layers'][1]['name']
    refusal = app.call('convert_precision', fail=True, document_id=screened, mode='rgba32',
                       structure='preserve')
    dry = app.call('convert_precision', document_id=screened, mode='rgba32',
                   structure='preserve', dry_run=True)
    blocked = dry.get('conversion', {}).get('blocked', [])
    rows['k_screen'] = (name in refusal and 'screen' in refusal.lower()
                        and [row.get('index') for row in blocked] == [1]
                        and dry.get('would_refuse') is not None
                        and f44_precision(app, screened) == 'rgba16')
    detail['k_screen'] = dict(refusal=refusal, blocked=blocked)

    # (l) the two refusals by name.
    flatten = app.call('convert_precision', fail=True, document_id=screened, mode='rgba32',
                       flatten=True)
    already = app.call('convert_precision', fail=True, document_id=reopened, mode='rgba32',
                       structure='preserve')
    rows['l_refusals'] = ('A conversion to 32-bit float keeps every layer' in flatten
                          and 'already uses the requested precision' in already)
    detail['l_refusals'] = dict(flatten=flatten, already=already)

    detail['rows'] = rows
    record(report, 'F44 New Document makes float documents and 8/16-bit documents convert '
                   'into float and back exactly', all(rows.values()), detail)


# ------------------------------------------------------------------- F45 --

# Fixed before anything is measured, never widened.
F45_DECODE = 1e-4      # absolute: one decoded display value
F45_KEPT = 1e-3        # relative: a value a filter's window never leaves
F45_FILTERS = [
    ('surface_blur', dict(radius=3, threshold=30)),
    ('dust_and_scratches', dict(radius=2, threshold=0.02)),
    ('reduce_noise', dict(radius=2, luminance=0.05, detail=0.4, color=0.6)),
    ('add_noise', dict(amount=0.2, seed=7)),
    ('add_grain', dict(amount=60, size=1.5, roughness=30, seed=7)),
    ('clouds', dict(seed=5)),
    ('difference_clouds', dict(seed=5)),
    ('emboss', dict()),
    ('edge_detect', dict()),
    ('pixelate', dict(block=4)),
    ('sharpen', dict(amount=1)),
    ('presence', dict(texture=50, clarity=50)),
    ('develop_filter', dict(exposure=1)),
]
# `smart_sharpen` and `depth_blur` run at 32 bits too, so get_precision lists them; F45
# does not run them (their own drives do).
F45_ALL = ['blur', 'box_blur', 'motion_blur', 'radial_blur', 'lens_blur', 'tilt_shift', 'offset',
           'lens_correction', 'liquify', 'unsharp_mask', 'high_pass'] + [n for n, _ in F45_FILTERS] \
    + ['smart_sharpen', 'depth_blur']
F45_EYE = (6, 16, 3)               # centre x, y and radius of the red disc
F45_EYE_RECT = (1, 11, 11, 11)     # x, y, width, height
F45_BLOCK = (8.0, 4.0, 2.0)        # the fixture's above-white block (`scene`)


def f45_pixelate(base, block):
    """Pixelate: each block-square cell anchored at (0, 0) is its premultiplied
    mean colour with the plain mean alpha; a cell whose alpha sums to zero is
    transparent black."""
    out = list(base)
    for cy in range(0, HEIGHT, block):
        for cx in range(0, WIDTH, block):
            cells = [(x, y) for y in range(cy, min(cy + block, HEIGHT))
                     for x in range(cx, min(cx + block, WIDTH))]
            alpha = sum(base[(y * WIDTH + x) * 4 + 3] for x, y in cells)
            mean = [0.0, 0.0, 0.0, 0.0]
            if alpha > 0.0:
                for c in range(3):
                    mean[c] = sum(base[(y * WIDTH + x) * 4 + c] * base[(y * WIDTH + x) * 4 + 3]
                                  for x, y in cells) / alpha
                mean[3] = alpha / len(cells)
            for x, y in cells:
                out[(y * WIDTH + x) * 4:(y * WIDTH + x) * 4 + 4] = mean
    return out


def gate_f45(app, folder, report):
    """The display-referred filters, Presence, Develop Filter and Red Eye on a
    float layer, over MCP."""
    copy = folder / 'f45-scene.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    doc = app.open(copy)
    base = scene()
    rows, detail = {}, dict(bounds=dict(relative=T_REL, decode=F45_DECODE, kept=F45_KEPT),
                            filters={})

    def samples(label):
        return export32(app, doc, folder / f'f45-{label}.tiff')['samples']

    def at(pixels, x, y):
        i = (y * WIDTH + x) * 4
        return pixels[i:i + 4]

    rows['base'] = bits_equal(samples('base'), base)
    precision = app.call('get_precision', document_id=doc)
    listed = precision.get('filters') or []
    rows['listed'] = sorted(listed) == sorted(F45_ALL)
    detail['listed'] = listed
    refused_rows = [row.get('operator', '') for row in
                    (precision.get('operators') or {}).get('refused') or []]
    rows['row_gone'] = bool(refused_rows) and not any(
        'display-encoded values' in row for row in refused_rows)
    detail['refused_rows'] = refused_rows

    applied = {}
    for name, arguments in F45_FILTERS:
        reply = app.call('apply_filter', document_id=doc, layer=0, filter=name, **arguments)
        applied[name] = samples(name)
        app.call('undo', document_id=doc)
        undone = bits_equal(samples(name + '-undo'), base)
        rows[name] = (reply.get('changed') is True and reply.get('storage') == 'rgba32_float'
                      and all(math.isfinite(v) for v in applied[name])
                      and not bits_equal(applied[name], base) and undone)
        detail['filters'][name] = dict(reply=reply, undo_bit_exact=undone)

    rows['pixelate_mean'], detail['pixelate_worst'] = f39_close(
        applied['pixelate'], f45_pixelate(base, 4))
    noise_zero, noise_ratio = True, 0.0
    for got, was in zip(applied['add_noise'], base):
        if was == 0.0:
            noise_zero = noise_zero and got == 0.0
        else:
            noise_ratio = max(noise_ratio, abs(got / was - 1.0))
    rows['add_noise_relative'] = noise_zero and noise_ratio <= 0.2 + 1e-6
    detail['add_noise'] = dict(zero_kept=noise_zero, worst_ratio=noise_ratio)
    doubled = [v * 2.0 if i % 4 != 3 else v for i, v in enumerate(base)]
    rows['exposure_doubles'] = bits_equal(applied['develop_filter'], doubled)
    # A flat region embosses to its encoded luma + 0.5 (the kernel's weights
    # sum to 1); in the block that passes each channel's encoded maximum, the
    # block's own code, so the finish holds the block's value.
    embossed = at(applied['emboss'], 5, 3)
    rows['emboss_flat_block'] = all(abs(embossed[c] - F45_BLOCK[c]) <= F45_DECODE * F45_BLOCK[c]
                                    for c in range(3))
    detail['emboss_5_3'] = embossed
    kept = {}
    for name in ('surface_blur', 'dust_and_scratches', 'reduce_noise', 'sharpen', 'presence'):
        red = at(applied[name], 2, 2)[0]
        kept[name] = red
        rows[name + '_above_white'] = red > 1.0
        if name in ('surface_blur', 'dust_and_scratches', 'sharpen'):
            rows[name + '_kept'] = abs(red - 8.0) <= F45_KEPT * 8.0
    detail['block_red'] = kept

    # A selection confines the write; the filter still reads the whole layer.
    x0, y0, w, h = F39_RECT
    app.call('select_rect', document_id=doc, x=x0, y=y0, width=w, height=h)
    reply = app.call('apply_filter', document_id=doc, layer=0, filter='surface_blur',
                     radius=3, threshold=30)
    selected = samples('selected')
    outside, inside = True, False
    for y in range(HEIGHT):
        for x in range(WIDTH):
            same = bits_equal(at(selected, x, y), at(base, x, y))
            if x0 <= x < x0 + w and y0 <= y < y0 + h:
                inside = inside or not same
            else:
                outside = outside and same
    rows['selection'] = outside and inside
    detail['selection'] = dict(reply=reply, outside_bit_exact=outside, inside_changed=inside)
    app.call('undo', document_id=doc)
    app.call('deselect', document_id=doc)
    rows['selection_undo'] = bits_equal(samples('selected-undo'), base)

    # Dehaze, Highlights and Vibrance run at float (F47 holds
    # their rules); each changes the layer, stays finite and undoes.
    ran = {}
    for key, name, arguments in (
            ('dehaze', 'presence', dict(dehaze=30)),
            ('highlights', 'develop_filter', dict(highlights=30)),
            ('vibrance', 'develop_filter', dict(vibrance=30))):
        reply = app.call('apply_filter', document_id=doc, layer=0, filter=name, **arguments)
        got = samples(key)
        app.call('undo', document_id=doc)
        undone = bits_equal(samples(key + '-undo'), base)
        rows[key + '_runs'] = (reply.get('changed') is True
                               and reply.get('storage') == 'rgba32_float'
                               and all(math.isfinite(v) for v in got)
                               and not bits_equal(got, base) and undone)
        ran[key] = dict(reply=reply, undo_bit_exact=undone)
    detail['runs'] = ran
    refused = {}
    refused['mask'] = app.call('apply_filter', fail=True, document_id=doc, layer=0,
                               filter='surface_blur', radius=3, target='mask')
    # Filters run on a float mask: a maskless layer refuses for it.
    rows['mask_refuses'] = refused['mask'] == 'The target layer has no mask.'
    rows['refusals_change_nothing'] = bits_equal(samples('refused'), base)
    detail['refusals'] = refused

    # Red Eye over a red disc on the scene's ramps.
    cx, cy, radius = F45_EYE
    app.call('select_ellipse', document_id=doc, x=cx - radius, y=cy - radius,
             width=2 * radius + 1, height=2 * radius + 1)
    app.call('fill_selection', document_id=doc, layer=0, color='#d01818')
    app.call('deselect', document_id=doc)
    filled = samples('eye')
    ex, ey, ew, eh = F45_EYE_RECT
    reply = app.call('red_eye', document_id=doc, layer=0, x=ex, y=ey, width=ew, height=eh)
    corrected = samples('red-eye')
    outside, inside = True, False
    for y in range(HEIGHT):
        for x in range(WIDTH):
            same = bits_equal(at(corrected, x, y), at(filled, x, y))
            if ex <= x < ex + ew and ey <= y < ey + eh:
                inside = inside or not same
            else:
                outside = outside and same
    app.call('undo', document_id=doc)
    undone = bits_equal(samples('red-eye-undo'), filled)
    rows['red_eye'] = (reply.get('changed') is True and reply.get('storage') == 'rgba32_float'
                       and inside and outside and undone
                       and all(math.isfinite(v) for v in corrected))
    detail['red_eye'] = dict(reply=reply, inside_changed=inside, outside_bit_exact=outside,
                             undo_bit_exact=undone)

    detail['rows'] = rows
    record(report, 'F45 the display-referred filters, Presence, Develop Filter and Red Eye '
                   'run on a float layer and undo', all(rows.values()), detail)


# ------------------------------------------------------------------- main --

# ------------------------------------------------------------------- F46 --

F46_BANDS = [0.25, 0.999, 1.0, 2.0, 8.0, -0.25]
F46_SEED = (36, 16)
F46_CENTRES = [(8 * k + 4, 16) for k in range(6)]
F46_INK = [4.0, 4.0, 4.0, 1.0]
F46_LINE = ((0.5, 16.5), (47.5, 16.5))
F46_SPONGE_LINE = ((0.5, 4.5), (47.5, 4.5))
F46_MASK_GRAY = 32896
F46_WHITE_BANDS = [0, 0, 65535, 65535, 65535, 0]
F46_MODES = ['Linear Burn', 'Linear Light', 'Pin Light', 'Divide']
F46_UNIT = [[1.0, 0.5, 0.25], [0.18, 1.0, -0.25], [0.999, 0.18, 1.0], [1.0, 1.0, 1.0],
            [1.0, 0.0, 0.0], [0.3, 0.6, 1.0], [1.0, 0.999, 0.18], [0.5, 1.0, 0.75]]
F46_SCALED = [
    ('vibrance', {'vibrance': 0.4}),
    ('selective_color', {'reds': {'c': -0.3, 'y': 0.2}, 'neutrals': {'k': 0.1}}),
    ('replace_color', {'colors': ['#c08060'], 'hue': 30, 'saturation': 0.2}),
    ('hue_saturation', {'saturation': 0.35}),
]


def f46_picture():
    """The F46 bands as f32-rounded Python floats, alpha 1."""
    out = []
    for _ in range(HEIGHT):
        for x in range(WIDTH):
            v = F46_BANDS[x // 8]
            out.extend(struct.unpack('<4f', struct.pack('<4f', v, v, v, 1.0)))
    return out


def f46_shaped(v, e, rng, burn):
    """Dodge/Burn's shaping, written out from the tool's stated curves."""
    k = e / 3.0
    if rng == 0:
        return max((v - k) / (1.0 - k), 0.0) if burn else k + v * (1.0 - k)
    if rng == 1:
        return v ** (1.0 + e) if burn else v ** (1.0 / (1.0 + e))
    return v * (1.0 - k) if burn else min(v / (1.0 - k), 1.0)


def f46_dodge(v, c, e, rng, burn, tables, forward):
    """R4 for one linear component on channel `c`."""
    if v > 1.0:
        t = v * curve_eval(f46_shaped(1.0, e, rng, burn), tables[c])
    elif v < 0.0:
        t = v + curve_eval(f46_shaped(0.0, e, rng, burn), tables[c])
    else:
        t = curve_eval(f46_shaped(encode_through(forward[c], v), e, rng, burn), tables[c])
    return min(max(t, -65504.0), 65504.0)


def f46_blend(mode, cb, cs):
    """`B(Cb, Cs)` of the four float-admitted modes, unclamped."""
    if mode == 'Linear Burn':
        return [b + s - 1.0 for b, s in zip(cb, cs)]
    if mode == 'Linear Light':
        return [b + 2.0 * s - 1.0 for b, s in zip(cb, cs)]
    if mode == 'Pin Light':
        return [min(b, 2.0 * s) if s <= 0.5 else max(b, 2.0 * s - 1.0) for b, s in zip(cb, cs)]
    return [min(max(b / max(s, 1.0 / 65504.0), -65504.0), 65504.0) for b, s in zip(cb, cs)]


def f46_hue_matrix(degrees):
    s, c = math.sin(math.radians(degrees)), math.cos(math.radians(degrees))
    return [[0.213 + c * 0.787 - s * 0.213, 0.715 - c * 0.715 - s * 0.715,
             0.072 - c * 0.072 + s * 0.928],
            [0.213 - c * 0.213 + s * 0.143, 0.715 + c * 0.285 + s * 0.140,
             0.072 - c * 0.072 - s * 0.283],
            [0.213 - c * 0.213 - s * 0.787, 0.715 - c * 0.715 + s * 0.715,
             0.072 + c * 0.928 + s * 0.072]]


def f46_times(m, v):
    return [sum(m[r][k] * v[k] for k in range(3)) for r in range(3)]


def gate_f46(app, folder, report):
    icc = read_float_tiff(FIXTURE)['icc']
    tables = icc_trc_tables(icc)
    forward = forward_tables(tables)
    picture = f46_picture()
    source = folder / 'f46-bands.tiff'
    write_float_tiff(source, WIDTH, HEIGHT, picture, icc)
    rows, detail = {}, dict(tolerance=T_REL, bands=F46_BANDS)
    sx, sy = F46_SEED

    def fresh(label, path=source):
        copy = folder / f'f46-{label}.tiff'
        copy.write_bytes(path.read_bytes())
        return app.open(copy)

    def after(doc, label):
        return export32(app, doc, folder / f'f46-{label}-out.tiff')['samples']

    def undone(doc, label, to):
        app.call('undo', document_id=doc)
        return bits_equal(after(doc, label + '-undo'), to)

    def coverage(doc):
        state = app.call('get_selection', document_id=doc,
                         points=[{'x': x, 'y': y} for x, y in F46_CENTRES])
        return [row.get('coverage16') for row in state.get('samples') or []]

    def stroke(doc, tool_name, line, size, **arguments):
        return app.call(tool_name, document_id=doc, points=f37_points(line), size=size,
                        hardness=100, spacing=10, **arguments)

    white = lambda x, y: 'in' if F46_BANDS[x // 8] >= 1.0 else None
    four = lambda x, y, p: {k: F46_INK[k] for k in range(4)}

    # (a) the bucket on the display copy, linear ink as given.
    doc = fresh('a')
    reply = app.call('fill', document_id=doc, layer=0, x=sx, y=sy, tolerance=0, rgba32=F46_INK)
    rows['a_fill'], detail['a'] = f37_check(after(doc, 'a'), picture, white, four)
    rows['a_reply'] = reply.get('storage') == 'rgba32_float' and reply.get('changed') is True
    detail['a_reply'] = reply
    rows['a_undo'] = undone(doc, 'a', picture)

    # (c) the same with Sample all layers.
    reply = app.call('fill', document_id=doc, layer=0, x=sx, y=sy, tolerance=0, rgba32=F46_INK,
                     sample_all_layers=True)
    rows['c_fill'], detail['c'] = f37_check(after(doc, 'c'), picture, white, four)
    rows['c_reply'] = reply.get('storage') == 'rgba32_float' and reply.get('changed') is True
    detail['c_reply'] = reply
    rows['c_undo'] = undone(doc, 'c', picture)

    # (d) a selection-wide fill with linear ink.
    app.call('select_rect', document_id=doc, x=0, y=0, width=24, height=HEIGHT)
    reply = app.call('fill', document_id=doc, layer=0, x=4, y=16, contiguous=False,
                     rgba32=F46_INK)
    rows['d_fill'], detail['d'] = f37_check(after(doc, 'd'), picture,
                                            lambda x, y: 'in' if x < 24 else None, four)
    rows['d_reply'] = reply.get('storage') == 'rgba32_float'
    detail['d_reply'] = reply
    rows['d_undo'] = undone(doc, 'd', picture)
    app.call('deselect', document_id=doc)

    # (e) Dodge and Burn on the layer: R4.
    core = f37_stroke(F46_LINE, 24)
    for label, (rng, burn, exposure) in (('midtones', (1, False, 50)),
                                         ('highlights', (2, True, 80)),
                                         ('shadows', (0, False, 60))):
        e = exposure / 100.0
        reply = stroke(doc, 'dodge_burn', F46_LINE, 24, layer=0, exposure=exposure,
                       range=label, burn=burn)
        rows[f'e_{label}'], detail[f'e_{label}'] = f37_check(
            after(doc, f'e-{label}'), picture, core,
            lambda x, y, p: {c: f46_dodge(p[c], c, e, rng, burn, tables, forward)
                             for c in range(3)})
        detail[f'e_{label}_reply'] = reply
        rows[f'e_{label}_reply'] = reply.get('storage') == 'rgba32_float'
        rows[f'e_{label}_undo'] = undone(doc, f'e-{label}', picture)

    # (b) a mask filled through the scalar bucket, then (f) dodged: R4m.
    app.call('add_layer_mask', document_id=doc, layer=0)
    masked = after(doc, 'b-mask')
    reply = app.call('fill', document_id=doc, layer=0, target='mask', x=sx, y=sy, tolerance=0,
                     color='#808080', color_space='document')
    filled = after(doc, 'b')
    gray = F46_MASK_GRAY / 65535.0
    # RGB within T_REL: the composite under a partial alpha divides it back out.
    rows['b_mask'] = all(abs(filled[i + 3] - gray) <= 1.0 / 65535.0 + 1e-6
                         and f14_close(filled[i:i + 3], picture[i:i + 3])
                         for i in range(0, len(filled), 4))
    rows['b_reply'] = reply.get('storage') == 'u16'
    detail['b_reply'] = reply
    shaped = round(65535.0 * f46_shaped(gray, 0.5, 1, False)) / 65535.0
    reply = stroke(doc, 'dodge_burn', F46_LINE, 24, layer=0, target='mask', exposure=50,
                   range='midtones')
    dodged = after(doc, 'f')
    worst_f, inside_f, outside_f = 0.0, True, True
    for y in range(HEIGHT):
        for x in range(WIDTH):
            i = (y * WIDTH + x) * 4
            where = core(x, y)
            if where == 'in':
                worst_f = max(worst_f, abs(dodged[i + 3] - shaped))
                inside_f &= f14_close(dodged[i:i + 3], picture[i:i + 3])
            elif where is None:
                outside_f &= bits_equal(dodged[i:i + 4], filled[i:i + 4])
    rows['f_mask'] = worst_f <= 1.0 / 65535.0 + 1e-6 and inside_f and outside_f
    rows['f_reply'] = reply.get('storage') == 'u16'
    detail['f'] = dict(worst=worst_f, want=shaped, reply=reply)
    rows['f_undo'] = undone(doc, 'f', filled)
    app.call('undo', document_id=doc)
    rows['b_undo'] = bits_equal(after(doc, 'b-undo'), masked)

    # (g) the Sponge on the fixture's colours: R5.
    sponged = fresh('g', FIXTURE)
    base = scene()

    def saturate(p):
        y = 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]
        return {c: min(max(y + 2.0 * (p[c] - y), -65504.0), 65504.0) for c in range(3)}
    reply = stroke(sponged, 'sponge_stroke', F46_SPONGE_LINE, 16, layer=0, mode='saturate',
                   strength=100)
    rows['g_sponge'], detail['g'] = f37_check(
        after(sponged, 'g'), base, f37_stroke(F46_SPONGE_LINE, 16),
        lambda x, y, p: saturate(p))
    rows['g_reply'] = reply.get('storage') == 'rgba32_float'
    rows['g_undo'] = undone(sponged, 'g', base)

    # (h)-(k) the smart selections on the display copy.
    doc = fresh('selections')
    app.call('select_magic_wand', document_id=doc, x=sx, y=sy, tolerance=0)
    detail['h'] = coverage(doc)
    rows['h_wand'] = detail['h'] == F46_WHITE_BANDS
    app.call('deselect', document_id=doc)
    app.call('select_color_range', document_id=doc, range='highlights')
    detail['i'] = coverage(doc)
    rows['i_highlights'] = (len(detail['i']) == 6 and detail['i'][0] == 0
                            and detail['i'][5] == 0
                            and all(v >= 65534 for v in detail['i'][1:5]))
    app.call('deselect', document_id=doc)
    app.call('quick_select', document_id=doc, seeds=[[sx, sy]], tolerance=0, edge=0)
    detail['j'] = coverage(doc)
    rows['j_quick'] = detail['j'] == F46_WHITE_BANDS
    app.call('deselect', document_id=doc)
    app.call('select_rect', document_id=doc, x=34, y=14, width=4, height=4)
    app.call('select_similar', document_id=doc, tolerance=0)
    detail['k'] = coverage(doc)
    rows['k_similar'] = detail['k'] == F46_WHITE_BANDS
    app.call('deselect', document_id=doc)
    rows['selections_change_no_pixel'] = bits_equal(after(doc, 'selections'), picture)

    # (l) the four modes admitted at float, on F14's shifted copy.
    doc = fresh('l', FIXTURE)
    app.call('duplicate_layer', document_id=doc, index=0)
    dx, dy = F14_SHIFT
    app.call('set_layer_properties', document_id=doc, index=1, offset_x=dx, offset_y=dy)

    def at(x, y):
        return base[(y * WIDTH + x) * 4:(y * WIDTH + x) * 4 + 4]

    def moved(x, y):
        if 0 <= x - dx < WIDTH and 0 <= y - dy < HEIGHT:
            return at(x - dx, y - dy)
        return [0.0, 0.0, 0.0, 0.0]
    for mode in F46_MODES:
        app.call('set_layer_properties', document_id=doc, index=1, blend_mode=mode)
        got = after(doc, 'l-' + mode.split()[-1].lower())
        failures = []
        for y in range(HEIGHT):
            for x in range(WIDTH):
                bg, src = at(x, y), moved(x, y)
                want = f14_composite(bg[:3], bg[3], src[:3], src[3],
                                     f46_blend(mode, bg[:3], src[:3]))
                out = got[(y * WIDTH + x) * 4:(y * WIDTH + x) * 4 + 4]
                if not f14_close(out, want):
                    failures.append(dict(x=x, y=y, got=out, want=want))
        detail['l_' + mode] = dict(failures=len(failures), first=failures[:2])
        rows['l_' + mode] = not failures

    # (m) closed forms, and the display forms by homogeneity.
    sepia = [[0.393, 0.769, 0.189], [0.349, 0.686, 0.168], [0.272, 0.534, 0.131]]
    closed = [('hue_rotate', {'degrees': 30}, lambda v: f46_times(f46_hue_matrix(30), v)),
              ('sepia', {}, lambda v: f46_times(sepia, v)),
              ('grayscale', {}, lambda v: [0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2]] * 3)]
    for op, params, oracle in closed:
        doc = fresh('m-' + op, FIXTURE)
        app.call('add_adjustment_layer', document_id=doc, op=op, params=params)
        got = after(doc, 'm-' + op)
        bad = [i for i in range(0, len(got), 4)
               if not f14_close(got[i:i + 3], oracle(base[i:i + 3]))
               or not bits_equal(got[i + 3:i + 4], base[i + 3:i + 4])]
        detail['m_' + op] = dict(failures=len(bad))
        rows['m_' + op] = not bad
    unit = folder / 'f46-unit.tiff'
    samples = []
    for k in (1.0, 2.0, 8.0):
        for colour in F46_UNIT:
            samples.extend(struct.unpack('<4f', struct.pack('<4f', *[k * v for v in colour], 1.0)))
    write_float_tiff(unit, len(F46_UNIT), 3, samples, icc)
    for op, params in F46_SCALED:
        doc = fresh('m-' + op, unit)
        app.call('add_adjustment_layer', document_id=doc, op=op, params=params)
        got = export32(app, doc, folder / f'f46-m-{op}-out.tiff')['samples']
        n = len(F46_UNIT) * 4
        bad = []
        for row, k in ((1, 2.0), (2, 8.0)):
            for i in range(0, n, 4):
                want = [k * v for v in got[i:i + 3]]
                if not f14_close(got[row * n + i:row * n + i + 3], want):
                    bad.append(dict(row=row, pixel=i // 4, got=got[row * n + i:row * n + i + 3],
                                    want=want))
        detail['m_' + op] = dict(failures=bad[:3])
        rows['m_homogeneous_' + op] = not bad

    # (n) the destructive apply equals its layer.
    for op, layer_params, flat in (('threshold', {'level': 0.5}, {'level': 0.5}),
                                   ('white_balance', {'temperature': 5000},
                                    {'params': {'temperature': 5000}})):
        layered = fresh('n-layer-' + op, FIXTURE)
        app.call('add_adjustment_layer', document_id=layered, op=op, params=layer_params)
        want = after(layered, 'n-layer-' + op)
        applied = fresh('n-apply-' + op, FIXTURE)
        reply = app.call('apply_filter', document_id=applied, layer=0, filter=op, **flat)
        got = after(applied, 'n-apply-' + op)
        rows['n_' + op] = (bits_equal(got, want) and reply.get('storage') == 'rgba32_float'
                           and not bits_equal(got, base))
        detail['n_' + op] = dict(reply=reply)
        rows['n_' + op + '_undo'] = undone(applied, 'n-' + op, base)

    # (o) Enhance Edge runs on the display codes (rule E), then the
    # refusals by name.
    doc = fresh('o')
    enhanced = app.call('quick_select', document_id=doc, seeds=[[sx, sy]], tolerance=0, edge=0,
                        enhance_edge=True)
    detail['o_enhance_edge'] = dict(reply=enhanced, coverage=coverage(doc))
    rows['o_enhance_edge'] = (detail['o_enhance_edge']['coverage'][4] >= 32768
                              and detail['o_enhance_edge']['coverage'][0] == 0)
    app.call('deselect', document_id=doc)
    refusals = dict(
        screen=app.call('set_layer_properties', fail=True, document_id=doc, index=0,
                        blend_mode='Screen'),
        sponge_mask=None, fill_mask_ink=None)
    app.call('add_layer_mask', document_id=doc, layer=0)
    refusals['fill_mask_ink'] = app.call('fill', fail=True, document_id=doc, layer=0,
                                         target='mask', x=sx, y=sy, rgba32=F46_INK)
    refusals['sponge_mask'] = stroke(doc, 'sponge_stroke', F46_LINE, 24, fail=True, layer=0,
                                     target='mask', mode='saturate')
    refusals['invert'] = app.call('add_adjustment_layer', fail=True, document_id=doc,
                                  op='invert', params={})
    refusals['shadows_highlights'] = app.call(
        'add_adjustment_layer', fail=True, document_id=doc, op='shadows_highlights',
        params={'shadows': {'amount': 0.35}})
    app.call('add_adjustment_layer', document_id=doc, op='exposure', params={'exposure': 1.0})
    layers = app.call('get_document', document_id=doc)['layers']
    adjustment = next(row for row in layers if row.get('is_adjustment'))
    refusals['dodge_adjustment'] = stroke(doc, 'dodge_burn', F46_LINE, 24, fail=True,
                                          layer=adjustment['index'])
    detail['o'] = refusals
    rows['o_screen'] = '32-bit float layer blending supports' in refusals['screen']
    rows['o_fill_mask_ink'] = F37_COVERAGE_INK in refusals['fill_mask_ink']
    rows['o_sponge_mask'] = 'The Sponge changes saturation' in refusals['sponge_mask']
    rows['o_invert'] = 'the invert adjustment' + F37_UNAVAILABLE in refusals['invert']
    rows['o_shadows_highlights'] = ('the shadows_highlights adjustment' + F37_UNAVAILABLE
                                    in refusals['shadows_highlights'])
    rows['o_dodge_adjustment'] = 'adjustment' in refusals['dodge_adjustment']

    detail['rows'] = rows
    record(report, 'F46 the display-defined tools, blend modes and adjustments at 32 bits',
           all(rows.values()), detail)


# ------------------------------------------------------------------- F47 --

# Fixed before the run, never widened.
F47_SIZE = (64, 48)
F47_SQUARE = (16, 12, 32, 24)       # x, y, width, height: 8.0
F47_INSET = (24, 18, 16, 12)        # 2.0 inside the square
F47_DEEP = (20, 16)                 # inside the square, outside the inset: 8.0
F47_TRACE = [[16, 12], [32, 11], [48, 12], [49, 24], [48, 36], [32, 37], [16, 36], [15, 24]]
F47_RECT = (18, 14, 26, 18)         # the selection Select and Mask refines
F47_BRUSH = [{'mode': 'add', 'radius': 3, 'points': [[16, 24], [16, 30]]}]
F47_DECONTAMINATE = "Select and Mask's Decontaminate Colors" + F37_UNAVAILABLE
F47_FILTER_SIZE = (96, 48)
F47_BANDS = [0.25, 0.999, 1.0, 2.0, 8.0, -0.25]   # the top half's grey bands, 16 px each
F47_FILTERS = [
    ('dehaze_up', 'presence', dict(dehaze=40), 'held'),
    ('dehaze_down', 'presence', dict(dehaze=-40), 'bounded'),
    ('highlights', 'develop_filter', dict(highlights=40), 'white'),
    ('shadows', 'develop_filter', dict(shadows=40), 'white'),
    ('cr_dehaze', 'develop_filter', dict(dehaze=40), 'held'),
    ('vibrance', 'develop_filter', dict(vibrance=40), 'grey'),
    ('saturation', 'develop_filter', dict(saturation=-40), 'grey'),
]
F47_REFUSED = [
    'a blend mode whose formula inverts about white (Screen, Overlay, Soft Light, Hard Light, '
    'Color Dodge, Color Burn, Vivid Light, Hard Mix, Exclusion)',
    'the invert adjustment',
    'the shadows_highlights adjustment',
    'the white_balance adjustment with legacy evaluation',
    'Assign Profile or Convert to Profile with a profile that has no matrix/TRC model, and Assign '
    'Profile with one whose tone curves fail the precision check',
    'Convert to Profile with absolute colorimetric intent, black-point compensation or '
    'preserve-appearance adjustments',
    'Apply Image',
    "Select and Mask's Decontaminate Colors",
    "automatic tone correction (Auto Tone, Auto Contrast, Auto Color and the Levels dialog's Auto "
    'buttons)',
]
F47_AUTO = F47_REFUSED[-1]
F47_ABOVE = ['a blend mode whose formula inverts about white', 'the invert adjustment',
             'the shadows_highlights adjustment',
             'the white_balance adjustment with legacy evaluation']


def f47_inside(rect, x, y):
    x0, y0, w, h = rect
    return x0 <= x < x0 + w and y0 <= y < y0 + h


def f47_pictures():
    """The square picture (float samples and the 16-bit twin's codes) and the
    filter picture, as f32-rounded Python floats."""
    width, height = F47_SIZE
    square, codes = [], []
    for y in range(height):
        for x in range(width):
            v = 2.0 if f47_inside(F47_INSET, x, y) else 8.0 if f47_inside(F47_SQUARE, x, y) else 0.0
            square.extend((v, v, v, 1.0))
            c = 65535 if v > 0.0 else 0
            codes.extend((c, c, c, 65535))
    fw, fh = F47_FILTER_SIZE
    filters = []
    for y in range(fh):
        for x in range(fw):
            if y < fh // 2:
                v = F47_BANDS[x // 16]
                filters.extend((v, v, v, 1.0))
            else:
                filters.extend((0.01 + 1.44 * x / (fw - 1), 0.02, 0.02 + 0.08 * y / (fh - 1), 1.0))
    pack = lambda values: list(struct.unpack('<%df' % len(values),
                                             struct.pack('<%df' % len(values), *values)))
    return pack(square), codes, pack(filters)


def gate_f47(app, folder, report):
    """Select and Mask, Enhance Edge, the Magnetic Lasso, Dehaze
    and the Develop Filter's Highlights, Shadows, Vibrance and Saturation on
    a float document, and the operator table's last rows."""
    icc = SYSTEM_SRGB.read_bytes()
    square, codes, picture = f47_pictures()
    width, height = F47_SIZE
    rows, detail = {}, dict(bounds=dict(relative=T_REL, coverage16=T_COV16))
    lattice = [(x, y) for y in range(height) for x in range(width)]

    def opened(label):
        doc = app.open(write_float_tiff(folder / f'f47-{label}.tiff', width, height, square, icc))
        png16(folder / f'f47-{label}-twin.png', width, height, codes)
        return doc, app.open(folder / f'f47-{label}-twin.png')

    def cover(document):
        return f43_coverage(app, document, lattice)

    def history(document):
        return app.call('list_history', document_id=document)

    def export(document, label):
        return export32(app, document, folder / f'f47-{label}.tiff')['samples']

    def at(pixels, x, y, w=width):
        return pixels[(y * w + x) * 4:(y * w + x) * 4 + 4]

    # ---- the Magnetic Lasso traces the display codes (rule M) ----
    doc, twin = opened('magnetic')
    precisions = [app.call('get_document', document_id=d).get('precision') for d in (doc, twin)]
    traced = [app.call('select_magnetic', document_id=d, points=F47_TRACE) for d in (doc, twin)]
    got, want = cover(doc), cover(twin)
    rows['magnetic'] = (precisions == ['rgba32_float', 'rgba16'] and got == want
                        and traced[0].get('outline_points') == traced[1].get('outline_points')
                        and got[F47_DEEP[1] * width + F47_DEEP[0]] == 65535 and got[0] == 0)
    detail['magnetic'] = dict(precisions=precisions, replies=traced,
                              differing=sum(1 for a, b in zip(got, want) if a != b))

    # ---- Quick Selection's Enhance Edge (rule E) ----
    for d in (doc, twin):
        app.call('deselect', document_id=d)
        app.call('quick_select', document_id=d, seeds=[list(F47_DEEP)], tolerance=0, edge=0,
                 enhance_edge=True)
    got, want = cover(doc), cover(twin)
    rows['enhance_edge'] = got == want and got[F47_DEEP[1] * width + F47_DEEP[0]] >= 32768
    detail['enhance_edge'] = dict(differing=sum(1 for a, b in zip(got, want) if a != b))

    # ---- Select and Mask: the edge stage and the brush (rule E) ----
    x0, y0, w, h = F47_RECT
    for d in (doc, twin):
        app.call('select_rect', document_id=d, x=x0, y=y0, width=w, height=h)
    rect = cover(doc)
    saved = history(doc)
    refine = dict(edge_radius=4, smart_radius=True, brush=F47_BRUSH)
    replies = [app.call('refine_selection', document_id=d, output='selection', **refine)
               for d in (doc, twin)]
    refined, want = cover(doc), cover(twin)
    rows['refine_selection'] = (refined == want and refined != rect
                                and replies[0].get('coverage_storage') == 'u16')
    detail['refine_selection'] = dict(replies=replies,
                                      differing=sum(1 for a, b in zip(refined, want) if a != b),
                                      moved=sum(1 for a, b in zip(refined, rect) if a != b))
    app.call('restore_history', document_id=doc, state_id=saved['current_state_id'])
    base = export(doc, 'refine-base')

    # mask: the refined coverage as the layer's mask.
    reply = app.call('refine_selection', document_id=doc, output='mask', layer=0, **refine)
    app.call('load_selection', **{'document_id': doc, 'from': 'layer_mask', 'layer': 0})
    masked = cover(doc)
    rows['refine_mask'] = (reply.get('changed') is True
                           and max(abs(a - b) for a, b in zip(masked, refined)) <= T_COV16)
    detail['refine_mask'] = dict(reply=reply)
    app.call('restore_history', document_id=doc, state_id=saved['current_state_id'])

    # new_layer and new_layer_with_mask: the float composite, 8.0 kept, read
    # where the refined coverage is full.
    full = [(x, y) for (x, y), v in zip(lattice, refined) if v == 65535]
    eight = next(((x, y) for x, y in full
                  if f47_inside(F47_SQUARE, x, y) and not f47_inside(F47_INSET, x, y)), None)
    two = next(((x, y) for x, y in full if f47_inside(F47_INSET, x, y)), None)
    layers = len(app.call('get_document', document_id=doc)['layers'])
    for output in ('new_layer', 'new_layer_with_mask'):
        reply = app.call('refine_selection', document_id=doc, output=output, **refine)
        state = app.call('get_document', document_id=doc)
        out = export(doc, output)
        deep = at(out, *eight) if eight else None
        inset = at(out, *two) if two else None
        rows['refine_' + output] = (reply.get('changed') is True
                                    and reply.get('new_layer') == layers
                                    and len(state['layers']) == layers + 1
                                    and state.get('precision') == 'rgba32_float'
                                    and deep is not None and inset is not None
                                    and bits_equal(deep, [8.0, 8.0, 8.0, 1.0])
                                    and bits_equal(inset, [2.0, 2.0, 2.0, 1.0])
                                    and at(out, 0, 0)[3] == 0.0)
        detail['refine_' + output] = dict(reply=reply, eight=eight, deep=deep, two=two,
                                          inset=inset, corner=at(out, 0, 0))
        app.call('restore_history', document_id=doc, state_id=saved['current_state_id'])
    rows['refine_restored'] = bits_equal(export(doc, 'refine-restored'), base)

    # Decontaminate Colors refuses by name and leaves no step.
    cursor = history(doc)['cursor']
    refused = app.call('refine_selection', fail=True, document_id=doc, output='new_layer',
                       decontaminate=True, **refine)
    rows['decontaminate_refuses'] = (F47_DECONTAMINATE in refused
                                     and history(doc)['cursor'] == cursor)
    detail['decontaminate'] = refused

    # ---- Dehaze, Highlights, Shadows, Vibrance and Saturation (rules D, H, V) ----
    fw, fh = F47_FILTER_SIZE
    source = write_float_tiff(folder / 'f47-filters.tiff', fw, fh, picture, icc)
    filtered = app.open(source)
    start = export32(app, filtered, folder / 'f47-filters-base.tiff')['samples']
    rows['filters_base'] = bits_equal(start, picture)

    def band(x, y):
        return F47_BANDS[x // 16] if y < fh // 2 else None

    for key, name, arguments, rule in F47_FILTERS:
        reply = app.call('apply_filter', document_id=filtered, layer=0, filter=name, **arguments)
        got = export32(app, filtered, folder / f'f47-{key}.tiff')['samples']
        app.call('undo', document_id=filtered)
        undone = bits_equal(export32(app, filtered, folder / f'f47-{key}-undo.tiff')['samples'],
                            picture)
        kept, worst = True, 0.0
        for y in range(fh):
            for x in range(fw):
                g, p = at(got, x, y, fw), at(picture, x, y, fw)
                if rule == 'held':
                    kept = kept and all(g[c] == p[c] for c in range(3) if p[c] > 1.0)
                elif rule == 'bounded':
                    kept = kept and all(g[c] <= max(p[c], 1.0) * (1 + 1e-6) for c in range(3))
                elif band(x, y) in (2.0, 8.0):
                    if rule == 'white':
                        kept = kept and bits_equal(g, p)
                    else:
                        worst = max(worst, max(abs(g[c] - p[c]) / abs(p[c]) for c in range(3)))
        if rule == 'grey':
            kept = worst <= T_REL
        rows[key] = (reply.get('changed') is True and reply.get('storage') == 'rgba32_float'
                     and all(math.isfinite(v) for v in got) and not bits_equal(got, picture)
                     and kept and undone)
        detail[key] = dict(reply=reply, kept=kept, worst_grey=worst, undo_bit_exact=undone)

    # ---- the operator table: nine rows, each reason above 1.0 where it is about range ----
    table = (app.call('get_precision', document_id=filtered).get('operators') or {}).get(
        'refused') or []
    names = [row.get('operator') for row in table]
    rows['refused_rows'] = names == F47_REFUSED
    reasons = {row.get('operator'): row.get('reason', '') for row in table}
    rows['above_white_reasons'] = all(
        any(name.startswith(prefix) and 'above 1.0' in reasons[name] for name in reasons)
        for prefix in F47_ABOVE)
    detail['refused'] = table

    # ---- Auto Tone and the Levels Auto refuse at 32 bits ----
    auto_reason = reasons.get(F47_AUTO) or None
    refused = app.call('auto_tone', fail=True, document_id=filtered)
    unchanged = bits_equal(export32(app, filtered, folder / 'f47-auto.tiff')['samples'], picture)
    rows['auto_refuses'] = auto_reason is not None and auto_reason in refused and unchanged
    detail['auto_refuses'] = dict(text=refused, unchanged=unchanged)
    refused = app.call('add_adjustment_layer', fail=True, document_id=filtered, op='levels',
                       auto={'mode': 'tone'})
    unchanged = bits_equal(export32(app, filtered, folder / 'f47-levels-auto.tiff')['samples'],
                           picture)
    rows['levels_auto_refuses'] = 'automatic tone correction' in refused and unchanged
    detail['levels_auto_refuses'] = dict(text=refused, unchanged=unchanged)

    detail['rows'] = rows
    record(report, 'F47 Select and Mask, Enhance Edge, the Magnetic Lasso, Dehaze and the Camera '
                   'Raw tone and colour controls run on a float document', all(rows.values()),
           detail)


GATE_ORDER = ['F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8', 'F10', 'F11', 'F12', 'F13', 'F14',
              'F15', 'F16', 'F17', 'F18', 'F20', 'F28', 'F30', 'F32', 'F34', 'F35', 'F36', 'F37',
              'F38', 'F39', 'F40', 'F42', 'F45', 'F9']
PREREQUISITES = {'F2': ('F1',), 'F3': ('F2',), 'F4': ('F3',), 'F5': ('F3',), 'F6': ('F5',),
                 'F7': ('F5',)}
GATE_ORDER.insert(GATE_ORDER.index('F9'), 'F43')
GATE_ORDER.insert(GATE_ORDER.index('F9'), 'F44')
GATE_ORDER.insert(GATE_ORDER.index('F9'), 'F46')
GATE_ORDER.insert(GATE_ORDER.index('F9'), 'F47')


def selected_gates(spec: str) -> list[str]:
    """The gates `--gates` names plus their prerequisites, in `GATE_ORDER` order."""
    named = [gate.strip().upper() for gate in spec.split(',') if gate.strip()]
    if not named:
        raise ValueError('--gates names no gate')
    unknown = [gate for gate in named if gate not in GATE_ORDER]
    if unknown:
        raise ValueError(f'unknown gate(s): {",".join(unknown)}')
    chosen, pending = set(), list(named)
    while pending:
        gate = pending.pop()
        if gate not in chosen:
            chosen.add(gate)
            pending.extend(PREREQUISITES.get(gate, ()))
    return [gate for gate in GATE_ORDER if gate in chosen]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--app')
    parser.add_argument('--output')
    parser.add_argument('--gates', default=','.join(GATE_ORDER),
                        help='comma list of gates to run (prerequisites added), from '
                             + ','.join(GATE_ORDER))
    parser.add_argument('--author-fixture', metavar='PATH',
                        help='Write the float fixture and exit.')
    args = parser.parse_args()
    if args.author_fixture:
        author_fixture(args.author_fixture)
        print('wrote', args.author_fixture)
        return
    try:
        selected = selected_gates(args.gates)
    except ValueError as error:
        parser.error(str(error))
    assert args.app and args.output, '--app and --output are required'
    executable = Path(args.app)
    if executable.is_dir():
        executable = executable / 'Contents/MacOS/Doctored'
    assert executable.is_file(), executable
    assert FIXTURE.is_file(), FIXTURE
    output = Path(args.output).expanduser().resolve()
    output.mkdir(parents=True, exist_ok=True)
    folder = output / 'work'
    if folder.exists():
        import shutil
        shutil.rmtree(folder)
    folder.mkdir(parents=True)

    report = {'gates': {}, 'selected': selected,
              'tolerances': {'relative': T_REL, 'codes8': T_CODE8, 'codes16': T_CODE16,
                             'coverage16': T_COV16},
              'fixture': dict(path=str(FIXTURE), sha256=digest(FIXTURE))}
    after = {'F10': gate_f10, 'F11': gate_f11, 'F13': gate_f13, 'F14': gate_f14,
             'F15': gate_f15, 'F16': gate_f16, 'F17': gate_f17, 'F18': gate_f18,
             'F20': gate_f20, 'F28': gate_f28, 'F30': gate_f30, 'F32': gate_f32,
             'F34': gate_f34, 'F35': gate_f35, 'F36': gate_f36, 'F37': gate_f37,
             'F39': gate_f39, 'F40': gate_f40, 'F42': gate_f42, 'F45': gate_f45}
    previous = phase5_support.APP
    after['F43'] = gate_f43
    after['F44'] = gate_f44
    after['F46'] = gate_f46
    after['F47'] = gate_f47
    phase5_support.APP = executable
    try:
        with App(folder / 'app', folder / 'idle-recovery', interval=600) as app:
            if {'F8', 'F9', 'F12', 'F38'} & set(selected):
                u16_document, u16_before = u16_state(app, folder, 'before')
            for gate in selected:
                if gate == 'F1':
                    document = gate_f1(app, folder, report)
                elif gate == 'F2':
                    samples = gate_f2(app, folder, document, report)
                elif gate == 'F3':
                    reopened = gate_f3(app, folder, document, samples, report)
                elif gate == 'F4':
                    gate_f4(app, folder, reopened, samples, report)
                elif gate == 'F5':
                    linear = gate_f5(app, folder, reopened, report)
                elif gate == 'F6':
                    gate_f6(app, folder, reopened, linear, report)
                elif gate == 'F7':
                    gate_f7(app, folder, reopened, linear, report)
                elif gate == 'F8':
                    gate_f8(app, folder, report, u16_document)
                elif gate == 'F12':
                    gate_f12(app, folder, report, u16_document)
                elif gate == 'F38':
                    gate_f38(app, folder, report, u16_document)
                elif gate == 'F9':
                    _, u16_after = u16_state(app, folder, 'after')
                    detail = dict(before=u16_before, after=u16_after)
                    record(report, 'F9 a 16-bit document is byte-identical before and after',
                           u16_before['native'] == u16_after['native']
                           and u16_before['export'] == u16_after['export']
                           and u16_before['first_words'] == [46, 13], detail)
                else:
                    after[gate](app, folder, report)
    finally:
        phase5_support.APP = previous
    if 'F4' in selected:
        gate_f4_recovery(folder, report, executable, samples)

    (output / 'receipt.json').write_text(json.dumps(report, indent=2, sort_keys=True) + '\n')
    if selected == GATE_ORDER:
        print('ALL GATES PASS', flush=True)
    else:
        print('SELECTED GATES PASS: ' + ','.join(selected), flush=True)


if __name__ == '__main__':
    main()
