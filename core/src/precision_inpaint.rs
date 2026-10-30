//! Native-depth synthesis using the shipped component planner and PatchMatch
//! pyramid/search/vote. Float RGB remains in native u16 code units through the
//! membrane solve; the destination quantizes exactly once. The source is an
//! immutable, document-profile canvas plate supplied by the retouch dispatcher.
//! This module knows nothing about layer creation or source-mode composition.
//!
//! # Admission
//!
//! [`estimate`] charges `8 MiB + 2n + min(covered,
//! INPAINT_SEARCH_PIXELS)·COMPONENT_BYTES + window·WINDOW_BYTES`, on top of
//! the typed retouch plan's own canvas, output and plate terms. The record
//! clamp counts every record there is: [`plan`] refuses a call whose
//! `doc_heal::component_count_bound` is over `INPAINT_SEARCH_PIXELS` before
//! the walk (the covered count itself is no longer capped, see
//! "Lifted holes" below). Both per-unit figures were measured
//! the way `doc_content_aware`'s `# Admission` measured the byte kernel:
//! `/usr/bin/time -l` "peak memory footprint", one operation per process,
//! each case run 3 times and the maximum kept, plus the footprint sampled
//! in-process (`proc_pid_rusage` `ri_phys_footprint`) immediately before the
//! edit call, so `E = peak − pre-edit` is the edit's own footprint with the
//! fixtures excluded.
//!
//! **`WINDOW_BYTES`.** A 4000×3000 u16 document (`plasma·257`, alpha
//! 65535), coverage 65535 in the 1000×1000 box at (1500, 1000),
//! `content_aware_fill` through `dr_precision_doc_edit_plan` then
//! `dr_precision_doc_edit` (limit `REFERENCE_WORKING_BYTES`, other 0, NULL control).
//! The fixtures' baseline peak (plan only) was 201,998,816 bytes and the
//! pre-edit footprint 201.90–202.00 MB in every run. `scratch_bytes` is the
//! plan's figure excluding the 96,011,451 retained bytes:
//!
//! | case | window W | `scratch_bytes` at 320 | at 72 | peak (max of 3) | pre-edit | E |
//! |---|---|---|---|---|---|---|
//! | ring 48 | 2,452,356 | 1,725,725,440 | 1,117,541,152 | 660,964,192 | 201,933,256 | 459,030,936 |
//! | ring 400 | 3,240,000 | 1,977,771,520 | 1,174,251,520 | 702,055,288 | 201,949,640 | 500,105,648 |
//! | ring 0 | 4,096,576 | refused: needs 2,347,887,291 with retained | 1,235,924,992 | 709,591,928 | 201,916,872 | 507,675,056 |
//! | ring 48, new layer | 2,452,356 | refused: needs 2,210,979,771 with retained | 1,506,784,032 | 1,109,050,544 | 201,949,640 | 907,100,904 |
//!
//! The ring-48 and ring-400 peaks were 654,426,976 / 656,196,448 /
//! 660,964,192 and 693,912,440 / 696,615,800 / 702,055,288 at 320, and
//! 655,639,392 / 658,015,072 / 657,490,784 and 696,697,720 / 698,778,488 /
//! 696,288,144 at 72 (the constant changes admission only, not the kernel).
//! The ring-0 and new-layer rows ran at 72, the first figure that admits
//! them; ring 0 at 72 peaked at 709,591,928 / 703,841,144 / 707,412,856 and
//! the new layer at 1,109,050,544 / 1,103,692,952 / 1,104,217,240. The
//! slope of peak against window pixels is (702,055,288 − 660,964,192) /
//! (3,240,000 − 2,452,356) = 52.2 B per window pixel (ring 48 → 0: 29.6;
//! ring 400 → 0: 8.8; the widest pairing of single runs, 654,426,976 against
//! 702,055,288: 60.5). It includes the destination rect read and write; the
//! plan charges no other per-window term, so nothing is subtracted. 320 is
//! more than 1.5·52.2, so the figure drops to the smallest multiple of 8 at
//! or above 1.25·52.2 = 65.2: **72**. Every case that ran stays inside its
//! plan (E ≤ `scratch_bytes` at 72, by 0.60–0.73 GB), and a 1 MP fill on a
//! 12 MP 16-bit photograph — ring 0, the automatic band — is now admitted.
//! `WINDOW_BYTES` stays ≥ 24, `inpaint_plan::plan`'s nested `base + 24·window`.
//!
//! The 12 MP canvas's whole-canvas plan terms could hide an intercept that
//! scales with the hole or the canvas, so the same 1000×1000 hole was also
//! measured where canvas ≈ window ≈ hole, at 72, same method. On 1100×1100
//! (hole at (50, 50); ring 48 settles at 283 and ring 0 at 565, so both
//! windows clamp to the canvas, W = 1,210,000; retained 9,690,115; baseline
//! peaks 24,330,600 / 23,118,160 / 23,134,544) and on 2000×2000 (hole at
//! (500, 500), ring 48, W = 1566² = 2,452,356; retained 32,010,427; baseline
//! peaks 69,681,536 / 69,697,920 / 69,697,920):
//!
//! | case | `scratch_bytes` | peaks (3 runs) | pre-edit (per run) | E (per run) | plan / max E |
//! |---|---|---|---|---|---|
//! | 1100², ring 48 | 315,951,520 | 185,483,768 / 178,553,336 / 179,798,520 | 23,052,624 / 23,085,392 / 23,118,160 | 162,431,144 / 155,467,944 / 156,680,360 | 1.945 |
//! | 1100², ring 0 | 315,951,520 | 182,092,280 / 179,290,616 / 182,764,024 | 23,101,776 / 24,232,272 / 23,085,392 | 158,990,504 / 155,058,344 / 159,678,632 | 1.98 |
//! | 2000², ring 48 | 589,541,152 | 351,912,560 / 355,304,048 / 353,436,272 | 69,632,384 / 69,681,536 / 69,665,152 | 282,280,176 / 285,622,512 / 283,771,120 | 2.06 |
//!
//! Every plan stays ≥ 1.25× its largest footprint (the small canvas's plan
//! is about 261 B per canvas pixel against 134 B of footprint), so 72 stands
//! with no per-hole or per-canvas term added.
//!
//! **`COMPONENT_BYTES`.** A 2000×2000 opaque `plasma` byte document
//! whose overlay is alpha 255 at every `(x, y)` with `x % s == 0` and
//! `y % s == 0`, through `dr_doc_spot_heal_layer_job` (fresh control, ring 0,
//! seed 7, sample 0); every run refuses on the work cap after the component
//! walk, as intended. Baseline (fixtures, no call) peaks 45,744,488 /
//! 45,744,488 / 45,728,104.
//!
//! | case | components K | peaks | pre-edit (max run) | E (max) |
//! |---|---|---|---|---|
//! | s = 4 | 250,000 | 96,567,704 / 95,666,584 / 95,060,376 | 45,695,312 | 50,872,392 |
//! | 2^19 + 1 of the s = 2 grid | 524,289 | 111,149,464 / 111,100,312 / 110,789,016 | 41,845,072 | 69,304,392 |
//! | s = 2 (the parts ceiling) | 1,000,000 | 137,822,640 / 136,167,880 / 136,462,768 | 49,480,016 | 88,342,624 |
//!
//! Slopes per component: from the peaks 55.0 (s 4 → 2), 53.2 (s 4 → 2^19 +
//! 1, across the `Vec` doubling), 56.1 (2^19 + 1 → s 2); from E 50.0, 67.2
//! and 40.0. The largest, c = 67.2, gives 1.25·c = 84 ≤ 128, so 128 stays.
//! The pre-edit footprints differ because the overlays touch different pages,
//! which is why E and not the baseline peak is the per-case figure.
//!
//! **Dense components.** The byte inpaint's reserves no longer charge
//! one record per covered pixel: before the walk they charge
//! `doc_heal::component_count_bound`, after it the walk's count. Its tight
//! case, canvas = window, was measured the same way: a 1100×1100 opaque
//! `plasma` byte document, overlay alpha 255 over (50, 50)–(1050, 1050)
//! (n = W = 1,210,000; 1,000,000 covered, one component), through
//! `dr_doc_spot_heal_layer_job` (strength 1.0, ring 0, seed 7, sample 0, a
//! control from `dr_job_control_create`); every run completes. The byte
//! figure `10n + 4R + 128 + 136·W + 8 MiB` (R, the union rect, is the
//! whole canvas here, so `14n + …`) = 189,888,736 (it was 317,888,608).
//! Peaks 116,294,064 / 119,701,960 / 124,748,256; pre-edit 15,450,424 /
//! 15,483,192 / 15,499,576; E 100,843,640 / 104,218,768 / 109,248,680;
//! post-edit 116,294,064 / 119,685,576 / 124,748,256. Baseline peaks
//! 15,548,752 / 15,515,984 / 15,515,984. 1.25 × 109,248,680 = 136,560,850 ≤
//! 189,888,736 (largest E 0.575 of it), so the byte window's 136 stays. The
//! typed plan keeps `min(covered, INPAINT_SEARCH_PIXELS)·COMPONENT_BYTES`:
//! it has no mask at plan time.
//!
//! **In-place layer term.** The byte inpaint's post-plan reserve
//! charges its destination clone — the windows' union rect R on the layer,
//! read and rewritten as RGBA8 — at `4R`, not the whole layer's bytes, and
//! its earlier reserves (before any clone exists) charge none. That leaves
//! the Current-mode canvas plate (`4n`) to the 10 B canvas term, so it was
//! measured where the canvas dominates, same method: a 6000×4000 opaque
//! `plasma` byte document, overlay alpha 255 over the 100×100 box at
//! (2950, 1950) (every overlay byte written before the pre-edit read),
//! strength 1, ring 0, seed 7, sample current. W = R = 214² = 45,796 (the
//! parked post-plan reserve reads back exactly the figure).
//!
//! - **S1**, `dr_doc_spot_heal_layer_job` under a control from
//!   `dr_job_control_create` (the export refuses NULL): figure `10n + 4R +
//!   128 + 136·W + 8 MiB` = 254,800,176 (350,616,992 with the whole layer).
//!   Peaks 464,241,336 / 461,750,968 / 464,519,864; pre-edit 289,915,384 /
//!   289,866,232 / 289,899,000; E 174,325,952 / 171,884,736 / 174,620,864.
//!   1.25 × 174,620,864 = 218,276,080 (largest E 0.685 of the figure).
//! - **S3**, `dr_doc_spot_heal_to_new_layer` (plate from
//!   `dr_doc_retouch_source`, current-and-below; NULL control): S1's figure
//!   plus the `4n` held plate, 350,800,176. Peaks 655,803,208 / 653,804,360
//!   / 655,721,288; pre-edit 485,130,912 / 484,917,920 / 485,802,680; E
//!   170,672,296 / 168,886,440 / 169,918,608 (0.487).
//!
//! The fill's case (S2) is in `doc_content_aware`'s `# Admission`. Every
//! figure stays ≥ 1.25·E, so the canvas constant stays 10. A first harness
//! that left the overlay's zero pages untouched measured E about `4n`
//! higher (S1 up to 268,255,472, S3 up to 282,034,344): the kernel's alpha
//! read faulted the caller's overlay in, a retained input rather than
//! scratch. Every app path writes its overlay or coverage in full before the
//! call (the UI heal copies it, MCP `spot_heal_stroke` builds it with
//! `[UInt8](repeating:count:)` and draws into it, Content-Aware Fill scans its
//! coverage for the box), so the second harness's fully written overlay is
//! the one that models the app.
//!
//! **Lifted holes.** [`plan`] no longer refuses a call covering over
//! `INPAINT_SEARCH_PIXELS` ("high-depth synthesis covers …"): it is lifted
//! (`doc_inpaint::inpaint_lift`; each window's plan carries the call's lift
//! to `inpaint_em::inpaint_plane`), and [`estimate`] is unchanged. One
//! lifted case was measured against it, `examples/inpaint_lift_admission.rs
//! 16 fill`: a 2600×2600 u16 position-hashed document (alpha 65535), coverage
//! 65535 over the 2000×2000 box at (300, 300) (4 MP, the most lift 1 takes),
//! `content_aware_fill`, ring 0, seed 7, current, output layer, through
//! `dr_precision_doc_edit_plan` then `dr_precision_doc_edit` (limit
//! `REFERENCE_WORKING_BYTES`, other 0, NULL control); the window is the
//! canvas, 6,760,000. Release, one call per process, `MallocLargeCache=0`,
//! `/usr/bin/time -l` "peak memory footprint"; the baseline is the `twin`
//! mode (the same document, coverage, request and plan call, no edit). The
//! plan's `scratch_bytes` is 1,081,851,520. Fill peaks 652,133,168 in each
//! of three runs; twin peaks 123,240,856 / 123,208,088 / 123,240,856.
//! Largest fill less smallest twin E = 528,925,080; 1.25·E = 661,156,350 ≤
//! the plan (E is 0.489 of it), so `WINDOW_BYTES` stays 72. The byte case is
//! in `doc_content_aware`'s `# Admission`.
//!
//! Metadata bounds admit a conservative peak before coverage allocation; exact component/ring
//! planning retains the shipped synthesis caps. This module doc's `# Admission` holds the measured
//! per-unit figures: 72 B per window pixel (was 320) and 128 B per component. The
//! covered count is not capped: a call over `INPAINT_SEARCH_PIXELS` is lifted, and a call
//! in more separate parts than that is refused before the walk (`parts_cap_message`).

