# Four threshold native layer modes: Hard Mix, Darker Color, Lighter Color, Dissolve (ADR)

- **Status:** Accepted — closed
- **Phase / dates:** 10D threshold slice, 2026-09-14 (verifier and GIMP receipts 2026-09-16)
- **Original:** [archive/phase-10d-threshold-modes-status.md](../archive/phase-10d-threshold-modes-status.md) — full history, measurements, receipts
- **Key commits:** `dd3474f` (implementation), `fa336cf` (MCP verifier), `502cc88` (GIMP receipt R6)

## Context
The [five-mode slice](phase-10d-five-modes-status.md) refused Hard Mix because f32 group arithmetic can pick the
wrong side of a discontinuity. This slice admits Dissolve 14, Darker Color 16, Lighter Color 17 and Hard Mix 21 for
native RGBA16 rasters and isolated groups through the **unchanged** f32 kernels, under an explicitly versioned
precision policy that says what is and is not guaranteed near each threshold. With it, every raster/isolated-group
mode the byte compositor offers is qualified at native depth.

## Decisions
- **Kernel definitions** (`core/src/blend.rs`, over encoded document RGB, used as-is):
  - Hard Mix per channel: `B(cb, cs) = 1` if `cb + cs > 1`, else 0; at the exact tie
    (`|cb + cs − 1| ≤ 2⁻²²`, `HARD_MIX_TIE`) 1 iff `cb − cs > 1/32768`, else 0
    ([cmyk-mode-shipped.md](cmyk-mode-shipped.md), "Hard Mix's exact tie").
  - Darker / Lighter Color: whole triple with lower / higher `lum = 0.3R + 0.59G + 0.11B` (f32 constants);
    backdrop on a tie.
  - Dissolve replaces compositing: the source shows fully opaque (straight colour, alpha 1) when
    `t(x, y) < sa`, where `sa = alpha × opacity × mask coverage`; otherwise the backdrop is untouched.
    `t` is a 24-bit Murmur3-style mix on **canvas-absolute** coordinates: `h = x·0x9E3779B9 + y·0x85EBCA6B`
    (wrapping u32); `h ^= h>>16; h *= 0x85EBCA6B; h ^= h>>13; h *= 0xC2B2AE35; h ^= h>>16; t = (h>>8)/2^24`
    (exact in f32, < 1). `precision_render.rs::render_layers` already passes canvas-absolute `(x, y)`, so a
    translated layer changes which positions show, not the pattern.
  - Byte and typed evaluation are matched bit for bit on equal normalized f32 inputs.
- **Mode sets / edit:** `COLOR_MODES` (27 entries) and `NativeLayerBlending.colorModes`; the version-1
  `layer_blend` request admits 14/16/17/21. No new FFI or edit schema.
- **DRDC34 / `LBLD` schema 6** adds exactly 14/16/17/21 to the prior twenty-two; schemas 1–5 frozen (patched older
  files refuse naming the mode). Writer picks the oldest sufficient wrapper (34, 33, 31, 30, 27, 26, none);
  capacity `16 + 8 × count`. Order `32 → 29 → 34 → body/tails → LBLD6 → MOUT → LCLP`; releasing the last threshold
  mode restores exact prior bytes. (Later wrappers — v35 `LBIF`, v36 `LSTY`, v37 — sit outside.)
- **16-bit PSD** carries `diss`/`dkCl`/`lgCl`/`hMix` both ways with no loss sentence; since this slice no
  raster/folder blend key is unrepresentable in the typed model.
- **Precision policy (v34):**
  - **Margin `m`** from the independent f64 oracle on exact authored codes: Hard Mix `|cb_c + cs_c − 1|` per
    channel; Darker/Lighter `|lum(cs) − lum(cb)|`; Dissolve `|sa − t(x,y)|`.
  - **Bands** (normalized): `HARD_MIX_BAND = 2^-21`, `COLOR_BAND = 2^-20`, `DISSOLVE_BAND = 2^-20`
    (`core/tests/common/native_blend_oracle.rs`), derived from the f32 error of the u16→f32 conversion
    (≤ 2⁻²⁵), one sum (Hard Mix), the `lum` products/sums, and `checked_alpha`'s products. The composite step is
    not budgeted — hence the restricted strong form.
  - **Direct inputs:** source is a stored raster's codes; backdrop is the transparent canvas or exactly one
    opaque, unmasked, opacity-1 raster; for Dissolve `sa` is direct when the layer is a raster. Anything else
    (group source, partial-alpha or multi-layer backdrop, a Dissolve group's accumulated alpha) is composited.
  - **Strong form (direct only):** if `m ≥ band`, the render takes the oracle's branch and every component is
    within 0.65 codes of that branch's f64 reference (quantization included).
  - **Weak form (every input):** output is within 0.65 codes of **one** of the two branch references — never a
    mixture — and the pick is a pure function of the f32 intermediates, frozen as the v34 evaluation. No branch
    is guaranteed for composited inputs. Dissolve additionally: a shown pixel equals the source codes exactly
    at alpha 65535; a hidden pixel equals the render without that layer exactly.
  - **In-band by construction (documented, not refused):** Hard Mix pairs with `b + s = 65535`, equal-luma
    triples, and the five-mode nested-group witness (margin 1/65535²).

## Rejected alternatives
- Quantize-then-compare — requires intermediate group quantization and a typed-only kernel divergence.
- An f64 accumulator — changes every previously persisted evaluation and still leaves a band at 2⁻⁵³.
- Per-depth composite bands — fixture-specific budgets a reader can't apply to an arbitrary document.

## Consequences
- Anything that composites threshold modes over an adjusted or composited source (e.g. adjustment-layer
  blending) may assert only the weak form. Code comments in `core/src/precision_blend_native.rs` and tests cite
  this policy by name.
- Changing any f32 intermediate in these kernels changes the frozen v34 evaluation; the one change since,
  Hard Mix's exact tie (2026-10-15), was made without a DRDC version
  ([cmyk-mode-shipped.md](cmyk-mode-shipped.md)).
- Qualified by [`core/tests/precision_threshold_blending.rs`](../core/tests/precision_threshold_blending.rs)
  (exhaustive direct Hard Mix over 65,536 codes: the exact-sum in-band row follows the tie rule; every
  mask code; nested groups; Dissolve at two offsets/opacities, raster shown/hidden pixels exact),
  [`scripts/verify_phase10d_threshold_modes.py`](../scripts/verify_phase10d_threshold_modes.py) (gates T1–T4)
  and GIMP 3.2.6 receipt R6 in [`scripts/verify_phase10_receipts.py`](../scripts/verify_phase10_receipts.py)
  (keys reopen as `hard-mix`, `luma-darken-only`, `luma-lighten-only`, `dissolve`; GIMP's own layered
  arithmetic differs and is recorded, not gated).

## Open items (verified 2026-09-26)
- `DONE` — Dedicated MCP verifier — `scripts/verify_phase10d_threshold_modes.py` (`fa336cf`, gates T1–T4 passed).
- `DONE` — GIMP reopen receipt for the four keys — R6 (`502cc88`).
- `DONE` — Blend If (`7c130e2`), layer styles (`b8aa5b5`), adjustment-layer blending (`ea4d9d1`, P7-19);
  knockout → carryover P7-37 verified (12E-3…12E-3d); P7-20/P7-21 verified.
