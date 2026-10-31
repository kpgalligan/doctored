# Experimental cutout output contract (ADR)

- **Status:** Accepted — closed (shipped as Select and Mask's New Layer outputs, 2026-10-11)
- **Phase / dates:** 6C (with 9I execution continuation), 2026-09-10 – 2026-09-12
- **Original:** [archive/phase-6-cutout-output-contract.md](../archive/phase-6-cutout-output-contract.md) — full history, measurements, receipts
- **Key commits:** 0155d41, 2e26401

## Context
An advanced refinement result is an opaque foreground RGB estimate plus a separate alpha. Applying alpha into pixels
would bake it (and risk applying it twice) and make later mask edits destructive. This defines how such a result
becomes one reversible document edit. It was built internal (no menu, brush, MCP command or Actions entry) and is
now Select and Mask's New Layer outputs (below); assembly correctness does not qualify the matte or colors.

## Decisions
- **FFI:** `dr_doc_add_refinement_cutout(document, foreground, alpha, control, err)` (`../core/src/ffi_cutout.rs`,
  model in `../core/src/doc_cutout.rs`) adds a top-level **Refined Cutout** raster layer at canvas origin/size via the
  shared insertion path and native capacity rules. RGB is copied exactly with pixel alpha 255; coverage becomes a
  separate enabled, linked mask with density 1, feather 0. *Why:* alpha is never applied twice and mask edits stay
  independent of foreground pixels.
- **Source preservation:** previous top-level entries become hidden (for inspection over transparency); all pixels,
  masks, descriptions, grouping and child visibility survive, buffers stay shared. Undo restores visibility and active
  layer. Native v10 already represents the result — no format change.
- **Refusals:** empty alpha is a no-op; canvas mismatch, nonopaque foreground, capacity refusal, cancellation and a
  reused job token publish nothing. Foreground RGB in the document's profile is a **caller precondition** (image
  handles carry no profile).
- **Host staleness:** `RefinementCutoutCapture` (`../app/Sources/RefinementCutout.swift`) captures `SelectionCapture`
  plus the categorical brush session snapshot; preparation and publication re-check document/selection identity and
  revision and session identity/revision. Drafts, later strokes, foreign sessions, document edits, or Undo back to the
  same handle cannot revive an old result. One prepared document commits through the central mutation/history path
  as one Undo group.
- **Resources:** core scratch query = 5 bytes/canvas pixel + cloned records/strings + 1 MiB bookkeeping; host admits
  its alpha array separately. Validation/transfer poll every 4,096 pixels; copies are chunked (strings in ≤ 64 KiB
  chunks) so large clones stay cancellable.
- **Display preparation** (`../app/Sources/RefinementCutoutPreparation.swift`, `PreparedDocumentEdit`): assembly, byte
  projection, native composition and preview conversion run as single-use child controls under one parent; Cancel and
  document close reach the active child; workers drain before return. JSON parse and CGImage construction remain
  internally noncancellable (checked at boundaries) — **no universal cancellation-latency promise**. Publication
  requires both core projection and display image.

- **Shipped and widened 2026-10-11:** Select and Mask's New Layer with Layer Mask output is this path
  (`../app/Sources/SelectionRefinement+NewLayer.swift`); New Layer applies the new layer's mask before the one
  publication. The foreground no longer has to be opaque: the layer keeps the foreground's own alpha (the typed
  rule), the projection composites it as stored, and the no-op is `alpha · pixel alpha == 0` everywhere. So the
  composite of a previous cutout is a valid foreground. Only Decontaminate Colors still needs an opaque composite
  (the estimator's rule).

## Consequences
- Any future exposed advanced workflow must output through this path (colors + separate editable mask, one Undo);
  selection-only or mask-only output cannot carry color changes.
- Tests: `../core/tests/refinement_cutout.rs`; AppKit probe `../scripts/selection-probe/cutout.swift`. The typed twin
  lives in `../app/Sources/NativeRefinementCutout.swift` / `../core/src/precision_cutout.rs` (9I).

## Open items (verified 2026-09-26)
- `DONE` — The cutout path is public: Select and Mask's New Layer outputs in the sheet, `refine_selection`
  and Actions (the corpus gate is no longer a ship condition, owner direction 2026-10-10).
