# Phase 9I cutout preparation execution and cancellation (ADR)

- **Status:** Accepted — closed
- **Phase / dates:** 9I execution continuation, 2026-09-12
- **Original:** [archive/phase-9i-execution-status.md](../archive/phase-9i-execution-status.md) — live-cancellation tables, probe receipts, hashes
- **Key commits:** `2e26401`

## Context
The internal byte and native refinement-cutout preparation ([phase-9i-cutout-status.md](phase-9i-cutout-status.md))
could not be cancelled during native final display, could let a fast worker finish before AppKit created its
progress panel, and copied byte output records without job control. This slice fixes execution only: no estimator,
pixel arithmetic, public tool, Actions schema or native format changed, and it qualifies no matting quality.

## Decisions
- **Single-use compound parent:** `RefinementCutoutPreparation` claims one parent control before dispatch via the
  additive `dr_job_control_begin_compound` (`core/src/ffi_jobs.rs`), which reuses the core atomic begin, owns no
  scratch, and cannot be reused by another preparation or ordinary kernel. Each real stage gets a **fresh child
  token** with its own scratch ceiling. Existing `PreparedDocumentEdit` callers keep their default path.
- **Owner-close forwarding:** preparation observes the owning document's close notification throughout; the main
  thread drains every worker before removing the observer; cancel after preparation still fails the final guard.
- **Panel before work:** cutout stages create the progress panel and modal session *before* constructing the
  `WorkerTask`, through a synchronous main-thread task-factory overload of `BackgroundWork.wait`
  (`app/Sources/BackgroundWork.swift`). Factories return their task immediately and never throw after starting
  a mailbox; cleanup restores the prior `isWaiting`, so a nested busy refusal cannot clobber an outer wait.
- **Progress ranges (native):** assembly 0–0.25, display 0.25–1; within display, native render 0–0.85 and byte-preview
  quantizer 0.85–0.98. Progress describes pipeline work, not time; cancellation/error state — not progress 1 —
  decides success. CoreGraphics image creation, individual serde parses and bounded ICC validation remain
  internally noncancellable (checked before/after); no universal cancellation latency is claimed.
- **Byte output copy:** cutout assembly uses `clone_with_job` (`core/src/document_clone.rs`), copying owned
  layer/channel strings in UTF-8-safe ≤ 64 KiB chunks with checks between records; pixels, masks, styles, profiles
  and metadata blobs keep Arc sharing. Ordinary callers still use `Clone`.
- **Byte render shortcut:** `core/src/doc_cutout_render.rs` accepts only the cutout shape — exactly one visible plain
  canvas root, Normal, opacity 1, enabled default mask, all older roots hidden — and drives the shared
  `composite_layer_into` + `quantize` in ≤ 4096-pixel chunks via `Layer::active_mask`'s raw-Arc fast path. Other
  shapes refuse; general byte rendering is unchanged.
- **Metadata admission:** the plan does not parse JSON. Matrix/TRC profiles skip hidden-metadata classification
  (as `flattened` does); for unmodelled profiles the render admits `256 ×` the largest leaf slot's UTF-8 length
  (+ bookkeeping; max slot 16 MiB) before `validate_context_with_job`, preserving the refusal for hidden calibrated
  adjustments. A working estimate, not an RSS limit.
- **Borrowed-matte accounting:** an unregistered raw u16 matte stays in host admission and in
  `PrecisionWorkContext.additionalRetainedBytes` through display preparation; a registered `PrecisionScalarBuffer`
  adds zero, since its lease is already in the retained union. No second lease.

## Rejected alternatives
- An 8192² live-cancellation witness — refused for memory risk; the fix (panel before dispatch) needs no larger fixture or artificial delay.
- Accepting cancellation observed only after output finished — every witness had to hit the core `operation cancelled` checkpoint mid-stage.

## Consequences
- Future cutout stages must use child tokens from the compound parent and create UI before dispatch.
- Verified by focused core tests (job control, byte/native cutout, calibrated adjustments; all 65,536 byte
  foreground/coverage pairs vs independent oracles) and an AppKit source probe (six live Cancel/owner-close
  outcomes across byte projection, native render and preview quantizer). Integrated full-Rust/Release/MCP
  qualification is recorded in [phase-9i-cutout-status.md](phase-9i-cutout-status.md).

## Open items (verified 2026-09-26)
- `DONE` — Production Release and broader integration qualification deferred to the phase owner (recorded in the cutout status continuation qualification, `2e26401`).
