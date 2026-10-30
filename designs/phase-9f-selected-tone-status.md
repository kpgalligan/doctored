# Selected tone and scoped analysis (ADR)

- **Status:** Accepted — closed (verifies P7-06, P7-07)
- **Phase / dates:** 9F, September 2026; issue records to 2026-10-04
- **Originals:** [archive/phase-9f-selected-tone-status.md](../archive/phase-9f-selected-tone-status.md) — the phase's full history, to 2026-09-26; [archive/2026-10/phase-9f-selected-tone-status.md](../archive/2026-10/phase-9f-selected-tone-status.md) — this doc as of 2026-10-07, with every measurement table and issue record
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
**Box Blur**, **Surface Blur** and **Add Grain** ship as destructive Filters items at 8 and 16 bits; the Filters
menu was regrouped into submenus.
- **Placement.** One `SpatialKernel` (`core/src/ops_spatial.rs`) serves `SelectedFilter::Spatial` (16-bit layers,
  masks, colour/channel planes) and the byte `dr_image_filter_op`, so a byte-origin 16-bit result is within one
  code of the 8-bit one. The sheets record `apply_filter` steps (Actions and ⌃F replay them); the MCP twin is
  `apply_filter`. Float documents refuse these as they refuse Gaussian Blur.
- **Box Blur**: premultiplied mean of a `(2r+1)²` clamped-edge window (integer radius 1–1000; the sheet shows
  1–250) by sliding sums, so cost does not depend on the radius. It filters alpha.
- **Surface Blur** (`sampling_surface.rs`): the exact tent filter per channel, `out = Σα·w(v−c)·v / Σα·w(v−c)`,
  `w(d) = max(0, 1 − |d|/(2.5·T))`, `d` and `T` in 8-bit levels, radius 1–100, threshold 2–255; exact at 8 bits,
  below one level of tent slope at 16. Colour only; alpha is exact and alpha-0 pixels are untouched.
- **Its work charge** has two coefficients (slide and evaluate counts), because one constant refused every 24 MP
  photo at the default threshold: **`SURFACE_SLIDE_WORK_PER_1024` = 644, `SURFACE_EVALUATE_WORK_PER_1024` = 1826**,
  the largest pair over five agreeing probe runs × `SURFACE_SAFETY_FACTOR` 1.15 (the `#[ignore]`d probe in
  `core/tests/spatial_filters.rs`). At 24 MP the charge is 2.18e10–2.22e10 of the 2.4e10 `MAX_DESTRUCTIVE_WORK` at
  T = 15. A filter run as a job has no work ceiling at any size, admitted by memory alone; the work constants
  remain the charge of an uncontrolled call. The seven-run table is in the archived original.
- **Add Grain**: a positional N(0,1) field from `SplitMix64::at(seed, x, y, 0)`, blurred by Size and mixed by
  Roughness at constant RMS, added as `amount·GRAIN_RMS_PER_AMOUNT·g·4Y(1−Y)` per channel (`GRAIN_RMS_PER_AMOUNT`
  frozen at 0.001: Amount 100 is 25.5 levels at mid-grey, zero at black and white). Colour only; amount 0 is the
  exact bypass.
- **Since:** grain in RAW develop is the last development stage, seeded from the camera file's SHA-256. Live-stack
  versions of these filters are not built.

## Radial Blur, Clouds / Difference Clouds, Offset
**Radial Blur…**, **Clouds** / **Difference Clouds** and **Offset…**: destructive 8/16-bit filters on layers,
masks, colour planes and channels, selection-scoped, recorded, ⌃F-repeatable, reachable through `apply_filter`.
- **Placement.** Three more `SpatialKernel` arms (`sampling_radial.rs`, `render_clouds.rs`, `sampling_offset.rs`):
  one arithmetic at both depths, no new export. Sheets are `SliderSheetController` factories with
  `FilterSheetAccessory` controls; Clouds have no dialog.
