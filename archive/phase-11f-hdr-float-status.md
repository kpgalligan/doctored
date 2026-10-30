# Phase 11F — HDR and floating-point document workflow

Owns carryover rows **P7-25** (HDR/extended-range storage, editing and output)
and **P7-36** (floating-point TIFF interchange). Builds on 11D's pure float
TIFF seam (`core/src/tiff_float.rs`) and 11E's evaluation policy
(`core/src/precision_evaluation.rs`).

This slice adds a **32-bit float document mode** to the typed
`PrecisionDocument`: extended-range linear light, a bounded admitted operator
set, native v40 persistence, float TIFF in and out, and a selectable tone map
for every SDR output. The byte and u16 modes are byte-identical — every new
branch is behind `float.is_some()` or F32 storage.

---

## 1. The contract

Every statement here is a **recorded decision**, made under the delegation in
`request.md`, with its reasoning.

### 1.1 Storage

| Question | Decision | Why |
|---|---|---|
| Sample type | Straight RGBA32 float (`PrecisionStorage::Rgba32Float`), the storage 7A already had and never exposed | No new pixel container; the compositor, the plans and the resource accounting already understand it. |
| Colour meaning | RGB is **linear light in the document profile's primaries**; the TRC is applied only when SDR output is encoded | A scene-linear document is the only kind for which "above white" is a physical statement rather than a code overflow. It also makes the mode 11E's `linear_light` policy by construction rather than a second policy beside it. |
| Alpha | **Straight**, in [0, 1] | Matches every other document in this build; an associated-alpha file is unpremultiplied at import and disclosed. |
| Range | Finite, `\|v\| ≤ FLOAT_RANGE_MAX = 65504.0`; alpha in [0, 1] | 65504 is the largest finite IEEE half. A document whose samples all survive a half-width round trip is one a host can hand on to any float format in this build; a larger bound would buy range no interchange format here can carry. |
| Negatives | **Stored and admitted by every admitted operator.** Display and SDR output map them to 0 | Wide-gamut colours outside the document's primaries are real data; clipping them at storage would make the mode a worse container than the u16 one it replaces. |
| Promotion | There is **no** conversion from u16/byte INTO float | No HDR information exists to add. `PrecisionDocument::from_image` with an F32 image still refuses with the u16 sentence. |

The **ONE storage gate** is `PrecisionDocument::build(.., float)`: every raster
leaf's `storage()` must agree with `float.is_some()`, or the build refuses with
`"high-depth document storage does not match the document mode"`.
`PrecisionDocumentLayer::raster` keeps its u16 refusal unchanged, and
`raster_float` (`pub(crate)`) is the only F32 leaf constructor — reachable only
from `float_io::from_float_tiff` and the v40 native reader.

### 1.2 The evaluation invariant

A float document **always** has `linear_light == None` and `float == Some(..)`.
`evaluation()` answers `LinearLight` from `float` alone, so:

* the writer and the native plan (which key 39 / `EVAL` on `evaluation()`)
  always emit both words, and no 2.36 MB `TypedTransfer` is ever allocated;
* `with_evaluation(LinearLight)` is `Ok(None)` — a no-op, never a phantom undo
  step;
* `with_evaluation(Encoded)` is
  `Err("evaluation is fixed to linear light in a 32-bit float document")`;
* `rebind_evaluation` is a no-op, which is consistent because profile edits are
  refused on a float document anyway.

`linear_light_refusal()` returns `float_refusal()` first on a float document.
That is what makes the two host guards 11E already installed
(`ImageDocument.applyEdit` and `AgentServer.performGroupedEdit`) refuse an
unadmitted float edit **with no new guard anywhere**.

### 1.3 The finite-value rule

At **import only** (`float_document::sanitize`, called once by `float_io`):

| Input | Stored | Counted as |
|---|---|---|
| NaN | 0 | `non_finite` |
| +∞ / −∞ | +65504 / −65504 | `non_finite` |
| `\|v\| > 65504` | ±65504 | `clamped` |
| alpha outside [0, 1] | clamped to [0, 1] | `clamped` |

The counts become import warnings — `"{n} non-finite samples replaced"` and
`"{n} out-of-range samples clamped"` — so the repair is disclosed rather than
silent. **Every other stage refuses instead of repairing**: a render, an edit
or a native record whose result is not finite and in range fails by sentence
(`PrecisionImage::from_rendered_float(extended: true)`,
`from_rgba32_extended`). 11D's seam deliberately left this policy open; this is
it.

