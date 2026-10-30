# Phase 4 — jobs and resource foundations (ADR)

- **Status:** Accepted — open items remain
- **Phase / dates:** Phase 4 (4A–4D) of the [implementation plan](professional-photo-editor-implementation-plan.md), 2026-09-09
- **Original:** [archive/phase-4-jobs-resource-status.md](../archive/phase-4-jobs-resource-status.md) — full history, measurements, receipts
- **Key commits:** `968e0ee` (Phase 3 was `f34201b`)

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
- **Cooperative cancellation through an explicit FFI token** (`RzJobControl`, `core/src/job_control.rs`):
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
- **Working limit = min(physical RAM / 3, 2 GiB)** (Superseded by [working-limit-ram-scaled](working-limit-ram-scaled.md).); projection refresh evicts core caches above 128 MiB; job
  admission evicts reusable caches before measuring and reserves scratch estimates before expensive work.
  These are admission estimates, not an OS ceiling: a job, render or save may refuse. Surfaced as
  Help > Memory Usage and MCP `resource_usage`.
- **4D measured migration:** spot healing, save/export, final raster affine/perspective transforms
  (`app/Sources/TransformWork.swift`) and RAW development moved to the worker because 24/48 MP medians
  justified it. Save captures source, projection and export settings together, writes atomically on the
  worker and marks only that captured handle saved; newer edits stay dirty, Undo to it becomes clean, export
  copies never change the saved handle, failed writes keep the previous file and dirty state.
- **Explicit boundaries:** save/export and RAW kernels had responsive but noncancellable progress (no Cancel
  button; transform kernels had it too until they were made
  cancellable); Healing Brush/Patch commits stayed synchronous (8-bit UI commits moved to nonmodal
  `DocumentJobs` jobs — their rasterize prompt now comes before the solve, and the panel's Cancel, Esc on the
  canvas and ⌘Z/⇧⌘Z stop them; their MCP byte twins followed); storage
  stayed straight RGBA8; no tiling and no native version change.

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

## Open items (verified 2026-09-26)
- `DONE` — Byte final Free Transform, Distort and
  `transform_layers` commits (UI, MCP, Actions, 16-bit MCP) are cancellable: `TransformWork.wait` runs the
  core kernel and the display preparation as two children of one `RasterJobControl`
  (`app/Sources/TransformWork.swift`), and the byte affine/perspective set polls its resample, linked and
  group masks and a retained member's re-render (`core/tests/byte_geometry_cancel.rs`). The 16-bit
  move/⌘T/crop gesture commits and Layer ▸ Warp (which already carried a control) now also treat a late
  cancel as a cancel. Every user cancel surfaces as one `JobCancelled` that never alerts;
  `scripts/jobs-probe/transform_cancel.swift` drives the panel's Cancel. Done since: the byte
  raster resample reports stage 6 per destination row inside each member's slice (0.8 of it,
  `LINKED_MASK_RASTER_SHARE`, when a linked mask follows — the mask resample reports no rows), in the
  affine and true-perspective sets alike (Straighten and Rotate Canvas layers too), and `make cancel-bench`
  runs `core/tests/byte_geometry_cancel.rs`'s ignored 24 MP bench. Recorded:
  `24 MP (6000×4000) affine set cancel inside the resample: 0.735 ms cancel-to-return (issued at
  2454.3 ms; entry checkpoint 4, k 11352, issued at 11356, 11359 at return (≥ n0 + k); progress 0.491397
  stage 6 at return); c 6.750 ms (longest gap, after checkpoint 4); bound T_f + c = 7.750 ms (T_f 1 ms);
  ≈0.00 gaps past the floor; PASS` and `24 MP (6000×4000) true-perspective set cancel inside the
  resample: 0.572 ms cancel-to-return (issued at 1632.9 ms; entry checkpoint 5, k 8192, issued at 8197,
  8200 at return (≥ n0 + k); progress 0.488 stage 6 at return); c 1.739 ms (longest gap, after
  checkpoint 683); bound T_f + c = 2.739 ms (T_f 1 ms); ≈0.00 gaps past the floor; PASS`.
- `DONE` — The 8-bit Crop straighten and the filtered
  Rotate Canvas (UI, MCP `crop`/`rotate`, Actions replay) commit through `rz_doc_geometry_edit`'s byte `straighten` /
  `rotate_canvas` requests on the bounded worker with progress and Cancel/Esc
  (`RasterDocument.performingByteCanvasGeometry`, `app/Sources/ByteCanvasGeometry.swift`); the core polls per layer
  and then per channel (`core/tests/byte_canvas_geometry_cancel.rs`). The three job-less exports
  (`straighten_layers`, `transform_channels`, `rotate_canvas`) are retired end to end.
- `DONE` — The Crop straighten and the filtered Rotate
  Canvas run as ONE compound job at both depths (`RasterDocument.performingCanvasGeometry`,
  `app/Sources/CanvasGeometryJob.swift`): the core geometry, then the described-artwork re-render (no second
  "Rendering … Artwork" wait), then the display preparation on the worker, published through `preparedEdit`; the
  panel reveals after `DocumentJobs.panelDelay`. The byte straighten refuses a window outside the canvas by name before
  any work, answers an inner-window identity with the plain crop and a stack with nothing to write as unchanged layers,
  so a whole-canvas unchanged result is MCP `crop`'s `changed:false`, which (with `canvas_size`'s) records no step.
  The jobs-probe case "canvas geometry cancel" covers every Cancel.
- `DONE` — The byte filtered Rotate Canvas and Straighten plans
  charge their destination once, a measured figure (`doc_source_geometry.rs`'s first `# Admission`: 12 MP 17° max E
  200.8 MB against plans of 692 / 608 MB, was 2,212 / 1,708 MB), with a straighten's saved channels charged at
  `(canvas + output) × C`, so a 12 MP 8-bit photo rotates; the typed (16-bit) filtered turn takes × 5/4 (404 → 505 MB
  against max E 401 MB). `TransformWork.wait` and
  `PrecisionGeometryGesture.finishOutcome` admit their largest sequential stage (`CanvasGeometryJob`'s model; a true
  perspective set keeps core + artwork summed), re-admit the artwork and selection stages on the worker and drop the
  display term; the jobs-probe case "transform admission" proves a 26 MP Rotate Canvas and a 22 MP ⌘T the sum
  refused.
- `DONE` — Geometry admission leftovers. The byte filtered turn's layer and canvas terms are measured
  (`doc_source_geometry.rs`'s second `# Admission`: nine 17° cases, one an 8000×6000 layer overhanging a 4000×3000
  canvas, which sets the layer factor): `2·pixels` and no canvas term (were 6 and 8), and both turns charge saved
  channels at 5/4 of `(canvas + output) × C`, so an 8000×6000 8-bit rotate fits (plan 1,604 MB, was 2,756 MB). The
  Crop tool's whole-canvas straighten that changes no pixel beeps "Nothing changed: straightening the whole W × H
  canvas by A° changes no pixel." at both depths (`CropSession.unchangedStraightenSentence`, MCP `crop`'s no-op text
  unchanged), and a 16-bit session kept after `.unchanged` or `.refused` re-arms its gesture so the box previews
  again. `MoveSelectedPixels.run` (the byte floating-transform commit) and the no-control `geometrySetPrepared` admit
  per stage like `CanvasGeometryJob`; preview reservations keep core + artwork on purpose, because a preview worker
  cannot re-admit its artwork stage against the live core output without its own `SelectionWorkBudget` bytes counting
  against it. The jobs-probe "transform admission" now proves a 37 MP Rotate Canvas, the 22 MP ⌘T and a 30 MP 16-bit
  Crop (`finishOutcome`) that the sum refused, printing each fixture's residency (8 bits 12.00 B/px: layer tiles,
  contiguous view, display image; 16 bits 20.04 B/px); the ⌘T's former ~90 MB shortfall was a transient second
  display image alive at the free reading, not a missing term. "canvas geometry cancel" (d) cancels a 12 MP Rotate
  Canvas and (g) checks the no-op sentence at both depths. Its two open items, the 16-bit Catmull-Rom straighten
  refused above ~21 MP and the byte selected-pixels admission no probe discriminated, are done below.