- **The frame.** Offset wraps within, and Radial Blur's centre is a
  fraction of, the bounding box of the plane pixels the selection covers, or the whole plane with no selection.
  It is never recorded: a replayed or ⌃F step takes the selection and placement at run time. At 16 bits the core
  derives it from the u16 coverage (`ops_spatial::coverage_frame`); at 8 bits the host passes `region_*`
  (`FilterFrame.swift`). The two agree except where an anti-aliased edge sample quantizes to byte 0.
- **Radial Blur**: the equal-weight premultiplied mean along each pixel's path about the centre. Spin is an arc
  of `amount` degrees, Zoom a ray of `amount × 0.5%` of the radius, both symmetric about the pixel. Good is
  bilinear at one sample per pixel of path (cap 64), Best at two (cap 128), Draft nearest at a quarter (cap 16)
  with deterministic jitter, so a replay is identical. Alpha is filtered.
- **Clouds**: eight octaves of seeded value noise, contrast gain 2 or 4 (⌥, high contrast), mixed
  `fg + (bg − fg)·t` on unit bytes with no gamma conversion. Clouds writes `[mix, 1]`; Difference Clouds writes
  `|source − mix|` on colour and keeps alpha. The menu items use the swatches and a fresh seed; the recorded step
  carries the seed and both colours.
- **Offset**: `out(x, y) = src(x − h, y − v)` inside the frame; outside it Repeat clamps to the frame's edge, Wrap
  is `rem_euclid` over the frame, Transparent is `[0,0,0,0]`, Fill is `[f,f,f,1]`. Codes move exactly. **Set to
  Background** is resolved by the host (`FilterFrame.kernelParameters`): on a layer the vacated area is
  transparent and the sheet says Set to Transparent (there is no Background layer kind); on a mask white; on a
  colour plane that component of the background colour; on the alpha plane or a saved channel its Rec. 709 gray.
- **Work constants**: `RADIAL_BILINEAR_WORK_PER_SAMPLE` = 37, `RADIAL_NEAREST_WORK_PER_SAMPLE` = 33,
  `CLOUDS_WORK_PER_PIXEL` = 86, `OFFSET_WORK_PER_PIXEL` = 34: the costlier depth's units, the largest over two
  runs of `radial_clouds_offset_calibration_probe`, × 1.15. At 24 MP Clouds (2.06e9) and Offset (8.16e8) are
  admitted up to the 100 MP pixel cap, and Radial Draft is admitted at Amount 100 (1.35e10; a whole 45 MP Draft
  plane is over it). Radial Good at Amount 10 (5.77e10) and Best (1.15e11) are over it for a whole 24 MP plane;
  none of these is a size limit for a job at any size (the work constants remain the charge of an
  uncontrolled call). Under a selection the 8-bit charge is the frame's own tiles;
  the 16-bit plan is frame-unaware by design, an upper bound for any frame.
- **Memory.** No plan term was raised: the byte plan is at least 1.25× the measured peak at 24 and 45 MP, and the
  16-bit `selected_edit_plan` (34n + 6 BK) covers a structural 32n (gate ≥ 1.0×). The 45 MP 8-bit commit-stage
  ratio of 1.247–1.249× was reviewed; nothing changed.
- **Preview.** Radial Blur and Offset show a proxy first (offsets divide by the factor). A refused exact
  stage leaves the proxy on screen with the work sentence.
- **Maintainer decisions.** Clouds are not seamlessly tileable (tile-aligned Clouds is not built).
  The centre picker shows the image; a 16-bit mask's picker frame falls back to the
  layer's rect. `apply_filter`'s colour defaults are #000000 / #ffffff, not the swatches. ⌃F on Clouds draws and
  records a new seed; an Action keeps its recorded one. The constants were not compared with Photoshop renders.
  The 24 MP Good / Best refusal stands. No 32-bit float. No UI drive was run on the sheets.
