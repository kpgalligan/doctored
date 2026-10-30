# Typed Blend If — native v35 / `LBIF` (ADR)

- **Status:** Accepted — open items remain (its original "16-bit PSD reported, not written" rule is superseded by [10C-2 slice 3](phase-10c-constructs-status.md))
- **Phase / dates:** 10D Blend If slice, 2026-09-15 (Photoshop measurement and verifier 2026-09-16)
- **Original:** [archive/phase-10d-blend-if-status.md](../archive/phase-10d-blend-if-status.md) — full history, measured maxima, Photoshop 15+1-bit agreement table, receipts
- **Key commits:** `7c130e2` (slice), `fc70079` (PSD ranges written), `d1b6c27` (measured against Photoshop, A4), `502cc88` (GIMP R7), `fa336cf` (MCP verifier)

## Context
The byte model has per-layer Blend If (`style_model::BlendIf`: one channel, two `[lo0, lo1, hi0, hi1]` ramps over
0…255) applied as a weight on the layer's effective alpha. The 16-bit typed model had none, so a Blend If was lost
on precision documents. This slice carries it per layer on the typed model and evaluates it where the byte
compositor does, with a precision policy that survives the move from 8-bit to 16-bit codes.

## Decisions
- **Model:** `PrecisionDocumentLayer` carries `Option<BlendIf>` (`../core/src/precision_blend_if.rs`); identity
  (both ramps full) stores as `None`, so a document without one keeps its prior native writer.
- **Targets:** every record: rasters, adjustments (styles scope phase 1; CMYK phase 2b-1) and groups. A Pass Through group
  carrying Blend If composites ISOLATED (`PrecisionDocumentLayer::isolates`, styles scope phase 2b-2), and a styled group
  may switch to Pass Through.
- **Evaluation point:** `precision_render.rs` multiplies `style_blend_if::source_weight_16` into the effective alpha
  AFTER `checked_alpha` (the 2^-96 floor keeps its meaning) in the Raster and Group arms; weight 0 skips like
  coverage 0; Dissolve thresholds on the weighted alpha; a never-touched accumulator pixel reads channel 0.
- **Ramps on 16-bit CODES:** knee `k` → code `257·k` (exact in f32). The this-layer value is
  `(unit·65535).round()`; the underlying value is the same rounding of the f32 composite beneath; gray is
  `LUMA_R·r + LUMA_G·g + LUMA_B·b` on unit values, rounded the same way. Weights are the byte `ramp_weight` table with
  the substituted knees — each a correctly rounded quotient of integers < 2^24.
- **Byte parity:** for a promoted document (every code `257·c8`) over a direct backdrop, red/green/blue weights are
  bit-identical to the byte ones and `|typed − 257·byte| ≤ 129` codes. Gray keeps sub-8-bit luminosity and may
  land on the other side of a knee — documented, not matched. A masked clip-base footprint differs (u8 vs f32) —
  documented, not matched.
- **Band policy:** `BLEND_IF_BAND = 2^-5` codes (unit rounding ≤ 0.002 + product rounding 2^-8, gray adds ≤ 0.016,
  doubled). Strong form (direct inputs, every margin ≥ band): within 0.65 codes of the f64 reference. Weak form
  (composited or in-band): within 0.65 of ONE of the neighbouring-code references, never a mixture.
- **Clip units:** a base's footprint is its UNWEIGHTED alpha × mask, recorded before the weight; the base's Blend If
  then weights only its own `buf` alpha; members are CLAMPED (`min(base_alpha)`), never forced, so they stay
  visible where the ramp hides the base. A base without Blend If keeps the force-equal loop bit-identical. A
  member's Blend If reads the clip buffer as underlying. (A styled base uses the byte u8 shape instead —
  [layer styles](phase-10d-layer-styles-status.md).)
- **Edit/FFI:** `{op:"layer_blend_if",version:1,layer,blend_if}`, strict (unknown fields, channel, unordered ramps,
  bad target refuse in plan and edit); capability family `layer_blend_if` on raster/group. A typed source asset
  refuses a layer carrying Blend If.
- **Host:** a typed layer's style row carries the Blend If; UI/MCP/Actions share the byte layer-style path
  (`DoctoredCore.layerStyle` / `withLayerStyle`, now `../app/Sources/NativeLayerStyle.swift` — the original
  `NativeBlendIf.swift` and its sheet were folded into it and the shared `LayerStyleSheetController` /
  `BlendIfPane` by the layer-styles slice).
- **Native v35:** outermost conditional wrapper, never nested; `LBIF` schema 1 after `LCLP`:
  `{u32 index asc, u32 channel 0…3, [u8;4] this, [u8;4] underlying}`. Empty/noncanonical/identity entries, `LBIF`
  under a non-35 header and 35 nested in 32 refuse before any PNG decode; every record kind may hold an entry (a Pass
  Through group since styles scope phase 2b-2). Releasing the last Blend If restores the prior writer's exact bytes.
- **PSD (both depths, since 10C-2 slice 3):** written as the record's 40-byte blending ranges, chosen channel in its
  slot, every other slot `[0,0,255,255]`; imported back iff exactly one of gray/red/green/blue is restricted.

## Rejected alternatives
- Continuous (unrounded) ramp evaluation — a hard edge could not be exact at the knee code; steep ramps carried ≈2/span codes of error.
- Source ramp on codes but underlying on unrounded floats — two footings for one weight.

## Consequences
- Blend If stays metadata over the f32 compositor, not a pixel kernel; any compositor change must keep the
  weight-after-`checked_alpha` point and the unweighted clip footprint.
