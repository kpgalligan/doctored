# v1 float editing (ADR)

- **Status:** Accepted, 2026-10-12. The working record is [archive/v1-float-editing.md](../archive/v1-float-editing.md).
- **Contract:** [phase-11f-hdr-float-status.md](phase-11f-hdr-float-status.md) and its shipped companion still hold; this ADR adds the v1 batches that made 32-bit float a full editing depth.

## Context

Phase 11F shipped a float document that opened, rendered, tone-mapped, converted and saved, but edited little: no painting, no filters, no retouching, no smart selections. The v1 batches (phases 1–5 and decision rows 1 and 3 of the working record) widened the admitted set one area at a time. Each operator runs under one stated rule with an independent oracle that includes values above 1.0, or is refused by name with a reason that says why it has no meaning above 1.0.

## Decisions in force

- **One arithmetic at every depth.** A float operator runs the kernel the 8- and 16-bit path runs, asked for `SampleRange::Extended` (`core/src/filter_kernels.rs`). `Unit` keeps every earlier expression, so 8- and 16-bit output is bit-identical.
- **Linear light, no colour clamp.** Samples are linear light and are edited as stored. Colour is never clamped to 0…1; alpha stays in 0…1; an output alpha below 2^-24 is transparent black; a non-finite sample or one outside ±65504 refuses and is never repaired.
- **An approximating kernel is measured first.** On a 0.1 background holding one 5000 pixel, and a 5000 disc of radius 6, `|out − ref| ≤ 2 %` of `ref` at every pixel (`core/tests/float_filters_highlight.rs`). A kernel that misses runs its exact path at float. So the Gaussian at float is the FIR at every σ (`sampling_gaussian::runs_fast`), and Motion and Radial Blur place samples in binary64 (`sampling_radial::sample_exact`).
- **The D3 clamp is for resamplers.** Lens Correction and Liquify (Catmull-Rom) clamp each resampled colour to the range the layer's visible pixels hold, never to 0…1. Every other float filter is a non-negative mean or an exact move.
- **Parameters with a display meaning in linear light**: High Pass writes `0.5 + source − low` (the stored 0.5 pivot at every depth); Unsharp Mask's threshold compares linear differences; Lens Blur's highlight weight reads linear luma; Lens Correction's vignette multiplies linear RGB by `2^stops`.
- **One display encoding.** `core/src/display_encoding.rs` holds the ONE in-range encode E (the profile's forward curve through the float policy's encoder) and decode D, and every rule below is stated there or in the kernel module named. Neither the tone map nor the view exposure is ever read.
  - **Display-referred filters** run on an encoded copy, E continued above white as `1 + s·ln v` (C1 at white, odd for negatives), and only the components the kernel changed are decoded back; the coverage mix stays in linear light.
  - **Display-defined tools** clamp to 0…1 first: a value above white compares as white.
- **A selected edit's coverage is charged at 5/4** (`float_adjust_apply::coverage_scratch`).
- **Measured admission.** Every new float plan is at least ⌈1.25 × the measured footprint⌉ (Release, one case per process, `MallocLargeCache=0`, three runs and a twin). The tables live in each module's doc; `float_adjust_apply_24mp.rs` and `selected_color_planes_24mp.rs` pin the floors.

## The rules

### Filters (`float_filter_route.rs`, `float_filter_apply.rs`)

