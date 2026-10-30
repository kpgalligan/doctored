//! Stack Modes: one per-pixel statistic across a set of raster layers.
//! A kernel: the byte adapter is `doc_stack_mode`, the typed one
//! `precision_stack_mode`; this module names neither document.
//!
//! The request is `{"op":"stack_mode","layers":[N,…],"above":N,
//! "mode":"mean|median|minimum|maximum|range|standard_deviation|summation|
//! variance|entropy|kurtosis|skewness|majority","name":S}`,
//! every key required and unknown keys refused. `layers` are the members;
//! `above` is the layer the result is inserted directly above, any index in
//! range that is not below the highest member (it need not be a member, nor
//! a raster).
//!
//! # The rule
//!
//! The extent is the union of the members' pixel rectangles; a member has
//! alpha 0 outside its own. Per extent pixel, with A the full alpha (255,
//! 65535, 1.0):
//!
//! - S₀ is the members with alpha > 0. When it is empty the pixel is
//!   transparent (all zero).
//! - The output alpha is the largest alpha in S₀, in every mode.
//! - **Mean**: per colour channel `Σ aᵢ·cᵢ / Σ aᵢ` over S₀ — alpha-weighted
//!   straight colour, so a barely covered edge pixel pulls the average by its
//!   coverage and not as a full vote.
//! - **Every other mode** reads S, the members with `2·alpha ≥ A` (at least
//!   half covered), or S₀ when S is empty, unweighted and per channel:
//!   Median is the middle value, or the mean of the two middle values;
//!   Minimum; Maximum; Range is `max − min`; Standard Deviation is the
//!   population σ. An order statistic has no meaning for a weighted sample,
//!   so coverage decides membership instead of weight.
//! - **Summation, Variance, Entropy, Kurtosis, Skewness, Majority** read S
//!   the same way: Σv; m₂ / F in codes; base-2 entropy over 256 equal bins
//!   of [0, F], over log₂ min(n, 256), × F; kurtosis and skewness scaled by
//!   their population bounds for n members so both span [0, F]; the most
//!   frequent value, a tie taking the Median rule over the tied values. The
//!   exact rule, its edge cases and why are `stack_moments`' module doc,
//!   where the per-pixel statistic lives.
//!
//! Integers round half up and are computed exactly: the mean as
//! `(2·num + den) / (2·den)`, an even median as `(lo + hi + 1) / 2`, σ as
//! `⌊σ + ½⌋` from f64 (the radicand `n·Σv² − (Σv)²` is exact in u128), the
//! variance exactly through the same radicand, every other f64 result
//! `⌊x + ½⌋`, each clamped to [0, F]. Float
//! computes in f64 and stores f32, unclamped, so values above white and below
//! zero survive; an odd Median, Minimum and Maximum are a member's own value
//! bit for bit. A Range past the storage bound 65504 is stored as the bound
//! (the typed adapter's `bounded`): members at +65504 and −65504 read 65504.
//!
//! 8- and 16-bit statistics are of the stored codes (the document's own
//! encoding), float of the linear values. Masks, opacity, blend modes, styles
//! and Blend If are not read: the statistic reads each member's stored
//! pixels, as Layer Via does.
//!
//! # Admission
//!
//! `plan_scratch` charges [`output_plane_copies`] copies of the output plane
//! (a factor per depth), one extent-wide row per member plus one, the
//! caller's structure term and 1 MiB. No member is made contiguous: the
//! statistic's own working set is one row per member, the contiguous output
//! plane and the tiles the stored layer is built from it. The factor is
//! larger than those two planes because the plan is held against the host's
//! WHOLE call — the statistic, then the display preparation and the published
//! edit — which is what the host reports as `working_memory_plan_bytes` and
//! what the gate measures. That part does not scale with the sample width
//! (the display is 8-bit at every depth), so the peak is fewer output planes
//! the wider the sample: about 5 at 8 bits, 4 at 16 and 2.6 at float. One
//! factor for all three (it was 7, the 8-bit floor) planned a 12 MP float
//! stack at 1.37 GB against a 0.49 GB peak, so each depth has its own.
//!
//! Measured with `python3 -B scripts/stack-admission/measure.py --app
//! build/Build/Products/Release/Doctored.app` (2026-10-12; Release, a fresh
//! app per case at an 8 GiB working limit, `MallocLargeCache=0`, three runs
//! and a baseline twin; 4032 × 3024 layers; E is the interval's peak
//! phys_footprint over its start around one `stack_mode` (median) call, and
//! around one `reduce_noise_from_stack` (mean) call for the denoise row).
//! Gate M-stack is plan ≥ ⌈1.25 × max E⌉ for every row whose footprint rose;
//! a row whose max E is not above its baseline twins' is `no rise` and puts
//! no floor under the plan. Each depth's rows are beside its constant.
//!
//! The factors began as one, 3. The first round (the same drive on a Debug
//! build, one run) read plans of 147,413,024 / 297,974,316 / 590,701,622 for
//! the two-layer cases against E 242,466,984 / 388,940,064 / 489,980,288,
//! each under its floor. A miss raises that depth's constant, never a new
//! one.

