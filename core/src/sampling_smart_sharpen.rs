//! Filter ▸ Sharpen ▸ Smart Sharpen: Amount, Radius, Reduce Noise,
//! Remove (Gaussian Blur, Lens Blur, Motion Blur with an Angle) and the
//! Shadows and Highlights fades, one plane of straight samples, a
//! `SpatialKernel` arm on the one spatial route (`SelectedFilter::Spatial` at
//! 16 bits, `filter_op` at 8, the float route's display domain at 32; no new
//! export). The export workspace's Smart Sharpen output sharpening
//! (`export_sharpen`) runs the same arm.
//!
//! Every stage is an existing kernel, called, never copied:
//!
//! 1. **Reduce Noise.** At noise > 0 the BASE `D` is Reduce Noise's luminance
//!    half: `sampling_presence::base_delta_in` at box half-width
//!    `NOISE_RADIUS` (σ 0.5) and level `noise/100 · NOISE_LEVEL_AT_FULL`,
//!    added through `sampling_denoise::applied_in`, which is bit for bit
//!    `reduce_noise {radius 0.5, luminance noise/1000, detail 0, color 0}`
//!    before quantization. At noise 0, `D` is the source. A pixel with alpha
//!    0 keeps its colour, as Reduce Noise leaves it.
//! 2. **The blur to remove** is a low-pass `B` of `D`: Gaussian Blur is
//!    `filter_kernels::neighborhood_plane_in`'s Gaussian at σ = Radius (the
//!    kernel Unsharp Mask reads); Lens Blur is `sampling_disc::disc_plane`'s
//!    disc mean of radius Radius (a fractional radius blends the two whole
//!    discs around it), the point-spread function of defocus; Motion Blur is
//!    `sampling_motion::motion_plane`'s line mean at Angle over
//!    [`motion_distance`] pixels.
//! 3. **The sharpen** is Unsharp Mask's combine (`filter_kernels::evaluated_in`
//!    with threshold 0) of `D` against `B` at `amount/100 · w`, `w` the tonal
//!    fade weight, so Gaussian with Reduce Noise 0 and both fades at 0 is
//!    `unsharp_mask` at the same amount and radius, threshold 0, bit for bit.
//! 4. **Lens Blur removal limits halos.** It is one Jansson–van Cittert
//!    iteration (`D + a·(D − B)`) with the lens (disc) PSF, then each RGB
//!    channel is projected onto `[lo − LENS_OVERSHOOT, hi + LENS_OVERSHOOT]`,
//!    `lo`/`hi` the least and greatest of `D` over the clamped 3×3 window's
//!    visible pixels. The projection onto the local-extrema box is what bounds
//!    a halo: an edge is steepened but cannot overshoot either side by more
//!    than two 8-bit levels, where Unsharp Mask's overshoot grows with the
//!    amount. A disc PSF is defocus's, which is the blur a lens leaves, and
//!    one iteration costs one disc pass, where more iterations amplify noise.
//!    Gaussian and Motion are not projected: they are a plain sharpen
//!    against their own blur.
//! 5. **Shadows and Highlights fades.** `L` is the alpha-weighted box mean,
//!    half-width the section's Radius, of `D`'s Rec. 709 luma (`blend::LUMA_*`)
//!    clamped to 0…1, streamed through two `sampling_box::BoxRows` (0 where the
//!    window holds no visible pixel). [`fade_weight`] is
//!    `(1 − fs·s(Ls))·(1 − fh·h(Lh))`, `s(L) = clamp(1 − L/Ts, 0, 1)`,
//!    `h(L) = clamp(1 − (1 − L)/Th, 0, 1)`, `f = fade/100`,
//!    `T = tonal_width/100`: a pixel whose neighbourhood is darker than the
//!    Shadows tonal width is sharpened less the darker it is, the Highlights
//!    mirrored, and a term with `T = 0` or `f = 0` is exactly 1.
//!
//! Alpha is the source's and a pixel with alpha 0 is returned as read: colour
//! only, no frame. Never an identity: amount is at least 1 %.
//!
//! **32 bits (`float_filter_route`, `Domain::Display`).** Smart Sharpen runs
//! whole on the layer's DISPLAY-ENCODED copy (`display_encoding`), as legacy
//! Sharpen and Reduce Noise do, so Reduce Noise's level, `LENS_OVERSHOOT` and
//! the tonal widths mean their 16-bit values on every value inside 0…1. The
//! fade classifier reads the encoded luma clamped to 0…1 (a pixel above white
//! has `L = 1`), and nothing is clamped (`SampleRange::color`), so a value
//! above white is sharpened in code and decoded back above white. The
//! Gaussian removal there is a float layer's Gaussian, the FIR at every σ
//! with tails cut under 1e-7 (`sampling_gaussian::runs_fast`), wider than the
//! 16-bit `ceil(3σ)` kernel: at Reduce Noise 0 and no fade it is legacy
//! Sharpen's float route bit for bit, not the 16-bit result encoded.
use crate::blend::{LUMA_B, LUMA_G, LUMA_R};
use crate::filter_kernels::SampleRange;
use crate::filter_kernels::{evaluated_in, neighborhood_plane_in, FilterKind, Neighborhood};
use crate::job_control::JobContext;
use crate::precision_image::{pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use crate::sampling_box::BoxRows;
use crate::sampling_denoise::applied_in;
use crate::sampling_disc::{disc_plan, disc_plane};
use crate::sampling_motion::{motion_plan, motion_plane};
use crate::sampling_presence::{base_delta_in, base_plan};

/// Which blur the sharpen removes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SharpenRemove {
    Gaussian,
    Lens,
    Motion,
}

