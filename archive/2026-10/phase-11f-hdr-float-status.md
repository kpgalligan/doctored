# HDR and 32-bit float document workflow (ADR)

- **Status:** Accepted — open items remain (deliberately bounded operator set)
- **Phase / dates:** 11F, shipped 2026-09-18 (drive gates F1–F10 pass); admitted set widened by 12E-2b/2c and 12F-1/1b
- **Original:** [archive/phase-11f-hdr-float-status.md](../archive/phase-11f-hdr-float-status.md) — full history, measurements, receipts
- **Key commits:** `a3e4f2c`
- **Carryover:** closes **P7-25** (HDR/extended-range storage, editing, output) and **P7-36** (float TIFF interchange)

## Context
Genuine HDR/scene-linear input (above-white and negative wide-gamut values) had no home: the typed document stored
u16 only. The plan required preserving real float input through editing, native/history/recovery and float TIFF
I/O, with HDR readouts and deliberate tone-mapped SDR output — qualified on genuine float data, not a promoted SDR
image. Byte and u16 modes had to stay byte-identical. Builds on [11D](phase-11d-format-additions-status.md)'s
`tiff_float.rs` seam and [11E](phase-11e-linear-light-status.md)'s evaluation policy.

## Decisions
- **Storage**: straight RGBA32 float (`PrecisionStorage::Rgba32Float`, which 7A already had). RGB = **linear light in
  the document profile's primaries**; TRC applied only at SDR encode. Straight alpha in [0, 1]. Finite,
  `|v| ≤ FLOAT_RANGE_MAX = 65504` (largest finite half, so any sample survives a half round trip). **Negatives are
  stored** and admitted by every admitted operator; mapped to 0 only at display/SDR output.
- **No promotion** from u16/byte into float — no HDR information exists to add; `from_image` with F32 still refuses.
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
  out-of-range result **refuses** (`from_rendered_float(extended: true)`, `from_rgba32_extended`). ONE named
  exception (D3): filtered geometry clamps each
  output RGB component to the source layer's own visible range (per channel over alpha > 0) and drops an output
  alpha below 2^-24 to transparent (`sampling::EXTENDED_ALPHA_FLOOR`, alpha 0 and RGB 0), because a kernel's
  overshoot is a property of the filter, not of the document. The floor is 2^-24, not the renderer's 2^-96: it is
  below one f32 ulp at 1.0 and far below a 16-bit code (the light dropped per pixel is at most 2^-24 × the layer's
  largest RGB: invisible for ordinary values, a few display codes at an extreme HDR edge), and alpha × any mask coverage
  (≥ 2^-16) × any UI opacity stays far above the 2^-96 render floor. At 2^-96 Lanczos tails left alphas near
  2e-29 that a mask pushed under the floor, and the whole document refused to render (`checked_alpha`); the 7A
  refusal rule itself is unchanged.
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
  exact** at size none — no map; a size mode resamples the rendered composite through the extended-range resize,
  RGB clamped to the composite's own visible range.
