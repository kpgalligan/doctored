# Phase 7 high-depth editing — overview (ADR)

- **Status:** Accepted — open items remain ([#9](https://github.com/kpgalligan/doctored/issues/9), [#11](https://github.com/kpgalligan/doctored/issues/11))
- **Phase / dates:** 7A–7E, 2026-09-10 (downstream notes through Phase 10D/9I)
- **Original:** [archive/phase-7-high-depth-status.md](../archive/phase-7-high-depth-status.md) — full history, measurements, receipts
- **Key commits:** `082ccb1` (7A), `2d9bc69` (7B), `4061646` (7C), `7e60190` (7D), `3bd4349` (7E)

## Context
Phase 7 made Doctored a genuinely high-depth editor. The byte editor lost source precision at import, at isolated
group boundaries and inside tone/color operations; a wider store alone could not fix that. Phase 7 was split into five
independently qualified slices, ending with a public 16-bit mode that exposes only a qualified subset and assigns every
remaining gap to a named later slice. Starting Phase 7 did not close the
[Phase 6 cutout quality gate](phase-6-selection-mask-status.md); it stopped being a ship condition on 2026-10-10.

## Decisions
- **Slice sequence and where each contract lives:**

  | Slice | Outcome | ADR |
  | --- | --- | --- |
  | 7A | Typed prototype; select RGBA16 UNORM storage with f32 processing | [decision](phase-7-high-depth-decision.md) |
  | 7B | Preserving PNG/TIFF16 import, typed layer tree, native v11, PNG/TIFF16 output | [neutral path](phase-7-neutral-path.md) |
  | 7C | All 21 adjustments on shared kernels (`precision_v1`), continuous ICC, native v12 | [tone/color](phase-7-tone-color.md) |
  | 7D | u16 masks/channels, geometry, paint/erase/clone, scalar artwork, healing, native v13 | [pixel editing](phase-7-pixel-editing.md), [geometry](phase-7-geometry.md), [batch outcome](phase-7-geometry-paint-status.md) |
  | 7E | Public mode inside existing history/recovery, explicit conversion, export dithering | [public mode](phase-7-public-mode-status.md), [conversion/readout](phase-7-conversion-readout.md) |

- **Authorities:** the [operation matrix](phase-7-operation-support.md) states what is enabled today; the
  [carryover ledger](phase-7-carryovers.md) gave every remaining gap a P7-nn row with a receiving slice and
  acceptance criteria; it closed 2026-10-30 and its open rows are issues. Phase 7 completion closed none of them.
- **Code anchors:** typed storage/compositing `core/src/precision_image.rs` / `core/src/precision_render.rs`; the
  typed document `core/src/precision_document.rs`; the family gate `core/src/precision_capability.rs` and its host
  mirror `app/Sources/DocumentPrecision.swift`; measurements `core/examples/phase7_precision.rs` and the source-linked
  probe [`scripts/precision-probe/`](../scripts/precision-probe/README.md) (`--large`, `--large-color`,
  `--large-pixels`, `--large-public`).
- **Invariants carried through every slice:** legacy byte documents keep their established evaluation and appearance;
  typed refusals never fall back to byte ImageIO or byte APIs; a byte preview is never a master; every slice qualifies
  against fixed numerical gates (`2e-6` float, `2e-6 + 0.5/65535` stored) and declared workloads at real size; a
  resource refusal is a failed target, never a reason to downscale; no resized substitute may count, and the limit is the
  RAM-scaled one, pinned in tests (the tracked working limit: 2 GiB at the test pin; qualification at the 8 GiB minimum); tracked admission
  is not an OS RSS ceiling (observed RSS 2.25–3.05 GiB across the 24 MP runs).
- **Qualified 24 MP workloads** (6000×4000, Apple M2 Max): 7B 9/9 stages, 7C 12/12, 7D 21/21, 7E public 9/9 — each a
  bounded workflow, not arbitrary multilayer/history qualification.

## Consequences
- Later phases (8–13) extend the typed path rather than the byte one, and each closes its P7 rows with its own UI,
  MCP/Actions, Undo, native and recovery evidence.
- Native typed documents now write layered conditional wrappers on top of the v11–v13 base records (up to the float
  v40 wrapper and later native tables); byte documents keep their own versioning, and each reader refuses the other's
  typed versions before decoding pixels.

## Open items (verified 2026-09-26)
- `DONE` — edge brushing and colour decontamination ship in Select and Mask (2026-10-11; `core/src/edge_refine.rs`,
  `core/src/foreground_solve.rs`); the Phase 6 / 9I quality gate is no longer a ship condition (carryover P7-12).
- `DONE` — linear-light phases 2–3 (carryover P7-24, 2026-11-12; [11E](phase-11e-linear-light-status.md)).
- `OPEN` [#9](https://github.com/kpgalligan/doctored/issues/9) — full-frame fallbacks and stage-only cancellation,
  slow 24 MP full projections/conversions (carryover P7-31).
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — uncompressed native payloads, and the PNG16
  encoder's compute before its first write (carryover P7-32); `save_projection16` and Save take a control since
  2026-11-12.
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — 48 MP and broader multilayer/history/snapshot
  workloads, app residency beyond the declared runs (carryover P7-33).
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — stroke/text/canvas caps (50,000 dabs, 4,096
  UTF-16 units, plane/aggregate limits; carryover P7-34). A brush now reaches 5,000 px, the stroke work cap is 2 billion units, a canvas stroke that reaches a cap
  stops laying ink and commits what was laid, and one 16-bit image holds the 100 MP document limit.
- `DONE` — native save plan and reopen charge measured retained operator data plus the
  peak preparation allowance, so long simple stacks reach 1,024 records; see [tone/color](phase-7-tone-color.md).
- `DONE` — every other capability gap recorded at 7A–7E close: see carryovers P7-01…P7-11, P7-13…P7-23, P7-25…P7-27,
  P7-29, P7-30, P7-35…P7-37 (verified/closed per the ledger header) and 12F (`07d9153`, `6400648`) for geometry.
