# HDR and 32-bit float document workflow — shipped runs

The settled record of 11F and its shipped follow-ups, moved verbatim from
[phase-11f-hdr-float-status.md](phase-11f-hdr-float-status.md) when slice 3c took that doc past about 24 KB:
the decisions in force, then each shipped item. That doc keeps the head and the open items.

> **Removed 2026-10-10:** smart objects, live filters and non-RAW retained sources (placed images, host
> media, Live Photo frames) are gone for v1; camera RAW layers stay. Where this record describes them, that
> code no longer exists: [removal ADR](convert-to-smart-object-status.md).

## Decisions
- **Storage**: straight RGBA32 float (`PrecisionStorage::Rgba32Float`, which 7A already had). RGB = **linear light in
  the document profile's primaries**; TRC applied only at SDR encode. Straight alpha in [0, 1]. Finite,
  `|v| ≤ FLOAT_RANGE_MAX = 65504` (largest finite half, so any sample survives a half round trip). **Negatives are
  stored** and admitted by every admitted operator; mapped to 0 only at display/SDR output.
- **No promotion** from u16/byte into float — REVERSED by the user 2026-10-12 ([status](phase-11f-hdr-float-status.md)); `from_image` with F32 still refuses.
- **One storage gate**: `PrecisionDocument::build(.., float)` requires every raster leaf's storage to match
  `float.is_some()` (else "high-depth document storage does not match the document mode"). `raster()` keeps its u16
  refusal; `raster_float` (`pub(crate)`) is the only F32 leaf constructor (from `float_io` and the v40 reader, and
  since 12E-2b the `add_layer` op's f32 zeros).
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
  renders (`checked_alpha`); the 7A refusal rule itself is unchanged.
- **Import alpha/profile**: `ExtraSamples=1` → unpremultiplied with a warning; `2` taken as is; absent → 1.0; 1–2
  samples fill R=G=B. An embedded ICC must be an RGB matrix/TRC profile passing `validate_precision` (the SDR encode
  needs its forward curve); none → sRGB primaries, with a warning.
- **Tone maps** (per RGB channel; alpha never mapped), then `PrecisionToneEncoder` (the doc's forward curve, no
  inverter): `clip` (default) `y = clamp(v,0,1)`; `reinhard` `u=max(v,0); y=min(u(1+u/W²)/(1+u),1)`, `W ∈ [1, 65504]`,
  default 4.0. `ToneMap::parse` is **hand-written** over a JSON map because serde's internally tagged enum accepts
  unknown keys on unit variants; any extra key (or `white` beside `clip`) refuses `unknown tone map field`.
- **Where the map applies**: display only via `PrecisionDocument::display_rgba8` (`PrecisionImage::to_rgba8` now
  **refuses** F32, so no clamped preview can leak); 8/16-bit export (`doc_export::export_stages` substitutes
  `tone_mapped_u16` before stage 1); conversion to u16/byte (`float_to_u16`, `downconvert`). **32-bit export is
  exact** at size none — no map; a size mode resamples the rendered composite through filtered geometry's extended-range resize,
  RGB clamped to the composite's own visible range.
- **Admitted operators** — the live table is `FLOAT_FAMILIES` in `core/src/precision_capability.rs:100` (and
  `float_document::float_operators_json`). At 11F: Normal/Multiply/Darken/Lighten/Difference (the `blend.rs`
  kernels with no clamp, called unchanged), opacity/name/visibility/clipping/duplicate/group-all, masks and
  channels, White Balance at `document_v2` via `Calibrated::apply_linear` (no TRC round trip), Exposure of either
  evaluation per stored channel, readouts,
  render, convert, export, native save, `set_tone_map`. Later: nine modes measured against Photoshop at 32 bits (slice 1 below),
  layer styles, Blend If and the global light (slice 2 below), the adjustments of slice 3 (next bullet),
  `layer_add` (12E-2b), `layer_stamp` /
  `layer_flatten` (12E-2c), exact canvas ops `geometry_canvas_size` / `_crop` (zero angle) / `_rotate` (quarter
  turns) / `_flip` (12F-1/1b), plus lock/link/merge/label/comp/move/group families. Everything else refuses
  `"<operator> is unavailable in a 32-bit float document"`; the refusal walk (`float_document::float_refusal`)
  asks the kernel (`apply_linear(..).err()`) rather than re-listing adjustments.
- **Adjustments at 32 bits (slice 3)**: each adjustment measured against Photoshop 26.10.0 at 32 bits takes its OWN closed
  form on the stored linear values, fitted from the take2 receipt
  (`doctored-fixtures/photoshop/float-adjust-473/take2/`, `judged.json` authoritative; driven by
  `scripts/photoshop-probe/float_adjust.py`; the fits are handoff-local, not in the repo), derived at parse
  (`adjust_float::prepare`, never persisted) and run on its master's statements. Max error over every patch, set and
  profile, `|PS32 − form| / max(1, |form|)`, against 2e-3:

  | op | 32-bit form | max err |
  |---|---|---|
  | Levels | the master stage with every clamp removed, `sgn(t)·abs(t)^(1/γ)` | 1.5e-6 |
  | Black & White (Tint off) | the master decomposition, `max(0, gray)` | 8.7e-8 |
  | Channel Mixer | the master matrix in 1/1024 steps, constant `trunc(512·K)/1024`, no clamp | 4.8e-8 |
  | Photo Filter | the density multiply on white-normalized XYZ in the profile's NATIVE basis, Y held | 4.2e-4 |
  | `bcs` vs legacy Brightness/Contrast | the master affine, no clamp | 8.4e-4 |
  | Exposure (both evaluations) | per stored channel `sgn(u)·abs(u)^(1/γ)`, `u = v·2^e + o`, no reference-basis matrices | 9.5e-8 |
  | Curves (point form) | the natural cubic spline, no clamp; above the last point the end slope; below the first flat, or its slope when it sits at 0 | 1.9e-7 |
  | Hue/Saturation (Colorize; master Lightness alone; master Hue alone) | C32 colorize; per-component two-sided lerp, a component past the target held; hexcone rotation, shift on a 1/24576-turn grid truncated toward 0 | take3 0.11 / 0.60, take4 0.74 tol. units |
  | Color Balance | per-channel Levels in whole units, r2 gamma, Hermite toe; beyond [0, 1] the 4096-stage end slopes | take6 0.74 / 4.3e-4 tol. units |
  | Curves (map / pencil form) | each map piecewise linear on `X = 255·v`, end segments continued, channel map first | take3 0.98 tol. units |
  | Color Lookup (3D, domain and entries in 0…1) | the table on stored values, tetrahedral, an input above 1 as 1, below 0 as 0 | take3 0.004 tol. units |
  | Black & White (Tint on) | gray `max(0, g)` capped at 16; SetLum of the tint's numbers/255 to it, ClipColor at 16; `+1/510` when fired | take6 0.003 tol. units |

  The master `ChannelMixer` truncates the mixer to 1/1024 steps at EVERY depth, the steps measured in Photoshop
  at 16 bits too (3.05e-5 against 1.95e-3 exact): an 8/16-bit Channel Mixer render moves by at most
  `Σ|v|/1024`, under one 8-bit code. Photoshop's Exposure gamma is the reciprocal power at every depth (slice 3b below). Everything else
  refuses by name with its own operator row: Hue/Saturation with Saturation, Hue with another master slider or a hue range edited,
  Color Lookup with an abstract profile, a 1D table, another domain or entries outside 0…1, and the ops Doctored has no 32-bit form for (none was measured). Oracle `core/tests/float_adjustments.rs`; drive gate F16.
  Photo Filter at 8/16 bits is NOT this form: the multiply in the profile's own D50 PCS
  (tag matrix, no adaptation), encoded, then (preserve) the Luminosity blend, W3C SetLum with Lum 0.3/0.59/0.11
  on encoded values (`adjust_photo_filter`). Fit: 16-bit 9.0e-4 (phase-10), 7e-4 (take2 sRGB); 8-bit 7.3 codes, at
  ClipColor highlights. No version tag: **every saved 8/16-bit Photo Filter renders differently, now within that fit of Photoshop's render.**
- **Destructive Image ▸ Adjustments at 32 bits (slice 3c)**: `float_adjust_apply` applies the layer's own
  form (`apply_linear`; measured against Photoshop, each op applied gives the same result as its layer:
  `judged.json`, Applied-same), then the straight alpha-preserving coverage lerp with no clamp (master `mix_codes`), through
  `rebuilt_rgba32_extended`. The applied set is the operator table's `*_apply` rows (Levels, Black & White (Tint
  included), Channel Mixer, Photo Filter, Exposure, point-form Curves since slice 3d, and `bcs` in its legacy and modern forms: destructive equals layer by decision, and the modern form passes values outside 0…1 through its power stages unchanged), a `document_v2` White Balance refuses (no Photoshop counterpart to measure), and an op whose layer
  form refuses keeps that sentence. Host: `FloatDocument.destructiveOps`; Levels…, Curves… and the adjustment sheets
  on a layer, MCP `apply_filter` on a layer. Admission measured at 24 MP (E ≈ 32 B/px + 2 MB; plan 40 B/px plus fixed
  terms, ⌈1.25·max E⌉ = 962,499,250 pinned by `float_adjust_apply_24mp`; table in the module doc). Oracle
  `core/tests/float_adjust_apply.rs`; drive gate F17.
