# Five native layer modes: Subtract, Vivid Light, Linear Light, Pin Light, Divide (ADR)

- **Status:** Accepted — closed (its Hard Mix refusal superseded by [phase-10d-threshold-modes-status.md](phase-10d-threshold-modes-status.md))
- **Phase / dates:** 10D five-mode slice, 2026-09-13
- **Original:** [archive/phase-10d-five-modes-status.md](../archive/phase-10d-five-modes-status.md) — full history, measurements, receipts
- **Key commits:** `9f8f37b` (Add five native layer blend modes with frozen format compatibility)

## Context
Continue native RGBA16 raster/isolated-group blending after the
[four-mode slice](phase-10d-four-modes-status.md) with Subtract 13, Vivid Light 18, Linear Light 19, Pin Light 20
and Divide 22. Hard Mix 21 was investigated and deliberately left unsupported because its discontinuity
cannot be qualified by an error tolerance.

## Decisions
- **Encoded-domain definitions (the native contract):** Subtract = `max(0, backdrop − source)`. Vivid Light =
  Color Burn at `2·source` when source ≤ 0.5, Color Dodge at `2·source − 1` above (their endpoint precedence
  kept). Linear Light = clamp(`backdrop + 2·source − 1`, 0, 1). Pin Light = `min(backdrop, 2·source)` when
  source ≤ 0.5, else `max(backdrop, 2·source − 1)`. Divide = `min(1, backdrop / source)`, with source 0 → 1
  **including 0/0** (this app's existing kernel). No claim of another application's blend space or zero-divisor
  behaviour.
- **No kernel, FFI or edit-schema change**; same Rust/Swift authoritative sets feed popup, MCP, Actions.
- **DRDC31 / `LBLD` schema 4** adds exactly 13/18/19/20/22 to the prior thirteen non-default modes; schemas 1–3
  frozen. Writer selects the oldest sufficient wrapper: 31, 30, 27, 26 or none; capacity
  `16 + 8 × non-default record count`. With outside-255 masks: `29 → 31 → prior body/tails → LBLD4 → MOUT1`;
  v31 never wraps 29; releasing either feature restores exact prior bytes. All validation precedes PNG decode.
- **Numerical policy:** 0.65 native codes vs unrounded f64 (quantization included) for ordinary RGB and every
  alpha. Only Vivid Light components classified from exact authored codes get **1.6**: upper branch
  source ≥ 64880, backdrop > 0 and backdrop ≤ 2·(65535 − source); lower branch source ≤ 655, backdrop < 65535
  and 65535 − backdrop ≤ 2·source (equality included). Witness: backdrop 212 / source 65428 (reference
  `65535 × 106 / 107`) with neighbours 65427/65429; saturation witness backdrop 340 / source 65365. Finite-fixture
  bounds, not universal.
- **Hard Mix refused (at this slice):** with opaque backdrop 65534 and an isolated Normal group of opaque 0 plus
  colour 2 at alpha 32767 vs 32768, the exact sum straddles 1 by 1/65535², but f32 group arithmetic rounds both
  cases to exactly 1. Widening the final comparison cannot recover precision lost in the inputs. Any Hard Mix
  support needed an explicit versioned policy, without silently changing prior persisted f32 evaluation.

## Rejected alternatives
- Admitting Hard Mix with a larger RGB tolerance — the branch itself is wrong, not the magnitude.
- f64 group intermediates — would change every previously persisted f32 evaluation.

## Consequences
- The nested-group Hard Mix witness is kept, re-homed as an in-band weak-form case in
  `core/tests/precision_threshold_blending.rs`.
- Measured maxima (production fixtures): Vivid Light ≈1.478 codes (classified), all others ≤ 0.506.
- Qualified by `core/tests/precision_remaining_separable.rs`, `core/tests/precision_blend_mask_wrappers.rs`,
  `scripts/verify_phase10d_blend_decoder.py` and
  [`scripts/verify_phase10d_five_modes.py`](../scripts/verify_phase10d_five_modes.py).

## Open items (verified 2026-09-26)
- `DONE` — Hard Mix (and Darker/Lighter Color, Dissolve) under an explicit versioned policy — `dd3474f`
  (v34/schema 6 strong/weak band policy).
- `DONE` — HSL modes — `ac33773`; clipping — `f65a0d2`; Blend If/styles — `7c130e2`, `b8aa5b5`.
- `DONE` — Knockout (P7-37), adjustment blending (P7-19) — carryover ledger rows verified.