use serde::Deserialize;

use crate::stack_moments::{integer_statistic, real_statistic};

/// Copies of the OUTPUT plane an 8-bit plan charges (module doc,
/// "Admission"). The two-layer case peaks at 4.98 planes (48,771,072 bytes
/// each), a floor of 6.22, so 7: 6 would plan 293,726,240, under the floor.
///
/// | layers                 | plan        | max E of 3  | ⌈1.25 × max E⌉ | plan / max E |
/// |------------------------|-------------|-------------|----------------|--------------|
/// | 2                      | 342,497,312 | 242,679,976 | 303,349,970    | 1.411        |
/// | 8                      | 342,602,624 | 81,920      | 102,400        | no floor     |
/// | 16                     | 342,743,040 | 65,536      | —              | no rise      |
/// | 16, reduce noise       | 342,743,040 | 98,304      | —              | no rise      |
/// | 8 at 24 MP             | 673,277,544 | 0           | —              | no rise      |
/// | 4, CMYK (10 B a pixel) | 859,115,980 | 149,471,304 | 186,839,130    | 5.748        |
///
/// The last two rows were measured 2026-10-16 (`--cases
/// stack-8-jpeg8-24mp,stack-4-cmyk`: eight 6000 × 4000 plane frames, and four
/// 4032 × 3024 layers converted to CMYK before the interval); the CMYK arm
/// takes this factor (`cmyk_stack_mode`).
///
/// With eight and sixteen 8-bit layers the footprint stays where the call
/// started (the stack's own views are already resident; sixteen start at
/// 1,708 MB and end at 1,032 MB): the eight-layer row read one 16 KB page
/// over its twins' 65,536 and the other two nothing over theirs, so the
/// two-layer row is the floor.
pub const OUTPUT_PLANE_COPIES_8: u64 = 7;

/// Copies of the OUTPUT plane a 16-bit plan charges. The peak is 3.99 planes
/// (97,542,144 bytes each) at every layer count, a floor of 4.99, so 5: 4
/// would plan 395,516,460, under the floor.
///
/// | layers | plan        | max E of 3  | ⌈1.25 × max E⌉ | plan / max E |
/// |--------|-------------|-------------|----------------|--------------|
/// | 2      | 493,058,604 | 389,218,616 | 486,523,270    | 1.267        |
/// | 8      | 493,276,812 | 389,284,104 | 486,605,130    | 1.267        |
/// | 16     | 493,567,756 | 389,497,120 | 486,871,400    | 1.267        |
pub const OUTPUT_PLANE_COPIES_16: u64 = 5;

/// Copies of the OUTPUT plane a 32-bit float plan charges. The peak is 2.51
/// to 2.57 planes (195,084,288 bytes each), a floor of 3.22 at sixteen
/// layers, so 4: 3 would plan 590,701,622, under the floor.
///
/// | layers | plan        | max E of 3  | ⌈1.25 × max E⌉ | plan / max E |
/// |--------|-------------|-------------|----------------|--------------|
/// | 2      | 785,785,910 | 490,078,592 | 612,598,240    | 1.603        |
/// | 8      | 786,197,654 | 494,371,176 | 617,963,970    | 1.590        |
/// | 16     | 786,746,646 | 502,071,680 | 627,589,600    | 1.567        |
pub const OUTPUT_PLANE_COPIES_32: u64 = 4;

/// The factor for a sample of `bytes_per_pixel` (4, 8 or 16: RGBA at 8, 16
/// and 32 bits). Any other width takes the 8-bit factor, the largest.
pub fn output_plane_copies(bytes_per_pixel: u64) -> u64 {
    match bytes_per_pixel {
        8 => OUTPUT_PLANE_COPIES_16,
        16 => OUTPUT_PLANE_COPIES_32,
        _ => OUTPUT_PLANE_COPIES_8,
    }
}

