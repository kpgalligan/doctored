# v1 float editing (status)

- **Status:** open. The working record of the v1 batches that widen what a 32-bit float document edits, after [phase-11f-hdr-float-shipped.md](phase-11f-hdr-float-shipped.md) reached its size cap.
- **Contract:** [phase-11f-hdr-float-status.md](phase-11f-hdr-float-status.md) and its shipped companion. Everything there still holds; this doc adds to it.

## Decisions in force

- **One arithmetic at every depth.** A float filter runs the kernel the 8- and 16-bit filter runs, asked for `SampleRange::Extended` (`core/src/filter_kernels.rs`). `Unit` keeps every expression the kernels had, so 8- and 16-bit output is bit-identical.
- **Linear light, no colour clamp.** A float layer's samples are linear light and a filter works on them as stored. Colour is never clamped to 0…1; alpha stays in 0…1; an output alpha below 2^-24 (`sampling::EXTENDED_ALPHA_FLOOR`) is transparent black; a non-finite sample or one outside ±65504 refuses and is never repaired.
- **A kernel that approximates is measured before a float layer may run it.** An approximation's error is a fraction of the brightest sample it reads. A 0…1 clamp hides that; a float picture does not. The bound, declared before measuring: on a 0.1 background holding one 5000 pixel, and separately a 5000 disc of radius 6, `|out − ref| ≤ 2 %` of `ref` at every pixel, `ref` the filter's own definition in f64. `core/tests/float_filters_highlight.rs` holds every float kernel to it and prints what it measures (`cargo test --manifest-path core/Cargo.toml --release --test suite_color_formats float_filters_highlight -- --nocapture`). A kernel that misses runs its exact path at float or stays refused; the bound is not widened.
- **The Gaussian at float is the FIR at every σ.** The recursive Gaussian 8 and 16 bits run from σ 4 up missed the bound at σ 8 and σ 32 (the measured table is on `sampling_gaussian::runs_fast`), so Gaussian Blur, Unsharp Mask and High Pass on a float layer run the windowed FIR, cut where its dropped tails are under 1e-7 of the weight (about 5.33σ a side). Its work grows with σ; it is a cancellable job with progress and no work ceiling.
- **A float layer's path samples are placed in binary64.** Motion and Radial Blur place and weigh each bilinear sample in f64 at float (`sampling_radial::sample_exact`); 8 and 16 bits keep the f32 body.
- **The D3 clamp is for resamplers.** Lens Correction and Liquify resample with Catmull-Rom, whose negative lobes ring, so each resampled colour is clamped to the range the layer's visible pixels hold (never to 0…1). Every other float filter is a mean with non-negative weights, or an exact move, and cannot leave that range; the means finish through the same clamp, which there only absorbs f32 rounding (a quotient one ulp past ±65504 would otherwise refuse).
- **Rules for the parameters with a display meaning**, each stated where it is computed:
  - High Pass writes `0.5 + source − low`, unclamped. The pivot is the stored value 0.5 at every depth (linear 0.5 at float, lighter than mid-grey on screen), so `source = low + (high − 0.5)` holds with one constant whatever the depth.
  - Unsharp Mask's threshold compares linear differences.
  - Lens Blur's highlight weight keeps its formula on the stored linear luma with `t = threshold / 255`, so a pixel above white takes the full weight at any threshold.
  - Lens Correction's vignette multiplies linear RGB by `2^stops`, with no transfer round trip and no clamp.
- **A float filter edits a raster layer's own pixels.** The menu items validate on a layer target with Quick Mask off; `apply_filter` with another `target` refuses. The one exception is older: in Quick Mask, Gaussian Blur is the Quick Mask blur at 16 and 32 bits.
- **A display-referred parameter reads the display encoding.** One adapter, `core/src/display_encoding.rs` (`E`), serves every filter whose parameter is defined on display values, and the float Red Eye: the profile's own forward curve (the encoder the float policy holds) on 0…1, continued above white as `1 + s·ln v` with `s` the curve's slope at white (C1), odd for negatives; the decode is the profile's curve and `exp((f − 1)/s)`, held to ±65504. The kernel runs on an encoded copy and only the components it changed are decoded back; the coverage mix stays in linear light. Extending the curve, not scaling by the layer's headroom, keeps every value inside 0…1 at its 16-bit code, so a threshold means the same at both depths and never depends on the layer's brightest pixel; the log spends equal code on equal light ratios (sRGB `E(65504)` ≈ 5.9). Surface Blur alone maps its 65 536-code histogram over the encoded copy's own range, because only it quantizes. It is not `retouch_encoding`'s W (sRGB-only, scaled to ±65535 for patch distances).
- **A selected edit's coverage is charged at 5/4.** The canvas-sized coverage the host builds after the plan was charged once, with no margin; measured inside the interval it missed the 1.25 rule by 4 to 8 MB at 24 MP. It scales with the canvas, not the layer, so the fix is on that term (`float_adjust_apply::coverage_scratch`), for float adjustments and float filters alike.

