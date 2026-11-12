# Phase 1 — save and rendering integrity (ADR)

- **Status:** Accepted — open items remain ([#11](https://github.com/kpgalligan/doctored/issues/11))
- **Phase / dates:** Phase 1 (1A–1D) of the [implementation plan](professional-photo-editor-implementation-plan.md), 2026-09-09
- **Original:** [archive/phase-1-integrity-status.md](../archive/phase-1-integrity-status.md) — full history, measurements, receipts
- **Key commits:** `95a8397` (Phases 1 and 2 together)

## Context
The editor could silently lose work: Save chose native vs flat by `layerCount > 1`, so a rich single-layer
document saved flat; an over-budget group rendered empty and could be exported; oversized documents failed
mid-encode; resize/blur interpolated straight RGB into transparent edges. Phase 1 turned each into an
explicit, tested outcome without changing capacity or colour policy.

## Decisions
- **1A One native-capacity preflight** for writer and reader (`core/src/drdc_capacity.rs`), before any PNG
  encode/decode; 400 M aggregate layer pixels (`MAX_DRDC_TOTAL_LAYER_PIXELS`, `core/src/drdc.rs`) and the
  existing count/plane/blob/group limits unchanged. Growth edits refuse; an oversized in-memory document stays
  reducible/exportable. Invalid masks fail the read instead of vanishing.
- **1B Save routes by a document-versus-format preservation check** (`core/src/doc_preservation.rs`,
  `app/Sources/ImageDocument+Preservation.swift`) over every persisted property (masks, descriptions, styles,
  locks/links, channels, guides, profile, density, packets, latent RGB under zero alpha). Save goes native when
  needed; Export is intentional flattening; flat `save_copy` reports losses and leaves the master dirty; MCP
  `get_document.save_preservation` reports the UI policy. PNG density compares within 0.013 PPI (integer
  pixels-per-metre).
- **1C Render failure is a state, not an empty group.** A group-buffer refusal (`MAX_GROUP_BUFFER_PIXELS` /
  `RenderBudget`, `core/src/doc_group.rs`) keeps the last valid preview with a failure status, refuses
  render/sample/flat save for that revision, and leaves native save available. *Why:* native capacity and
  render capacity are independent.
- **Safe writes stage then replace** (`writeSafely` override): every refusal precedes destination
  replacement; file attributes are retained. *Why:* default Save As cleanup deleted the existing file on refusal.
- **1D Premultiplied sampling** for resize, Gaussian blur and sharpening's blur (`core/src/sampling.rs`);
  storage stays straight RGBA8 in encoded colour; nearest/identity keep latent RGB; masks stay scalar.
- **Non-goals:** no capacity increase, no linear-light compositing.
- **Power-loss durability**:
  every published file is barrier-flushed (`F_BARRIERFSYNC`) before its publishing rename and its parent directory is
  fully flushed (`F_FULLFSYNC`, best effort) after it; an unsupported `fcntl` falls back to `fsync` (SQLite's rule).
  Core writers inherit it from `save_atomically` (`core/src/durable_file.rs`); the app publishes through
  `DurableFile.publish`, and preference/library/MCP-artifact `.atomic` writes add the directory flush (`DurableFile.write`).
- **Leftovers of a killed save** (2026-11-12): a failed write removes its own staged file; a killed one cannot, so
  each save first sweeps its destination's folder. The core removes `<name>.dr-tmp-<pid>-<seq>` regular files whose
  pid is not its own and answers `ESRCH` to `kill(pid, 0)` (`core/src/stale_temp.rs`); the app names its staged
  files `<prefix><pid>-<UUID>` and removes them by the same rule, and a UUID-only file an older build left once it is
  a day old (`app/Sources/StagedFile.swift`). A `.drsnapshot` package is staged as a directory and swept by pid only;
  an older build's `.snapshot-<UUID>` directory is too plain a name to delete by and stays. A live pid (another
  instance) or a symlink is never touched.

## Rejected alternatives
- The `layerCount > 1` predicate — misses every single-layer rich-state case.
- Rendering a budget-refused group as transparent — plausible-looking wrong output.
- Gigabyte test fixtures — reduced per-call budgets exercise the same boundaries.

## Consequences
- Every new persisted property must join the preservation inventory or flat Save drops it silently.
- Render/export paths propagate refusal; they never substitute pixels.
- Spatial kernels use the premultiplied sampler ([phase-4-rendering-contract.md](phase-4-rendering-contract.md)).
- Tests: `core/tests/{native_capacity,preservation_tests,render_failure_tests,alpha_sampling}.rs`,
  `scripts/verify_phase1.py`, `scripts/save-preservation-probe/` (small generated fixtures only).

## Open items (verified 2026-09-26)
- `DONE` — Power-loss durability: `save_atomically` barrier-flushes
  before the rename and flushes the directory after it (`core/src/durable_file.rs`); app saves, exports, snapshots and
  recovery manifests publish through `app/Sources/DurableFile.swift` (see "Power-loss durability" above).
- `DONE` — Optimized Swift build: `make app` builds Release (`Makefile` `app:` target), qualified from Phase 7E on.
- `DONE` — TIFF profile/PPI round trips → Phase 3A ([phase-3-color-metadata-status.md](phase-3-color-metadata-status.md)).
- `DONE` — Native recovery → Phase 5 ([phase-5-recovery-history-status.md](phase-5-recovery-history-status.md)).
- `DONE` — Fixed 24-level undo → budgeted history (`levelsOfUndo = 0`, `app/Sources/ImageDocument.swift`).
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — Large-document and real-photograph corpora
  (24/48 MP workloads measured at the 8 GiB working limit).
