# Basic native layer blending: Normal/Multiply/Screen and Pass Through (ADR)

- **Status:** Accepted — closed (first 10D slice; later slices extend it)
- **Phase / dates:** 10D basic slice, 2026-09-13
- **Original:** [archive/phase-10d-basic-blending.md](../archive/phase-10d-basic-blending.md) — full history, measurements, receipts
- **Key commits:** `1ac7029` (Add native Multiply and Screen layer blending with atomic property edits)

## Context
Typed (RGBA16) documents had only Normal compositing. This slice set the pattern every later 10D mode slice
follows: reuse the byte `blend.rs` kernel at native depth, add a strict metadata edit, and persist new
meaning only through a conditional, versioned wrapper so older files and writers keep their exact bytes.

## Decisions
- **Evaluation:** the captured profile's *encoded* RGB goes through the existing `blend.rs` W3C f32
  source-over kernel. Raster placement, native masks and opacity gate contribution once. Isolated groups keep
  straight f32 colour/alpha across every nested boundary; only the final requested projection stores u16.
  Pass Through evaluates children against the enclosing backdrop, then gates their combined premultiplied
  change through the group kernel. No byte preview, transfer-curve conversion or intermediate quantization.
- **Edit request:** `{op:"layer_blend", version:1, layer:<postorder index>, blend_mode:<DrBlendMode>}`.
  Here modes 0/1/2 (Normal/Multiply/Screen) and 27 (group-only Pass Through). Unknown versions, fields,
  modes and incompatible targets refuse at planning and editing; exact no-ops return no new owner; existing
  metadata admission and single-use cancellation tokens apply.
- **Atomic compound properties:** a typed compound property request prepares every named property through
  throwing APIs before one captured publication — a later refusal cannot publish an earlier blend change.
- **Mode survives structure edits:** raster/source/artwork data and masks stay shared; descendant edits,
  duplication and native retouch ancestor rebuilds preserve the parent mode. Layers popup, MCP
  `set_layer_properties` and Actions share one mode set (Rust `precision_layer_blend.rs`, Swift
  `NativeLayerBlending.swift`).
- **Native format DRDC26 / `LBLD` schema 1:** v26 prepends its version to the prior typed version/body
  (including v25's channel wrapper); after the complete body and tails comes `LBLD`, u32 schema, u32 count,
  then ordered (u32 postorder index, u32 mode) pairs. Only non-default Multiply/Screen records appear; Normal
  and Pass Through keep their older representation. Added capacity is exactly `16 + 8 × record_count` bytes.
- **Conditional writer:** default-only documents keep their prior writer; setting the last non-default mode
  back removes the wrapper and restores exact prior bytes. Every older reader stays.
- **Predecode validation:** layer references, modes/schema, unique ascending indices and target kinds
  (hidden records included) validate before any raster/source PNG decoding.

## Rejected alternatives
- Quantizing group intermediates to u16 — loses sub-code alpha/colour; the f32 chain is the contract.
- Widening an existing schema's meaning in place — each new mode set gets a new frozen schema instead.

## Consequences
- Every later mode slice adds a new wrapper version and schema, freezes the older whitelists, and must restore
  exact older bytes when its modes are released:
  [separable](phase-10d-separable-status.md) (v27/2), [four modes](phase-10d-four-modes-status.md) (v30/3),
  [five modes](phase-10d-five-modes-status.md) (v31/4), [HSL](phase-10d-hsl-modes-status.md) (v33/5),
  [threshold](phase-10d-threshold-modes-status.md) (v34/6), adjustment blending (v37/7).
- Files carrying blend metadata require the v26+ typed reader. MCP catalog and Actions schema were unchanged.
- Qualified by [`core/tests/precision_layer_blending.rs`](../core/tests/precision_layer_blending.rs) (f64
  source-over across every u16 coverage code), the independent decoder `scripts/live-filter-probe/native.py`
  and [`scripts/verify_phase10d_blending.py`](../scripts/verify_phase10d_blending.py).

## Open items (verified 2026-09-26)
- `DONE` — Remaining blend modes — later slices (`50cfea6`, `b6c1fd7`, `9f8f37b`, `ac33773`, `dd3474f`);
  27-entry `COLOR_MODES` in `core/src/precision_layer_blend.rs`.
- `DONE` — Adjustment layers beyond Normal — `ea4d9d1` (v37/schema7, verifies P7-19).
- `DONE` — Clipping, Blend If, layer styles — 10D clipping/Blend If/styles slices (`f65a0d2`, `7c130e2`, `b8aa5b5`; P7-20 verified).
- `DONE` — Knockout — carryover P7-37 verified (12E-3…12E-3d).
- `DONE` — Exchange validation — 16-bit PSD carriage in later slices; plan §10A–10C closed.
- `TRACKED` — Original 9I gate → carryover P7-12.