## Shipped

### Eleven filters on a float layer (phase 2, first half; 2026-10-12)

Gaussian Blur, Box Blur, Motion Blur, Radial Blur, Lens Blur, Tilt-Shift, Offset, Lens Correction, Liquify, Unsharp Mask and High Pass run on a raster layer of a 32-bit float document, confined by the selection, as cancellable jobs with their previews, from the Filter menu and over MCP (`apply_filter` for ten, `liquify`; replies carry `storage: "rgba32_float"`; `get_precision` lists them under `filters`). Repeat Last Filter repeats one.

- Core: `float_filter_apply.rs` is the adapter (plan, run, coverage mix); the operator table's `<op>_filter` rows are the admitted set (`float_document::admits_filter`). No new export and no `.rz` change.
- The writeback: coverage 0 copies the pixel bit for bit, full coverage takes the filter's sample, and between them a sample whose alpha is unchanged is the straight lerp and any other the premultiplied mix.
- The Liquify workspace previews on the layer through the document's tone map (the Layers panel's thumbnail); Apply moves the layer's linear samples.

What the highlight fixture decided (the test prints every measured error; the tables that chose a path are in the source comments named here):

- The recursive Gaussian missed the 2 % bound at σ 8 and σ 32, so float runs the FIR, which meets it at σ 2, 8, 32 and 64 (`sampling_gaussian::runs_fast`). σ 64 is the largest σ the filters take; 128 refuses at every depth.
- Radial Blur with f32 sample positions missed it and left an opaque picture just under alpha 1 in places, so float places samples in f64, where alpha stays exactly 1. Motion Blur met the bound and takes the f64 placement too (`sampling_radial::sample_exact`).
- Box Blur, Motion Blur, Radial Blur, Lens Blur and Tilt-Shift as shipped meet it.

The FIR's work grows with σ: seconds at small σ and over a minute at σ 64 on a 24 MP float layer (the timings are on `sampling_gaussian::half_width`).

Admission at 24 MP: every filter's plan is at least 1.25 times its measured footprint, under a canvas coverage too. `float_filter_apply.rs` holds the table and `float_adjust_apply_24mp.rs` pins the floors.

### The display-referred filters, Presence, Develop Filter and Red Eye (phase 2, second half; 2026-10-12)

Thirteen more filters run on a raster layer of a float document, confined by the selection, recorded, one undo step, cancellable, from the Filter menu, the sheet previews and `apply_filter`; Red Eye runs from the tool, Filter ▸ Remove Red Eye, `red_eye` and `red_eye_auto` (`.floatTool`). `get_precision` lists all twenty-four filters. No new export, no header or `.rz` change. Each runs under one rule, stated in its kernel module:

- **On the display encoding** (encoded copy, decoded back): Surface Blur (the one headroom map: its histogram spans `[min(0, lo), max(1, hi)]` of the encoded copy, the tent's reach divided by that span); Dust & Scratches; Reduce Noise (its luminance model and mix unclamped); Add Grain (`4Y(1 − Y)` on the encoded luma clamped to 0…1, so a pixel whose luma is at or above white gets none; the sum unclamped); Clouds and Difference Clouds (the hex colours mixed in encoded units, as at 16 bits); Emboss and Edge Detect (finished with `SampleRange::display_bounded`, inside `[min(0, lo_c), max(1, hi_c)]` of the encoded copy, so an SDR layer stays in 0…1 as at 16 bits); legacy Sharpen (Unsharp Mask amount 1, threshold 0 at σ = min(amount, 5) on the encoding with the FIR Gaussian, its own route arm so a user's `unsharp_mask` stays linear); Presence's Texture and Clarity (unclamped bases); Red Eye (the red test reads the encoded rect clamped to 0…1, the correction is applied unclamped).
- **In linear light:** Add Noise `v·(1 + δ)` with the 16-bit draw δ, drawn for every pixel in index order before the coverage gate (black stays black, highlights get proportional grain); Pixelate, the premultiplied mean of the cell's light.
- **Develop Filter:** Temperature/Tint (`WhiteBalance::apply_linear`) and Exposure (`v·2^e`) on the stored values; then the encoded copy for Contrast, Whites and Blacks (`Levels::apply_float`), Texture and Clarity. A request with no display part runs the linear stages alone.
- **Refused by name**, at the plan, before allocating, in this order: the Develop Filter's Highlights and Shadows (`L^e` on 0…1 luma reverses above white; Shadows/Highlights is unavailable at 32 bits), Dehaze on Presence and the Develop Filter (a 0…1 dark channel and airlight, clamped), the Develop Filter's Vibrance and Saturation (HSL on 0…1, clamped). The sheets hide Presence's Dehaze and disable the Develop Filter rows at float (a preset loads them as 0 and says so).