/// The sentence for members that hold no pixel anywhere.
pub const NO_PIXELS: &str = "stack mode found no pixels in the selected layers";

/// Fixed slack for the per-pixel buffers, allocator rounding and bookkeeping.
const PLAN_SLACK_BYTES: u64 = 1024 * 1024;

/// The statistic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StackMode {
    Mean,
    Median,
    Minimum,
    Maximum,
    Range,
    StandardDeviation,
    Summation,
    Variance,
    Entropy,
    Kurtosis,
    Skewness,
    Majority,
}

/// A parsed Stack Mode request; [`Self::members`] validates the set.
#[derive(Clone, Debug)]
pub struct StackModeRequest {
    /// The members, as given.
    pub layers: Vec<usize>,
    /// The layer the result is inserted directly above.
    pub above: usize,
    pub mode: StackMode,
    /// The new layer's name.
    pub name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    op: String,
    layers: Vec<usize>,
    above: usize,
    mode: StackMode,
    name: String,
}

impl StackModeRequest {
    /// Parses the JSON request; `Err` for malformed JSON, a missing or
    /// unknown key, an unknown mode or an `op` other than `stack_mode`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let wire: Wire =
            serde_json::from_str(text).map_err(|e| format!("invalid stack mode request: {e}"))?;
        if wire.op != "stack_mode" {
            return Err("invalid stack mode request: op must be stack_mode".into());
        }
        Ok(Self {
            layers: wire.layers,
            above: wire.above,
            mode: wire.mode,
            name: wire.name,
        })
    }

    /// The members sorted bottom-first. `Err` for fewer than two, for an
    /// index listed twice, and for an `above` below the highest member (the
    /// result would land under a layer it was computed from).
    pub fn members(&self) -> Result<Vec<usize>, String> {
        if self.layers.len() < 2 {
            return Err("stack mode requires at least two layers".into());
        }
        let mut sorted = self.layers.clone();
        sorted.sort_unstable();
        if let Some(pair) = sorted.windows(2).find(|pair| pair[0] == pair[1]) {
            return Err(format!("stack mode lists layer {} more than once", pair[0]));
        }
        let highest = sorted[sorted.len() - 1];
        if self.above < highest {
            return Err(format!(
                "stack mode cannot insert above layer {}: it is below layer {highest}, the highest \
                 stacked layer",
                self.above
            ));
        }
        Ok(sorted)
    }
}

/// One stored RGBA sample of a depth the statistic runs at.
pub trait StackSample: Copy {
    /// The all-zero sample: what a member reads outside its own rectangle.
    const TRANSPARENT: Self;
    /// True when alpha is not 0.
    #[doc(hidden)]
    fn covered(&self) -> bool;
    /// The output sample for column `x` of `rows` (module doc, "The rule").
    #[doc(hidden)]
    fn statistic(mode: StackMode, rows: &[&[Self]], x: usize, scratch: &mut StackScratch) -> Self;
}

/// Per-pixel buffers, reused across pixels and rows: the gathered members
/// and one channel's values for the order statistics.
pub struct StackScratch {
    wide: Vec<[u64; 4]>,
    real: Vec<[f64; 4]>,
    order_wide: Vec<u64>,
    order_real: Vec<f64>,
}

impl StackScratch {
    /// Buffers for `layers` members. `Err` when they cannot be allocated.
    pub fn new(layers: usize) -> Result<Self, String> {
        fn reserved<T>(count: usize) -> Result<Vec<T>, String> {
            let mut out = Vec::new();
            out.try_reserve_exact(count)
                .map_err(|_| "could not allocate stack mode buffers".to_string())?;
            Ok(out)
        }
        Ok(Self {
            wide: reserved(layers)?,
            real: reserved(layers)?,
            order_wide: reserved(layers)?,
            order_real: reserved(layers)?,
        })
    }
}

impl StackSample for [u8; 4] {
    const TRANSPARENT: Self = [0; 4];
    fn covered(&self) -> bool {
        self[3] != 0
    }
    fn statistic(mode: StackMode, rows: &[&[Self]], x: usize, scratch: &mut StackScratch) -> Self {
        scratch.wide.clear();
        scratch.wide.extend(
            rows.iter()
                .filter_map(|row| row.get(x))
                .filter(|p| p[3] != 0)
                .map(|p| p.map(u64::from)),
        );
        integer_statistic(mode, &mut scratch.wide, &mut scratch.order_wide, 255).map(|v| v as u8)
    }
}