- **Linear light:** Gaussian, Box, Motion, Radial, Lens Blur, Tilt-Shift, Offset, Lens Correction, Liquify, Unsharp Mask, High Pass, Pixelate (premultiplied cell mean) and Add Noise (`v·(1 + δ)`, the 16-bit draw).
- **On the encoded copy:** Surface Blur (its histogram spans `[min(0, lo), max(1, hi)]` of the copy), Dust & Scratches, Reduce Noise, Add Grain, Clouds, Difference Clouds, Emboss and Edge Detect (finished inside the copy's own range), legacy Sharpen, Presence, Smart Sharpen and the Develop Filter's display stages, and Red Eye.
- **S, Smart Sharpen** (`sampling_smart_sharpen`): the whole filter on the encoded copy, as legacy Sharpen and Reduce Noise. Reduce Noise's level, Lens Blur's 2/255 overshoot and the tonal widths mean their 16-bit values inside 0…1; the fade classifier reads the encoded luma clamped to 0…1 (above white is `L = 1`); nothing is clamped, so a value above white is sharpened in code and decoded back. Its Gaussian is the float FIR at every σ, so Gaussian removal at noise 0 with no fade is legacy Sharpen's float route bit for bit, not the 16-bit result encoded.
- **D, Dehaze** (`sampling_dehaze`; Presence and the Develop Filter): on the encoded copy. Airlight and veil are measured with RGB clamped to 0…1 (`unit_rgb`), so `A ≤ 1` and an above-white highlight cannot drag the airlight. Positive amount: a component `I ≤ 1` takes the 16-bit `clamp((I − A)/t + A, min(I, 0), 1)`, a component `I > 1` is returned as read. Negative amount: the 16-bit mix toward `A`, unclamped. No component moves above `max(I, 1)`. Linear-light application was rejected: a veil measured on encoded values overstates the linear veil below the airlight.
- **H, Highlights and Shadows** (`ShadowsHighlights::apply_float`, Develop Filter stage 4 on the copy): the guide as at 16 bits; a pixel whose luma is at or above 1 is returned unchanged (`L^e` fixes white); any other takes the 16-bit expression without its final clamp.
- **The Develop Filter** (`sampling_develop_filter`, `DevelopFilter::split_float`): Temperature, Tint (`WhiteBalance::apply_linear`) and Exposure (`v·2^e`) on the stored values; Contrast, Highlights, Shadows, Whites, Blacks (`Levels::apply_float`), Texture, Clarity and Dehaze on the encoded copy; then **V**, Vibrance and Saturation, on the decoded linear value through the Scaled form below with the 16-bit `Vibrance` master (`Route::back`), the form Image ▸ Adjustments ▸ Vibrance takes at float.

### Filters on a mask, alpha channel or colour plane (`float_plane_filter.rs`)

Every float filter but Liquify, Depth Blur and the Develop Filter runs on a layer mask, a saved alpha channel or one Red/Green/Blue/Alpha plane of a float layer (Filter menu, `apply_filter` with a `target`, Actions), through `Route` on a one-channel plane:

- **Read.** The plane is read as an opaque gray pixel `[v, v, v, 1]`, at stored alpha 0 too (the 16-bit adapter's rule).
- **Colour (R/G/B of a float layer).** `v` is the stored linear component. The route's own `Domain` applies: Linear reads `v`; Display encodes the gray triple (`DisplayEncoding::encode`), runs the kernel on the copy and `decode_changed`s it back. The range is `SampleRange::Extended { visible }`, where `visible` is `visible_range` over the read triples (or over the encoded copy). Nothing is clamped.
- **Coverage (mask, alpha channel, the layer's Alpha).** `v = code/65535`, or the stored alpha. It is never encoded in either domain, because coverage is not light (R4m). The range is Extended over the plane's own visible range, and the result is clamped to 0…1.
- **Result.** If `r == g == b` bit for bit, the result is `r`. Otherwise it is the Rec. 709 luma `LUMA_R·r + LUMA_G·g + LUMA_B·b` (`blend.rs`), computed in f64 and rounded once to f32.
- **Write.** Weight 0 keeps the stored bits; weight 65535 takes the result. Between those it is `v0·(1−t) + v1·t` in f64, rounded once (a mask or channel through the existing u16 walk). A non-finite result refuses with `float_filter_apply`'s sentence. A colour result outside ±65504 refuses through `rebuilt_rgba32_extended`. No alpha floor. An unchanged plane is `None`.
- **Frame.** Radial Blur and Offset take their frame from the selection's bounding box on the plane (`ops_spatial::coverage_frame`), as in the existing walk.
- **Add Noise on coverage.** On a Coverage plane, Add Noise takes the 16-bit mask's rule, not the route's light-relative `v·(1 + δ)`: the additive, clamped `SelectedFilter::Noise` draw. A mask or channel calls `precision_scalar_filter::evaluate` itself (the 16-bit document's mask result exactly). The float Alpha runs `SelectedFilter::evaluate` on `[v, v, v, 1]` in index order with the request's seed. A Colour plane keeps the route's Add Noise.
- Liquify, Depth Blur and the Develop Filter refuse by name on every plane, before the route parses (so an identity request or Develop Filter masks never answer first). The plan of a float R/G/B/A plane replaces the u16 plane term with the route's two stages as a max, so it is at most the whole layer's plan; the measured table is in the module doc.

### Display-defined tools and selections (`display_encoding.rs`)

- **R1, the display copy:** RGB `round(65535·E(clamp(v, 0, 1)))`, alpha `unit_to_u16`.
- **R2, comparisons:** the Magic Wand, Paint Bucket, Grow and Similar compare 16-bit codes on R1; Color Range and Quick Selection their Lab ΔE76 on R1. Output is native u16 coverage.
- **E, the edge stage:** Select and Mask's `refine_edge` (edge radius, smart radius, the Refine Edge brush) reads R1 as its guide, output native u16. It covers Select and Mask, Quick Selection's Enhance Edge and Object Selection's refinement. Object Selection's refinement is opt-in at float (`ObjectSelection.refines` is the default; `object_select refine: true` runs it): Vision found the object on the document's own tone map, and on R1 an above-white subject on an above-white background is flat white.
- **M, the Magnetic Lasso:** the trace image is the composite through Clip at exposure 0 at 8 bits (`ImageDocument.edgeTraceComposite`), never the document's tone map or the window's exposure.
- **C, Select and Mask's New Layer outputs** (`precision_cutout`): the float composite is the foreground, every bit kept, the u16 matte its mask; New Layer bakes it through the float Apply Layer Mask.
- **R3, the bucket:** the R1 region filled in linear light through the float paint walk; `rgba32` or above-white ink as given.
- **R4, Dodge and Burn on a float layer:** in 0…1 `t = D(shaped(E(v)))`, above 1 `t = v·D(shaped(1))`, below 0 `t = v + D(shaped(0))`; out `v + c·(t − v)`, held to ±65504. **R4m**, on a mask or alpha channel at every native depth: `65535·shaped(s/65535)`.
- **R5, the Sponge:** `retouch_brush::targets_in` at `Extended`, unclamped.
- **Vision selections** (Subject, Objects, People, Face Parts, the Subject and Object Selection tools) read the document's own tone map at exposure 0 (`analysisComposite`).

### Blend modes and adjustments

- **B:** the float layer set (`NativeLayerBlending.floatModes`: the kernels with no clamp of their own and the nine measured in 11F), plus Linear Burn `cb + cs − 1`, Linear Light `cb + 2cs − 1`, Pin Light and Divide `clamp(cb / max(cs, 1/65504), ±65504)`, on layers, ink and effects.
- **A, adjustments** (layer and destructive apply, equal): the 11F fitted forms; Hue Rotate and Sepia their matrix and Grayscale the Rec. 709 luma on linear values; Threshold, Posterize and Gradient Map **Mapped** `D(master(E(clamp(v, 0, 1))))`; Vibrance, Selective Color, Replace Color and every Hue/Saturation setting with no closed form **Scaled** `m·(D(master(E(max(c, 0)))) + min(c, 0))`, `m = max(1, max rgb)`; every Color Lookup table; the `document_v2` White Balance.

### Painting, retouching and conversion

- The Brush, Eraser and Gradient paint in linear light (`float_paint`); Fill… and Stroke… on a layer, mask, alpha channel or colour plane.
- The Clone Stamp copies in linear light; heal, spot heal, patch, Remove, Content-Aware Fill and Move solve in the working encoding W (`retouch_encoding`) and decode once where a code changed; the Blur, Sharpen, Smudge and History brushes run the masters' kernels unclamped (`float_retouch`, `float_retouch_brush`).
- New Document at 32 bits and 8/16 → 32-bit float: each u16 code becomes the profile's TRC decode; narrowing back returns every code (`precision_conversion_float`).

### Levels and Curves at float

- Levels and Curves are 32-bit adjustments, both with the ⌥-drag clipping display.
- Float Levels and Curves act on stored linear values with the 0…255 handles at linear 0…1, so the sheet plot is binned on that axis (`float_bins: "unit"`, `round(clamp(v, 0, 1) · 255)`), not through the display encode, which would put each handle under other tones.
- Info and the MCP `histogram` keep stop bins.
- ⌥ shows the edited composite's channels `>= 1.0` (Highlights) and `<= 0.0` (Shadows), inclusive because float Curves holds an inside black end point flat at exactly 0.0; the After plot shows the unit bins of the preview's projection.
- The plot's clipping wedges count the same inclusive rule: each is the largest RGB channel's `exact_zero + below_zero` (or `exact_one + above_white`) weight, the float histogram's `exact_one_*` keys holding `v == 1.0`.

## Still refused at float, by name

The operator table (`float_document.rs`, `get_precision` / `get_tone_map` `operators.refused`) holds nine rows, each with its reason:

- The nine blend modes that invert about white (Screen, Overlay, Soft Light, Hard Light, Color Dodge, Color Burn, Vivid Light, Hard Mix, Exclusion): each formula inverts about white or divides by `1 − v`, so above 1.0 it has no meaning.
- Invert: `1 − v` inverts about white, so above 1.0 it has no meaning (8.0 would become −7.0).
- The Shadows/Highlights adjustment: its luma curve `L^e` is defined on 0 to 1 and reverses above white, and its Color and Midtone Contrast pivot about 0 to 1 midtones; the Develop Filter's Highlights and Shadows run, held at white.
- The legacy White Balance: its matrix acts through the sRGB curve on 0 to 1 numbers, so a value above 1.0 has no encoded form; the `document_v2` White Balance is admitted.
- Assign or Convert to Profile with a profile that has no matrix/TRC model.
- Convert to Profile with absolute intent, black-point compensation or preserve-appearance adjustments.
- Apply Image: its blend runs on u16 planes clamped to 0 to 1.
- Select and Mask's Decontaminate Colors: its foreground solve reads and writes RGBA16 codes clamped to 0 to 1, so a fringe above white would be clipped. The sheet disables it; `refine_selection decontaminate: true` refuses before any job.
- Auto Tone, Auto Contrast, Auto Color and the Levels dialog's Auto buttons: each reads its black and white points off the clipped ends of a 0 to 1 histogram. The menu items and the Levels buttons are disabled; `auto_tone`, `auto_contrast`, `auto_color` and a Levels `auto` refuse with the row's reason.

Not built: a proxy preview stage at float; RAW Develop's own Presence at float; Image ▸ Adjustments, Auto and Invert on a float mask, channel or colour plane; filters on the Quick Mask at float beyond Gaussian Blur.

## After v1

What v1 left for float documents (EDR display, HDR Toning and 32-bit PSD):

- **EDR (HDR) display.** Shipped in v1.x: View ▸ HDR Preview shows values above 1.0 brighter than white on an EDR screen ([v1x-hdr-photos.md](v1x-hdr-photos.md)); HDR photos open as float documents there too.
- **HDR Toning.** Image ▸ Adjustments ▸ HDR Toning… (and 8/16 Bits/Channel on a float document) flattens and tone maps with Equalize Histogram, Exposure and Gamma and Highlight Compression since phase 4a of merge and stacks, and with Local Adaptation, the local tone map and the dialog's default, since its phase 4b ([v1-stacks-status.md](v1-stacks-status.md)); measured against Photoshop: Exposure and Gamma at (0, 1) matches, and every other method and value misses the 2 / 0.5 gate and ships as declared with its numbers.
- **32-bit PSD read and write.** A float document saves as a 32-bit RGB PSD or PSB (`psd_export_float`: Photoshop's form — an empty layer section, an empty `Mt32`, the records in `Lr32` with a positive count, its 32-bit color-mode data; linear-light f32 samples under the profile's linear (gamma 1.0) twin, f32 masks, Raw channels, the composite matted over white; Photoshop opens it, take2 of `scripts/photoshop-probe/psd32_653_take.py`) and a 32-bit RGB PSD or PSB opens as a float document with either choice (`psd_import_float`, `import_precision` `rgba32`). In the layered mode a gradient or pattern fill, a style that would be rasterized or an adjustment with no Photoshop record refuse by name (a Solid Color fill is written as `SoCo`, below). A file whose profile is the linear twin of one of the four built-ins (its name plus " (Linear RGB Profile)" and that built-in's matrix over identity curves) reopens under the built-in, so a later 8- or 16-bit export is tagged and encoded as before the save; any other profile stays the document's. Photoshop's own file is a fixture (`psd_photoshop/depth32`, take1). Photoshop opens both forms: take2 the PSD, and `scripts/photoshop-probe/takes_675_679.py` take1 the PSB (`rz32.psb` at 32 bits, its six rows as declared, every per-layer sample within take2's bound).
- **32-bit layered TIFF.** A float RGB document's File ▸ Export as PSD… writes a TIFF with layers at 32 bits in Photoshop's own `layered32.tif` form, big-endian (`psd_export_float` through `tiff_layers::write_with`, one buffer): the same records as the PSD in an `Lr32` block in tag 37724 with a positive count and no `Mt32`, Raw f32 channels, an IEEE-float flat image (straight RGBA, not matted) and the profile's linear twin; a 32-bit Grayscale document's layered TIFF still refuses by name. The layered open reads it and Photoshop's own little-endian (IBM PC) form, whose codes are byte-reversed (`MIB8`, `23rL`): `tiff_layers_order` re-lays such a file big-endian for the existing reader, walking every record field by field and refusing by name what it does not carry (descriptor blocks such as `lfx2`, fills and adjustments; a Raw 32-bit channel). Photoshop opens both: `scripts/photoshop-probe/tiff32_679_take.py` take1 (Doctored's `rz32.tiff` and its little-endian copy from the `layered_tiff_little_endian` seam, each at 32 bits, its six rows as declared, every per-layer sample within take2's bound). `tiff_layers32.rs` holds the round trip in both orders and the field-by-field comparison with Photoshop's file; `psd_photoshop_float32_679.rs` opens that file to `photoshop.json`'s rows.
- **32-bit fills and style colours.** At 32 bits a descriptor colour is the document's LINEAR light × 255 (`fill_style32.psd`'s `SoCo` 40.0000014 = f32(40/255) · 255). The layered 32-bit PSD, PSB and TIFF write every style and fill colour that way (`FxColours::Float`, `style_float::FloatStyle::descriptor_numbers`), and a Solid Color fill as Photoshop's pixel-less 32-bit `SoCo` (rect 0, empty Raw channels, its `-2` mask, flags `0x18`; the 16-bit record walk at depth 32). The 32-bit open carries a `SoCo` back as a solid fill layer re-rendered from its description (`psd_fill_layer::render_float`); a built-in's linear twin crosses its colours as that built-in's own twin, the relabel its samples take. Photoshop has shown no 32-bit `GdFl` or `PtFl`, so a gradient or pattern fill refuses by name both ways, as does a style that would bake; the little-endian TIFF form refuses `SoCo` and `lfx2` by name. `psd_float_fill_style.rs` holds the round trips and refusals; `psd_photoshop_float32_679.rs` opens `fill_style32.psd` to `photoshop.json`'s rows and compares Doctored's re-save with its `Lr32` field by field. Photoshop opens all three: `scripts/photoshop-probe/fill_style32_679_take.py` take1 (Doctored's `rz32.psd`, `rz32.psb` and `rz32.tif`, each at 32 bits, its three rows as declared, the fill's `solidColorLayer` and the Drop Shadow's colour within 1 of 255 L(c), every per-layer sample within take2's bound).

## Checks

- Core: `float_filters*.rs`, `float_paint*.rs`, `float_retouch*.rs`, `float_smart_selection.rs` (rule E against a 16-bit twin of R1 codes; a 2.0 | 8.0 edge refines as flat white), `float_filters_develop.rs` (rules D, H and V against f64 restatements and 16-bit twins), `precision_cutout.rs` (rule C), `precision_edge_refine.rs` (Decontaminate refuses), `float_display_adjustments.rs`, `float_promote*.rs`, `psd_depth32_profile.rs` (a reopened twin exports 8 bits under its gamma source, against the published curves), `float_scoped_statistics.rs` (unit bins against an f64 oracle and a u16 twin), `clip_display.rs` (the float `>= 1.0` / `<= 0.0` display, a float Curves black point).
- Plane filters: `float_plane_filters.rs` (a colour plane against the layer route on a gray twin, bit for bit; coverage the clamped twin; coverage Add Noise and the display filters against a 16-bit mask; Pixelate, Offset, the gate, the refusals), and `scripts/verify_v1_float_planes.py` (RED, ALPHA, MASK, CHANNEL, REFUSED, ACTIONS).
- MCP drive: `scripts/verify_phase11f_float.py` gates F37–F47; F47 holds the closing batch (the Magnetic Lasso, Enhance Edge and Select and Mask against a 16-bit twin, the New Layer outputs, Dehaze, Highlights, Shadows, Vibrance and Saturation, and the nine refused rows, Auto Tone and the Levels Auto refusing).
- Admission: the 24 MP examples under `core/examples/float_*_admission.rs` and `float_adjust_apply_24mp.rs`'s `measure`.
