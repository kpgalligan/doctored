# Phase 5 — native recovery and history

Phase 5 (5A–5D) is implemented and qualified after Phase 4 commit `968e0ee`.

| Slice | Progress |
| --- | --- |
| 5A — Recovery policy | Native checkpoints for dirty native, imported and untitled documents; independent of source-file saves. |
| 5B — Recovery lifecycle | Restart discovery, recover/discard, two atomic generations, source-move handling, confirmed-save/close cleanup and visible failures. |
| 5C — History | Enumerable full states, selection history, shared-allocation accounting, budget eviction and an explicit nearest-step exception. |
| 5D — Named snapshots | Session captures, full-state restoration, portable `.rzsnapshot` files and recovery of the session list. |

Recovery and snapshots share a versioned host archive containing a native `.rz` image and selection, layer targeting and export settings. Recovery restores a dirty untitled native document; the recorded source path is informational, including when the source moved. Normal `.rz` files retain their current version and pixel/document semantics.

## Recovery policy and lifecycle

The installed SDK headers establish that `NSDocumentController.autosavingDelay` defaults to zero, `autosavesDrafts` defaults to true and `autosavingFileType` defaults to the source type. The prior `autosavesInPlace = false` did not establish verified native recovery. Enabling inherited autosaving without adaptation could write an imported type and pass through saved-marker logic. The app now disables inherited draft/in-place behavior and routes explicit AppKit autosave requests and scheduling through its native coordinator.

- Recovery lives in `~/Library/Application Support/Rasterize/Recovery`. Tests use `RZ_RECOVERY_DIR` and a private preference domain.
- A checkpoint becomes due **30 seconds** after an uncheckpointed committed change, with a **0.5-second polling tick**. Active gestures, Action replay and the app-wide worker defer capture; encoding and retries add time. This is a scheduling interval, not an unconditional durability deadline. Tests may override it with `RZ_RECOVERY_INTERVAL_SECONDS`.
- The worker receives captured values only. It writes a complete generation before atomically publishing a small manifest. Current and previous generations are retained; failed writes preserve the previous manifest. A damaged current image, selection or snapshot list can fall back to the previous generation with an explicit warning.
- File leases prevent another live instance from discovering, claiming or discarding an open document's copy. Forced termination releases the operating-system lease.
- Inaccessible or incomplete entries are reported individually, so they do not hide readable checkpoints beside them. Root enumeration errors appear in the nonmodal recovery window.
- Recovery opens a dirty native untitled copy. The original source path is informational, including when it moved. Imported JPEG/RAW files are never overwritten by recovery.
- A confirmed preserving save removes obsolete recovery only when the current image is clean and the session snapshot list is empty. Saving an older captured revision preserves newer work. Actual normal close discards the session's recovery and prevents a late worker from publishing it again.
- Session snapshots require recovery even when the image itself is clean. They survive ordinary Save through recovery, and expire on normal document close. Independently saved snapshot files remain on disk.

## History and snapshots

`EditingState` contains the immutable native document, current and previous selection, Quick Mask mode, layer selection, paint target and export settings. Saved-file identity remains separate. The native document carries pixels, masks, channels, guides, styles, descriptions, profile and capture metadata.

`DocumentHistory` owns the enumerable timeline; `NSUndoManager` retains transition IDs and supplies normal grouping/Undo/Redo behavior. Actions and live gestures produce one transition at their existing central boundaries. A new edit after navigation drops the redo branch. Window > History and MCP `list_history` / `restore_history` expose the same sequence.

The default incremental history budget is the smaller of 512 MiB and half the working limit. Unique core allocations are counted once across states, with deduplicated Swift selection arrays and estimates for display images and contours. Active and saved-file state form the baseline. Old transitions are evicted when enforcing the budget, but the nearest reversible transition survives even if it alone exceeds the budget; the panel and report disclose the exception. A separate **4096-transition cap** bounds inexpensive bookkeeping. Budgets can be changed in the panel or through `set_history_budget`.

Window > Snapshots and the matching MCP tools create, rename, remove, restore, save and load named full states. Restoring a different state is one undoable action; restoring the current state is a no-op. The session list holds at most **32 snapshots**, with an incremental budget of the smaller of **512 MiB** and one quarter of the working limit. New captures are refused when they exceed it. Existing named captures stay pinned if later image changes alter the baseline and increase reported usage.

Explicit Save Snapshot / Load Snapshot writes or reads a portable `.rzsnapshot` package. Ordinary `.rz` saves do not embed the session list, transient selection or undo history. Snapshot packages and recovery use host schema version 1 around unchanged native **RZDC version 9** images. Selection files are dimension-checked; names, geometry, indices, targets, package versions and metadata sizes are validated before applying state. Worker decoding validates selection materializability so corrupt coverage cannot bypass recovery fallback.