Core: `float_filter_route.rs` holds the route (parse, domain, plan, plane, combine) and `PrecisionDocument::display_encoding`; `float_filter_apply.rs` runs both domains. 8- and 16-bit output is bit-identical (each kernel's `Unit` arm is the expression it replaced). Admission at 24 MP: a display-domain filter holds the n×16 encoded copy beside the apply stage (E 48 bytes a pixel) and plans it at 5/4; the thirteen measured plans are at least 1.25 times their footprints (the table is in `float_filter_apply.rs`, the floors in `float_adjust_apply_24mp.rs`); the Develop Filter with Texture and Clarity plans past the 2 GiB test pin and was measured at 8 GiB. Red Eye's whole-canvas case is in `float_retouch.rs`'s table (`FLOOR_RE32`).

Checks: `core/tests/float_filters_display.rs` and `float_filters_develop.rs` (16-bit twins built from the same codes, f64 oracles restating the profile's IEC table and the log above white, the Relief bounds, the refusals); gate F45 of `scripts/verify_phase11f_float.py`; the jobs-probe case "float filters".

### Still refused at float

The refused adjustments and blend modes and Select and Mask's edge stage (the operator table); the Paint Bucket, Dodge, Burn, the Sponge and the selections that compare pixel values run since the display-defined tools below. Dehaze, and the Develop Filter's Highlights, Shadows, Vibrance and Saturation, by name (above).

Not built: a float filter on a mask, an alpha channel or a colour plane from the Filter menu; Auto Tone / Contrast / Color at float; a proxy preview stage at float.

### With the same batch

- A float export's preview is admitted before it decodes: the open's plan for the written bytes plus the display's (`doc_export_float.rs`, "The preview", which holds the 24 MP record; the open's plan alone missed on Radiance by 4.7 MB).
- The view exposure drops a wrong-size image before it asks for the projection, and a failed exposure render clears the slot instead of leaving the previous exposure's image up (`EditorViewController+ViewExposure.swift`).

### Retouching and Stroke Path on a float layer (the float half of phase 5; 2026-10-12)

Builds on the float paint tools and [v1-paths.md](v1-paths.md).

#### Context
A 32-bit float document had the Brush, Eraser and Gradient but no retouching: the typed retouch dispatcher
read u16 buffers only, three display previews refused a float plate, and the host gates kept the tools off.
Every 16-bit retouch gesture and MCP handler already sent one typed request from depth-agnostic Swift.

#### What shipped
- The Clone Stamp, Healing Brush, Spot Healing Brush, Patch, Remove, Content-Aware Fill and Content-Aware
  Move work on a float layer's pixels with their 16-bit options, jobs and MCP mirrors (`clone_stamp`,
  `heal_stroke`, `spot_heal_stroke`, `patch_region`, `remove_stroke`, `content_aware_fill` and its preview,
  `content_aware_move`). Values above white and below zero are kept; nothing is clamped to 0…1 and no result
  holds a NaN or an infinity.
- Stroke Path runs the float Brush or Eraser along a path. `stroke_path` takes `rgba32` (brush only), and the
  Paths panel passes it for a foreground above white on a layer.
- No new export, no signature change and no `.rz` change: the requests are the 16-bit ones, and only what a
  float document admits changed.