- `DONE` — The typed filtered turn (Straighten, filtered
  Rotate Canvas) is bounded by the one-shot `MAX_DESTRUCTIVE_WORK` (24e9) instead of the interactive
  `MAX_WORK_UNITS` (2e9), so a 24 MP (6000×4000) 16-bit photo straightens and rotates at the default Catmull-Rom
  sampler. Measured (`precision_geometry_plan`'s record; 3 runs, one case per process, E = peak − pre-edit
  footprint): 7° straighten of (300, 200, 5400, 3600) max E 678.5 MB against a plan of 851.3 MB (1.255×, 2.18e9
  work units), 17° straighten 810.9 / 1,016.2 MB, Rotate Canvas 17° 810.9 / 1,016.2 MB (2.78e9 units); the 5/4
  headroom is unchanged and retained + plan stays under the 2 GiB working limit (1,208 MB). The Crop tool's preview
  is NOT given the new limit: at 24 MP one preview landed in 2.49 s, but the next angle change was refused on memory
  (2,183,707,515 bytes against 2 GiB), so the preview sends `"interactive":true` on its typed `straighten`
  request and keeps the interactive limit — unavailable there exactly as before — while the commit succeeds. The
  jobs-probe "transform admission" runs its 30 MP 16-bit Crop at Catmull-Rom and drives the 24 MP Crop tool through
  two angle changes (preview refused by name, commit published in 2.32 s); `core/tests/canvas_geometry_admission.rs`
  pins both plans and the interactive refusal. The Crop preview's memory refusal
  and the 16-bit ⌘T of a 24 MP layer at Catmull-Rom are both done below.
- `DONE` — A typed transform COMMIT (`affine`,
  `perspective`, `affine_set`, `perspective_set` without `"interactive":true`) is bounded by `MAX_DESTRUCTIVE_WORK`
  with a 5/4 scratch headroom, so 16-bit ⌘T, Distort, Transform Again and set transforms of a 24 MP layer commit at
  Catmull-Rom; every preview (`PrecisionGeometryGesture.request`) sends the key and keeps `MAX_WORK_UNITS`. The rule
  and records moved to `precision_geometry_budget.rs`. Measured (6000×4000 RGBA16, bicubic, 3 runs, one case per
  process): `affine` 17° and `affine_set` max E 810,975,832 against a plan of 1,016,225,660 (1.2531×, 2.78e9 units),
  `perspective` and `perspective_set` (a true perspective) 767,853,144 / 767,836,760 against 962,889,980 (1.2540×,
  2.58e9 units); retained + plan ≤ 1,208 MB. The unscaled plan covered only ~1.003× the peak. The `.transform` and
  `.crop` commit arms now pass the job's control to the core, so Cancel stops the kernel (24 MP ⌘T and Crop: ~0.11 s
  after the click; ⌘T measured 1.77 s before). Jobs-probe "transform admission" H1; `core/tests/canvas_geometry_admission.rs`.
  Not covered: placed-image layers under ⌘T or a turn (`retained_source_render::work_plan`'s per-layer cap,
  done below); reduced previews at Catmull-Rom.
