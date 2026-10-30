# Phase 4 rendering and kernel contract (ADR)

- **Status:** Accepted — closed (its document-level successor is the Phase 13A regional contract, [phase-13a-regional-status.md](phase-13a-regional-status.md))
- **Phase / dates:** Phase 4C/4D of the [implementation plan](professional-photo-editor-implementation-plan.md), 2026-09-09
- **Original:** [archive/phase-4-rendering-contract.md](../archive/phase-4-rendering-contract.md) — full history, measurements, receipts
- **Key commits:** `968e0ee`

## Context
Phase 4C had to define how a bounded region or preview of an operation is requested — precision, profile and
alpha semantics, neighbourhood needs, cancellation and failure — before any regional/tiled work, and prove it
on one real operation. It deliberately did not build a regional compositor: the document compositor still
rendered full frames. It also records the 4D measurements used to decide which slow paths moved to the worker
([phase-4-jobs-resource-status.md](phase-4-jobs-resource-status.md)).

## Decisions
- **Pixel semantics.** `DrImage` is tightly packed, top-row-first straight RGBA8; the handle carries no ICC —
  the owning document or explicit host context names the RGB space. Copy/Invert operate on encoded bytes, with
  no transfer-curve or matrix conversion, and preserve alpha exactly. Invert is `255 - channel` including
  latent RGB under zero alpha (the float adjustment twin keeps `1 - channel`). The image-level plans reported
  `rgba8_unorm` storage, `exact_u8` processing, `encoded_rgb_passthrough`; Phase 3's `document_v2` evaluation
  is separate.
- **One validated `RenderRequest`** (`core/src/render_request.rs`) was behind `dr_image_render_plan` and
  `dr_image_render_region` (`core/src/ffi_render.rs`); that image-level request and its exports were
  deleted. It took an operation (Copy/Invert; unknown codes failed), a nonempty source rectangle fully inside the
  source (overflow/out-of-range was an error), a positive integer preview divisor and an output-byte budget.
  - Divisor 1 was authoritative and exact. Divisor > 1 was preview-only: ceiling-division dimensions, sampling
    `(x + i·d, y + j·d)` — top-left of each cell, no interpolation, no centring shift. These divisor semantics
    and the request's named refusals survive in the 13A renderer.
  - One output allocation of `w × h × 4`, no scratch heap; the budget refused before allocation.
  - The plan was owned JSON with `contract_version: 1`, policies, requested and required input rectangles,
    neighbourhood, exact output/read counts and cancellation cadence. Planning scanned no pixels.
  - Requests were transient; UI/Actions Invert produces a full authoritative result (it runs `ops::invert`,
    `dr_image_invert` in `core/src/ffi.rs`), so undo and selection writeback are unchanged. Copy was a render
    primitive, never an undo step.
- **Cancellation, ownership, errors.** The image-level request took the same single-use `DrJobControl` as
  Content-Aware Fill, claimed after plan validation; output bytes were reserved against its scratch budget with
  an RAII guard released on every return. It checked before allocation, at most every **4096 output pixels**,
  and before returning; progress stage 6 then 5. Errors returned `NULL` plus an owned error string — never a
  partial image, substituted black or a successful-looking no-op. A null source returned null. Renders touched
  no document caches and no AppKit. The cadence (`CHECKPOINT_PIXELS`, now `core/src/regional_plan.rs`)
  survives in the 13A renderer.
- **Extension rules for future kernels** (cited by `core/src/regional.rs`):
  - Spatial kernels must use the premultiplied floating-point sampler (`core/src/sampling.rs`) and
    unpremultiply at storage; never interpolate hidden straight RGB. Masks stay scalar coverage.
  - A spatial kernel must declare, *before* rendering, its source-coordinate halo, border-extension rule and
    source↔preview unit conversion; a region near a border must match the full-frame reference's edge
    semantics (no "crop then blur the crop as if its edges were image edges").
  - A global-analysis operator must declare full-source analysis, its analysis scratch bound and whether
    evaluation can then be regional; until adapted it may use a declared **full-frame fallback** with a
    full-frame allocation estimate that the caller may reject. No automatic fallback hides an unsupported
    request.
  - Any future evaluator must carry the document's evaluation version and profile through every stage and must
    not invent a new persisted stage order. Previews may lower resolution but never replace the authoritative
    source. Higher depth needs new typed interfaces (Phase 7).
- **4D migration decision:** measured 24/48 MP medians (M2 Max; e.g. bilinear affine ≈0.56/1.07 s, PNG
  export with projection ≈0.57/1.12 s, CFA RAW development ≈1.1/2.3 s) justified moving final raster
  affine/perspective transforms, save/export and RAW development to the worker via `TransformWork`
  (immutable document, matrices/quads, sampler, descriptions to drop; main thread re-verifies handle and
  revision before one undoable edit). Exact 90°/flip commands and ordinary native open stayed where they were;
  described (text/shape) sources still render on the main thread.

## Rejected alternatives
- Treating the kernel request as an alternative compositor — it would fork evaluation order from the
  document projection.
- Centred or interpolated preview sampling — nearest top-left keeps previews cheap and unambiguous.
- Silent full-frame fallback for spatial/global operators — hides the real cost from the caller's budget.

## Consequences
- The 13A regional document renderer (`core/src/regional.rs`, `dr_doc_render_region[_plan]`) inherits these
  rules: declared halos, named refusals with no hidden full-frame fallback, the 4096-sample cadence and
  divisor semantics.
- Tests: `core/tests/regional_render.rs` was deleted. Invert's bytes are pinned by
  `core/tests/integration.rs` (`filters_grayscale_invert_sepia_blur_sharpen`); divisor, budget, refusal and
  cancellation behaviour by `core/tests/regional_document.rs` and the `regional_byte_*` suites.
  `cargo run --release --example phase4_latency` no longer reproduces the region/preview rows (full tables stay
  in the archive).

## Open items (verified 2026-09-26)
- `DONE` — Deleted end to end: the `dr_image_render_plan` / `dr_image_render_region` exports, `DrRenderOperation`, the Swift wrappers and `RenderRequest`, `render_request.rs` / `ffi_render.rs`, and `regional_render.rs`; Invert now runs `ops::invert`.
- `DONE` — Layered compositor regional path — 13A (`core/src/regional.rs`, `core/src/doc_regional.rs`,
  `core/src/precision_regional.rs`) at 8/16/float.
- `DROPPED` — A padded regional Gaussian adapter on the image-level request: the regional work moved to the
  13A document renderer (feathered masks declare their blur halo there; live filters and global operators were
  closed by recorded decision in 13A-4a).
- `TRACKED` — Cooperative cancellation for transform resampling / platform RAW / encoders: codec and RAW →
  P7-32 (plan §13C); the byte final transform job is an `OPEN` item in
  [phase-4-jobs-resource-status.md](phase-4-jobs-resource-status.md).
- `TRACKED` — Remaining regional/preparation cancellation and latency → P7-31, 13A-latency.