/// One of the Shadows / Highlights sections: Fade Amount and Tonal Width in
/// percent, Radius the classifier's box half-width in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TonalFade {
    pub fade: f64,
    pub tonal_width: f64,
    pub radius: u32,
}

/// Filter ▸ Sharpen ▸ Smart Sharpen's settings: amount in percent (1…500),
/// radius in pixels (0.1…64), noise in percent (0…100), angle in degrees
/// (read only by Motion Blur).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SmartSharpen {
    pub amount: f64,
    pub radius: f64,
    pub noise: f64,
    pub remove: SharpenRemove,
    pub angle: f64,
    pub shadows: TonalFade,
    pub highlights: TonalFade,
}

/// The request keys, in the sheet's order (`apply_filter`, the selected
/// edit, `dr_image_filter_op` and the export request's Smart method).
pub(crate) const SMART_SHARPEN_KEYS: [&str; 11] = [
    "amount",
    "radius",
    "noise",
    "remove",
    "angle",
    "shadow_fade",
    "shadow_tonal_width",
    "shadow_radius",
    "highlight_fade",
    "highlight_tonal_width",
    "highlight_radius",
];

/// Lens Blur removal's projection slack beyond the window's extremes: two
/// 8-bit levels, enough that a soft edge visibly gains contrast at its
/// shoulders, small enough that no halo is seen beside a hard one.
pub(crate) const LENS_OVERSHOOT: f64 = 2.0 / 255.0;
/// Reduce Noise's box half-width: `sampling_presence::base_delta_in`'s σ
/// `(2 − 0.5)/3 = 0.5`, the 5×5 window that holds pixel-scale grain and no
/// edge structure wider than it.
const NOISE_RADIUS: u32 = 2;
/// Reduce Noise 100 % removes luminance noise up to this level (normalized),
/// Reduce Noise's own `luminance` at 0.1: about 25 8-bit levels, the heavy
/// end of sensor grain. Linear in the percentage below it.
pub(crate) const NOISE_LEVEL_AT_FULL: f64 = 0.1;
/// One kept plane, the noise delta or one fade classifier: a binary32 a
/// pixel.
const KEPT_BYTES_PER_PIXEL: u64 = 4;
/// Each stage's allowance beside what it plans: the measured admission's
/// knob (a miss raises this, never a new constant).
///
/// Record (`scripts/smart-sharpen-admission/measure.py`, Lens
/// radius 4, amount 200, Reduce Noise 50, both fades 50 at width 50 and
/// radius 8, so every stage runs; the Release app at
/// `DR_WORKING_LIMIT_BYTES` 8 GiB, a 6000×4000 document of two-colour Clouds,
/// `MallocLargeCache=0`, E = `ri_interval_max_phys_footprint` − pre around the
/// one call, a fresh process per case, 3 runs + a baseline twin at E 0 each).
/// [`SmartSharpen::plan`] is 774,357,724 there: the disc stage beside the
/// three kept planes and the margin. No constant was raised.
/// - 8-bit `apply_filter`: E 863,765,056 / 863,683,160 / 863,158,848, the
///   commit's peak; `filter_op_scratch_bytes` 973,190,035 is below
///   `ByteFilterAdmission.commitStageBytes` 1,020,000,000, so the host admits
///   that plus what the edit leaves retained (192,003,794): 1,212,003,794,
///   1.403×.
/// - 16-bit `apply_filter`: E 675,561,952 / 675,496,392 / 675,447,240
///   against `selected_edit_plan` 1,020,649,180 (1.511×).
/// - 8-bit `export_image` (PNG, size none): E 771,146,232 / 771,146,232 /
///   771,211,768 against the host's 64 B/px + 16 MiB, 1,552,777,216 (2.013×).
/// - 16-bit `export_image`: E 773,767,720 / 773,882,408 / 773,947,944
///   against the typed `export_plan` 2,445,720,028 (3.160×; its 48n encoder
///   term is the PNG writer's own).
/// - 32-bit (`tests/float_adjust_apply_24mp.rs::measure`): `float_filter_apply`'s
///   record, 1.314×.
const STAGE_MARGIN_BYTES_PER_PIXEL: u64 = 4;
/// The plan's work for a pass that is not a kernel's: one fade classifier's
/// two streamed box sums, or the final combine with Lens's nine reads of the
/// base.
const WORK_PER_PIXEL: u64 = 32;

