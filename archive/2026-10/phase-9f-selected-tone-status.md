# Selected tone and scoped analysis (ADR)

- **Status:** Accepted — closed (verifies P7-06, P7-07)
- **Phase / dates:** 9F, September 2026
- **Original:** [archive/phase-9f-selected-tone-status.md](../archive/phase-9f-selected-tone-status.md) — full history, measurements, receipts
- **Key commits:** `2e7717d` (Complete selected tone and scoped analysis, phase 9F)

## Context
Destructive tone and filter commands had to run natively on 16-bit documents limited by a selection,
Auto commands needed an explicit choice of what they analyse, and histograms/readouts needed exact,
selection-weighted scopes (composite, layer, partial backdrop) — without changing old Action meanings or
byte-depth results.

## Decisions
- **Every exposed destructive filter has a native raster path** (tone/colour suite, legacy Blur/Sharpen,
  Unsharp Mask, High Pass, Reduce Noise, Dust & Scratches, Edge Detect, Emboss, Pixelate, Add Noise) over
  straight RGBA16 in the document profile. Canvas u16 coverage crosses the selected-edit ABI separately.
- **Selection mixing:** coverage 0 copies original codes exactly (hidden RGB, tiny alpha); full coverage
  takes the operator result; fractional coverage mixes in premultiplied space when alpha changes.
  Neighbourhoods read original storage outside the write region.
- **Legacy operator meanings kept:** tone/colour-only operators preserve alpha; Gaussian Blur and Pixelate
  filter alpha; Edge Detect/Emboss keep opaque gray output and hidden-RGB luminance reads; Noise keeps its
  row-major RGB sequence including transparent pixels; legacy Sharpen amount stays its historical
  Gaussian scale (not Unsharp Mask). The image-crate Gaussian runs in bounded tiles with halo; native
  legacy Blur is capped at sigma 64.
- **`analysis_scope`** (`whole_target` default — preserves old Actions — or `selection`) weights the
  histogram and neutral candidates by captured coverage. Auto Tone/Contrast/Color share endpoint,
  neutral-candidate and gamma helpers. Typed analysis uses 65,536 bins; omitted/whole-target byte analysis
  is bit-identical to before.
- **Fine entry:** Levels/Threshold have normalized and native-code fields (scale 255 or 65,535; gamma
  dimensionless); Curves keeps fractional coordinates through reopen. Stored values never change on a unit
  switch; old normalized arguments and Curves' 0–255 meaning are unchanged; new Actions keep full values.
- **Scoped statistics.** `histogram`/`sample_pixel` take `source: composite|layer|backdrop`; backdrop needs
  `below` in 0…layer_count and is a visible-contribution cut (enclosing group gates stay in effect), not an
  isolated group's internal backdrop. Adjustment entries refuse layer scope. Layer scope uses canvas
  coordinates and includes transparent padding.
- **Statistics schema 2:** `maximum_code`, storage, profile; exact integer `bin_weight_units` and
  `weighted_code_sums_rgba` alongside fractional display `bins`; `exact_zero_rgba`/`exact_max_rgba` and
  `*_weight_units_rgba`, `full_rgb_weight_units`, `full_alpha_weight_units`. RGB eligibility is alpha > 0
  (alpha is not an extra weight); sample means include hidden RGB at alpha 0. `stride` affects display bins
  only — clipping wedges use complete weighted endpoints. `histogram.selection` defaults true,
  `sample_pixel.selection` false; unscoped byte `sample_pixel` keeps its old response.
- **Capture and publish once.** Sheets and MCP capture source, target, selection and revision; preview and
  commit run the same selected command; publication rechecks the capture and goes through central history.
  Read-only source/toggle changes never dirty the document or add Undo.
- **Admission:** plans admit owners, coverage, output, scratch and parameters before computing; dense scans,
  writeback and tiles check cancellation. No document format version or adjustment meaning changed.

## Consequences
- Old Actions replay unchanged; byte filter outputs matched the prior build bit for bit.
- Native scalar targets and Quick Mask refused here; 9G later added them.
- No large-document latency, Retina layout or HDR claim is made.

## Qualification
[`core/tests/selected_adjustments.rs`](../core/tests/selected_adjustments.rs) (native ramps, exact
outside/alpha, every spatial operator, weighted Auto, cancellation) and
[`core/tests/scoped_statistics.rs`](../core/tests/scoped_statistics.rs) (fractional coverage down to
1/65535, hidden-RGB means, group backdrop cuts, exact endpoints at any stride), plus
[`scripts/verify_phase9f.py`](../scripts/verify_phase9f.py) and
[`scripts/verify_phase9f_ui.py`](../scripts/verify_phase9f_ui.py) against the production app. Selected native
results stayed within one native code of independent references; measurements are in the archive.

## Additional spatial filters
Parts A/C/B add **Box Blur**, **Surface Blur** and **Add Grain** as destructive Filters items. The open
questions were answered with the plan's recommendations: destructive first (live-stack operators later), the
Filters menu regrouped into submenus in the same change, and Radial/Zoom Blur (D), Clouds (E) and Offset (F)
deferred — Offset with the Pattern track.
- **Placement.** One `SpatialKernel` (`core/src/ops_spatial.rs`) is planned, run as a plane and combined per
  pixel; `SelectedFilter::Spatial` runs it for 16-bit layers, masks and colour/channel planes, and
  `rz_image_filter_op` (byte) runs the same three calls over straight byte samples, so a byte-origin 16-bit
  result is within one code of the 8-bit one. `rz_image_filter_op` dispatches no other op, so no existing filter
  gains a second byte path. Float documents refuse the three items as they refuse Gaussian Blur. Sheets
  (`BlurSheets.swift`, `NoiseSheets.swift`) record `apply_filter` steps, so Actions and Repeat Last Filter
  replay them; the MCP twin is `apply_filter` with flat arguments.
- **Box Blur** is the premultiplied mean of a `(2r+1)²` clamped-edge window (integer radius 1–1000; the sheet
  shows 1–250) through `sampling_box::box_sum`'s sliding passes, so its cost is independent of the radius.
  It filters alpha, as Gaussian Blur does.
- **Surface Blur** (`core/src/sampling_surface.rs`) is the exact tent filter per channel,
  `out = Σα·w(v−c)·v / Σα·w(v−c)` with `w(d) = max(0, 1 − |d|/(2.5·T))` and `d` and `T` in 8-bit levels, over a
  `(2r+1)²` clamped-edge window (radius 1–100, threshold 2–255 levels). Perreault column histograms of Σα and
  Σα·v (256 bins of u16 `code >> 8`) and a kernel histogram slid across each row; each bin is weighted at its own
  mean, which is exact at 8 bits (one level per bin) and below one level of tent slope at 16. Colour only; alpha
  is exact and alpha-0 pixels are untouched.