- The calibration, memory-gate and drive tables are in the archived original.

## Staged, cancellable slider previews (2026-10-02)
The heavy slider-filter sheets show a downscaled **proxy** first, then the exact full-resolution preview, and every
superseded exact render is **cancelled**, at 8 and 16 bits. Apply and MCP `apply_filter` never see a proxy.
- **The `dr_image_filter_op` fold.** `dr_image_filter_op` dispatches seven ops: `box_blur`, `surface_blur`, `add_grain`,
  `unsharp_mask`, `high_pass`, `dust_and_scratches`, `reduce_noise`. The four photographic byte exports are
  deleted end to end. Parameters go through `SelectedFilter::parse`, so keys, ranges and defaults are the 16-bit
  selected filter's (unsharp_mask `radius` 1.5, `amount` 1, `threshold` 0; high_pass `radius` 3;
  dust_and_scratches `radius` 2, `threshold` 0; reduce_noise `radius` 1, `luminance` 0.01, `detail` 0.5, `color`
  0.5). Output is byte-identical; only refusal wording changed. Gaussian Blur kept `dr_image_blur` until it
  became the op `blur` (below).
- **Cancellation.** At 8 bits `dr_image_filter_op` takes an optional single-use `control` (`DrJobControl`): each
  exact preview gets a fresh one and the next request cancels it. A cancel answers "operation cancelled";
  controlled output equals uncontrolled (`core/tests/filter_preview_cancel.rs`). At 16 bits
  `PrecisionAdjustmentSession.requestEdit` already cancelled. The 8-bit Gaussian preview is now cancelled the
  same way (`dr_image_filter_op` `blur`).
- **Proxy scale.** Only above 4,194,304 px: `k = ceil(sqrt(W·H / 262,144))`, the proxy `W/k × H/k`, the factor
  `W / proxyWidth`; `dr_doc_resize` (bilinear) reduces the document and `dr_selection_resize` the selection.
- **Parameters.** Radius-like values divide by the factor (Gaussian, Unsharp Mask, High Pass, Reduce Noise, Dust
  & Scratches `r/f`; Box and Surface Blur `max(1, round(r/f))`); amounts and thresholds are kept.
- **Which sheets.** Those seven get a proxy; Add Grain, Add Noise, Pixelate, Hue Rotate, Threshold and Posterize
  do not. Every `SliderSheetController` sheet gets the generation filter. No proxy at 32-bit float, on 16-bit
  scalar targets or in Quick Mask; tone sheets are out of scope.
- **Ordering** (`StagedPreview`, `ProxyPreview.swift`). Each request bumps a generation and older deliveries are
  dropped. A generation's exact is queued only once its proxy has landed, so a drag runs proxies only. Proxy
  sources are built once per sheet off main (`ProxySourceLoader`). Color Samplers read "—" under a proxy.
- Typed geometry previews are not covered here; a 16-bit ⌘T or Crop straighten has its own reduced-resolution
  preview (`GeometryProxyPreview.swift`, under Open items).
- The before/after latency table is in the archived original (24 MP 8-bit Surface Blur: first visible update
  17.0 s → 0.5 s).

## Banded 16-bit Surface Blur / legacy Gaussian (2026-10-03)
The 16-bit layer path of a selected edit builds the legacy Gaussian's and Surface Blur's f32 plane one
`BAND_ROWS` (128-row) band at a time, bit-identical to the whole plane (`selected_filter::plane_band`,
`sampling_surface::surface_band`). The plan charges one band plus the n×8 tiled copy `rebuilt_rgba16` makes. The
scalar mask/channel path and the byte `filter_op` keep the whole plane. Banded 24 MP plans: Gaussian σ2
456,991,744, Surface Blur r5 464,968,704.

