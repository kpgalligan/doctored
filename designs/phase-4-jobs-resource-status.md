# Phase 4 — jobs and resource foundations (ADR)

- **Status:** Accepted — open items remain ([#9](https://github.com/kpgalligan/doctored/issues/9), [#10](https://github.com/kpgalligan/doctored/issues/10), [#11](https://github.com/kpgalligan/doctored/issues/11))
- **Phase / dates:** Phase 4 (4A–4D) of the [implementation plan](professional-photo-editor-implementation-plan.md), 2026-09-09; issue records added through October 2026
- **Originals:** [archive/phase-4-jobs-resource-status.md](../archive/phase-4-jobs-resource-status.md) — the phase's full history, to 2026-09-26; [archive/2026-10/phase-4-jobs-resource-status.md](../archive/2026-10/phase-4-jobs-resource-status.md) — this doc as of 2026-10-07, with every measurement table and issue record
- **Key commits:** `968e0ee` (Phase 3 was `f34201b`)

> **Removed 2026-10-10:** smart objects, live filters and non-RAW retained sources (placed images, host
> media, Live Photo frames) are gone for v1; camera RAW layers stay. Where this record describes them, that
> code no longer exists: [removal ADR](convert-to-smart-object-status.md).

## Context
Expensive edits (Content-Aware Fill, healing, saves, RAW development, transforms) ran synchronously on the
main thread with no cancellation, and nobody could say how much memory undo, previews and caches actually
held. Phase 4 introduced one bounded worker seam with immutable capture and stale-result refusal, cooperative
Rust cancellation, deduplicated memory accounting with admission, and a regional kernel contract (recorded
separately in [phase-4-rendering-contract.md](phase-4-rendering-contract.md)).

## Decisions
- **One final operation app-wide on a serial worker** (`app/Sources/BackgroundWork.swift`). The closure owns
  immutable values only; AppKit state, document mutation and undo stay on the main thread. The main thread
  polls an owned result mailbox rather than depending on reentrant main-queue dispatch while MCP or an Action
  waits. Interactive fill/heal show nonmodal progress; waiting calls use an AppKit modal session with Cancel.
  *Why:* one bounded seam instead of concurrency spread through every controller; background work must not
  starve interaction.
- **Stale results are refused, never merged** (`app/Sources/DocumentJobs.swift`). Completion checks a
  monotonic revision (monotonic even through undo), the exact retained document handle, the captured layer
  position/fingerprint scoped to the immutable stack, and selection state. Any intervening mutation — even
  Undo back to the same handle — refuses. Cancelled/stale/unchanged results make no undo entry, and neither
  does a result the central mutation path refuses (a linear-light operator, a persisted warp it would drop),
  which ends `refused` with that sentence instead of `completed`; success is exactly one edit. Closing a document cancels. Only the last 16 terminal reports are kept, without buffers.
- **Cooperative cancellation through an explicit FFI token** (`DrJobControl`, `core/src/job_control.rs`):
  single-use, claimed by the kernel, checkpoints return an ordinary error so unwinding drops scratch and the
  reservation. Checks inside component walks, planning, EDT, PatchMatch search/voting and Poisson. *Why:*
  discarding an eventual result alone does not stop computation or release memory.
- **MCP:** `content_aware_fill` / `spot_heal_stroke` / `patch_region` and the 8-bit `heal_stroke` accept
  `background: true` and return a `job_id` (listed in `AgentServer.documentJobTools`);
  `job_status` / `cancel_job` observe and control it; default calls and Actions wait. Recording Start/Stop
  drains a pending job first so a late result cannot drop a step or land in a newer recording.
- **Weak host ownership registry + unique core allocation accounting** (`app/Sources/RasterResources.swift`,
  `core/src/resources.rs`). The registry follows every live handle wrapper (including those held only by
  undo closures, gestures, previews and jobs) without owning them; the core deduplicates backing allocations
  across the live set so an `Arc`-shared plane counts once. Reports separate measured retained payload,
  scratch estimates, allocator overhead and unaccounted platform memory; process RSS is not claimed to match.
- **Working limit: RAM-scaled** (see the ADR, [working-limit-ram-scaled](working-limit-ram-scaled.md)); projection refresh evicts core caches above 128 MiB; job
  admission evicts reusable caches before measuring and reserves scratch estimates before expensive work.
  These are admission estimates, not an OS ceiling: a job, render or save may refuse. Surfaced as
  Help > Memory Usage and MCP `resource_usage`.
- **4D measured migration:** spot healing, save/export, final raster affine/perspective transforms
  (`app/Sources/TransformWork.swift`) and RAW development moved to the worker because 24/48 MP medians
  justified it. Save captures source, projection and export settings together, writes atomically on the
  worker and marks only that captured handle saved; newer edits stay dirty, Undo to it becomes clean, export
  copies never change the saved handle, failed writes keep the previous file and dirty state.
- **Explicit boundaries:** save/export and RAW kernels had responsive but noncancellable progress (no Cancel
  button; transform kernels had it too until they were made cancellable); Healing Brush/Patch commits stayed
  synchronous (8-bit UI commits moved to nonmodal `DocumentJobs` jobs — their rasterize prompt now comes
  before the solve, and the panel's Cancel, Esc on the canvas and ⌘Z/⇧⌘Z stop them; their MCP byte twins
  followed); storage stayed straight RGBA8; no tiling and no native version change.

## Rejected alternatives
- Result-discard-only cancellation — leaves CPU and scratch memory busy.
- Counting retained bytes per snapshot — overcounts shared `Arc` payloads by an order of magnitude.
- Moving all slow paths at once — only measured-justified paths moved.

## Consequences
- New expensive edits go through `BackgroundWork`/`DocumentJobs` with immutable capture, stale refusal and a
  scratch reservation; kernels that can run long take a `JobContext`.
- Cancellation claims are per fixed case (the original qualified ≤ 2 s on 24/48 MP fixtures), not a
  universal latency guarantee.
- Tests: `core/tests/{job_control,resources}.rs`, `scripts/verify_phase4.py`,
  `scripts/jobs-probe/`, `core/examples/phase4_latency.rs`, `scripts/phase4_raw_latency.py` (which compiles every
  app source except `main.swift` against the release core archive — `make rust` first).

## Open items (verified 2026-10-07)
Each `DONE` entry keeps what shipped and what a later batch must hold. The measurements, drives and tables
(gates M, R and R′ among them) are in the archived original.
- `DONE` — Byte final Free Transform, Distort and `transform_layers` commits (UI, MCP, Actions, 16-bit MCP)
  are cancellable: `TransformWork.wait` runs the core kernel and the display preparation as two children of
  one `RasterJobControl`, and the byte affine/perspective set polls its resample, masks and a retained member's
  re-render (`core/tests/byte_geometry_cancel.rs`). A late cancel of a 16-bit gesture commit or Layer ▸ Warp is a
  cancel. Every user cancel surfaces as one `JobCancelled` that never alerts. The byte resample reports stage 6
  per destination row (0.8 of the member's slice, `LINKED_MASK_RASTER_SHARE`, when a linked mask follows) and
  `make cancel-bench` runs the ignored 24 MP bench.
- `DONE` — The 8-bit Crop straighten and the filtered Rotate Canvas commit through `dr_doc_geometry_edit`'s
  byte `straighten` / `rotate_canvas` requests on the worker, with progress and Cancel
  (`CanvasGeometryJob.swift`, `core/tests/byte_canvas_geometry_cancel.rs`). The three job-less exports are
  retired end to end.
- `DONE` — The Crop straighten and the filtered Rotate Canvas run as ONE compound job at both depths
  (`CanvasGeometryJob.swift`): core geometry, described-artwork re-render, display preparation on the worker,
  published through `preparedEdit`. The byte straighten refuses a window outside the canvas by name; a
  whole-canvas unchanged result is MCP `crop`'s `changed:false` and records no step.
- `DONE` — The byte filtered Rotate Canvas and Straighten plans charge their destination once, a measured
  figure (`doc_source_geometry.rs`'s `# Admission: a filtered turn's destination`); the typed filtered turn
  takes × 5/4. `TransformWork.wait` and `PrecisionGeometryGesture.finishOutcome` admit their largest sequential
  stage (`CanvasGeometryJob`'s model; a true perspective set keeps core + artwork summed), re-admit the artwork
  and selection stages on the worker and drop the display term.
- `DONE` — The byte filtered turn charges `2·pixels` per layer and no canvas term, and saved channels at 5/4 of
  `(canvas + output) × C` (`# Admission: a filtered turn's layer and canvas terms`). A whole-canvas straighten that
  changes no pixel beeps "Nothing changed: …" at both depths (`CropSession.unchangedStraightenSentence`).
  `MoveSelectedPixels.run` and the no-control `geometrySetPrepared` admit per stage. Preview reservations keep
  core + artwork summed on purpose: a preview worker cannot re-admit its artwork stage without its own budget
  counting against it.
- `DONE` — The typed filtered turn (Straighten, filtered Rotate Canvas) is bounded by the one-shot
  `MAX_DESTRUCTIVE_WORK` (24e9), not the interactive `MAX_WORK_UNITS` (2e9), so a 24 MP 16-bit photo turns at
  Catmull-Rom (`precision_geometry_budget.rs`'s record). The Crop tool's preview sends `"interactive":true` and
  keeps the interactive limit. `core/tests/canvas_geometry_admission.rs` pins both plans and the refusal.
- `DONE` — A typed transform COMMIT (`affine`, `perspective`, `affine_set`, `perspective_set` without
  `"interactive":true`) is bounded by `MAX_DESTRUCTIVE_WORK` with a 5/4 scratch headroom (the unscaled plan
  covered only about 1.003× the peak); every preview (`PrecisionGeometryGesture.request`) sends the key. The rule
  and records are in `precision_geometry_budget.rs`. The `.transform` and `.crop` commit arms pass the job's
  control to the core, so Cancel stops the kernel.
- `DONE` (1), `DROPPED` (2) — (1) A preview worker counted its own `SelectionWorkBudget` reservation in
  `otherRetained` while its core plan charged the same bytes; both preview stages now run as
  `Reservation.runAsOwnLease`. (2) After an undo, a 24 MP 16-bit rotate's display is refused while the redo tail's
  document is still live; it needs a decision (discard the redo tail first, or a reduced projection).
- `DONE` — A placed image under a 16-bit ⌘T, Distort, Transform Again, set transform or filtered turn COMMIT takes
  `MAX_DESTRUCTIVE_WORK` (`retained_source_render::work_plan_within` at the edit's `budget::work_limit`), and so
  does the direct source `transform`, with the 5/4 headroom; previews keep `MAX_WORK_UNITS`. A one-shot plan did
  not charge a re-rendered source layer's unexecuted raster plane; it now charges that raster scratch again (work
  units still exclude it). The source `transform` accepts `"interactive"`. `core/tests/retained_source_work.rs`. A
  controlled commit now has no work ceiling; previews keep `MAX_WORK_UNITS`.
- `DONE` — Every preview owner whose worker re-reads the registry while its own stages charge its
  reservation runs as `runAsOwnLease` (the masked-transform, mask-sheet, profile, Evaluation, opacity-drag,
  Convert to Profile and adjustment-session previews). A described-artwork stage's scratch is a payload lease every admission counts;
  only the core stage is the reservation. Jobs-probe "preview own lease".
- `DONE` (partial) — The typed native reopen envelope's per-pixel decode term is the measured
  `precision_png::NATIVE_DECODE_BYTES_PER_PIXEL`, 27 B/px (was 32), the smallest that keeps every case ≥ 1.25×. It
  is shared by the edit-time envelope, the reader, the asset inspector and the PNG decoder's limit. Finished by
  Parts A and B below.
- `DONE` — `precision_png::native_png_encoded_bound` is the exact stored length of a native RGBA16
  PNG (stored deflate: R = h·(8w+1), Z = R + 5·⌈R/65,535⌉ + 6, L = 33 + Z + 12·⌈Z/(2^31−1)⌉ + 12; the proof is
  `precision_png`'s `# Encoded bound`), not 10 B/px + 1 MiB. The bytes are unchanged (no version bump).
  The plan's linear-light header term is exact too. `core/tests/native_png_bound.rs`.
- `DONE` (Part A) — The banded reopen. Each 16-bit plane is decoded row by row straight into 256×256 tiles
  (`precision_png_rows`). The reopen term is `precision_native_plan::reopen_scratch`: base = retained + file + band
  (16 rows of 8·w + 1 bytes plus 4 MiB) + bookkeeping, plus a quarter; the cap at the 27 B/px figure is gone (float
  unchanged; an 8-bit original still decodes whole). Interlaced native planes are refused. No format change. A
  single-layer 16-bit placed photo transformed up to 7842×5228 at the 2 GiB pin (a figure superseded by the 8 GiB
  minimum), and File ▸ New at 16 bits was bounded by the 512 MiB stored-plane record limit (8191² built, 8192²
  refused) until it was raised to 1 GiB and the decoder cap on a 16-bit plane dropped: every canvas within the
  100 MP document limit builds and reopens (10000² included). `core/tests/native_banded_reopen.rs`,
  `core/tests/precision_blank.rs`.
- `DONE` (Part B) — The two-layer turn. A 16-bit raster whose samples are all bitwise equal
  (`precision_sampling::uniform_source`, the ONE predicate the plan and `transform_image` ask) resamples with a
  constant read, one destination row at a time (`sampling_walk::resample_rows_into`), into tiles that share one
  `Arc` per uniform `((w, h), value)`; output is bit-identical. The plan charges such a plane's transient as one
  band (`uniform_band_bytes`: 8·dw·257 + 64 B per tile). A banded reopen keeps fresh tiles, so the saving does not
  survive a reopen: a second 17° turn of the reopened 24 MP two-layer document is admitted at 8 GiB. The 16-bit
  display after a reopen carries `ProjectionMargin`'s 6/16. `core/tests/uniform_plane_turn.rs`.
- `DONE` — The 16-bit native PNG is written row by row from the tiles (`precision_png::put_native_png` into
  fdeflate's `StoredOnlyCompressor`). The bytes are unchanged (pins in `core/tests/native_save_bytes.rs`) and the
  written length is checked equal to `native_png_encoded_bound`. The typed writer now streams to a temporary file
  (`core/src/native_stream.rs`), so the save plan no longer charges the file
  ([working limit](working-limit-ram-scaled.md)): its write term is base + ⌈base/4⌉, base = 2·header + one writer
  transient + `NATIVE_STREAM_WINDOW_BYTES` + bookkeeping, at both depths (`precision_native_plan`'s `# Streamed
  reader and writer`). The byte (8-bit) writer still assembles its file in memory.
- `DONE` — Decided: the reopen envelope keeps charging header bytes (a RAW payload among them) in
  both `reopen_retained` and `encoded_bound`, because the reader holds them twice and its own admission charges
  both (`precision_native_plan`'s `# Header and retained payloads counted twice` record).
- `DONE` — `RasterResources.admit(scratch:own:remedy:)` first checks the job's own need (scratch plus `own`, the
  non-evictable bytes its own document retains). At or over the whole working limit it refuses at once, with no
  measure or eviction: "This job needs N of working memory on its own, at least the L working limit, so closing
  other documents will not help." plus a remedy (`smallerJobRemedy`, or `depthJobRemedy`). Below the limit the
  measure → evict → measure order and the generic sentence are unchanged. Native and snapshot saves pass `own`.
- `DONE` — "Close other documents" appears only where closing them can help. Recovery checkpoints, exports, the
  export preview, Export Layers and `DocumentJobs.start` pass `RasterDocument.ownRetainedBytes()`. A refused
  recovery checkpoint evicts caches before rethrowing. Core `reserve`: every `RasterJobControl` carries the host's
  whole working limit (`dr_job_control_set_working_limit`). 16-bit plans: the native plan refuses with
  `too_large_to_reopen` or `too_large_to_save`, and a source past the per-image cap says "…one image holds at most
  about N megapixels…" (once 67 at 16 bits; one image now holds the 100 MP document limit at either depth, so the
  pixel-limit sentence answers first). Host sources: `HostSourceWork.place` returns its disclosures to the UI.
- `DONE` — The 8-bit native save and snapshot admit a core plan: `drdc_save_plan.rs`
  (`dr_doc_native_save_scratch_bytes`, `RasterDocument.nativeSaveScratchBytes()`, the one source for `CapturedSave`
  and `EditingStateArchive.capture`) answers `W + held + T + BOOKKEEPING_BYTES + SAVE_OVERHEAD_BYTES` (32 MiB). W
  bounds the body in the writer's order, and the writer reserves it; T is the largest per-plane encode peak beyond
  its own bytes; the module holds the formulas. No margin is added (orchestrator ruling: every gate-M case is
  covered ≥ 1.25× on the bound alone). The plan reads stored dimensions and lengths only, so it is the same with
  views live or evicted. Gate M's table is in the archived original; its peaks are asserted in
  `core/tests/native_save_plan8.rs` and `core/tests/native_save_plan8_45mp.rs`.
- `DONE` — The crash-recovery capture of a 45 MP 8-bit document. `RecoveryCoordinator.begin` runs
  `RecoveryCapacity.validate` (the reopen check: Σ reopened + max(temporary, projection), since decode and
  projection each admit themselves) before `admit` (the capture: the 8-bit native save plan + 4 MiB + masks), so a
  reopen refusal neither evicts nor takes the worker slot. A refusal is a `RecoveryCapacity.Refusal`, reported as
  `memory` by `get_recovery_status` beside `state: "failed"`. The editor's status bar shows "Recovery off", the
  panel lists the unprotected documents, and closing a document or changing the snapshots retries. Each session
  snapshot is written and reopened as its own full copy.
- `DONE` — The 8-bit editing-state archive takes both figures from a core reopen plan
  (`drdc_reopen_plan.rs`, `dr_doc_native_reopen_plan`, `RasterDocument.nativeReopenPlan()`), so a placed layer's
  shared source counts once: `reopenedRetainedEstimate = retained + selections`, `decodeEstimateBytes = decode +
  2·selections`, `decode = encoded_bound + serialized + T + 64 MiB`. It is the same with views live or evicted.
  The 8-bit reader's layer decode uses `into_rgba8()`. The archive charges the 8-bit projection at P + ⌈P/4⌉.
  Gate: ≥ 1.25×, or ≥ 1.15× for the two structural-bound cases (orchestrator ruling).
  `core/tests/native_reopen_plan8.rs`.
- `DONE` — The 16/32-bit display preparation is charged as the max of its two sequential stages:
  `projectionScratchEstimate()` = max(`render_scratch_bytes`, `render_output_bytes` + `display_scratch_bytes`) +
  `display_overhead_bytes`, three additive descriptor keys (display scratch 4·n + 1 MiB at u16, 12·n + 1 MiB at
  float; the overhead is page tables and edge-tile rounding). When the all-documents check refuses and at most one
  document is open, `admit` says "…and no other document is open to close."; the count is kept under
  `RasterResources`' lock. A fresh process peaks above the max-of-stages figure.
  `core/tests/precision_inspection.rs`.
- `DONE` — A re-rendered placed layer's never-run raster plane adds no work units to the 16-bit geometry plan; its
  scratch, skipped for previews and commits here, is charged again. An 8-bit placed transform commit takes the
  one-shot budget with the 5/4 headroom; the 8-bit masked ⌘T preview sends `interactive`. The 8-bit placed ⌘T plan
  still carries the ×6 unexecuted raster terms, within a 16 GB Mac's 8 GiB.
- `DONE` — The jobs-probe "transform admission" discriminates the byte selected-pixels admission:
  the core plan alone for a move, `max(core, selection)` for a transform.
- `DONE` — Measured, no app change: the 16-bit ⌥⌘T per-tick layer duplicate is well inside a frame,
  so no duplicate cache was built. The 8-bit ⌥⌘T parallelogram copy replays through its recorded
  `transform_layers` step.
- `DONE` — Part A: the 8-bit Healing Brush and Patch commit as nonmodal `DocumentJobs` jobs over a
  cancellable core heal (`dr_doc_heal_layer` / `dr_doc_heal_to_new_layer` take a nullable `DrJobControl`;
  `core/tests/heal_job.rs`), with a display pin, a panel only after 0.25 s, a busy-worker gesture guard and the
  rasterize prompt before the solve. Part B: the 8-bit `heal_stroke` and `patch_region` at both depths commit
  through the same job and reply through `finishJobCall`; a result `applyEdit` refuses ends the job `refused`.
  Follow-ons, all shipped:
  - Esc and ⌘Z/⇧⌘Z cancel the running UI job; MCP `undo`, `redo` and `restore_history` refuse by name while a
    job runs.
  - No empty undo steps: `applyEdit`'s shared preflight (`ImageDocument+EditPreflight.swift`) runs before every
    `withUndoGroup`.
  - Retouch admission is stated in each kernel's own measured terms: plate copies charged, 136 B (byte) and 72 B
    (typed) per fill window pixel, one component record per component, a 9n new-layer canvas term.
  - Kernels poll every 4096 pixels or per tile, and `make cancel-bench` times the heal and fill gaps.
  - 16-bit strokes run as jobs and `heal_stroke` takes `background` at 16 bits.
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — Codecs that still cancel only between stages.
  Since 2026-11-12 the export workspace's encoders and the 16-bit PNG and TIFF opens poll inside the codec call,
  every 64 KiB of encoded bytes (`core/src/cancel_io.rs`; PNG, WebP, GIF and JPEG XL only while their finished output
  is copied out), and Save, Save As, Save a Copy and Open take a job control, so their panels show Cancel
  (`CapturedSave.run`, `ImageDocument.openWork`). The float and colour-mode decoders cancel at their stage
  boundaries. RAW (CIRAWFilter) and movie-frame (AVFoundation) decodes are host calls with no cancel inside: they are
  cancelled before and after each camera or frame stage.
- `OPEN` [#9](https://github.com/kpgalligan/doctored/issues/9) — Broader cancellable/regional processing: the
  remaining full-frame fallbacks, stage-only render and preparation cancellation, and the regional latency regression.
- `OPEN` [#10](https://github.com/kpgalligan/doctored/issues/10) — Tiled document storage: masks, channels and typed
  mask planes on tiles, a measured history budget and one bounded cache registry.
- `DONE` — Higher precision storage — Phase 7 (typed 16-bit) and 11F (float).
- `DONE` — Budgeted history and recovery — Phase 5 ([phase-5-recovery-history-status.md](phase-5-recovery-history-status.md)).
- `DONE` — "Comprehensive legacy-operation admission": `RasterResources.shared.admit` now guards 113 call
  sites across 68 Swift files (not audited for completeness).
- `DONE` — Optimized Swift unqualified (Release `make app`, qualified from Phase 7E on).
- `DONE` — the 24 MP Catmull-Rom Crop straighten and ⌘T (over the interactive work cap) preview at reduced resolution: the geometry plan JSON reports `work_units`, the gesture builds a proxy of factor `ceil(sqrt(units / 1e9))` off main and lands with "Preview current (reduced resolution; the commit renders at full size)"; commits never see it (`GeometryProxyPreview.swift`, jobs probe "transform admission").
- `DROPPED` — (2) of the preview-worker entry above: after an undo the redo-tail document keeps a 24 MP 16-bit display from fitting.
- `DROPPED` (2026-10-30) — a decision on the 8-bit placed ⌘T at 100 MP (`MAX_PIXELS`), which plans about 10.6 GB, over a 16 GB Mac's 8 GiB: only RAW layers remain placed, and a RAW opens at 8 GiB only to about 90 MP, so the case barely arises on the minimum machine; the RAM-scaled limit admits it on larger Macs. Exact typed edits of a placed layer and an exact 8-bit Rotate Canvas no longer count the never-run source render's work.
- `DROPPED` (2026-10-30) — working-memory estimate follow-ups (the fresh-process display peak, typed fill plan coverage, the legacy 16-bit `blur` σ4 work margin): calibration of shipped admission figures with no user-facing failure; admission is not an RSS ceiling.
- `DROPPED` — recovered and loaded session snapshots share no memory.