- Photoshop 26.10.0 (15+1-bit) agrees on every red-channel case and both 16-bit knees land on `257·k` exactly; the
  gray channel agrees since Gray took Photoshop's weights. Photoshop accepts ranges only via the `blendRange` string ID and
  never reads them back, so written bytes are the only readout.
- Qualification: `../core/tests/precision_blend_if.rs`, `../core/tests/psd_photoshop_constructs.rs` (fixture
  `../core/tests/fixtures/psd_photoshop/blend_if/`), `../scripts/verify_phase10d_blend_if.py` (B1–B5),
  `../scripts/verify_phase10_photoshop_appearance.py --receipt A4`, GIMP R7 (GIMP ignores ranges).

## Open items (verified 2026-09-26)
- `DONE` — live typed preview in the Layer Style sheet at 16 bits, Blend If included (`PrecisionStylePreview.swift`; a 32-bit float document still has none).
- `DONE` Gray — Gray is `0.30 R + 0.59 G + 0.11 B` of the stored triple (`blend::lum`, in `style_blend_if::channel_unit`), at every depth; fitted on the receipts and confirmed on a fresh take (`../scripts/photoshop-probe/style_v1x_take.py`, `style-parity-v1x/take2`): take2 2.67 / 0.13 codes at 8 bits (the 8-bit soft ramp's own residual, which Red shares), 0.06 at 16, 0.00 at 32, no hard-knee patch differing; Rec. 709 missed by 126 / 6. Tests `../core/tests/blend_if_gray_parity.rs`.
- `DONE` Styles scope phase 1 — Blend If on an RGB adjustment layer at 8, 16 and 32 bits: This Layer reads the ADJUSTED colour, Underlying the composite beneath, both confirmed by the take (`../scripts/photoshop-probe/styles_scope_558_take.py`, `styles-scope-558/take1`, A4, 0 codes); a Blend-If-only adjustment renders through the styled path under an identity style. Recorded for phase 4: two channels combine closest to PRODUCT (M1 Gray + Red 2.4 codes; M2 Cyan + Black on 255 − ink 2.4), and Photoshop accepts a range naming an alpha channel (M3).
- `DONE` Styles scope phase 2a — Blend If on several channels at once (Gray plus Red / Green / Blue; Gray plus the CMYK plates) at 8, 16 and 32 bits: `BlendIf.extra` (canonical: identity ranges dropped, channel order, no channel twice), the ranges' weights MULTIPLIED (`style_blend_if::product`), `.rz` word 101 at both depths (`blend_if_channels_native`) with `LBIF` schema 2, every nondefault PSD slot carried both ways; a channel the mode lacks refuses in any range as written. Tests `../core/tests/blend_if_multichannel.rs`, `../core/tests/precision_blend_if_multichannel.rs`. PRODUCT is CONFIRMED by the multichannel take, run in phase 2b-1 (`../scripts/photoshop-probe/blend_if_multichannel_take.py`, `blend-if-multichannel-558/take1`, its RECORDED RESULT): it met every judged row (c3 RGB8 0.93, RGB16 0.009, u3 0.93 / 0.009, p3 CMYK8 1.05 codes; MIN 9.6–39). Fixture tests: `photoshops_rows_meet_the_takes_winner_within_its_bound` and the c3 / p3 imports. The take also found that Photoshop 26 writes some gradient stops as `redFloat`/`greenFloat`/`blueFloat`, which declined a record's whole style; `psd_layer_effects_fx::colour` now reads them.
- `DONE` Styles scope phase 2b-1 — Blend If on a CMYK document's adjustment layer at 8 and 16 bits: This Layer reads the ADJUSTED ink, Underlying the composite beneath (K2 T-ADJUSTED 0 codes on the same take; [layer styles](phase-10d-layer-styles-status.md) holds the rest). A range naming `Alpha 1` is written to slot 0 of the PSD's blending ranges and is Photoshop's Gray range (take1 N; the close-out below).
- `DONE` Styles scope phase 2b-2 — Blend If on a Pass Through group at 8, 16 and 32 bits and on CMYK: the group composites ISOLATED (P3 0 codes on the take; [layer styles](phase-10d-layer-styles-status.md) has the record).
- `DONE` Styles scope close-out — alpha-channel Blend If is NOT built, because a PSD's blending ranges cannot hold one: a range naming an alpha channel is written to slot 0 and renders as the Gray range. Offline, take1's n1 / n2 and styles-scope M3 change exactly the gray > 128 pixels (0 pixels off; M3's alpha 14 720 off), and the PSDs name no channel. take2 (`../scripts/photoshop-probe/blend_if_alpha_take.py`, `blend-if-multichannel-558/take2`, the alpha on the top half) judged GRAY 0 codes on N1 and N2 (ALPHA 255), verdict VALID. Doctored already reads slot 0 as Gray: `blend_if_multichannel.rs` `photoshops_alpha_named_ranges_import_and_render_as_gray` imports take1's n1 / n2 within 1 code.
- `DONE` — PSD ranges written at both depths (`fc70079`; `precision_blend_if.rs` test `blend_if_is_written_as_blending_ranges_through_sixteen_bit_psd`).
- `DONE` — Photoshop 16-bit layered import that this slice lacked (`5e0ed4d`); MCP verifier (`fa336cf`); GIMP R7 (`502cc88`).
- `DONE` — typed fill opacity/effects ([layer styles](phase-10d-layer-styles-status.md), `b8aa5b5`), adjustment blending (`ea4d9d1`), knockout (P7-37 verified).
