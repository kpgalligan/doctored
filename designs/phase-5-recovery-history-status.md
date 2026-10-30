# Phase 5 — native recovery and history (ADR)

- **Status:** Accepted — open items remain
- **Phase / dates:** Phase 5 (5A–5D) of the [implementation plan](professional-photo-editor-implementation-plan.md), 2026-09-09 (layer-comp note added 2026-09-19)
- **Original:** [archive/phase-5-recovery-history-status.md](../archive/phase-5-recovery-history-status.md) — full history, measurements, receipts
- **Key commits:** `83de68c` (Phase 5 with Phase 6 selection work), `5adcb8d` (12D-1 layer comps note); Phase 4 was `968e0ee`

## Context
Phase 1 found no verified recovery: `autosavesInPlace = false`, `autosavingDelay` defaulting to zero and an
inherited autosave file type that would write the imported format (e.g. JPEG) through saved-marker logic.
Undo was a fixed 24 levels with no visible history. Phase 5 built native recovery for dirty native, imported
and untitled documents, an enumerable memory-budgeted history, and named snapshots, sharing one host archive.

## Decisions
- **Own the autosave path.** `ImageDocument` disables inherited draft/in-place autosave
  (`autosavesInPlace`/`autosavesDrafts` return false, `app/Sources/ImageDocument.swift:222`) and routes explicit
  AppKit autosave requests and scheduling through `app/Sources/RecoveryCoordinator.swift`. *Why:* inherited
  autosave could write an imported type and mark it saved.
- **Recovery store** in `~/Library/Application Support/Doctored/Recovery` (tests: `DR_RECOVERY_DIR` and a
  private preference domain). A checkpoint is due **30 s** after an uncheckpointed committed change, polled on a
  **0.5 s** tick (`DR_RECOVERY_INTERVAL_SECONDS` overrides, clamped 1…3600); gestures, Action replay and the
  busy worker defer capture — a scheduling interval, not a durability deadline.