impl StackSample for [u16; 4] {
    const TRANSPARENT: Self = [0; 4];
    fn covered(&self) -> bool {
        self[3] != 0
    }
    fn statistic(mode: StackMode, rows: &[&[Self]], x: usize, scratch: &mut StackScratch) -> Self {
        scratch.wide.clear();
        scratch.wide.extend(
            rows.iter()
                .filter_map(|row| row.get(x))
                .filter(|p| p[3] != 0)
                .map(|p| p.map(u64::from)),
        );
        integer_statistic(mode, &mut scratch.wide, &mut scratch.order_wide, 65535).map(|v| v as u16)
    }
}

impl StackSample for [f32; 4] {
    const TRANSPARENT: Self = [0.0; 4];
    fn covered(&self) -> bool {
        self[3] != 0.0
    }
    fn statistic(mode: StackMode, rows: &[&[Self]], x: usize, scratch: &mut StackScratch) -> Self {
        scratch.real.clear();
        scratch.real.extend(
            rows.iter()
                .filter_map(|row| row.get(x))
                .filter(|p| p[3] > 0.0)
                .map(|p| p.map(f64::from)),
        );
        real_statistic(mode, &mut scratch.real, &mut scratch.order_real).map(|v| v as f32)
    }
}

/// The statistic for one row: `rows[i]` is member `i`'s samples across the
/// extent (transparent where the member does not reach), each
/// `out.len()` long; a shorter row reads as transparent past its end. Returns
/// true when any output alpha is not 0.
pub fn stack_row<P: StackSample>(
    mode: StackMode,
    rows: &[&[P]],
    out: &mut [P],
    scratch: &mut StackScratch,
) -> bool {
    let mut any = false;
    for (x, slot) in out.iter_mut().enumerate() {
        let pixel = P::statistic(mode, rows, x, scratch);
        any |= pixel.covered();
        *slot = pixel;
    }
    any
}

/// One member's extent-wide row: transparent everywhere, then `runs` (the
/// member's own row as `(x, samples)` runs in its own columns) copied in
/// starting at extent column `at`. A run reaching past the row is dropped.
pub fn fill_row<'a, P: StackSample + 'a>(
    row: &mut [P],
    at: usize,
    runs: impl Iterator<Item = (u32, &'a [P])>,
) {
    row.fill(P::TRANSPARENT);
    for (x, run) in runs {
        let start = at + x as usize;
        if let Some(target) = row.get_mut(start..start + run.len()) {
            target.copy_from_slice(run);
        }
    }
}

/// The union of pixel rectangles `(x, y, width, height)` as the same shape.
/// `None` when there is none, when every one is empty, and when the union
/// does not fit an i32 origin with u32 sides.
pub fn union_extent(
    rects: impl IntoIterator<Item = (i32, i32, u32, u32)>,
) -> Option<(i32, i32, u32, u32)> {
    let mut bounds: Option<(i64, i64, i64, i64)> = None;
    for (x, y, w, h) in rects {
        if w == 0 || h == 0 {
            continue;
        }
        let (x0, y0) = (i64::from(x), i64::from(y));
        let (x1, y1) = (x0 + i64::from(w), y0 + i64::from(h));
        bounds = Some(match bounds {
            None => (x0, y0, x1, y1),
            Some((a, b, c, d)) => (a.min(x0), b.min(y0), c.max(x1), d.max(y1)),
        });
    }
    let (x0, y0, x1, y1) = bounds?;
    Some((
        i32::try_from(x0).ok()?,
        i32::try_from(y0).ok()?,
        u32::try_from(x1 - x0).ok()?,
        u32::try_from(y1 - y0).ok()?,
    ))
}

/// The scratch a plan charges: [`output_plane_copies`] output planes for the
/// sample width, one extent-wide row per member plus one, the caller's
/// `structure` term and 1 MiB. `None` on overflow.
pub fn plan_scratch(
    extent_pixels: u64,
    extent_width: u64,
    bytes_per_pixel: u64,
    layers: u64,
    structure: u64,
) -> Option<u64> {
    let planes = output_plane_copies(bytes_per_pixel)
        .checked_mul(extent_pixels)?
        .checked_mul(bytes_per_pixel)?;
    let rows = layers
        .checked_add(1)?
        .checked_mul(extent_width)?
        .checked_mul(bytes_per_pixel)?;
    planes
        .checked_add(rows)?
        .checked_add(structure)?
        .checked_add(PLAN_SLACK_BYTES)
}