- **Readouts**: above-white (`v > 1`) / below-zero (`v < 0`) instead of clipping. Sample
  (`PrecisionImage::float_sample`) with `stops = log2(max(v, 2^-16))`; histogram stop-binned, 256 bins over
  −12…+6 EV, channel 4 = Rec.709 luma of linear components, plus min/max/zero/above/below counts. The
  histogram and a weighted sample are the float arm of the schema2 scoped analysis (`float_stats::evaluate`), so
  Composite, Active layer and Below active layer, with selection weighting, answer at 32 bits; RGB eligibility is
  alpha > 0, and the bins and the `*_weight_units_rgba` twins carry each pixel's coverage weight (the plain
  `exact_zero_rgba` / `below_zero_rgba` / `above_white_rgba` counts are unweighted).
- **Source-plane selection**: Load Selection from the
  Red/Green/Blue/luminosity plane or a layer's transparency (`alpha` / `luminance` / `color` kinds,
  `source_plane_selection` in `ADMITTED`) maps each f32 sample to u16 coverage by **clamping the scene-linear value**:
  `unit_to_u16(v) = round(65535 · clamp(v, 0, 1))`, alpha the same, whatever the tone map; luminosity is the profile's
  Y row over the UNclamped samples, only the sum clamped. Measured against Photoshop 2025 26.10.0 before any code
  (`scripts/photoshop-probe/float_selection.py`, the rule fixed in advance; `photoshop.json` sha256
  `45b48832c71748cb64c0158da1d668cc3cde0ff15cb9a13aec99e4e8a5610d3f`): Red/Green/Blue over 13 neutral patches took
  the **exact branch**, **E_LIN = 0.00196** (Photoshop's 8-bit selection quantisation) against **E_ENC = 0.281**
  for the tone-mapped display encode; coloured patches read `clamp(component)` too. The patch table and the
  readback calibration are in the archived original. Luminosity: the take
  (`float-selection-442/take1`) matched sRGB and ROMM exactly.
  Magic Wand, Color Range, Select Similar and Quick Selection refuse at plan by name
  (`float_document::source_selection_refusal`) — an above-white highlight would collapse to 65535 and flood. Oracle:
  `core/tests/float_source_selection.rs`; drive gate F13.
- **Native v40** (`core/src/float_native.rs`): `40` outermost, next word **must** be `39`; body storage tag **2**
  (admitted only under 40, so earlier readers refuse rather than misread); raster payload `u32 w, u32 h, u64 w·h·16,
  raw LE f32 RGBA` (PNG has no float); tail `"FLOT" | schema 1 | operator (0 clip / 1 reinhard) | f32 white`, all
  validated before any pixel decode. The reader builds via `raster_float` + `from_parts_float` and skips the
  `with_evaluation` step. u16 documents' bytes unchanged. History, snapshots, archive (`storage: "rgba32_float"`) and
  recovery ride the `.rz`.
- **Interchange**: `open_preserving` branches `DRDC` → native, a float TIFF, EXR or `.hdr` → float document, else
  the unchanged u16 codec. `bit_depth: 32` export = 4 samples, `ExtraSamples=2`, Deflate, no predictor, document
  profile and packets; requires `tiff|exr|hdr`, `size.mode: none`, no `sharpen`, `profile: document`,
  `dither: none` (each refused by name). `export16` refuses float documents.
- **Host**: Image ▸ Tone Map… (`ToneMapSheetController`, live preview; one undo step); Convert Precision offers
  16/8-bit; a float TIFF opens only as float (`precision: "rgba8"` refused); Export offers/defaults "32-bit float";
  MCP `get_tone_map` (`nonRecordable`) / `set_tone_map` (step), `get_document.float`, `summary.precision =
  "rgba32_float"`. Tool rail admits selection, navigation and eyedropper tools; selection work (canvas coverage) is
  admitted identically in `FloatDocument.tools` and the editor whitelist.
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
it is tiled. Tests:
`core/tests/float_{document,native,interchange}.rs`; drive `scripts/verify_phase11f_float.py`.

## Shipped items
- `DONE` — filtered geometry: premultiplied extended-range
  f32 through the shared walks, with the clamp and alpha floor of Decisions, **Finite rule**; f64 oracle
  `core/tests/float_filtered_geometry.rs`.
- `DONE` — warp: the same finish over the binary64 warp
  walk, applied at once with no persisted field (a float layer still carries no WARP binding, so a float set
  compounds and Undo is the way to adjust or remove one); oracle `core/tests/float_warp.rs`.
- `DONE` — 32-bit export resampling (Decisions, **Where
  the map applies**; `core/tests/float_interchange.rs`).
- `DONE` — Load Selection from a colour plane, the
  luminosity plane or a layer's transparency at 32-bit float, UI and MCP `load_selection` / `add_channel
  from:plane` (Decisions, **Source-plane selection**).
- `DONE` — Phase A: the core refuses a float document's
  PSD report and save with one sentence naming the two routes that work (a 32-bit TIFF export, or Convert
  Precision to 16-bit with Preserve Layers), before any inventory (`float_document::psd_refusal`, report `depth`
  32); File ▸ Export as PSD… stays enabled and shows it before any panel.
- `DONE` — `layer_add`, stamp/flatten, exact canvas ops admitted (12E-2b/2c, 12F-1/1b; `precision_capability.rs:100`).
- `DONE` — Quick Mask Fill at 32-bit float: Fill…, its
  five chords and `fill_selection` fill the native u16 coverage as one file-clean "Fill" step
  (`DocumentPrecision.refusal` asks the Quick Mask coverage exemption before the float table).
- `DONE` slice 1 (`03ad6f0`) — nine blend modes at 32 bits, measured against Photoshop (`blend_float.rs`; receipt `float-blend-473/take3`; oracle `float_blend_modes.rs`, F14).
  Linear Dodge/Subtract unclamped, Hue…Luminosity in the document's linear primaries with no ClipColor; Photoshop's 128/255 half opacity and
  partially transparent backdrops are set aside.
- `DONE` slice 2 (`67385ec`) — layer styles, Blend If and the global light at 32 bits (`style_float`, `float-styles-473/take5`/`take6`; F15).
  Effect colours are the document's LINEAR numbers (`LinearInk`), the effect menu is the layer set, Blend If reads stored values unclamped.
- `DONE` slice 3a — Levels, `bcs`, Black & White without
  Tint, Channel Mixer and Photo Filter adjustment layers at 32-bit float, each under its fitted form (Decisions,
  **Adjustments at 32 bits**), rendered (full and regional), merged, saved and pasted; the refusal table names every
  other op with its reason; Channel Mixer's 1/1024 truncation at every depth.
- `DONE` slice 3b — Exposure raises to `1/gamma`
  at every depth (`adjust_tone`; take2: `1/g` fits 9.5e-8 at 32 bits where `g` misses by 3.07, and 9e-5 / 3.9e-3 on
  the 16-bit ROMM twin where `g` misses by 0.163 / 0.190), so gamma > 1 brightens, as Levels does. The stored `gamma`
  is Photoshop's parameter (PSD `expA` and `.eap` map it verbatim), with no version tag: **every saved `.rz`, Action
  or imported PSD Exposure with gamma ≠ 1 renders differently, now as Photoshop renders it.** The float form is
  `Exposure::apply_linear` per stored channel for `document_v2` and `legacy_srgb` alike (`FloatForm::Exposure`).
  The Exposure sheet keeps its 9.99 → 0.01 axis, so dragging right darkens; the caption says so.
- `DONE` slice 3c — destructive Image ▸ Adjustments at
  32-bit float for Levels, Black & White without Tint, Channel Mixer, Photo Filter and Exposure, bit-exact with the
  layer render at full coverage (Decisions, **Destructive Image ▸ Adjustments at 32 bits**); `bcs` and White
  Balance refuse the apply by name.
- `DONE` slice 3d — Curves is the NATURAL cubic spline
  at every depth (`adjust_curves::Spline`, the one spline for the 8-bit table, the 16-bit evaluation and
  float; take2 against Photoshop: PS16 within 4.4e-5, PS32 within 1.9e-7, where Fritsch–Carlson missed by 0.214),
  and a point-form Curves adjustment layer and Image ▸ Adjustments ▸ Curves… work at 32-bit float with the
  measured end rules (`Spline::value_float`); the Curves editor draws the same spline. The map (pencil) form followed
  (its own `DONE` item below). No version tag: **every saved `.rz`, Action, preset
  or imported PSD curve with 3 or more points renders differently, now as Photoshop renders it** — about 0.0056
  (1.4 codes, 2 in the rounded 8-bit table) on take2's S-curve, more on steep curves; two-point curves are
  unchanged. Oracles `core/tests/float_adjustments.rs`, `core/tests/precision_tone_accuracy.rs`; drive gates
  F16/F17.
- `DONE` slice 4 (`89e7dbd`) — Pass Through groups at 32 bits (`float-pass-through-473/take1`, outcome a; F18).
  The float render runs `precision_render`'s `Kind::PassThrough`, as the 8/16-bit master does (measured against Photoshop: outcome a); no `.rz` change;
  premultiplied vs straight is unmeasured.
- `DONE` phase 187a — placed images (no live filter) at
  32-bit float, with phase 187a0 (11E linear light holds a retained source without a live filter) first. Measured by
  `scripts/photoshop-probe/float_placed.py` (receipt `float-placed-473/take1`, valid): Photoshop 26.10.0 keeps a
  placed image a smart object at 32 bits, converts it UNCLAMPED, resamples a transform in LINEAR light, re-renders
  from the original, and keeps a pasted 16-bit smart object placed. `retained_source_float` renders the 8/16-bit
  master that way (`promoted_image` Convert, then `transform_image` on the f32 plane), with no depth authorization;
  `placed_image` is admitted; the layers paste keeps an unfiltered placed record; Preserve Layers narrowing carries
  the binding; `.rz` needs no change. `FLOAT_SOURCE_MARGIN_PER_PIXEL` = 0 here, 26 since the host stills and RAW item
  below (24 MP place / 150 % re-render / paste admit 1.50–3.12× the measured peak; the re-render rows re-measured,
  their plan larger since the later batch that charges the placed raster again). Oracles
  `core/tests/float_placed_image.rs`, `core/tests/evaluation_retained_source.rs`; drive gates F19 and 11E E7.
- `DONE` — an 8-bit placed image in a LUT RGB profile
  renders at 32 bits through its table, UNCLAMPED, then resamples linear (receipt `float-lut-placed-601/take1`: H-ext,
  H-lin; `lut_promoted_image`). Margin 0 (24 MP: 1.72× / 3.12×). Oracle `float_placed_image.rs`; drive F31. A LUT
  raster paste refuses, raised-black LUT BPC is unmeasured, and there is no 16-bit LUT master.
- `DONE` — host stills, movie frames and camera RAW layers at
  32 bits render their 8/16-bit master into linear light as a placed image does (receipts `float-host-602/take2`
  V-SDR; `float-raw-602/take2` R-SDR by ruling, Adobe Camera Raw rolling highlights off below white). Phase 1 admits Place
  Live Photo, Select Retained Frame and four tools; phase 2 a pasted RAW layer's Redevelop, presets and settings, Sync,
  decoder migration and locals (13 tools, five menu items; no handler change). `FLOAT_SOURCE_MARGIN_PER_PIXEL` 0 → 26
  (24 MP 150 % redevelop's render read 1.001×; now 1.25×). Oracles `float_placed_image.rs`, `float_raw_source.rs`;
  drives F33, F34.
- `DONE` phase 187b — text and shape layers at 32-bit
  float. Measured by `scripts/photoshop-probe/float_artwork.py` (receipt `float-artwork-473/take2`, valid): Photoshop
  26.10.0 renders a stored type/shape colour number as LINEAR light (H-raw: SH32 reads c/255 to 3e-8), keeps the
  16-bit antialias coverage (C-same, worst 1.5e-5), mixes two inks in linear light (H-lin), and keeps both kinds through
  a paste into 32 bits and Image ▸ Mode 32 → 16 with the numbers rewritten to look the same (PSH 6.9679, 55.0442,
  147.2818; NR 117.7647, 187.8444, 229.1000). `artwork_render::render_float` (the shared loop, numbers through the
  sRGB → document matrix with no decode, glyph RGBA8 decoded through sRGB) behind `dr_artwork_render_float`; the
  typed publication takes the f32 image; `text_layer`/`shape_layer` admitted and the description row gone; the
  layers paste keeps a description (Preserve only in the destination's profile bytes) and Preserve Layers narrowing
  keeps it with a note, the bytes verbatim. The host writes description v4 (`"ink":"linear_srgb"`) at 32 bits and
  converts on read and before render/publish (`ArtworkInk`). `floatMarginPerPixel` = 15 (24 MP add/edit admit 1.28×
  and 2.10× the measured peak). Oracles `core/tests/float_artwork.rs`; drive gate F20.
- `DONE` — a floating-point TIFF placed in a float
  document keeps a 32-bit float master (`SourcePixels::Rgba32F`; Photoshop's PL32), converted by the clipboard matrix
  and resampled linear; `.rz` word 67 (raw f32 RSRC asset, depth 32). 8/16-bit documents refuse it; narrowing drops
  its binding. 24 MP admission held with no margin (140 %; at 150 % a 24 MP f32 master exceeded the native 1.6 GB decoded cap,
  since raised to 8 GiB); `scratchPerPixel32` 60.
  Oracles `core/tests/float_placed_master.rs`, drive F22.
- `DONE` phase 187c3 part 2 — live filters in the host
  at 32 bits: `get_precision` `live_filters`, the sheet, Filters ▸ Live Filters / Presets / Remove and the MCP tools
  (`.floatTool`, so Actions replay) read operators and modes from the core's table; Reduce Noise, Dust & Scratches
  and Divide show disabled with the core's sentence, the menu-less modes are named in a note. 24 MP edit admits 2.49×+
  the measured peak (margin 0); float preview charge 24 → 40 B/px (pu binds, 1.26×). Pin `float_live_filter_24mp`; F21.
- `DONE` phase 187c3 part 1 — take3
  (receipt `float-live-filter-473/take3`, valid, take2
  reproduced): all twelve 32-bit smart-filter blend modes read their LAYER formula on linear values (worst 5.5e-5); High
  Pass r8 keeps −0.083…1.083 (`extended`). `blend_of(filter, range)` uses `blend_float::blend_kind_at` at float;
  eleven `live_filter_<mode>_blend` rows admitted; Divide refuses (`REFUSED_LIVE_DIVIDE`); the "unmeasured" row
  is gone. Host wiring is part 2. Oracle `admitted_filter_blends_match_their_oracle`.
- `DONE` phase 187c2 — live filters at 32-bit float, core
  only. Measured by `scripts/photoshop-probe/float_live_filter.py` (receipt `float-live-filter-473/take2`, valid):
  Gaussian, Unsharp Mask and High Pass filter in LINEAR light with the range kept (P3 red plateau 1.22494, Unsharp
  overshoot 1.7226), High Pass pivots at linear 0.5, Unsharp's threshold is linear; the filter's opacity and Multiply
  mix linear; Reduce Noise, Dust & Scratches and Screen are refused; a 16 → 32 paste keeps it linear, 32 → 16 keeps
  it encoded. `retained_source_float::source_float` runs the one `live_filter_evaluate::filtered` (generic over
  `StageSample`, `CombineRange::Extended`, no clamp) on the linear plane before the transform; five `live_filter_*`
  rows admitted; refused rows: Reduce Noise and Dust & Scratches (take2), the twelve menu-less modes (take2 SC32), and
  the twelve modes whose 32-bit arithmetic is unmeasured (187c3's first step). High Pass beyond [0, 1] is unmeasured
  and renders unclamped. The layers paste keeps an admitted stack; Preserve Layers narrowing carries it and the result
  evaluates Encoded with a note; `.rz` needs no change. Admission is structural (187c3 measures it).
  Oracles `core/tests/float_live_filter.rs`.
- `DONE` — Fill… / Stroke…, the ⌫ fill chords, MCP
  `fill_selection` / `stroke_selection` and the layer-edge stroke at 32-bit float on a layer, its mask or an alpha
  channel: `float_paint`, the f32 arm of `paint_blend` (linear ink through the profile's own TRC, never boosted;
  the five admitted modes; f64 oracle `core/tests/float_fill_stroke.rs`). The bucket, Dodge and Burn
  stay refused (`REFUSED_PAINT`); the Brush, Eraser and Gradient paint since the v1 float batches ([v1-float-editing.md](v1-float-editing.md)).
- `DONE` — float scoped statistics for the Info panel
  and MCP `histogram` / `sample_pixel` / `sample_color` (Decisions, **Readouts**); the composite counts the
  retained float projection.
- `DONE` — MCP `histogram source:"scalar"` at 32-bit
  float: a mask or `channel:<name>` measures through the 16-bit native scalar counter (`storage: "u16_scalar"`);
  red/green/blue/alpha refuse by name at plan in the core (`ScalarStatistics::plan`; `PlaneRead::plan` for
  `read_plane` on f32 storage).
- `DONE` — Photo Filter at 8/16 bits is the D50 PCS multiply
  and Luminosity blend measured against Photoshop (Decisions, **Adjustments at 32 bits**); the 32-bit form is
  unchanged. Every saved 8/16-bit Photo Filter layer re-renders (no version tag: the stored parameters are
  Photoshop's own). Oracle `core/tests/adjust_photo_filter_tests.rs`. Residuals: 8-bit ClipColor highlights
  ≤ 7.3 codes, ROMM toe 3.4e-3.
- `DONE` — Move and Free Transform of a layer's selected pixels at f32: one walk, D3 clamp; admission `precision_selected_pixels.rs`.
- `DONE` — above white at 32 bits, row B1 of `scripts/photoshop-probe/hdr_colour.py` (`hdr-colour-317-608/take2`):
  the eyedropper picks the float mean into the foreground, above white kept; text/shape colour takes RGB 0–65504
  (core, the sheets' Intensity, MCP); 32 → 16/8 clips it at 1 (N-clip).
- `DONE` — Curves in map (pencil) form at 32-bit float, layer and
  apply: each map straight between its entries on `X = 255·v`, first/last segments continued, channel map first (`map_value_float`;
  `float_adjust_take3.py`, receipt `float-adjust-473/take3`; oracle `float_lookup_tint_map.rs`; F16/F17 `curves_map`). The same take
  pinned a device-link Color Lookup `tetrahedral` (built since) and matched no Black & White Tint form (take6 since).
- `DONE` — Black & White's Tint is SetLum of the tint's NUMBERS to the grey with ClipColor (`blend::set_lum_within`,
  `BlackAndWhite::apply`/`apply_float`; take6, Ruling 1): 8/16 bits `setlum-clip16`, `+1/32768` after a low clip over
  a grey above 0; 32 bits admitted (layer, apply, paste) with the grey capped at 16 (take6's fit; take7 stalled).
  Residuals: 8 bits 3 codes, 16-bit high-only minimum 2 15-bit codes. Oracle `black_and_white_tint.rs`; drive F35.
