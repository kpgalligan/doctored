//! Versioned, cancellable source-filter DENOISE sampling: an alpha-weighted
//! guided filter (the He/Sun/Tang local linear model) over a Rec. 709 opponent
//! decomposition, returned as the CORRECTION that model asks for.
//!
//! The PER-PIXEL cost of this neighborhood is independent of its radius, because
//! every window statistic is a box sum computed as two 1-D sliding passes. That
//! is the whole reason a 25-pixel chroma window is admissible at full
//! photographic resolution: a convolution-family or bilateral kernel at the same
//! window would cost two orders of magnitude more and refuse on an ordinary
//! photograph. The accumulators still have to be SEEDED once per row and once
//! per column, which is `2r+1` reads each and is not radius-independent; on an
//! ordinary photograph that term is noise, on a plane narrower than the window
//! it dominates, and `plan_bounded` charges it either way. Nothing here is approximated — no subsampling, no bilateral
//! grid, no silent radius collapse — so the plane stays checkable against a
//! direct binary64 oracle.
//!
//! The statistics are STREAMED: every box sum is a
//! `sampling_box::BoxRows` ring of `min(2r+2, h)` rows, the regression is one
//! row at a time between two ring stages, and the halves run one after the
//! other, so beyond the n×16 correction plane a half holds O(r·w) binary64
//! rows rather than 6–8 whole n×8 planes. The arithmetic is the whole-plane
//! form's, bit for bit (`tests/banded_reduce_noise.rs` pins it), and that is
//! what admits a 6000×4000 layer at both depths.
//!
//! Every sum is ALPHA-WEIGHTED and every mean divides by the summed alpha, so a
//! fully transparent neighbour contributes nothing and cannot pull the average
//! toward its stored colour. That is the premultiplied Gaussian's law written
//! for a box window.
//!
//! The plane holds `[dY, dU, dV, alpha]` — a DELTA, not a smoothed sample. An
//! inactive half therefore stores an exact `0.0`, which is what makes a half
//! that is turned off a bitwise identity rather than a correction that cancels
//! to within a rounding, and what makes a chroma-only correction leave
//! luminance exactly unchanged.
//!
//! The luminance half decides what counts as noise (`luminance` is the level to
//! remove and enters `eps`); the chroma half smooths chroma along the LUMINANCE
//! guide, so a colour boundary carrying no luminance step is invisible to it
//! and will bleed. That limitation is inherent to every luma-guided chroma
//! denoiser and is what the `color` control exists to trade against.
//!
//! `reduce_noise` (luminance/detail/color) reads `sampling_denoise.rs` — an alpha-weighted guided
//! filter over a Rec.709 opponent decomposition whose cost is INDEPENDENT of radius, because every
//! window statistic is a sliding box sum; its statistics are binary64 because `S2/S0 - m*m` cancels
//! far below f32 at a small noise level, and they are STREAMED, never whole
//! planes: every box sum is a `sampling_box::BoxRows` ring of min(2r+2, h) rows (the ONE
//! clamped-edge sliding box sum; Box Blur's `box_sum` is its whole-plane wrapper), the regression
//! runs a row at a time between two ring stages and the halves run one after the other, so
//! `plan_bounded` charges n×16 plus the larger half's rings (`half_bytes`), bit-identical to the
//! whole-plane sums (`tests/banded_reduce_noise.rs`); the plane it caches is a DELTA so an inactive
//! half is a bitwise identity.
use crate::blend::{LUMA_B, LUMA_G, LUMA_R};
use crate::filter_kernels::SampleRange;
use crate::job_control::JobContext;
use crate::precision_image::{pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use crate::sampling_box::BoxRows;

/// The chroma window is this multiple of the luminance window: chroma noise is
/// lower frequency than luminance noise and chroma detail is almost never real
/// image content, so it wants a much wider estimate.
pub(crate) const CHROMA_RADIUS_FACTOR: usize = 4;
pub(crate) const CHROMA_RADIUS_LIMIT: usize = 256;
/// The chroma regression's own epsilon, fixed rather than exposed: it only has
/// to keep the slope finite where the luminance guide is flat.
const CHROMA_GUIDE_EPSILON: f64 = (4.0 / 255.0) * (4.0 / 255.0);
/// Without this a flat noisy patch, whose guide variance sits near the epsilon,
/// can amplify luminance grain into chroma swings of half a unit. Four units of
/// chroma per unit of luminance is already an extreme slope.
const CHROMA_SLOPE_LIMIT: f64 = 4.0;

/// Rec. 709, the one copy in `blend.rs`. Deliberately NOT the 0.3/0.59/0.11
/// set, which is the W3C non-separable blend weighting and is scoped to the
/// Luminosity blend function.
const WR: f64 = LUMA_R as f64;
const WG: f64 = LUMA_G as f64;
const WB: f64 = LUMA_B as f64;
const KR: f64 = WR / WG;
const KB: f64 = WB / WG;

/// `(luminance radius, chroma radius)` in whole source pixels.
pub(crate) fn radii(sigma: f64) -> (usize, usize) {
    let luma = ((3.0 * sigma).ceil() as usize).max(1);
    (
        luma,
        luma.saturating_mul(CHROMA_RADIUS_FACTOR)
            .min(CHROMA_RADIUS_LIMIT),
    )
}

/// The opponent decomposition: `(Y, U, V)` with `U = B - Y` and `V = R - Y`.
/// The Rec. 709 normalizing divisors are deliberately omitted; nothing is
/// stored in opponent form and the inverse stays a two-constant expression.
pub(crate) fn opponent(sample: [f32; 4]) -> (f64, f64, f64) {
    let (r, g, b) = (
        f64::from(sample[0]),
        f64::from(sample[1]),
        f64::from(sample[2]),
    );
    let y = WR * r + WG * g + WB * b;
    (y, b - y, r - y)
}

/// The inverse, applied to a DELTA and added to the untouched RGB rather than
/// round-tripped. `M^-1(M x)` returns `x` only to a few ulp, so mapping the
/// delta makes a zero correction bitwise identity with no special case. It also
/// makes a chroma-only correction leave luminance exactly unchanged:
/// `WR*dv + WG*(-KR*dv - KB*du) + WB*du` is identically zero.
///
/// Each component is finished with `range.color`, clamped to 0…1 at `Unit`
/// (the 8- and 16-bit expression, bit for bit) and kept as
/// computed at `Extended`, where the float route runs Reduce Noise and
/// Presence on the layer's display-encoded copy (`display_encoding`).
///
/// Reduce Noise at 32 bits: the guided filter, its Luminance level and the
/// Preserve Details / Colour Noise mix read the ENCODED copy, so they mean
/// at every value inside 0…1 what they mean at 16 bits; a value above white
/// is smoothed in code (the encoding's logarithm, equal steps for equal
/// ratios of light) and decoded back, never clamped to white.
pub(crate) fn applied_in(
    source: [f32; 4],
    dy: f64,
    du: f64,
    dv: f64,
    range: &SampleRange,
) -> [f32; 4] {
    [
        range.color(f64::from(source[0]) + dy + dv) as f32,
        range.color(f64::from(source[1]) + dy - KR * dv - KB * du) as f32,
        range.color(f64::from(source[2]) + dy + du) as f32,
        source[3],
    ]
}

/// Which halves a set of parameters actually runs. A half that is off computes
/// nothing at all — it is not a zero-weighted pass — which is why the presence
/// bits belong in the cache kernel key.
pub(crate) fn halves(luminance: f64, detail: f64, color: f64) -> (bool, bool) {
    (luminance > 0.0 && detail < 1.0, color > 0.0)
}

/// Per-pixel charge for the luminance half. A complete 2-D box pass over one
/// scalar field is a few units — two sub-passes of one add and one subtract —
/// independent of the radius; the rest is the pointwise decomposition, the
/// local regression and the reconstruction.
///
/// Both half charges are MEASURED (the ignored probe in
/// `tests/banded_reduce_noise.rs`): each half alone at r1 and r16 on
/// 3000×2000 noise, byte `dr_image_filter_op` and the 16-bit selected edit,
/// timed in the radius-16 unit. The byte runs measured 38–40 (luminance) and
/// 69–74 (chroma) units per pixel against the former 44 / 92; the 16-bit
/// edit (the whole selected edit, kernel included) measured 117–121 and
/// 265–286, so each figure is the smallest integer for which n·charge +
/// seeding covers 1.15× every measured run.
const DENOISE_LUMA_WORK: u64 = 139;
/// Per-pixel charge for the chroma half, in the same units.
const DENOISE_CHROMA_WORK: u64 = 326;

fn half_ops(luma: bool, chroma: bool) -> u64 {
    u64::from(luma) * DENOISE_LUMA_WORK + u64::from(chroma) * DENOISE_CHROMA_WORK
}

/// One streamed half's transient bytes: `boxes` `BoxRows` rings, the
/// `min(r+1, h)`-row summed-alpha delay ring and `rows` regression rows.
fn half_bytes(w: u64, h: u64, r: u64, boxes: u64, rows: u64) -> u64 {
    boxes
        .saturating_mul(BoxRows::bytes(w, h, r))
        .saturating_add(
            r.saturating_add(1)
                .min(h)
                .saturating_add(rows)
                .saturating_mul(w)
                .saturating_mul(8),
        )
}

/// Box passes per half, for the seeding charge below.
fn half_passes(luma: bool, chroma: bool) -> (u64, u64) {
    (u64::from(luma) * 5, u64::from(chroma) * 11)
}

pub(crate) fn plan_bounded(
    width: u32,
    height: u32,
    sigma: f64,
    luma: bool,
    chroma: bool,
    cap: u64,
) -> Result<(u64, u64), String> {
    if !sigma.is_finite() || !(0.0..=64.0).contains(&sigma) {
        return Err("denoise radius must be finite and in [0,64] source pixels".into());
    }
    let n = pixel_count(width, height)? as u64;
    if !luma && !chroma {
        return Ok((n * 16 + BOOKKEEPING_BYTES, n));
    }
    // The SLIDING cost is independent of radius; the SEEDING cost is not. Each
    // box pass re-seeds its horizontal accumulator once per row and its
    // vertical accumulator once per column, each with `2r+1` reads, so a plane
    // narrower or shorter than the window pays a term the per-pixel charge
    // cannot see. On an ordinary photograph it is noise; on a tall thin strip
    // at the chroma radius it dominates, and a forecast that omitted it would
    // admit work it had not charged for.
    let (luma_passes, chroma_passes) = half_passes(luma, chroma);
    let (luma_radius, chroma_radius) = radii(sigma);
    let edges = u64::from(width) + u64::from(height);
    let seeding = luma_passes
        .saturating_mul(2 * luma_radius as u64 + 1)
        .saturating_mul(edges)
        .saturating_add(
            chroma_passes
                .saturating_mul(2 * chroma_radius as u64 + 1)
                .saturating_mul(edges),
        );
    let work = n
        .saturating_mul(half_ops(luma, chroma))
        .saturating_add(seeding);
    if work > cap {
        return Err("denoise source filter exceeds the bounded pixel-work limit".into());
    }
    // The correction plane, plus the larger half's streamed statistics: the
    // halves run one after the other, so their transients never coexist. The
    // luminance half holds 5 box rings (3 statistics, 2 regression sums) and 2
    // regression rows; the chroma half 11 rings (alpha, Y, Y² and, per
    // channel, C and Y·C, then 2 regression sums per channel) and 6 rows
    // (mean, variance, slope and offset per channel). Binary64 is not optional
    // here: the local variance is `S2/S0 - m*m`, and at a small noise level
    // that difference cancels far below f32's resolution, which would move the
    // result by tens of codes.
    let (w, h) = (u64::from(width), u64::from(height));
    let luma_bytes = if luma {
        half_bytes(w, h, luma_radius as u64, 5, 2)
    } else {
        0
    };
    let chroma_bytes = if chroma {
        half_bytes(w, h, chroma_radius as u64, 11, 6)
    } else {
        0
    };
    Ok((
        (n * 16)
            .saturating_add(luma_bytes.max(chroma_bytes))
            .saturating_add(4 * BOOKKEEPING_BYTES),
        work,
    ))
}

/// The opponent correction plane `[dY, dU, dV, alpha]`. An inactive half leaves
/// its own slots at an exact zero.
///
/// Each active half is its own sequential pass over the rows (luminance, then
/// chroma), so the peak is the larger half, not their sum. Inside a half the
/// box sums are `BoxRows` rings: stage A streams the window statistics, each
/// emitted stage-A row becomes one row of the local regression, which is
/// pushed straight into stage B's rings, and each emitted stage-B row is
/// written into `out`. The arithmetic is the whole-plane form's expression for
/// expression, so the plane is bit-identical to it.
/// The luminance half's smoothed model is
/// `range.color`'s, clamped to 0…1 at `Unit` and kept at `Extended` (a
/// float layer's display-encoded copy, whose values above white are code
/// above 1 the model must not pull down to white).
#[allow(clippy::too_many_arguments)]
pub(crate) fn guided_bounded_in(
    width: u32,
    height: u32,
    sigma: f64,
    luminance: f64,
    luma: bool,
    chroma: bool,
    cap: u64,
    range: &SampleRange,
    read: impl Fn(usize) -> [f32; 4],
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    plan_bounded(width, height, sigma, luma, chroma, cap)?;
    let (w, h) = (width as usize, height as usize);
    let n = w * h;
    let (luma_radius, chroma_radius) = radii(sigma);
    let mut out: Vec<[f32; 4]> = reserved_vec(n)?;
    for index in 0..n {
        job.every(index)?;
        out.push([0.0, 0.0, 0.0, read(index)[3]]);
    }
    if !luma && !chroma {
        return Ok(out);
    }
    let split = match (luma, chroma) {
        (true, true) => 0.5,
        (true, false) => 1.0,
        _ => 0.0,
    };
    let plane = Plane { w, h, read: &read };
    if luma {
        plane.luma(
            &mut out,
            luma_radius,
            luminance,
            range,
            job.range(0.0, split),
        )?;
    }
    if chroma {
        plane.chroma(&mut out, chroma_radius, job.range(split, 1.0))?;
    }
    Ok(out)
}

/// The input plane one half streams over.
struct Plane<'a, F: Fn(usize) -> [f32; 4]> {
    w: usize,
    h: usize,
    read: &'a F,
}