- **Admitted operators** — the live table is `FLOAT_FAMILIES` in `core/src/precision_capability.rs:100` (and
  `float_document::float_operators_json`). At 11F: Normal/Multiply/Darken/Lighten/Difference (the `blend.rs`
  kernels with no clamp, called unchanged), opacity/name/visibility/clipping/duplicate/group-all, masks and
  channels, Exposure and White Balance at `document_v2` via `Calibrated::apply_linear` (no TRC round trip), readouts,
  render, convert, export, native save, `set_tone_map`. Later: `layer_add` (12E-2b), `layer_stamp` /
  `layer_flatten` (12E-2c), exact canvas ops `geometry_canvas_size` / `_crop` (zero angle) / `_rotate` (quarter
  turns) / `_flip` (12F-1/1b), plus lock/link/merge/label/comp/move/group families. Everything else refuses
  `"<operator> is unavailable in a 32-bit float document"`; the refusal walk (`float_document::float_refusal`)
  asks the kernel (`apply_linear(..).err()`) rather than re-listing adjustments.
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
  `unit_to_u16(v) = round(65535 · clamp(v, 0, 1))`, alpha the same, whatever the tone map; luminosity stays Rec.709
  over those codes (the 8/16-bit kernel). Measured against Photoshop 2025 26.10.0 before any code
  (`scripts/photoshop-probe/float_selection.py`, the rule fixed in advance; `photoshop.json` sha256
  `45b48832c71748cb64c0158da1d668cc3cde0ff15cb9a13aec99e4e8a5610d3f`): readback calibration through a layer's
  transparency at 100/75/50/25/10/1 % read 1.0/0.74902/0.50196/0.25098/0.10196/0.01176 (max error 0.00196);
  Red/Green/Blue over 13 neutral patches, **exact branch**, **E_LIN = 0.00196** (Photoshop's 8-bit selection
  quantisation) against **E_ENC = 0.281** for the tone-mapped display encode:

  | v | −1 | −0.25 | 0 | 0.01 | 0.05 | 0.18 | 0.5 | 0.75 | 0.9 | 1 | 1.5 | 4 | 100 |
  | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
  | Photoshop R=G=B | 0 | 0 | 0 | 0.01176 | 0.05098 | 0.18039 | 0.50196 | 0.74902 | 0.90196 | 1 | 1 | 1 | 1 |

  Red/Green/Blue on coloured patches also read `clamp(component)` within 0.002. Photoshop's RGB-composite load
  diverges on coloured patches (it weights UNclamped components, apparently by the profile's Y row);
  the luminance weights and order are unchanged here.
  Magic Wand, Color Range, Select Similar and Quick Selection refuse at plan by name
  (`float_document::source_selection_refusal`) — an above-white highlight would collapse to 65535 and flood. Oracle:
  `core/tests/float_source_selection.rs`; drive gate F13.
- **Native v40** (`core/src/float_native.rs`): `40` outermost, next word **must** be `39`; body storage tag **2**
  (admitted only under 40, so earlier readers refuse rather than misread); raster payload `u32 w, u32 h, u64 w·h·16,
  raw LE f32 RGBA` (PNG has no float); tail `"FLOT" | schema 1 | operator (0 clip / 1 reinhard) | f32 white`, all
  validated before any pixel decode. The reader builds via `raster_float` + `from_parts_float` and skips the
  `with_evaluation` step. u16 documents' bytes unchanged. History, snapshots, archive (`storage: "rgba32_float"`) and
  recovery ride the `.rz`.
- **Interchange**: `open_preserving` branches `RZDC` → native, `tiff_float::inspect` accepts → float document, else
  the unchanged u16 codec. `bit_depth: 32` export = 4 samples, `ExtraSamples=2`, Deflate, no predictor, document
  profile and packets; requires `format: tiff`, `size.mode: none`, no `sharpen`, `profile: document`,
  `dither: none` (each refused by name). `export16` refuses float documents.
- **Host**: Image ▸ Tone Map… (`ToneMapSheetController`, no live preview; one undo step); Convert Precision offers
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
`FLOAT_FAMILIES` entry; exact permutation/copy ops may be admitted on that argument alone (12F precedent). No f32
plane above `CONTIGUOUS_CACHE_BYTES` is admitted (13B-3e-4c-2b residual; tiling question in 13B-8). Tests:
`core/tests/float_{document,native,interchange}.rs`; drive `scripts/verify_phase11f_float.py`.

## Open items (verified 2026-09-26)
- `TRACKED` — blend modes beyond the five (per-mode unclamped kernel with f64 oracle, or a stated clamp domain),
  paint/erase, healing/clone/CAF, styles and Blend If, non-calibrated adjustments, Assign/Convert
  profile, retained sources/live filters/artwork, tolerance/distance source selections (Magic Wand, Color Range,
  Select Similar, Quick Selection), Pass Through groups → plan §11 "Gate assessment" NOT MET (3). Filtered geometry is `DONE`:
  premultiplied extended-range f32 through the shared
  walks, checked against an independent f64 oracle (`core/tests/float_filtered_geometry.rs`). Warp is `DONE`:
  the same finish over the binary64 warp walk, applied at
  once with no persisted field (a float layer still carries no WARP binding, so a float set compounds and Undo is the
  way to adjust or remove one); oracle `core/tests/float_warp.rs`. 32-bit export resampling is `DONE`
  (`core/tests/float_interchange.rs`).
- `DONE` — Load Selection from a colour plane, the
  luminosity plane or a layer's transparency at 32-bit float (UI and MCP `load_selection` / `add_channel from:plane`),
  clamping scene-linear samples, measured against Photoshop (Decisions above).
- `TRACKED` — RAW-developed HDR fixture (+2 EV development opened as float with highlights > 1.0) → plan §11 gate (3).
- `TRACKED` — F16 TIFF export, float-predictor encoding (owned writer checked by `tiffinfo`), OpenEXR → P7-36 row
  note and plan §11 gate (3).
- `TRACKED` — EDR display output (measured on a capable display); luminance/ACES tone maps → plan §11 gate (3).
- `DONE` — Phase A: a float document's PSD report and save are refused by the core in both modes
  with one sentence naming the two routes that work (a 32-bit TIFF export, or Convert Precision to 16-bit with Preserve
  Layers), before any inventory (`float_document::psd_refusal`, report `depth` 32); File ▸ Export as PSD… stays
  enabled and shows it before any panel.
- `DONE` — `layer_add`, stamp/flatten, exact canvas ops admitted (12E-2b/2c, 12F-1/1b; `precision_capability.rs:100`).
- `DONE` — Quick Mask Fill at 32-bit float: Fill… and its five
  chords validate in Quick Mask and fill the native u16 coverage as one file-clean "Fill" step, and `fill_selection`
  does the same there (`DocumentPrecision.refusal` asks the Quick Mask exemption before the float table; since the Fill / Stroke item below,
  `fill_selection` is in `FloatDocument.tools` and fills outside Quick Mask too). Coverage gray is the existing rule. Proof: jobs-probe "float quick mask fill".
- `DONE` — Edit ▸ Fill… / Stroke…, the ⌫ fill chords, MCP
  `fill_selection` / `stroke_selection` and the layer-edge stroke at 32-bit float on a layer, its mask or an alpha
  channel. The core's `float_paint` is the f32 arm of `paint_blend` (linear ink decoded through the profile's own
  TRC, never boosted; the five admitted modes through the unchanged `composite_source_into`; an f64 W3C oracle over
  extended-range input with negatives in `core/tests/float_fill_stroke.rs`), `fill_stroke` is in `ADMITTED` and
  `FLOAT_FAMILIES` (raster, mask, channel), and both tools are now in `FloatDocument.tools`. The brush, Eraser,
  bucket and gradient stay refused (`REFUSED_PAINT`). Colour planes refuse by name
  (`FloatDocument.colorPlaneFillRefusal`).
- `DONE` — float scoped statistics: the Info panel histogram
  and readout and MCP `histogram` / `sample_pixel` / `sample_color` answer Composite, Active layer and Below active
  layer with selection weighting at 32 bits through `rz_precision_doc_scoped_analysis` /
  `rz_precision_image_scoped_analysis` (`float_stats::evaluate`); the composite counts the retained float projection.
- `DONE` — MCP `histogram source:"scalar"` at 32-bit float:
  a mask or `channel:<name>` (u16 coverage) measures through the 16-bit native scalar counter with the same reply
  (`storage: "u16_scalar"`); red/green/blue/alpha refuse by name at plan, before any admission, in the core
  (`ScalarStatistics::plan`; `PlaneRead::plan` refuses `read_plane` components on f32 storage the same way). The
  host-side float refusal and its handler are deleted; `histogram` goes straight to `scopedHistogram`.
- `DROPPED` — promotion of u16/byte to float (never, by decision); `PrecisionStateArchive.swift` float coverage (out
  of scope; internal probe archive only).