use crate::cmyk_image::CmykImage;
use crate::doc_heal::Component;
use crate::doc_inpaint::{parts_cap_message, Caller, INPAINT_SEARCH_PIXELS};
use crate::heal_writeback::HealSample;
use crate::inpaint_plan::ComponentPlan;
use crate::job_control::JobContext;
use crate::patchmatch::Plane;
use crate::precision_image::{pixel_count, PrecisionImage, PrecisionRun};
use crate::spot_heal_type::SpotHealType;

const BOOKKEEPING: u64 = 8 * 1024 * 1024;
/// Includes geometric Vec capacity, component records, pixel lists and
/// planning ladders even for one-pixel components. Measured (module
/// doc `# Admission`): at most 67.2 B per component, across a `Vec` doubling;
/// 128 ≥ 1.25× that. The byte inpaint and heal reserves charge it too.
pub(crate) const COMPONENT_BYTES: u64 = 128;
/// Per pixel of the largest window; windows are evaluated sequentially.
/// Measured (module doc `# Admission`): 52.2 B per window pixel, so
/// 72 = the smallest multiple of 8 ≥ 1.25× that (it was 320). Checked on
/// 1.2 MP and 4 MP canvases too: every plan ≥ 1.94× its footprint.
const WINDOW_BYTES: u64 = 72;

