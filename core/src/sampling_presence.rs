//! Filters ▸ Presence: Texture, Clarity and Dehaze on one plane of
//! straight samples, a `SpatialKernel` arm on the one spatial route
//! (`SelectedFilter::Spatial` at 16 bits, `filter_op` at 8; no new export).
//! The three are free functions over explicit radii, so the Develop RAW
//! stage can call them with its own.
//!
//! Texture and Clarity are both local contrast in a band of spatial
//! frequencies, and a band is the difference of two low-passes. The low-pass
//! is EDGE-AWARE: [`base_delta`] is `sampling_denoise::guided_bounded_in`'s
//! luminance half (the alpha-weighted He/Sun/Tang guided filter over streamed
//! box sums), which averages low-contrast detail and follows a strong edge, so
//! the difference of two bases is near 0 beside an edge and boosting it draws
//! no halo. It is the crate's one edge-aware filter whose cost per pixel does
//! not depend on its radius; Surface Blur's tent costs 256 bins a pixel and is
//! refused on a 24 MP plate at a wide threshold, so it is not the base.
//!
//! With `d_r = B_r(Y) − Y` the base at radius `r` less the luminance:
//! - Texture adds `(amount/100)·TEXTURE_GAIN·(d_fine − d_mid)`, the detail
//!   between the fine and the mid radius. The finest detail (under the fine
//!   radius, where pixel noise lives) is in neither base's difference, so it is
//!   not amplified.
//! - Clarity adds `(amount/100)·CLARITY_GAIN·4Y(1 − Y)·(d_mid − d_coarse)`, the
//!   detail between the mid and the coarse radius, weighted by `4Y(1 − Y)`: 1 at
//!   mid-grey and 0 at black and white, so highlights and shadows are not
//!   pushed into clipping.
//!
//! A negative amount subtracts the same band. The sum is a LUMINANCE delta
//! added to R, G and B alike (`sampling_denoise::applied_in`, which clamps to
//! 0…1 at `Unit`, unclamped at `Extended`, and keeps alpha), so hue and
//! saturation hold. Dehaze
//! (`sampling_dehaze`) runs first and the bases are taken of the dehazed
//! picture, through a closure over the source, the airlight and the veil, not
//! a second plane. A pixel with alpha 0 is returned as read.
use crate::filter_kernels::SampleRange;
use crate::job_control::JobContext;
use crate::precision_image::{pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use crate::sampling_dehaze::{airlight, dehaze_in, unit_rgb, veil};
use crate::sampling_denoise::{applied_in, guided_bounded_in, opponent, plan_bounded};

/// The radii below are stated for a plane whose longer side is this many
/// pixels and scale with it, so the look belongs to the picture and not to its
/// pixel count: a 6000-pixel photograph and the 1024-pixel proxy the sheet
/// previews it from get the same result at their own sizes. 2048 is an
/// ordinary screen-sized picture, where the radii are easy to reason about.
pub(crate) const REFERENCE_SIDE: f64 = 2048.0;
/// Texture's lower edge at the reference side: a box of half-width 1 (3
/// pixels) removes the pixel-to-pixel alternation that is sensor noise, so the
/// band starts just above it.
const FINE_RADIUS: f64 = 1.0;
/// Where Texture ends and Clarity begins: half-width 6 (13 pixels) at the
/// reference side passes detail a few pixels across (pores, fabric, foliage)
/// into Texture and leaves forms tens of pixels across to Clarity.
const MID_RADIUS: f64 = 6.0;
/// Clarity's upper edge: half-width 40 (81 pixels, 4% of the reference side).
/// Wider and Clarity becomes a tone curve over whole regions; narrower and it
/// overlaps Texture.
const COARSE_RADIUS: f64 = 40.0;
/// Dehaze's veil is smoothed at half-width 24 (49 pixels, 2.4% of the
/// reference side): wide enough to average a textured surface into one haze
/// level, narrow enough that a depth edge is inside few windows.
const HAZE_RADIUS: f64 = 24.0;
/// Every radius stops here: `sampling_denoise::plan_bounded` admits a sigma of
/// at most 64, which `base_delta`'s `(radius − 0.5)/3` reaches just above 192.
pub(crate) const RADIUS_LIMIT: u32 = 192;
/// The guided filter's epsilon for the three bases, as the level
/// `sampling_denoise` squares. A window whose luminance deviates by much less
/// than this (texture, at a few percent of the range) is averaged; one holding
/// a step much taller (an edge, tens of percent) is followed. 0.05, about 13
/// 8-bit levels, puts the boundary between the two.
const BASE_EPSILON_LEVEL: f64 = 0.05;
/// Amount 100 adds the whole band once, doubling its contrast; amount −100
/// removes it. The band tests in `tests/filter_presence.rs` hold at 1.0 with
/// the bounds they declared before measuring, so neither gain is tuned.
const TEXTURE_GAIN: f64 = 1.0;
const CLARITY_GAIN: f64 = 1.0;
/// One kept plane, the veil or one base delta: a binary32 a pixel.
const KEPT_BYTES_PER_PIXEL: u64 = 4;
/// The plan's work for the passes that are not a guided base: the airlight's
/// two reads and the final sum and clamp, a few units a pixel.
const WORK_PER_PIXEL: u64 = 16;
/// [`Presence::plan`]'s per-pixel term for the result, the n×16 plane of
/// straight samples the arm returns. It is live with every kept plane but
/// never with a guided pass's transient, which is the same size and is charged
/// through [`base_plan`].
///
/// Record (`scripts/presence-admission/measure.py`: texture 50, clarity
/// 50, dehaze 50, so all four guided passes run; the Release app at
/// `DR_WORKING_LIMIT_BYTES` 8 GiB, `apply_filter` on a one-layer 6000×4000
/// document of two-colour Clouds; `MallocLargeCache=0`, E =
/// `ri_interval_max_phys_footprint` − pre around the one call, a fresh process
/// per case, 3 runs + a baseline twin each). This plan is 836,931,456 there:
/// the coarse pass's n×16 correction plane, its 62,640,000 of rings, 4 MiB and
/// its n×4 delta, the three planes kept before it, and 2 MiB.
/// - 16-bit: E 771,719,720 / 771,670,568 / 771,654,160 (baseline 0 / 0 /
///   16,384) against `selected_edit_plan` 1,083,222,912 (1.404×), which this
///   plan sets. The peak is 32n, the four kept planes beside one n×16 plane:
///   a pass's rings are freed before its delta is built, so the plan's charge
///   for both at once is margin.
/// - 8-bit: E 866,779,736 / 865,960,536 / 866,845,296 (baseline 0 ×3): the
///   peak is the commit's, the figure Lens Correction's record has for the
///   same document, and the host charges it. `filter_op_scratch_bytes` is
///   1,051,407,200, above `ByteFilterAdmission.commitStageBytes`
///   1,020,000,000, so the host admits the plan, plus what the edit leaves
///   retained (192,003,794 in every run): 1,243,410,994, 1.434×,
///   `ProjectionMargin`'s rule (1.213× before the retained term).
const PLANE_BYTES_PER_PIXEL: u64 = 16;

/// Filters ▸ Presence: each amount is −100…100 and 0 is off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Presence {
    pub texture: f64,
    pub clarity: f64,
    pub dehaze: f64,
}