## Honest Swift-side charges (2026-10-03)
A 6000 × 4000 16-bit layer gets the Surface Blur and Gaussian sheet previews and an Apply that commits whenever
it is pressed.
1. **Core.** `selected_edit_plan` charges the n×8 copy `rebuilt_rgba16` makes: additive on the tone, auto and
   banded arms, and max(kernel scratch, held plane + codes + copy) on the whole-plane arm. The plan JSON gains
   `output_bytes` (n×8).
2. **A preview reserves the max of its sequential phases**:
   `PrecisionCapturedEdit.previewReservationBytes(display:)` is max(scratch, output + display) for a selected
   edit and the sum for every other operation.
3. **A coverage lease exists only for an insertion's mask** (`.create` with a selection, n×2). A destructive
   edit's coverage is its own `PrecisionScalarBuffer` payload, which the core plan already charges.
4. **Supersession.** `StagedPreview.request(proxy:makeExact:)` runs the exact's plan, lease and reservation on
   main only once its proxy has been delivered and the exact renderer is idle, so two exact reservations never
   coexist. `supersede()` drops not-yet-run closures. The display stage polls its own `RasterJobControl`.
5. **Apply drains the sheet first.** `commit(filter:)` and `commit(op:)` call `drainForCommit()`: cancel, wait for
   both renderers and the proxy source build, drop the sampler projection.

Every caller of the session gets the deferral and the drain (the 16-bit sheets, the menu, MCP `apply_filter`,
the precision paint path); `ByteFilterPreview` still uses `request(proxy:exact:)`. **Bound later batches hold:**
at the measured 24 MP Apply retained set (487,117,150 B after the drain) a 16-bit plan admits up to
1,660,366,498 (`APPLY_BOUND` in `core/tests/banded_reduce_noise.rs`). The attribution, probe and footprint
tables are in the archived original.

