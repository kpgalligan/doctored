# Phase 10D: four nonseparable native layer modes (Hue, Saturation, Color, Luminosity)

The [threshold slice](phase-10d-threshold-modes-status.md) has since added Hard
Mix, Darker Color, Lighter Color and Dissolve with conditional native
v34/schema6 above the v33 wrapper described below, under its own strong/weak
band policy, and the [Blend If slice](phase-10d-blend-if-status.md) has added
typed Blend If with v35/`LBIF` outermost; schema5 and every older schema stay
frozen.

This bounded slice adds Hue23, Saturation24, Color25 and Luminosity26 for
native RGBA16 raster layers and isolated groups through the UNCHANGED f32
`blend.rs` NonSeparable kernel. Existing native modes, ordinary byte rendering,
adjustment Normal-only admission and group-only Pass Through retain their
meanings. Hard Mix21, Darker/Lighter Color16/17 and Dissolve14 remain
unsupported (their whole-pixel/threshold picks share the discontinuity question
documented in the [five-mode slice](phase-10d-five-modes-status.md)). Blend If,
styles and knockout remain outside this slice. Full 10D and P7-19/P7-20 remain
open; the carryover ledger is unchanged at 18 verified / 18 open.

## Meaning, host routing and persistence

The four modes are the W3C nonseparable equations over encoded document RGB:
Hue is `SetLum(SetSat(Cs, Sat(Cb)), Lum(Cb))`, Saturation is
`SetLum(SetSat(Cb, Sat(Cs)), Lum(Cb))`, Color is `SetLum(Cs, Lum(Cb))` and
Luminosity is `SetLum(Cb, Lum(Cs))`, with `Lum = 0.3R + 0.59G + 0.11B` and the
ClipColor rescale toward luminance. `blend_kind` / `composite_source_into`
(`core/src/blend.rs`) already evaluate `NonSeparable`; no pixel math was added
or changed on either the byte or the typed side (P7-19: "preserve established
legacy composites"). The explicit W3C equations are the native contract.

The authoritative Rust (`precision_layer_blend::COLOR_MODES`, now 23 entries)
and Swift (`NativeLayerBlending.colorModes`) mode sets feed the existing Layers
popup, MCP `set_layer_properties`, Actions and atomic captured publication.
There are no new FFI signatures or edit-request schemas: the version1
`layer_blend` request now admits `blend_mode` 23–26 on raster and
isolated-group targets. Native mask coverage, density and opacity gate
contribution once; group intermediates remain f32 until final projection
storage. Raw source RGB/alpha/hidden RGB, masks, placement, descriptions,
retained masters, channels, ICC/PPI and metadata remain exact through metadata
edits and persistence.

Conditional RZDC33/LBLD schema5 adds exactly 23/24/25/26 to the previous
eighteen nondefault modes. Schemas 1–4 remain explicitly frozen: a v26/27/30/31
file patched to carry 23–26 refuses naming the blend mode. The writer selects
the oldest sufficient blend wrapper: 33, 31, 30, 27, 26 or no blend wrapper.
Its capacity stays `16 + 8 × nondefault_record_count` bytes; the plan
(`precision_native_plan.rs`) charges by count only and was not changed.
Older typed bodies and their channel/retained/artwork tails retain exact
framing. The full wrapper order is
`32 → 29 → 33 → prior body/tails → LBLD5 → MOUT → LCLP`; 33 never wraps 29 or
32, and releasing the last HSL mode restores the prior writer's exact bytes
(v31, then v30/v27/v26/body as each further mode is released).
`RZDC_VERSION` is 33 so a byte read of a 32/33 file refuses naming the typed
reader; `isPrecisionNative` admits 33.

16-bit PSD carries the four modes in both directions through the existing
`PSD_BLEND_KEYS` table: `hue `/`sat `/`colr`/`lum ` import as the typed mode
with no loss sentence on rasters and folders, and the export writes those keys.
The `nested16.psd` fixture's `hue ` layer, which the 10C depth slice reported
as "imported as Normal", now imports as Hue with an empty `verified_losses`.

## Numerical policy

Definitions (Hue/Saturation only; per pixel, from the authored u16 codes,
before the opacity/mask lerp, because the kernel evaluates `B(Cb, Cs)` on the
layer colors): the rescaled triple is the source pixel for Hue and the backdrop
for Saturation; `Δ` is its `max − min` in codes; `s` is the target saturation,
`(max − min) / 65535` of the other pixel (`Sat(Cb)` for Hue, `Sat(Cs)` for
Saturation).

Ordinary bound: 0.65 native codes against unrounded f64 (including final u16
quantization) for every RGB component and alpha. It applies to Color and
Luminosity always, and to Hue and Saturation iff
`Δ == 0 || s == 0 || Δ ≥ 6830·s` — the region where the SetSat amplification
term `1024·s/Δ` fits inside the 0.15-code slack above the 0.5 quantization
half-step. There is NO fixed spread threshold; eligibility is this formula,
asserted per pixel by the grid test before the 0.65 bound is applied.

Why: `core/src/blend.rs` `set_sat` computes
`out[mid] = (c[mid] - c[lo]) * s / (c[hi] - c[lo])`, dividing two f32
differences whose near-equal operands each carry ≤ 2^-24 absolute rounding.
That gives ≈ 512·s/Δ codes of output error; the witness bound
`0.65 + 1024·s/Δ` (identically `0.65 + 65535·2^-22·s/(Δ/65535)`) doubles it.
This is a finite-fixture qualification of the existing kernel, not a product
change; an f64-intermediate SetSat that would remove the near-gray
amplification is phase 2 work alongside Hard Mix.

### Measured maxima (Release, `precision_hsl_blending.rs`)

Maximum error in native codes against unrounded f64, including final
quantization. Every RGB and alpha comparison in these fixtures required 0.65.

| Fixture | Hue | Saturation | Color | Luminosity |
| --- | ---: | ---: | ---: | ---: |
| Grid (23 × 23 eligible colors), opacity 1 | 0.503420 | 0.500001 | 0.503420 | 0.503420 |
| Grid, opacity 0.625 | 0.503440 | 0.500001 | 0.500001 | 0.500001 |
| Every mask code × alpha × density/opacity | 0.506918 | 0.506918 | 0.506918 | 0.506949 |
| Translated nested group in Pass Through | 0.434933 | 0.487810 | 0.319391 | 0.447474 |

Values are rounded upward. The grid uses the 8 opaque endpoint triples, 5 gray
triples (Δ = 0) and 10 asymmetric triples whose channels are pairwise ≥ 6830
codes apart; it deliberately does NOT reuse the five-mode `[b, s, 65535−b]`
pair grid, whose near-32767 pairs produce spread-1 triples outside the
ordinary-bound region.

Qualified ordinary-bound region: the minimum `Δ/s` over all Hue/Saturation
pixels in the three fixtures above with `Δ > 0` and `s > 0` is
**13900.0** (grid, both modes; mask fixture Saturation 13900.0 / Hue 21648.9;
group fixture Hue 21063.0 / Saturation 88559.6). No fixed spread threshold is
claimed; the formula above is the eligibility rule.

### Near-gray conditioning witness

Rescaled-triple spread Δ ∈ {1, 2, 16} codes around 32000, target saturation
s ∈ {0.25, 1}, opaque, both Hue and Saturation (the eligibility predicate is
false for every case). Measured maximum RGB error versus the bound
`0.65 + 1024·s/Δ`:

| Δ | s | Hue | Saturation | Bound |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 1 | 0.071 | 0.286 | 1024.65 |
| 2 | 1 | 0.334 | 0.017 | 512.65 |
| 16 | 1 | 0.334 | 0.454 | 64.65 |
| 1 | 0.25 | 0.440 | 0.100 | 256.65 |
| 2 | 0.25 | 0.280 | 0.290 | 128.65 |
| 16 | 0.25 | 0.280 | 0.040 | 16.65 |

These particular authored codes (whose f32 differences happen to be exact)
land well inside the bound; the bound, not the measurement, is the claim for
the near-gray region, because a different near-gray triple can realize the
full ≈ 512·s/Δ amplification.

## Qualification scope

`core/tests/precision_hsl_blending.rs` (independent f64 oracle in
`core/tests/common/native_blend_oracle.rs`): the eligible-color grid at opacity
1 and 0.625 with the per-pixel eligibility precondition; every native mask
code with tiny/partial/opaque alpha and density/opacity; translated nested
isolated groups in each mode inside a Pass Through container plus Pass Through
equality, rename and duplicate; exact v33 downgrade through v31/v30/v27/v26/
body with unchanged `encoded_bound` and frozen-schema refusals; strict FFI
no-ops, hidden/adjustment/pass-through targets, capacity, cancellation and
predecode refusals for every corrupted schema/count/index/mode/body word,
truncation and trailing byte; the near-gray witness; and a 16-bit PSD round
trip (one raster per mode → `hue `/`sat `/`colr`/`lum ` keys → modes 23–26,
empty `verified_losses`, projection equal to the source render) plus an
independently authored closed `sat ` folder at opacity 128 importing as an
`isolated_blend_group` with mode 24.

Updated existing tests: the five-mode and four-mode refusal lists drop 23–26;
`precision_blend_mask_wrappers.rs` adds 23–26 at version 33 to both wrapper
tests (including the v31/schema4 frozen check); `psd_depth16.rs` expects an
empty loss list and a Hue `Deep` layer. `verify_phase10d_blend_decoder.py`
adds the schema5 valid/malformed/frozen cases, four v33 mask29 compositions
and four unwrapped v33 files; `scripts/live-filter-probe/native.py` decodes
v33/schema5 independently.

### Receipts (2026-09-14, implementation run)

- `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test --release --test precision_hsl_blending`: 7 passed, 0 failed.
- The listed regression binaries (`precision_remaining_separable` 7,
  `precision_four_blending` 5, `precision_blend_mask_wrappers` 3,
  `precision_layer_blending` 12, `precision_clipping` 11, `psd_depth16` 9,
  `psd_constructs` 8, `psd_export_corpus` 1, `precision_document` 8): all pass.
- Full Release suite (`cargo test --release --no-fail-fast`): **1,563 passed,
  0 failed, 2 pre-existing ignored across 164 result groups**. One earlier
  fail-fast run tripped the timing-dependent
  `phase8e_coverage::coverage_cancellation_before_and_during_work_is_atomic_and_releases_scratch`
  ("fixture did not overlap live coverage work") under parallel load; that
  file is untouched by this slice and the binary passes alone and in the full
  rerun.
- `python3 scripts/verify_phase10d_blend_decoder.py`: seven PASS lines (schema5
  adds 50 malformed/atomic cases, 16 frozen-old-schema refusals, four v33
  mask29 compositions with 16 malformed variants and four unwrapped v33 files).
- `make typecheck`: 0 errors.
- Root-owned final gate (not yet run): `make app` plus an MCP drive
  (`set_layer_properties` blend_mode "Hue" on a 16-bit document → `render` →
  `save_copy` .rz header 33 → reopen → `export_psd` 16 → `inspect_psd` with no
  loss), `scripts/verify_phase10d_hsl_modes.py`, the
  `scripts/selection-probe/native_hsl_blending.swift` source probe and GIMP
  reopen receipts.

## Open

- Hard Mix, Darker/Lighter Color and Dissolve have since shipped in the
  [threshold slice](phase-10d-threshold-modes-status.md); an f64-intermediate
  SetSat remains open.
- Phase 3, typed Blend If, has since shipped in the
  [Blend If slice](phase-10d-blend-if-status.md), and phase 4, typed layer
  styles, in the [layer-styles slice](phase-10d-layer-styles-status.md).
  Adjustment-layer blending has since shipped ([adjustment-blending slice](phase-10d-adjustment-blending-status.md): the same twenty-six modes on a typed adjustment under the byte `composite_adjustment_into` rule, conditional v37/schema7), which verifies P7-19; knockout has no byte implementation to migrate and is tracked as P7-37 under 12E.
- P7-19 is verified by the adjustment-blending slice and P7-20 by the
  layer-styles slice; P7-21 is verified by 10C-2 slice 3 (receipts taken
  2026-09-16).