/// The box half-widths, in pixels, the three kernels run at on one plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PresenceRadii {
    pub fine: u32,
    pub mid: u32,
    pub coarse: u32,
    pub haze: u32,
}

/// The radii for a plane of `dimensions`: the reference radii scaled by
/// `max(w, h) / REFERENCE_SIDE`, rounded, kept apart (`fine < mid < coarse`, so
/// each band is a real difference on a small plane) and each at most
/// `RADIUS_LIMIT`.
pub(crate) fn presence_radii(dimensions: (u32, u32)) -> PresenceRadii {
    let unit = f64::from(dimensions.0.max(dimensions.1)) / REFERENCE_SIDE;
    let scaled = |radius: f64| (radius * unit).round() as u32;
    let fine = scaled(FINE_RADIUS).max(1);
    let mid = scaled(MID_RADIUS).max(fine + 1);
    let coarse = scaled(COARSE_RADIUS).max(mid + 1);
    PresenceRadii {
        fine: fine.min(RADIUS_LIMIT),
        mid: mid.min(RADIUS_LIMIT),
        coarse: coarse.min(RADIUS_LIMIT),
        haze: scaled(HAZE_RADIUS).clamp(2, RADIUS_LIMIT),
    }
}

/// The sigma `sampling_denoise::radii` maps to exactly `radius`: it takes
/// `ceil(3·sigma)`, and `3·(radius − 0.5)/3` is within a rounding of
/// `radius − 0.5`, whose ceiling is `radius`.
fn sigma(radius: u32) -> f64 {
    (f64::from(radius) - 0.5) / 3.0
}

/// `(scratch, work)` of ONE base pass: `sampling_denoise::plan_bounded`'s
/// luminance-only figures (the n×16 correction plane and the streamed
/// statistics) plus the n×4 plane [`base_delta`] returns, which is built while
/// the correction plane is still held.
pub(crate) fn base_plan(
    dimensions: (u32, u32),
    radius: u32,
    cap: u64,
) -> Result<(u64, u64), String> {
    let (scratch, work) =
        plan_bounded(dimensions.0, dimensions.1, sigma(radius), true, false, cap)?;
    let n = pixel_count(dimensions.0, dimensions.1)? as u64;
    Ok((
        scratch.saturating_add(n.saturating_mul(KEPT_BYTES_PER_PIXEL)),
        work,
    ))
}

