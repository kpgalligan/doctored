# Four native layer modes: Color Dodge, Color Burn, Linear Dodge (Add), Linear Burn (ADR)

- **Status:** Accepted — closed
- **Phase / dates:** 10D four-mode slice (plus mask-wrapper integration), 2026-09-13
- **Original:** [archive/phase-10d-four-modes-status.md](../archive/phase-10d-four-modes-status.md) — full history, measurements, receipts
- **Key commits:** `b6c1fd7` (Add four native layer blend modes with mask-compatible persistence)

## Context
Extend native RGBA16 raster/isolated-group blending (after [basic](phase-10d-basic-blending.md) and
[separable](phase-10d-separable-status.md)) with Color Dodge 10, Color Burn 11, Addition 12 (menu name
**Linear Dodge (Add)**) and Linear Burn 15. Dodge/Burn divide, so f32 rounding near a singular divisor can
exceed the usual error budget; the slice also had to compose with the independently developed mask29
(outside-255 mask) wrapper.

## Decisions
- **Kernels unchanged.** Color Dodge/Burn follow
  [W3C endpoint precedence](https://www.w3.org/TR/2024/CRD-compositing-1-20240321/#blendingcolordodge);
  Addition clamps the encoded sum at 1; Linear Burn clamps `sum − 1` at 0. Masks and opacity gate source
  alpha once; group intermediates stay f32.
- **Frozen schema table** (each whitelist explicitly frozen; the current editable set never widens an old one):

  | Wrapper | `LBLD` schema | Allowed non-default modes |
  | --- | --- | --- |
  | 26 | 1 | Multiply 1, Screen 2 |
  | 27 | 2 | modes 1–9 |
  | 30 | 3 | modes 1–9 plus 10, 11, 12, 15 |

  The writer picks v30 only when one of the four is present; removing them restores exact v27/v26/older
  bytes. Old schemas carrying a new mode refuse; all validation precedes PNG decoding.
- **Composition with masks:** canonical order is outer mask **29**, then blend **30**, then the prior typed
  body and tails, `LBLD` schema 3, then `MOUT` schema 1. v30 never wraps 29. The mask reader admits 30 as a 29
  inner version; host routing admits byte 28 and typed 29/30. Each wrapper is independently optional and
  removing either restores the other's exact standalone bytes.
- **Numerical policy — component-classified bound.** Ordinary components, sums, meaningful-alpha group
  fixtures and every alpha comparison keep **0.65 native codes** against unrounded f64 (final quantization
  included). Only Dodge/Burn components classified from exact authored codes as near-singular division (or
  the exact saturation boundary) get **1.6 codes**. Retained witness: opaque Color Dodge with Cb = 106/65535,
  Cs = 65428/65535 (reference `65535 × 106 / 107`), plus its source-code neighbours. These are finite-fixture
  qualification bounds, **not** a proof over every f32 intermediate.
- **No new FFI or edit schema**; the authoritative sets remain in `precision_layer_blend.rs` and
  `NativeLayerBlending.swift`, shared by popup, capability reporting, MCP and Actions.

## Rejected alternatives
- A blanket 1.6-code tolerance — review required narrowing it to classified Dodge/Burn components so ordinary
  errors can't hide behind it.

## Consequences
- The 0.65 / classified-1.6 pattern is the numerical precedent reused by the
  [five-mode slice](phase-10d-five-modes-status.md) (Vivid Light).
- Measured maxima (finite grid, including quantization): Color Dodge ≈1.477 codes, Color Burn ≈1.389 codes,
  Addition/Linear Burn < 1e-9; masked projections ≤ 0.501. Details in the archive.
- Qualified by [`core/tests/precision_four_blending.rs`](../core/tests/precision_four_blending.rs),
  `core/tests/precision_blend_mask_wrappers.rs`, `scripts/verify_phase10d_blend_decoder.py` and
  [`scripts/verify_phase10d_four_modes.py`](../scripts/verify_phase10d_four_modes.py).

## Open items (verified 2026-09-26)
- `DONE` — Remaining native modes incl. Hard Mix — `9f8f37b`, `ac33773`, `dd3474f`.
- `DONE` — Clipping, styles, Blend If, broad PSD exchange — `f65a0d2`, `7c130e2`, `b8aa5b5`; plan §10A–10C closed.
- `DONE` — Knockout — carryover P7-37 verified; P7-19/P7-20 verified.
