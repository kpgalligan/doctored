# Phase 2 — selection and mask targeting (ADR)

- **Status:** Accepted — closed
- **Phase / dates:** Phase 2 (2A–2D) of the [implementation plan](professional-photo-editor-implementation-plan.md), 2026-09-09
- **Original:** [archive/phase-2-selection-status.md](../archive/phase-2-selection-status.md) — full history, measurements, receipts
- **Key commits:** `95a8397` (Phases 1 and 2 together)

## Context
Commands did not reliably edit what the user indicated: a selected mask could still send a filter or fill to
layer pixels, destructive filters ignored the selection, and selection changes had no undo. Phase 2 defined
one target/scope policy for UI, MCP and Actions, confined edits to coverage, and gave selections history
without making them persistent document state.

## Decisions
- **One `OperationScope` policy** (`core/src/doc_scope.rs`, `app/Sources/OperationScope.swift`) validates
  targets and locks across layer pixels, layer/group mask, colour plane and saved channel; unsupported
  combinations and missing masks/channels refuse explicitly. Groups and adjustment layers have no pixel
  target. MCP `target` defaults to `"layer"` and may name `"mask"`, a plane or `"channel:<name>"`. (At the
  time retouch/text/clear/transforms refused non-pixel targets; 6B and 9G later widened this.)
- **Read everywhere, write through coverage.** Filters read full target storage so neighbourhoods include
  unselected pixels; results blend through selection coverage mapped by the layer/group offset. Zero-coverage
  and off-canvas bytes stay identical; with no selection all stored pixels are eligible. RGBA uses
  encoded-colour premultiplied interpolation; scalar targets interpolate coverage (Rec. 709 luma). Auto
  Tone/Contrast/Color analyse the whole target, write only the selection.
- **Preview equals commit.** Sheets capture one immutable operation (revision, target, layer, selection) and
  refuse to apply if any changed. MCP mutations refuse during a live-edit gesture.
- **Selection history is transient undo.** `ImageDocument` owns selection, previous selection and Quick Mask
  state; each change and each Quick Mask stroke is undoable without dirtying the file; no-ops open no group;
  geometry edits clear/restore selection in their own entry; feather is recorded before combination.
- **Quick Mask replays recorded coverage**, not brush parameters: premultiplied gray/alpha runs as
  little-endian `UInt32` lengths + bytes, base64 (`app/Sources/QuickMaskStroke.swift`), validated before
  applying, canvas-absolute, one shared Rust source-over kernel for preview and replay.
- **Selections are not written to `.doctored`**; saved channels are the persistent workflow. No native version change.

## Rejected alternatives
- Filtering a cropped selection — treats the selection edge as an image edge.
- Replaying Quick Mask from brush parameters — not reproducible across pressure/timing.
- Counting selection edits as file edits — dirties documents for transient state.

## Consequences
- New destructive commands declare target support through `OperationScope` and share the coverage writers.
- Commands that change canvas geometry must clear/restore selection in their own undo entry.
- Tests: `core/tests/{operation_scope,quick_mask}.rs`, `scripts/verify_phase2.py`, `scripts/selection-history-probe/`.

## Open items (verified 2026-09-26)
- `DONE` — Freehand lasso (`EditorTool.freehandLasso`, `app/Sources/Tools.swift:36`), Color Range and
  independent mask transforms (`app/Sources/MaskTransformSheetController.swift`) — Phase 6.
- `DONE` — Broader history budgets and recovery — Phase 5.
- `DONE` — Advanced refinement: edge brushing and colour decontamination ship in Select and Mask
  (`app/Sources/RefineSelectionSheetController+Edge.swift`, MCP `refine_selection` in
  `app/Sources/AgentServer+SelectionRefinement.swift`). `DROPPED` — the cutout quality gate as a ship condition
  (2026-10-10).