/// `B_r(Y) − Y` per pixel: the edge-aware luminance base at `radius`, less the
/// luminance; 0 for a pixel with no visible neighbour. One call of
/// `sampling_denoise::guided_bounded_in` with the chroma half off, of whose n×16
/// plane only the luminance slot is kept.
pub(crate) fn base_delta(
    read: impl Fn(usize) -> [f32; 4],
    dimensions: (u32, u32),
    radius: u32,
    epsilon_level: f64,
    cap: u64,
    job: JobContext<'_>,
) -> Result<Vec<f32>, String> {
    base_delta_in(
        read,
        dimensions,
        radius,
        epsilon_level,
        cap,
        &SampleRange::Unit,
        job,
    )
}

/// [`base_delta`] in `range`: the guided base is
/// `sampling_denoise::guided_bounded_in`'s, unclamped at `Extended`.
pub(crate) fn base_delta_in(
    read: impl Fn(usize) -> [f32; 4],
    dimensions: (u32, u32),
    radius: u32,
    epsilon_level: f64,
    cap: u64,
    range: &SampleRange,
    job: JobContext<'_>,
) -> Result<Vec<f32>, String> {
    let plane = guided_bounded_in(
        dimensions.0,
        dimensions.1,
        sigma(radius),
        epsilon_level,
        true,
        false,
        cap,
        range,
        read,
        job,
    )?;
    let mut out: Vec<f32> = reserved_vec(plane.len())?;
    for (i, sample) in plane.iter().enumerate() {
        job.every(i)?;
        out.push(sample[0]);
    }
    Ok(out)
}

/// Texture's luminance delta from the fine and mid base deltas of one pixel.
pub(crate) fn texture(fine: f32, mid: f32, amount: f64) -> f64 {
    amount / 100.0 * TEXTURE_GAIN * (f64::from(fine) - f64::from(mid))
}

/// Clarity's luminance delta from the mid and coarse base deltas of one pixel
/// whose luminance is `luma`.
pub(crate) fn clarity(mid: f32, coarse: f32, luma: f64, amount: f64) -> f64 {
    let y = luma.clamp(0.0, 1.0);
    amount / 100.0 * CLARITY_GAIN * 4.0 * y * (1.0 - y) * (f64::from(mid) - f64::from(coarse))
}

impl Presence {
    /// All three off: the exact bypass.
    pub(crate) fn identity(&self) -> bool {
        self.texture == 0.0 && self.clarity == 0.0 && self.dehaze == 0.0
    }

    /// The radius of each guided pass this setting runs, in run order: the
    /// veil (a positive Dehaze only; a negative one is pointwise), then the
    /// fine, mid and coarse bases. The mid base serves both bands and is run
    /// once.
    fn passes(&self, radii: PresenceRadii) -> [Option<u32>; 4] {
        let (texture, clarity) = (self.texture != 0.0, self.clarity != 0.0);
        [
            (self.dehaze > 0.0).then_some(radii.haze),
            texture.then_some(radii.fine),
            (texture || clarity).then_some(radii.mid),
            clarity.then_some(radii.coarse),
        ]
    }

    /// `(scratch, work)`, refused before any allocation when the work is above
    /// `cap`. The work is every guided pass's plus `WORK_PER_PIXEL`. The
    /// scratch is the larger of two stages, plus two bookkeeping allowances
    /// (the airlight's histogram is inside one): the widest guided pass, with
    /// the planes kept from the passes before it, and the result, with every
    /// kept plane. A pass's transient and the result are never held together.
    pub(crate) fn plan(&self, dimensions: (u32, u32), cap: u64) -> Result<(u64, u64), String> {
        let n = pixel_count(dimensions.0, dimensions.1)? as u64;
        let mut work = n.saturating_mul(WORK_PER_PIXEL);
        let (mut widest, mut kept) = (0_u64, 0_u64);
        for radius in self
            .passes(presence_radii(dimensions))
            .into_iter()
            .flatten()
        {
            let (scratch, pass) = base_plan(dimensions, radius, u64::MAX)?;
            work = work.saturating_add(pass);
            widest = widest.max(scratch);
            kept += 1;
        }
        if work > cap {
            return Err("presence exceeds the bounded pixel-work limit".into());
        }
        let plane = n.saturating_mul(KEPT_BYTES_PER_PIXEL);
        let guided = widest.saturating_add(kept.saturating_sub(1).saturating_mul(plane));
        let result = n
            .saturating_mul(PLANE_BYTES_PER_PIXEL)
            .saturating_add(kept.saturating_mul(plane));
        Ok((
            guided.max(result).saturating_add(2 * BOOKKEEPING_BYTES),
            work,
        ))
    }

