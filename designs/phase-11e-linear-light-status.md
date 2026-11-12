# Versioned linear-light evaluation, P7-24 (ADR)

- **Status:** Accepted — closed: phases 2 and 3 shipped 2026-11-12, nothing is refused by name
- **Phase / dates:** 11E phase 1, drive run 2026-09-18 (all seven gates E1–E7 pass); phases 2–3, 2026-11-12
- **Original:** [archive/phase-11e-linear-light-status.md](../archive/phase-11e-linear-light-status.md) — full history, measurements, receipts
- **Key commits:** `6d4b1b0` (phase 1); phases 2–3: `3ea54e60` (the non-separable modes and Dissolve), `ef489702` (the guide), `6123afbb` (styles and Blend If), `032a5e3f` (resampling), `6c94cb46` (closure)
- **Carryover:** **P7-24**, all three slices

> **Removed 2026-10-10:** smart objects, live filters and non-RAW retained sources (placed images, host
> media, Live Photo frames) are gone for v1; camera RAW layers stay. Where this record describes them, that
> code no longer exists: [removal ADR](convert-to-smart-object-status.md).

## Context
Compositing on encoded code values is not physically linear, but every existing document's appearance depends on
it. The plan required specifying transfer, compositing, blending and filter semantics against independent
references, then adding an explicit, versioned evaluation choice — while existing encoded byte/u16 documents keep
their evaluation exactly, and each operation is migrated and verified before it is exposed.

## Decisions
- **A per-document policy**: `encoded` (default, unchanged) or `linear_light` — decode every sample through the
  document profile's TRC, composite/blend/mix at f32, re-encode exactly **once** at the output. Chosen only
  deliberately (Image ▸ Evaluation…, `set_evaluation`); never implied.