pub(crate) struct InpaintPlan {
    canvas: (u32, u32),
    hole: Vec<bool>,
    components: Vec<Component>,
    windows: Vec<ComponentPlan>,
    soft_write: bool,
    /// The spot-heal Type (Content-Aware for a fill): which synthesis
    /// [`apply`] runs for every component.
    kind: SpotHealType,
    pub(crate) scratch_bytes: u64,
    pub(crate) work_units: u64,
}

fn scratch(canvas_pixels: u64, covered: u64, window: u64, channels: usize) -> u64 {
    BOOKKEEPING
        .saturating_add(canvas_pixels.saturating_mul(2))
        .saturating_add(covered.saturating_mul(COMPONENT_BYTES))
        .saturating_add(window.saturating_mul(window_bytes(channels)))
}

/// [`WINDOW_BYTES`] for a plate of `channels` colour channels: the measured
/// RGB figure scaled by `channels / 3`, rounded up — 72 for RGB exactly, 96
/// for a CMYK document's four inks (#547). Every per-window buffer is at most
/// proportional to the channel count (the pyramid's colour planes, the vote,
/// the membrane's per-channel solve), so the scaled figure over-charges the
/// channel-free masks and the distance field, never under-charges.
fn window_bytes(channels: usize) -> u64 {
    (WINDOW_BYTES * channels as u64).div_ceil(3)
}