    /// The final straight samples (RGB and alpha in 0…1 at `Unit`): Dehaze, then Texture
    /// and Clarity of the dehazed picture. Each guided pass has an equal share
    /// of the progress and the final sum the last one. The 8- and 16-bit
    /// form: [`Self::plane_in`] at `SampleRange::Unit`.
    pub(crate) fn plane(
        &self,
        read: impl Fn(usize) -> [f32; 4],
        dimensions: (u32, u32),
        cap: u64,
        job: JobContext<'_>,
    ) -> Result<Vec<[f32; 4]>, String> {
        self.plane_in(read, dimensions, cap, &SampleRange::Unit, job)
    }

    /// [`Self::plane`] in `range`. At `Extended` the bases are
    /// `base_delta_in`'s and the luminance delta is added by
    /// `sampling_denoise::applied_in`, neither clamped to 0…1, and Dehaze is
    /// `sampling_dehaze`'s rule D.
    ///
    /// Presence at 32 bits: Texture, Clarity and Dehaze run on the layer's
    /// DISPLAY-ENCODED copy (`display_encoding`), where the base epsilon and
    /// the bands are the 16-bit ones; Clarity's `4Y(1 − Y)` reads the encoded
    /// luma clamped to 0…1, so a pixel whose luma is above white gets no
    /// Clarity, and a value above white is never clamped to it. Dehaze's
    /// airlight and veil read the copy through `unit_rgb` (RGB clamped to
    /// 0…1) and its recovery holds a component above white as read
    /// (`sampling_dehaze::dehaze_in`).
    pub(crate) fn plane_in(
        &self,
        read: impl Fn(usize) -> [f32; 4],
        dimensions: (u32, u32),
        cap: u64,
        range: &SampleRange,
        job: JobContext<'_>,
    ) -> Result<Vec<[f32; 4]>, String> {
        self.plan(dimensions, cap)?;
        let n = pixel_count(dimensions.0, dimensions.1)?;
        let [haze, fine, mid, coarse] = self.passes(presence_radii(dimensions));
        let stages = [haze, fine, mid, coarse].iter().flatten().count() + 1;
        let mut stage = 0_usize;
        let mut share = || {
            let range = job.range(
                stage as f64 / stages as f64,
                (stage + 1) as f64 / stages as f64,
            );
            stage += 1;
            range
        };
        let amount = self.dehaze;
        // No visible pixel has no airlight, and nothing to dehaze either.
        // Rule D: the haze estimate reads RGB clamped to 0…1, which is the
        // sample itself at `Unit`.
        let clipped = |i: usize| unit_rgb(read(i));
        let light = if amount != 0.0 {
            airlight(clipped, n, job.checkpoints_only())?
        } else {
            None
        };
        let veiled = match (light, haze) {
            (Some(light), Some(radius)) => {
                Some(veil(clipped, dimensions, light, radius, cap, share())?)
            }
            (None, Some(_)) => {
                share();
                None
            }
            _ => None,
        };
        let hazed = |i: usize| match light {
            Some(light) => dehaze_in(
                read(i),
                light,
                veiled.as_ref().map_or(0.0, |v| v[i]),
                amount,
                range,
            ),
            None => read(i),
        };
        let mut base = |radius: Option<u32>| {
            radius
                .map(|r| {
                    base_delta_in(
                        hazed,
                        dimensions,
                        r,
                        BASE_EPSILON_LEVEL,
                        cap,
                        range,
                        share(),
                    )
                })
                .transpose()
        };
        let fine = base(fine)?;
        let mid = base(mid)?;
        let coarse = base(coarse)?;
        let job = share();
        let (w, h) = (dimensions.0 as usize, dimensions.1 as usize);
        let mut out: Vec<[f32; 4]> = reserved_vec(n)?;
        for row in 0..h {
            for i in row * w..(row + 1) * w {
                job.every(i)?;
                let sample = hazed(i);
                if sample[3] <= 0.0 || mid.is_none() {
                    out.push(sample);
                    continue;
                }
                let mut delta = 0.0_f64;
                if let (Some(fine), Some(mid)) = (&fine, &mid) {
                    delta += texture(fine[i], mid[i], self.texture);
                }
                if let (Some(mid), Some(coarse)) = (&mid, &coarse) {
                    delta += clarity(mid[i], coarse[i], opponent(sample).0, self.clarity);
                }
                out.push(applied_in(sample, delta, 0.0, 0.0, range));
            }
            job.report((row + 1) as f64 / h as f64, 6)?;
        }
        Ok(out)
    }
}
