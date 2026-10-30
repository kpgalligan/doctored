# Phase 4 — Jobs and resource foundations

Implementation record for the [professional photo editor plan](professional-photo-editor-implementation-plan.md), September 9, 2026. **Phase 4 (4A–4D) is implemented and passes its completion gate within the scope below.** Phase 3 is committed in `f34201b`.

| Slice | Delivery | Verification |
| --- | --- | --- |
| 4A | Bounded final worker, immutable revision/selection/target capture, cooperative Rust cancellation/progress, stale refusal and one-edit completion; UI/MCP/Actions | Core cancellation and independent pixel/source tests; actual AppKit sheet/Cancel; built-app cancellation, stale mutation/reorder and undo/redo |
| 4B | Weak host ownership registry across undo/jobs/projections, unique core allocation accounting, synchronized cache eviction, scratch estimates/reservations and admission | Independent allocation counts, concurrent cache access/eviction, fixed 24/48 MP core and built-app corpora |
| 4C | Explicit RGBA8 region/preview kernel contract and shared bounded Copy/Invert adapter | Five regional FFI tests: independent bytes/coordinates, stitching, alpha, budget/overflow, cancellation and NULL handling |
| 4D | Spot-healing jobs; captured-revision save/export; final raster transforms and RAW development on the worker | Actual save during a newer edit, window dirty indication, failed writes, UI/MCP transforms/Actions, RAW import/export regressions and measured 24/48 MP costs |

## Execution and ownership

One final operation runs app-wide on a serial worker. Its closure owns immutable values; all AppKit state, document mutation and undo remain on the main thread. Shared Rust caches and Swift ICC memoization use locks. The main thread polls an owned result mailbox, avoiding a dependency on reentrant main-queue dispatch when MCP or an Action waits. Interactive fill/heal jobs present nonmodal progress. Waiting calls use an AppKit modal session with Cancel.

MCP `content_aware_fill` and `spot_heal_stroke` accept `background: true` and return a `job_id`; `job_status` and `cancel_job` report/control it. Default calls and Actions wait for completion. Existing operation parameters and successful rasterization reporting remain in the response. Recording Start/Stop drains a pending command before changing or saving its recording session, so a late result cannot omit a step or enter a newer recording.

Completion checks a monotonic revision, the exact retained document handle, the captured layer position/fingerprint and selection state. Any intervening document mutation, even Undo back to the same handle, refuses the result. Target identity is scoped to the immutable captured stack, so identical-looking reordered layers cannot fool the check. Rasterization confirmations revalidate their capture after the modal prompt. Cancelled, stale and unchanged results create no undo entry. Closing a document requests cancellation. The last 16 terminal reports retain no image input, selection or result buffers.

Rust checks cancellation within component walks, planning, Euclidean distance transforms, PatchMatch search/voting and Poisson classification/sweeps/cycles. Plain raster Sample All preparation uses bounded tiles through the existing compositor; groups/styles use an explicitly admitted full-frame fallback. A cancellation error releases the operation's scratch reservation and partial outputs. Preview cancellation also signals its token; other immutable preview closures can finish after their completion is discarded.

## Memory and rendering

The weak registry follows every live Rust handle wrapper, including handles held only by NSUndoManager closures, live gestures, previews and jobs. It adds no snapshot ownership. Core diagnostics deduplicate backing allocations across one complete live set, including shared pixels/masks/channels/profiles/metadata and synchronized caches. They distinguish measured retained payload capacities from scratch estimates, allocator overhead and unaccounted Swift/CoreGraphics/AppKit/ImageIO/Core Image/GPU allocations. Process RSS is not claimed to equal the report.

The initial working limit is the smaller of one third of physical RAM and 2 GiB. Job admission evicts reusable caches before measuring; projection refresh evicts core caches above 128 MiB. Fill planning reserves its measured window estimates before expensive work. Shared projection estimates include nested groups, clipping, adjustments and padded styles; they intentionally overestimate some cached/shared work. Native save estimates include referenced serialized planes, channels and descriptions/capture packets, even when editing shares their allocations. These are admission estimates, not an OS-enforced memory ceiling. They apply to migrated work; budgeted history and comprehensive legacy-operation admission remain later work. A job, render or save may refuse when retained memory plus its estimate exceeds the limit.

Help > Memory Usage and MCP `resource_usage` expose the same diagnostics and cache eviction. The rendering contract and measured migration decisions are in [phase-4-rendering-contract.md](phase-4-rendering-contract.md). Current storage remains straight RGBA8 in document-encoded RGB. Copy/Invert accept explicit source regions, preview divisors and output budgets; authoritative divisor 1 is distinct from nearest-sampled previews. This phase does not introduce tiled document storage, higher precision or a new native format version.

## Save and migration behavior

Save captures source, projection and export settings together, encodes and atomically replaces the destination on the worker, and marks that source handle saved. Newer edits and their window dirty indication remain dirty; Undo to the saved handle becomes clean. Export copies leave the saved handle unchanged. Failed writes retain the previous file and dirty state. Final projection work can report failure while retaining the previous display; native save remains independently subject to native capacity and memory admission.