### 1.4 Alpha at import

`ExtraSamples = 1` (associated) → unpremultiplied (`rgb/α` for α > 0, the
canonical zero at α = 0) with the warning `"associated alpha unpremultiplied"`;
`2` (unassociated) → taken as is; absent → alpha 1.0. One or two samples are
grayscale and fill R = G = B — there is no curve to undo, since the samples are
already linear.

An embedded ICC must be an RGB matrix/TRC profile this build can model
(`IccProfile::parse` + `model().is_some()` + `validate_precision`), because the
SDR encode runs its forward curve; anything else refuses by sentence. No
profile at all means sRGB primaries interpreted as linear light, with a
warning.

### 1.5 Tone maps

Applied per RGB channel to a linear value `v`; alpha is coverage and never
passes through a map. The mapped triple then goes through
`icc_transform::PrecisionToneEncoder` — the document profile's own forward
curve, the same encoder 11E uses, with no curve inverter anywhere.

| Operator | Formula | JSON |
|---|---|---|
| `clip` (default at open) | `y = clamp(v, 0, 1)` | `{"operator":"clip"}` |
| `reinhard` | `u = max(v, 0);  y = min(u·(1 + u/W²)/(1 + u), 1)` | `{"operator":"reinhard","white":4.0}` |

`W ∈ [1, 65504]`, 4.0 by default — two stops of headroom, which is what a
`clip` map loses on a scene with ordinary specular highlights. `W` maps exactly
to 1.0.

`ToneMap::parse` reads a `serde_json::Map` **by hand** rather than through a
`Deserialize` derive: an internally tagged enum accepts unknown keys on a unit
variant even under `deny_unknown_fields`, so `{"operator":"clip","white":4}`
would have been silently accepted as a plain clip and the caller would have
believed a white point was in force. `operator` is required, `white` is
admitted for `reinhard` only, and any other key — or `white` beside `clip` —
refuses with `unknown tone map field "<key>"`. The MCP handler forwards `white`
verbatim whenever the caller supplied it, so that refusal reaches the caller
rather than being dropped host-side.

### 1.6 Where the map applies

* **Display** — `PrecisionDocument::display_rgba8` (map → TRC → the existing
  undithered quantizer) is the ONLY way a float sample becomes display bytes.
  `PrecisionImage::to_rgba8` now **refuses** F32 storage by sentence, so a
  clamped preview cannot be shown by accident anywhere.
* **8/16-bit export** — `doc_export::export_stages` replaces the source with
  `tone_mapped_u16(source)` before stage 1, so the whole 11A chain then runs on
  ordinary u16 pixels.
* **32-bit export** — the map does **not** apply: the samples are written
  exactly.
* **Conversion to u16/byte** — `float_to_u16` and `downconvert` both apply it.

### 1.7 Scratch and accounting

| Quantity | Formula |
|---|---|
| Retained float raster | `w · h · 16` (the existing `PrecisionStorage::bytes_per_pixel`) |
| Float policy | the encoder's three 65537-entry f64 tables (≈1.57 MB), counted by `record_resources` |
| Render | the existing `PrecisionScene::plan` with `Rgba32Float` output |
| Native raster record | `w · h · 16` serialized **and** retained, with **no** PNG bound and no codec scratch on either side |
| Display / `tone_mapped_u16` | `+ w · h · 12` (the u16 plane plus bookkeeping) |
| Export plan | `+ source · 8` on a float document, plus `source · 16 · 2 + 1 MiB` at bit depth 32 |
| Float TIFF import | the seam's own plan plus `w · h · 16` |

---

## 2. The admitted operator set