/// The sampling plate a synthesis reads (#547): a native RGB plate, or a CMYK
/// document's ink plate, whose four ink codes are the colour channels.
#[derive(Clone, Copy)]
pub(crate) enum SynthesisPlate<'a> {
    Precision(&'a PrecisionImage),
    Cmyk(&'a CmykImage),
}

impl SynthesisPlate<'_> {
    pub(crate) fn dimensions(&self) -> (u32, u32) {
        match self {
            Self::Precision(image) => image.dimensions(),
            Self::Cmyk(image) => image.dimensions(),
        }
    }

    /// The colour channels a pixel carries: 3, or 4 inks.
    pub(crate) fn channels(&self) -> usize {
        match self {
            Self::Precision(_) => 3,
            Self::Cmyk(_) => 4,
        }
    }

    /// Calls `f(x, colour, solid)` for every sample of row `y`, columns
    /// `x0..x1`, in the solve's code units: a u16 run's codes, an f32 run
    /// through `retouch_encoding::encode`, a CMYK run's ink codes; `solid`
    /// is the half-alpha data rule (alpha ≥ 32768, or ≥ 0.5 in f32). `C` must
    /// be [`Self::channels`] (`apply` checks it first).
    fn read_row<const C: usize>(
        &self,
        y: u32,
        x0: usize,
        x1: usize,
        mut f: impl FnMut(usize, [f32; C], bool),
    ) {
        match self {
            Self::Precision(image) => {
                for (sx, run) in image.row_segments(y, x0 as u32, x1 as u32) {
                    match run {
                        PrecisionRun::Rgba16(run) => {
                            for (k, px) in run.iter().enumerate() {
                                let colour = std::array::from_fn(|c| f32::from(px[c]));
                                f(sx as usize + k, colour, px[3] >= 32768);
                            }
                        }
                        PrecisionRun::Rgba32(run) => {
                            for (k, px) in run.iter().enumerate() {
                                let colour =
                                    std::array::from_fn(|c| crate::retouch_encoding::encode(px[c]));
                                f(sx as usize + k, colour, px[3] >= 0.5);
                            }
                        }
                    }
                }
            }
            Self::Cmyk(image) => {
                for (sx, run) in image.row_segments(y, x0 as u32, x1 as u32) {
                    for (k, s) in run.iter().enumerate() {
                        let colour = std::array::from_fn(|c| f32::from(s[c]));
                        f(sx as usize + k, colour, s[4] >= 32768);
                    }
                }
            }
        }
    }
}