/// Motion Blur removal's line length: a line mean of `d + 1` samples has a
/// spread of about `d/2` either side, so `2·radius` matches the Gaussian's
/// reach at the same Radius.
pub(crate) fn motion_distance(radius: f64) -> u32 {
    ((2.0 * radius).round() as u32).max(1)
}

/// Lens Blur removal's largest disc: the whole disc at or above the radius.
fn lens_reach(radius: f64) -> u32 {
    (radius.ceil() as u32).max(1)
}

/// One section's term: 1 where it does not apply.
fn fade_term(section: &TonalFade, ramp: f64) -> f64 {
    let (f, t) = (section.fade / 100.0, section.tonal_width / 100.0);
    if f == 0.0 || t == 0.0 {
        return 1.0;
    }
    1.0 - f * ramp.clamp(0.0, 1.0)
}

/// The amount's weight at a pixel whose Shadows classifier reads
/// `shadow_luma` and whose Highlights classifier reads `highlight_luma`
/// (module doc, 5).
pub(crate) fn fade_weight(
    shadows: &TonalFade,
    highlights: &TonalFade,
    shadow_luma: f64,
    highlight_luma: f64,
) -> f64 {
    let ts = shadows.tonal_width / 100.0;
    let th = highlights.tonal_width / 100.0;
    let s = if ts > 0.0 {
        1.0 - shadow_luma / ts
    } else {
        0.0
    };
    let h = if th > 0.0 {
        1.0 - (1.0 - highlight_luma) / th
    } else {
        0.0
    };
    fade_term(shadows, s) * fade_term(highlights, h)
}

/// A section's classifier radius when it runs (a fade and a width above 0).
fn active(section: &TonalFade) -> Option<u32> {
    (section.fade > 0.0 && section.tonal_width > 0.0).then_some(section.radius)
}

/// The Rec. 709 luma of a straight sample, clamped to 0…1.
fn unit_luma(p: [f32; 4]) -> f64 {
    (f64::from(LUMA_R) * f64::from(p[0])
        + f64::from(LUMA_G) * f64::from(p[1])
        + f64::from(LUMA_B) * f64::from(p[2]))
    .clamp(0.0, 1.0)
}

