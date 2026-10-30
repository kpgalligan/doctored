# Phase 11E — Versioned linear-light evaluation (phase 1 of P7-24)

Status: **implemented and qualified; all seven drive gates pass.** This is the
FIRST of three slices of carryover row **P7-24**, which stays open: the live
filter stack and resampling (phase 2) and the mixing constructs (phase 3) are
refused by name rather than evaluated, each with an acceptance criterion below.

The plan text this delivers:

> **11E — Versioned linear-light evaluation.** First specify transfer/profile,
> compositing, blending and premultiplied spatial-filter semantics with
> independent references; then add an explicit evaluation choice,
> conversion/preview and native/history/recovery tags. Existing encoded
> byte/u16 documents retain their established evaluation. Migrate and verify
> each enabled operation under the new policy before exposing it.

---

## The contract

A document carries an **evaluation policy**: `encoded` — the unchanged default,
under which the projection composites the stored code values — or
`linear_light`, under which every sample is DECODED through the document
profile's TRC, composited/blended/mixed in linear light at f32, and re-encoded
exactly ONCE at the output. It is chosen deliberately in Image ▸ Evaluation… /
`set_evaluation`, never implied by anything else.

**The projection is the invariant.** Only what this slice migrated may be
evaluated in linear light. Everything else is refused BY NAME at three places,
which is what makes the policy a property of the picture rather than of the
moment it was switched:

1. **at the switch** (`with_evaluation`);
2. **at every later edit** that would introduce one — the host's
   `ImageDocument.applyEdit` guard (an alert) and `AgentServer.performGroupedEdit`'s
   pre-check (an in-band `ToolError`);
3. **at every render**, as the backstop nothing can route around:
   `RzDocument::transfer` / `PrecisionDocument::transfer` runs the walk, and
   every `composite_env` caller and `scene_cut` obtains its transfer there, so
   `flattened`, the typed `render`, `render_plan` and `info.render_refusal` all
   carry the sentence.

An ENCODED document holds no transfer (`linear_light: None`), takes the
`transfer == None` branch everywhere, and is byte-for-byte what it was before
this slice existed. `linear_light_refusal()` returns `None` before it walks
anything. The 188 existing core test files and the drive's E6 gate are the
proof.

---

## Migrated, and refused

| Migrated | Where |
|---|---|
| Normal + the 19 separable blend modes | `blend::composite_source_into`, unchanged; only its INPUTS are decoded |
| Layer opacity, layer masks (density, feather, outside coverage, link) | the existing coverage path |
| Pass-through, isolated and clip groups | `doc_group`, `precision_render` |
| Pointwise adjustment layers, including `document_v2` exposure / white balance | the transcode rule below |
| Text, shape and Live Photo rasters | they are rasters |
| Export, preview, history, snapshots, recovery | they are projections |
| Profile assign / convert | `rebind_evaluation` rebuilds the tables |
| Depth conversion | the tables are rebuilt for the TARGET depth |

| Refused, by name | Phase | Acceptance criterion for closing it |
|---|---|---|
| Darker Color, Lighter Color, Hue, Saturation, Color, Luminosity | 3 | Each non-separable function re-derived against an independent reference **in linear light**, with the W3C `Lum`/`ClipColor` weights restated for a linear domain rather than reused unexamined |
| Dissolve | 3 | Its canvas-absolute dither must be shown to be domain-independent, or given a domain |
| Layer styles and Blend If | 3 | The effect planes are authored on ENCODED numbers and quantized per plane; a linear pipeline needs the plane contract restated and the Blend If ramp's domain chosen |
| Shadows/Highlights with a guide | 3 | Its guide is an alpha-weighted blur of ENCODED luma; the guide's domain must be chosen and measured, not inherited |
| Any layer with a retained source or a live filter | 2 | The filter stack must decode before its Gaussian neighbourhood and its cache keys must carry the policy; the retained-source affine resample must be premultiplied-linear |
| Image Size (`image_size`) | 2 | Resampling must happen in premultiplied linear; the sentence is `doc_evaluation::resize_refusal_sentence` and the core writes it once so the host guard and the MCP pre-check cannot drift |
| A profile with no matrix/TRC model, or one `validate_precision` rejects | — | Not a phase: the policy needs a finite monotone forward TRC per channel, and says so |

