# Four nonseparable native layer modes: Hue, Saturation, Color, Luminosity (ADR)

- **Status:** Accepted — closed
- **Phase / dates:** 10D HSL slice, 2026-09-14 (production gate and GIMP receipt R9 2026-09-27)
- **Original:** [archive/phase-10d-hsl-modes-status.md](../archive/phase-10d-hsl-modes-status.md) — full history, measurements, receipts
- **Key commits:** `ac33773` (Add Hue, Saturation, Color and Luminosity to typed layer blending)

## Context
Add Hue 23, Saturation 24, Color 25 and Luminosity 26 to native RGBA16 rasters and isolated groups through the
**unchanged** f32 `blend.rs` NonSeparable kernel (P7-19: preserve established legacy composites). The kernel's
`set_sat` divides two nearly equal f32 differences, so near-gray inputs can amplify **input** rounding; the slice had to
state exactly where the ordinary error bound holds.

## Decisions
- **Meaning = W3C nonseparable equations over encoded document RGB:** Hue `SetLum(SetSat(Cs, Sat(Cb)), Lum(Cb))`,
  Saturation `SetLum(SetSat(Cb, Sat(Cs)), Lum(Cb))`, Color `SetLum(Cs, Lum(Cb))`, Luminosity `SetLum(Cb, Lum(Cs))`,
  `Lum = 0.3R + 0.59G + 0.11B`, ClipColor rescales toward luminance. Implemented by `blend_kind` /
  `composite_source_into` in `core/src/blend.rs`; no pixel math added or changed at either depth.
- **Mode sets:** `precision_layer_blend::COLOR_MODES` and `NativeLayerBlending.colorModes` gained 23–26; the
  version-1 `layer_blend` request admits them on raster and isolated-group targets. No new FFI or edit schema.
- **DRDC33 / `LBLD` schema 5** adds exactly 23–26 to the prior eighteen non-default modes; schemas 1–4 frozen
  (a v26/27/30/31 file patched to carry 23–26 refuses naming the mode). Writer picks the oldest sufficient
  wrapper (33, 31, 30, 27, 26, none); capacity `16 + 8 × count`, charged by count only in
  `precision_native_plan.rs`. Wrapper order `32 → 29 → 33 → body/tails → LBLD5 → MOUT → LCLP`; 33 never wraps 29
  or 32; releasing the last HSL mode restores exact prior bytes. (`DRDC_VERSION` has since advanced; see
  `core/src/drdc.rs`.)
- **16-bit PSD carries all four both ways** via `PSD_BLEND_KEYS` (`hue `/`sat `/`colr`/`lum `), with no loss
  sentence on rasters or folders.
- **SetSat conditioning policy (the numerical contract).** For Hue/Saturation, per pixel from authored u16
  codes before the opacity/mask lerp: the rescaled triple is the source (Hue) or backdrop (Saturation); `Δ` is its
  `max − min` in codes; `s` is the other pixel's saturation `(max − min)/65535`.
  - **Ordinary bound 0.65 codes** vs unrounded f64 (quantization included) for every RGB component and alpha —
    always for Color and Luminosity, and for Hue/Saturation **iff** `Δ == 0 || s == 0 || Δ ≥ 6830·s`.
    There is no fixed spread threshold; eligibility is this formula.
  - **Why:** `set_sat` computes `(c[mid] − c[lo]) · s / (c[hi] − c[lo])`; each difference carries ≤ 2⁻²⁴ absolute
    error, giving ≈ `512·s/Δ` codes of output error. Outside the eligible region the claimed bound is
    `0.65 + 1024·s/Δ` (twice the estimate).
  - Qualified region: minimum observed `Δ/s` over eligible fixtures was 13900.0. Near-gray witnesses
    (Δ ∈ {1, 2, 16} around code 32000, s ∈ {0.25, 1}) land far inside the bound, but the **bound, not the
    measurement, is the claim** for near-gray input.

## Rejected alternatives
- An f64-intermediate SetSat — removes ≤ 0.01 codes; the error is input conditioning (worst 63.87 codes at Δ = 2,
  s = 1); see open items.
- Reusing the five-mode `[b, s, 65535−b]` grid — its near-32767 pairs produce spread-1 triples outside the
  ordinary-bound region.

## Consequences
- Any change to `set_sat` or the NonSeparable kernel changes persisted v33 evaluation and must be versioned.
- Qualified by [`core/tests/precision_hsl_blending.rs`](../core/tests/precision_hsl_blending.rs) (oracle in
  `core/tests/common/native_blend_oracle.rs`; eligible grid, every mask code, nested groups in Pass Through,
  near-gray witness, v33 downgrades, 16-bit PSD round trip), `core/tests/precision_blend_mask_wrappers.rs`,
  `scripts/verify_phase10d_blend_decoder.py` and `scripts/live-filter-probe/native.py`.
- Gated on a Release build by [`scripts/verify_phase10d_hsl_modes.py`](../scripts/verify_phase10d_hsl_modes.py)
  (gates H1–H5) and GIMP 3.2.6 receipt R9 in [`scripts/verify_phase10_receipts.py`](../scripts/verify_phase10_receipts.py)
  (keys reopen as `lch-hue`, `lch-chroma`, `lch-color`, `lch-lightness`; GIMP's own layered arithmetic differs and
  is recorded, not gated).

## Open items (verified 2026-09-27)
- `DROPPED` — won't do: an f64-intermediate SetSat. The
  f32 arithmetic in `set_sat` is bounded at ≤ 0.01 codes for any f32 input (measured 0.0086); the near-gray error is
  input conditioning, not arithmetic — the s = 1 sweep measured 0 codes at Δ = 1, 63.87 at Δ = 2 and 15 at Δ = 16,
  with f32 and f64 agreeing within 0.001 code. The real fix is an f64 or code-exact input path, plus an f64
  accumulator shared with the byte compositor for Saturation: L-sized, versioned (see Consequences), and touching
  only hue-undefined pixels.
- `DONE` — Production gate taken 2026-09-27 on Release app
  SHA-256 `7a0a0bf68d2e040b4f27296f75ebb4c396d5ef04077e7ce8998f4723ea22110f`:
  `scripts/verify_phase10d_hsl_modes.py` passed H1–H5 (minimum fixture `Δ/s` 20971; H1 maximum 0.5003 codes,
  H5 maximum 0.5 codes, both vs the 0.65 bound; every 16-bit export loss-free), result
  `~/devel/doctored-fixtures/phase10-receipts-hsl/hsl-modes/result.json`; GIMP 3.2.6 receipt R9 reopened the keys
  as `lch-hue`, `lch-chroma`, `lch-color`, `lch-lightness`, the merged preview at 0 codes and the recorded
  (ungated) layered projection at up to 19082 codes, receipt
  `~/devel/doctored-fixtures/phase10-receipts-hsl/R9/receipt.json`. No source probe is needed: the Layers popup is
  generated from `NativeLayerBlending.colorModes`, as in the threshold precedent.
- `DONE` — Hard Mix, Darker/Lighter Color, Dissolve — `dd3474f`.
- `DONE` — Blend If (`7c130e2`), layer styles (`b8aa5b5`), adjustment blending (`ea4d9d1`, P7-19); knockout P7-37 verified; P7-20/P7-21 verified.