/// The alpha-weighted box mean of `base`'s luma at half-width `radius`, one
/// binary32 a pixel (module doc, 5).
fn tone_plane(
    base: impl Fn(usize) -> [f32; 4],
    dimensions: (u32, u32),
    radius: u32,
    job: JobContext<'_>,
) -> Result<Vec<f32>, String> {
    let (w, h) = (dimensions.0 as usize, dimensions.1 as usize);
    let r = radius as usize;
    let mut weighted = BoxRows::new(w, h, r)?;
    let mut weights = BoxRows::new(w, h, r)?;
    let mut out: Vec<f32> = reserved_vec(w * h)?;
    for _ in 0..h {
        weighted.push(
            |i| {
                let p = base(i);
                f64::from(p[3]) * unit_luma(p)
            },
            job,
        )?;
        weights.push(|i| f64::from(base(i)[3]), job)?;
        while weighted.ready() {
            let (sum, alpha) = (weighted.emit(job)?, weights.emit(job)?);
            for x in 0..w {
                // A window of no visible pixel (or only the sliding sums'
                // rounding residue) reads 0; the clamp absorbs the residue.
                out.push(if alpha[x] > 0.0 {
                    (sum[x] / alpha[x]).clamp(0.0, 1.0) as f32
                } else {
                    0.0
                });
            }
            job.report(out.len() as f64 / (w * h) as f64, 6)?;
        }
    }
    Ok(out)
}

impl SmartSharpen {
    /// The classifier radii this setting runs, Shadows then Highlights.
    fn tones(&self) -> [Option<u32>; 2] {
        [active(&self.shadows), active(&self.highlights)]
    }

    /// [`Self::plan_in`] at `SampleRange::Unit` (8 and 16 bits).
    pub(crate) fn plan(&self, dimensions: (u32, u32), cap: u64) -> Result<(u64, u64), String> {
        self.plan_in(dimensions, cap, &SampleRange::Unit)
    }

    /// `(scratch, work)`, refused before any allocation when the work is
    /// above `cap`. Stages run in order, each beside the planes kept before
    /// it: Reduce Noise (`sampling_presence::base_plan`, which includes the
    /// n×4 delta it keeps), each fade classifier (its two `BoxRows` rings and
    /// its n×4 plane), then the low-pass (the kernel's own plan, whose n×16
    /// plane the combine overwrites in place). Each stage adds
    /// `STAGE_MARGIN_BYTES_PER_PIXEL`; the scratch is the largest stage plus
    /// two bookkeeping allowances, the work every stage's sum. Only the
    /// Gaussian reads `range` (a float layer's is the FIR at every σ).
    pub(crate) fn plan_in(
        &self,
        dimensions: (u32, u32),
        cap: u64,
        range: &SampleRange,
    ) -> Result<(u64, u64), String> {
        let n = pixel_count(dimensions.0, dimensions.1)? as u64;
        let (w, h) = (u64::from(dimensions.0), u64::from(dimensions.1));
        let plane = n.saturating_mul(KEPT_BYTES_PER_PIXEL);
        let margin = n.saturating_mul(STAGE_MARGIN_BYTES_PER_PIXEL);
        let mut work = n.saturating_mul(WORK_PER_PIXEL);
        let (mut widest, mut kept) = (0_u64, 0_u64);
        if self.noise > 0.0 {
            let (scratch, pass) = base_plan(dimensions, NOISE_RADIUS, u64::MAX)?;
            widest = widest.max(scratch.saturating_add(margin));
            work = work.saturating_add(pass);
            kept = kept.saturating_add(plane);
        }
        for radius in self.tones().into_iter().flatten() {
            let r = u64::from(radius);
            let rings = BoxRows::bytes(w, h, r).saturating_mul(2);
            widest = widest.max(
                kept.saturating_add(plane)
                    .saturating_add(rings)
                    .saturating_add(margin),
            );
            // Two box sums re-seeded once a row and once a column.
            let seeding = 4 * (2 * r + 1) * (w + h);
            work = work
                .saturating_add(n.saturating_mul(WORK_PER_PIXEL))
                .saturating_add(seeding);
            kept = kept.saturating_add(plane);
        }
        let (low, pass) = match self.remove {
            SharpenRemove::Gaussian => crate::sampling_gaussian::plan_bounded(
                dimensions.0,
                dimensions.1,
                self.radius,
                range,
                u64::MAX,
            )?,
            SharpenRemove::Lens => disc_plan(dimensions, lens_reach(self.radius), u64::MAX)?,
            SharpenRemove::Motion => {
                motion_plan(dimensions, motion_distance(self.radius), u64::MAX)?
            }
        };
        widest = widest.max(kept.saturating_add(low).saturating_add(margin));
        work = work.saturating_add(pass);
        if work > cap {
            return Err("smart sharpen exceeds the bounded pixel-work limit".into());
        }
        Ok((widest.saturating_add(2 * BOOKKEEPING_BYTES), work))
    }