fn window_work(pixels: u64) -> u64 {
    // A target can cover the whole window. The 10-EM single-level schedule has
    // 23 search sweeps and 10 distance refreshes; at most 2 propagated plus 27
    // random candidates (legal dimensions) each visit 147 channel differences.
    // Pyramid areas sum below 4/3; 200,000 cell-visits is a conservative bound
    // including boundary-hanging targets. The membrane has its own shared cap.
    pixels
        .saturating_mul(200_000)
        .saturating_add(crate::poisson::precision_sweep_budget(pixels as usize).saturating_mul(3))
}

/// The largest component window a request confined to `bounds` can plan:
/// the box padded by the requested ring (the settled ring is never larger,
/// and a component's box lies inside the request's), clamped to the canvas.
/// [`estimate`]'s validation and refusals, in its order; the byte
/// Content-Aware Fill admission (`doc_content_aware`) shares it.
/// `caller` picks whose words the refusals use: the gesture's own
/// range sentence, and `window_cap_message`'s arm for the window. `kind` is
/// the spot-heal Type, whose requested ring (`inpaint_plan::requested_ring`)
/// pads the box.
pub(crate) fn estimate_window(
    canvas: (u32, u32),
    bounds: (u32, u32, u32, u32),
    covered_upper: u64,
    ring: u32,
    caller: Caller,
    kind: SpotHealType,
) -> Result<u64, String> {
    pixel_count(canvas.0, canvas.1)?;
    let (x, y, w, h) = bounds;
    if ring > 512
        || w == 0
        || h == 0
        || u64::from(x) + u64::from(w) > u64::from(canvas.0)
        || u64::from(y) + u64::from(h) > u64::from(canvas.1)
    {
        return Err(match caller {
            Caller::Fill => "content-aware fill bounds or ring are out of range",
            Caller::SpotHeal => "spot healing bounds or ring are out of range",
        }
        .into());
    }
    let covered = covered_upper.min(u64::from(w) * u64::from(h));
    let pad = crate::inpaint_plan::requested_ring(ring, covered, kind).ceil() as u32;
    let ww = x.saturating_add(w).saturating_add(pad).min(canvas.0) - x.saturating_sub(pad);
    let wh = y.saturating_add(h).saturating_add(pad).min(canvas.1) - y.saturating_sub(pad);
    let window = u64::from(ww) * u64::from(wh);
    if window > crate::doc_heal::MAX_SOLVE_WINDOW_PIXELS {
        return Err(crate::doc_inpaint::window_cap_message(
            caller,
            ww as usize,
            wh as usize,
            window,
        ));
    }
    Ok(window)
}