#### Decisions
- **The f32 arm mirrors the master.** `float_retouch.rs` is the float arm of `precision_retouch`'s dispatcher:
  the same plan, coverage domain, lock gate, rect writer and new-layer and grown-extent staging over f32. The
  plate cut, the source mapping, the component walk, the membrane, the synthesis and the blend are the shared
  implementations.
- **Clone copies in linear light.** The plate is mapped with premultiplied bilinear taps
  (`retouch_transform::transform_extended`) and composited through `blend::composite_source_*` at
  `blend_float::blend_kind_at`'s kind. No encoding and no clamp: a cloned 8.0 reads 8.0. Its modes are the
  float layer set; another mode refuses by name. A result outside the stored range (a Multiply of two large
  values) refuses, as the float Brush's does.
- **Heal, spot heal and fill work in a working encoding W.** Where a kernel compares or solves (the PatchMatch
  search and vote, the membrane, the soft-coverage and opacity mix) it runs on
  `W(v) = sign(v) · S · f(|v|)`: `f` is the sRGB curve up to 1 and `1 + (1.055 / 2.4) · ln u` above it (the
  same slope at white), and `S = 65535 / f(65504)`, so the stored range lands on the 16-bit code range the
  kernels and the membrane's residual gate were measured on (`retouch_encoding.rs` says why). The result is
  decoded once at the write, and only where a code changed, so a pixel the retouch does not change keeps its
  bits.
- **An overshoot lands at the stored bound.** The membrane writeback clamps `source + u` to ±65535 W codes, as
  the 8- and 16-bit writeback clamps to its own range, and the decode answers inside ±65504. A heal, patch,
  move, spot heal or fill whose membrane overshoots therefore lands at the bound instead of refusing. This is
  the stored range, not a clamp to 0…1.
- **Layer RGB only.** A mask, a channel, one colour channel and Quick Mask refuse a float retouch by name, as
  at 16 bits (`FloatRetouch.swift`). The healing family blends in Normal only; `clone_stamp` alone takes
  `blend_mode`.
- **Plates share the document's depth.** `rz_precision_doc_retouch_from` (Content-Aware Move) refuses plates
  of another depth.
- **Admission.** `retouch_plan` charges a float document twice the 16-bit pixel-buffer figure, plus one
  16-byte-a-pixel plane for the W copy of a heal, spot heal or fill. The measured table (Release, 24 MP, three
  runs and a twin per case) is in `float_retouch.rs`'s module doc, with the five floors pinned in
  `core/tests/selected_color_planes_24mp.rs`.

#### Still refused at float, by name
- **The Sponge**: a saturation change follows a display-referred rule with no decided linear-light form.
  (Red Eye, refused when this section was written, runs on the display encoding since phase 2; the
  Sponge, the Paint Bucket, Dodge, Burn and the value-comparing selections run since the display-defined
  tools below.)
- **Paint Bucket, Dodge and Burn**: tolerance and tonal ranges are defined on display-encoded values.
- **The selections that compare pixel values** (Magic Wand, Color Range, Select Similar, Quick Selection;
  Select and Mask's edge stage with them): extended-range tolerances and colour distances are undecided. The
  Vision selections run since the section below.
- Edit ▸ Clear at float, retouching a mask, a channel or one colour channel of a float layer, and filtering
  the options bar's Mode popups to the float modes are not in this batch.

The core's operator table (`float_document.rs`, `REFUSED_RETOUCH` and `REFUSED_PAINT`) carries the first two
rows with their reasons, and `get_precision` reports them. The Blur, Sharpen, Smudge and History brushes,
refused when this section was written, shipped next (below).

#### Checks
- Core oracles: `core/tests/float_retouch.rs` (the clone against a W3C composite of the premultiplied bilinear
  sample; the heal against a Gauss–Seidel membrane in W written out in binary64, including the overshoot at
  both bounds; spot heal and fill against holes with a known continuation; refusals, cancellation, the
  capability rows and the operator table).
- MCP drive: gate F40 of `scripts/verify_phase11f_float.py`; `scripts/verify_v1_paths.py` for `stroke_path`.
- UI paths: the jobs-probe case "float retouch strokes" (`scripts/jobs-probe/float_retouch.swift`).
- Admission: `core/examples/float_retouch_admission.rs`.

### The Blur, Sharpen, Smudge and History brushes on a float layer (2026-10-12)

The four brushes edit a raster layer of a float document from the canvas and over MCP (`blur_stroke`,
`sharpen_stroke`, `smudge_stroke`, `history_brush_stroke`; replies and job details carry
`storage: "rgba32_float"`), as the jobs they are at 16 bits, with the 16-bit requests unchanged. No new
export, no signature change and no `.rz` change.

- **The f32 arm mirrors the masters.** `float_retouch_brush.rs` is the float arm of
  `precision_retouch_brush` and `precision_smudge`, entered at the top of their six entry points after the
  lock gate. The arithmetic is the shared kernels' (`retouch_brush::targets_in` / `mix`,
  `retouch_smudge::smudge`); 8- and 16-bit output is bit-identical.
