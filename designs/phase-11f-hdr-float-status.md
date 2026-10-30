# HDR and 32-bit float document workflow (ADR)

- **Status:** Accepted — open items remain (deliberately bounded operator set)
- **Phase / dates:** 11F, shipped 2026-09-18 (drive gates F1–F10 pass); admitted set widened by 12E-2b/2c and 12F-1/1b
- **Originals:** [archive/phase-11f-hdr-float-status.md](../archive/phase-11f-hdr-float-status.md) — the phase's full history, to 2026-09-26; [archive/2026-10/phase-11f-hdr-float-status.md](../archive/2026-10/phase-11f-hdr-float-status.md) — this doc as of 2026-10-07, with every measurement table and issue record
- **Key commits:** `a3e4f2c`
- **Carryover:** closes **P7-25** (HDR/extended-range storage, editing, output) and **P7-36** (float TIFF interchange)

> **Removed 2026-10-10:** smart objects, live filters and non-RAW retained sources (placed images, host
> media, Live Photo frames) are gone for v1; camera RAW layers stay. Where this record describes them, that
> code no longer exists: [removal ADR](convert-to-smart-object-status.md).

## Context
Genuine HDR/scene-linear input (above-white and negative wide-gamut values) had no home: the typed document stored
u16 only. The plan required preserving real float input through editing, native/history/recovery and float TIFF
I/O, with HDR readouts and deliberate tone-mapped SDR output — qualified on genuine float data, not a promoted SDR
image. Byte and u16 modes had to stay byte-identical. Builds on [11D](phase-11d-format-additions-status.md)'s
`tiff_float.rs` seam and [11E](phase-11e-linear-light-status.md)'s evaluation policy.

## Open items (verified 2026-10-07)
- `DONE` (2026-10-12) — tolerance/distance source selections (Magic Wand, Color Range, Select Similar, Quick
  Selection) compare the R1 display copy (`display_encoding`; [v1-float-editing.md](v1-float-editing.md)), and so
  does Select and Mask's edge stage (Refine Edge, Quick Selection's Enhance Edge, Object Selection's refinement; rule
  E, the v1 float closing batch); Decontaminate Colors stays refused by name. Placed images are admitted
  since phase 187a, text and shape layers since 187b and live filters since 187c3 (the shipped companion);
  Divide is admitted under rule B (below); above-white text/shape colour ships (the shipped companion), but
  Photoshop's own above-white TYPE colour is unmeasured (a script-made type layer ignores the foreground, take2
  T-none); Fill/Stroke take an above-white foreground (below) and the options-bar Text and Shape swatches author
  one (below); colour-glyph type at float stays open, as do the last 32-bit float master gaps (phase 1, a
  float master in a float document, phase 2a, Convert to Smart Object at 32 bits, phase 2b, Edit Contents and its
  commit at 32 bits, phase 2c, Convert to Layers, Export Contents and Convert to Linked at 32 bits, phase 2d, a
  layered `.rz` or PSD placed, replaced, updated and relinked at 32 bits, phase 3a, a float master in an 8/16-bit
  document rendered clipped (`.rz` words 67 / 72), phase 3b-1, float smart-object contents in an 8/16-bit
  document (`.rz` words 68 / 73), phase 3b-2, their Convert to Layers, phase 3c-1, a PSD smart object's 32-bit contents read in an 8/16-bit file, phase 3c-2, written there, and phase 3c-3, their masks, effects and adjustment layers, shipped; whether Photoshop keeps one whose converted colours pass ±65504 is
  unmeasured, and the place refuses; 24 MP admits a 140 % transform, and at 150 % the f32 master exceeded the native
  1.6 GB decoded cap, since raised to 8 GiB).