A **staleness net** sits beside the walk: a held transfer whose profile is not
`Arc::ptr_eq` to the document's own refuses every projection. It exists so a
missed `rebind_evaluation` is a loud refusal rather than a quietly wrong
picture; no test can reach it through the shipped paths, which is the point.

---

## The numbers

**Working precision** is f32 accumulators at both depths. Transfer tables are
filled from f64 `Curve::eval` — the ONE forward curve evaluator — and built
**once per document** (at the switch, at open, at a depth conversion and at a
profile write), held behind an `Arc`, never per projection.

| | Decode | Re-encode | Bytes |
|---|---|---|---|
| `ByteTransfer` | 256 entries/channel; linear interpolation at `v·255`, exact on a lattice code | the existing `icc_transform::ToneEncoder` forward-threshold search | ≈ 6 KB |
| `TypedTransfer` | 65536 entries/channel, the same rule at `v·65535` | `icc_precision::PrecisionToneEncoder::rgb`, continuous; the existing `round(v·65535)` in `from_float_samples` is the only rounding | ≈ 2.36 MB, counted by `record_resources` and by `PrecisionScene::plan` |

**No curve inverter was added.** Both directions go through the two encoders
this crate already had.

### The adjustment transcode, and its disclosed double quantization

A pointwise adjustment kernel is defined on ENCODED numbers, so under
`linear_light` the rule is

```
adjusted = decode( kernel( encode(backdrop) ) ),   then the k = opacity·coverage lerp IN LINEAR
```

At BYTE depth `encode` here is `encode_f32` — a LATTICE value, `ToneEncoder`
code / 255 — so the kernel sees exactly the number the encoded path shows it,
and an adjustment at full opacity over an opaque lattice backdrop reproduces
the encoded projection. That is deliberate (the byte `curves` op is itself a
256-entry LUT, so its input is quantized under both policies anyway), and it is
a **second quantization** whenever the backdrop is itself a composite. The cost
is measured rather than asserted:
`evaluation_composite::a_pointwise_adjustment_over_a_half_opacity_layer_stays_within_two_codes`
compares the shipped result against the IDEAL transcode — the same kernel with
no intermediate rounding — and is gated at **2 codes**, fixed before measuring.

At TYPED depth `encode` is continuous, so there is no extra quantization at
all.

### The byte group boundary keeps its byte quantization

`doc_group::rendered_group` re-encodes its private buffer through `env.transfer`
and the synthetic layer is decoded again on composite, so a group boundary
rounds exactly the way the canvas does — the invariant `doc::quantize`'s own
doc comment states. Typed groups stay f32 throughout.

---

## Persistence

`.rz` gains an **outermost conditional wrapper**, written ONLY for a
linear-light document:

```
"RZDC" | u32 38 (byte) / 39 (typed) | <the exact prior version words and body> | "EVAL" | u32 schema = 1 | u32 policy = 1
```

`RZDC_VERSION` is now 39. The wrapper nests OUTSIDE every earlier optional
feature — an outside-mask document writes 38 then 28 — and an ENCODED document
writes neither the word nor the table, so its bytes, first word included, are
exactly what the prior writer made. Policy 0 is never written and is refused on
read: a file that says "encoded" in the one slot that only exists to say
"linear" is malformed, not a document with a default. A bad schema, an unknown
policy, a trailing word and a profile the policy cannot decode all refuse
BEFORE the first PNG is decoded.