- **Blur and Sharpen read the float Gaussian.** `targets_in` at `SampleRange::Extended` takes the FIR cut
  where its dropped tails are under 1e-7 of the weight (11 pixels a side at σ 2, 6 at σ 1), never
  `ceil(3σ)`, and does not clamp: a blur spreads a value above white, and a Sharpen result below zero is
  stored. A Sharpen target past ±65504 is written as the bound. On the highlight fixture of "Decisions in
  force" the worst error is 3.0e-5 (one 5000 pixel) and 1.5e-4 (the disc) for Blur and 1.2e-4 and 3.0e-4
  for Sharpen, against the 2 % bound (Sharpen's is 2 % of `max(|ref|, 0.1)`, because its result crosses
  zero).
- **Smudge carries the premultiplied f32 field unquantized.** Alpha is clamped to 0…1 and is transparent
  black below 2^-24; colour is `sample / alpha` clamped to the stored bound. Its own 256-million work cap
  applies as at 16 bits.
- **The History Brush restores bits.** Coverage × opacity of 1 writes the source state's own sample; below
  1 it is the premultiplied mix in binary64 rounded once to f32. The source is a float state of the same
  canvas size ("history brush source has a different bit depth" otherwise).
- **Layer RGB only**, as at 16 bits: a mask, a channel, one colour channel and Quick Mask refuse by name
  (`FloatRetouch.brushTools`), and so does an adjustment layer (capability `retouch_brush`).
- **Admission.** Blur, Sharpen and the History Brush plan the masters' figure at f32 width plus 76 and 32
  bytes a layer pixel. Smudge plans what it allocates: 40 bytes a footprint pixel (the field and the rect
  read) plus 20 a pixel of the tiles the write-back rebuilds (`PrecisionImage::rect_write_pixels`: the
  footprint rounded out to 256-pixel tiles, or the whole plane when the target is contiguous). The measured
  table (Release, 24 MP, three runs and a twin per case; Smudge at size 200, size 1000 and a thin stroke
  across a tile row) is in `float_retouch_brush.rs`'s module doc, with the six floors pinned in
  `core/tests/selected_color_planes_24mp.rs`.

