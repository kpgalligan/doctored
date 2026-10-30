# Phase 9I typed refinement — the typed half of P7-12 (ADR)

- **Status:** Accepted (typed half closed; the other half of P7-12 shipped without the quality gate, 2026-10-11)
- **Phase / dates:** 9I, 2026-09-15; Actions replay receipt 2026-09-16/17
- **Original:** [archive/phase-9i-typed-cutouts-status.md](../archive/phase-9i-typed-cutouts-status.md) — thirteen-check drive list, receipt paths
- **Key commits:** `b2e0c80` (typed refine), `fa336cf` (Actions replay, Phase 10 closeout Run 5)

## Context
P7-12 has two halves: advanced edge brushing/decontamination behind the Phase 6 quality gate, and typed (u16)
refinement with reusable high-depth masks. The basic Refine Selection controls previously computed from an 8-bit
projection even on 16-bit documents. This slice makes them native and closes only the typed half; it measures no
matte quality.

## Decisions
- **Kernel:** [`core/src/precision_selection_refine.rs`](../core/src/precision_selection_refine.rs), reached as the
  `refine` kind of the existing `dr_selection_u16` contract (`PlaneOperation::Refine { smooth, feather, shift, contrast }`,
  fields default 0, unknown fields refused). A composition, in this order: `Smooth{smooth}` (skip 0) →
  `Grow{shift}`/`Shrink{-shift}` → `Feather{feather}` (skip 0) → contrast.
- **Contrast arithmetic (the only new math):** `c == 1` is `v >= 32768 ? 65535 : 0`; `0 < c < 1` in f64:
  `t = ((v/65535) − 0.5)/(1 − c) + 0.5`, clamp to [0,1], `round(t·65535)` half away from zero; `c == 0` skipped.
  All-zero controls copy input codes exactly.
- **Ranges** match the byte kernel: smooth/feather ∈ [0,64], shift ∈ [−64,64], contrast ∈ [0,1], finite; otherwise
  refused at `dr_selection_u16_plan` before allocation. Work forecast = sum of stage forecasts; plan adds one `2·n`
  inter-stage plane. The byte path (`selection_refine.rs`, `dr_selection_refine`) is untouched.
- **Swift:** `SelectionRefinementCapture` captures `coverage16` (`maskSamples16()`) on high-depth documents;
  `isNative` selects [`app/Sources/NativeSelectionRefinement.swift`](../app/Sources/NativeSelectionRefinement.swift).
  Selection output commits `.mask16`; mask output is a canvas-sized u16 mask at (0,0) — enabled, linked, density 1,
  feather 0, replacing any mask — via `PrecisionScalarBuffer` → `PrecisionPixelEdit(.setMask)` → the one
  `PrecisionEditorCapture.publish`. Never routed through `dr_doc_adding_layer_mask` or `dr_selection_refine`.
- **Previews are byte display derivatives** (`(v + 128) / 257`, the `maskBytes()` rule) and never enter a commit.
- **Entry points:** Select ▸ Refine Selection, MCP `refine_selection` (adds `"coverage_storage": "u16" | "u8"` to its
  result; added to `typedTools` in `DocumentPrecision.swift`) and Actions replay.
- **Non-goals / unchanged:** every experimental control (`edge_radius`, `edge_strength`, `brush`, `decontaminate`,
  `trimap`, `foreground`) still refuses; no estimator, cutout-assembly, new FFI export, MCP tool, native record or
  `.drsnapshot` schema; no typed render preview of the mask output in the sheet; `refine` not exposed through
  `modify_scalar` or Channels ▸ Modify Plane.
- **Precision receipts:** contrast 0.25 maps 32770/32771 → 32771/32772 (both display as byte 128), and the exact
  odd-code identity. **Never cite contrast 1 as a precision receipt** — its 32768 threshold is exactly the byte-128
  boundary, so a byte pipeline agrees.

- **Superseded 2026-10-11:** `edge_radius`, `smart_radius`, `brush` and `decontaminate` are now
  `refine_selection` arguments at 16 bits too. The edge stage is the typed `refine_edge` SOURCE selection kind
  (`dr_precision_doc_select`: the document is the guide, the coverage and area categories the input), run before
  the unchanged `refine` plane kind, which still refuses `edge_radius`. Decontaminate is
  `dr_precision_doc_estimate_foreground`; the New Layer outputs are the typed cutout assembly. Tests:
  `core/tests/precision_edge_refine.rs`.

## Consequences
- Tests: `core/tests/precision_selection_refine.rs` (identity, threshold, depth receipt, each control = standalone
  kind, chained order, refusals, precancel purity, typed cutout mask with pointer-equal RGBA16 and native round trip),
  with `core/tests/precision_cutout.rs` as foundation. Owned MCP drive: [`scripts/verify_phase9i_typed.py`](../scripts/verify_phase9i_typed.py) (thirteen checks, incl. Actions replay, `transform_mask`, `save_selection`, snapshot and crash recovery).
- The sheet's typed preview/apply branch is driven by the selection probe's `refinement workspace` check
  (`scripts/selection-probe/refinement.swift`): on a 16-bit document it sets an edge radius plus Feather, waits for the
  preview, clicks Apply and undoes it.

## Open items (verified 2026-09-26)
- `DONE` — The other half of P7-12 (edge brushing, color decontamination) ships in Select and Mask at both depths; the assisted run is no longer its ship condition (owner direction 2026-10-10).
- `DONE` — Photographic corpus "no longer on disk" at the time: re-acquired under `~/devel/doctored-fixtures/phase6/` ([gate v2](phase-9i-gate-v2-status.md) decision 6).
- `DONE` — Actions replay entry point for `refine_selection` (`fa336cf`, drive check in `scripts/verify_phase9i_typed.py`).
- `DROPPED` — "Pending RVM checkpoint terms" as a blocker: RVM dropped (gate v2 decision 5).