- **The projection is the invariant.** Phase 1 refused every unmigrated operator by name at the switch, at every
  later edit (host `ImageDocument.applyEdit` guard, `AgentServer.performGroupedEdit` pre-check) and at every
  render (`DrDocument::transfer` / `PrecisionDocument::transfer`, through which every `composite_env` caller and
  `scene_cut` obtains its transfer). Phases 2–3 evaluated every operator, so the `REFUSED` table and its walk are
  gone; the three points remain for the staleness net below (and a float document's own table), and
  `dr_evaluation_operators` answers `{"migrated":[..],"refused":[]}`.
- **Migrated:** every operator. Phase 1: Normal + the 19 separable modes, opacity, masks, pass-through/isolated/clip
  groups, pointwise adjustment layers (incl. `document_v2` exposure/WB), text/shape rasters, RAW layers,
  export/preview/history/snapshots/recovery, profile assign/convert (`rebind_evaluation` rebuilds tables), depth
  conversion. Phases 2–3 (D1–D11 below): the six non-separable modes, Dissolve, a guided Shadows/Highlights, layer
  styles (all effects, Fill, knockout, grouped interiors, styled clip bases and adjustments), Blend If, Image Size,
  Free Transform (affine, perspective, a set, perspective crop), Warp and a RAW layer's re-render.
- **D1, one rule:** only what enters the accumulator is decoded (a stored pixel, an effect's ink); every alpha,
  coverage, blend and mix statement runs unchanged on linear components. Rules authored on encoded numbers read
  `doc_evaluation::encoded_view` (`encoded_view16` typed) of a composited sample.
- **D2, non-separable modes:** the `blend.rs` masters on linear components, W3C weights 0.3/0.59/0.11 in the
  document's primaries, ClipColor at [0, 1] (8/16-bit linear light has no range above 1).
- **D3, Dissolve** is domain-independent: its threshold reads position and effective alpha, and a shown pixel is
  the source's own colour, which decode then encode returns.
- **D4, effect colours:** an effect contributes the colour the encoded path draws (authored sRGB into the profile,
  a gradient interpolated on encoded numbers, a pattern tile's codes), decoded per sample
  (`StyleColors::Linear8/16`, `style_render::InkDecode`); a gradient effect keeps its encoded ramp.
- **D5, Blend If reads encoded values on both sides:** This Layer is the stored code (or a composited buffer
  re-encoded), Underlying is the accumulator re-encoded (to the lattice at 8 bits, continuously at 16, then
  `source_weight_16`'s own rounding). A range selects the pixels it selects in the encoded document. A float
  document reads stored linear values; narrowing one discloses the change as a rendering-difference row.
- **D6, the guide** is a kernel input, built from the re-encoded backdrop: over an opaque lattice backdrop a guided
  Shadows/Highlights at full opacity equals the encoded result; the `k` lerp is linear.
- **D7, resampling** (`sampling_linear`): decode each tap, accumulate premultiplied linear through the existing
  extended adapters, unpremultiply, clamp to [0, 1], encode once. Nearest and the exact forms (identity, integer
  translation, quarter turns, flips) still copy codes; masks, alpha channels and selections are coverage and
  unchanged. A byte document's 16-bit RAW master renders through the byte tables (`WideByteCodes`), so no 16-bit
  tables are built for it.
- **D8, a policy switch re-renders nothing:** a RAW fallback and a warped layer are stored rasters; the next
  geometry edit, replace or profile conversion re-renders them under the policy in force — a conversion through
  the DESTINATION profile's tables.
- **D9, storage edits take no domain:** paint, fill, gradients, filters, Puppet Warp, Perspective Warp, Liquify and
  Move/Transform of selected pixels write stored codes under either policy; whether each family should take the
  document's domain: [#35](https://github.com/kpgalligan/doctored/issues/35).
- **D10, no `EVAL` schema bump:** an older build opening a linear-light file that holds a style or a Hue layer
  refuses its render by name, phase 1's disclosure.
- **D11, a baked style is the layer as the canvas shows it:** Merge Down, the merged-layer paths and PSD export's
  style bake (`psd_export_rasterize`, `bake_style16`) composite under the transfer and encode through it.
- **Not measured:** no timing was taken for phases 2–3 (lanes ran in parallel); a linear-light Blend If costs one
  accumulator encode per pixel, and a linear byte Image Size no longer takes the `image` crate's opaque path.
- **Encoded documents take the `transfer == None` branch everywhere** and are byte-for-byte unchanged;
  `linear_light_refusal()` returns `None` before comparing anything.
- **Staleness net:** a held transfer whose profile isn't `Arc::ptr_eq` to the document's refuses every projection,
  so a missed `rebind_evaluation` is loud, not wrong.
- **Numbers:** f32 accumulators at both depths. Tables come from f64 `Curve::eval` (the ONE forward evaluator), built
  once per document (switch, open, depth conversion, profile write) behind an `Arc`. `ByteTransfer`
  (`core/src/doc_evaluation.rs`): 256 entries/channel, linear interpolation, re-encode via
  `icc_transform::ToneEncoder`'s forward-threshold search (~6 KB). `TypedTransfer`
  (`core/src/precision_evaluation.rs`): 65536 entries/channel, re-encode via `PrecisionToneEncoder::rgb` (~2.36 MB,
  counted by `record_resources` and `PrecisionScene::plan`). **No curve inverter was added.**
- **Adjustment transcode**: `adjusted = decode(kernel(encode(backdrop)))`, then the opacity·coverage lerp in linear.
  At byte depth `encode` is the lattice value, so a full-opacity adjustment over an opaque lattice backdrop equals
  the encoded result; this is a disclosed **second quantization** over composited backdrops, gated at 2 codes vs
  the ideal transcode. Typed depth has no extra quantization.
- **Byte group boundaries keep byte quantization** (`doc_group::rendered_group` re-encodes through `env.transfer`),
  matching `doc::quantize`'s invariant; typed groups stay f32.
- **Persistence** (`core/src/evaluation_native.rs`): an outermost conditional wrapper written **only** for a
  linear-light document: `"DRDC" | 38 (byte) / 39 (typed) | <prior words and body> | "EVAL" | schema 1 | policy 1`
  (legacy `"RZDC"` still read).
  Encoded documents write neither word nor table. Policy 0 is never written and refused on read; bad
  schema/policy/trailing word or an undecodable profile refuse before any PNG decode. Typed descriptor tags
  `encoded_rgb_f32_v1` / `linear_light_f32_v1`; editing-state archive **v4** (`legacy` / `encoded_rgb_f32_v1` /
  `linear_light_f32_v1`, cross-checked on read; v1–v3 still read). An older build refusing such a file is the
  disclosure. (`DRDC_VERSION` was 39 at ship; later wrappers raised it.)
- **Policy needs a finite monotone forward TRC per channel**: LUT-only profiles or ones `validate_precision` rejects
  are refused (not a phase item).
- **A RAW layer is admitted.** No compositor reads the binding, so the layer composites its fallback like any
  raster; its re-render resamples in linear light (D7, D8).
- **Recordability:** `get_evaluation` is `nonRecordable` (like every sibling read); `set_evaluation` is a recordable
  step (per-document, undoable, persisted). Typed `get_document` reports `evaluation` as the same block as byte, with
  the core descriptor tag moved to `evaluation_tag`.

## Rejected alternatives
- Implying linear light from profile or depth — the policy must be a deliberate, versioned property.
- A curve inverter for re-encoding — the existing forward encoders suffice and stay the single source of truth.
- Refusing `get_evaluation` as an Action step — inconsistent with every sibling read tool.
- A `curves` oracle for the non-lattice fixture — it would duplicate the spline; `invert` keeps the oracle independent.

## Consequences
A new operator or resample path follows D1–D11 and comes with an independent oracle. 11F's float mode is linear
light by construction on top of this policy. Tests: `core/tests/evaluation_{transfer,composite,native,ffi}.rs`
(1 code gate; 2 for the disclosed fixture; typed 24/65535, derived from the sampled-TRC-vs-analytic bound);
phases 2–3 `evaluation_{nonseparable,guide,styles,resample,retained_source}.rs` over the shared
`tests/common/linear_oracle.rs`, each case paired with the regional render where one exists; drives
`scripts/verify_phase11e_linear.py` (E1–E7; its oracle decodes through the sRGB profile's 1024-sample
`curv`, checked against the export's embedded profile, so E2 holds T2's 8 codes, which the analytic curve
cannot in the toe; E7 admits every operator) and `verify_phase12e2_adjustments.py` a5.

## Open items
- `DROPPED` — PSD export of a linear document beyond today's behaviour and the style bake (D11); CMYK/Gray
  destinations; HDR/float (done separately in 11F); `PrecisionStateArchive.swift` coverage (internal probe archive
  used only by `scripts/precision-probe/`) — all declared out of scope for all three phases.