- `DONE` (1); (2) not fixed — (1) The 24 MP 16-bit Crop
  preview's memory refusal was the preview worker counting its OWN `SelectionWorkBudget` reservation in
  `otherRetained` while its core plan charged the same bytes: refused at N = 2,183,707,515 = R + L + (L − 65,536)
  with R 481,084,739 and L 851,344,156 (bilinear, 7°; the 6° preview too at 2,148,269,595). No stale reservation or
  leaked handle (snapshots through `RasterResources.liveHandleSummary()`: only the displayed preview image). Both
  preview stages now run as `Reservation.runAsOwnLease`, and two successive 24 MP bilinear previews land (probe H2).
  A Catmull-Rom 24 MP preview still exceeds the interactive work cap. (2) The post-undo 24 MP rotate's display is
  refused because the redo tail's 5400×3600 document (≈ 243 MB with its latent off-canvas storage) is still live
  when the new display is admitted; nothing else is held. That needs a decision (discard the redo tail before a
  new edit's display, or a reduced projection). Other preview owners' own-reservation double count: done below.
- `DONE` — A placed image (a retained source) under a
  16-bit ⌘T, Distort, Transform Again, set transform or filtered canvas turn COMMIT takes `MAX_DESTRUCTIVE_WORK`
  (`retained_source_render::work_plan_within` at the edit's `budget::work_limit`), and so does the direct source
  `transform` the ⌘T of a single placed image sends, with the 5/4 headroom; previews keep `MAX_WORK_UNITS`. Under a
  one-shot edit the plan stops charging a re-rendered source layer's unexecuted raster plane. The source
  `transform` accepts `"interactive"`, which also fixes the 63467a5 regression: the 16-bit ⌘T preview of a single
  placed image had been refused as "unknown field `interactive`" (jobs-probe "placed image cmd-t preview", failing
  at 63467a5's successor c7809a0 and passing now). Measured on a 5700×3800 placed 16-bit photo (bicubic, 3 runs, one
  case per process; `precision_geometry_budget`'s record): source `transform` 17° max E 731,972,136 against a
  plan of 917,677,360 (1.2537×, 2.59e9 units), `affine_set` and Rotate Canvas 17° 731,955,752 / 731,972,136 against
  922,922,700 (1.2609×, 5.10e9 units), `perspective` 693,502,480 against 874,722,220 (1.2613×, 4.75e9 units);
  retained + plan ≤ 1,097 MB. At 6000×4000 every case is still refused, by the typed native reopen envelope
  (2,367,746,912 bytes; the result's raster beside its 24 MP original) before any work limit is asked.
  `core/tests/canvas_geometry_admission.rs`, `core/tests/retained_source_work.rs`.
- `DONE` — Every preview owner whose worker re-reads the
  registry while its own stages charge its reservation now runs it as `runAsOwnLease`: the 8-bit masked
  `TransformMaskPreview` geometry stage, `MaskSheetPreview`, the typed profile preview
  (`PrecisionProfileEditing`), the typed Evaluation sheet preview, `PrecisionOpacityDrag`, the Convert to Profile
  sheet's retained-state preview and `PrecisionAdjustmentSession`'s preview, whose Swift-held coverage moved to a
  `PrecisionEditPayload` lease. A described-artwork stage (text/shape in the moving set) keeps earlier parts'
  coverage live while later parts admit, so in `TransformMaskPreview` and the 16-bit `PrecisionGeometryGesture`
  its scratch is a payload lease every admission counts and only the core stage is the reservation. Owners
  that already subtract their own lease, and those whose worker never reads the registry, are unchanged.
  Jobs-probe "preview own lease": the typed opacity drag's preview lands with P + 64 KiB ≤ free < 2P + 64 KiB (and
  fails with the drag back on `run`), and a 16- and 8-bit text-layer ⌘T preview holds its ≥ 48 MiB artwork lease.
- `DONE` (partial) — The typed native reopen envelope's
  per-pixel decode term is the measured `precision_png::NATIVE_DECODE_BYTES_PER_PIXEL`, 27 B/px (was 32), shared by
  the edit-time envelope, the reader, the asset inspector and the PNG decoder's limit. Measured on 17°-turned
  5700×3800 placed 16-bit images (A17, S17, P, R17 and two noise variants; `open_native`, 3 runs, one case per
  process; `precision_png`'s record): reopen max E 1,463,944,320 (P 1,386,365,984) against the reader's figure
  2,022,923,853 at 32, so K = 27 is the smallest that keeps every case ≥ 1.25× (1.2589–1.2634×). Outcome B: at
  6000×4000 the turns still need 2,175,013,712 bytes (2,175,003,646 in the app), so 24 MP is still refused; the
  largest 3:2 placed image that commits is 5961×3974 (envelope 2,146,998,404, 485,244 spare, so about 242 KB more
  header still admits; header bytes count twice). Its four commits measured max E 800,080,448 (P 758,350,376)
  against plans of 1,003,102,460–1,008,347,800 (P 955,974,780), ≥ 1.2538×. MCP drive (port 4943, a non-uniform
  RGB16 PNG with a Display P3 iCCP and a 70 KB eXIf, placed into a 16-bit document, background deleted,
  `transform_layers` 17° bicubic): 5961×3974 ok; 6000×4000 refused "precision operation needs 2175003646 bytes".
  Banding the reopen (decoding PNG rows straight into tiles) is the follow-up that would reach 24 MP (follow-up
  filed by coordinator). `core/tests/canvas_geometry_admission.rs`. Since the banded native write the 6000×4000
  reopen is measured (`precision_png`'s M2 record; `open_native`, 3 runs, one case per process,
  `MallocLargeCache=0`): max E 1,427,801,096 (A17), 1,427,801,120 (S17), 1,344,193,472 (P, a 7800×4600 result),
  1,427,784,712 (R17) and 1,427,817,480 (N4-R17) against the reader's figures 2,047,870,782 / 2,047,870,782 /
  1,933,202,749 / 2,047,870,814 / 2,047,870,814 (1.4343–1.4382×) and H 2,047,870,990–2,047,871,054
  (P 1,933,203,049), so K = 27 holds at 24 MP (R-ok).
- `DONE` — `precision_png::native_png_encoded_bound` is the
  exact stored length of a native RGBA16 PNG (stored deflate: R = h·(8w+1), Z = R + 5·⌈R/65,535⌉ + 6, L = 33 + Z +
  12·⌈Z/(2^31−1)⌉ + 12; the proof is `precision_png`'s `# Encoded bound`), not 10 B/px + 1 MiB. The bytes
  are unchanged (no version bump) and every older file still reads. The plan's linear-light header term counted 12
  of the 16 bytes its writer writes; it is exact now. At 6000×4000 turned 17° the two plane bounds drop from
  627,563,552 to 500,421,006 and the reopen envelope to 2,047,871,054 (99,612,594 spare), so a single-layer 24 MP
  16-bit placed image's commits admit; the largest 3:2 size is 6144×4096 (6147×4098's turns need 2,149,464,576).
  Measured at 6000×4000 (3 runs, one case per process; `precision_geometry_budget`'s record): max E 810,910,272
  (P 767,771,200) against plans of 1,016,175,680–1,021,421,020 (P 968,088,220), ≥ 1.2531×. The 16-bit save of the
  result was still refused here ("needs 2355988889 bytes"); since
  the banded native write it saves and its reopen measures R-ok at 6000×4000. MCP drive (port 4955, a non-uniform 6000×4000 RGB16
  PNG placed into a 16-bit white document, background deleted, `transform_layers` 17° about the centre, bicubic):
  the turn commits (a 6908×5580 layer; HEAD refused "precision operation needs 2175003646 bytes"), and `save_copy` to
  `.rz` is refused "This job needs more working memory than the current budget…" with the document still open with
  unsaved changes and no file written. `core/tests/native_png_bound.rs`
  (`a_native_png_is_exactly_its_stored_bound`, `a_placed_document_s_source_png_is_exactly_its_stored_bound`,
  `the_plan_s_png_term_is_exact_at_every_size`), `core/tests/canvas_geometry_admission.rs`
  (`a_24_mp_16_bit_placed_image_commits_with_catmull_rom`).
- `DONE` (Part A) — Part A, the banded reopen. The typed
  native reopen is banded: each 16-bit plane is decoded row by row through png's row reader straight into 256×256
  tiles (`precision_png_rows`), so a reopen holds the file, the decoded tiles and one band instead of two extra
  whole-plane 8 B/px copies. The reopen term is `precision_native_plan::reopen_scratch`: base = retained + file + band
  (16 rows of 8·w + 1 bytes plus 4 MiB) + bookkeeping, plus a quarter, capped by HEAD's 27 B/px figure (float
  unchanged; an 8-bit original still decodes whole). Interlaced native planes are refused (no Rasterize writer emits
  one). No format change. Measured (`precision_png_rows`' record; `open_native`, 3 runs, one case per process,
  `MallocLargeCache=0`, E = interval peak − pre): 6000×4000 placed R17 max E 1,003,782,864 against base 1,007,995,774
  and A 1,259,994,718 (1.2552×; HEAD max E 1,427,784,712, A 2,047,870,814), noise 1.2553×; white background + placed
  photo turned 17° max E 1,005,175,648, A 1,740,018,175 (≥ 1.45× after allowing for the up-to-192,000,000 B
  `phys_footprint` undercount the record states); placed 8064×6048 at identity max E 1,564,705,896, A 1,960,119,563
  (1.2527×); RS0 / RS1 1.2799× / 1.2748× (the 1.17–1.18× shortfall gone for 16-bit originals). Every max E
  is below its base and H ≥ A. The remaining part, the two-layer turn, was then still refused: Rotate Canvas 17° of
  the white background + placed photo document failed the turn's own admission (needs 2,416,422,387 B), before the
  reopen plan (Part B below admits it); that plan has no decode term, so banding does not reach it. The route is a uniform-plane path through
  the turn (the ≤ 4-tile white background is charged today as a full 192,000,000 B contiguous copy plus output plus
  adoption, which the resample really binds), then a new two-layer measurement before the 5/4 headroom can be relied
  on. A single-layer 16-bit placed photo now transforms up to 7842×5228 (3:2), and File ▸ New at 16 bits is bounded by
  the 512 MiB stored-plane record limit (8191² builds, 8192² refuses with "A 16-bit 8192 × 8192 document is too large
  to reopen at 16 bits…"). MCP drive (port 4961, two owned instances): `new_document` 6000×4000 depth 16 white,
  `place_image` a non-uniform 6000×4000 RGB16 PNG, `transform_layers` 17° bicubic on the placed layer and `save_copy`
  to `.rz` (692,445,161 bytes) succeed, and a fresh instance's `open_document` reopens it with 2 layers.
  `scripts/host-still-admission/verify.py` exits 0 and its recorded R-heic10-48 and R-avif10-48 now open 8064×6048 at
  16 bits (`rgba16`), no longer refused with the "save and reopen" sentence. `core/tests/native_banded_reopen.rs`,
  `core/tests/canvas_geometry_admission.rs`, `core/tests/precision_blank.rs`.
- `DONE` (Part B) — Part B, the two-layer turn. A 16-bit
  raster whose samples are all bitwise equal (File ▸ New's white background; `precision_sampling::uniform_source`, the
  ONE predicate the plan and `transform_image` ask) resamples with a constant read, one destination row at a time
  (`sampling_walk::resample_rows_into`, the walk's one body, which `resample_into` now calls over all rows), into a
  band of tiles that shares one `Arc` per uniform `((w, h), value)` (`TilePlane::from_rows_shared_with_job`, adopted
  by `PrecisionImage::from_rgba16_tiles_unseeded`): no contiguous source copy, contiguous output or adoption copy.
  Every output sample is bit-identical to HEAD (SHA-256 pins captured before the change over three values × Rotate
  Canvas 17° and −33.3° at all four filters, Straighten 7°, layer affine 17°, perspective, rotate90 and
  flip_horizontal, plus one-corner twins that take the former path); only the blank pass's checkpoints go. The plan
  charges a uniform raster plane's transient as one band (`uniform_band_bytes`: 8·dw·257 + 64 B per tile) instead of
  source copy + max(temporary, adoption); its output (n·8) and work units are unchanged, and float, mask, channel,
  Resize and non-uniform planes charge what HEAD did (the per-record bookkeeping term grows by 16 B, the new
  `PlaneWork::uniform` flag). `TRANSFORM_HEADROOM` and the turn's 5/4 are unchanged. Measured
  (`precision_geometry_budget`'s two-layer record; 3 runs, one case per process, `MallocLargeCache=0`, E =
  interval peak − pre around `geometry`): (U) the white 6000×4000 background alone, Rotate Canvas 17° bicubic, max E
  21,332,016 against scratch 408,512,840 (19.15×); (2L) the same plus a placed 6000×4000 photo, max E 821,101,168
  against scratch 1,424,691,000 (1.7351×; ≥ 1.6595× after allowing for the up-to-37,400,568 B `phys_footprint`
  undercount the record states), plan total 1,617,943,395 where HEAD refused "needs 2416422387 bytes". The baseline
  twins read E 0. The turned (2L) result reopens at its plan envelope (2,030,964,413); its reopened background
  retains 308,386,993 B (the banded reopen keeps fresh tiles, so the shared-tile saving does not survive a reopen),
  and a second Rotate Canvas 17° of it is refused by the native reopen envelope ("At 16 bits the result needs more
  than the 2 GB working limit to save and reopen on its own…") at both the core plan and the host at the 2 GiB reference
  limit (reopen plan 2,663,683,195). Re-measured at the 8 GiB limit a 16 GB Mac gets (batch 163, 3 runs, MCP, E =
  interval peak − pre): the second turn is admitted (scratch 2,810,377,520, max E 1,781,793,968, 1.577×) and its `.rz`
  save too (write scratch 1,333,945,560, max E 1,019,888,048, 1.308×; the file is 1,061,842,238 B, exactly the reopened
  app document's own `encoded_bound` (batch 164; an in-process resave writes the same length). Batch 163's 22 B
  "overshoot" compared it with the probe twin's bound 1,061,842,216, which is that twin's own file length: the twin's
  header differs by its layer name ("Photo" against "P16", −2 B) and its source id (the 12-byte `uniform-turn`
  against a 36-byte UUID, written once, +24 B), so no bound is exceeded; `MAX_NATIVE_BYTES` does not bind). The
  reopen in a fresh instance is admitted: the core `open_native` is covered (in-process max E 2,129,004,032
  against the reader's 2,663,682,924, 1.251×), and since batch 165 the 16-bit display that follows it carries
  `ProjectionMargin`'s 6/16: the 1,790,050,327 the
  reopened document retains plus the margined display 2,004,925,382 (structural 1,458,127,550, render-bound) is
  3,794,975,709, 1.505× the app's max E 2,521,712,392 (batch 164's display estimate was 0.9993×). The
  second turn's own plan, 551,091,513 + 2,810,377,520 = 3,361,469,033, exceeds an 8 GB Mac's 2,862,612,480. MCP drive (port
  4971, two owned instances, `RZ_BACKGROUND=1`, `RZ_AUTOMATION_ALERTS=record`): `new_document` 6000×4000 depth 16
  white, `place_image` a non-uniform 6000×4000 RGB16 PNG, `rotate` 17° bicubic (ok, canvas 6908×5580), `save_copy`
  to `.rz` (808,828,773 bytes) succeed, a fresh instance's `open_document` reopens it with 2 layers, and no
  `[AutomationAlerts]` line was logged. `core/tests/uniform_plane_turn.rs`.
- `DONE` — The 16-bit native PNG is written row by row
  from the tiles: `precision_png::put_native_png` streams `row_segments` through one 8·w-byte row into fdeflate's
  `StoredOnlyCompressor`, straight into the `.rz` body buffer, which the writer reserves once at `encoded_bound`.
  The bytes are the former image/png path's (SHA-256 pins captured before the change, layer ids zeroed:
  `core/tests/native_save_bytes.rs`, eleven shapes including placed, projective, masked, linear-light, float, warp
  and byte documents with 16- and 8-bit sources), and the length written is checked equal to the exact PNG bound. The
  plan's write term is the file, the header, one row and bookkeeping plus a quarter (2·`encoded_bound` + 1 MiB at
  float, unchanged). Measured (`precision_png`'s `# Banded native write`; 3 runs, one case per process,
  `MallocLargeCache=0`, E = interval peak − pre around `save_native`): HEAD's save peaked at 32 B/px of the largest
  plane (5700×3800 R17 E 1,114,047,256); banded, every case's max E sits below the base (6000×4000 R17 500,679,016
  against write 632,162,413, 1.2626×; perspective 1.2631×; an unplaced raster 1.2706×; noise 1.2625×), so the
  6000×4000 placed result saves at 1,132,553,784 (was 2,355,988,889). The whole `.rz` is still built in memory
  before it is written. MCP drive (port 4959, a non-uniform 6000×4000 RGB16 PNG placed into a 16-bit white document,
  background deleted, `transform_layers` 17° about the centre, bicubic): `save_copy` to `.rz` now succeeds (a
  500,426,388-byte file; HEAD refused "This job needs more working memory…"), and a fresh instance's
  `open_document` reopens it as one 6908×5580 layer at (−454, −790) whose source is 6000×4000 at master depth 16.
  `core/tests/native_save_bytes.rs`.
- `DONE` — Decided: the reopen envelope keeps charging
  header bytes (a RAW payload among them) in both `reopen_retained` and `encoded_bound`, because the reader holds them
  twice (the file stays alive through `parse`; metadata and RAW payloads are copied out of it) and its own admission
  charges both. Measured on 4000×3000 placed images reopened without and with a 25,000,000-byte RAW
  (`open_native`, 3 runs, one case per process; `precision_native_plan`'s record): the RAW adds 50,069,552 to the
  peak when the layer raster is the largest plane (2.0028×) and 49,840,176 when a source PNG is (1.9936×). With
  the exact PNG term the 24 MP placed image with a 25 MB RAW commits. `core/tests/canvas_geometry_admission.rs`
  (`a_raw_backed_24_mp_placed_image_commits`, `a_raw_backed_turned_placed_image_reopens_within_its_envelope`).
- `DONE` — `RasterResources.admit(scratch:own:remedy:)`
  first checks the job's own need (scratch plus `own`, the non-evictable bytes its own document retains). When that
  reaches the whole working limit it refuses at once, with no measure or eviction: "This job needs N of working
  memory on its own, at least the L working limit, so closing other documents will not help." plus a remedy
  (`smallerJobRemedy`, or `depthJobRemedy` naming Image ▸ Mode ▸ Convert Precision… for a 16/32-bit document).
  Below the limit the measure → evict → measure order and the generic sentence are unchanged. Native saves (File ▸
  Save, `save_copy` `.rz` at every depth, Save Portable Image Copy) and snapshot saves pass `own`: typed is the
  descriptor's `retained_bytes`, 8-bit the single-document report's `unique_retained_bytes − cache_bytes`. Every
  other caller passes none, so it says "too large" only when its scratch alone reaches the limit. MCP drive: an
  8-bit 9000×9000 two-layer `save_copy` `.rz` gives the new sentence (3.99 GB; the 8-bit estimate counts live
  contiguous layer views four times); two distinct 7500×7500 layers
  with caches evicted (scratch 1,867,108,864 below the limit) give it at 2.16 GB through the own term alone.
- `DONE` — "Close other documents" now appears only where
  closing them can help. Recovery checkpoints, File ▸ Export / `export_image` / `export_preview` / Quick Export, the
  export sheet's preview, Export Layers and `DocumentJobs.start` pass the document's own retained bytes
  (`RasterDocument.ownRetainedBytes()`: the single-document report's `unique_retained_bytes − cache_bytes`, either
  depth) to `admit`; an 8-bit export with sharpening names "Export without sharpening". A refused recovery
  checkpoint evicts caches before rethrowing, so the periodic retry recaptures without live views.
  Core `reserve`: every `RasterJobControl` sets the host's whole working
  limit through `rz_job_control_set_working_limit`, and a reservation over the scratch limit that also reaches it
  says "on its own … closing other documents will not help". 16-bit plans:
  the native plan's reopen admit refuses with
  `too_large_to_reopen` ("At 16 bits the result needs more than the 2 GB working limit to save and reopen on its
  own…") and every fixed cap with `too_large_to_save`; a source past the per-image decoder cap says "A W × H image is
  too large to keep at 16 bits: one image holds at most about 67 megapixels…"; a 16-bit host-source canvas is built
  with `blankRGBA16`. Host sources:
  `HostSourceWork.place` returns its disclosures, which the UI's Place/Replace/Refresh show in the "Image Source
  Notice" and `replace_image_source` now returns like `place_image`. MCP drive (port 4967): two distinct 5600×5600
  8-bit layers, sharpened `export_preview` and `export_layers` refuse at 2.12 GB on its own (unsharpened control
  clean); two distinct 7500×7500 layers with caches evicted refuse `write_recovery` at 2.16 GB on its own, status
  `failed` with the same error, and a 1000×1000 control checkpoints. A 7000×7000 16-bit `canvas_size` to 7100×7100
  commits (the background raster keeps its 7000×7000 extent, so the plan does not refuse); its display
  preparation used to refuse with "Close other documents" from the all-documents check, fixed by the
  display-preparation entry below; `image_size` to
  8000×8000 gives the reopen sentence and to 9000×9000 the save sentence. A 10000×10000 8-bit `save_copy` `.rz`
  refuses at 3.42 GB on its own (no file to reopen). A gain-map HEIC's `place_image` and `replace_image_source` both
  return "The HDR gain map is not applied…". `core/tests/job_control.rs`, `core/tests/host_source_packing.rs`.
- `DONE` — The 8-bit native save and snapshot admit a core plan
  instead of `referenced_bytes × 4 + 64 MiB`. `rzdc_save_plan.rs` (`rz_doc_native_save_scratch_bytes`,
  `RasterDocument.nativeSaveScratchBytes()`, the one source for `CapturedSave` and `EditingStateArchive.capture`)
  answers `W + held + T + BOOKKEEPING_BYTES + SAVE_OVERHEAD_BYTES` (32 MiB; gate S measured 16.5–20.6 MB
  unattributed). W bounds the body in the writer's order — per layer `64 + name + px·5 + 4096` plus mask, meta and
  style slots, per channel `32 + name + px·2 + 4096`, the blobs, guides, the source table with each asset's
  `source_png_encoded_bound`, and every tail table through its module's `header_bytes` / `encoded_len` — and the
  writer reserves it (`buf` at W, each layer/channel/8-bit-asset PNG Vec at its bound; bytes unchanged). `held` is
  the WARP payload, the sampler/comps/refusal/projective tables, `32·layers`, the largest style JSON, 128 KiB and
  the sources' `extra_nonpixel_retained` (descriptor expansion and three profile copies, as the typed plan charges it).
  The per-plane peak model: with `f = c·px + h`, `Cm = ⌈3f/2⌉ + 4096` (fdeflate UltraFast), `Sm = f + 5·⌈f/65535⌉ +
  11` (stored fallback), `K = 4·px` for a four-channel plane, `R = 2·(c·w + 1)`, `X = max(K + R + 2·Cm, K + R + Cm +
  2·Sm, K + Sm + G, 2·G) + 1 MiB` (the compressed Vec mid doubling, the stored Vec mid doubling beside it, the IDAT
  copy, the append into the body) and `T = max_k (X_k − G_k)`; a 16-bit asset uses `native_png_encode_scratch`. No
  margin is added: the first design's ¼ had no measurement behind it and refused the drive's document by 34,472,958
  B, so it was dropped (orchestrator ruling); every gate-M case is covered ≥ 1.25× on the bound alone. The plan reads
  stored dimensions and lengths only, so it is the same with views live or evicted. The archive's untyped
  arm uses `stored = referenced − contiguous_view_bytes` for `decodeEstimateBytes` (`2·stored + 64 MiB + 2·selections`)
  and the reopened retained figure (replaced by the core reopen plan), so neither depends on cached views; `RecoveryCapacity` reads only those captured
  figures and inherits it. Gate M (`ri_interval_max_phys_footprint` rise around `rz_doc_save_native`, one case per
  process, `MallocLargeCache=0`, 3 runs + a baseline twin at 0, views live and evicted, M2 Max): S1 a white
  8256×5504 Background + a placed RGB-noise layer, S2 one noise raster, S2p S1 without the Background, S3 two
  7500×7500 noise layers, S4 S1 + a 64,000,000-byte incompressible RBIN resource on the placed layer, S5 S1 with
  four-channel noise; need = plan + the document's own bytes. The plan column includes `extra_nonpixel_retained`,
  charged after the measurement (+7,704 B for S1, S2p and S5, +67,629 B for S4; the ratios are unchanged to three
  places).

  | case | max E (live / evicted) | file | encoded_bound | plan | ÷ max E | need | HEAD ×4 (live / evicted) |
  |---|---|---|---|---|---|---|---|
  | S1 | 744,620,632 / 744,637,016 | 364,461,702 | 682,675,302 | 1,309,312,114 | 1.758 | 1,672,863,772 | 4,429,660,464 / 2,975,547,696 |
  | S2 | 561,955,272 / 561,971,704 | 181,783,673 | 227,209,622 | 853,838,698 | 1.519 | 1,035,618,445 | 1,521,284,236 / 794,227,852 |
  | S2p | 743,735,896 / 743,752,304 | 363,569,876 | 455,466,004 | 1,082,102,784 | 1.455 | 1,263,883,552 | 2,975,520,520 / 2,248,464,136 |
  | S3 | 920,650,472 / 920,666,856 | 450,049,657 | 562,508,671 | 1,329,662,005 | 1.444 | 1,779,687,713 | 3,667,211,696 / 1,867,211,696 |
  | S4 | 728,597,056 / 728,597,080 | 428,461,979 | 746,675,579 | 1,373,372,316 | 1.885 | 1,800,924,600 | — |
  | S5 | 795,493,000 / 795,443,848 | 364,461,702 | 682,675,302 | 1,309,312,114 | 1.646 | 1,672,863,772 | — |

  `referenced − views` is identical live and evicted in every case. ΔE against batch 151's gate S is within
  +0.12 MB for S1, S2, S3 and S2p evicted. Observation: S2p with views live now peaks one plane higher
  (+181,944,464 B, 743.7 MB, equal to its evicted peak); at a77a225 the live peak sat one plane below the evicted
  one. Not explained; the plan covers it 1.455×. S4 note: the 64 MB resource leaves E below S1's (728.6 vs 744.6 MB)
  while the plan rises by the resource plus its framing and descriptor expansion (64,060,202 B); a placed original's retained file counts at its
  stored size, so a large incompressible original still raises the save's need toward the limit. The binding check in
  the app is the second admission step (plan < working limit − the app's retained bytes after eviction), which also
  counts the host's non-evictable 181.8 MB display projection. MCP drive (port 4983, own background instance):
  D1 `new_document` 8256×5504 8-bit + `place_image` of a 2,774,294-byte JPEG (`sips` quality 90 of an 8256×5504
  plasma PNG): retained 545,326,093 after eviction, so the second check is 1,309,312,114 (the probe's S1 plan for these
  planes; the app exposes no readout) < 2,147,483,648 − 545,326,093 = 1,602,157,555 (292,845,441 to spare; with the
  ¼, before `extra_nonpixel_retained` was charged, it failed by 34,472,958); `save_copy` `.rz` 0.5 s
  (23,970,496 B), `open_document` 1.8 s, two layers at 8256×5504, five `sample_pixel` points match. D2:
  `save_snapshot` before and after `resource_usage(evict_caches)` both 0.5 s, both `state.json` carry
  `decodeEstimateBytes` 1,521,328,426 (live `contiguous_view_bytes` was already 0, since the D1 save's admission had
  evicted; test (c) carries the live/evicted check). D4: predicted 1,521,328,426 + 545,335,597 = 2,066,664,023 <
  limit, and `load_snapshot` passes (0.7 s); `write_recovery` afterwards refuses with the generic "needs more working
  memory than the current budget" sentence (retained 908,886,352 with the loaded snapshot). D3 (16-bit control):
  6000×4000 + an RGB16 PNG, `save_copy` `.rz`, `open_document`, `get_document` all pass (run after D1's document was
  closed; with it open the reopen was refused by the typed plan at 2,477,652,339 B, a no-change path). D5: the same
  document with an 8256×5504 RGBA noise PNG (181.8 MB) placed saves (1.1 s, 364,461,729 B); the PNG's file is not
  retained as an RBIN resource. Tests: `core/tests/native_save_plan8.rs`, `core/tests/native_save_plan8_45mp.rs`.
- `DONE` — The crash-recovery capture of a 45 MP 8-bit
  document. Two checks gate it. (A) The capture's own admission in `RecoveryCoordinator.begin` (scratch = the native save
  plan + 4 MiB + masks, beside the app's retained bytes); D4's refusal above is this one: 908,886,352 retained +
  1,309,312,114 + 4,194,304 = 2,222,392,770 ≥ 2,147,483,648, and the plan stays. (B) `RecoveryCapacity.validate`,
  the reopen check, which at f8ff2fd refused even an ordinary 45 MP document: 727,109,781 stored + 794,218,645
  temporary + 1,090,584,576 projection = 2,611,913,002 ≥ 2 GiB. `recover` decodes, then projects, and each stage
  admits itself (`EditingStateArchive.read`, `PreparedDocumentEdit.init`), so the reopen peak is now Σ reopened +
  max(temporary, projection) — 1,817,693,381 for the document alone. `begin` runs `validate` before `admit`, so a
  reopen refusal neither evicts nor takes the worker slot; a refusal is a `RecoveryCapacity.Refusal` (stage, needed,
  in use, limit), reported as `memory` by `get_recovery_status` beside `state: "failed"`. Before, the user saw the
  Recover Unsaved Work panel open once with "Recovery could not be updated. Any previous completed copy is
  retained." and the generic admission sentence — even when no copy existed — and nothing in the document window.
  Now a refused checkpoint adds "Recovery off" to the editor's status bar (`EditorViewController+Recovery.swift`;
  the tooltip carries the reason and either "Its recovery copy from … is kept." or "It has no recovery copy."), the
  panel lists "These open documents are not protected against a crash:" with each document's reason, and closing
  another document or changing the session snapshots retries the refused capture at the next tick. Gate R (scratch
  Swift probe built from the app sources with `-O -wmo`; `ri_interval_max_phys_footprint` rise, one case per
  process, `MallocLargeCache=0`, 3 runs + a baseline twin at 0, M2 Max; p45 = white 8256×5504 Background + the
  placed plasma JPEG, n45 = the same with the 8256×5504 RGBA noise PNG; write = `RecoveryStore.write` after
  `report(evict: true)`, reopen = `RecoveryStore.read` + `PreparedDocumentEdit`):

  | case | write max E | capture scratch | ÷ | reopen max E | `validate` peak | ÷ |
  |---|---|---|---|---|---|---|
  | p45 | 217,612,504 | 1,313,506,482 | 6.036 | 1,275,528,200 | 1,817,693,381 | 1.425 |
  | n45 | 795,493,000 | 1,313,506,481 | 1.651 | 1,275,511,816 | 1,817,693,380 | 1.425 |

  MCP drive (port 4991, own background instance, private `RZ_*_DIR`s, 0 `[AutomationAlerts]` lines):

  | step | result |
  |---|---|
  | R1 `open_document` p45.rz, rename layer 1, `write_recovery` | `protected` in 0.47 s (retained 545,343,020 after eviction; capture 1,313,506,482 fits 1,602,140,628) |
  | R2 + one session snapshot | refused at reopen: needs 2,544,802,184 ("2.37 GB") |
  | R3 + a second (loaded) snapshot | refused at reopen: needs 3,271,910,988 ("3.05 GB"); its capture would need 1,313,506,482 beside 908,885,487 retained = 2,222,391,969 ≥ 2,147,483,648, refused if reached; `save_copy` `.rz` refused too (information) |
  | snapshots deleted, `write_recovery` | `protected` (0.46 s); `kill -9` |
  | relaunch, `recover_document` | 1.43 s; 8256×5504, 2 layers, "R1 edit", five `sample_pixel` points match; retained 545,324,011 |
  | R4 24 MP 16-bit p24.rz, rename, `write_recovery` | `protected` in 0.74 s |

  R1's write holds the worker slot for about half a second, well under the 5 s `anonymousWaitLimit`. The ceiling
  is a session snapshot: each one is written and reopened as its own full copy. Out of scope and unchanged: the core
  8-bit save plan, snapshot deduplication in the recovery format, and the untyped `stored` estimate (727.1 MB per
  state for a document whose reopen retains 545.3 MB including the 181.8 MB display projection).
- `DONE` — The 8-bit editing-state archive counted a placed layer's shared source twice: `stored`
  (`referenced − contiguous_view_bytes`) charged the layer plane, the source's `rendered` raster and the asset
  each at full size, 727,108,805 B per 45 MP state against 363.5 MB the reopen retains. The archive's untyped arm
  now takes both figures from a core reopen plan, `rzdc_reopen_plan.rs` (`rz_doc_native_reopen_plan`,
  `RasterDocument.nativeReopenPlan()`): `reopenedRetainedEstimate = retained + selections`, `decodeEstimateBytes =
  decode + 2·selections`. N is the document's authoritative referenced bytes with caches excluded
  (`Inventory::retained_referenced_bytes`), so the plan is the same with views live or evicted. From N each retained
  source's `rendered` raster (never written; the reader rebuilds it as `layer.pixels.clone()`) and its asset are
  removed, then each effective asset of `byte_sources` (the writer's pointer-deduplicated table) is added once:
  `serialized`. `retained = serialized −` the layer pixels of every binding whose plane shares all of its 8-bit
  asset's tiles (`LayerRaster::shares_samples`), because `materialize_byte` adopts the asset's tiles for it. `decode
  = encoded_bound + serialized + T + 64 MiB`, T the largest reader stage beside the end state: a layer plane's
  `4·w·h` (the decoded buffer while `unseeded` tiles it), the channels' `2·w·h` of the canvas (the decoded image and
  its `to_luma8` copy), an 8-bit asset's `4·w·h` (`decode_assets`' `into_rgba8` buffer), a 16-bit asset's
  `native_png_band_scratch`. Shares the reader does not re-create (a duplicated layer, a mask, a channel, a WARP
  original) still count per reference. The 8-bit reader's layer decode moved from `to_rgba8()` to `into_rgba8()`,
  which hands back the decoder's RGBA8 buffer instead of holding it and a copy at once; image 0.25.10 copies every
  other colour type exactly as before. Its output was pinned before the change and is unchanged after it:
  `core/tests/native_reopen_plan8.rs` (h) hashes six fixtures plus a layer PNG spliced in as Rgb8 and as Rgba16
  (the reader accepts both), and the gate probe hashed each reopened layer's pixels and mask for p45, n45, s45 and
  a45 — every digest matched. The 8-bit projection is now charged at P + ⌈P/4⌉ in the archive (`validate` is
  unchanged): with retained exact, gate R's 8-bit projection stage (912 MB against P = 1,090.6 MB, 1.196×) would
  otherwise sit under 1.25×. Gate R′ (gate R's method: the scratch Swift probe, `ri_interval_max_phys_footprint` rise,
  one case per process, `MallocLargeCache=0`, 3 runs + a baseline twin, every twin 0, M2 Max; decode =
  `RecoveryStore.read` alone, reopen = it plus `PreparedDocumentEdit`; s45 = the 8256×5504 noise PNG as one raster,
  a45 = it placed on a 3000×2000 Background and scaled to 2999×2000, so its 8256×5504 asset outsizes every layer
  plane). s45 and a45 are structural bounds whose only slack is `encoded_bound` − file plus 64 MiB, gated at ≥ 1.15×
  (orchestrator ruling); every other case at ≥ 1.25×:

  | case | plan | max E | ÷ | gate |
  |---|---|---|---|---|
  | a45 decode | 766,920,677 | 618,201,520 | 1.2406 | 1.15 |
  | s45 decode | 657,862,329 | 546,406,808 | 1.2040 | 1.15 |
  | n45 decode | 1,476,887,189 | 1,093,878,528 | 1.3501 | 1.25 |
  | p45 decode | 1,476,887,191 | 753,205,776 | 1.9608 | 1.25 |
  | p45 reopen, 0 snapshots (gate R: 1,817,693,381 ÷ 1,275,528,200 = 1.425) | 1,726,799,717 | 1,275,413,512 | 1.3539 | 1.25 |
  | p45 reopen, 1 snapshot | 2,090,368,714 | 1,458,767,144 | 1.4330 | 1.25 |
  | n45 reopen, 0 snapshots (gate R: 1,817,693,380 ÷ 1,275,511,816 = 1.425) | 1,726,799,716 | 1,275,397,128 | 1.3539 | 1.25 |
  | n45 reopen, 1 snapshot | 2,090,368,712 | 1,458,799,888 | 1.4329 | 1.25 |

  The plan's p45 terms: file 23,970,496, `encoded_bound` 682,675,330, serialized 545,338,901, retained 363,568,997
  (the R1–R4 MCP drive's relaunch retained 545,324,011 less the 181,764,096 display, plus 9,082 B), T 181,764,096; the
  reopen peaks are `validate`'s retained + max(decode − retained, P + ⌈P/4⌉ = 1,363,230,720). MCP drive (port 4991,
  own background instance, private `RZ_*_DIR`s, 0 `[AutomationAlerts]` lines):

  | step | result |
  |---|---|
  | D1 `open_document` p45.rz, rename layer 1, `write_recovery` | `protected` in 0.49 s (retained 545,334,733 after eviction) |
  | D2 `create_snapshot`, `write_recovery` | `protected` in 0.93 s (R2 was refused at 2,544,802,184); `kill -9` |
  | D3 relaunch, `recover_document` | 2.08 s; 8256×5504, 2 layers, "D1 edit", five `sample_pixel` points match, one session snapshot; retained 908,874,765 live (the snapshot decodes to its own copy) |
  | D4 `delete_snapshot`, `resource_usage(evict_caches)`, `load_snapshot` of D2's `.rzsnapshot` | loads in 0.72 s: 545,324,011 in use + its `decodeEstimateBytes` 1,476,887,189 = 2,022,211,200, 125,272,448 B under the limit; restored, the five points match |
  | D5 p45.rz with two session snapshots, `write_recovery` | refused at reopen: needs 2,453,937,711 ("2.29 GB") |
  | D6 24 MP 16-bit p24.rz, rename, `write_recovery` | `protected` in 1.07 s |

  D4's package was saved in launch A: in D3's document `save_snapshot` of the recovered snapshot is refused by the
  8-bit save admission (the core save plan beside 908.9 MB retained, because a recovered snapshot no longer shares the
  current state's allocations). Re-measured at bff4261 (batch 163, `p45.jpg` a 41,768,492 B `sips` q90 JPEG of a
  photo PNG; recovered retained 908,884,269): at the 2 GiB reference limit `save_snapshot` is refused with the
  generic sentence and, after a rename, `write_recovery` with "needs 1.22 GB of working memory and 866.8 MB is
  already in use" (needed 1,313,506,481). At the 8 GiB limit a 16 GB Mac gets both are admitted (3 runs):
  `save_snapshot` max E 676,053,496 (its plan has no readout), `write_recovery` max E 270,090,432 against
  1,313,506,481 (4.863×); 908,885,486 + 1,313,506,481 = 2,222,391,967 fits an 8 GB Mac's 2,862,612,480.
  Tests: `core/tests/native_reopen_plan8.rs`.
- `DONE` — The 16/32-bit display preparation is charged as
  the max of its two sequential stages: `projectionScratchEstimate()` returns
  max(`render_scratch_bytes`, `render_output_bytes` + `display_scratch_bytes`) + `display_overhead_bytes`, three new
  additive descriptor keys (`display_scratch_bytes` is `rgba8_preview_scratch` = 4·n + 1 MiB at u16,
  `float_document::display_scratch` = 12·n + 1 MiB at float; `display_overhead_bytes` is
  `PrecisionDocument::display_overhead_bytes` = ⌈max / 2048⌉ for the page tables of the freshly mapped buffers, one 16
  KiB table per 32 MiB, plus (⌈w/256⌉ + ⌈h/256⌉ − 1) · 32 KiB for the output's clipped edge tiles rounding up to the
  malloc quantum, about 2.39 MB at 7100²). It used to add 8·n to the render's peak, which already includes the u16
  output. The suspected cause was refuted: MCP `canvas_size` at 16 bits runs `canvasResized` →
  `PrecisionPixelEdit.run()` (whose `take()` zeroes `BackgroundWork.scratchEstimate`), then `preflightPublication` →
  `PreparedDocumentEdit`; `CanvasGeometryJob` is not on the path and `admit` never reads `BackgroundWork.scratch`.
  Measured on a 7000×7000 white 16-bit `new_document` → `canvas_size` 7100×7100 (n = 50,410,000; working limit
  2,147,483,648; one run per fresh instance): before, render 1,210,888,576, estimate 1,614,168,576, transient
  1,614,168,576 (= estimate; `SelectionWorkBudget` 0, `BackgroundWork.scratchEstimate` 0), own 2,025,179, retained
  591,103,859 (the previous revision's 392,946,688-byte projection and 196,009,248-byte display), headroom −57,788,787
  → refused. After, without the overhead term, estimate 1,210,888,576, transient 1,210,888,576, own 2,025,179,
  retained 591,103,859, headroom +345,491,213 → admitted, `projection_error` null, also after `undo`; `phys_footprint`
  sampled every 5 ms across the high-depth branch of `PreparedDocumentEdit.init` rose by 1,211,106,144 / 1,211,237,192
  / 1,212,040,032 B (3 runs), 0.2–1.2 MB above that estimate. No pixel buffer was under-charged; the excess is the
  page tables and edge-tile rounding above, so they became `display_overhead_bytes`. With it the estimate is
  1,213,282,071 (headroom +343,097,718) and the footprint rose by 1,211,220,832 / 1,210,827,592 / 1,211,302,752 B (3
  runs), 2.0–2.5 MB below the estimate. The same sampler over `new_document`'s own 7000×7000 preparation rose by
  1,762,297,056–1,766,982,904 B (36·n: render 24·n, then projection 8·n and display 4·n; exit == peak), above both its
  new estimate (1,179,425,547) and the old one (1,569,048,576): in a fresh process the freed accumulator pages are not
  reused, while the warm canvas_size process reused them and matched 24·n. That challenges the max-of-stages rule
  against `phys_footprint`. Float is charged 4·n more than before (output 16·n + display 12·n + 1 MiB over the 24·n + 1 MiB render), plus
  the overhead: a 4000×4000 float document's estimate is 450,283,646 (was 385,048,576). When the all-documents check
  refuses and at most one document is open, `admit` says "This job needs more working memory than the current budget,
  and no other document is open to close." plus its remedy; with more open it keeps the "Close other documents"
  sentence. The count is kept under `RasterResources`' lock: `ImageDocument.makeWindowControllers()` adds a document
  once, `close()` removes it if it was counted, so a recovered document that is never shown is not counted; it only
  picks the sentence. Before the fix the D0 drive (one document) got the new sentence and D0b (a 100×100 8-bit
  document open too) the old one. Every preview reservation built on `projectionScratchEstimate()` now charges 8·n
  less at 16 bits, offset by the small overhead term: `previewReservationBytes`, `PrecisionOpacityDrag`,
  `ProxyPreview`, `PrecisionProfileEditing`, `PrecisionGeometryGesture`, the live-filter, mask and refinement
  previews. MCP drives: a float 4000×4000 → 4100×4100 and a 16-bit 5000×5000 `crop` 4500×4500 at 7° (the
  `CanvasGeometryJob` path) commit with `projection_error` null. `core/tests/precision_inspection.rs`.
- `DONE` — A re-rendered placed layer's never-run raster
  plane adds no work units to the 16-bit geometry plan (its progress range is the source render's units, so the
  render now reports to the end), and its scratch is skipped for previews as for commits (Resize still charges
  it); a 5700×3800 S17 now counts the direct `transform`'s 2,591,644,128 units (was 5,096,648,256). An 8-bit
  placed transform commit takes the one-shot budget: the byte geometry plan bounds each placed layer's source
  render by `one_shot_work_limit(one_shot(edit))`, and a byte source `transform` without `interactive` takes
  `MAX_DESTRUCTIVE_WORK` with the 5/4 headroom, which its measurement required (5700×3800, 3 runs, one case per
  process; `precision_geometry_budget`'s record: max E 366,477,600 against an unscaled plan of 369,169,568,
  1.0073×, now 461,461,960, 1.2592×; S17 / R17 / PS max E 366,493,984 / 366,477,600 / 346,882,336 against
  1,901,483,776 / 1,242,478,352 / 1,824,363,008). The 8-bit masked ⌘T preview's `TransformWork` sends
  `interactive` and keeps `MAX_WORK_UNITS`. MCP drive: an 8-bit 4000×3000 placed photo's `transform_layers` 17°
  lanczos (≈2.9e9 source units) commits. At 6000×4000 the 8-bit placed ⌘T is refused by memory (S17 plan
  2,105,881,088 + retained 96,013,913), the plan's ×6 unexecuted raster terms.
  `core/tests/canvas_geometry_admission.rs`, `core/tests/retained_source_work.rs`.
- `DONE` — The jobs-probe "transform admission" now
  discriminates the byte selected-pixels admission: a 48 MP 4:3 (8064×6048) select-all `move_selected_pixels` whose
  core plan (980 MB) fits the 1,562 MB free by 583 MB while core + W·H·24 (2,150 MB) would have refused by 588 MB,
  and a 28 MP 4:3 (6208×4656) select-all `transform_selected_pixels` 15° bilinear whose `max(core, σ)` (core
  1,045 MB, selection 814 MB) fits the 1,801 MB free by 756 MB while core + W·H·24 + σ (2,552 MB) would have refused
  by 752 MB. Sizes follow the probe header's balance rule (Move c = 20.1 B/px; transform c = 36.1, σ/P = 28.1).
- `DONE` — Measured, no app change. The jobs-probe case
  "copy preview tick cost" printed: `24 MP 16-bit text ⌥⌘T preview tick on main (Swift -Onone, release core):
  duplicate median 3.88 ms, max 5.05 ms; ⌥⌘T work median 26.62 ms, max 28.31 ms; ⌘T work median 22.02 ms, max
  23.84 ms; ⌥⌘T tick median 30.90 ms, max 32.96 ms; ⌘T tick median 27.15 ms, max 32.41 ms; frame budget 16.7 ms;
  verdict ok`. The per-tick duplicate is well inside a frame and ⌥⌘T's tick is over budget only where ⌘T's is too, so
  the gesture's duplicate cache was not built. The 8-bit ⌥⌘T parallelogram copy of a text layer replays to the UI's
  copy (pixels, offset, description) through its recorded `transform_layers` step and a direct `transform_layers`
  with the recorded `source_rect`/`corners` (`rasterized_layers: []`): case "parallelogram copy replay".
- `DONE` — Part A (byte UI commits): the 8-bit
  Healing Brush and Patch commit as nonmodal `DocumentJobs` jobs over a cancellable core heal
  (`rz_doc_heal_layer` / `rz_doc_heal_to_new_layer` take a nullable `RzJobControl`; `core/tests/heal_job.rs`),
  with a display pin of the drag's picture while the job runs, a progress panel only after 0.25 s, a busy-worker
  gesture guard, and the rasterize prompt moved before the solve. Part B (MCP): the 8-bit `heal_stroke` and
  `patch_region` at both depths commit through the same job (`healJob` / `healToNewLayerJob`,
  `PatchEditCapture.startByte` / `startNative`) and reply through `finishJobCall`, so `background: true` returns
  a `job_id` for `job_status` / `cancel_job`, the default waits, and a no-op still answers `changed: false`;
  the sync `healLayer` / `healToNewLayer` wrappers are gone, and a result `applyEdit` refuses now ends the job
  `refused` (an in-band error) rather than `completed` (`scripts/jobs-probe/heal_jobs.swift`
  `verifyAgentHealJobs`). Done since: Esc on the canvas and ⌘Z/⇧⌘Z cancel the document's running UI job
  instead of deselecting or popping a step, and MCP `undo`/`redo` refuse by name (the `job_id`) while any job
  runs on the document; `finish` answers `applyEdit`'s depth beep guard
  before its undo group, so a refused result leaves no empty undo step.
  Done since: every other `withUndoGroup`
  publication site asks `applyEdit`'s shared preflight (`ImageDocument+EditPreflight.swift`) before its group
  opens; choosing a History state while a UI job
  runs cancels the job, and MCP `restore_history` refuses by name like `undo`;
  Esc that nothing else in the editor window
  consumed cancels the UI job (`EditorWindow`), and MCP `undo`/`redo` answer an error for a step that changed
  nothing. DONE: the same-class leftovers — a refused or no-op agent session commit
  leaves no undo step (`withUndoGroupDiscardingEmpty`), keys reach the canvas when the editor window itself
  holds focus, Esc from History, Snapshots, Layer Comps and Actions cancels the UI job (`JobEscape.swift`),
  and a presented `SilentEditRefusal` beeps — and MCP `set_layer_properties open` adds no undo step, like
  the panel triangle. Done since: a new-layer heal copies the plate
  once and charges it (`w·h·8` while copying, `w·h·4` held under every heal reservation); the heal's window
  planes poll every 4096 pixels, its final rect write and the byte inpaint's poll per tile,
  `rz_doc_spot_heal_to_new_layer` closes its control, and `make cancel-bench` times both heal gaps (`heal_job`).
  The 8-bit Content-Aware Fill admission is stated in the byte kernel's own terms (136 B per window pixel, not the
  typed 320), so a 1 MP fill on a 12 MP photo is admitted (746 MB where it asked 2.08 GB; measured peak ≈ 400 MB
  with fixtures) and the documented limits stand (`doc_content_aware`'s `# Admission`).
  The batch-44 follow-ups followed: the heal's and byte inpaint's union reads and the
  new-layer deposit poll per tile, the retouch layer is one filled tile, component records are charged
  before the walk, and the typed window figure is measured at 72 B, not 320, so a 1 MP 16-bit fill on a
  12 MP photo is admitted too (`precision_inpaint`'s
  `# Admission`). Then the 8-bit Content-Aware Fill's new-layer arm lifts through
  the same polled rect deposit, `make
  cancel-bench` times the union reads and the new-layer deposit,
  and the Healing Brush charges one component record per component, bounded before the walk, so a dense
  16 MP Patch no longer reserves over 2 GiB before it starts (`doc_heal`'s
  `# Admission`). The byte inpaint reserves by the same bound, the new-layer
  deposit reads only the healed rect, and the 8-bit fill's new-layer canvas term is measured at 9n, not 16n
  (`doc_content_aware`'s `# Admission`). The
  in-place byte inpaint charges its union-rect clone instead of the whole layer, the 8-bit fill re-admits by
  its coverage's component bound instead of the box's area, and `make cancel-bench` times the fill's
  new-layer deposit. The 16-bit `heal_stroke` gap is closed: typed strokes now run as jobs
  and `heal_stroke` takes `background` at 16 bits.
- `TRACKED` — Noncancellable save/export encode and RAW/decode paths → P7-32 (plan §13C).
- `TRACKED` — Broader cancellable/regional processing → P7-31 / plan §13A (closes with 13A-latency).
- `TRACKED` — Tiled document storage → plan §13B.
- `DONE` — Higher precision storage — Phase 7 (typed 16-bit) and 11F (float).
- `DONE` — Budgeted history and recovery — Phase 5 ([phase-5-recovery-history-status.md](phase-5-recovery-history-status.md)).
- `DONE` — "Comprehensive legacy-operation admission": `RasterResources.shared.admit` now guards 113 call
  sites across 68 Swift files (not audited for completeness).
- `DONE` — Optimized Swift unqualified (Release `make app`, qualified from Phase 7E on).
