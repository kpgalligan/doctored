# Versioned linear-light evaluation, phase 1 of P7-24 (ADR)

- **Status:** Accepted — open items remain ([#12](https://github.com/kpgalligan/doctored/issues/12): P7-24 phases 2–3)
- **Phase / dates:** 11E phase 1, drive run 2026-09-18 (all seven gates E1–E7 pass)
- **Original:** [archive/phase-11e-linear-light-status.md](../archive/phase-11e-linear-light-status.md) — full history, measurements, receipts
- **Key commits:** `6d4b1b0`
- **Carryover:** first of three slices of **P7-24** (the rest: [#12](https://github.com/kpgalligan/doctored/issues/12))

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
- **The projection is the invariant.** Anything not migrated is refused **by name** at three places: the switch
  (`with_evaluation`), every later edit (host `ImageDocument.applyEdit` guard and
  `AgentServer.performGroupedEdit` pre-check), and every render (`DrDocument::transfer` /
  `PrecisionDocument::transfer`, which every `composite_env` caller and `scene_cut` goes through, so `flattened`,
  `render`, `render_plan` and `info.render_refusal` all carry the sentence). The operator table is
  `doc_evaluation.rs`'s `REFUSED`, exported as JSON by `dr_evaluation_operators`.
- **Migrated:** Normal + the 19 separable modes (`blend::composite_source_into` unchanged — only inputs decoded),
  opacity, masks, pass-through/isolated/clip groups, pointwise adjustment layers (incl. `document_v2`
  exposure/WB), text/shape rasters, RAW layers, export/preview/history/snapshots/recovery, profile assign/convert
  (`rebind_evaluation` rebuilds tables), depth conversion (tables rebuilt for the target depth).
- **Encoded documents take the `transfer == None` branch everywhere** and are byte-for-byte unchanged;
  `linear_light_refusal()` returns `None` before walking.
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
  raster; its re-render on transform/replace is unchanged and resamples in the STORED encoding, as raster Free
  Transform and warp already do under linear light. `MIGRATED` holds `retained_source`.
- **`image_size`'s refusal sentence lives in the core** (`doc_evaluation::resize_refusal_sentence`, shipped in the
  operators JSON as `resize_refusal`) so the host guard and MCP pre-check can't drift.
- **Recordability:** `get_evaluation` is `nonRecordable` (like every sibling read); `set_evaluation` is a recordable
  step (per-document, undoable, persisted). Typed `get_document` reports `evaluation` as the same block as byte, with
  the core descriptor tag moved to `evaluation_tag`.

## Rejected alternatives
- Implying linear light from profile or depth — the policy must be a deliberate, versioned property.
- A curve inverter for re-encoding — the existing forward encoders suffice and stay the single source of truth.
- Refusing `get_evaluation` as an Action step — inconsistent with every sibling read tool.
- A `curves` oracle for the non-lattice fixture — it would duplicate the spline; `invert` keeps the oracle independent.

## Consequences
Every newly migrated operator must come with an independent oracle and be removed from `REFUSED` only then. The
three refusal points must stay in step; the render-time walk is the backstop. 11F's float mode is linear light by
construction on top of this policy. Tests: `core/tests/evaluation_{transfer,composite,native,ffi}.rs` (1 code
gate; 2 for the disclosed fixture; typed 24/65535, derived from the sampled-TRC-vs-analytic bound); drive
`scripts/verify_phase11e_linear.py`.

## Open items (verified 2026-10-30)
- `OPEN` [#12](https://github.com/kpgalligan/doctored/issues/12) — **Phase 2** (P7-24): Image Size / typed
  `resize` in premultiplied linear (refused, `doc_evaluation::REFUSED`), and geometric resampling (Free Transform,
  warp, RAW re-render) in premultiplied linear (admitted today, resampling in the stored encoding).
- `OPEN` [#12](https://github.com/kpgalligan/doctored/issues/12) — **Phase 3** (P7-24), each needing its own
  linear-light oracle: the six non-separable modes (W3C `Lum`/`ClipColor` weights restated for linear), Dissolve
  (show its dither is domain-independent or give it one), layer styles and Blend If (restate the plane contract,
  choose the ramp's domain), the Shadows/Highlights guide (choose and measure the guide's domain), and whether
  destructive paint/fill/blur acquire a domain (today they are storage edits and not refused).
- `DROPPED` — PSD export of a linear document beyond today's behaviour; CMYK/Gray destinations; HDR/float (done
  separately in 11F); `PrecisionStateArchive.swift` coverage (internal probe archive used only by
  `scripts/precision-probe/`) — all declared out of scope for all three phases.