## Qualification

- `make typecheck`: passed with the six existing warnings. Xcode Debug build: passed with release Rust and Swift `-Onone` / whole-module compilation.
- Final Phase 5 MCP suite: **13 groups passed**, covering 36 undoable edits, saved/dirty navigation, selection budgets, grouped Actions, snapshot restoration and portable files, malformed archives, forced termination of native/JPEG/RAW and untitled sessions, original-file hashes, moved sources, concurrent leases, failed writes and previous-generation fallback. `/tmp/rz-phase5-mcp-final.log`.
- AppKit source probe: **16 groups passed**, including 40 edits within a 512 KiB test budget, actual History/Snapshots panels, weak-owner release after eviction, saved identity and export/target restoration, bookkeeping and navigation-baseline policies, aggregate recovery admission boundaries, plus real `NSDocument` save races, cleanup, late-publication prevention, recovery controls and discovery beside inaccessible entries. `/tmp/rz-phase5-history-probe-discovery.log`.
- All **14 Phase 1–4 MCP regression groups passed**, including the 24/48 MP resource corpus. `/tmp/rz-phase5-regressions.log`.
- On the small 128 × 96 history corpus, 36 metadata edits retained 10,141 additional measured bytes, within the 512 MiB default budget. A one-byte test budget retained one reversible transition and explicitly reported its 49,482 additional bytes.
- With the interval override removed, a dirty native document received its checkpoint after **30.265 seconds**. The durable manifest's revision/serial matched status, and the document remained dirty. Final corruption checks also passed for a missing snapshot subtree, a valid-length all-zero mask and an off-canvas selection. Capacity checks independently exercised duplicate serialized sources, the strict admission boundary and unchanged prior publication after refusal.

The final test app is `/tmp/rz-phase5-private/Rasterize.app`, copied from the successful Xcode build with only its bundle identifier changed to `com.kgalligan.rasterize.phase5-verification` and its signature renewed. Build and typecheck logs are `/tmp/rz-phase5-app-final.log` and `/tmp/rz-phase5-typecheck-final.log`. Qualification used macOS 26.5.1. MCP artifacts are at `$TMPDIR/rasterize-phase5-dmo6z0qe`; AppKit probe artifacts are at `$TMPDIR/rasterize-history-probe-5cj7smr2`.

Reproducible runners: `scripts/verify_phase5.py` (MCP and forced restart), `scripts/history-probe/verify.py` (AppKit lifecycle and controls), and the existing Phase 1–4 scripts. All launches use dummy credentials, isolated Actions/recovery files and owned processes. Rust pixel/FFI/native-format code is unchanged in this phase; no new Rust test run is claimed.

## Limits

- Recovery protects against process termination. Atomic publication does not promise power-loss durability: explicit file/device synchronization is not implemented.
- Working-memory admission can refuse recovery, saves and snapshot loading. Recovery's aggregate preflight includes independently decoded snapshot storage, temporary decoding/selection estimates and the current projection. Native files do not preserve in-memory sharing between snapshots; a large snapshot list can therefore be refused even when it is cheap to retain in the current session. The previous completed recovery remains available, and separate snapshot files provide a way to reduce the session list.
- Byte accounting is a working estimate, not an OS memory ceiling. Allocator/object overhead and some platform or decoder buffers remain unaccounted; selection image/contour costs and projection fallbacks are conservative estimates.
- Undo/redo navigation preserves the retained state union; changes in active-state baseline credit can increase reported incremental usage until the next edit or budget enforcement. Navigation does not discard the redo states merely to change that accounting classification.
- History is session-only. Persistent branches, tiled backing and unlimited revisions remain out of scope. Snapshot files contain full states, not selective layer comps. **Phase 12D-1 shipped the other half of that sentence**: a LAYER COMP (`core/src/doc_comps.rs`, Window ▸ Layer Comps…) records a chosen subset of per-layer visibility, position and appearance by stable layer identity and carries no pixels, no selection and no history, so the two features stay distinct — a snapshot restores a moment, a comp restores an arrangement. Native descriptions retain existing rendered fallbacks and external source references; embedded asset recovery belongs to the retained-source phase.
- A transient selection alone on an otherwise clean image does not trigger recovery. Dirty images and session snapshots carry their current and previous selections in each checkpoint. History/snapshot management commands are not recorded as portable Action steps.
- Optimized Swift and large high-depth workflows remain unqualified. Current image storage is RGBA8.