Measured 24/48 MP medians justify moving final affine transforms (556/1073 ms), native save (193/469 ms), PNG projection/export (565/1115 ms) and actual CFA RAW development (1144/2282 ms). The architecture note records fixtures, ranges and hardware. Save/export, RAW and raster-transform kernels currently have responsive, **noncancellable** progress; they offer no Cancel button. Platform rendering of described layers stays on the main thread. The existing Healing Brush/Patch commits also remain synchronous. Recovery remains Phase 5, and broader cancellable/regional processing remains future work.

## Verification record

All **813 standard Rust tests passed**, with 0 failures and 2 default-ignored extended checks, across 61 Cargo-built test executables. Both extended checks also passed separately: the fixed 24/48 MP resource corpus and the required million-unknown Poisson reference. The latter measured maximum errors of 0.0006 at the seam, 0.0025 at 3–10 pixels and 0.0066 in the interior. Clippy, formatting and diff checks passed.

The first parallel Cargo run passed every correctness test but hit one existing timing ratio while builds competed for CPU. It passed unchanged in isolation (556 ms preview versus 1300 ms fill) and in the complete serial run (551 versus 1267 ms). The definitive serial runner executed the exact Cargo-built binaries and recorded their SHA-256 hashes in `/tmp/rz-phase4-serial-test-manifest.json`; results are in `/tmp/rz-phase4-tests.log`. This avoided repeated native-dependency rebuilds caused by switching between the Xcode and shell build environments. No timing threshold was weakened. An earlier disk-space failure was resolved by clearing owned, reproducible build caches.

Core cancellation-to-return measured 0.032/0.048 ms for 24/48 MP planning, 0.058/0.040 ms for active fill/spot PatchMatch, 0.039 ms for Poisson, 0.291 ms for 48 MP tiled Sample All, and 455 ms for the qualified 24 MP grouped-source fallback. Built-app cancellation completed in 2.9 ms; document reads during computation took 21.6–22.0 ms. The final actual AppKit Cancel probe measured 28.8 ms with its main-run-loop heartbeat continuing. All are within the **two-second qualification bound for these fixed cases**; this is not a universal scheduling or advanced-style latency guarantee. Cancellation returned scratch reservations to zero and released input/result ownership.

| Live app corpus | Initial unique retained bytes | Four renames plus shared duplicate | One pixel-edit allocation |
| --- | ---: | ---: | ---: |
| 24 MP | 192,015,811 | 192,020,778 | 288,021,233 |
| 48 MP | 384,016,627 | 384,022,002 | 576,022,457 |

Undo retains the edited allocation through redo ownership. The independent core corpus with 20 shared snapshots, one edited snapshot and a projection measured 288,015,306 / 576,015,306 unique bytes at 24/48 MP, despite referenced layer payloads of 2,016,000,000 / 4,032,000,000 bytes. The distinction between snapshot sharing and new pixel buffers is explained by exact dimension-derived counts.

All four Phase 4 built-app MCP suites and all ten Phase 1/2/3 regression suites passed. The four Phase 4 suites also passed again against the final Xcode-built app after the reporting/recording fixes; cancellation completed in about 2 ms. All ten final AppKit source-probe groups passed, including actual fill sheet/Cancel, spot gesture callbacks, saved revision/window indication, failed saves, UI/MCP transforms and Actions, rasterization reporting, and stopping/saving a recording with a pending job. Swift typecheck passed with 0 errors and six existing warnings.

- `scripts/verify_phase4.py`: reproducible built-app cancellation/stale/undo/native/Actions and 24/48 MP resource gates.
- `scripts/jobs-probe/verify.py --keep`: actual AppKit and MCP-handler workflow probes; macOS file-coordination services require an unsandboxed probe launch.
- `core/examples/phase4_latency.rs` and `scripts/phase4_raw_latency.py`: bounded core and actual CFA RAW measurements; RAW verification uses macOS GPU services.
- Final source-probe artifacts: `$TMPDIR/rasterize-jobs-probe-pz3_f_70`.
- Final Xcode-app MCP artifacts: `$TMPDIR/rasterize-phase4-53gm9v6k`, log `/tmp/rz-phase4-mcp-final.log`.
- Initial full Phase 4 MCP artifacts: `$TMPDIR/rasterize-phase4-h9k_p97y`; logs `/tmp/rz-phase4-mcp.log` and `/tmp/rz-phase4-phase{1,2,3}-regression.log`.

Qualification uses the release Rust core and Swift `-Onone`, whole-module compilation. Optimized Swift output remains unqualified. An isolated app linked from the current sources at `/tmp/rz-phase4-standalone` allowed MCP checks while Xcode waited for Cargo's build lock; the final Xcode Debug build at `/tmp/rz-phase4-build` also passed (`/tmp/rz-phase4-app-final.log`). Test instances use private ports/settings/Actions, dummy credentials, temporary files and only processes the runners launch. Generated binaries, fixtures and caches remain outside the committed source tree.