/// Metadata-only upper bound for a request whose coverage is confined to bounds
/// (x,y,width,height). The caller accounts for its source plate, destination and
/// input coverage separately and validates the supplied mask against the bounds.
/// `channels` is the plate's colour channel count (3, or 4 on a CMYK document);
/// `kind` is [`estimate_window`]'s.
pub(crate) fn estimate(
    canvas: (u32, u32),
    bounds: (u32, u32, u32, u32),
    covered_upper: u64,
    ring: u32,
    caller: Caller,
    kind: SpotHealType,
    channels: usize,
) -> Result<(u64, u64), String> {
    let window = estimate_window(canvas, bounds, covered_upper, ring, caller, kind)?;
    let n = pixel_count(canvas.0, canvas.1)? as u64;
    let covered = covered_upper.min(u64::from(bounds.2) * u64::from(bounds.3));
    // Many disconnected components may repeatedly visit the same source pixels.
    // The byte planner bounds their summed windows independently of this box.
    let components = covered.min(INPAINT_SEARCH_PIXELS);
    let summed_windows = window
        .saturating_mul(components)
        .min(crate::doc_inpaint::MAX_INPAINT_PLAN_PIXELS);
    let work = summed_windows
        .saturating_mul(200_000)
        .saturating_add(
            crate::poisson::precision_sweep_budget(summed_windows as usize).saturating_mul(3),
        )
        .saturating_add(
            crate::poisson::precision_sweep_budget(0)
                .saturating_mul(3)
                .saturating_mul(components),
        );
    Ok((scratch(n, components, window, channels), work))
}

/// Exact component/ring plan; all costs are decided before the first synthesis
/// pass. base_scratch is caller storage already alive alongside these buffers;
/// `channels` and `kind` are [`estimate`]'s.
#[allow(clippy::too_many_arguments)]
pub(crate) fn plan(
    canvas: (u32, u32),
    coverage: &[u16],
    ring: u32,
    soft_write: bool,
    kind: SpotHealType,
    base_scratch: u64,
    channels: usize,
    job: JobContext<'_>,
) -> Result<InpaintPlan, String> {
    job.check()?;
    let n = pixel_count(canvas.0, canvas.1)?;
    if coverage.len() != n || ring > 512 {
        return Err("high-depth synthesis coverage or ring is invalid".into());
    }
    let threshold = if soft_write { 1 } else { 32768 };
    let mut covered = 0u64;
    for (i, c) in coverage.iter().enumerate() {
        job.every(i)?;
        covered += u64::from(*c >= threshold);
    }
    // The component records the admission priced: one per covered pixel up
    // to `INPAINT_SEARCH_PIXELS` (`estimate`'s clamp). The covered count
    // itself has no ceiling (`doc_inpaint::inpaint_lift`); the parts refusal
    // below is what keeps the clamp a count of every record the walk makes.
    let records = covered.min(INPAINT_SEARCH_PIXELS);
    let preliminary = base_scratch.saturating_add(scratch(n as u64, records, 0, channels));
    job.reserve(preliminary)?;
    let hole: Vec<bool> = coverage.iter().map(|c| *c >= threshold).collect();
    let caller = if soft_write {
        Caller::Fill
    } else {
        Caller::SpotHeal
    };
    let starts =
        crate::doc_heal::component_count_bound(&hole, canvas.0 as usize, canvas.1 as usize, job)?;
    if starts > INPAINT_SEARCH_PIXELS {
        return Err(parts_cap_message(caller, starts));
    }
    let components =
        crate::doc_heal::components_with_context(&hole, canvas.0 as usize, canvas.1 as usize, job)?;
    let windows = crate::inpaint_plan::plan(
        &components,
        (canvas.0 as usize, canvas.1 as usize),
        ring,
        caller,
        kind,
        job.range(0.0, 0.15),
        preliminary,
    )?;
    let max_window = windows
        .iter()
        .map(ComponentPlan::window_pixels)
        .max()
        .unwrap_or(0);
    let scratch_bytes =
        base_scratch.saturating_add(scratch(n as u64, records, max_window, channels));
    let work_units = windows.iter().fold(0u64, |work, window| {
        work.saturating_add(window_work(window.window_pixels()))
    });
    job.reserve(scratch_bytes)?;
    Ok(InpaintPlan {
        canvas,
        hole,
        components,
        windows,
        soft_write,
        kind,
        scratch_bytes,
        work_units,
    })
}