impl<F: Fn(usize) -> [f32; 4]> Plane<'_, F> {
    fn alpha(&self, index: usize) -> f64 {
        f64::from((self.read)(index)[3])
    }

    /// The guide is always the INPUT luminance, recomputed in binary64 rather
    /// than read back from the f32 plane: the two halves are independent, so
    /// turning one on cannot move the other's result, and a f32 round trip of
    /// Y before the variance would reintroduce exactly the cancellation the
    /// binary64 statistics exist to avoid.
    fn guide(&self, index: usize) -> f64 {
        opponent((self.read)(index)).0
    }

    fn component(&self, index: usize, channel: usize) -> f64 {
        let (_, u, v) = opponent((self.read)(index));
        if channel == 1 {
            u
        } else {
            v
        }
    }

    /// A ring of `min(r+1, h)` rows holding stage A's summed alpha until the
    /// stage-B row that divides by it is written: stage-B row `b` is ready
    /// right after stage-A row `min(b + r, h − 1)`, at most `r` rows later.
    fn delay(&self, r: usize) -> Result<(Vec<f64>, usize), String> {
        let rows = r.saturating_add(1).min(self.h);
        let mut ring: Vec<f64> = reserved_vec(rows * self.w)?;
        ring.resize(rows * self.w, 0.0);
        Ok((ring, rows))
    }

    fn row(&self) -> Result<Vec<f64>, String> {
        let mut row: Vec<f64> = reserved_vec(self.w)?;
        row.resize(self.w, 0.0);
        Ok(row)
    }

    fn luma(
        &self,
        out: &mut [[f32; 4]],
        r: usize,
        luminance: f64,
        range: &SampleRange,
        job: JobContext<'_>,
    ) -> Result<(), String> {
        let (w, h) = (self.w, self.h);
        let mut s0 = BoxRows::new(w, h, r)?;
        let mut s1 = BoxRows::new(w, h, r)?;
        let mut s2 = BoxRows::new(w, h, r)?;
        let mut slope_bar = BoxRows::new(w, h, r)?;
        let mut offset_bar = BoxRows::new(w, h, r)?;
        let (mut weights, delay) = self.delay(r)?;
        let mut slope = self.row()?;
        let mut offset = self.row()?;
        let epsilon = luminance * luminance;
        let (mut regressed, mut written) = (0usize, 0usize);
        for _ in 0..h {
            s0.push(|i| self.alpha(i), job)?;
            s1.push(|i| self.alpha(i) * self.guide(i), job)?;
            s2.push(
                |i| {
                    let value = self.guide(i);
                    self.alpha(i) * value * value
                },
                job,
            )?;
            while s0.ready() {
                let at = (regressed % delay) * w;
                weights[at..at + w].copy_from_slice(s0.emit(job)?);
                let (sum1, sum2) = (s1.emit(job)?, s2.emit(job)?);
                for x in 0..w {
                    job.every(x)?;
                    let weight = weights[at + x];
                    if weight <= 0.0 {
                        slope[x] = 1.0;
                        offset[x] = 0.0;
                        continue;
                    }
                    let mean = sum1[x] / weight;
                    let variance = (sum2[x] / weight - mean * mean).max(0.0);
                    // Zero variance means the window has no detail to keep,
                    // so the model is the mean alone. The branch is not an
                    // optimization: a `luminance` small enough to square to
                    // zero is admissible in [0,0.25], and on a flat window
                    // that would make this 0/0 — one NaN then poisons the
                    // running box accumulator for the rest of its row and
                    // every row below it, and an in-range parameter would make
                    // the document unrenderable rather than merely unfiltered.
                    let a = if variance > 0.0 {
                        variance / (variance + epsilon)
                    } else {
                        0.0
                    };
                    slope[x] = a;
                    offset[x] = mean * (1.0 - a);
                }
                let base = regressed * w;
                slope_bar.push(|i| self.alpha(i) * slope[i - base], job)?;
                offset_bar.push(|i| self.alpha(i) * offset[i - base], job)?;
                regressed += 1;
                while slope_bar.ready() {
                    let (sb, ob) = (slope_bar.emit(job)?, offset_bar.emit(job)?);
                    let at = (written % delay) * w;
                    for x in 0..w {
                        job.every(x)?;
                        let weight = weights[at + x];
                        if weight <= 0.0 {
                            continue;
                        }
                        let index = written * w + x;
                        let value = self.guide(index);
                        let smoothed = range.color((sb[x] * value + ob[x]) / weight);
                        out[index][0] = (smoothed - value) as f32;
                    }
                    written += 1;
                    job.report(written as f64 / h as f64, 6)?;
                }
            }
        }
        Ok(())
    }

    fn chroma(&self, out: &mut [[f32; 4]], r: usize, job: JobContext<'_>) -> Result<(), String> {
        let (w, h) = (self.w, self.h);
        let mut c0 = BoxRows::new(w, h, r)?;
        let mut cy = BoxRows::new(w, h, r)?;
        let mut cyy = BoxRows::new(w, h, r)?;
        // Per channel (U, V): the alpha-weighted component and its product
        // with the guide.
        let mut sc = [BoxRows::new(w, h, r)?, BoxRows::new(w, h, r)?];
        let mut syc = [BoxRows::new(w, h, r)?, BoxRows::new(w, h, r)?];
        let mut slope_bar = [BoxRows::new(w, h, r)?, BoxRows::new(w, h, r)?];
        let mut offset_bar = [BoxRows::new(w, h, r)?, BoxRows::new(w, h, r)?];
        let (mut weights, delay) = self.delay(r)?;
        let mut mean = self.row()?;
        let mut variance = self.row()?;
        let mut slope = [self.row()?, self.row()?];
        let mut offset = [self.row()?, self.row()?];
        let (mut regressed, mut written) = (0usize, 0usize);
        for _ in 0..h {
            c0.push(|i| self.alpha(i), job)?;
            cy.push(|i| self.alpha(i) * self.guide(i), job)?;
            cyy.push(
                |i| {
                    let value = self.guide(i);
                    self.alpha(i) * value * value
                },
                job,
            )?;
            for (k, channel) in [1usize, 2].into_iter().enumerate() {
                sc[k].push(|i| self.alpha(i) * self.component(i, channel), job)?;
                syc[k].push(
                    |i| self.alpha(i) * self.guide(i) * self.component(i, channel),
                    job,
                )?;
            }
            while c0.ready() {
                let at = (regressed % delay) * w;
                weights[at..at + w].copy_from_slice(c0.emit(job)?);
                let (sum_y, sum_yy) = (cy.emit(job)?, cyy.emit(job)?);
                for x in 0..w {
                    job.every(x)?;
                    let weight = weights[at + x];
                    let m = if weight > 0.0 { sum_y[x] / weight } else { 0.0 };
                    variance[x] = if weight > 0.0 {
                        (sum_yy[x] / weight - m * m).max(0.0)
                    } else {
                        0.0
                    };
                    mean[x] = m;
                }
                for k in 0..2 {
                    let (sum_c, sum_yc) = (sc[k].emit(job)?, syc[k].emit(job)?);
                    for x in 0..w {
                        job.every(x)?;
                        let weight = weights[at + x];
                        if weight <= 0.0 {
                            slope[k][x] = 0.0;
                            offset[k][x] = 0.0;
                            continue;
                        }
                        let component_mean = sum_c[x] / weight;
                        let covariance = sum_yc[x] / weight - mean[x] * component_mean;
                        let a = (covariance / (variance[x] + CHROMA_GUIDE_EPSILON))
                            .clamp(-CHROMA_SLOPE_LIMIT, CHROMA_SLOPE_LIMIT);
                        slope[k][x] = a;
                        offset[k][x] = component_mean - a * mean[x];
                    }
                }
                let base = regressed * w;
                for k in 0..2 {
                    let (sl, of) = (&slope[k], &offset[k]);
                    slope_bar[k].push(|i| self.alpha(i) * sl[i - base], job)?;
                    offset_bar[k].push(|i| self.alpha(i) * of[i - base], job)?;
                }
                regressed += 1;
                while slope_bar[0].ready() {
                    let at = (written % delay) * w;
                    for (k, channel) in [1usize, 2].into_iter().enumerate() {
                        let (sb, ob) = (slope_bar[k].emit(job)?, offset_bar[k].emit(job)?);
                        for x in 0..w {
                            job.every(x)?;
                            let weight = weights[at + x];
                            if weight <= 0.0 {
                                continue;
                            }
                            let index = written * w + x;
                            let smoothed = (sb[x] * self.guide(index) + ob[x]) / weight;
                            out[index][channel] =
                                (smoothed - self.component(index, channel)) as f32;
                        }
                    }
                    written += 1;
                    job.report(written as f64 / h as f64, 6)?;
                }
            }
        }
        Ok(())
    }
}
