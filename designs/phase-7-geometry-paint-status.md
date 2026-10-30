# Phase 7D geometry, paint and retouch — outcome (ADR)

- **Status:** Accepted — open items remain
- **Phase / dates:** 7D, 2026-09-10
- **Original:** [archive/phase-7-geometry-paint-status.md](../archive/phase-7-geometry-paint-status.md) — full history, measurements, receipts
- **Key commits:** `7e60190` (7D, on top of 7C `4061646`)

## Context
7D migrated geometry, masks/channels, paint, cloning, rasterized artwork and healing onto the internal typed
document, each batch qualified independently, before 7E exposed a public 16-bit mode. The contracts live in
[pixel editing](phase-7-pixel-editing.md) and [geometry](phase-7-geometry.md); this record keeps the batch outcome,
the rules it established for qualification, and the 7E handoff.

## Decisions
- **Six delivered batches:** (1) persistent u16 masks and named channels, native v13; (2) shared typed
  affine/perspective/resize/crop/quarter-turn/flip sampling; (3) continuous-color paint, coverage fill, RGB-preserving
  erase, integer-offset clone; (4) scalar TextKit/shape coverage assembled into u16 rasters; (5) explicit-window healing
  on the shared membrane solver; (6) three strict C exports (`dr_precision_doc_edit_plan`, `dr_precision_doc_edit`,
  `dr_precision_doc_read_plane`) with Swift wrappers, captured edits, payload leases and native/archive integration.
- **Qualification rule — fixtures may change, gates may not.** The first 24 MP run legitimately failed (a transparent
  column split the heal domain into a no-op); only the fixture changed (two admitted typed paints prepare the patch).
  Two Content-Aware-Fill timing ratios failed under concurrent builds and passed in isolation; no threshold changed.
- **Numerical results kept as reference points:** float `2e-6`, stored `2e-6 + 0.5/65535 = 9.6295e-6`; paint max
  `7.76e-6`; filtered affine max `7.68e-6`; four f64 healing domains within one u16 code (two-code test bound).
- **Healing solver requalified:** the opt-in million-unknown benchmark in `heal_tests.rs` still meets its f64
  reference (0.0006 / 0.0025 / 0.0066 byte codes at seam / band / interior); only the timing note in
  `core/src/poisson.rs` was refreshed.
- **Outcome:** 974 Rust tests, 13 Swift + 2 independent format groups, 16 Release MCP checks, and **21/21 stages** of a
  native 6000×4000 PNG16 workflow (import, u16 mask/channel, masked projection, turns, bilinear affine, native/archive
  round trips, paint/erase/clone, healing, text/shape) under the 2 GiB tracked limit; 2.72 GiB RSS; all tracked
  resources zero at exit. Reproduce with `python3 -B scripts/precision-probe/verify.py --keep --large-pixels`
  ([probe README](../scripts/precision-probe/README.md)).
- **Process note:** freeze Swift/header inputs before the app build, then run the Rust suite, so the probe links an
  immutable library copy with a recorded hash.

## Consequences
- The 7E handoff defined the public integration that followed: typed ownership inside the existing document/history/
  recovery model (no second Undo system), one operation/target/depth capability decision for UI/MCP/Actions backed by
  core refusals, explicit mode selection and flattened conversion (×257 promotion does not preserve old group/adjustment
  evaluation), deliberate export dithering with depth-loss reporting, and authoritative typed readouts. A byte preview
  is never a master.
- These 24 MP numbers are bounded workflow measurements (Release Rust, Swift `-Onone`), not interactive latency.

## Open items (verified 2026-09-26)
- `DONE` — all six 7E handoff tasks (7E, `3bd4349`; [public mode](phase-7-public-mode-status.md)).
- `DONE` — styles, clipping and other blend modes at depth (carryovers P7-19, P7-20, verified 10D).
- `DONE` — group/adjustment-content free transforms (12F-2, `07d9153`; `geometry_affine_set` in
  `core/src/precision_capability.rs`).
- `DONE` — mask/channel paint/filter/morphology and RGBA-plane writeback (P7-08); Sample All/composite sources and
  fractional clone (P7-15/P7-16); content-aware, spot healing, red-eye (P7-17/P7-18); editable/attributed artwork
  (P7-04/P7-05).
- `TRACKED` — uncompressed native payloads → carryover P7-32 (13C).
- `TRACKED` — 48 MP and general large multilayer/history workloads → carryover P7-33 (13D).