Admitted (depth 32; the core's own table is `float_document::float_operators_json`
and `precision_capability`'s `FLOAT_FAMILIES`):

* **Blending** — Normal, Multiply, Darken, Lighten, Difference. These are
  exactly the `blend.rs` kernels whose W3C formulas are defined on the whole
  real line and carry no clamp of their own. **No kernel was copied or
  modified**: the compositor calls the shipped `blend::composite_source_into`
  unchanged, and no clamp was added or removed in `blend.rs`.
* **Structure** — `layer_opacity`, `layer_name`, `layer_visible`,
  `layer_clipped`, `layer_duplicate`, `layer_group_all`, and since
  [12E-2b](phase-12e-layer-ops-status.md) **`layer_add`** (`new_layer`), the
  ONE layer-creating family a float document admits: the core's `add_layer` op
  writes f32 ZEROS through `raster_float`, which is a transparent Normal leaf
  and adds no operator, so the projection is bit-identical. `place_image` and
  the artwork tools stay refused BY NAME — the retained-source pipeline has no
  float-aware half yet (`REFUSED_RETAINED`). Since
  [12E-2c](phase-12e-layer-ops-status.md) **`layer_stamp`** (`stamp_visible`)
  and **`layer_flatten`** (`flatten_image`) join them: both render the SAME
  float projection the display already does into ONE canvas-sized `raster_float`
  leaf, so nothing new is evaluated either. Since
  [12F-1](phase-12f-geometry-status.md) **`geometry_canvas_size`**
  (`canvas_size`) joins them — the FIRST canvas-geometry op a float document
  admitted. It reads no sample: every f32 raster keeps its own allocation and
  moves its offset, every mask rides with it and every saved channel becomes a
  canvas-sized plane holding an exact copy at the origin and 0 elsewhere, so
  no resampling question arises and nothing is evaluated. Since
  [12F-1b](phase-12f-geometry-status.md) the other EXACT canvas ops join it —
  **`geometry_crop`** (a zero-angle `crop`), **`geometry_rotate`** and
  **`geometry_flip`** — for the same kind of reason: a crop keeps every raster
  and mask allocation and walks each u16 channel with Nearest, and a quarter
  turn or a flip is a Nearest PERMUTATION, so an extended-range component is
  only ever copied. STRAIGHTENING — a `crop` with a non-zero angle — is the
  one half of that tool a float document still refuses, by its own named
  sentence.
* **Masks and channels** — `mask_set` / `mask_remove` / `mask_properties`,
  `channel_set` / `channel_remove` / `channel_read`, `read_plane`
  (mask / effective_mask / channel).
* **Adjustments** — `adjustment_add` / `adjustment_set` for `exposure` and
  `white_balance` at `document_v2` evaluation only. These are already defined
  in linear light; `PrecisionAdjustment::apply_linear` evaluates them through
  `Calibrated::apply_linear` — the two reference-basis matrices and the
  tone/Bradford kernel, with **no TRC decode on the way in and no forward
  encode on the way out**, and nothing clipped.
* **Readouts and delivery** — `sample`, `histogram`, `render`, `convert`,
  `export`, native save, `set_tone_map`, `set_evaluation` (a no-op to
  `linear_light`).

Everything else refuses BY NAME with
`"<operator> is unavailable in a 32-bit float document"`. The refused table,
with the acceptance criterion that would admit each:

| Refused | Acceptance criterion |
|---|---|
| Any blend mode outside the five | An unclamped extended-range kernel per mode, checked against an f64 oracle, or a stated clamp domain |
| Paint, erase, fill, gradient | An f64 oracle for authored ink over extended-range backdrops |
| Image Size, straighten (a crop with a non-zero angle), ARBITRARY canvas rotation (a `rotate` by an angle that is not a multiple of 90 degrees, [12F-1b3](phase-12f-geometry-status.md)), free transform | A resampling oracle on extended-range input, including negative components. Canvas Size, a zero-angle crop, the quarter turns and the flips are EXACT and are ADMITTED since [12F-1b](phase-12f-geometry-status.md) — they keep every allocation or permute, so no sample is ever combined |
| Healing, cloning, spot healing, Content-Aware Fill | A membrane-solver oracle on extended-range input |
| Layer styles and Blend If | Extended-range shape/ramp semantics above white |
| Any adjustment beyond Exposure / White Balance | A per-operator f64 oracle on extended-range input; every other kernel in `adjust.rs` is defined on ENCODED numbers in [0, 1] |
| Assign Profile, Convert to Profile | A linear-light conversion oracle that keeps negatives expressible |
| Retained sources, live filters, artwork descriptions | A float-aware source pipeline |
| Source-derived selections, scoped statistics | Extended-range thresholds and counters |
| Pass Through groups | A float backdrop-sharing oracle |
| Layered PSD export | A float layer representation in the classic format |

The refusal walk (`float_document::float_refusal`) has the same postorder,
bottom-first shape `precision_evaluation::walk` uses, and the adjustment arm
asks the KERNEL (`apply_linear([0,0,0]).err()`) rather than re-listing the
admitted operators — one implementation of that question.

---

## 3. Readouts

A float document has no maximum code, so "clipping" is not a meaningful count.
The readouts report **above white** (`v > 1.0`) and **below zero** (`v < 0.0`)
instead, plus the exact minimum and maximum seen.

* **Sample** (`PrecisionImage::float_sample`) —
  `{x, y, reach, center_rgba32, mean_rgba32, sample_count, above_white[3],
  below_zero[3], stops[3], storage, schema_version}`, with
  `stops = log2(max(v, 2^-16))`.
* **Histogram** (`PrecisionImage::float_histogram`) — **stop-binned**: 256 bins
  uniformly covering −12 EV … +6 EV, `bin = clamp(floor((log2(max(v, 2^-12)) + 12)/18 · 256), 0, 255)`,
  channel 4 the Rec.709 luma of the LINEAR components. A linear histogram would
  put almost every photographic sample in the bottom bin. Reported beside the
  bins: `exact_zero_rgba`, `below_zero_rgba`, `above_white_rgba`, `min_rgba`,
  `max_rgba`, and the `{min_ev, max_ev}` window.

The Info panel routes to these on a float document; its endpoint markers report
below-zero and above-white rather than clipping.

---

## 4. Persistence — native v40

`RZDC_VERSION` is now **40**. The wrapper follows `evaluation_native.rs`'s
shape exactly (`core/src/float_native.rs`):

```
RZDC | 40 | 39 | 36 | 35 | 32 | 29 | 37 | <prior body/tails> | LBLD | MOUT | LCLP | LBIF | LSTY | EVAL | FLOT
```

* 40 is the **outermost** word, before every prior word including 39.
* Under 40 the next word **must** be 39 — a float document is always linear
  light — and anything else refuses before a pixel is read.
* The body header's **storage tag is 2** instead of 1, and tag 2 is admitted
  **only** under 40. Every earlier reader knows neither the word nor the tag,
  so it refuses the file rather than misreading its samples. **That refusal is
  the disclosure.**
* A raster record's payload is `u32 width, u32 height, u64 length = w·h·16,
  raw little-endian f32 RGBA` instead of a PNG — PNG has no float sample
  format, and the point of the mode is that a stored sample survives exactly.
* The tail table is `"FLOT", u32 schema = 1, u32 operator (0 clip / 1
  reinhard), f32 white`. A bad schema, an unknown operator, a white point
  outside the parser's own range and a trailing word all refuse **before any
  pixel decode**.
* The v40 reader builds leaves through `raster_float` and the document through
  `from_parts_float(.., tone_map)`, and **skips** the
  `with_evaluation(LinearLight)` step at `precision_native_read.rs:834`, which
  stays exactly as it was for 39-only files.
* A u16 document's bytes are unchanged, first word included.

History, snapshots, the editing-state archive and crash recovery all ride on
that `.rz`, so they carry the float samples exactly. The archive's manifest
records `storage: "rgba32_float"` and cross-checks it against the decoded
document.

---

## 5. Interchange

**In.** `PrecisionDocument::open_preserving` reads the file once and branches:
`RZDC` → the native reader; a TIFF that `tiff_float::inspect` accepts → the
float document; everything else → the unchanged u16 codec. An integer TIFF
fails `inspect` by naming its `SampleFormat`, so the u16 path keeps its own
refusals verbatim.

**Out.** `bit_depth: 32` is the float document's EXACT deliverable: four
samples, `ExtraSamples = 2`, Deflate, predictor none, the document profile and
packets from the shared `EncodeSidecar`. It requires `format: "tiff"`,
`size.mode: none`, no `sharpen`, `profile: "document"` and `dither: "none"` —
each refused by name, because every one of those stages would move a sample.
`export16` (the untone-mapped 16-bit writer) refuses a float document by name.

---

## 6. Host surface

* **Image ▸ Tone Map…** (`ToneMapSheetController`) — operator popup plus white
  field, no live preview (a float document's canvas is already the tone-mapped
  projection, so previewing a candidate would render it twice per popup
  change), Apply is one undo step.
* **Mode ▸ Convert Precision…** asks 16-bit or 8-bit on a float document; both
  apply the document's own map.
* **Open** — a float TIFF's import alert offers only "Open as 32-bit float" and
  Cancel, and `precision: "rgba8"` on one is refused by sentence.
* **Export** — the workspace sheet offers "32-bit float" only on a float
  document, and defaults a float document to it.
* **MCP** — `get_tone_map` (read-only, `nonRecordable`) and `set_tone_map` (one
  undo step, an Action step, `unchanged: true` when equal); `get_document`
  gains a `float` block and `summary.precision` becomes `"rgba32_float"`;
  `get_precision.mode` is `"rgba32"`; `histogram` / `sample_pixel` return the
  float JSON; `export_image` / `export_preview` accept `bit_depth: 32`;
  `convert_precision` runs `float_to_u16` for `rgba16`.
* **Tool gating** — `DocumentPrecision.refusal` asks the float table first, and
  the editor's selector whitelist and tool rail admit only the selection,
  navigation and eyedropper tools. Selection work is canvas COVERAGE, not
  document pixels, so `FloatDocument.tools` and that whitelist admit the same
  set on both sides: the marquee/lasso tools, Select All / Deselect /
  Reselect, `modify_selection`, Quick Mask, Save and Load Selection, the
  guide/ruler-origin tools and the read-only profile tools.

---

## 7. The fixture

`core/tests/fixtures/float_11f/scene.tiff` — 48 × 32 RGBA f32, Deflate,
`ExtraSamples = 2`, with the system sRGB ICC embedded. Linear ramps, a genuinely
above-white block at (8.0, 4.0, 2.0), a block with a supported negative
component (−0.25, 0.5, 0.75) and a half-alpha band.

**Recorded decision:** it is authored by
`python3 scripts/verify_phase11f_float.py --author-fixture core/tests/fixtures/float_11f/scene.tiff`
(stdlib `struct` + `zlib`, the same code the drive's parser reads back), not by
`magick`. The gate needs EXACT 8.0 and −0.25 samples and ImageMagick's Q16-HDRI
pipeline scales float output by its own quantum range, so a `magick`-authored
file could not carry those numbers verbatim. The file is then verified by
`tiffinfo` and `magick identify`, which is what makes it a genuine float TIFF
rather than one this build agrees with itself about:

```
$ tiffinfo core/tests/fixtures/float_11f/scene.tiff
  Image Width: 48 Image Length: 32
  Bits/Sample: 32
  Sample Format: IEEE floating point
  Compression Scheme: AdobeDeflate
  Photometric Interpretation: RGB color
  Extra Samples: 1<unassoc-alpha>
  Samples/Pixel: 4
  ICC Profile: <present>, 3144 bytes
$ magick identify -format '%[depth] %[type] %[channels]' …   # 32 TrueColorAlpha srgba
```

This is an authored analytic scene-linear fixture, which the request lists as
an accepted alternative to a RAW development. A **+2 EV RAW development opened
as float with highlights above 1.0** stays open as the acceptance criterion for
the RAW route (see §9).

---

## 8. Verification

* **Rust.** `core/tests/float_document.rs` (tone-map parsing and formulas, the
  import rules and their disclosure, associated alpha, the storage gate, the
  evaluation invariant, the admitted and refused operators, display and the
  readouts, the FFI null sweep), `core/tests/float_native.rs` (40-then-39,
  storage tag 2, the FLOT table, bit-exact reopen with equal `retained_bytes`
  and refusal wording, every malformed-file refusal, and a u16 document's
  unchanged bytes), `core/tests/float_interchange.rs` (the exact 32-bit
  export and its reopen, the 32-bit request's five refusals, the tone-mapped
  SDR numbers against the map's own formula plus the sRGB transfer function,
  and `float_to_u16`'s 39 word and linear-light policy).
* **Drive.** `scripts/verify_phase11f_float.py`, gates F1–F10, receipts under
  `~/devel/rasterize-fixtures/phase11-receipts/11f/`. Tolerances are fixed in
  the file before anything is measured: `T_REL = 1e-5` relative for the f64
  composite oracle, 1 code of 255 for the tone-mapped PNG, 1 code of 65535 for
  the 16-bit conversion.

The results of that run are recorded in §10.

---

## 9. What stays open

| Open | Acceptance criterion |
|---|---|
| Blend modes beyond the five | An unclamped extended-range kernel per mode, oracle-tested, or a stated clamp domain |
| Paint / fill / erase, geometry, healing, styles, Blend If, non-calibrated adjustments, profile assign/convert, retained sources and live filters, artwork, source-derived selections, scoped statistics | Each is refused by name today; each needs its own f64 oracle on extended-range input |
| Promotion of a u16/byte document to float | Deliberately never: no HDR information exists to add |
| A RAW-developed HDR fixture | A +2 EV D70s development opened as float with highlights above 1.0 in the drive. There is no RAW corpus under `~/devel/rasterize-fixtures/` in this tree, so the analytic fixture is what ships |
| F16 TIFF **export**, float-predictor encoding, OpenEXR | An owned writer checked against `tiffinfo` (11D's own open item) |
| HDR (EDR) display output | A measured EDR path on a capable display |
| Luminance-based and ACES tone maps | Their own oracles; `clip` and `reinhard` are the two this slice qualifies |
| PSD export of a float document | A float layer representation in the classic format |
| `PrecisionStateArchive.swift` float coverage | Out of scope for this slice |

---

## 10. Results

**All nine drive gates pass** (receipt:
`~/devel/rasterize-fixtures/phase11-receipts/11f/receipt.json`), on the built
Release app, with the tolerances as fixed in the file:

| Gate | Result |
|---|---|
| F1 open | `precision: "rgba32_float"`, `float.range: "extended"`, `float.tone_map: {"operator":"clip"}`, `evaluation.policy: "linear_light"`, import warnings EMPTY, and every sample bit-exact against the file (8.0/4.0/2.0 above white, −0.25 below zero, the 0.5 alpha band) |
| F2 editing | Exposure +1 EV worst **2.08e-13** relative and the Multiply composite worst **1.17e-07** relative, both against `T_REL = 1e-5`; 8.0 → **16.0** and −0.25 → **−0.5** |
| F3 persistence | first two `.rz` words **[40, 39]**, reopen bit-exact, re-encode byte-identical |
| F4 snapshots / recovery | a snapshot restore and a forced-crash `recover_document` both bit-exact |
| F5 exact delivery | `tiffinfo`: IEEE floating point, 32 bits/sample, 4 samples, `1<unassoc-alpha>`, ICC present (**3144** bytes); `magick identify`: `32 TrueColorAlpha srgba 4.0`; the stdlib parser agrees and the samples are bit-exact |
| F6 tone-mapped delivery | worst **0** codes of 255 against `T_CODE8 = 1`, under BOTH maps, and the two deliverables differ |
| F7 conversion | worst **1** code of 65535 against `T_CODE16 = 1`; `precision: "rgba16"`, first `.rz` word **39**, `get_evaluation` `linear_light` |
| F8 refusals | all eight refuse by name with `get_document` unchanged — `brush_stroke is unavailable in a 32-bit float document`, `32-bit float layer blending supports Normal, Multiply, Darken, Lighten, Difference only`, `the curves adjustment is unavailable in a 32-bit float document`, `image_size is unavailable…`, `evaluation is fixed to linear light in a 32-bit float document`, `unknown tone map field "white"`, `“…tiff” is a floating-point TIFF: it opens as a 32-bit float document`, `a 32-bit float export requires a 32-bit float document` |
| F9 the u16 mode | the `.rz` and the PNG16 export are sha256-identical before and after the whole run, and the `.rz`'s first word is still **13** |
| F10 agent parity (added after review) | on a float document `select_rect`, `get_selection`, `save_selection`, `list_channels` and `load_selection` all succeed — the selection work the float menus enable is the selection work the agent answers |

The Rust suites (`cargo test --release`), `cargo fmt --check`, `cargo clippy
--all-targets -- -D warnings`, `make typecheck` and `make app` are all clean.

**One defect found and fixed during the drive.** `add_adjustment_layer` with an
unadmitted operator reached `ImageDocument.applyEdit`'s linear-light guard,
which presents a MODAL ALERT — and an MCP tool runs on the main thread, so the
call deadlocked instead of refusing. The refusal is now pre-checked in
`PrecisionEditorCapture.publish` and thrown, so it is in-band for MCP and
presented in the same alert for the UI. This was latent from 11E and is not
specific to the float mode.

**One oracle corrected during the drive.** F7's first measurement read 6 codes
of 65535 against the fixed tolerance of 1. The TOLERANCE was not moved: the
oracle was incomplete. It had used the analytic sRGB transfer function, but the
document profile carries a 1024-entry `curv` TABLE (several codes of 65535 away
from the analytic curve in the toe) and the converted document is linear-light,
so exporting it decodes and re-encodes through that same curve. The oracle now
parses the fixture's own ICC and reproduces both stages from the forward-search
rule, and the measurement is 1 code.