impl InpaintPlan {
    /// The union of every component window, canvas space, exclusive ends
    /// (the canvas rect [`apply`] reads and writes the destination in);
    /// `None` when there are no windows.
    pub(crate) fn window_union(&self) -> Option<(i64, i64, i64, i64)> {
        self.windows.iter().fold(None, |union, window| {
            let (x, y, w, h) = window.window;
            let (x0, y0) = (x as i64, y as i64);
            let (x1, y1) = (x0 + w as i64, y0 + h as i64);
            Some(match union {
                None => (x0, y0, x1, y1),
                Some((a, b, c, d)) => (a.min(x0), b.min(y0), c.max(x1), d.max(y1)),
            })
        })
    }
}

/// Content-Aware Fill's opt-in `fill_transparent` pad, the
/// one copy every depth calls: every canvas pixel `ci` with `hole(ci)` that
/// lies on the destination storage (`pixels`, `C + 1` samples a pixel, alpha
/// last, a `dimensions` plane at canvas `offset`) gets its alpha sample set to
/// `full`; RGB is untouched. It runs on the destination BEFORE the
/// synthesis writes it, because the write-back leaves a transparent
/// destination alone (the contract above `apply`): padded, the hole is an
/// opaque destination the coverage-weighted write-back fills like any other,
/// so a partly transparent rim pixel inside the hole keeps its straight
/// colour and is mixed with the fill by its coverage. The source plate was
/// captured before the pad, so the hole is never a source. Answers whether
/// any alpha sample moved (the byte arm's changed latch; the typed arms
/// compare the written rect).
pub(crate) fn opaque_hole<T: Copy + PartialEq, const C: usize>(
    pixels: &mut [T],
    dimensions: (u32, u32),
    offset: (i32, i32),
    canvas: (u32, u32),
    hole: impl Fn(usize) -> bool,
    full: T,
) -> bool {
    let width = dimensions.0 as usize;
    if width == 0 {
        return false;
    }
    let mut moved = false;
    for (i, pixel) in pixels.chunks_exact_mut(C + 1).enumerate() {
        let cx = (i % width) as i64 + i64::from(offset.0);
        let cy = (i / width) as i64 + i64::from(offset.1);
        let Some(ci) = crate::precision_paint::canvas_index(cx, cy, canvas.0, canvas.1) else {
            continue;
        };
        if hole(ci) && pixel[C] != full {
            pixel[C] = full;
            moved = true;
        }
    }
    moved
}