    /// The final straight samples in `range` (module doc): the base, the
    /// low-pass of the blur to remove, the fade-weighted Unsharp Mask combine
    /// and, for Lens Blur, the projection, written in place over the
    /// low-pass plane. Each stage has an equal share of the progress.
    pub(crate) fn plane_in(
        &self,
        read: impl Fn(usize) -> [f32; 4],
        dimensions: (u32, u32),
        cap: u64,
        range: &SampleRange,
        job: JobContext<'_>,
    ) -> Result<Vec<[f32; 4]>, String> {
        self.plan_in(dimensions, cap, range)?;
        let tones = self.tones();
        let stages = usize::from(self.noise > 0.0) + tones.iter().flatten().count() + 2;
        let mut stage = 0_usize;
        let mut share = || {
            let part = job.range(
                stage as f64 / stages as f64,
                (stage + 1) as f64 / stages as f64,
            );
            stage += 1;
            part
        };
        let delta = if self.noise > 0.0 {
            Some(base_delta_in(
                &read,
                dimensions,
                NOISE_RADIUS,
                self.noise / 100.0 * NOISE_LEVEL_AT_FULL,
                cap,
                range,
                share(),
            )?)
        } else {
            None
        };
        // Reduce Noise's own combine at detail 0 and colour 0: a pixel with
        // alpha 0 is left as read.
        let base = |i: usize| -> [f32; 4] {
            let p = read(i);
            match &delta {
                Some(dy) if p[3] > 0.0 => applied_in(p, f64::from(dy[i]), 0.0, 0.0, range),
                _ => p,
            }
        };
        let mut tone = |radius: Option<u32>| {
            radius
                .map(|r| tone_plane(base, dimensions, r, share()))
                .transpose()
        };
        let shadow = tone(tones[0])?;
        let highlight = tone(tones[1])?;
        let job_low = share();
        let mut low = match self.remove {
            SharpenRemove::Gaussian => neighborhood_plane_in(
                Neighborhood::Gaussian { sigma: self.radius },
                dimensions,
                cap,
                range,
                base,
                job_low,
            )?,
            SharpenRemove::Lens => disc_plane(
                base,
                dimensions,
                |_, _| self.radius,
                lens_reach(self.radius),
                None,
                range.alpha_floor(),
                job_low,
            )?,
            SharpenRemove::Motion => motion_plane(
                base,
                dimensions,
                self.angle,
                motion_distance(self.radius),
                range,
                job_low,
            )?,
        };
        let job = share();
        let (w, h) = (dimensions.0 as usize, dimensions.1 as usize);
        let lens = self.remove == SharpenRemove::Lens;
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                job.every(i)?;
                let source = read(i);
                if source[3] <= 0.0 {
                    low[i] = source;
                    continue;
                }
                let d = base(i);
                let weight = fade_weight(
                    &self.shadows,
                    &self.highlights,
                    shadow.as_ref().map_or(0.0, |l| f64::from(l[i])),
                    highlight.as_ref().map_or(0.0, |l| f64::from(l[i])),
                );
                let kind = FilterKind::UnsharpMask {
                    amount: self.amount / 100.0 * weight,
                    threshold: 0.0,
                };
                let mut out = evaluated_in(kind, d, low[i], range);
                if lens {
                    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
                    for yy in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                        for xx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                            let p = base(yy * w + xx);
                            if p[3] <= 0.0 {
                                continue;
                            }
                            for c in 0..3 {
                                lo[c] = lo[c].min(f64::from(p[c]));
                                hi[c] = hi[c].max(f64::from(p[c]));
                            }
                        }
                    }
                    for c in 0..3 {
                        // Defensive: the window holds its own opaque pixel, so
                        // its range inverts only if every sample is NaN, which
                        // float sanitizing keeps out; `clamp` would panic on it.
                        if lo[c] <= hi[c] {
                            out[c] = f64::from(out[c])
                                .clamp(lo[c] - LENS_OVERSHOOT, hi[c] + LENS_OVERSHOOT)
                                as f32;
                        }
                    }
                }
                low[i] = out;
            }
            job.report((y + 1) as f64 / h as f64, 6)?;
        }
        Ok(low)
    }
}
