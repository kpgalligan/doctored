# Smart Sharpen — ADR

- **Scope:** Smart Sharpen, and Smart Sharpen as an export sharpening method, both shipped by the v1 batch of
  2026-10-13.
- **Record before the build:** [archive/smart-sharpen-status.md](../archive/smart-sharpen-status.md), the Photoshop
  26.10.0 take (`~/devel/doctored-fixtures/photoshop/smart-sharpen-65/take1`) and its verdict tables.

## Decisions in force

- **Doctored's own algorithm, stated, tested against an independent oracle.** (The Photoshop take, archived, found
  the non-legacy modes not linear, Reduce Noise a smoother and the fades with no closed form; nothing is fitted to
  it.)
- **One kernel module, every stage an existing kernel** (`core/src/sampling_smart_sharpen.rs`, a `SpatialKernel`
  arm on the one spatial route; no new export, no `.rz` change, no new MCP tool):
  1. Reduce Noise: the base `D` is `reduce_noise {radius 0.5, luminance noise/1000, detail 0, color 0}` before
     quantization (`sampling_presence::base_delta_in` + `sampling_denoise::applied_in`); noise 0 is the source.
  2. The blur to remove is a low-pass `B` of `D`: Gaussian (`neighborhood_plane_in`, σ = Radius), Lens
     (`sampling_disc::disc_plane`, the defocus disc), Motion (`sampling_motion::motion_plane`, `max(1, round(2r))`
     px at Angle).
  3. Unsharp Mask's combine (`evaluated_in`, threshold 0) of `D` against `B` at `amount/100 · w`, so Gaussian with
     noise 0 and no fade IS `unsharp_mask` at the same amount and radius, threshold 0, bit for bit.
  4. Lens Blur removal is one Jansson–van Cittert iteration with the lens PSF, projected per channel onto the
     clamped 3×3 window's visible range of `D` ± 2/255 (`LENS_OVERSHOOT`): the projection bounds the halo.
  5. Fades: `w = (1 − fs·s(Ls))·(1 − fh·h(Lh))`, `s(L) = clamp(1 − L/Ts)`, `h(L) = clamp(1 − (1 − L)/Th)`, `L` the
     alpha-weighted box mean of `D`'s Rec. 709 luma (clamped 0…1) at the section's radius (two `BoxRows`).
  Colour only, no frame, never an identity; alpha-0 pixels as read.
- **32 bits (rule S, `designs/v1-float-editing.md`):** the whole filter on the display-encoded copy, as legacy
  Sharpen and Reduce Noise; the classifier reads encoded luma clamped to 0…1; nothing is clamped. The float
  Gaussian is the FIR at every σ, so Gaussian removal at noise 0 with no fade is legacy Sharpen's float route bit
  for bit, and has no 16-bit twin (it measured 8 codes off one).
- **Export:** `export_sharpen.rs` holds `OutputSharpen` (Unsharp Mask, byte-identical to before, or Smart
  Sharpen via `"method": "smart_sharpen"` and the filter's keys); 8 bits run `ops_spatial::spatial_bytes`, 16 bits
  the plane and combine on the u16 codes, quantized as the selected edit's full-coverage write, so an export
  equals the filter applied then exported. The typed plan term is the kernel plan + 8n; the host's byte estimate
  keeps 52 B/px for an active sharpen of either method. A 32-bit export refuses it. Presets record it through
  `OutputSharpen.json`; an Unsharp Mask is still written without `method`.
- **UI:** Filter ▸ Sharpen ▸ Smart Sharpen… (`SmartSharpenSheet.swift`, ten sliders and a Remove popup, proxy
  divides the pixel radii); the export sheet's Output sharpening gains a Method popup and the Smart fields.

## Verification

- Core: `tests/filter_smart_sharpen.rs` (ORACLE against `tests/common/smart_sharpen_reference.rs` ≤ 1 code at 8
  bits and ≤ 3 at 16; USM bit equality; NOISE ≤ 4 codes; HALO H_lens 10 vs H_usm 122 levels, worst 2; REDUCE NOISE
  std 0.0053 vs 0.0264; TONES 0.56× in each section, bit-identical outside; the 24 MP byte plan pinned),
  `tests/float_filters_smart_sharpen.rs`, `tests/export_smart_sharpen.rs`, the depth-parity row.
- Drives: `scripts/verify_v1_smart_sharpen.py` (MCP, 8/16/32 bits, export and presets) and the jobs-probe case
  "smart sharpen sheet".

## Measured admission (24 MP)

Every measured call: Lens radius 4, amount 200, Reduce Noise 50, both fades 50 at width 50 and radius 8. Plan ≥
⌈1.25 × max E⌉ held at every depth, no constant raised; the per-run figures are on
`STAGE_MARGIN_BYTES_PER_PIXEL` in `sampling_smart_sharpen.rs`.

| case (`scripts/smart-sharpen-admission/measure.py --runs 3`, Release, 8 GiB) | max E | plan | plan / max E |
|---|---|---|---|
| M8, `apply_filter` 8-bit (commit stage + 192,003,794 retained) | 863,765,056 | 1,212,003,794 | 1.403 |
| M16, `apply_filter` 16-bit (`selected_edit_plan`) | 675,561,952 | 1,020,649,180 | 1.511 |
| E8, `export_image` PNG 8-bit (host 64 B/px + 16 MiB) | 771,211,768 | 1,552,777,216 | 2.013 |
| E16, `export_image` PNG 16-bit (typed `export_plan`) | 773,947,944 | 2,445,720,028 | 3.160 |

- Float (`tests/float_adjust_apply_24mp.rs::measure`, `DR_FF_LIMIT` 8 GiB, three runs, baseline E 0): max E
  1,152,664,416 against a plan of 1,514,238,611 (1.314×); the floor 1,440,830,520 is pinned.

## Open items

None.