- **Calibration.** Work is `⌈(slide·S + evaluate·V)/1024⌉ + 4n` with `S = SURFACE_SLIDE_WORK_PER_1024`,
  `V = SURFACE_EVALUATE_WORK_PER_1024`, `slide = 3·(258·n + (2r+1)·(256·h + w))` (column and kernel histogram
  updates) and `evaluate = 3·n·E`, `E = min(256, 2·⌈2.5T⌉+2)` (tent bins in reach). Two coefficients, because a
  single constant frozen at the evaluation-heavy corner overcharged the slide-bound default T = 15 by about half
  and refused every 24 MP photo. The column histograms hold u32 Σα and Σα·(v & 255), rebuilt exactly into the
  u64 kernel bins, which halves the bytes the slide reads (r1 T2 3.40 s → 2.34 s). The evaluation is branch-free:
  every bin in reach is evaluated, an empty one adding exactly +0.0. With the old empty-bin skip, r = 5, T = 255 on
  noise took 13.8 s against 9.5 s at the r = 100 corner (a randomly half-empty window mispredicts the skip), so the
  corners did not bound the interior and the earlier single constant under-charged there; branch-free, time is
  linear in the two counts and r = 5 T = 255 is 7.7–8.1 s.
  The `#[ignore]`d probe in `core/tests/spatial_filters.rs` times byte High Pass at radius 16 (Gaussian σ16, charge
  `n·(8·97+12)`) and Surface Blur on 3000×2000 opaque noise (full-range above T = 2, mid-grey ±5 levels at T = 2),
  fits time = a·slide + b·evaluate by least squares over the four corners r∈{1,100} × T∈{2,255}, and scales both
  coefficients by the one factor that makes the charge at least the measured time at every corner and at four
  interior checks (r5 T15, r5 T255, r10 T60, r50 T255). Seven serial runs (`--test-threads=1`) on 12 cores:

  | run | load | reference σ16 | r1 T2 | r1 T255 | r100 T2 | r100 T255 | r5 T15 | r5 T255 | r10 T60 | r50 T255 | factor | slide / evaluate |
  |---|---|---|---|---|---|---|---|---|---|---|---|---|
  | 1 | 12–15 | 4.080 s | 2.671 | **12.524** | 2.978 | 8.398 | 3.556 | 7.527 | 6.733 | 7.755 | 1.206 | 709 / 2511 |
  | 2 | ≈ 12 | 3.997 s | 2.335 | 7.888 | 2.527 | 8.298 | 3.835 | 7.998 | 7.119 | 8.082 | 1.017 | 554 / 1587 |
  | 3 | ≈ 11 | 4.030 s | 2.366 | 7.919 | 2.525 | 8.148 | 3.825 | 7.671 | 6.918 | 7.985 | 1.006 | 547 / 1537 |
  | 4 | ≈ 9 | 4.068 s | 2.352 | 7.869 | 2.594 | 8.050 | 3.805 | 8.075 | 7.164 | 8.079 | 1.023 | 560 / 1520 |
  | 5 | ≈ 6 | 4.045 s | 2.358 | 7.874 | 2.505 | 8.097 | 3.851 | 8.046 | 7.164 | 7.997 | 1.016 | 547 / 1537 |
  | 6 | ≈ 6 | 4.041 s | 2.336 | 7.854 | 2.514 | 8.008 | 3.784 | 7.893 | 7.135 | 8.043 | 1.014 | 546 / 1524 |
  | 7 (min of 3) | 39 → 12 | 3.940 s | 2.324 | 7.763 | 2.501 | 8.210 | 3.674 | 7.507 | 6.620 | 7.641 | 1.019 | 560 / 1589 |

  Times in seconds. Runs 1–6 timed each point and the reference once; the probe now takes the minimum of three,
  so a spike on one timing is rejected by the probe itself, and run 7 is its first record. Run 1 is excluded as
  interference, not kernel cost, on this evidence: the kernel is deterministic and branch-free on a seeded input;
  runs 2–6 agree on r1 T255 within 1% (7.85–7.92 s) where run 1 measured 12.5 s, and run 1's two T = 2 corners were also
  +14% to +18% (r100 T255 +3%) while its interior checks were 3–7% low; and run 1's reference (4.08 s) is in line with the
  others, so the spike missed the reference and the ratio did not cancel it. Load alone is not the reason (runs
  2–3 ran at load ≈ 11–12 and agree with runs 4–6). That one point set run 1's factor and its pair (709 / 2511).
  The largest of each coefficient over runs 2–6 is 560 / 1587, at least every one of those runs' own pair; run 7
  implied 560 / 1589, 0.1% above it on the evaluate coefficient and well inside the margin below. The 560 / 1587
  pair sits only 2–6% above the 24 MP wall-clock times (r = 100 and r = 5 at T = 15), the reference itself varies
  about 2% between runs, and the 16-bit `SelectedFilter` read path, which feeds every histogram shift, is not
  calibrated (the MCP drive's 16-bit r5 T15 took 17.9 s against 16.2 s at 8 bits). So both coefficients carry an
  explicit **safety factor of 1.15** (`SURFACE_SAFETY_FACTOR` in the probe):
  **`SURFACE_SLIDE_WORK_PER_1024` = 644, `SURFACE_EVALUATE_WORK_PER_1024` = 1826**. At 6000×4000 the charge is
  2.18e10 (r = 5, T = 15) and 2.22e10 (r = 100, T = 15) of the 2.4e10 `MAX_DESTRUCTIVE_WORK`, so **24 MP is
  admitted at the default threshold at every radius**. The charge is 4.47e10 and 4.50e10 at T = 255, so 24 MP
  refuses there. The largest 3:2 layer admitted is about 26 MP at T = 15 and about 12.7 MP at T = 255. With run
  1's pair (709 / 2511), 24 MP would refuse at T = 15 as well (2.68e10).
  At 16 bits the margin is thinner, about 5–8% by the read-path estimate (a tiled plane adds a div/mod and tile
  lookup per read); a few-percent overrun only lengthens a near-cap run. Dropped: a per-pass u16 decode scratch
  that would equalize the two depths' read cost (no user would notice).
  Preview latency follows those times: the sheet recomputes the whole target per slider change, and a 6 MP layer
  took 2.3–8.4 s in the probe, so a Surface Blur preview on a large layer lagged by seconds. Done in the
  staged previews (below): a proxy shows first and a superseded exact render is cancelled.
- **Add Grain** draws one positional N(0,1) field (Box–Muller from `SplitMix64::at(seed, x, y, 0)`), blurs it
  with `sampling_gaussian::gaussian_taps(size)` separably and divides by Σk² for unit variance, mixes it with the
  white field by Roughness and renormalizes by the pair's covariance (`k[mid]²/Σk²`), so every roughness has the
  same RMS. It adds `amount·GRAIN_RMS_PER_AMOUNT·g·4Y(1−Y)` to each channel (Y Rec. 709): RMS
  `amount × 0.001` at mid-grey (`GRAIN_RMS_PER_AMOUNT` frozen at 0.001, so Amount 100 is 25.5 levels), zero at
  black and white. Colour only; amount 0 is the exact bypass. Measured in the tests: within 5% of the target at
  roughness 100 and within 10% at size 2, roughness 0.
- **Deferred:** Radial/Zoom Blur, Clouds and Offset; live-stack operators for the three filters. Grain in RAW
  develop is done: the last development stage, seeded
  from the embedded camera file's SHA-256. Folding the photographic byte twins into
  `rz_image_filter_op` is done (below).

## Additional spatial filters, Parts D/E/F — Radial Blur, Clouds / Difference Clouds, Offset
Parts D, E and F add Filters ▸ Blur ▸ **Radial Blur…**,
Filters ▸ Render ▸ **Clouds** / **Difference Clouds** and Filters ▸ Other ▸ **Offset…** as destructive 8/16-bit
filters on layers, masks, colour planes and channels, selection-scoped, recorded, ⌃F-repeatable and reachable
through `apply_filter`.
- **Placement.** Three more `SpatialKernel` arms (`core/src/sampling_radial.rs`, `render_clouds.rs`,
  `sampling_offset.rs`) on the existing route: `SelectedFilter::Spatial` at 16 bits, `ops_spatial::filter_op`
  behind `rz_image_filter_op` at 8, so there is one arithmetic at both depths and no new export. The sheets are
  `SliderSheetController` factories (`BlurSheets.swift`, `OtherFilterSheets.swift`) with the new
  `FilterSheetAccessory` controls (`FilterSheetAccessories.swift`: a choice, an integer field, the Radial Blur
  centre picker); Clouds have no dialog (`EditorViewController+Clouds.swift`).
- **The frame (the selection's bounds).** Offset shifts and wraps within, and Radial Blur's centre
  is a fraction of, the FRAME: the bounding box of the plane pixels the selection covers, in the target's own
  pixels, or the whole plane with no selection (or none of it on the plane). It is never recorded; a replayed or ⌃F
  step takes the selection and placement at run time. At 16 bits the core derives it from the u16 coverage
  (`ops_spatial::coverage_frame`, applied by `SelectedFilter::with_frame`, which always replaces a parsed region),
  following the layer or mask offset; the 16-bit plans pass no frame, and the Radial work bound holds for any
  centre inside any frame. At 8 bits the host adds `region_x/y/width/height` from `OperationCapture`
  (`FilterFrame.swift`: the bbox of the nonzero selection bytes inside the target's rect), scaled outward to a
  proxy. The byte and u16 frames agree except where an anti-aliased edge sample quantizes to byte 0. The picker
  draws the canvas composite mapped so the frame (the selection's bounds ∩ the target's canvas extent) fills it.
- **Radial Blur.** The equal-weight premultiplied mean along each pixel's path about the centre: Spin over an arc
  of `amount` degrees (`SPIN_RADIANS_PER_AMOUNT` = π/180), Zoom over a ray of `amount × 0.5%` of the radius
  (`ZOOM_SPAN_PER_AMOUNT` = 0.005), both symmetric about the pixel. `N = 1 + min(cap, ⌈length · density⌉)`: Good
  bilinear at one sample per pixel of path (cap 64), Best at two (cap 128), Draft nearest at a quarter (cap 16)
  with stratified jitter `t_k = (k + u_k)/N` from `SplitMix64::at(RADIAL_DRAFT_SEED, x, y, k)` — grainy,
  but deterministic, so a replay is identical. Good and Best take each N's steps from a table built once (no
  trig per sample), and the bilinear body runs in f32 (25.4 s → 21.6 s at 24 MP Spin Good); the path mean is
  summed in binary64. Box Blur's combine and its half-16-bit-code alpha floor: alpha is filtered. Pixels outside
  the frame hold their premultiplied source.
- **Clouds.** Eight octaves of smoothstep-interpolated value noise on lattices of period 256/2^o pixels, lattice
  values from `SplitMix64::at(seed, ix, iy, o)`, weighted 0.5^o and normalized, then `clamp(0.5 + g(t − 0.5))`
  with g = 2 (`CLOUD_GAIN`) or 4 for high contrast (⌥). The mix is `fg + (bg − fg)·t` on unit bytes with no gamma
  conversion. Clouds writes `[mix, 1]`; Difference Clouds writes `|source − mix|` on colour, keeps alpha and leaves
  alpha-0 pixels untouched. The sheetless menu items use the swatches and a fresh seed; the recorded step carries
  the seed and both colours (`apply_filter`'s defaults are #000000 / #ffffff, not the swatches).
- **Offset.** `out(x, y) = src(x − h, y − v)` inside the frame when that source is inside it; otherwise Repeat
  clamps to the frame's edge, Wrap is `rem_euclid` over the frame, Transparent is `[0,0,0,0]`, Fill is
  `[f,f,f,1]`. Samples move exactly (codes round-trip at both depths); offsets are bounded only by ±`MAX_PIXELS`;
  both 0 is the identity. **Set to Background** is resolved by the host (`FilterFrame.kernelParameters`, the one
  translation every caller uses): on a LAYER it leaves the vacated area transparent (Rasterize has no Background
  layer kind), and the sheet calls it Set to Transparent; on a mask it fills white; on a red/green/blue plane
  that component of the background colour; on the alpha plane or a saved channel the colour's Rec. 709 gray
  (read back through the core, `blend.rs`'s `LUMA_*`). The scalar route writes one gray, so one `fill` value
  per target is what the kernel takes.
- **Calibration** (`radial_clouds_offset_calibration_probe` in `core/tests/radial_clouds_offset.rs`, the
  `surface_blur_calibration_probe` method: units per second from the Gaussian σ3.9 reference converted to the r16
  unit, 6000×4000 opaque noise, min of 3, the 8-bit `rz_image_filter_op` and the whole 16-bit `selected_edit`;
  each constant is the larger depth's units per planned sample or pixel, the largest over two runs, × 1.15;
  Apple M2 Max, release). Run 1 (reference 1.546 s, 1.1507e9 units/s) ran with the radial constants at 1, so
  nothing refused; run 2 (1.543 s, 1.1528e9) is the record at the first frozen values, where 24 MP Good and Best
  are refused before they run, so it re-checks them at 2400×1600:

  | Case | run 1: 8-bit / 16-bit | units | run 2: 8-bit / 16-bit | units | count |
  |---|---|---|---|---|---|
  | Spin Good a10 | 21.178 s / 42.737 s | 15.62 / 31.52 | 2400×1600: 3.137 s / 5.620 s | 14.49 / 25.96 | 1.5600e9 (2.4960e8) samples |
  | Zoom Good a10 | 18.789 s / 33.007 s | 13.86 / 24.35 | refused | — | 1.5600e9 |
  | Spin Best a10 | 40.063 s / 75.838 s | 14.89 / 28.19 | 2400×1600: 6.006 s / 10.727 s | 13.98 / 24.97 | 3.0960e9 (4.9536e8) |
  | Spin Draft a10 | 6.076 s / 9.441 s | 17.14 / 26.63 | 6.083 s / 10.122 s | 17.19 / 28.60 | 4.0800e8 |
  | Zoom Draft a100 | 3.555 s / 8.202 s | 10.03 / 23.13 | 3.631 s / 8.602 s | 10.26 / 24.31 | 4.0800e8 |
  | Clouds | 1.178 s / 1.499 s | 56.48 / 71.85 | 1.173 s / 1.506 s | 56.36 / 72.33 | 2.4e7 pixels |
  | Difference Clouds, high contrast | 1.197 s / 1.530 s | 57.37 / 73.35 | 1.207 s / 1.543 s | 57.98 / 74.12 | 2.4e7 |
  | Offset wrap 1234, −567 | 0.219 s / 0.612 s | 10.49 / 29.36 | 0.221 s / 0.614 s | 10.60 / 29.50 | 2.4e7 |
  | Offset repeat 1234, −567 | 0.218 s / 0.612 s | 10.47 / 29.33 | 0.219 s / 0.612 s | 10.52 / 29.38 | 2.4e7 |

  **`RADIAL_BILINEAR_WORK_PER_SAMPLE` = 37** (31.52 × 1.15), **`RADIAL_NEAREST_WORK_PER_SAMPLE` = 33** (28.60),
  **`CLOUDS_WORK_PER_PIXEL` = 86** (74.12), **`OFFSET_WORK_PER_PIXEL` = 34** (29.50). Run 1 alone had given 31 and
  85 for nearest and Clouds; run 2's 16-bit rows came in up to 7% higher, so the largest over the two is frozen,
  as Surface Blur's coefficients were. The 16-bit tiled read sets
  every constant; one constant serves both depths, so the 8-bit run is charged about twice its time. At 24 MP:
  Clouds 2.06e9 and Offset 8.16e8 of the 2.4e10 `MAX_DESTRUCTIVE_WORK` (admitted up to the 100 MP pixel cap);
  Radial Draft even at Amount 100 1.35e10 (admitted; a whole 45 MP plane at Draft, 2.55e10, is refused — the
  plan counts every tile at its farthest frame corner, so Draft saturates at N = 17 at any Amount); **Radial Good at Amount 10 5.77e10 and Best 1.15e11 —
  refused** for a whole 24 MP plane (Good fits about 10 MP, Best about 5 MP). Under a selection the 8-bit charge
  is the frame's own tiles (the host's `region_*`), so the drive admitted Good at Amount 10 inside a 2000×1333
  selection of a 24 MP layer (2.7 s) and a 2752×1834 one of a 45 MP layer (5.7 s); the 16-bit plan is
  frame-unaware by design (an upper bound for any frame) and refused both. A first build at 44.8 s / 65.6 s for Spin Good (trig per
  sample, binary64 bilinear) was sped up as above before freezing; the refusal stands as a maintainer decision.
- **Memory gate** (scratch probe, one case per process, `MallocLargeCache=0`, pre = `ri_phys_footprint` after
  `proc_reset_footprint_interval`, E = `ri_interval_max_phys_footprint` − pre, 3 runs + a baseline twin at E 0
  ×1 per case; opaque splitmix noise; Radial at Draft Amount 1, which has Good's memory. The rows ran at the first
  frozen nearest constant, 31, which admitted the whole 45 MP plane (2.395e10); at the final 33 that plane is
  refused before it allocates, so the 45 MP Radial rows record the kernel's memory, the same as the other three
  ops'):

  | Case | E (3 runs, every op) | Plan | Plan / max E |
  |---|---|---|---|
  | byte 6000×4000, all four ops | 480,264,528 – 480,280,912 | `filter_op_scratch_bytes` 605,242,880 | 1.2602 |
  | byte 8256×5504, all four ops | 909,279,880 – 909,296,264 | 1,141,268,480 | 1.2551 |
  | 16-bit 6000×4000, all four ops | 769,262,120 – 769,311,272 | `selected_edit_plan` 822,291,456 | 1.0689 |
  | 16-bit 8256×5504, all four ops | 1,456,309,256 – 1,456,391,176 | 1,551,286,272 | 1.0652 |

  The byte E is the n×16 plane beside the n×4 output (20n + < 0.3 MB), as for the other `filter_op` filters. The
  16-bit E is a **structural count**: the n×16 plane, the n×8 output codes and the n×8 tiled copy
  `rebuilt_rgba16` makes while they are held, 32n = 768,000,000 at 24 MP (E − 32n ≤ 1,311,272) and 1,454,112,768
  at 45 MP (≤ 2,278,408), against the plan's 34n + 6 BK, so the 16-bit gate is the ≥ 1.0× of the batch 162 precedent
  (the Box Blur G3 row's 1.080×). No plan term was raised.
  The Release app at `RZ_WORKING_LIMIT_BYTES` 8 GiB (a fresh instance per run, `open_document` then the one
  `apply_filter`; baseline twin `get_document` 0 – 212,992): 8-bit 6000×4000 E 861,176,456 – 865,845,896 and
  8256×5504 1,633,453,256 – 1,639,744,712 (Radial Draft the largest), the commit and first projection's peak as for
  every 8-bit filter (Box Blur's G2 1,633,141,912), which the host charges as its own stage
  (`ByteFilterAdmission.commitStageBytes`); 16-bit 6000×4000 764,953,128 – 766,640,704 and 8256×5504 1,451,656,200 –
  1,455,293,472, below the in-process rows. On these noise fixtures the 45 MP 8-bit E is up to 6.6 MB above
  `ProjectionMargin`'s C2 photo row, which puts it at 1.247× of the recorded 2,044,852,610 publication charge
  (Clouds 1.2489×, Offset 1.2492×); the commit-stage margin is outside this change. Resolved by batch 168
  (outcome C, nothing changed). Under batch 165's
  harness, noise and photo45 peak within 82 KB of a flat gray image, and Clouds and Offset on noise read
  1,633,404,104 and 1,633,240,240 (1.252×). Clouds with batch 167's extra argv read 1,633,502,432. So the excess
  above is not the content, and the argv does not reproduce it either. Radial Draft at 45 MP is refused by the final work limit
  and has no row. Every 45 MP 8-bit row is ≥ 1.251× at the unchanged 6/16 (`designs/working-limit-ram-scaled.md`,
  Batch 168).
- **Preview latency at 24 MP.** The exact preview runs the Apply's kernel (the table above: Draft 6.1 s / 9.4 s,
  Clouds 1.2 s / 1.5 s, Offset 0.22 s / 0.61 s at 8 / 16 bits), and a Good or Best Radial preview of a 24 MP layer
  is refused with the work sentence on the sheet's status line. Radial Blur and Offset preview a ≤ 262,144-pixel
  proxy first (the amount and centre are scale-free, the offsets divide by the factor). At a 24 MP layer's
  627×418 proxy a whole process (startup, input, kernel) took Radial Good 0.14 s / 0.22 s, Best 0.24 s / 0.41 s,
  Draft 0.04 s / 0.06 s and Offset ≤ 0.01 s at 8 / 16 bits, so the proxy lands at once and a refused exact stage
  leaves it on screen with the sentence.
- **Drive** (own Release instance, port 5167, `RZ_BACKGROUND=1`, 8 GiB, noise PNG fixtures at 6000×4000 and
  8256×5504, 8 and 16 bits; no `[AutomationAlerts]` line). Every filter on the layer and on a mask (made from an
  ellipse) under a rect selection changed what it covers, left four outside probes identical, rendered, and undid
  to the original — except 16-bit Radial Good at Amount 10, refused by the work limit at both sizes. On a 400×300
  grid picture: Offset wrap inside a selection wraps within its bounds and the inverse wrap restores it; Radial
  Best at (0.5, 0.5) under an off-centre selection leaves a probe near the selection's centre unchanged where a
  probe near its corner changes, also on a layer moved by (−37, 23); Offset with Set to Background on the Red
  plane with #ff0000 writes 255; a recorded Clouds Action replays twice to identical pixels. ⌃F has no MCP twin,
  so its fresh seed is on the hand-check list.
- **Maintainer decisions.**
  - Set to Background is built for masks (white), colour planes (that component) and the alpha plane and
    channels (Rec. 709 gray); layers get Set to Transparent, since there is no Background layer kind.
  - Offset and the Radial centre follow the selection's bounds.
  - Draft's jitter is built, deterministic.
  - Clouds are not seamlessly tileable (tile-aligned Clouds is not built).
  - No 32-bit float (an open cut): a float document treats the four as it treats Box Blur, Surface Blur and Add
    Grain.
  - The centre picker shows the image, not a wireframe.
  - A 16-bit mask's picker frame falls back to the layer's rect (the facade does not expose the mask's storage).
  - `apply_filter`'s colour defaults are #000000 / #ffffff, not the swatches.
  - ⌃F on Clouds and Difference Clouds draws and records a new seed; an Action keeps its recorded one.
  - The constants were not compared with Photoshop renders.
  - 24 MP Radial Blur at Good or Best is refused by the destructive work limit (above).
  - The sheets (Radial Blur's picker and choices, Offset's fields) need the orchestrator's on-screen hand check;
    no `*_ui.py` drive was run.

## Staged, cancellable slider previews (2026-10-02)

The heavy slider-filter sheets show a downscaled
**proxy** first and the exact full-resolution preview after it, and every superseded exact render is
**cancelled**, at 8 and 16 bits. Apply and MCP `apply_filter` are unchanged and byte-identical.

- **The `rz_image_filter_op` fold, first.** `rz_image_filter_op` dispatches seven ops: `box_blur`, `surface_blur`, `add_grain`,
  `unsharp_mask`, `high_pass`, `dust_and_scratches` and `reduce_noise`. `rz_image_unsharp_mask`,
  `rz_image_high_pass`, `rz_image_dust_and_scratches`, `rz_image_reduce_noise`, their byte functions and Swift
  wrappers are deleted end to end. Parameters go through `SelectedFilter::parse`, so the keys, ranges and
  defaults are the 16-bit selected filter's (unsharp_mask `radius` 1.5, `amount` 1, `threshold` 0; high_pass
  `radius` 3; dust_and_scratches `radius` 2, `threshold` 0; reduce_noise `radius` 1, `luminance` 0.01,
  `detail` 0.5, `color` 0.5); `SelectedFilter::Photo` answers its `bypass` as `Ok(None)` and otherwise runs
  `ops_photographic::apply`. Gaussian Blur keeps `rz_image_blur`. The output is byte-identical because the
  `Neighborhood`/`FilterKind` values, the `/255` read, the `u8::quantize(v·255)` write and the plan-before-
  allocate step are the deleted functions' own, and doubles cross JSON exactly (`JSONSerialization` writes
  round-tripping doubles; serde_json has `float_roundtrip`). A temporary test proved it before the exports were
  deleted: 2,820 cases over the sharpen and cleanup plates (adversarial alpha), a grid covering every bypass,
  the defaults and out-of-range refusals, bytes equal and NULL matched to NULL. Only refusal wording changed
  (it now comes from `parse` and still names the parameter); a non-finite value can no longer be expressed.
- **Both mechanisms.** Cancellation alone would still pay the full kernel for the first update after a change
  (24 MP Surface Blur ≈ 17 s); a proxy alone would leave a superseded multi-second exact holding the serial
  preview queue. At 16 bits the exact stage was already cancelled (`PrecisionAdjustmentSession.requestEdit`
  cancels its control; the selected kernels poll). At 8 bits `rz_image_filter_op` gains an optional
  single-use `control` (`RzJobControl`/`JobContext`, the DocumentJobs mechanism): each exact preview gets a
  fresh `RasterJobControl` and the next request cancels it. Claimed before the op and parameters are
  validated; a cancel answers "operation cancelled"; controlled output equals uncontrolled
  (`core/tests/filter_preview_cancel.rs`, parked mid-run cancels). 8-bit Gaussian (`rz_image_blur`, one opaque
  `image::imageops::blur`) stays uncancellable; its proxy runs, and a cancelled exact skips the flatten
  (`OperationCapture.preview(control:)`).
- **Proxy scale.** Only above 4,194,304 px: `k = ceil(sqrt(W·H / 262,144))`, the proxy `W/k × H/k`, the factor
  `W / proxyWidth` (6000×4000: k = 10, 600×400). The document is reduced with `rz_doc_resize` (bilinear) and the
  selection with the same plane kernel through the new `rz_selection_resize`. A drawn image narrower than the
  canvas is drawn with `.high` interpolation (`ImageCanvasView.interpolation(for:)`), so a proxy is never shown
  as k×k blocks at 100% and above.
- **Parameters.** Radius-like values are divided by the factor, amounts and thresholds kept: Gaussian, Unsharp
  Mask, High Pass, Reduce Noise and Dust & Scratches `r/f` (a Dust & Scratches proxy radius below 1 is the
  core's exact bypass, so that proxy shows the picture unfiltered); Box and Surface Blur `max(1, round(r/f))`.
- **Which sheets.** Gaussian, Box, Surface, Unsharp Mask, High Pass, Reduce Noise and Dust & Scratches get a
  proxy; Add Grain, Add Noise, Pixelate, Hue Rotate, Threshold and Posterize do not (cheap, and per-pixel grain
  or noise would show k× too coarse). Every `SliderSheetController` sheet gets the generation filter. No proxy
  at 32-bit float, on 16-bit scalar targets, or in Quick Mask; tone sheets are out of scope.
- **Ordering** (`StagedPreview`, `ProxyPreview.swift`). Each request bumps a generation and older deliveries are
  dropped; the proxy runs first and that generation's exact is queued only once its proxy has landed, so a
  drag runs proxies only; a closure the queue reaches after its generation was superseded returns at once. A
  nil proxy is not shown and its exact still runs; a proxy landing after its exact is dropped.
- **Proxy sources** are built once per sheet off main (`ProxySourceLoader`, measured 75–88 ms for 24 MP at
  8 bits). A request made before the build finishes queues its exact at once, and the proxy for the current
  generation runs when the source lands (this is how the opening preview gets one).
- **Color Samplers** read "—" while a proxy is shown (the readers require the canvas size, or at 16 bits the
  projection); the exact restores them.
- **Typed geometry previews are not rolled in.** They come from `PrecisionGeometryGesture`'s native work and are
  drawn through the crop/transform session's own window or matrix, not a sheet's `previewImage`; a proxy there
  means mapping matrices, crop windows and sampler support into proxy space, a separate design.

**Measured** (`scripts/jobs-probe` "slider preview latency", release core, one fresh document per sheet, a
warm-up sheet first — the first sheet a process presents takes ~1 s longer to appear; ms from the slider
change, or from `presentAsSheet` for the opening; first-visible / exact; four changes are 100 ms apart and
report the final exact):

| Layer | Filter | Opening before → after | One change before → after | Four changes before → after |
|---|---|---|---|---|
| 24 MP 8-bit | Surface Blur r5 T15 → r10 | 16,973 / 16,973 → **533** / 16,643 | 17,158 / 17,158 → **214** / 17,098 | 15,606 / 31,258 → **576** / **17,627** |
| 24 MP 8-bit | Gaussian σ2 → σ12 | 883 / 883 → **371** / 900 | 2,096 / 2,096 → **67** / 2,082 | 1,536 / 3,445 → **69** / **2,352** |
| 24 MP 16-bit | Surface Blur | refused → refused (working memory) | refused → refused | refused → refused |
| 24 MP 16-bit | Gaussian | refused → refused (working memory; σ12 work limit) | refused → refused | refused → refused |
| 12 MP 16-bit | Surface Blur | 8,710 / 8,710 → **553** / 8,741 | 8,756 / 8,756 → **249** / 8,905 | 9,096 / 9,096 → **252** / 9,240 |
| 12 MP 16-bit | Gaussian | 1,019 / 1,019 → **376** / 999 | refused → refused (σ12 work limit) | none / refused → **82** (σ9 proxy) / refused |

On 24 MP at 8 bits the first visible update is at or below 0.6 s for the opening and every change, and four
quick changes now cost one exact render instead of two (Surface Blur 31.3 s → 17.6 s). **At 16 bits a 24 MP
preview is refused before and after** — the Surface Blur and Gaussian previews by the 2 GiB working limit
("This job needs more working memory than the current budget"), σ12 by the legacy Gaussian's work limit (the
16-bit `blur` refuses above σ6 at 24 MP and σ9.5 at 12 MP, Apply included) — and a refused exact stages no
proxy, so the 16-bit sheets were also measured on 4000 × 3000, still above the proxy threshold. The probe's
assertion that every exact lands therefore fails on those refused cells, before and after alike. The MCP drive
(`apply_filter` on `host_stills_opaque/source8.png` and `source16.png`, blur, box_blur, surface_blur,
unsharp_mask, high_pass, reduce_noise and dust_and_scratches, explicit params and defaults, with and without a
`select_rect`) produced the same 56 export digests before and after.

## 24 MP 16-bit previews, Part A1 — banded 16-bit Surface Blur / legacy Gaussian (2026-10-03)

**Decision.** The 16-bit layer path of a selected edit builds the legacy Gaussian's and Surface Blur's f32
plane one `BAND_ROWS` (128-row) band at a time instead of whole (n×16 B), with output bit-identical to the
whole plane (`selected_filter::plane_band` / `band_scratch`, `sampling_surface::surface_band`; a Surface Blur
band re-seeds its column histograms at its first row). The plan charges one band plus the n×8 tiled copy of
the output codes `rebuilt_rgba16` makes, measured as a missing term. The scalar mask/channel path and the byte
`filter_op` keep the whole plane. **24 MP 16-bit previews are still refused after A1:** the remaining terms
are on the Swift side.

**Terms** (6000 × 4000 RGBA16, Gaussian sheet open, measured in the probe process):

| Term | Bytes |
|---|---|
| Working limit | 2,147,483,648 (2 GiB) |
| Retained with the sheet open (`unique_retained_bytes`: source 192 MB + projections, display, history, proxy, payloads) | 974,101,830 |
| Display estimate (`projectionScratchEstimate()`, render scratch + n×8) | 769,048,576 |
| Coverage lease (`PrecisionEditPayload.admitted(n×8)`) | 192,000,000 |
| Core plan, Gaussian σ2: HEAD → banded | 636,703,744 → 456,991,744 |
| Core plan, Surface Blur r10 T15: HEAD → banded | 644,680,704 → 464,968,704 |
| **Total (Gaussian σ2), banded** | **2,392,142,150, 244.7 MB over** (HEAD 2,571,854,150) |

With a core plan of 0 the preview would fit by only 212 MB, which is below the measured footprint, so no honest
core plan fits it. What still has to shrink is the display estimate held while a sheet is open, the retained
set, and the coverage lease when there is no selection.

**Footprint** (3 runs, one case per process, E = `/usr/bin/time -l` peak − pre-edit
`ri_phys_footprint`, opaque, no coverage):

| Case | Max E | Banded plan | Plan / E |
|---|---|---|---|
| Surface Blur r10 T15 | 415,973,712 | 464,968,704 | 1.118× |
| Gaussian σ2 | 402,604,344 | 456,991,744 | 1.135× |

Without the n×8 term the plans (273 MB / 265 MB) were below E. The whole-plane path makes the same uncharged
n×8 copy; it is left for the next part.

**Apply** (the probe's new Apply stage, pressed as soon as the sheet is visible; before → after A1):

| Layer | Surface Blur r5 T15 | Gaussian σ2 |
|---|---|---|
| 24 MP 16-bit | committed 17,929 → 16,992 ms | committed 1,673 → 1,731 ms |
| 12 MP 16-bit | committed 9,034 → 8,653 ms | committed 970 → 965 ms |

The 24 MP Apply is admitted only when it is pressed straight after the sheet opens. Pressed 3 s later, the
24 MP Surface Blur Apply was refused by the core admit ("precision operation needs 2378103782 bytes including
retained inputs; working limit is 2147483648 bytes"), because ~1.72 GB besides the plan was retained by then.
The 24 MP 16-bit preview rows in the table above remain refused → refused, and the probe FAILs them.

## 24 MP 16-bit previews, Part A2 — honest Swift-side charges (2026-10-03)

**Outcome.** A 6000 × 4000 16-bit layer now gets the Surface Blur (r5 T15) and Gaussian σ2 sheet previews
(proxy, then exact) and an Apply that commits whether it is pressed at once or 3 s after the sheet opens.
σ12 stays refused by the legacy Gaussian's work cap (24 MP ceilings
Part D).

**Attribution** (step 1: a temporary diagnostic, never committed, printed `RasterResources.report()` and
`liveHandleSummary()` at each moment; 6000 × 4000 RGBA16; HEAD 2f93245). A1's "retained with the sheet
open 974,101,830" was reproduced as 966,010,082 at the 24 MP 16-bit Surface Blur one-change request, with
the probe's full row order:

| Owner | Bytes | After A2 |
|---|---|---|
| Document: the source layer | 192,000,000 | live, unchanged |
| Document: the retained u16 projection | 192,000,000 | live, unchanged |
| Document: the 8-bit display image | 96,000,000 | live, unchanged |
| Sheet: coverage lease (`requestEdit`, n×8, charged with no selection) | 192,000,000 | **0** (decision 3) |
| Sheet: proxy source (600 × 400 typed document) | 1,920,000 | live, unchanged |
| Sheet: capture and request payloads | 2,125,322 | live, unchanged |
| Probe: the closed 8-bit Gaussian row's document, not yet freed by AppKit (layer and proxy layer 96,960,000; projection and display images 192,960,000) | 289,920,000 | **0**: the probe waits after each close |
| Thumbnails, profiles, structures | 44,760 | live |

The timing-dependent refusal reproduces to the byte. "precision operation needs 2,378,103,782 bytes" is the
Surface Blur r5 plan 464,968,704, plus the sheet's own superseded exact preview reservation 1,234,017,280
(its scratch 464,968,704 + display estimate 769,048,576), plus a retained set of 679,117,798. That
reservation was still held by the in-flight closure that `cancel()` had only cancelled. The retained set is
the source and projection 384,000,000, two proxy sources 3,840,000, the display image 96,000,000, the
coverage lease 192,000,000, payloads 3,187,656 and 90,142 of thumbnails and structures. So the "~1.72 GB
besides the plan" was 1,913,135,078, all of it the sheet's own work or the document. Whether Apply won the
race depended only on how quickly the cancelled exact reached its next poll.

At the **sampler moment** (one Color Sampler placed, Gaussian σ2, Apply 3 s after the sheet is visible),
the exact has landed by the press. The sheet holds its projection (192,000,000, in `previewFailure` and
`canvas.previewPrecisionSource`) and the preview's display image (96,000,000). HEAD's `cancel()` released
only the image, so the 192 MB projection was charged against the Apply. `drainForCommit()` releases all
288,003,112 B before admission (measured retained 1,168,221,099 at the press, 880,217,987 at admission).
No other sheet-owned term exceeds 16 MiB.

One more term appears only in the probe. Every dismissed sheet stays in the editor's
`presentedViewControllers`, because the probe process runs with the prohibited activation policy and an
explicit dismiss raises. Its session's capture therefore keeps the pre-Apply source. With history in place
that source is shared, so it costs nothing. Once the probe clears the history between its two Applies, it
leaves 196,052,368 B unshared: the source 192,000,000, its proxy source and payloads. So the second Apply
is measured with that much more retained than the first, a stricter case.

**Decisions** (24 MP 16-bit; before → after):

1. **Core: the retile copy where it is live.** `selected_edit_plan` charges the n×8 tiled copy
   `rebuilt_rgba16` makes as an additive term on the tone, auto and banded arms, whose state is still held
   when the copy is made. On the whole-plane arm it is max()'d against the kernel's scratch: the kernel's
   transient buffers are freed when `plane()` returns, so the arm charges max(kernel scratch, held plane +
   copy), the held plane being n×16 for a filter with a plane and 0 for `add_noise`, `levels_channels` and an
   identity. The banded totals are unchanged (Surface Blur r5 464,968,704; Gaussian σ2 456,991,744). The
   plan JSON gains `output_bytes`, n×8 = 192,000,000.
2. **The preview reservation is the max of its sequential phases.** A preview worker computes, then
   prepares its display; by then the plan's scratch has been released, but the result (`output_bytes`) is
   still live. `PrecisionCapturedEdit.previewReservationBytes(display:)` is therefore
   max(scratch, output + display) for a selected edit, and the sum for every other operation. Exact:
   Surface Blur 1,234,017,280 → 961,048,576; Gaussian 1,226,040,320 → 961,048,576. The 600 × 400 proxy:
   23,898,880 → 15,170,304 (Surface Blur), 26,591,360 → 17,862,784 (Gaussian).
3. **Coverage lease only for an insertion's mask.** The `PrecisionEditPayload` lease and the
   `nativeSelection` capture exist only for `.create` with a selection, at n×2 (48,000,000; it was n×8 for
   every mode). A destructive edit's coverage is its own `PrecisionScalarBuffer` payload (n×2 + 512,
   `SelectedAdjustment.edit`), and the core plan already charges canvas×2 for it. `commit(op:)` adds the
   same n×2 or 0 to its scratch, instead of n×8.
4. **Supersession.** `StagedPreview.request(proxy:makeExact:)` queues a new request's proxy at once.
   The exact's plan, coverage lease and reservation run on main only once that proxy has been delivered
   and the exact renderer is idle. So two exact reservations never coexist: at HEAD a superseded exact kept
   its 1.23 GB until its closure returned, under the newer one's admission. `supersede()` now also drops the
   renderers' not-yet-run closures, releasing their reservations. The display stage polls its own
   `RasterJobControl`. The next request and `cancel()` cancel it with the compute's, and a cancelled
   display records no failure.
5. **Apply drains the sheet first.** `commit(filter:)` and `commit(op:)` call `drainForCommit()`, which
   cancels, waits for the exact and proxy renderers (`queue.sync {}`) and the proxy source build, and drops
   the sampler projection. The 3 s Apply is then admitted at 683,103,283 retained with no reservation,
   instead of racing the 1,234,017,280 one.

Every caller of the session gets the deferral and the drain: the Adjust, Gaussian, slider and adjustment
layer sheets, `AdjustmentSheet`, Curves, Levels, the menu and MCP `apply_filter` at 16 bits, and the
precision paint path. `.create` and `.edit` keep `outputEstimate == nil`, so their reservation is
unchanged. `ByteFilterPreview` still uses `request(proxy:exact:)`, so 8-bit behaviour is unchanged.

**Probe rows** ("slider preview latency", ms first-visible / exact; the final run):

| Layer | Filter | Opening | One change | Four changes | Apply (immediate) | Apply (3 s) |
|---|---|---|---|---|---|---|
| 24 MP 16-bit | Surface Blur r5 → r10 | refused → **537** / **18,325** | refused → **235** / **18,979** | refused → **617** / **19,348** | 18,264 | refused or committed by race → **18,234** |
| 24 MP 16-bit | Gaussian σ2 → σ12 | refused → **388** / **1,728** | none → **77** / refused (σ12) | **88** / refused (σ12) | 1,675 | **1,687** |
| 12 MP 16-bit | Surface Blur | 526 / 9,157 | 237 / 9,342 | 616 / 9,717 | 9,174 | 9,176 |
| 12 MP 16-bit | Gaussian | 378 / 998 | 79 / refused (σ12) | 104 / refused (σ12) | 1,034 | 984 |
| 24 MP 8-bit | Surface Blur | 537 / 17,079 | 216 / 17,932 | 578 / 18,183 | – | – |
| 24 MP 8-bit | Gaussian | 373 / 895 | 68 / 2,166 | 70 / 2,436 | – | – |

At HEAD with the probe's full row order, the 24 MP 16-bit previews were refused by the working limit. A
σ12 change at HEAD showed no proxy, because a refused request staged nothing; now the proxy shows, then the
exact is refused. The final FAIL line names only the σ12 stages (24 MP ceilings Part D). Both the 24 MP and 12 MP
rows give the second Apply the dismissed first sheet's source, as described above.

**12 MP 16-bit rows, HEAD against A2** (same session, same probe file, two runs each; HEAD is the HEAD
app sources with this core, whose banded plans are unchanged). The rows are no worse:

| Stage | HEAD | A2 |
|---|---|---|
| Surface Blur opening | 549 / 9,137; 575 / 9,230 | 541 / 9,070; 542 / 9,128 |
| Surface Blur one change | 237 / 9,382; 239 / 9,295 | 238 / 9,329; 238 / 9,392 |
| Surface Blur four changes | 631 / 10,042; 624 / 9,999 | 636 / 9,868; 622 / 10,180 |
| Gaussian opening | 379 / 1,013; 385 / 1,019 | 379 / 999; 380 / 1,021 |
| Gaussian one change (σ12) | none / refused (both runs) | 77 / refused; 80 / refused |
| Gaussian four changes (σ12) | 91; 87 / refused | 98; 94 / refused |
| Surface Blur Apply immediate / 3 s | 9,195 / 9,227; 9,133 / 9,222 | 9,112 / 9,167; 9,318 / 9,501 |
| Gaussian Apply immediate / 3 s | 1,002 / 979; 1,009 / 975 | 1,054 / 969; 986 / 987 |

The 12 MP four-change first-visible is about 630 ms at HEAD too (the staged-previews table's 252 ms predates A1). The
first proxy of the burst is always superseded before it lands, both before and after this change.

**Footprints.**

- **Preview peak (step 8b).** `phys_footprint` was sampled every 5 ms from just before a 24 MP 16-bit
  exact preview's request (no proxy) to its full-size delivery. The throwaway driver was a jobs-probe case
  in the scratchpad. Peak − pre-request stays within the reservation plus leases:

  | Case | Growth | Reservation + leases |
  |---|---|---|
  | Gaussian σ2 | 755,581,504; 756,236,888 | 961,048,576 + 600 |
  | Surface Blur r5 | 768,115,288; 767,328,808 | 961,048,576 + 648 |

- **Edit footprint (step 8c).** E is peak − pre, over 3 runs, one case per process,
  opaque, no coverage. The plan stays ≥ max E:

  | Case | Max E | Plan after (before) | Plan / E |
  |---|---|---|---|
  | `box_blur` r10 | 1,537,934,440 | 1,592,388,608 (1,592,388,608) | 1.035× (1.035×) |
  | `edge_detect` | 769,425,984 | 822,291,456 (631,340,032) | 1.069× (0.821×) |
  | `invert` (`adjustment`) | 385,204,536 | 442,500,718 (250,500,718) | 1.149× (0.650×) |

  So a tone adjustment's plan was 135 MB below its peak, and `edge_detect`'s 138 MB below: the copy is
  genuinely live beside the codes and the held plane. Box Blur's peak is in its plane phase, whose freed
  binary64 sums (n×40 beyond the plane) already cover the copy, so its plan is unchanged.

**Whole-plane admission (step 8d).** These are the 24 MP plans, before → after. A 16-bit Apply admits
when retained + plan ≤ 2 GiB. At the measured Apply retained set of 487,117,150 B (the immediate Apply,
after the drain), the plan can be at most 1,660,366,498.

- The banded ops are unchanged and still admitted: `blur` σ2 456,991,744; `sharpen` 1.5 456,876,544;
  `surface_blur` r5 T15 464,968,704.
- Unchanged, because the kernel's freed scratch already covers the plane and the copy:
  - `gaussian_blur` r2, `unsharp_mask` and `high_pass`: 1,016,388,7xx.
  - `add_grain`: 824,388,664.
  - `box_blur` r10: 1,592,388,608, admitted (487,117,150 + 1,592,388,608 = 2,079,505,758 ≤ 2 GiB).
- These rise to plane + codes + copy (n×24) or codes + copy (n×16), and are still admitted:
  - `dust_and_scratches`: 632,388,868 → 822,291,456.
  - `edge_detect`, `emboss` and `pixelate`: 631,340,032 → 822,291,456.
  - `add_noise` and `levels_channels`: 246,291,456 → 438,291,456.
- These are +192,000,000 (the copy is additive) and still admitted:
  - `auto_tone`, `auto_contrast` and `auto_color`: 248,388,608 → 440,388,608.
  - The tone `adjustment`s: 250,500,520–250,501,312 → 442,500,520–442,501,312; Shadows/Highlights
    259,505,642 → 451,505,642.
- `reduce_noise` (defaults) is refused by its own plan both before and after (2,362,498,715 bytes including
  its source, unchanged).

## 24 MP ceilings Part D phase 1 — a radius-independent legacy Gaussian (2026-10-03)

**Outcome.** From σ4 up, the legacy Gaussian Blur family runs Deriche's 4th-order recursive Gaussian
(`core/src/sampling_gaussian_fast.rs`). The family is the 8-bit `rz_image_blur` and `rz_image_sharpen`
through `sampling::blur`, and the 16-bit `blur`/`sharpen` selected edits through `selected_filter`. Its
cost per pixel does not depend on σ, so a 24 MP 16-bit layer admits every σ the slider offers, up to 64.
Below σ4 the output is byte-identical to HEAD. The probe/driver are the `gaussian_blur` job in
`scripts/photoshop-probe/photoshop.jsx` and `scripts/photoshop_gaussian_reference.py`.

**Rule corrected after the run.** As fixed before the run, the rule made nothing eligible. Its "live"
clause asks for ≤ 0.1/255 mean against C0, `sampling_gaussian`'s 3σ-truncated kernel. Every
radius-independent filter fails it at σ40/64, because of C0's own truncation (rows below). The orchestrator
ruled that the live clause was phase-2-only and mis-specified against a truncated kernel. Phase 1 changes
only the legacy family, so its eligibility is the legacy clauses, the Photoshop-closeness clause and
finiteness, each exactly as fixed before the run. Under that rule the measured winner is C4 (Deriche), with
the switch at σ_legacy = 4. Phase 2 must re-derive the live clause against an untruncated reference.

**The rule** (fixed in the spec before any run). Every metric is on premultiplied unit-scale RGBA, over the
whole image including edges, on fixtures F1–F4 at 8 and 16 bits, at σ ∈ G = {0.5, 1, 1.5, 2, 2.5, 3, 4, 5, 6,
8, 12, 16, 25, 40, 64}. E/M_live is the max/mean |X − C0|, where C0 is `sampling_gaussian`'s kernel
(normalized binary64 taps, `2·ceil(3σ)+1`, clamped). E/M_legacy is the same against CL, which is
`image::imageops::blur` on premultiplied f32 (today's legacy kernel). σ_legacy is the smallest σ ≥ 2 in G
with E ≤ 1/255 and M ≤ 0.1/255 at every larger σ in G, measured against CL. σ_live is the same against C0,
with no floor. A candidate is eligible when σ_legacy ≤ 6, σ_live ≤ 6, it is within P(C0, r) + 0.25/255 of
Photoshop at every radius r ≥ σ_legacy, and its output is finite at σ = 1000 and σ = f32::MAX.

**Fixtures.** Each is 512 × 512:

- F1 is a quadrant step, 0 or full scale per channel.
- F2 is a 1-px full-scale line grid every 64 px.
- F3 is splitmix64 noise, seed 0x5EED.
- F4 is a 50%-alpha radial disc (radius 160) on transparent, with saturated hidden RGB. It was built in the
  harness, so no Photoshop file exists for it.

The SHA-256s of F1–F3:

| Fixture | 8-bit | 16-bit |
|---|---|---|
| F1 `quadrant` | `bd169655…721121` | `25ef64ef…b8a935` |
| F2 `lines` | `c7d0adc1…414a84` | `8326b0ec…0f6908` |
| F3 `noise` | `3cfcf4f2…943fb13` | `c50c46ae…dd08a991` |

**Photoshop.** Adobe Photoshop 2025 26.10.0, executable SHA-256 `a502d3b62279299a77f85d69a3d8f3d5379e0fb406d40d746a00ee49031bfa04`.
The take used one `run_photoshop.Session` with six jobs, one per fixture and depth, each at radii 1, 2, 4, 8,
12 and 25. Photoshop was already running, so nothing was launched or quit (`was_running` true, `launched`
false). Every job reported the file's own depth and an untagged profile. Manifest SHA-256:
`f10dc99a4611ba2709aa52948256565ffeaf3209a76245365c1df247a5d59760`.

**Harness.** This was a scratchpad Rust program, never committed, using `image` 0.25.10 only. It runs the
production structure: 128-row tiles, each from its own window of ±H rows (H = ceil(4σ)+4 for an IIR, and the
support K·(r+1) for an extended box), replicated at the window edge and clamped at the image edge. Line math
is f64 and storage f32, and every output is clamped to a valid premultiplied sample. The candidates:

- C1 is an extended box with K = 3 (Gwosdek 2011), and C2 the same with K = 5. Both pad the line by their
  support so the image edge is exactly clamped.
- C3 is Young–van Vliet 3rd order, with Triggs–Sdika right-boundary states computed by running the two
  recursions to convergence.
- C4 is Deriche 4th order (the 1993 coefficients), parallel causal plus anticausal. It is normalized by its
  DC gain, computed in a product form that stays accurate as the poles approach 1, and initialized by
  constant extension.

**E/M against C0 (live)**, worst over F1–F4 at both depths, in 1/255 units:

| σ | C1 E / M | C2 E / M | C3 E / M | C4 E / M |
|---|---|---|---|---|
| 0.5 | 3.495 / 0.602 | 3.703 / 0.506 | 18.311 / 4.426 | 0.136 / 0.024 |
| 1 | 2.543 / 0.413 | 9.961 / 1.512 | 16.621 / 2.166 | 0.101 / 0.014 |
| 1.5 | 6.625 / 0.615 | 1.856 / 0.201 | 11.035 / 1.037 | 0.062 / 0.007 |
| 2 | 3.392 / 0.323 | 2.331 / 0.219 | 8.544 / 0.584 | 0.237 / 0.020 |
| 2.5 | 4.402 / 0.367 | 1.549 / 0.144 | 5.091 / 0.371 | 0.134 / 0.010 |
| 3 | 3.115 / 0.260 | 1.896 / 0.168 | 6.000 / 0.300 | 0.348 / 0.019 |
| 4 | 2.890 / 0.255 | 1.812 / 0.141 | 6.152 / 0.266 | 0.414 / 0.018 |
| 5 | 2.948 / 0.253 | 1.879 / 0.140 | 5.897 / 0.237 | 0.457 / 0.019 |
| 6 | 2.980 / 0.247 | 1.979 / 0.142 | 5.532 / 0.207 | 0.488 / 0.020 |
| 8 | 3.016 / 0.234 | 2.005 / 0.133 | 4.811 / 0.208 | 0.527 / 0.021 |
| 12 | 3.043 / 0.204 | 2.053 / 0.118 | 3.824 / 0.268 | 0.569 / 0.033 |
| 16 | 3.055 / 0.213 | 2.072 / 0.121 | 3.186 / 0.316 | 0.591 / 0.044 |
| 25 | 3.069 / 0.327 | 2.083 / 0.187 | 2.434 / 0.392 | 0.615 / 0.071 |
| 40 | 3.078 / 0.506 | 2.090 / 0.291 | 2.260 / 0.491 | 0.631 / **0.113** |
| 64 | 3.083 / 0.767 | 2.097 / 0.447 | 1.965 / 0.550 | 0.641 / **0.178** |

**E/M against CL (legacy)**, in the same units:

| σ | C1 E / M | C2 E / M | C3 E / M | C4 E / M |
|---|---|---|---|---|
| 0.5 | 3.701 / 0.638 | 3.870 / 0.533 | 18.439 / 4.459 | 0.288 / 0.058 |
| 1 | 3.477 / 0.486 | 9.224 / 1.474 | 18.253 / 2.377 | 2.395 / 0.311 |
| 1.5 | 8.367 / 0.778 | 3.902 / 0.499 | 14.781 / 1.360 | 4.372 / 0.418 |
| 2 | 3.755 / 0.350 | 2.694 / 0.252 | 9.517 / 0.641 | 1.352 / 0.101 |
| 2.5 | 4.532 / 0.374 | 1.680 / 0.154 | 5.514 / 0.381 | 0.615 / 0.038 |
| 3 | 3.290 / 0.270 | 2.266 / 0.181 | 6.664 / 0.323 | 1.092 / 0.056 |
| 4 | 2.890 / 0.255 | 1.812 / 0.141 | 6.152 / 0.266 | 0.414 / 0.018 |
| 5 | 2.948 / 0.253 | 1.879 / 0.140 | 5.897 / 0.237 | 0.457 / 0.019 |
| 6 | 2.980 / 0.247 | 1.979 / 0.142 | 5.532 / 0.207 | 0.488 / 0.020 |
| 8 | 2.934 / 0.234 | 1.915 / 0.132 | 4.649 / 0.201 | 0.337 / 0.014 |
| 12 | 2.929 / 0.202 | 1.929 / 0.115 | 3.600 / 0.254 | 0.314 / 0.019 |
| 16 | 2.902 / 0.213 | 1.903 / 0.119 | 2.878 / 0.290 | 0.238 / 0.019 |
| 25 | 2.898 / 0.327 | 1.896 / 0.182 | 2.095 / 0.352 | 0.222 / 0.027 |
| 40 | 2.896 / 0.505 | 1.890 / 0.281 | 2.032 / 0.439 | 0.211 / 0.040 |
| 64 | 2.893 / 0.759 | 1.889 / 0.425 | 1.927 / 0.563 | 0.205 / 0.061 |

**P(X, r)**: the mean |X(σ = r), quantized to the depth, − Photoshop(r)| over the interiors of F1–F3 (a border
of ceil(4r)+4 excluded), in 1/255 units:

| Depth | r | C0 | CL | C1 | C2 | C3 | C4 |
|---|---|---|---|---|---|---|---|
| 8 | 1 | 0.824 | 0.953 | 0.873 | 1.429 | 1.089 | 0.820 |
| 8 | 2 | 0.307 | 0.356 | 0.240 | 0.219 | 0.475 | 0.302 |
| 8 | 4 | 0.180 | 0.180 | 0.070 | 0.044 | 0.311 | 0.181 |
| 8 | 8 | 0.130 | 0.130 | 0.119 | 0.074 | 0.242 | 0.132 |
| 8 | 12 | 0.148 | 0.145 | 0.128 | 0.084 | 0.238 | 0.145 |
| 8 | 25 | 0.232 | 0.222 | 0.177 | 0.094 | 0.402 | 0.226 |
| 16 | 1 | 0.827 | 0.965 | 0.876 | 1.426 | 1.098 | 0.823 |
| 16 | 2 | 0.322 | 0.369 | 0.220 | 0.235 | 0.461 | 0.318 |
| 16 | 4 | 0.171 | 0.171 | 0.078 | 0.040 | 0.315 | 0.169 |
| 16 | 8 | 0.120 | 0.118 | 0.095 | 0.006 | 0.219 | 0.118 |
| 16 | 12 | 0.126 | 0.123 | 0.095 | 0.007 | 0.215 | 0.121 |
| 16 | 25 | 0.184 | 0.178 | 0.148 | 0.007 | 0.358 | 0.179 |

The best-fit σ of C0 against Photoshop's radius r (information only) at 16 bits is 1.080, 2.096, 4.114,
8.197, 12.285 and 25.573 for r = 1, 2, 4, 8, 12 and 25 (8 bits: 1.080, 2.098, 4.112, 8.158, 12.251,
25.485). Photoshop's radius is σ, about 2% wider. C2's near-zero 16-bit error at r ≥ 8 suggests that
Photoshop's own large-radius Gaussian is an iterated box.

**Rule outcome** (as run; "Eligible" is the rule as fixed before the run, with the live clause; the
correction above drops that clause, which makes C4 eligible):

| Candidate | σ_legacy | σ_live | P rule | Check (d) | Eligible |
|---|---|---|---|---|---|
| C1 extended box K=3 | none (E ≈ 2.9–3.1/255 at every σ) | none | holds | finite | no |
| C2 extended box K=5 | none (E ≈ 1.9–2.1/255) | none | holds | finite | no |
| C3 Young–van Vliet | none (E ≥ 1.9/255) | none | holds | finite | no |
| C4 Deriche 4th order | 4 | **none**: M_live 0.113/255 at σ40, 0.178/255 at σ64 | holds | finite | **no** |

Deriche's only failure is M_live above σ25, and it comes from C0's own truncation rather than from the
approximation. C0 cuts the Gaussian at ceil(3σ) and renormalizes. The per-fixture rows below compare Deriche
with C0, with CL (cut at about 3.3σ), and with C0's arithmetic at ceil(6σ) support (information only, 16
bits, 1/255 units):

| σ | Fixture | vs C0 E / M | vs CL E / M | vs 6σ support E / M |
|---|---|---|---|---|
| 40 | F1 quadrant | 0.631 / 0.113 | 0.211 / 0.040 | 0.040 / 0.008 |
| 64 | F1 quadrant | 0.641 / 0.178 | 0.205 / 0.061 | 0.037 / 0.011 |
| 64 | F4 disc | 0.610 / 0.083 | 0.186 / 0.028 | 0.034 / 0.005 |
| 64 | F3 noise | 0.106 / 0.011 | 0.035 / 0.004 | 0.009 / 0.001 |

The M ≤ 0.1/255 bound against C0 therefore rejects any untruncated filter at large σ on a large-scale edge,
however close it is to a true Gaussian.

**Information rows** (recorded, not part of the outcome):

- U, the 8-bit production arm against HEAD's opaque byte path (`imageops::blur` on u8) on F1–F3, as max /
  mean byte change, at every σ ≥ 4 in G. C4: 1 / 0.007–0.018 (2 / 0.052 at σ2). C2: 2 / 0.075–0.155.
  C1: 3 / 0.142–0.276. C3: 2–7 / 0.122–0.198.
- t, the min-of-3 ns/px at σ25 on 6000 × 4000 noise, measured through the harness's band driver (not a
  production implementation). C3: 58.8 (2 passes). C4: 113.0 (2 passes). C1: 113.2 (3 passes). C2: 197.6
  (5 passes).
- Check (d): every candidate is finite at σ = 1000 and σ = f32::MAX. The harness caps σ at 2048 before
  computing coefficients, because a pole at exactly 1 has no gain. Against C0 at σ1000 on a 256² crop, C4 is
  0.037 / 0.013 (E / M, 1/255), C1 0.871 / 0.314, C2 0.473 / 0.169 and C3 29.5 / 10.7: Young–van Vliet loses
  its accuracy there. A 256² crop is nearly flat at σ1000, so these are finiteness checks, not kernel-accuracy
  bounds.

**Implementation.**

- `sampling_gaussian_fast::band` runs each 128-row tile from its own window of ±(ceil(4σ)+4) rows, so a
  band is bit-identical to the whole plane (A1's contract). The window is replicated at its inner edge and
  exactly clamped at the image edge.
- The horizontal pass covers every window row. The vertical pass walks rows with a four-row column state,
  causal and then anticausal, the causal half held in the f32 output.
- Line math is binary64 and storage f32, and every sample is clamped to a valid premultiplied one.
- σ1000 (`MAX_SIGMA`) is the largest σ it accepts: `plan` and `band` answer Err above it, and
  `rz_image_blur` / `ops::blur` answer NULL, so no σ is run as a smaller one. The MCP `apply_filter` `blur`
  refuses a sigma above 1000 at 8 bits by name; the 16-bit edits stop at 64. At σ1000 an 8001-pixel step with
  each clamped edge 4σ away is within 0.83 codes of the exact Gaussian at 8 bits, under the 2-code bound
  (`sigma_1000_is_within_the_bound_and_larger_sigmas_are_refused`). Further up the poles approach 1, and at
  f32::MAX they would be exactly 1.
- `legacy_rows_scratch` and `legacy_band` branch at `LEGACY_FAST_SIGMA` (σ4). The band divides the
  premultiplied result back out with the unchanged rule.
- `sampling::blur` feeds `accumulate`/255 one band at a time and converts each band through
  `straight(v·255)`. If a band cannot allocate, it falls back to HEAD's kernel for the whole image, so
  `rz_image_blur` keeps the header's NULL contract.

**Calibration** (the `#[ignore]`d probe in `core/tests/gaussian_radius_independent.rs`; Apple M2 Max,
release). The reference, byte High Pass r16 on 3000 × 2000 noise, took 4.029 s, which is 1.1734e9 work
units/s. Implied units per window pixel:

| σ | byte `rz_image_blur` | 16-bit `blur` edit |
|---|---|---|
| 4 | 27.5 | 48.4 |
| 12 | 25.6 | 41.8 |
| 25 | 23.5 | 36.0 |
| 64 | 22.1 | 30.6 |

The largest implied figure is 48.4, and 48.4 × 1.15 = 55.7, so `FAST_WORK_PER_PIXEL` = 56. The window rows
at 6000 × 4000, from the plan's own formula, are W = 7,204, 10,376 and 19,656 at σ12, σ25 and σ64. The
spec's 7,224 and 10,448 missed the second-to-last tile's clipped window. So F_max = floor(24e9 / (W·6000))
is 555, 385 and 203. F = 56 is below F_max(64), so the outcome is the first one: every slider σ is admitted
at 24 MP, and there is no σ_ceil.

**Footprint** (3 runs, one case per process, 6000 × 4000 opaque noise, E = `/usr/bin/time -l`
peak − pre-edit `ri_phys_footprint`, bytes):

| Case | Max E | Charge | Charge / E |
|---|---|---|---|
| 16-bit `blur` σ12, `selected_edit_plan` | 422,887,760 | 479,246,336 | 1.133× |
| 16-bit `blur` σ25, `selected_edit_plan` | 432,849,256 | 489,230,336 | 1.130× |
| 8-bit `rz_image_blur` σ25, arm buffers (n×4 output + one band's `fast::plan`; not charged by any plan) | 146,292,880 | 146,938,880 | 1.004× |

No code charges the byte arm: `sampling::blur` runs with no job and no plan, so its row is a by-hand sum and
its thin margin gates nothing. At four bookkeeping allowances that sum was 145,890,304 (0.997×). Probed at σ4,
σ25, σ64 and σ1000, the byte E exceeded the output and the buffers by a fixed 4.55–4.75 MB, so `fast::plan`
carries a fifth allowance, sized from this unenforced row. The fifth adds 1 MiB to the 16-bit plans above.

**Admission.** At 6000 × 4000 the 16-bit `blur` plans for σ12 and σ25 fit `selected_banded_filters`'
one-band bound, and σ64 is admitted. At 4000 × 3000, σ12 fits the same bound.

**Tests** (`core/tests/gaussian_radius_independent.rs`):

- Against an independent binary64 3σ Gaussian, on premultiplied samples, at σ4, 12, 25 and 64 on quadrant,
  noise and disc fixtures (131 × 300, three bands). The bounds are 2 codes at 8 bits and 166 at 16 bits:
  ceil(0.6409/255 · (2^d − 1)) + 1, from the measured max E_live at σ ≥ 4.
- Byte input agrees across depths within 1 level at σ12.
- Hidden saturated colour cannot tint at σ12, at both depths.
- A precancelled σ12 edit answers only the cancel, with scratch 0.
- σ1000 at 8 bits against the same oracle and bound, and σ1000.5, σ2000 and f32::MAX are NULL.
- The admission rows above.
- `selected_banded_filters.rs` runs its library-blur case at σ3.5 (was σ6) and now admits σ12 at 24 MP.
  It also checks the fast arm's bands against its whole plane: the σ12 banded 16-bit layer edit equals the
  scalar-target edit (which runs `legacy_blur` once over the whole plane) code for code.

**Probe rows** ("slider preview latency", ms first-visible / exact; Gaussian σ2 → σ12; A2's rows beside them).
The probe printed no FAIL:

| Layer | Stage | A2 | Phase 1 |
|---|---|---|---|
| 24 MP 16-bit | opening (σ2) | 388 / 1,728 | 384 / 1,723 |
| 24 MP 16-bit | one change (σ12) | 77 / refused | 76 / **1,977** |
| 24 MP 16-bit | four changes (σ12) | 88 / refused | 80 / **2,284** |
| 24 MP 16-bit | Apply immediate / 3 s | 1,675 / 1,687 | 1,673 / 1,709 |
| 12 MP 16-bit | opening | 378 / 998 | 384 / 1,006 |
| 12 MP 16-bit | one change (σ12) | 79 / refused | 77 / **1,045** |
| 12 MP 16-bit | four changes (σ12) | 104 / refused | 85 / **1,375** |
| 12 MP 16-bit | Apply immediate / 3 s | 1,034 / 984 | 986 / 976 |
| 24 MP 8-bit | opening | 373 / 895 | 378 / 905 |
| 24 MP 8-bit | one change (σ12) | 68 / 2,166 | 68 / **1,306** |
| 24 MP 8-bit | four changes (σ12) | 70 / 2,436 | 69 / **1,624** |

The Surface Blur rows are unchanged within run-to-run noise (24 MP 16-bit: 556 / 18,538 opening, Applies
18,031 / 17,902).

## 24 MP ceilings Part C — Reduce Noise statistics in row rings, a measured rank charge (2026-10-03)

**Outcome.** Reduce Noise's guided filter keeps its binary64 window statistics as streamed row rings
instead of 6–8 whole n×8 planes, bit-identical to before. A 6000 × 4000 layer is admitted at 8 and 16 bits
by the 16-bit sheet (preview and Apply), MCP `apply_filter` and the live filter's core plans; in the app the
16-bit live filter is still refused over a 24 MP photo background (drive below). The Dust & Scratches and
Reduce Noise work charges are re-calibrated against measured time, so their 24 MP admissions are honest
bounds. Dust & Scratches' 24 MP ceiling stays r3.5.

**Decision.**

- `core/src/sampling_box.rs` is the ONE clamped-edge sliding box sum. `BoxRows` takes input rows in order
  (`push`) and hands back output rows in order (`emit`) as soon as the rows they read are in. It holds a
  ring of min(2r+2, h) horizontal rows and one running column. The arithmetic is the former whole-plane
  `box_sum`'s, expression for expression: rows seeded 0.0 + the 2r+1 clamped reads, then slid; the column
  seeded 0.0 + the 2r+1 clamped rows, then slid by the difference `H[add] − H[drop]`. `box_sum` (Box Blur)
  is now its whole-plane wrapper.
- `sampling_denoise::guided_bounded` runs each active half as its own sequential pass (luminance, then
  chroma; the peak is the larger half, not the sum). Inside a half, stage A streams the window statistics
  through `BoxRows` (luma: alpha, αY, αY²; chroma: those three plus αC and αYC per channel). Each emitted
  stage-A row becomes one row of the local regression, pushed straight into stage B's rings (luma: αa and
  αb; chroma: two per channel). Each emitted stage-B row is written into the correction plane. Stage A's
  summed alpha waits in a min(r+1, h)-row delay ring: stage-B row b is ready right after stage-A row
  min(b + r, h − 1). Progress is one report per written row, luminance in [0, ½] and chroma in [½, 1] when
  both run.
- The plan (`plan_bounded`) charges n×16 (the correction plane) + max(luma ? `half_bytes`(w,h,rl,5,2) : 0,
  chroma ? `half_bytes`(w,h,rc,11,6) : 0) + 4 BK. `half_bytes(w,h,r,boxes,rows)` =
  boxes·(min(2r+2,h)+1)·w·8 + (min(r+1,h)+rows)·w·8: the rings, the delay ring and the regression rows
  (luma: slope, offset; chroma: mean, variance, slope and offset per channel). At 6000 × 4000 the kernel is
  403,362,304 at the defaults (r1: rl 3, rc 12), 602,082,304 at r16 and 672,738,304 at r64. HEAD's was
  1,924,194,304 at every radius (80n + 4 BK).
- Box Blur's plan keeps HEAD's figure, now an over-charge: its transient pass is a ring, not a plane (dropped by the byte filter_op plan, see [its section](#the-byte-filter_op-plan-and-admission)).

**Pins** (`core/tests/banded_reduce_noise.rs` (a)). Captured on the unchanged tree (a4a5859), one SHA-256 per
path over its cases' outputs in order. The fixtures are F_A 131 × 300 splitmix noise, opaque, with a 50%-alpha
disc (radius 40) and a 60 × 60 fully transparent block of saturated hidden RGB at (5, 5); F_B the same at
300 × 131; F_C 7 × 5; F_D 1 × 97; F_E 97 × 1. P1 `{}`, P2 `{radius:4, luminance:0.05, detail:0, color:1}`, P3
luminance only, P4 chroma only, P5 `{radius:16}` (rc 192, so the ring is the whole height). Paths: byte
`rz_image_filter_op` (all F × P), the 16-bit selected edit (F_A and F_C × P1, P2, P5), the live filter at 16
and 8 bits (F_A × P1, P2), and byte Box Blur r3 and r200 (F_A, F_D). All five pass on the changed tree, and
so do `live_filter_denoise_numeric.rs` and `cleanup_filters.rs` untouched.

**Calibration** (the `#[ignore]`d probe in `core/tests/banded_reduce_noise.rs`; Apple M2 Max, release,
load average about 22–32). The reference is batch 149's: byte High Pass σ3.9 on 3000 × 2000 noise, charged
n·(8·25+12), 0.375 s, R3.9 3.3964e9 units/s (0.6% from 149's recorded 3.3768e9, gate 10%). It is stated in
the radius-16 unit every other work constant uses (÷ `R39_PER_R16` 2.86): 1.1876e9 units/s. U is the
implied units per pixel, min of 3.

| Dust & Scratches, threshold 0 | k | byte U | 16-bit edit U | charge before (24k+24) |
|---|---|---|---|---|
| r1 | 5 | 129.3 | 180.9 | 144 |
| r2 | 13 | 415.2 | 493.5 | 336 |
| r3 | 29 | 560.1 | 701.2 | 720 |
| r4 | 49 | 723.4 | 947.8 | 1,200 |
| r6 | 113 | 1,223.7 | 1,724.4 | 2,736 |
| r8 (3000 × 1500) | 197 | 1,793.0 | 2,655.2 | 4,752 |

r8 is timed on the plate's first 1500 rows, because at 3000 × 2000 the charge before calibration refuses it.
The least-squares fits are byte U = 8.039·k + 263.463 and 16-bit U = 12.289·k + 285.590. The rule takes
a = 12.289 and b = 285.590 (the larger of each), f = 1.1081 (the worst point's U / (a·k + b)), and ⌈1.15·f·a⌉
and ⌈1.15·f·b⌉. So `RANK_NEIGHBOUR_WORK` = 16 and `RANK_PIXEL_WORK` = 364 (were 24 and 24). A disc costs less
per neighbour and much more per pixel than the old model said: it under-charged r1–r2 (by up to 1.5× at
r2) and over-charged large discs. At 6000 × 4000 r2 charges 13.7e9 (was 8.1e9; budget 24e9), and the ceiling in
quarter-pixel steps to 16 is r3.5 before and after.

| Reduce Noise | byte U | 16-bit edit U | charge per pixel before |
|---|---|---|---|
| luminance only, r1 | 37.8 | 117.2 | 44 |
| luminance only, r16 | 40.3 | 121.2 | 44 |
| chroma only, r1 | 69.1 | 264.7 | 92 |
| chroma only, r16 | 74.3 | 286.3 | 92 |

Each half's charge is raised to the smallest integer for which n·charge + seeding covers 1.15 × every run:
`DENOISE_LUMA_WORK` 44 → 139, `DENOISE_CHROMA_WORK` 92 → 326. The 16-bit edit's runs (the whole selected
edit, kernel included) set both. At 6000 × 4000 the defaults
charge 11.16e9 and r64 11.24e9 (budget 24e9), so every radius 0–64 is admitted by work before and after. The
live filter's interactive ceilings drop accordingly (`phase-9-live-filters-status.md`).

**Footprint** (`precision_png_rows.rs`'s method: one case per process, 3 runs, `MallocLargeCache=0`, a
baseline twin per case that read E 0 every time; pre = `ri_phys_footprint` after
`proc_reset_footprint_interval`, E = `ri_interval_max_phys_footprint` − pre; 6000 × 4000 opaque splitmix
noise, the live cases batch 149's plasma Display P3 master placed in an sRGB document with a cold cache):

| Case | E ×3 | Retained | Scratch | Scratch / max E | Gate | 2 GiB − (retained + scratch) |
|---|---|---|---|---|---|---|
| S16-N, 16-bit selected `reduce_noise` `{}` | 753,877,544 / 753,861,160 / 753,877,544 | 192,012,955 | 822,291,456 | 1.0907× | ≥ 1.0× | 1,133,179,237 |
| S16-N16, `{radius:16}` | 768,655,912 / 768,623,144 / 768,623,144 | 192,012,955 | 848,373,760 | 1.1037× | ≥ 1.0× | 1,107,096,933 |
| L16-N, live `reduce_noise` defaults, 16-bit | 964,444,880 / 964,428,496 / 964,395,728 | 384,027,210 | 1,475,423,405 | 1.5298× | ≥ 1.25× | 288,033,033 |
| L8-N, the same at 8 bits | 960,660,128 / 960,660,128 / 960,676,512 | 192,027,170 | 1,234,690,482 | 1.2852× | ≥ 1.25× | 720,765,996 |

Every gate passes. The S16 gate is 1.0×, not 1.25×: the selected arm's scratch comes from the 16-bit previews' arm formula,
whose terms this batch does not change, and every allocation is fixed-size. At defaults the arm's floor (held
n×16 plane + n×8 codes + n×8 copy) sets it; at r16 the kernel (602,082,304) exceeds that 24n floor and the
scratch is the kernel + 10n + 6 BK.
HEAD refused S16-N at 2,362,498,715 bytes including the source.

B8-N (information only, the byte `rz_image_filter_op` arm has no plan): E 864,567,944 ×3 against the by-hand
32n + max(rings, 4n) + 4 BK = 868,194,304 (1.0042×). That arm's missing plan and host admission are
added in [the byte filter_op plan section below](#the-byte-filter_op-plan-and-admission) (F-RN: E 480,346,472).

**Admission at 6000 × 4000.** The 16-bit selected plans are 822,291,456 (defaults), 848,373,760 (r16) and
919,029,760 (r64), all under Part A2's Apply bound of 1,660,366,498. The live plans admit the defaults
at both depths (work 12,075,100,000). Dust & Scratches r2 is admitted, r3.5 is the ceiling and r3.75 refuses by work.

**Tests** (`core/tests/banded_reduce_noise.rs`): (a) the pins; (b) the 24 MP 16-bit plans at the defaults,
r16 and r64, retained + scratch ≤ 2 GiB, the defaults' scratch ≥ ⌈max E(S16-N)⌉ and ≤ the Apply bound;
(c) the 24 MP live plans at both depths, scratch ≥ ⌈1.25 × max E(L*-N)⌉; (d) the dust ceiling r3.5 admitted
and r3.75 refused naming work; (e) precancelled byte and 16-bit edits answer only the cancel (the selected
edit with scratch 0 and its input unchanged), a cancel parked in the byte op's chroma half answers the cancel,
and an uncancelled run's reports are monotone and end at 1.0; (f) at 2000 × 1500 `{radius:64}` the selected
scratch equals HEAD's 280,485,760 − 64n + `half_bytes(2000,1500,256,11,6)` = 94,848,000, written out by hand;
(g) the calibration probe. Since the rank charge fell per neighbour, a stack of three r8 discs on 1400 × 1400
fits the budget, so `live_filter_neighborhood.rs` and `live_filter_stack_native.rs` refuse three r10 discs
(each admitted alone) instead.

**Probe rows** ("slider preview latency", ms first-visible / exact; the burst's figure is its final exact; the
run took 670 s, so `verify.py`'s bound is 1005 s). The probe printed no FAIL:

| Layer | Sheet | Opening (r) | One change | Four changes | Apply immediate / 3 s |
|---|---|---|---|---|---|
| 24 MP 8-bit | Reduce Noise (r1 → r2) | 388 / 2,380 | 84 / 2,248 | 85 / 2,519 | — |
| 24 MP 8-bit | Dust & Scratches (r2 → r3) | 383 / 3,226 | 66 / 6,325 | 68 / 6,633 | — |
| 24 MP 16-bit | Reduce Noise (r1 → r2) | 444 / 7,694 | 140 / 7,632 | 458 / 8,012 | 7,674 / 7,786 |
| 24 MP 16-bit | Dust & Scratches (r2 → r3) | 379 / 5,493 | 77 / 9,149 | 77 / 9,489 | 5,537 / 5,558 |

The Surface Blur and Gaussian Blur rows are within run-to-run noise of Part D phase 1's (24 MP 16-bit
Gaussian: 380 / 1,714 opening, Applies 1,624 / 1,687).

**App drive** (MCP, own background instance on port 4975, `RZ_AUTOMATION_ALERTS=record`, no
`[AutomationAlerts]` line; a 6000 × 4000 plasma PNG, RGB16 then RGB8, opened as the document):

| Call | 16-bit | 8-bit |
|---|---|---|
| `open_document` | ok, 1.4 s | ok, 0.5 s |
| `apply_filter` `reduce_noise` `{}`, `undo` | ok, 7.6 s; ok, 0.6 s | ok, 2.5 s; ok, 0.4 s |
| `apply_filter` `reduce_noise` radius 16, `undo` | ok, 7.9 s; ok, 0.5 s | ok, 2.3 s; ok, 0.4 s |
| `apply_filter` `dust_and_scratches` `{}`, `undo` | ok, 5.6 s; ok, 0.6 s | ok, 2.3 s; ok, 0.4 s |
| `place_image` the same PNG | ok, 1.7 s | ok, 0.7 s |
| `set_live_filter` `reduce_noise` defaults | **refused by the app**: "This job needs more working memory than the current budget, and no other document is open to close. Work on a smaller image or region." | ok, 2.2 s |
| `get_live_filter` | ok (no filter attached) | ok, 0.2 s |

The 16-bit live refusal is the host's working-memory admission, not the core plan. The core plan for the
placed layer alone is retained 384,027,210 + scratch 1,475,423,405 (L16-N), 288 MB under 2 GiB, and this
document also retains the opened 16-bit photo (192 MB) beneath it. As information, the same call on a new
white 6000 × 4000 16-bit document (a uniform background) with the photo placed is admitted in 2.9 s. This is
the refusal class of the operations on a live-filtered layer (the app admits less than the core plans at 16
bits); it is an app ceiling, not fixed here (since 45c97f9 it is 62,200 bytes over the 2 GiB reference limit; at the
8 GiB limit a 16 GB Mac gets it is admitted, batch 163: plan 1,473,377,920, max E 935,445,176, 1.575×).

**Newly refused interactive-cap operations (a user-visible change).** Raising the two charges also raises
what every operation that re-renders a live-filtered layer under the interactive `MAX_WORK_UNITS` (2e9) is
charged: the ⌘T preview (a source `transform` with `interactive`), Replace Contents, RAW redevelopment (a
Replace Contents plus its attachment, so the same plan) and Convert to Profile (byte
`live_filter_profile::plan`, typed `color_plan`). Core plans only, on a 3:2 plasma Display P3 master placed at
identity in an sRGB document with the live filter set (scratch probe `rnint`, built against a4a5859 and
against this change; ⌘T at ×0.9 about the centre; Convert to Display P3). Each cell is a4a5859 → now; the
16-bit and 8-bit verdicts agree in every cell, and every refusal is by work:

| Filter | Operation | 4 MP (2448 × 1632) | 8 MP (3462 × 2308) | 12 MP (4242 × 2828) |
|---|---|---|---|---|
| Reduce Noise defaults | ⌘T preview, bicubic (the app's Catmull-Rom default) | admitted → **refused** | admitted → **refused** | refused → refused |
| | ⌘T preview, Lanczos3 | admitted → **refused** | refused → refused | refused → refused |
| | Replace Contents / RAW redevelop | admitted → **refused** | admitted → **refused** | refused → refused |
| | Convert to Profile | admitted → **refused** | admitted → **refused** | refused → refused |
| Dust & Scratches r1 | ⌘T preview, bicubic | admitted → **refused** | admitted → **refused** | refused → refused |
| | ⌘T preview, Lanczos3 | admitted → **refused** | refused → refused | refused → refused |
| | Replace Contents / RAW redevelop | admitted → admitted | admitted → **refused** | refused → refused |
| | Convert to Profile | admitted → admitted | admitted → **refused** | refused → refused |
| Dust & Scratches r2 | ⌘T preview, bicubic | admitted → **refused** | refused → refused | refused → refused |
| | ⌘T preview, Lanczos3 | admitted → **refused** | refused → refused | refused → refused |
| | Replace Contents / RAW redevelop | admitted → **refused** | refused → refused | refused → refused |
| | Convert to Profile | admitted → **refused** | refused → refused | refused → refused |

The ceilings (2e9 over the plan's units per pixel, from the 4 MP plans; a4a5859 → now) are:

| Filter | ⌘T preview, bicubic | ⌘T preview, Lanczos3 | Replace / RAW redevelop / Convert (1:1) |
|---|---|---|---|
| Reduce Noise defaults | 8.6 → 3.6 MP | 6.7 → 3.2 MP | 11.5 → 4.0 MP |
| Dust & Scratches r1 | 8.8 → 3.8 MP | 6.8 → 3.4 MP | 11.8 → 4.3 MP |
| Dust & Scratches r2 | 4.8 → 3.1 MP | 4.1 → 2.8 MP | 5.5 → 3.3 MP |

The live edit itself (one-shot) and the ⌘T commit are unaffected. No charge was lowered to keep these
admitted; restoring them is follow-up work: the ⌘T preview, and Replace Contents, RAW redevelop and Convert to
Profile. The old latency, for the record: at a4a5859 the 12 MP 8-bit ⌘T preview of a Reduce Noise layer was
already refused by its plan ("source transform exceeds the bounded sampling work limit"; ceiling ≈8.6 MP), so the largest admitted case, the 8 MP 8-bit bicubic
preview (1,858,303,660 units), was timed: 1.328 s with a cold live-filter cache and 0.552 s warm (min of 3,
release, Apple M2 Max). These ceilings are restored, with every charge above unchanged, by
[the phase-9 "Operations on a live-filtered layer" section](phase-9-live-filters-status.md#operations-on-a-live-filtered-layer):
Replace, RAW redevelop and Convert of a filtered layer are one-shot, and a refused non-enlarging ⌘T preview
resamples the layer's rendered pixels.

## The byte filter_op plan and admission

**Outcome.** The seven 8-bit filters behind `rz_image_filter_op` (Box Blur, Surface Blur, Add Grain, Unsharp
Mask, High Pass, Dust & Scratches, Reduce Noise) have one core plan, `ops_spatial::filter_op_scratch_bytes`
(`rz_image_filter_op_scratch_bytes`, `RasterImage.filterOpScratchBytes`), and the host admits it before the
filter allocates: the sheet's exact preview (`ByteFilterAdmission.reservePreview`, a `SelectionWorkBudget`
reservation), sheet Apply and MCP `apply_filter` (`ByteFilterAdmission.admit`, plan + the document's own
retained bytes), and so the replayed Actions and ⌃F that go through `apply_filter`. A filter that does not fit
is refused with admission's sentence: the sheet shows it on its status line (`tone.preview.status`) instead of
silently clearing the preview, Apply presents it, `apply_filter` answers it in-band. Every output is
byte-identical (pins in `core/tests/byte_filter_op_plan.rs`).

**Decision.**

- `ops_photographic::apply` no longer holds an n×16 straight copy of the input beside the n×16 plane. It
  reads the caller's bytes in place through a static 256-entry table holding `f32::from(v) / 255.0` (the
  value the copy stored, so bit-identical). Converting on every read instead made the multi-read kernels
  (the direct Gaussian below σ4, the denoise windows) 1.19–1.43× slower; the table brings them within 1.11×
  (gate T).
- The plan, refused exactly as `filter_op` refuses before it allocates (one private `dispatch`: op name,
  `SelectedFilter::parse`, bypass / identity, then the work ceiling): K = `neighborhood_plan(…).0`
  (photographic) or `SpatialKernel::plan(…).0` (spatial); base = max(K, 16n + 4n) + 4 BK (phase 1 the
  kernel; phase 2 its held n×16 plane beside the n×4 output); scratch = base + ⌈base / 4⌉. 0 is an exact
  bypass. Nothing is re-derived: K is the kernels' own plan.
- Box Blur's `SpatialKernel::plan` drops its n×8 transient term (step 7a): with it the 24 MP Box Blur was
  refused with a second 24 MP document open ("This job needs more working memory than the current budget.
  Close other documents or reduce the region before trying again."). The peak is the plane plus the four
  sums, 48n: `BoxRows`' ring (at most n×8) only lives while three sums or fewer are held. Measured at r5,
  r250 and r1000 at 24 MP, E is 1,152,680,776 every time. The 16-bit selected Box Blur plan is 58n + 8 BK.
  Since batch 164 `box_plane` holds ONE sum at a
  time, alpha first, bit-identically (the PINS, the oracle and the Reduce Noise pins unchanged): the kernel's
  peak is 24n + ring (r5: 576,323,992 at 24 MP, 1,091,158,808 at 45 MP). Since
  batch 165 the plan is that stage's own, 24n +
  ring + 2 BK (`ops_spatial`'s `BOX_BLUR_BYTES_PER_PIXEL` record; batch 164's 33n had folded in the commit),
  1.264× / 1.258× G1 after the quarter. The commit and first projection after every 8-bit filter is charged by
  the host as its own stage (`ByteFilterAdmission.commitStageBytes`: 8n + the margined projection), max() against
  the plan. The 16-bit selected Box Blur plan is 34n + ring + 8 BK (G3 825,492,608, 1.080× its 764,133,928).
- The preview reserves max(plan, `commitStageBytes`, i.e. 8n + `projectionScratchEstimate()`): the filter,
  then its n×4 result and the written layer, then the display. Apply admits the same max() since batch 165. `ByteFilterPreview.drain()` (cancel, then wait for the in-flight stage
  and the proxy source build) runs before Apply admits, because `RasterResources.report` adds
  `SelectionWorkBudget.scratchBytes` to the requested scratch and a live reservation would be charged twice.

**Gate F** (`bfa` scratch probe, one case per process, 3 runs + a baseline twin that read E 0 every time,
`MallocLargeCache=0`; pre = `ri_phys_footprint` after `proc_reset_footprint_interval`, E =
`ri_interval_max_phys_footprint` − pre; uncontrolled byte `rz_image_filter_op` on opaque splitmix noise; the
tree with the table and 7a):

| Case | E ×3 | Scratch | Scratch / max E | 2 GiB − (scratch + 4n) |
|---|---|---|---|---|
| F-RN, 6000 × 4000 `reduce_noise` `{}` | 480,346,472 ×3 | 605,242,880 | 1.2600 | 1,446,240,768 |
| F-RN16, `{radius:16}` | 598,016,432 ×3 | 757,845,760 | 1.2673 | 1,293,637,888 |
| F-DS, `dust_and_scratches` `{radius:2}` | 480,264,528 ×3 | 605,242,880 | 1.2602 | 1,446,240,768 |
| F-DS3, `{radius:3}` | 480,264,528 ×3 | 605,242,880 | 1.2602 | 1,446,240,768 |
| F-USM, `unsharp_mask` `{radius:1.5}` | 480,280,912 ×3 | 605,242,880 | 1.2602 | 1,446,240,768 |
| F-USM8, `{radius:8}` | 480,264,528 / 480,280,912 / 480,264,528 | 605,242,880 | 1.2602 | 1,446,240,768 |
| F-HP, `high_pass` `{radius:3}` | 480,280,912 ×3 | 605,242,880 | 1.2602 | 1,446,240,768 |
| F-BOX, `box_blur` `{radius:5}` (batch 164, one sum at a time; was 1,152,680,776 ×3 against 1,447,864,320) | 576,323,992 ×3 | 998,644,320 | 1.7328 | 1,052,839,328 |
| F-SURF, `surface_blur` `{radius:5,threshold:15}` | 480,248,144 ×3 | 605,242,880 | 1.2603 | 1,446,240,768 |
| F-GRAIN, `add_grain` `{seed:1}` | 576,340,376 ×3 | 727,864,390 | 1.2629 | 1,323,619,258 |
| G-RN, 8256 × 5504 `reduce_noise` `{}` | 909,394,592 ×3 | 1,141,268,480 | 1.2550 | 824,451,072 |
| G-BOX, 8256 × 5504 `box_blur` `{radius:5}` (batch 164) | 1,091,158,808 ×3 | 1,883,379,840 | 1.7260 | 82,339,712 |
| G-USM8, `unsharp_mask` `{radius:8}` | 909,279,880 ×3 | 1,141,268,480 | 1.2551 | 824,451,072 |
| G-DS, `dust_and_scratches` `{radius:2}` | refused by work at HEAD; not measurable | — | — | — |
| G-SURF, `surface_blur` `{radius:5,threshold:15}` | refused by work at HEAD; not measurable | — | — | — |

Every plan is ≥ ⌈1.25 × max E⌉ and no ratio exceeds 2.0. F-RN against B8-N: 480,346,472 vs 864,567,944,
−384,221,472 (16n, the dropped copy). Before 7a F-BOX planned 1,687,864,320 (1.4643×). G-DS and G-SURF are
refused by `MAX_DESTRUCTIVE_WORK` before any plan, with the same sentences at HEAD ("rank source filter
exceeds the bounded pixel-work limit", "surface blur exceeds the bounded pixel-work limit"); no work charge
changed. Both are still refused with those sentences at the 8 GiB limit (batch 163): the work cap does not scale.

**Gate T** (min of 3 wall-clock runs at 6000 × 4000, `bfa` built on HEAD and on the change, each order;
incremental and non-incremental builds; new / head, bound 1.15):

| Case | inc, head first | inc, new first | non-inc, head first | non-inc, new first |
|---|---|---|---|---|
| F-RN | 1.027 | 1.031 | 1.081 | 1.107 |
| F-DS | 0.999 | 1.007 | 1.019 | 1.010 |
| F-DS3 | 0.997 | 1.003 | 1.009 | 1.016 |
| F-USM | 1.005 | 1.004 | 1.034 | 1.039 |
| F-USM8 | 0.945 | 0.951 | 0.969 | 0.973 |
| F-HP | 1.029 | 1.044 | 1.084 | 1.075 |

Without the table (per-read conversion) the same gate read F-HP 1.26 / 1.43, F-RN 1.22 / 1.27 and F-USM
1.19 / 1.23 (incremental / non-incremental), which stopped the first attempt.

**Drive** (own background instance, port 4987; `png8` plasma + noise RGB fixtures; per row
`open_document`, `resource_usage`, `apply_filter`, `undo`, `close_document`; no `[AutomationAlerts]` line):

| Row | 6000 × 4000 | 8256 × 5504 |
|---|---|---|
| `reduce_noise` `{}` | ok 2.2 s | ok 4.1 s |
| `dust_and_scratches` r2 | ok 8.7 s | refused: "rank source filter exceeds the bounded pixel-work limit" (work, pre-existing) |
| `unsharp_mask` r1.5 | ok 1.1 s | ok 2.1 s |
| `unsharp_mask` r8 | ok 1.2 s | ok 2.4 s |
| `high_pass` r3 | ok 1.4 s | ok 2.8 s |
| `box_blur` r5 | ok 0.9 s | refused: "This job needs 2.72 GB of working memory on its own, at least the 2 GB working limit, so closing other documents will not help. Work on a smaller image or region." |
| `surface_blur` r5 T15 | ok 15.3 s | refused: "surface blur exceeds the bounded pixel-work limit" (work, pre-existing) |
| `add_grain` seed 1 | ok 1.3 s | ok 2.4 s |
| `box_blur` r5 with a second 24 MP document open | ok 0.8 s (retained 576,035,797 before) | — |

Before 7a the two-document row was refused (retained 576,035,797; plan 1,687,864,320). The jobs probe's
"slider preview latency" 6000 × 4000 8-bit row now ends with an immediate Apply for each sheet; every Apply
committed and no preview reservation outlived `drain()` (the stage FAILs unless
`SelectionWorkBudget.scratchBytes` is back at its pre-sheet value when the Apply action returns).

**Ceilings.** 45 MP 8-bit Box Blur is still refused at the 2 GiB reference limit, with the generic "needs more
working memory than the current budget" sentence. Since batch 164 it holds one sum at a time, so its kernel peak is
24n. The app's 45 MP `apply_filter` peak fell from 2,361,165,480 to 1,633,240,216, and that peak is the edit's
commit and first projection, shared by every 8-bit filter (Invert peaks at 1,633,698,968). Since batch 165 the
plan is 1,372,168,320 (24 B/px), and the commit is charged at the publication: 8n + the projection with
`ProjectionMargin`'s 6/16, 2,044,852,610 with the retained growth, 1.252× Box Blur's peak (1.254× Invert's). Its
need beside the 545,320,845 retained, 2,408,402,829 (53 B/px), is refused at 2 GiB and fits an 8 GB Mac's
2,862,612,480 (driven); the 8-bit filter-commit ceiling is about 40.5 MP at 2 GiB, 54 MP on an 8 GB Mac and
162 MP at 8 GiB. 45 MP 8-bit Dust & Scratches r2 and Surface Blur r5/T15 (the work limit,
unchanged here, and still refused at 8 GiB). The proxy stage (≤ 262,144 px) is not reserved.

## The byte filter_op Apply as a job

**Outcome.** Sheet Apply and MCP `apply_filter` for the seven 8-bit `rz_image_filter_op` filters (Box Blur,
Surface Blur, Add Grain, Unsharp Mask, High Pass, Dust & Scratches, Reduce Noise) run on the `BackgroundWork`
worker under a `RasterJobControl` with the progress panel, Cancel and Esc (`ByteFilterAdmission.run`, the
`PrecisionCapturedWork.run` pattern; the panel is revealed after `DocumentJobs.panelDelay`). Admission,
`drain()` before admit, undo, dirty tracking, selection clipping, the recorded step and the outputs are
unchanged; a plan of 0 (exact bypass) still runs inline. A cancel surfaces as `JobCancelled`
(`NSUserCancelledError`): nothing is shown, recorded or added to history. `apply_filter` still waits for its
result and returns no `job_id`.

**Decision.**

- The MCP worker wait's bounded pump moved to `WorkerSlotWait.swift` (`BackgroundWork.waitForAnonymousOccupant`,
  `anonymousWaitApplies`, `busyMessage`); `AgentServer+WorkerWait.swift` keeps its tool checks and calls it, so
  MCP's predicate, limit, slice and exempt set are unchanged. `ByteFilterAdmission.run` waits before its
  `WorkerTask`; the 16-bit `PrecisionAdjustmentSession.commit(filter:)`, `commit(op:)` (adjustment sheets and
  adjustment layers) and `PrecisionScalarAdjustmentSession.commit` wait before building their edit (each after
  its session is cancelled, so no preview request runs during the pump). A slot still held after the wait is
  refused by `WorkerTask` with `busyMessage`, as before.
- `SliderSheetController.applyClicked` (both depths) waits BEFORE `drain()` and `dismiss`, and if the slot is
  still held shows `busyMessage` on its status line, beeps and stays open with its values
  (`BackgroundWork.awaitSlotForSheetApply`). Refusing before dismissal is smaller than moving `dismiss` after
  the job, which cannot tell a refusal from a commit.
- The job runs inside `applyEdit`'s transform (`applyCapturedOperation` / `applyRasterizingEdit` take a
  throwing closure; `OperationCapture.applying` rethrows), after `isCurrent`, the rasterize prompt and
  `publicationRefusal`; `applyEdit` re-checks revision and state afterwards.
- The wait never covers a cancellable occupant (`DocumentJobs.active`, a running Content-Aware Fill review):
  the sheet shows `busyMessage` at once and the user can cancel that job and press Apply again. Nor does it
  run during an Action replay, ⌃F or a batch (`anonymousWaitApplies`): a byte Apply or `apply_filter` there,
  and an `apply_filter` while a cancellable job holds the slot, answers `busyMessage` at once (before this change
  those seven ran without the slot). `apply_filter` does not wait inside `ByteFilterAdmission.run`
  (`waitForSlot: false`): `execute`'s worker wait already covered the call, so that wait's behaviour is
  unchanged.
- An 8-bit exact bypass (plan 0) runs inline and needs no slot, so the sheet's Apply skips the slot wait for
  it (`ByteFilterAdmission.isExactBypass`).
- `apply_filter`'s job runs inside `applyScopedFilter`'s operator, which can service other calls, so the
  handler re-checks the editing state captured before it (`isCurrentEditingState`) and refuses with "The
  document changed before the edit could be applied" if the selection or target changed during the job.

**Core.** `every_byte_filter_op_reports_progress_and_cancels_mid_run` (`core/tests/filter_preview_cancel.rs`,
240 × 160): for each of the seven ops the controlled output equals the uncontrolled one (not a bypass), every
report's progress is in [0, 1], a stage-6 report lies strictly inside (0, 1), the log ends at (1.0, stage 5),
and a cancel parked at the second stage-6 report returns "operation cancelled". No core source changed.

**Probe** (jobs probe "slider preview latency", 2026-10-04, 891 s; no FAIL). 6000 × 4000 8-bit row, Apply
pressed at the opening value; the cancel stage clicks the job panel's Cancel at the first occupied tick whose
bar reads above 0 and FAILs unless nothing changed (handle, revision, undo, dirty, alerts, the worker slot and
its scratch, the preview registry); the immediate stage FAILs unless the job occupied the worker with the
preview registry at its pre-sheet value, no worker-side gap above 250 ms and a moving bar. Apply gap is the
largest main-thread stall from the press to the action's return (the write and projection after the job):

| Sheet | Cancel → return | Committed | Job gap | Apply gap | Progress max |
|---|---|---|---|---|---|
| Surface Blur r5 T15 | 7 ms | 15,947 ms | 20 ms | 285 ms | 1.0 |
| Reduce Noise r1 | 9 ms | 2,381 ms | 21 ms | 295 ms | 1.0 |
| Dust & Scratches r2 | 28 ms | 3,083 ms | 21 ms | 283 ms | 1.0 |
| Gaussian Blur σ2 (synchronous, out of scope) | — | 818 ms | — | 817 ms | — |

Busy-slot stage (Surface Blur; `WorkerHolder`, an anonymous occupant drained by a `.common` timer):

| Row | Slot held 0.5 s | Slot held past the 5 s limit |
|---|---|---|
| 6000 × 4000 8-bit | committed in 16,377 ms | kept open after 5,017 ms (status `busyMessage`, values, document and history unchanged); re-Apply committed in 15,828 ms |
| 4000 × 3000 16-bit | committed in 8,907 ms | kept open after 5,012 ms; re-Apply committed in 8,515 ms |
| 4000 × 3000 16-bit Levels (Black 0.1, `commit(op:)`) | committed in 1,155 ms | — |

The 16-bit rows' existing Apply stages are unchanged and all committed. The run took 891 s against
`verify.py`'s 1005 s bound (670 s before): the new stages add about 220 s, so the bound is now 1337 s
(1.5 × 891 s).

**Drive** (own background instance, port 4993; a 6000 × 4000 8-bit RGB noise + gradient PNG; per row
`open_document`, render SHA-256, `apply_filter`, `undo`, render SHA-256, `close_document`; no
`[AutomationAlerts]` line):

| Row | `apply_filter` | `changed` | Hash after undo |
|---|---|---|---|
| `reduce_noise` `{}` | 2.2 s | true | equal |
| `dust_and_scratches` r2 | 8.6 s | true | equal |
| `unsharp_mask` r1.5 | 1.1 s | true | equal |
| `high_pass` r3 | 1.6 s | true | equal |
| `box_blur` r5 | 0.9 s | true | equal |
| `surface_blur` r5 T15 | 14.8 s | true | equal |
| `add_grain` seed 1 | 1.3 s | true | equal |

**Residual.** The slot is free at Apply, then a holder takes it during the rasterize prompt and keeps it past
`ByteFilterAdmission.run`'s own 5 s wait: Apply is then refused (`busyMessage`) after the sheet was
dismissed, as every 16-bit commit is. The post-job write and projection still
run on main (the Apply gap above; the `PreparedDocumentEdit` pattern is out of scope), and 8-bit Gaussian Blur
(`rz_image_blur`) stays synchronous.

## Open items (verified 2026-09-26)
- `DONE` — Native scalar-target and Quick Mask editing (handed to 9G) — `f457cdb`.
- `TRACKED` — P7-12/9I and the Phase 9 gate → carryover P7-12.
- `DONE` — the 8-bit `rz_image_filter_op` arm has a core plan and host admission ([section above](#the-byte-filter_op-plan-and-admission)).
- `DONE` — the 8-bit `rz_image_filter_op` Apply and `apply_filter` run as a cancellable job ([section above](#the-byte-filter_op-apply-as-a-job)).