## A radius-independent legacy Gaussian (2026-10-03)
From σ4 up (`LEGACY_FAST_SIGMA`) the legacy Gaussian family (8-bit `dr_image_filter_op` `blur`, formerly
`dr_image_blur`, / `dr_image_sharpen`, 16-bit
`blur` / `sharpen` selected edits) runs Deriche's 4th-order recursive Gaussian
(`core/src/sampling_gaussian_fast.rs`). Cost per pixel does not depend on σ, so a 24 MP 16-bit layer admits every
σ the slider offers, up to 64. Below σ4 the output is byte-identical to before.
- **How it was chosen.** Extended box K = 3 and K = 5, Young–van Vliet and Deriche were measured on premultiplied
  unit-scale RGBA over four 512² fixtures at both depths and 15 σ values, against the legacy kernel
  (`image::imageops::blur`) and Photoshop 26.10.0. Eligible: max error ≤ 1/255 and mean ≤ 0.1/255 against the
  legacy kernel from the switch up, the switch ≤ 6, within P(exact, r) + 0.25/255 of Photoshop, finite at σ = 1000
  and f32::MAX. Only Deriche qualifies, switching at σ4. Its largest error against the exact 3σ kernel at σ ≥ 4
  is **0.6409/255** (quadrant step, σ64), which sets the tests' bounds (2 codes at 8 bits, 166 at 16). The spec's
  "live" clause against the 3σ-truncated kernel was ruled mis-specified and re-derived against an
  untruncated reference ([phase 9](phase-9-live-filters-status.md#live-filters-at-24-mp-both-depths)).
- **Photoshop.** Its radius is σ, about 2% wider (best-fit σ 12.29 at r = 12), and its large-radius Gaussian
  looks like an iterated box; Gaussian Blur now runs an iterated box from r 7 up, fitted to the takes
  (next section). The error tables, fixture hashes and Photoshop manifest are in the archived original; the
  probe is the `gaussian_blur` job in `scripts/photoshop-probe/photoshop.jsx` with
  `scripts/photoshop_gaussian_reference.py`.
- **Implementation.** `sampling_gaussian_fast::band` runs each 128-row tile from its own window of ±(ceil(4σ)+4)
  rows, so a band is bit-identical to the whole plane. `MAX_SIGMA` is 1000: `plan` and `band` answer Err above it
  and `dr_image_filter_op` `blur` refuses ("sigma must be finite in (0,1000]"), so no σ runs as a smaller one;
  `apply_filter` `blur` refuses a sigma above 1000 at 8 bits by name, and the 16-bit edits stop at 64.
  `sampling::blur` runs every σ band by band under the caller's job and a band that cannot allocate is its `Err`
  (there is no whole-image fallback).
- **Charges.** `FAST_WORK_PER_PIXEL` = 56 (48.4 measured × 1.15), below the 24 MP ceiling at σ64 (203), so there is
  no σ ceiling. The byte arm runs under the job with its own plan (`sampling::blur_scratch_bytes`, the record
  there). Tests: `core/tests/gaussian_radius_independent.rs`.

## Gaussian Blur from radius 7 (2026-10-13)
Filter ▸ Blur ▸ Gaussian Blur (`blur`: 8-bit `dr_image_filter_op`, the 16-bit edit, the float route; sheet,
Actions, MCP) runs `core/src/gaussian_blur_filter.rs` from Radius 7 up: K = 4 Gwosdek extended boxes per
axis at σ = 0.99·r, pad-once clamp-to-edge, banded like the fast filter. Below 7 the old kernels stay. The
legacy Sharpen, Unsharp Mask, High Pass, Smart Sharpen, feathering, styles and Surface Blur keep theirs; the
takes measured the box only in Gaussian Blur.
- **Takes** (`scripts/photoshop-probe/blur_wand_take.py`, receipts `blur-wand-v1x/take1c`, `take2`; take1 and
  take1b aborted and measured nothing). take1c fitted EB(4, 0.99) as the one member within 1 code at every
  fitted radius from 3 to 100. take2 (fresh 30° edge, zone plate, noise): 8-bit max **1 code at r 9, 14, 20,
  40, 75, 250** (the bound); r 3.6 / 4.5 max 3 / 2 (closer than the old kernel's 5 / 4), r 6.5 max 3 (the old
  kernel's 3, mean 0.139 vs 0.171: not closer by the declared 0.05), r 1.3 max 7 (= old). The one-threshold
  rule puts the law's start above 6.5, at the fitted radius 7. 16-bit max/mean at r ≥ 9: 61–103 / 2.3–12.0
  (of 32768), reported, not gated.
- **Memory.** 24 MP plans against core E (`gaussian_blur_filter::plan`'s record): 8-bit r25/r64 1.348× /
  1.341×, 16-bit 1.255× / 1.330×, float ≥ 1.333×. The app drive
  (`scripts/gaussian-blur-admission/measure.py`) passes M8, M16 and M32. M16 was re-declared after its first
  run, which held the call against the edit plan alone and failed (E 576.7 MB, the same at r6 on the old
  kernel: the edit's output plus the 16-bit display preparation). Like M8 it now holds the call against what
  the host admits, max(plan, `projectionScratchEstimate()` 795,586,399) + dR = 988,652,495 (1.714× /
  1.715×). The same probe found the recursive filter's 16-bit plan
  at σ6 1.226×, so `sampling_gaussian_fast::plan` now charges fourteen allowances (was five). Recorded
  `blur` Actions at r ≥ 7 render the box now.

## Reduce Noise statistics in row rings, a measured rank charge (2026-10-03)
Reduce Noise's guided filter keeps its window statistics as streamed row rings instead of 6–8 whole n×8 planes,
bit-identical. A 6000 × 4000 layer is admitted at 8 and 16 bits by the sheet, `apply_filter` and the live
filter's core plans. Dust & Scratches' and Reduce Noise's work charges are re-calibrated against measured time.
- `core/src/sampling_box.rs` is the ONE clamped-edge sliding box sum. `BoxRows` takes input rows in order
  (`push`) and hands back output rows in order (`emit`), holding a ring of min(2r+2, h) rows and one running
  column; `box_sum` (Box Blur) is its whole-plane wrapper.
- `sampling_denoise::guided_bounded` runs each active half as its own pass through `BoxRows` (luminance, then
  chroma; the peak is the larger half). Progress: luminance in [0, ½], chroma in [½, 1].
- **Plan** (`plan_bounded`): n×16 + max(luma ? `half_bytes`(w,h,rl,5,2) : 0, chroma ? `half_bytes`(w,h,rc,11,6) :
  0) + 4 BK, `half_bytes(w,h,r,boxes,rows)` = boxes·(min(2r+2,h)+1)·w·8 + (min(r+1,h)+rows)·w·8. At 6000 × 4000:
  403,362,304 at the defaults (was 1,924,194,304 at every radius).
- **Work charges** (the `#[ignore]`d probe in `core/tests/banded_reduce_noise.rs`): `RANK_NEIGHBOUR_WORK` = 16,
  `RANK_PIXEL_WORK` = 364 (were 24 and 24); `DENOISE_LUMA_WORK` = 139, `DENOISE_CHROMA_WORK` = 326 (were 44 and
  92). Each is the costlier depth's measured figure × 1.15. At 24 MP Dust & Scratches' ceiling stays r3.5 (r3.75
  refuses by work) and Reduce Noise is admitted at every radius 0–64.
- **Footprint gates the tests hold.** The 16-bit selected `reduce_noise` default plan, 822,291,456, is at least
  its measured peak 753,877,544 (`S16_FLOOR`; gate 1.0×, the arm's allocations are fixed-size) and at most the
  Apply bound. The live plans are at least 1.25× their peaks (964,444,880 at 16 bits, 960,676,512 at 8). Pins:
  one SHA-256 per path, captured on the unchanged tree (`a4a5859`).
- **A user-visible cost.** The raised charges lowered what every operation that re-renders a live-filtered layer
  under the interactive `MAX_WORK_UNITS` (2e9) admits. Ceilings before → after, in MP, for ⌘T bicubic / ⌘T
  Lanczos3 / Replace, RAW redevelop and Convert to Profile: Reduce Noise defaults 8.6 → 3.6, 6.7 → 3.2,
  11.5 → 4.0; Dust & Scratches r1 8.8 → 3.8, 6.8 → 3.4, 11.8 → 4.3; r2 4.8 → 3.1, 4.1 → 2.8, 5.5 → 3.3. No charge
  was lowered to keep them. They are restored, charges unchanged, by
  [the phase-9 "Operations on a live-filtered layer" section](phase-9-live-filters-status.md#operations-on-a-live-filtered-layer).
- The 16-bit live Reduce Noise over an opened 24 MP photo was refused by the host, 62,200 bytes over the 2 GiB
  reference limit; it is admitted under the RAM-scaled working limit.
- The calibration, footprint, probe and drive tables are in the archived original.

## The byte filter_op plan and admission
The seven 8-bit filters behind `dr_image_filter_op` have one core plan, `ops_spatial::filter_op_scratch_bytes`
(`dr_image_filter_op_scratch_bytes`, `RasterImage.filterOpScratchBytes`), and the host admits it before the
filter allocates: the sheet's exact preview (`ByteFilterAdmission.reservePreview`, a `SelectionWorkBudget`
reservation), sheet Apply and MCP `apply_filter` (`ByteFilterAdmission.admit`, plan + the document's retained
bytes), and so replayed Actions and ⌃F. A refusal carries admission's sentence: on the sheet's status line
(`tone.preview.status`), at Apply, or in-band from `apply_filter`. Outputs are byte-identical (pins in
`core/tests/byte_filter_op_plan.rs`).
- `ops_photographic::apply` reads the caller's bytes in place through a 256-entry table of `f32::from(v) / 255.0`
  instead of holding an n×16 straight copy (bit-identical, within 1.11× of the copy's speed).
- **The plan**, refused exactly as `filter_op` refuses (one private `dispatch`): K is the kernel's own plan; base
  = max(K, 16n + 4n) + 4 BK; scratch = base + ⌈base / 4⌉. 0 is an exact bypass.
- **Gate F**: every plan is at least ⌈1.25 × max E⌉ and no ratio exceeds 2.0, E being the largest of three
  `ri_interval_max_phys_footprint` rises of an uncontrolled byte `dr_image_filter_op` on opaque noise, one case
  per process. At 6000 × 4000 most cases measure about 480.3 MB against a 605,242,880 plan (1.260×).
  `core/tests/byte_filter_op_plan.rs` holds the asserted figures; the gate F and gate T tables are in the
  archived original.
- **Box Blur.** `box_plane` holds one sum at a time, bit-identically (kernel peak 24n + ring). Its plan is
  24n + ring + 2 BK (`BOX_BLUR_BYTES_PER_PIXEL`); the 16-bit selected plan is 34n + ring + 8 BK.
- **The commit stage.** The commit and first projection after every 8-bit filter are the host's own stage
  (`ByteFilterAdmission.commitStageBytes`: 8n + the projection with `ProjectionMargin`'s 7/16). Preview and Apply
  both admit max(plan, `commitStageBytes`). `ByteFilterPreview.drain()` runs before Apply admits, because a live
  reservation would be charged twice. The proxy stage is not reserved.
- **Ceilings.** The 8-bit filter-commit ceiling is about 157.6 MP on a 16 GB Mac's 8 GiB (54.5 B/px;
  it was 53). At 45 MP, 8-bit Dust & Scratches r2 and Surface Blur r5/T15 run as jobs, admitted by memory
  alone; an uncontrolled call keeps `MAX_DESTRUCTIVE_WORK`.

## The byte filter_op Apply as a job
Sheet Apply and MCP `apply_filter` for the seven 8-bit `dr_image_filter_op` filters run on the `BackgroundWork`
worker under a `RasterJobControl`, with the progress panel, Cancel and Esc (`ByteFilterAdmission.run`, the
`PrecisionCapturedWork.run` pattern). Admission, undo, dirty tracking, the recorded step and the outputs are
unchanged. A plan of 0 (exact bypass) runs inline and needs no slot. A cancel surfaces as `JobCancelled`: nothing
is shown, recorded or added to history. `apply_filter` still waits for its result and returns no `job_id`.
- The bounded slot-wait pump lives in `WorkerSlotWait.swift` (`BackgroundWork.waitForAnonymousOccupant`,
  `anonymousWaitApplies`, `busyMessage`); MCP's predicate, limit, slice and exempt set are unchanged.
  `ByteFilterAdmission.run` waits before its `WorkerTask`; the 16-bit `commit(filter:)`, `commit(op:)` and
  `PrecisionScalarAdjustmentSession.commit` wait before building their edit.
- `SliderSheetController.applyClicked` (both depths) waits BEFORE `drain()` and `dismiss`. If the slot is still
  held it shows `busyMessage` on its status line, beeps and stays open with its values.
- The job runs inside `applyEdit`'s transform, after `isCurrent`, the rasterize prompt and `publicationRefusal`.
- The wait never covers a cancellable occupant (`DocumentJobs.active`, a Content-Aware Fill review) and does not
  run during an Action replay, ⌃F or a batch: there a byte Apply or `apply_filter` answers `busyMessage` at
  once. `apply_filter` passes `waitForSlot: false`, because `execute`'s slot wait already covered the call.
- `apply_filter`'s job can service other calls, so the handler re-checks the editing state
  (`isCurrentEditingState`) and refuses with "The document changed before the edit could be applied".
- Core test: `every_byte_filter_op_reports_progress_and_cancels_mid_run`. The jobs probe's "slider preview
  latency" case holds the cancel, busy-slot and Apply stages; `verify.py`'s bound for it is 1337 s (1.5 × 891 s).
  Its latency rows are in the archived original.
- **The byte Apply job's residual** (all three items shipped): 8-bit Gaussian Blur is `dr_image_filter_op` `blur` — `dr_image_blur`,
  `RasterImage.blurred` and `BlurSheetController` are gone, the sheet is `SliderSheetController.gaussianBlur`,
  and `sampling::blur` runs band by band under the job with byte-identical output
  (`core/tests/byte_gaussian_blur_op.rs`). Its plan is the n×4 output plus one band, plus 4 BK, plus a quarter
  (no 20n combine stage). Every 8-bit sheet's Apply and Render ▸ Clouds go through
  `ByteFilterAdmission.runPrepared` (`ImageDocument.applyCapturedByteFilter`): the job also writes the result
  and prepares the first projection on the worker. `apply_filter` keeps `ByteFilterAdmission.run`.
  Item 3: when a holder takes the slot after the sheet's own slot wait, behind the
  rasterize prompt, Apply's refusal is not presented. `BackgroundWork.isBusyRefusal` recognises it (`busyMessage`
  from `WorkerTask` at 8 bits, `preparingBusyMessage` from the typed edit constructors at 16), and the commit's
  `onLateBusy` has `SliderSheetController` present a fresh sheet from the same arguments, sliders at the refused
  positions and `busyMessage` on its status line (jobs probe "late busy reopen"). Render ▸ Clouds and the
  bespoke adjustment sheets (`commit(op:)`: Levels, Curves, the phase-5 sheets) pass no handler and still alert.
  `BackgroundWork.wait` draws one more frame from the control before a successful return, so the job panel's
  last frame is the job's final progress. The latency probe's `Apply gap` (about 270 ms at 24 MP) did not move: it is the
  sheet's own closing animation, which `dismiss(self)` runs synchronously before the job starts; the stretch
  after the job, which held the write and the projection, is what fell (the probe prints both).

## Open items (verified 2026-10-07)
- `DONE` — Native scalar-target and Quick Mask editing (handed to 9G) — `f457cdb`.
- `TRACKED` — P7-12/9I and the Phase 9 gate → carryover P7-12.
- `DONE` — the 8-bit `dr_image_filter_op` arm has a core plan and host admission ([section above](#the-byte-filter_op-plan-and-admission)).
- `DONE` — the 8-bit `dr_image_filter_op` Apply and `apply_filter` run as a cancellable job ([section above](#the-byte-filter_op-apply-as-a-job)).
- `DONE` — a 16-bit ⌘T or Crop straighten whose preview the interactive work cap refuses previews at reduced resolution, sized from the commit plan's `work_units` (`GeometryProxyPreview.swift`).
- `DONE` — Gaussian Blur from radius 7, measured against Photoshop ([section above](#gaussian-blur-from-radius-7-2026-10-13)).
- `DONE` — 45 MP 8-bit Dust & Scratches r2 and Surface Blur r5/T15 run as jobs with no work ceiling under a control; uncontrolled calls keep `MAX_DESTRUCTIVE_WORK`.
- `OPEN` — live (smart) filter versions of Box Blur, Surface Blur, Add Grain, Radial Blur and Offset.
- `OPEN` — 32-bit float documents refuse every destructive filter.
- `DONE` — the byte Apply job's residual: 8-bit Gaussian Blur is a planned, cancellable `dr_image_filter_op` `blur` job, the 8-bit sheets' write and first projection run on that job's worker, and (item 3) a late busy refusal after a slider sheet closed brings the sheet back with its values at 8 and 16 bits ([section above](#the-byte-filter_op-apply-as-a-job)).
- `OPEN` — the Radial Blur and Offset sheets and ⌃F on Clouds have no UI drive or recorded hand check.
