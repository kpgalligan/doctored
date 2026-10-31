# HDR and 32-bit float document workflow (ADR)

- **Status:** Accepted — open items remain ([#18](https://github.com/kpgalligan/doctored/issues/18))
- **Phase / dates:** 11F, shipped 2026-09-18 (drive gates F1–F10 pass); admitted set widened by 12E-2b/2c, 12F-1/1b and the v1 float batches
- **Originals:** [archive/phase-11f-hdr-float-status.md](../archive/phase-11f-hdr-float-status.md) — the phase's full history, to 2026-09-26; [archive/2026-10/phase-11f-hdr-float-status.md](../archive/2026-10/phase-11f-hdr-float-status.md) — this doc as of 2026-10-07, with every measurement table and issue record; [archive/phase-11f-hdr-float-shipped.md](../archive/phase-11f-hdr-float-shipped.md) — the shipped companion as of 2026-10-30, with each item's take readings and admission ratios; [archive/2026-10-30/phase-11f-hdr-float-status.md](../archive/2026-10-30/phase-11f-hdr-float-status.md) — this doc as of 2026-10-30, before the companion fold, with its take records
- **Key commits:** `a3e4f2c`
- **Carryover:** closes **P7-25** (HDR/extended-range storage, editing, output) and **P7-36** (float TIFF interchange)
- **Photoshop harness:** the runners under `scripts/photoshop-probe/` named below live in the private archive `kpgalligan/doctored-dev`; receipts live under `doctored-fixtures/photoshop/`.
- **Later batches:** [v1-float-editing.md](v1-float-editing.md) (rules R1–R5, A–E; float filters, retouching, Stroke Path, promotion) and [v1x-hdr-photos.md](v1x-hdr-photos.md) (HDR stills, EDR display).

> **Removed 2026-10-10:** smart objects, live filters and non-RAW retained sources (placed images, host
> media, Live Photo frames) are gone for v1; camera RAW layers stay. Where this record describes them, that
> code no longer exists: [removal ADR](convert-to-smart-object-status.md).

## Context
Genuine HDR/scene-linear input (above-white and negative wide-gamut values) had no home: the typed document stored
u16 only. The plan required preserving real float input through editing, native/history/recovery and float TIFF
I/O, with HDR readouts and deliberate tone-mapped SDR output — qualified on genuine float data, not a promoted SDR
image. Byte and u16 modes had to stay byte-identical. Builds on [11D](phase-11d-format-additions-status.md)'s
`tiff_float.rs` seam and [11E](phase-11e-linear-light-status.md)'s evaluation policy.

## Decisions
- **Storage**: straight RGBA32 float (`PrecisionStorage::Rgba32Float`). RGB = **linear light in the document
  profile's primaries**; TRC applied only at SDR encode. Straight alpha in [0, 1]. Finite,
  `|v| ≤ FLOAT_RANGE_MAX = 65504` (largest finite half, so any sample survives a half round trip). **Negatives are
  stored** and admitted by every admitted operator; mapped to 0 only at display/SDR output.
- **Promotion** from u16/byte into float is Image ▸ Mode ▸ 32 Bits/Channel… / `convert_precision mode: rgba32`
  (`precision_conversion_float`; the original no-promotion decision was reversed on 2026-10-12, because a 16-bit
  picture or a RAW development must be able to join a float composite); `from_image` with F32 still refuses.
- **One storage gate**: `PrecisionDocument::build(.., float)` requires every raster leaf's storage to match
  `float.is_some()` (else "high-depth document storage does not match the document mode"). `raster()` keeps its u16
  refusal; `raster_float` (`pub(crate)`) is the only F32 leaf constructor.
- **Evaluation invariant**: a float document has `linear_light == None`, `float == Some(..)`, and `evaluation()`
  answers `LinearLight` from `float` alone — no 2.36 MB `TypedTransfer`. `with_evaluation(LinearLight)` is
  `Ok(None)` (no phantom undo); `Encoded` is refused; `rebind_evaluation` is a no-op. `linear_light_refusal()`
  returns `float_refusal()` first, so 11E's two host guards refuse unadmitted float edits with **no new guard**.
- **Finite rule is import-only** (`float_document::sanitize`): NaN→0, ±∞→±65504, out-of-range clamped, alpha
  clamped, each counted into import warnings. Everywhere else (render, edit, native record) a non-finite or
  out-of-range result **refuses** (`from_rendered_float(extended: true)`, `from_rgba32_extended`). ONE named exception
  (D3): filtered geometry clamps each output RGB component to the source layer's own visible range (per channel over
  alpha > 0) and drops an output alpha below 2^-24 to transparent (`sampling::EXTENDED_ALPHA_FLOOR`,
  alpha 0 and RGB 0), because a kernel's overshoot is a property of the filter, not of the document. The floor is
  2^-24, not the renderer's 2^-96: it is below one f32 ulp at 1.0 and far below a 16-bit code, and alpha × any mask
  coverage (≥ 2^-16) × any UI opacity stays far above the 2^-96 render floor, so a filtered layer under a mask still
  renders (`checked_alpha`).
- **Import alpha/profile**: `ExtraSamples=1` → unpremultiplied with a warning; `2` taken as is; absent → 1.0; 1–2
  samples fill R=G=B. An embedded ICC must be an RGB matrix/TRC profile passing `validate_precision` (the SDR encode
  needs its forward curve; linear values are defined by the profile's primaries); none → sRGB primaries, with a
  warning.
- **Tone maps** (per RGB channel; alpha never mapped), then `PrecisionToneEncoder` (the doc's forward curve, no
  inverter): `clip` (default) `y = clamp(v,0,1)`; `reinhard` `u=max(v,0); y=min(u(1+u/W²)/(1+u),1)`, `W ∈ [1, 65504]`,
  default 4.0. `ToneMap::parse` is **hand-written** over a JSON map because serde's internally tagged enum accepts
  unknown keys on unit variants; any extra key (or `white` beside `clip`) refuses `unknown tone map field`.
- **Where the map applies**: display only via `PrecisionDocument::display_rgba8` (`PrecisionImage::to_rgba8`
  **refuses** F32, so no clamped preview can leak); 8/16-bit export (`doc_export::export_stages` substitutes
  `tone_mapped_u16` before stage 1); conversion to u16/byte (`float_to_u16`, `downconvert`). **32-bit export is
  exact** at size none — no map; a size mode resamples the rendered composite through filtered geometry's
  extended-range resize, RGB clamped to the composite's own visible range.
- **Admitted operators** — the live table is `FLOAT_FAMILIES` in `core/src/precision_capability.rs` (and
  `float_document::float_operators_json`). Everything not in it refuses `"<operator> is unavailable in a 32-bit
  float document"`; the refusal walk (`float_document::float_refusal`) asks the kernel (`apply_linear(..).err()`)
  rather than re-listing adjustments. The display-defined tools and the rule-A adjustments run under
  `display_encoding`'s stated rules ([v1-float-editing.md](v1-float-editing.md)); the nine blend modes that invert
  about white refuse by name (Blend modes, below).
- **Adjustments at 32 bits**: each adjustment measured against Photoshop 26.10.0 at 32 bits takes its OWN closed
  form on the stored linear values (`adjust_float::prepare`, derived at parse, never persisted), each within 2e-3 of
  the take2 receipt (`float-adjust-473/take2`) or within the later takes' tolerance units:

  | op | 32-bit form |
  |---|---|
  | Levels | the master stage with every clamp removed, `sgn(t)·abs(t)^(1/γ)` |
  | Black & White (Tint off) | the master decomposition, `max(0, gray)` |
  | Black & White (Tint on) | gray `max(0, g)` capped at 16; SetLum of the tint's numbers/255 to it, ClipColor at 16; `+1/510` when fired |
  | Channel Mixer | the master matrix in 1/1024 steps, constant `trunc(512·K)/1024`, no clamp |
  | Photo Filter | the density multiply on white-normalized XYZ in the profile's NATIVE basis, Y held |
  | `bcs` vs legacy Brightness/Contrast | the master affine, no clamp |
  | Exposure (both evaluations) | per stored channel `sgn(u)·abs(u)^(1/γ)`, `u = v·2^e + o`, no reference-basis matrices |
  | Curves (point form) | the natural cubic spline, no clamp; above the last point the end slope; below the first flat, or its slope when it sits at 0 |
  | Curves (map / pencil form) | each map piecewise linear on `X = 255·v`, end segments continued, channel map first |
  | Hue/Saturation (Colorize; master Lightness alone; master Hue alone) | C32 colorize; per-component two-sided lerp, a component past the target held; hexcone rotation, shift on a 1/24576-turn grid truncated toward 0 |
  | Color Balance | per-channel Levels in whole units, r2 gamma, Hermite toe; beyond [0, 1] the 4096-stage end slopes |
  | Color Lookup (3D, domain and entries in 0…1) | the table on stored values, tetrahedral, an input above 1 as 1, below 0 as 0 |

  The master `ChannelMixer` truncates to 1/1024 steps at EVERY depth (an 8/16-bit render moves by at most
  `Σ|v|/1024`, under one 8-bit code). Exposure's gamma is the reciprocal power at every depth. Every other
  Hue/Saturation setting and the remaining adjustments run under rule A ([v1-float-editing.md](v1-float-editing.md));
  Invert, Shadows/Highlights and the legacy White Balance refuse by name. Color Lookup outside its measured form (a
  1D table, another domain, entries outside 0…1, abstract-baked) runs `CubeLut::apply` on the stored values under
  rule A. Oracle `core/tests/float_adjustments.rs`; drive gate F16.
- **Destructive Image ▸ Adjustments at 32 bits**: `float_adjust_apply` applies the layer's own form
  (`apply_linear`; each op applied gives the same result as its layer, measured), then the straight alpha-preserving
  coverage lerp with no clamp (master `mix_codes`), through `rebuilt_rgba32_extended`. The applied set is the
  operator table's `*_apply` rows; the `document_v2` White Balance applies under rule A; the legacy White Balance
  refuses by name, and an op whose layer form refuses keeps that sentence. Admission measured at 24 MP (plan
  40 B/px plus fixed terms, pinned by `float_adjust_apply_24mp`). Oracle `core/tests/float_adjust_apply.rs`; drive
  gate F17.
- **Readouts**: above-white (`v > 1`) / below-zero (`v < 0`) instead of clipping. Sample
  (`PrecisionImage::float_sample`) with `stops = log2(max(v, 2^-16))`; histogram stop-binned, 256 bins over
  −12…+6 EV, channel 4 = Rec.709 luma of linear components, plus min/max/zero/above/below counts. The histogram and a
  weighted sample are the float arm of the schema2 scoped analysis (`float_stats::evaluate`), so Composite, Active
  layer and Below active layer, with selection weighting, answer at 32 bits; RGB eligibility is alpha > 0, and the
  bins and the `*_weight_units_rgba` twins carry each pixel's coverage weight.
- **Source-plane selection**: Load Selection from the Red/Green/Blue/luminosity plane or a layer's transparency
  (`alpha` / `luminance` / `color` kinds, `source_plane_selection` in `ADMITTED`) maps each f32 sample to u16
  coverage by **clamping the scene-linear value**: `unit_to_u16(v) = round(65535 · clamp(v, 0, 1))`, alpha the same,
  whatever the tone map; luminosity is the profile's Y row over the UNclamped samples, only the sum clamped. Measured
  against Photoshop 26.10.0 before any code (`float_selection.py`; `float-selection-442/take1` matched sRGB and ROMM
  exactly). Magic Wand, Color Range, Select Similar and Quick Selection compare the R1 display copy
  (`display_encoding`; [v1-float-editing.md](v1-float-editing.md)), as does Select and Mask's edge stage (rule E);
  Decontaminate Colors refuses by name. Oracle `core/tests/float_source_selection.rs`; drive gate F13.
- **Native v40** (`core/src/float_native.rs`): `40` outermost, next word **must** be `39`; body storage tag **2**
  (admitted only under 40, so earlier readers refuse rather than misread); raster payload `u32 w, u32 h, u64 w·h·16,
  raw LE f32 RGBA` (PNG has no float); tail `"FLOT" | schema 1 | operator (0 clip / 1 reinhard) | f32 white`, all
  validated before any pixel decode. u16 documents' bytes unchanged. History, snapshots, archive (`storage:
  "rgba32_float"`) and recovery ride the native file.
- **Interchange**: `open_preserving` branches `DRDC` → native, a float TIFF, EXR or `.hdr` → float document, else
  the unchanged u16 codec; a 32-bit PSD or PSB (`Lr32`) opens as a float document (`psd_import_float.rs`). `bit_depth: 32` export writes `tiff`, `exr`
  or `hdr` (TIFF: 4 samples, `ExtraSamples=2`, Deflate, no predictor, document profile and packets;
  `float_samples: "f16"` a half TIFF or OpenEXR) and requires `profile: document`, no `sharpen` and `dither: none`
  (each refused by name); Export as PSD writes a 32-bit PSD, PSB or layered TIFF (`psd_export_float.rs`).
- **Host**: Image ▸ Tone Map… (`ToneMapSheetController`, live preview; one undo step); Convert Precision offers
  16/8-bit; a float TIFF opens only as float (`precision: "rgba8"` refused); Export offers/defaults "32-bit float";
  MCP `get_tone_map` (`nonRecordable`) / `set_tone_map` (step), `get_document.float`, `summary.precision =
  "rgba32_float"`. Tool rail: `FloatDocument.tools`, then the core's depth-32 table, decide each tool
  ([v1-float-editing.md](v1-float-editing.md)).
- **Resource accounting**: raster `w·h·16`; policy tables ≈1.57 MB via `record_resources`; display/tone-map
  `+w·h·12`; export plan `+source·8`, plus `source·16·2 + 1 MiB` at bit depth 32.
- **Fixture** `core/tests/fixtures/float_11f/scene.tiff` is authored by `scripts/verify_phase11f_float.py
  --author-fixture` (stdlib), not `magick` (whose Q16-HDRI scales float output), and verified by `tiffinfo` /
  `magick identify`.

## Rejected alternatives
- Clipping negatives at storage — would make float a worse container than u16.
- A second evaluation policy beside `linear_light` — float is linear light by construction.
- A range bound above 65504 — buys range no interchange format here can carry.
- serde-derived tone-map parsing — silently accepts `{"operator":"clip","white":4}`.
- Previewing tone-map candidates live — the canvas already is the tone-mapped projection.

## Consequences
Admitting any new float operator requires an f64 oracle on extended-range input (including negatives) and a
`FLOAT_FAMILIES` entry; exact permutation/copy ops may be admitted on that argument alone (12F precedent). An f32
plane above `CONTIGUOUS_CACHE_BYTES` is refused at the 2 GiB pin but admitted at the 8 GiB limit of a 16 GB Mac;
it is tiled. Tests: `core/tests/float_{document,native,interchange}.rs`; drive `scripts/verify_phase11f_float.py`.

## Open items (verified 2026-10-30)
- `OPEN` [#18](https://github.com/kpgalligan/doctored/issues/18) — Behind and Clear for Fill/Stroke and painting,
  which Doctored has at no depth (not a float-only gap).
- `DROPPED` (2026-10-30) — the operations still refused at 32 bits, each by name with its reason in the operator
  table: Apply Image (its blend runs on u16 planes clamped to 0…1), Assign or Convert with a profile that has no
  matrix/TRC model or whose curves fail the precision check, Convert with absolute colorimetric, black-point
  compensation or preserve-appearance, a converted component outside ±65504. A deliberately bounded set; a float
  document requires a matrix/TRC profile by design.
- `DROPPED` (2026-10-30) — luminance and ACES tone maps: HDR Toning's methods and the clip/Reinhard maps cover SDR
  delivery.
- `DROPPED` (2026-10-30) — float-predictor (predictor 3) TIFF encoding: a Deflate float TIFF without the predictor
  is valid and reopens bit-exact, only larger.
- `DROPPED` (2026-10-30) — measuring Photoshop's above-white TYPE colour: above-white type colour ships.
- `DROPPED` (2026-10-30) — the unmeasured plane-fill corners (the bucket, gradient and brush on a colour plane, Quick
  Mask fill and `modify_scalar` keep Rec. 709; a `3059r` rounding tie, built half up; Dissolve and Stroke over
  partly transparent pixels).
- `DROPPED` (2026-10-30) — a Color Lookup Dither key (the take found 16-bit dither a no-op and the 8-bit form
  aperiodic noise) and the unmeasured `.cube` load at 32 bits (three takes could not load one).
- `REMOVED` — float masters and smart-object contents at 32 bits: the feature was removed 2026-10-10.
- `DROPPED` — `PrecisionStateArchive.swift` float coverage (out of scope; internal probe archive only).

## Shipped
- **Geometry** — filtered geometry in premultiplied extended-range f32 through the shared walks with the D3 clamp
  and alpha floor (`float_filtered_geometry.rs`); warp over the binary64 walk, applied at once with no persisted
  field (`float_warp.rs`); 32-bit export resampling (`float_interchange.rs`); Move and Free Transform of selected
  pixels (`precision_selected_pixels.rs`); `layer_add`, stamp/flatten and exact canvas ops (12E-2b/2c, 12F-1/1b).
- **Blend modes** — slice 1 (`03ad6f0`): nine modes measured at 32 bits (`blend_float.rs`, `float_blend_modes.rs`,
  F14): Linear Dodge/Subtract unclamped, Hue…Luminosity in the document's linear primaries with no ClipColor.
  Divide by rule B `clamp(cb / max(cs, 1/65504), ±65504)`, with Linear Burn, Linear Light and Pin Light; Screen,
  Overlay, Soft Light, Hard Light, Color Dodge, Color Burn, Vivid Light, Hard Mix and Exclusion refuse by name (each
  inverts about white or divides by 1 − v). Slice 4 (`89e7dbd`): Pass Through groups (`Kind::PassThrough`, F18).
- **Styles** — slice 2 (`67385ec`): layer styles, Blend If and the global light at 32 bits (`style_float`, F15);
  effect colours are the document's LINEAR numbers (`LinearInk`), Blend If reads stored values unclamped.
- **Adjustments** — slices 3a–3d: the forms above as layers (rendered full and regional, merged, saved, pasted) and
  as destructive applies; Exposure's `1/gamma` at every depth (slice 3b: every saved Exposure with gamma ≠ 1
  re-renders); Curves the NATURAL cubic spline at every depth (`adjust_curves::Spline`; slice 3d: every saved curve
  with 3 or more points re-renders). Photo Filter at 8/16 bits is the D50 PCS multiply and Luminosity blend
  (`adjust_photo_filter`, `adjust_photo_filter_tests.rs`; every saved 8/16-bit Photo Filter re-renders; residuals
  8-bit ClipColor highlights ≤ 7.3 codes, ROMM toe 3.4e-3). Later takes (`float-adjust-473/take3`–`take7`,
  `adjust-parity-535-536/take4`–`take6`): Color Lookup's one 3D lookup is tetrahedral at every depth
  (`adjust_lut::sample_3d`) and a plain-grid device link of at most 33 nodes is stored at its own grid
  (`LutPipeline::plain_grid`); 16-bit map Curves reads 15-bit stage tables (`adjust_curves::stage15`); the map form
  and Color Lookup run at 32 bits (`float_lookup_tint_map.rs`); Black & White's Tint is SetLum of the tint's
  numbers to the grey with ClipColor (`blend::set_lum_within`, `black_and_white_tint.rs`, F35; residual 16-bit
  high-only minimum 2 15-bit codes; 8 bits on the integer path, `adjust_bw8.rs`); Hue/Saturation's reference form R0
  at 8 and 16 bits (`adjust_hue_saturation.rs`); Color Balance replaced at 8/16 bits and admitted at 32 as a layer
  and an apply (`adjust_color_balance`). The adjustments once refused at 32 bits run under rule A since 2026-10-12,
  except Invert, Shadows/Highlights and the legacy White Balance, which still refuse by name.
- **Selections** — Load Selection from a colour plane, the luminosity plane or a layer's transparency (UI and MCP
  `load_selection` / `add_channel from:plane`); from the RGB composite: float reads the profile's Y row over
  unclamped samples (`precision_selection.rs`), and 8/16 bits load the luminosity in Edit ▸ Color Settings' Gray
  working space (`gray_working.rs`, `dr_doc_composite_gray_plane`; `gray_working_selection.rs`); every other luma
  stays Rec. 709. Tolerance and distance selections and the edge stage on R1 (2026-10-12).
- **Paint and fill** — Fill… / Stroke…, the ⌫ chords, MCP `fill_selection` / `stroke_selection` and the
  layer-edge stroke on a layer, its mask or an alpha channel (`float_paint`'s f32 arm of `paint_blend`,
  `float_fill_stroke.rs`); Quick Mask Fill on the native u16 coverage; Fill / Stroke on the Red, Green, Blue and
  Alpha planes writing the ink's 30/59/11 luma, raw (`plane_swatch::plane_fill_value`, F32; Darker and Lighter Color
  refuse there; opacity exact; `t` not scaled by the pixel's stored alpha; linear ink writes the luma of the linear
  numbers; an Alpha target clamped to 0…1), and at 8/16 bits the colour-channel gray (`plane_ink_u8` rounded,
  `plane_ink_u16` unrounded, not weighted by alpha; a mask and a saved alpha channel keep their Rec. 709 gray, an
  Alpha plane takes the plane gray; a moved colour channel leaves its 8-bit code over 255, `color_plane_hole`; F30).
  Above-white ink: `color_space` `linear` on `paint_blend` and `stroke_layer_edge`, RGB 0…65504 as is, recorded as
  `rgba32`; a mask, alpha channel and Quick Mask clip it to 0…1. The Brush, Eraser and Gradient on a layer in linear
  light with no clamp (`float_paint_ink::FloatInk`; no options-bar intensity control: above-white ink is the Brush's
  `rgba32` and the Gradient's `start_rgba32` / `end_rgba32`, the UI offering only the Foreground → Background ramp
  and stored stops staying sRGB hex; the Gradient converts each stop to linear light and interpolates there; the
  Eraser `alpha × (1 − coverage × opacity)`, RGB untouched; above-white ink aimed at a mask or channel refuses by
  name, a Red, Green, Blue or Alpha target refuses for all three, and a blend mode outside
  `NativeLayerBlending.floatModes` refuses before the stroke or drag starts; `float_brush_tools.rs`, F37). The Paint
  Bucket, Dodge, Burn and Sponge run under `display_encoding`'s rules ([v1-float-editing.md](v1-float-editing.md)).
- **Type and shapes** — text and shape layers at 32 bits (`artwork_render::render_float`, `dr_artwork_render_float`;
  stored colour numbers as LINEAR light, 16-bit antialias coverage kept, `float_artwork.rs`, F20; the host writes
  description v4 `"ink":"linear_srgb"`); intrinsic colour-glyph pixels decode through sRGB at float. Above-white
  text/shape colour (RGB 0–65504 in the core, the sheets' Intensity and MCP; 32 → 16/8 clips it at 1); the
  options bar's Text and Shape swatches open the Color Picker with its Intensity row in a float document, and keep
  the system panel at 8 and 16 bits; no MCP twin, as a view preference (`app/CLAUDE.md`).
- **Readouts** — float scoped statistics for the Info panel and MCP `histogram` / `sample_pixel` /
  `sample_color`; `histogram source:"scalar"` on a mask or channel through the 16-bit scalar counter; the eyedropper
  picks the float mean, above white kept.
- **Sources** — camera RAW layers at 32 bits render their 8/16-bit master into linear light
  (`retained_source_float`, `FLOAT_SOURCE_MARGIN_PER_PIXEL` 26; Redevelop, presets, Sync and locals admitted;
  `float_raw_source.rs`, F34); a camera RAW opens as a float document with `raw.output_encoding: linear_float`
  (`scripts/verify_v1_raw_float.py`; [phase-8g-raw-enhancements-status.md](phase-8g-raw-enhancements-status.md)).
  Placed images and floating-point TIFFs place at 32 bits through the same master render and are rasterized on
  landing since 2026-10-10 (`float_placed_image.rs`, `float_placed_master.rs`).
- **Interchange and display** — v1 float phase 1: OpenEXR (`exr_float`) and Radiance `.hdr` (`radiance_hdr`) open as
  float documents (the first non-deep part with R, G and B; associated alpha unpremultiplied; xy primaries within
  0.002 of Rec.709 or Display P3 take that profile, any other set converts once in linear light to sRGB primaries
  with a warning, `float_primaries`; a file holding any deep part refuses); `bit_depth: 32` exports `tiff`, `exr` or
  `hdr`, with F16 TIFF and OpenEXR (`doc_export_float`); View ▸ 32-bit Exposure… (±20 stops, per window, never
  document state: `float_view`) and the live Tone Map… candidate drawn in the proof slot (a preview that writes
  `canvas.previewImage` is drawn at 0 stops); measured at 24 MP (`float_interchange_24mp`, F38). EDR display: View ▸
  HDR Preview (`float_view::display_extended`). The 32-bit PSD reader and writer (`Lr32`, `psd_import_float`,
  `psd_export_float`).
- **Documents** — File ▸ New at 32 bits and Image ▸ Mode ▸ 32 Bits/Channel… (preserve only; every record carried,
  each code decoded through the profile's TRC; the round trip back is exact; what float cannot hold blocks by name).
  The v1 float gaps: Apply Layer Mask (`precision_mask_apply`), Layer Via Copy / Cut (`precision_layer_via`), Assign
  Profile (the linear samples relabelled) and Convert to Profile (the matrix between the primaries in linear light,
  no clamp; `float_color`), Calculations, Add Luminosity Masks and Contact Sheet (`scripts/verify_v1_float_gaps.py`).