Still refused at float, by name: the Sponge (the operator table's `REFUSED_RETOUCH` row; Red Eye shipped later), and
Dodge, Burn and the Paint Bucket (`REFUSED_PAINT`). Each needs a linear-light rule that waits on a decision.
(All four run since the display-defined tools below, and both rows are gone.)

Checks: `core/tests/float_retouch_brush.rs` (f64 oracles: the alpha-weighted Gaussian out to 8σ, the
sub-stepped smudge walk, the History Brush's bits and mix; the highlight bound; refusals, cancellation, the
capability row and the operator table); gate F42 of `scripts/verify_phase11f_float.py`; the jobs-probe case
"float retouch brushes"; `core/examples/float_retouch_brush_admission.rs`.

### Subject, Objects and People selections (2026-10-12)

Select ▸ Subject, Select ▸ Objects…, Select ▸ All People, the Subject tool and the Object Selection tool run in
a 32-bit float document, with their MCP mirrors `select_subject`, `inspect_subjects`, `select_subjects`,
`select_people` and `object_select`. The selection is what 16 bits commits: Vision's byte coverage, read as u16
(× 257) wherever a typed edit, a layer mask or `get_selection` takes it. It confines float filters, paint and
retouching and makes a layer mask.

- **What Vision sees.** The document's own tone map applied to the linear composite at view exposure 0, then the
  profile's tone curve and 8-bit quantisation: `ImageDocument.analysisComposite(of:)`, the image every menu and
  MCP path already took, and `canvas.image` for the Subject tool. These are Vision requests on a picture, so
  they need no linear-light rule; only admission was missing. The window's exposure and a candidate tone map
  are view state and are never read, so looking at the highlights cannot change what is selected.
- **A subject entirely above white under Clip is found poorly.** It renders flat white. No second tone map is
  made for analysis: set the document's tone map to Reinhard first.
- **Object Selection has no edge stage at float.** The core refuses `refine_edge` there (it compares colours in
  display terms), so the result is Vision's coverage clipped to the region. `ObjectSelection.refines` is the one
  answer: the tool records `refine: false`, `object_select` defaults `refine` to it, replies `refined` at every
  depth, and refuses `refine: true` on a float document by name.
- No core change, no new export, no memory plan and no `.rz` change: the Vision input is the canvas-sized RGBA8
  image it is at 16 bits.

Still refused at float, by name: Magic Wand, Quick Selection, Color Range, Select Similar and the Paint Bucket
(each compares pixel values with a tolerance defined in display terms), and with them Select and Mask,
`refine_selection`, Object Selection's edge refinement and the Magnetic Lasso.

Checks: gate F43 of `scripts/verify_phase11f_float.py` (the portrait in linear light with its chest scaled above
white, against the same picture at 16 bits; properties and IoU, never mask bytes) and the selection-probe case
"float subject targeting" (`scripts/selection-probe/float_subjects.swift`: the menu items, the Subject tool's
canvas events at exposure 0 and −6 EV, All People, the Object Selection commit and the Objects picker).

### New Document at 32 bits and 8/16 → 32-bit float (decision row 1; 2026-10-12)

File ▸ New… (`depth: 32` over MCP), Image ▸ Mode ▸ 32 Bits/Channel… and `convert_precision mode: "rgba32"
structure: "preserve"` make a 32-bit float document. The user reversed the "no promotion into float" decision:
a 16-bit picture or a RAW development must be able to join a float composite, and painting with light needs a
float document.

- **The rule** (`core/src/precision_conversion_float.rs`). Each u16 code `c` becomes the document profile's TRC
  decode of `c / 65535` as f32; alpha is `c / 65535`. An 8-bit source first takes the exact ×257 widening. The
  result is a float document in the same profile under `clip`. Masks and channels stay u16. Every record keeps
  its id. The pixel work is the layers paste's own promotion under Preserve (`promoted_layers`). Narrowing back
  returns every code, at 16 and at 8 bits. New Document at 32 converts the 16-bit blank, so white is exactly 1.0.
- **What blocks, by name, before any plane.** A blend mode, an effect blend mode or an adjustment the float mode
  refuses (on the record as it is, so no paste reset fires). An effective warp (a stale binding is disclosed
  and dropped). A profile with no matrix/TRC model. Only when nothing else blocks: a result too large to save
  and reopen at 32 bits, checked as the native geometry plan charged as float (`native_fits_geometry_as_float`).
  A float source is an error. `flatten: true` at rgba32 refuses.
- **Admission.** Output is 16 B/px of every converting raster. Scratch is the promotion's working memory plus
  `LAYER_PROMOTE_MARGIN_PER_PIXEL`, raised from 0 to 10 by this measurement. From 8 bits the 8→16 plan's scratch
  and output are added. Headline: 24 MP 16→float A 1,015,655,460 against max E 768,393,768 (1.32×); 61 MP
  8→float A 3,057,933,860 against 2,441,742,016 (1.25×). The full table is the module doc's.

Checks: `make t F=float_promote` (the published TRCs of the four built-ins, every 16-bit code round-tripped
exactly, carriage, refusals, New Document fills, cancellation and NULL safety); `make t F=float_promote_24mp`
(the capacity row one byte under the float result's own limit, and the floors); gate F44 of
`scripts/verify_phase11f_float.py`. The menu item and the sheet are covered by the Debug build and the jobs-probe
type check only; no UI drive was run.

### Display-defined tools, the rest of the blend modes and adjustments (decision row 3; 2026-10-12)

The Paint Bucket, Dodge, Burn and the Sponge on a float layer; Dodge and Burn on its mask or an alpha channel
(at 16 bits too); the Magic Wand, Quick Selection, Select ▸ Color Range…, Grow and Similar; Linear Burn, Linear
Light, Pin Light and Divide; and every adjustment but three, from the canvas, the menus and MCP. No new export,
header or `.rz` change; MCP `fill` gains `rgba32`, `dodge_burn` gains `target`, and a float `sample` reply gains
`center_display_codes`.

Decisions (each stated once in `core/src/display_encoding.rs`'s module doc, on top of its E; E the profile's forward curve
through the policy's own encoder, D its decode, never the tone map or the view exposure):

- **R1, the display copy.** RGB `round(65535·E(clamp(v,0,1)))`, alpha `unit_to_u16`: `display_sample` under
  Clip, bit for bit.
- **R2, comparisons.** Wand, Bucket, Grow and Similar compare 16-bit codes on R1; Color Range and Quick
  Selection their Lab ΔE76 on R1. A value above white compares as white. Output is native u16 coverage.
- **R3, the bucket.** The R1 region filled in linear light through the float paint walk in the float modes;
  `linear` ink (`rgba32`, an above-white foreground) as given on every layer fill.
- **R4, Dodge/Burn on a float layer.** In 0…1 `t = D(shaped(E(v)))`; above 1 `t = v·D(shaped(1))`; below 0
  `t = v + D(shaped(0))`; out `v + c·(t − v)` in f64, held to ±65504; alpha-0 skipped.
- **R4m, on a mask or alpha channel** (u16 at 16 and 32 bits): `65535·shaped(s/65535)` lerped by stroke ×
  selection. A mask is coverage, not light.
- **R5, the Sponge.** `retouch_brush::targets_in` at `Extended`, unclamped; on a mask or channel it refuses at
  every depth (a one-plane target has no saturation).
- **B, blend modes** (layer, ink and effect alike): Linear Burn `cb+cs−1` and Linear Light `cb+2cs−1`
  unclamped, Pin Light on its master, Divide `clamp(cb / max(cs, 1/65504), ±65504)`. Refused, because each
  inverts about white or divides by `1 − v`: Screen, Overlay, Soft Light, Hard Light, Color Dodge, Color Burn,
  Vivid Light, Hard Mix, Exclusion. A layers paste no longer resets Linear Burn, Linear Light or Pin Light.
- **A, adjustments** (layer and destructive apply, equal): Hue Rotate and Sepia their matrix and Grayscale the
  Rec. 709 luma on linear values; Threshold, Posterize and Gradient Map display-mapped `D(master(E(clamp)))`;
  Vibrance, Selective Color, Replace Color and every Hue/Saturation setting with no closed form display-scaled
  `m·(D(master(E(max(c,0)))) + min(c,0))`, `m = max(1, max rgb)`; every Color Lookup table on the stored
  values; the `document_v2` White Balance apply. Refused: Invert (no inverse above white),
  Shadows/Highlights (no linear-light guide) and the legacy White Balance (an sRGB-encoded matrix).

Still refused at float, by name: the nine modes above, Invert, Shadows/Highlights, the legacy White Balance,
Select and Mask's edge stage (Refine Edge, Quick Selection's Enhance Edge, Object Selection's refinement) and
the Magnetic Lasso. Rebased onto lane C's 3cf8a8aa (Red Eye and the display-referred filters): the rules use
its one in-range encode and decode in `display_encoding.rs`; R1's clamp to 0…1 is the comparison rule on top,
where a display-referred filter continues E above white instead.

Admission at 24 MP (Release, one case per process, `MallocLargeCache=0`, three runs and a twin reading 0, E the
interval peak over the call): bucket A 1,976,388,608 / max E 818,053,720; dodge 1,544,388,608 / 769,966,608;
mask dodge 824,388,608 / 96,092,232; sponge 3,368,388,608 / 1,442,399,264; wand, range and quick 2,340,582,912 /
769,999,400; display-scaled Vibrance apply 1,030,501,774 / 818,053,696. Every plan is at least 1.25 × max E, so
no term moved; the module docs hold the tables and `selected_color_planes_24mp.rs` /
`float_adjust_apply_24mp.rs` pin the floors. `examples/float_paint_admission.rs` now reads the interval peak,
as the retouch and selection examples do: `/usr/bin/time`'s whole-process peak includes building the document.

Checks: `make t F=float_paint_tone`, `F=float_smart_selection`, `F=float_display_adjustments` (f64 oracles over
1.0, 2.0, 8.0, 0.999, −0.25 and 0.18; the display forms against a 16-bit encoded render of `E(c)`) and the
edited float suites; gate F46 of `scripts/verify_phase11f_float.py` with F8, F13, F14, F16, F17, F37, F40 and
F42 updated. The jobs-probe and selection-probe edits are type-checked only; no UI drive was run.