/// Synthesizes into caller-owned native destination storage. All source reads
/// use the captured plate. Destination alpha (including 1/65535), uncovered RGB
/// and off-canvas storage remain exact; transparent destinations are no-ops.
/// Additive selected stroke path, preserving the established synthesis domain
/// while carrying exact canvas selection until the shared final writeback.
///
/// The destination's sample type is the caller's: u16 codes, or a 32-bit
/// float document's W codes (`retouch_encoding`). The plate is read in
/// its own storage: a u16 run as its codes with data at alpha ≥ 32768, an f32
/// run (linear light) through `retouch_encoding::encode` with data at alpha
/// ≥ 0.5, the same half-alpha policy.
///
/// `C` is the colour channel count and `pixels` holds `C + 1` samples a
/// pixel, alpha last: 3 with an RGB [`SynthesisPlate::Precision`] plate, 4
/// with a CMYK document's [`SynthesisPlate::Cmyk`] plate (#547), whose four
/// ink codes are searched, voted and healed as the RGB channels are, with the
/// same half-alpha data rule. A plate of the other width is refused.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply<T: HealSample, const C: usize>(
    plan: &InpaintPlan,
    source: SynthesisPlate<'_>,
    coverage: &[u16],
    eligibility: Option<&[bool]>,
    strength: f32,
    seed: u64,
    pixels: &mut [T],
    dimensions: (u32, u32),
    offset: (i32, i32),
    selection: Option<&[u16]>,
    job: JobContext<'_>,
) -> Result<bool, String> {
    job.check()?;
    if source.dimensions() != plan.canvas
        || source.channels() != C
        || coverage.len() != plan.hole.len()
        || eligibility.is_some_and(|e| e.len() != plan.hole.len())
        || pixels.len() as u64 != u64::from(dimensions.0) * u64::from(dimensions.1) * (C as u64 + 1)
        || !strength.is_finite()
        || !(0.0..=1.0).contains(&strength)
    {
        return Err(
            "high-depth synthesis source, coverage or destination does not match its plan".into(),
        );
    }
    job.reserve(plan.scratch_bytes)?;
    let cw = plan.canvas.0 as usize;
    let total = plan.work_units.max(1);
    let mut done = 0u64;
    let mut changed = false;
    for (component, window) in plan.components.iter().zip(&plan.windows) {
        let control = job.range(
            done as f64 / total as f64,
            (done + window_work(window.window_pixels())) as f64 / total as f64,
        );
        let (x0, y0, w, h) = window.window;
        let n = w * h;
        let mut rgb = vec![0f32; n * C];
        let mut data = vec![false; n];
        // Which window pixels carry real colour, hole or not: Create
        // Texture's pool (`spot_heal_type`); no other type reads it.
        let texture = plan.kind == SpotHealType::CreateTexture;
        let mut solid_plane = if texture { vec![false; n] } else { Vec::new() };
        let mut permitted = eligibility.map(|_| vec![false; n]);
        for wy in 0..h {
            control.check()?;
            let row = (y0 + wy) as u32;
            // Window index and canvas index of the sample at canvas column
            // `x`.
            let at = |x: usize| {
                let wx = x - x0;
                (wy * w + wx, (y0 + wy) * cw + x0 + wx)
            };
            source.read_row::<C>(row, x0, x0 + w, |x, colour, solid| {
                let (i, ci) = at(x);
                rgb[i * C..i * C + C].copy_from_slice(&colour);
                // Same normalized half-alpha source policy as byte synthesis.
                data[i] = solid && !plan.hole[ci];
                if texture {
                    solid_plane[i] = solid;
                }
                if let (Some(out), Some(mask)) = (&mut permitted, eligibility) {
                    out[i] = mask[ci];
                }
            });
        }
        let mut hole = vec![false; n];
        let mut write = vec![0u16; n];
        for &p in &component.pixels {
            let p = p as usize;
            let i = (p / cw - y0) * w + p % cw - x0;
            hole[i] = true;
            write[i] = if plan.soft_write {
                coverage[p]
            } else {
                u16::MAX
            };
        }
        let plane = Plane::<f32, C> {
            w,
            h,
            rgb,
            hole,
            data,
            eligibility: permitted,
        };
        let caller = if plan.soft_write {
            Caller::Fill
        } else {
            Caller::SpotHeal
        };
        let (synthesized, region) = crate::spot_heal_type::synthesize(
            plan.kind,
            crate::spot_heal_type::SHIPPED,
            plane,
            texture.then_some(&solid_plane[..]),
            window.ring,
            seed,
            None,
            caller,
            window.lift,
            control.range(0.0, 0.85),
        )?;
        // These are binary topology samples, not a reduction of RGB or authored
        // coverage. The shared contour algorithm has exactly one interpretation.
        let region: Vec<u16> = region.into_iter().map(|v| u16::from(v) * 257).collect();
        changed |= crate::heal_writeback::heal_with_selection_channels::<_, _, _, C>(
            plan.canvas,
            dimensions,
            pixels,
            offset,
            (x0 as i64, y0 as i64, w, h),
            &synthesized,
            &region,
            &write,
            strength,
            selection,
            control.range(0.85, 1.0),
        )?;
        done += window_work(window.window_pixels());
    }
    Ok(changed)
}