- v1 float gaps, shipped: Apply Layer Mask (alpha ·
  coverage in f32, RGB bit for bit; `precision_mask_apply`), Layer Via Copy / Cut (`precision_layer_via`'s f32 arm;
  the cut is the float Cut's erase), Assign Profile (the linear samples relabelled) and Convert to Profile (the matrix
  between the two profiles' primaries in linear light, no clamp; `float_color`), and, with no core change,
  Calculations, Add Luminosity Masks (planes read clamped to 0…1) and Contact Sheet (tone-mapped 8-bit cells). Menu
  items and MCP twins are admitted; `scripts/verify_v1_float_gaps.py` drives them. Left refused, each by name with
  its reason in the operator table: Apply Image (its blend runs on u16 planes clamped to 0…1, so writing float
  pixels needs a rule for values above white and below zero); Assign or Convert with a profile that has no
  matrix/TRC model, and Assign of one whose tone curves fail the precision check (linear values are defined by the
  profile's primaries, and display and SDR export encode through its curves); Convert with absolute colorimetric
  intent, black-point compensation or preserve-appearance (each needs an undecided rule for linear extended-range
  values); a converted component outside ±65504 (refused, not clipped).
- `DONE` (2026-10-12) — the adjustments refused at 32 bits: every Hue/Saturation setting, every Color Lookup table,
  Hue Rotate, Grayscale, Sepia, Threshold, Posterize, Gradient Map, Vibrance, Selective Color, Replace Color and the
  `document_v2` White Balance apply run under rule A ([v1-float-editing.md](v1-float-editing.md)). Refused by name, with
  their reasons in the operator table: Invert, Shadows/Highlights and the legacy White Balance.
- float_adjust take3 (`float-adjust-473/take3`, `scripts/photoshop-probe/float_adjust_take3.py`), 32-bit verdicts:
  Color Lookup, measured only as Photoshop's render of a device-link `profile`, pins `linear`, `tetrahedral`,
  `table-clip`, `clamp`, `zero`, apply `same` — tetrahedral was outside take3's own build table, and it is
  built since (below); Black & White with Tint matched none of the twelve BT forms (best 221 units), so
  Tint at 32 bits stays refused; the Curves map form pinned `kept-map`, `lin`, `channel-first`, `slope`/`slope`, apply `same` and
  ships. At 16 bits BT16 and CM16 are unmatched against today's 8/16-bit kernels; the 8/16-bit Color Lookup was trilinear before the tetrahedral item below.
- float_adjust take4 (`float-adjust-473/take4`, `scripts/photoshop-probe/float_adjust_take4.py`): three scripted shapes
  each left a Color Lookup an identity, so Photoshop's own load of a `.cube` is unmeasured (NL `invalid`) and a 1D
  table, a domain other than 0…1 and entries outside 0…1 refuse at 32 bits. CM16 is `unmatched`: the composed
  4096-stage table scores 2.93 units (it holds on M2, M4, M6 and misses M5, mostly a stored 1.0 rendered 237.5/255
  against the table's 239/255), every other candidate 42 or more, so 16-bit map Curves is unchanged until take5.
- float_adjust take5 (`float-adjust-473/take5`, `scripts/photoshop-probe/float_adjust_take5.py` sha `3d2ac9fb…`): CM16X
  pins `int15`, all-integer 15-bit stage tables, exact on all 1368 samples of M2, M4–M8 (rivals 2 to 58887 codes off);
  LG pins `own` (a link read on its own grid) at 16 and 32 bits (0.048 / 1.2e-4 units; 33-node bake 31, trilinear 47).
  Color Lookup Dither: 16 bits `none` (on = off); 8 bits `other` — an aperiodic, repeatable noise rounding about half of
  the half-code pixels down (1061 of 2048) and 4 near-integer ones up, not the 8×8 Bayer threshold, so the Dither item
  stops; the key is not built.
- float_adjust take6 (`float-adjust-473/take6`, `scripts/photoshop-probe/float_adjust_take6.py` sha `acd84bee…`),
  Black & White with Tint as SetLum of the tint numbers to the gray with ClipColor: 32 bits BT32-low pins
  `clip/e510/num` (0.003 units, G < 4.5), apply `same`, but BT32-hi is unmatched — Photoshop caps the gray at 16 —
  so the 32-bit Tint stops; 16 bits pins `setlum-clip16` (0.14 units;
  today's `hsl16` 135) and the low floor `e15`, but the high-only floor (±2 15-bit codes) and BT8 (worst 3 codes) are
  unmatched, so the 8/16-bit Tint stops. Ruling 1 shipped the pinned forms
  under the near-match rule (shipped companion).
- float_adjust take7 (`scripts/photoshop-probe/float_adjust_take7.py` sha `ba79af9d…`, `float-adjust-473/take7`)
  was fixed to confirm take6's two fits on fresh stimuli — the 32-bit gray cap at 16 (`cap16`) and Photoshop's 8-bit
  integer tint path (`h8a`, exact on all 792 take6 8-bit samples) — and stalled with no reading. Ruling 2 shipped the
  build as it stands: 32 bits cap the gray at 16 (take6's fit); residual 16-bit
  high-only minimum 2 15-bit codes. The 8-bit residual (`setlum-clip8`, 3 codes) closed with
  [#578](https://github.com/kpgalligan/doctored-dev/issues/578) phase 4: a byte document's Tint runs Photoshop's integer
  path (h8a-LE, `adjust_bw8.rs`), within 1 code on `eight-bit-578/take1`.
- `DONE` — Color Lookup's one 3D lookup (`adjust_lut::sample_3d`)
  is tetrahedral at 8, 16 and 32 bits, as take3 pinned at 16 and 32. Nothing persisted changes; every saved Color
  Lookup layer re-renders. Since take5 a plain-grid device link of at most 33 nodes is stored at its own grid
  (`icc_lookup`, `LutPipeline::plain_grid`); larger or curved links and abstracts stay sampled at 33³.
- `DONE` — 16-bit map-form Curves reads take5's 15-bit stage
  tables (`adjust_curves::stage15`/`read15`, three composed tables built at parse); 8 and 32 bits unchanged. Nothing
  persisted changes; saved 16-bit map Curves layers re-render.
- `DONE` — Color Lookup renders and applies at 32 bits for a 3D
  table with a 0…1 domain and entries in 0…1, whatever loaded it, under take3's device-link form
  (`FloatForm::ColorLookup` over the 8/16-bit `CubeLut::apply`); the unloaded 1D identity passes through.
- Hue/Saturation take4 (`adjust-parity-535-536/take4`,
  `scripts/photoshop-probe/hue_sat_take4.py`), max tolerance units at 16 / 32 bits:
  T4-hue `s4096t` (1/24576-turn shift, truncated toward 0) unmatched 1.13 / pinned 0.74 — master Hue alone ships at
  32; T4-gain `tid` unmatched 1.15 / 1.09 (sat+ mid residual), so master Saturation and every master combination take
  the display-scaled form at 32; T4-master unmatched 1.09 / pinned 0.69; T4-light16 `float` unmatched 1.12 at
  16, Lm32 pinned 0.51; T4-band `h359` unmatched 1.39 / 1.26; T4-band-net netted unmatched 1.14 / pinned 0.64;
  T4-band-order `before` unmatched 1.21 / 881; T4-combine `mgrid` unmatched 16.4 / 1828. At 8 and 16 bits take4's
  reference form R0 ships (2026-10-13, `adjust_hue_saturation.rs`): take4's 8-bit max codes 2 (light,
  gain, hue), 3 (master), 7 (band), 5 (combine), and the `hue2` fixture within 2 codes at 8 bits.
- Color Balance take5 and take6 (`adjust-parity-535-536`,
  `color_balance_parity.py` / `color_balance_take6.py`), max tolerance units at 16 / 32 bits: take5 pinned toe `hermite`,
  round `round2`, range `free`, pref `norm`, pgamma `none`, x32 `lut-ends` on rail ends; bw and form missed on T3 alone
  (9.25) and the black/white-point ends at 32 (4.4e4). take6 pinned tone `sum` t = M + trunc((S + H)/2) 0.64 / 0.61,
  pmid `midtrunc` 0.31 / 0.21, bw `levels` 0.68 / 0.74, form `fitted` 0.68 / 0.74 (`today` 6776) and the 32-bit
  end slopes `stages` 4.3e-4 (`stages-dr` 2.4e4): build row 1, Color Balance replaced at 8/16 bits and admitted at 32
  as a layer and a destructive apply (`adjust_color_balance`).
- `DONE` — RAW-developed HDR: a camera RAW opens as a 32-bit float document with
  `raw.output_encoding: linear_float`. `camera.NEF` at +2 EV holds lattice values to 2.2 with over 300 of 640 points
  above 1.0, where its 16-bit development clips at 65535 (`scripts/verify_v1_raw_float.py`; ADR
  `phase-8g-raw-enhancements-status.md`).
- `TRACKED` — float-predictor TIFF encoding (owned writer checked by `tiffinfo`) → P7-36 row note and plan §11
  gate (3). F16 TIFF export and OpenEXR shipped with v1 float phase 1 (next item).
- `DONE` v1 float phase 1 — float in, out
  and on screen. OpenEXR (`exr_float`, over the `exr` crate) and Radiance `.hdr` (`radiance_hdr`) open as float
  documents: the first part that is not deep and has R, G and B, its display window as the canvas, associated alpha
  unpremultiplied; stated xy primaries within 0.002 of Rec.709 or Display P3 take that built-in profile, any other
  set is converted once in linear light to sRGB primaries with a warning (`float_primaries`). `bit_depth: 32`
  exports to `tiff`, `exr` or `hdr`, and `float_samples: "f16"` writes a half TIFF or OpenEXR (`doc_export_float`);
  OpenEXR and Radiance declare the document profile's primaries and convert no pixel. View ▸ 32-bit Exposure…
  (±20 stops, per window, never document state: `float_view`, drawn through the proof slot; `get_view` /
  `set_view` `exposure` and `exposure_displayed`, `render` `exposure`), and Image ▸ Tone Map… previews live as a
  candidate map in the same slot. The open and export plans are measured at 24 MP (`float_interchange_24mp`);
  drive gate F38. A later slice must know: a file holding ANY deep part refuses, because the `exr` crate validates
  every header and reads no deep data; a preview that writes `canvas.previewImage` is drawn at 0 stops. The v1 float work closed
  without EDR display, HDR Toning or 32-bit PSD: they are the after-v1 issue its closing batch drafted
  ([v1-float-editing.md](v1-float-editing.md), "After v1").
- `DONE` (v1.x) — EDR display output: View ▸ HDR Preview, per window, on an EDR screen (rule X, `float_view::display_extended`; [v1x-hdr-photos.md](v1x-hdr-photos.md)). `TRACKED` — luminance/ACES tone maps → plan §11 gate (3).
- `DONE` (v1.x) — the 32-bit PSD reader and writer (`Lr32`, `psd_import_float` / `psd_export_float`); the Phase A PSD
  refusal is gone ([v1-float-editing.md](v1-float-editing.md)).
- `DONE` — Fill… / Stroke… on the Red, Green, Blue and
  Alpha planes at 32-bit float: `paint_blend`'s `plane` (`float_paint`, `# Admission`; gate F32). Receipt
  `plane-fill-431/take1` (`scripts/photoshop-probe/plane_fill.py`, B431): the value is the ink's 30/59/11 luma over
  its components, raw (`3059x` / `raw`; `plane_swatch::plane_fill_value`); Normal, Multiply, Darken, Lighten,
  Difference, Linear Dodge, Subtract (`scalar`) and Dissolve (`dither`); Darker Color and Lighter Color refuse by
  name there (unmeasured: the take's Fill threw on them). Decisions: opacity stays exact (Photoshop's is a
  byte); `t` is not scaled by the pixel's stored alpha (SOFT `written`; a stored-alpha-0 pixel takes the
  write too, unreadable in Photoshop's TIFFs); linear ink on a plane writes the luma of the linear numbers
  (H-LIN); an Alpha target is clamped to 0…1. The plan is the layer fill's figure, P32 pinned in
  `selected_color_planes_24mp.rs`.
- `DONE` — above-white Fill / Stroke ink at 32-bit float:
  `color_space` `linear` on `paint_blend` and `stroke_layer_edge`, RGB 0…65504 used as is (receipt
  `hdr-colour-317-608/take2`, FG: fill = float); an above-white foreground and MCP's `rgba32` take it, recorded as
  the step's `rgba32`; a mask, alpha channel and Quick Mask clip the colour to 0…1 (the Brush and Gradient take
  it too, below).
- `DONE` — the colour-channel gray at 8 and 16 bits and the
  vacated value. Receipt `plane-gray-431-633/take1` (`scripts/photoshop-probe/plane_gray.py`, rows F8, S8, F16, S16,
  H): `fill8` `3059r`, `fill16` `3059x`, both strokes `band`, `soft8` / `soft16` `written` (32 pixels each),
  `hole8` / `hole16` / `hole32` `3059r`, `form32` `raw`. Edit ▸ Fill… / Stroke… and `fill_selection` /
  `stroke_selection` on a Red, Green, Blue or Alpha plane write the colour's 30/59/11 luma: rounded to a code at
  8 bits (`plane_swatch::plane_ink_u8`, `PlaneAlgebra.fillColor`), unrounded at 16 (`plane_ink_u16`,
  `PrecisionScalarOperations.planeInk`), not weighted by the pixel's alpha; a moved colour channel leaves that
  8-bit code over 255 at every depth (`color_plane_hole`; the plane-gray take2's `bg.601f` agreed only on its own three
  pairs). Gates F30 (`#0000ff`: 28, not 29) and F32 rows i, i-undo, j. Decisions: a mask and a saved alpha channel
  keep their Rec. 709 gray; an Alpha plane takes the plane gray, as at 32 bits. Unmeasured, unchanged: the bucket,
  gradient and brush on a colour plane, Quick Mask fill and `modify_scalar` (Rec. 709); a rounding tie of `3059r`
  (built half up); Dissolve and Stroke over partly transparent pixels.
- `DONE` — in a 32-bit float document the options bar's
  Text colour and Shape fill / stroke swatches open the Color Picker sheet with its Intensity row
  (`presentSwatchPicker`; System Colors… is still one click away), so they author an above-white colour; the shape
  options store keeps each colour as an sRGB hex plus stops (`ShapeToolOptions.fillIntensity` / `strokeIntensity`),
  clipped per linear channel at 1 outside a float document. At 8 and 16 bits the swatches keep the system panel.
  No MCP twin (`app/CLAUDE.md`, view preferences); checked by the Debug build, with no automated drive.
- `DONE` v1 float phase 1 — the Brush,
  Eraser and Gradient tools and `brush_stroke` / `eraser_stroke` / `gradient` in a 32-bit float document, on a layer
  in linear light with no clamp on colour (`float_paint`'s `paint_blend`, `erase_native` and `gradient` arm,
  `float_paint_ink::FloatInk`) and on its mask or an alpha channel through the unchanged 16-bit coverage routes
  (u16 at every depth). Decisions: no options-bar intensity control (the Color Picker's Intensity row and the
  eyedropper already set an above-white foreground, which paints as its own value); above-white ink is the Brush's
  (`rgba32`) and the Gradient's two endpoints (`start_rgba32` + `end_rgba32`; in the UI the Foreground → Background
  ramp only, stored stops being sRGB hex), and one aimed at a mask, a channel or an adjustment layer's mask refuses
  by name (`FloatPaint.coverageInkRefusal`); the Gradient converts each stop to linear light and interpolates there;
  the Eraser is `alpha × (1 − coverage × opacity)` with RGB untouched bit for bit (`blend::erase_alpha`); a Red,
  Green, Blue or Alpha target refuses by name for all three (Fill… is the float plane route); a blend mode outside
  `NativeLayerBlending.floatModes` refuses before the stroke or drag starts. Retouching and Stroke Path at float,
  and what stays refused there and why: [v1-float-editing.md](v1-float-editing.md).
  The three plans are the layer fill's figure, measured in `float_paint`'s `# Brush, Eraser, Gradient` table
  and pinned in `selected_color_planes_24mp.rs` (`FLOOR_B32`, `FLOOR_E32`, `FLOOR_G32`). Oracles
  `core/tests/float_brush_tools.rs`; gate F37; jobs-probe "float paint strokes".
- `DONE` — Load Selection from the RGB composite. Float:
  the profile's Y row over UNclamped samples, clamped as a sum (`precision_selection.rs`, 03c1798), confirmed by
  `float-selection-442/take1` (`a` sRGB, `b` ROMM) and take 2's rows (the issue body); `float_source_selection.rs`'s
  `luminance_reproduces_photoshops_442_receipts` pins all 18 receipt rows. 8/16 bits (RGB) load the luminosity in
  Edit ▸ Color Settings' Gray working space (`ColorSettings.workingGray`, seven spaces, Dot Gain 20 % by
  default): `Y^e`, Y the profile's relative PCS Y (`cmyk_color::document_xyz`), Dot Gain d % `e = ln 0.5 /
  ln(0.5 − d/100)`, Gray Gamma γ `1/γ` (`gray_working.rs`, `dr_doc_composite_gray_plane`, the native `luminance`
  kind's `gray`). `gray-luminance/take1` (`gray_luminance_take.py`) read it 0 codes off on 26 patches at 8 and 16
  bits under Dot Gain 20 %, Gray Gamma 2.2 and Dot Gain 15 %; `gray_working_selection.rs` pins the take. Only Load
  Selection (sheet, ⌘-click, MCP `load_selection`, `add_channel from: plane`) reads it; every other luma stays
  Rec. 709.
- `DONE` — Divide at 32-bit float, layer, Fill and effect, by
  rule B: `clamp(cb / max(cs, 1/65504), ±65504)` (Photoshop's 2^-39 stand-in writes values a float document
  cannot store). Linear Burn, Linear Light and Pin Light ship with it; Screen, Overlay, Soft Light, Hard Light, Color
  Dodge, Color Burn, Vivid Light, Hard Mix and Exclusion stay refused by name (each inverts about white or divides by
  1 − v).
- `OPEN` — Behind and Clear for Fill/Stroke, which
  Doctored has at no depth (not a float-only gap).
- `DONE` — File ▸ New at 32 bits
  and Image ▸ Mode ▸ 32 Bits/Channel… / `convert_precision mode: rgba32` (preserve only): an 8- or 16-bit document
  becomes a float document with every record carried, each code decoded through the profile's TRC to linear light
  (`precision_conversion_float`); the round trip back is exact; what float cannot hold, a result too large to save
  and reopen at 32 bits included, blocks by name. Rule, refusals and admission: [v1-float-editing.md](v1-float-editing.md).
- `REVERSED` — promotion of u16/byte to float, once dropped by decision: reversed by the user on 2026-10-12, because a
  16-bit picture or a RAW development must be able to join a float composite, and a user who wants to paint with
  light needs a float document (shipped above). `DROPPED` — `PrecisionStateArchive.swift` float coverage (out of
  scope; internal probe archive only).

## Shipped runs
[phase-11f-hdr-float-shipped.md](phase-11f-hdr-float-shipped.md) — Decisions, Rejected alternatives, Consequences and every `DONE` item, moved there verbatim.
[v1-float-editing.md](v1-float-editing.md) — what the v1 float editing batches shipped after that companion reached its cap, starting with the eleven float filters (phase 2), and retouching and Stroke Path (the float half of phase 5).