- **Two atomic generations** (`app/Sources/RecoveryStore.swift`): the worker receives captured values only,
  writes a complete generation, then atomically replaces a small manifest; current and previous generations
  are kept, and a damaged current image/selection/snapshot list falls back to the previous with a warning.
  Publication is ordered for power loss: `fsync` the
  staged tree, move it to its generation folder, write `.manifest-<UUID>`, barrier-flush the recovery directory,
  `rename` it over `manifest.json` (the only step under the publication gate's lock), then fully flush the
  directory (a first checkpoint also flushes the recovery root and its parent). Cleanup removes stray
  `.staging-`/`.manifest-` entries.
  File leases (`RecoveryLease`) stop another live instance from claiming an open document's copy; forced
  termination releases the OS lease. Unreadable entries are reported individually, never hiding readable ones.
- **Recovery opens a dirty untitled native copy.** The recorded source path is informational, including when
  the source moved; imported JPEG/RAW originals are never overwritten. A confirmed preserving save removes
  recovery only when the image is clean and the session snapshot list is empty; saving an older captured
  revision keeps newer work; a normal close discards recovery and blocks a late worker publication.
- **Shared host archive** (`app/Sources/EditingStateArchive.swift`) wraps an unchanged native `.rz` image plus
  selection, layer targeting and export settings; its schema is independent of DRDC (v1 then; now v4, adding
  typed/float storage and the linear-light tag). Names, geometry, indices, targets, versions and metadata sizes
  are validated before applying; selection materializability is checked on decode so corrupt coverage cannot
  bypass the previous-generation fallback.
- **History model.** `EditingState` (`app/Sources/EditingState.swift`) is one full immutable state: document,
  current/previous selection, Quick Mask mode, layer selection, paint target, export settings; saved-file
  identity stays on `ImageDocument`. `DocumentHistory` owns the enumerable timeline; `NSUndoManager` keeps
  transition IDs and grouping (`levelsOfUndo = 0`). A new edit after navigation drops the redo branch.
  Window > History and MCP `list_history` / `restore_history` / `set_history_budget` expose it.
- **History budget** = working limit / 4 (`DocumentHistory.budgetBytes`), counting unique core allocations once plus
  deduplicated selection arrays and display/contour estimates; oldest transitions are evicted, but the nearest
  reversible transition always survives (disclosed in panel and report). A separate **4096-transition cap**
  bounds bookkeeping.
- **Named snapshots** (`app/Sources/NamedSnapshots.swift`): at most **32** per session with budget
  working limit / 4 (`NamedSnapshots`); captures over budget are refused; restoring a different state is one undoable
  action, restoring the current one a no-op. Save/Load Snapshot writes/reads portable `.drsnapshot` packages.
  Ordinary `.rz` saves embed neither the session list, the transient selection nor history. Session snapshots
  force recovery even for a clean image and expire on normal close.
- **Deliberate limits:** a transient selection alone on a clean image does not trigger recovery; history and
  snapshot management are not recordable Action steps; history is session-only (no persistent branches or
  unlimited revisions); snapshots are full states, distinct from layer comps (12D-1 `core/src/doc_comps.rs`
  restores an arrangement without pixels, selection or history).

## Rejected alternatives
- Enabling inherited `NSDocument` autosave — writes the source type and passes through saved-marker logic.
- Treating `NSUndoManager` as the history list — it is not enumerable; it only groups/navigates transitions.
- Fixed step count — a byte budget with a nearest-step exception tracks real cost.

## Consequences
- New editing state that must survive a crash belongs in `EditingState` and the archive (bump its version).
- Recovery/snapshot writes are subject to working-memory admission and can refuse; native files do not
  preserve in-memory sharing between snapshots, so a large snapshot list may be refused though cheap in session.
- Tests: `scripts/verify_phase5.py` (MCP, forced restart), `scripts/history-probe/` (AppKit lifecycle),
  `scripts/phase5_{history,recovery,snapshots}.py`.

## Open items (verified 2026-09-26)
- `DONE` — Power-loss durability: the manifest publication is ordered and
  flushed ("Two atomic generations" above); see also [phase-1-integrity-status.md](phase-1-integrity-status.md).
- `DONE` — `verify_phase5.py --suite recovery` and `--suite snapshots` follow
  the current formats: typed layer rows (`mask`, group kinds) and the archive version `EditingStateArchive.swift` declares.
- `DONE` — the capped-history edit's O(n) term: `DocumentHistory`
  evicts its oldest entry by clearing the slot and advancing a head offset (compacting once the dead prefix reaches
  the live count) instead of `removeFirst()`, so the evicted state is released at once and nothing shifts. Medians
  of three runs each (jobs-probe "history bookkeeping cost", -Onone), 50 → 2000 entries: before, `cap_edit_ms`
  0.163 → 0.392 and `cap_drop_ms` 0.003 → 0.113 (growth 0.229 / 0.110, remainder 0.119 ms); after, 0.151 → 0.307
  and 0.003 → 0.115 (growth 0.156 / 0.112, remainder 0.044 ms, inside the 0.05 ms bar). Per run the remainder
  varies: 0.025 / 0.198 / 0.137 ms before, 0.027 / 0.049 / 0.043 ms after — every after run clears the bar, which is
  the evidence for `DONE` (one before run did too, so the medians overstate the contrast). What remains is
  `dropHistoryUndo`'s NSUndoManager walk, AppKit's residual (`levelsOfUndo` stays 0).
- `TRACKED` — Snapshot/recovery payload compression and cross-generation dedupe → P7-32 (plan §13C).
- `TRACKED` — Tiled undo backing → plan §13B; broader history/snapshot workloads at 24/48 MP → P7-33 (§13D).
- `DONE` — Selective layer comps — 12D-1 (`core/src/doc_comps.rs`, Window > Layer Comps…).
- `DONE` — Recovery of embedded assets and high-depth buffers (plan §5 boundary) — Phase 8 retained sources
  (`archive/phase-8-retained-source-status.md`, "Native save, portable snapshots and recovery retain assets")
  and typed/float archive storage (`EditingStateArchive` v2–v4).
- `DONE` — Optimized Swift unqualified (Release `make app`, qualified from Phase 7E on).
- `DROPPED` — Persistent branch history and unlimited revisions: declared out of scope (plan §5 boundary).
