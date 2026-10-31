# Blend modes on 16-bit adjustment layers — v37 / `LBLD` schema 7 (ADR)

- **Status:** Accepted — open items remain ([#14](https://github.com/kpgalligan/doctored/issues/14))
- **Phase / dates:** 10D adjustment-blending slice, 2026-09-15 (verifier and GIMP receipt 2026-09-16)
- **Original:** [archive/phase-10d-adjustment-blending-status.md](../archive/phase-10d-adjustment-blending-status.md) — full history, per-mode measured maxima, receipts
- **Key commits:** `ea4d9d1` (slice, closes 10D), `502cc88` (GIMP R8), `fa336cf` (MCP verifier)

## Context
Typed (RGBA16) raster and isolated-group layers already had twenty-six non-Normal blend modes, but a typed
adjustment layer was Normal-only, and a 16-bit PSD adjustment record's blend key was imported as Normal with a
loss. This slice closes P7-19 by giving typed adjustments the same modes with the byte compositor's own rule,
without changing any existing mode meaning or any previously written file.

## Decisions
- **Meaning = the byte rule verbatim** (`../core/src/doc.rs` `composite_adjustment_into`, unchanged): the layer's
  own pixels are ignored; `adjusted = adjustment(backdrop)`, `effective = B(backdrop, adjusted)`,
  `out = backdrop + (effective − backdrop)·k` with `k = opacity × mask coverage`. Alpha never changes; an
  `acc` alpha-0 pixel is untouched, RGB included. Normal is bit-identical to the prior build (`b_normal` returns
  `cs`). No/disabled mask = canvas-wide; an enabled mask is 0 outside its extent.
- **Dissolve** keeps its all-or-nothing dither on `blend.rs::dissolve_threshold(x, y) < k`; the typed accumulator is
  canvas-at-origin, so the typed pattern IS the byte pattern.
- **Nonfinite guard stays on the kernel output**, right after `apply_at` and before any blend branch (so Dissolve's
  shown branch cannot route NaN), plus a second check with the same text on `effective`.
- **No new pixel math**: the f32 `blend.rs` table, `blend::mix_adjusted_rgb`, no intermediate quantization. Byte and
  typed are matched bit-for-bit on equal normalized f32 inputs.
- **Routing:** one `BlendTarget` (`Raster | IsolatedGroup | PassThroughGroup | Adjustment`) evaluated by
  `precision_layer_blend::validate` for every caller (`with_blend_mode`, the edit target, `native_preflight`,
  `PrecisionLayer::with_blend_mode`). No new FFI and no new edit schema: the version-1 `layer_blend` request admits
  modes 1…26 on adjustments. `precision_layer_blend::COLOR_MODES` (27 entries) and Swift
  `NativeLayerBlending.colorModes` stay the authoritative sets; Pass Through remains group-only.
  `DocumentPrecision.support.adjustment_blend_modes` lists the full set.
- **Native v37 / `LBLD` schema 7 adds no mode, only a TARGET** (kind-3 adjustment records). Framing is schema 6's
  (`u32 schema, u32 count`, ascending `(postorder index, mode)` pairs, capacity `16 + 8n`); `encoded_bound` is
  unchanged. The writer picks 37 **iff some nondefault record is an adjustment**, else the oldest sufficient of
  34/33/31/30/27/26; releasing the last adjustment mode restores the prior writer's exact bytes. Wrapper order
  `36 → 35 → 32 → 29 → 37 → body/tails → LBLD7 → MOUT → LCLP → LBIF → LSTY` (37 never wraps 29 or 32).
  Schemas 1–6 are frozen and still refuse a kind-3 target before PNG decode; a 37 header with schema ≠ 7, or schema 7
  under ≤ 34, refuses; mode 27 (Pass Through) is never storable. (`DRDC_VERSION` has since moved on.)
- **16-bit PSD:** export already wrote `blend_key(layer.blend_mode())` on the pixel-less record; import now takes the
  same `typed_blend` path as a raster, so the "16-bit adjustment layer cannot represent" loss sentence is retired.
- **Precision policy:** continuous modes within 0.65 native codes of an f64 oracle (Invert adjustment,
  `../core/tests/common/native_blend_oracle.rs`, never calling `blend.rs`); Color Dodge, Color Burn and Vivid
  Light keep the declared 1.6-code allowance in their divisor neighbourhoods; threshold modes (Dissolve, Darker/
  Lighter Color, Hard Mix) under the v34 band policy's WEAK form only, because an adjusted colour is a composited
  input ([threshold modes](phase-10d-threshold-modes-status.md)); Dissolve exact on both branches.
- **Knockout is excluded, not deferred here:** it had no byte implementation to migrate (recorded as P7-37).

## Rejected alternatives
- Moving the nonfinite guard to `effective` — a Dissolve NaN would reach the accumulator.
- Re-resolving `blend_kind` in the adjustment arm — `render_layers` already resolves it once per layer.
- An f64 accumulator — changes every persisted evaluation.
- Widening the RGB tolerance to cover threshold branches — the band policy exists so that never happens.

## Consequences
- Every change to adjustment compositing must stay identical at both depths; `composite_adjustment_into` is the
  spec. Schemas 1–6 must keep refusing kind-3 targets.
- Qualification: `../core/tests/precision_adjustment_blending.rs` (14 tests); independent decoders
  `../scripts/live-filter-probe/native.py` and `../scripts/verify_phase10d_blend_decoder.py`; MCP verifier
  `../scripts/verify_phase10d_adjustment_blending.py` (gates A1–A4); GIMP receipt R8 via
  `../scripts/verify_phase10_receipts.py --receipts R8` (GIMP keeps the record as an empty raster with the key).

## Open items (verified 2026-09-26)
- `DONE` Styles scope phase 1 — Blend If and layer styles on RGB adjustment layers at 8, 16 and 32 bits ([layer styles](phase-10d-layer-styles-status.md) holds the take and the rule). An UNSTYLED adjustment keeps `composite_adjustment_into` byte for byte at both depths; a STYLED one applies the same per-pixel body in place after its own Below effects, alpha kept, at `k = mask × Fill × Blend If × opacity`. A CMYK adjustment followed in phase 2b-1.
- `DONE` Styles scope phase 2b-1 — Blend If and layer styles on a CMYK adjustment layer at 8 and 16 bits ([layer styles](phase-10d-layer-styles-status.md) holds the take). An UNSTYLED CMYK adjustment keeps `cmyk_stack`'s `Entry::Tone`. A STYLED one (any style, Fill-only included, or a Blend If) applies its tone in place through `cmyk_composite::tone_mix_mode` after its own Below effects, alpha kept, at `k = mask × Fill × Blend If × opacity` (`cmyk_style_row`).
- `OPEN` [#14](https://github.com/kpgalligan/doctored/issues/14) — a typed rendered-plane cache for styled layers (`precision_style_render.rs` renders planes per render).
- `DONE` — knockout / `blend_interior_as_group` (P7-37): byte `78cb0ba`, typed `1a69abb`, PSD `1063561`, Photoshop `e03026e`; carryovers P7-37 VERIFIED. (P7-38/P7-39 are owned by 10C-3.)
- `DONE` — MCP verifier (`fa336cf`) and GIMP reopen receipt R8 (`502cc88`).