The typed descriptor's `evaluation` tag is `encoded_rgb_f32_v1` (the 7B
spelling, unchanged) or `linear_light_f32_v1`. The editing-state archive is
**v4**: its `evaluation` is `legacy` for a byte encoded document,
`encoded_rgb_f32_v1` for a typed one and `linear_light_f32_v1` for either under
the new policy, cross-checked against the decoded document's own policy on
read. v1–v3 archives read unchanged; a tag this build does not know refuses.
**An older build meeting `linear_light_f32_v1` refuses the document — that is
the disclosure, not a defect.**

---

## What the tests measure

Core, all through the FFI where the surface allows it and against oracles
written in the test files:

* `core/tests/evaluation_transfer.rs` — both transfers against the analytic
  sRGB piecewise curve, Display P3 and a gamma-2.2 `para` profile assembled in
  the test. `encode(decode(k)) == k` for every lattice code and channel at both
  depths; decode within **1e-5 linear** of the analytic curve at lattice codes
  (and at 16-bit midpoints); the byte interpolation rule pinned exactly at
  8-bit midpoints, because a chord across one 8-bit step of a gamma curve sits
  up to ~4.6e-5 from the curve and that is the rule working; a LUT profile
  refused by one sentence at both depths.
* `core/tests/evaluation_composite.rs` — an f64 oracle that decodes, composites
  with the W3C formula, replicates the byte transcode's rounding and re-encodes
  with the same forward-threshold rule. All twenty separable modes over an
  opaque backdrop, opacity + a mask ramp, a clip group, an isolated group
  boundary, adjustments at full and half coverage, the disclosed non-lattice
  fixture, every refusal at the switch, every refusal AFTER the switch
  (including `flattened` returning the walk's own sentence), the unchanged-policy
  NULL, the encoded projection's byte identity, and the profile rebind.
  **Gate: 1 code**, except the disclosed fixture at 2. Its last section is the
  TYPED twin of three of those measurements — the separable-mode oracle, the
  adjustment transcode and the post-switch refusal of `render` / `render_plan`
  — through `rz_precision_doc_*`. **Typed gate: 24 codes of 65535**, wider than
  the spec's typed figure of 2 and derived rather than measured: the oracle is
  the ANALYTIC sRGB curve while the built-in profile's TRC is 1024 samples of
  it rounded to 16 bits, a difference `evaluation_transfer` bounds at 1e-5
  linear, and the typed encode's steepest slope (the toe, `65535 · 12.92`
  codes per unit linear) turns that into 8.5 codes on each of the decode and
  encode sides. 24 codes is 0.09 of one byte code, so the typed section is
  still far tighter than the byte one; the spec's 2-code figure belongs to
  `evaluation_transfer`, whose oracle reads the crate's tables directly rather
  than through a profile.
* `core/tests/evaluation_native.rs` — v38/v39 round trips, the wrapper nesting
  outside MOUT, the encoded writer's untouched first word, crafted
  policy/schema/trailing-word refusals, the typed `info.evaluation` tags, and
  the typed tables inside admission (`retained_bytes` grows by ≥ 2,359,320).
* `core/tests/evaluation_ffi.rs` — every new export under NULL, and the two
  image comparisons on equal, different and mismatched images.
* The two null-safety sweeps carry every new export.

**Drive** (`scripts/verify_phase11e_linear.py`, receipts under
`~/devel/rasterize-fixtures/phase11-receipts/11e/`), tolerances fixed before
measuring:

| Gate | What | Measured |
|---|---|---|
| **E1** | an 8-bit layered fixture (opaque base, `document_v2` exposure directly above it, Multiply at 50 %, masked Screen, a group) switched after `get_evaluation preview:true`; `render` AND `export_image` vs the Python oracle | mean **0.00098**, max **1** code (gate: mean ≤ 0.05, max ≤ 1). The preview's own readout: mean 6.01, max 23 codes over 70 % of samples — the policy is a visible change, not a rounding |
| **E2** | the same shape at 16 bits, exported at `bit_depth: 16` | mean **0.446**, max **8** codes of 65535 (gate: mean ≤ 2, max ≤ 8) |
| **E3** | `save_copy` `.rz` → `open_document` | first word **38**, policy `linear_light`, every sample identical |
| **E4** | snapshots, and `write_recovery` + a forced crash + relaunch + `recover_document` | the policy and the projection survive both |
| **E5** | one `undo` | back to `encoded`, and the pre-switch export's sha256 reproduced exactly |
| **E6** | an encoded document across the run | first word **10**, and two exports byte-identical across a switch out and back |
| **E7** | refusals | a live-filter document and a Hue document each refuse `set_evaluation` naming the layer; on the switched document the blend-mode, layer-style and Blend If tools and `image_size` each refuse with the evaluation sentence, after which `get_document` still reports `linear_light` with `projection_error` null and `render`'s sha256 is unchanged |

Two things E2 and E7 record rather than claim:

* **E2's max is exactly 8 codes of 65535**, at the gate. The gate was chosen
  before measuring from the encoder's stated bound (one 65536-interval grid
  step per encode, two encodes in the transcode, plus f32 rounding); the
  measurement sits at it rather than under it, and that is written here rather
  than rounded away.
* **E7's post-switch `add_live_filter` refuses by its own target precondition,
  not by the evaluation guard.** A live filter needs a retained source, and a
  retained source is already refused by the policy at the switch, so the tool
  can never reach the evaluation guard on a linear document. The evaluation
  sentence for that operator is measured on the pre-switch document instead
  (`switch_live_filter` in the receipt).

---

## Recorded decisions

* **The typed E2 fixture differs in shape from E1**, because the typed model
  imposes it: new typed layers append at the TOP (so the adjustment sits above
  the pair rather than directly over the base) and typed grouping takes every
  top-level layer at once. The oracle follows the stack that is actually built.
* **`get_evaluation` is dropped from recordings (`nonRecordable`) rather than
  refused as an Action step (`notAStepTool`).** Every sibling read tool —
  `get_document`, `get_color_profile`, `get_selection` — is handled that way,
  and refusing a harmless read that all of them accept would be an
  inconsistency, not a safety property. `set_evaluation` IS a recordable step:
  per-document, undoable and `.rz`-persisted, which is the four-part test
  `AgentServer+Guides.swift`'s header states.
* **`Image Size`'s refusal sentence lives in the CORE** and travels in
  `rz_evaluation_operators`' JSON under `resize_refusal`, so the editor's guard
  and the MCP pre-check read one string instead of keeping two in step.
* **A typed `get_document` now reports `evaluation` as the same block the byte
  reply does**, with the core's descriptor tag moved beside it as
  `evaluation_tag`. One shape at both depths was worth the key move.
* **The non-lattice adjustment fixture uses `invert`, not `curves`.** The byte
  `curves` op is a 256-entry LUT, so its own input is quantized under both
  policies and an oracle for it would have to duplicate `adjust_curves`'
  monotone spline. `invert` is `1 - v` in one line, which keeps the oracle
  genuinely independent and leaves the transcode as the only thing measured.

---

## Open — what P7-24 still owes

* **Phase 2**: the live-filter stack under `linear_light` (decode before the
  Gaussian neighbourhood; cache keys carry the policy), retained-source affine
  resampling, and Image Size / typed `resize` in premultiplied linear.
* **Phase 3**: layer styles, Blend If, the six non-separable modes, Dissolve,
  the Shadows/Highlights guide, and destructive mixing edits (paint, fill,
  blur) under the policy.
* **Destructive layer-pixel edits are storage edits and keep their contracts.**
  Paint, fill, destructive filters and transforms write a layer's stored
  numbers; they are not the projection, they are not refused, and phase 3 owns
  the question of whether any of them should acquire a domain. This is an open
  row, stated here rather than left implicit.
* HDR/float (11F), CMYK and Gray destinations, `PrecisionStateArchive.swift`
  (no callers) and PSD export of a linear document beyond today's behaviour are
  out of scope for all three phases.
