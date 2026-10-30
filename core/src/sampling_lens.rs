//! Filters ▸ Lens Correction: radial distortion, transverse chromatic
//! aberration, vignette and a perspective / angle / scale transform on one
//! plane of straight samples, a `SpatialKernel` arm on the one spatial route
//! (`SelectedFilter::Spatial` at 16 bits, `filter_op` at 8; no new export).
//!
//! The geometry is an inverse map. For the output pixel centre `p` on a W×H
//! plane, `c = (W/2, H/2)` and `R = max(W, H)/2` (RAW development's
//! normalisation): `n = (p − c)/R`, divided by `scale/100`, turned by the angle
//! (`(n.x cos θ − n.y sin θ, n.x sin θ + n.y cos θ)`, so a positive angle turns
//! the picture counterclockwise as seen on the y-down plane), divided by
//! `d = 1 + h·n.x + v·n.y` (the perspective; the point is outside the picture
//! when `d ≤ 0`), and finally mapped through `lens_model::radial_map` with `k1` from
//! Distortion and the channel's lateral scale from the two chromatic sliders.
//! The radial map and its fold test are `lens_model`'s, the copy RAW
//! development's `Lens` source map uses, so the two corrections are one
//! arithmetic. A point past the first fold radius (`lens_model::radial_monotone`
//! on the pre-radial `n`, which covers every enlargement Scale and Perspective
//! make) is outside the picture: with it, a barrel correction would paint a
//! mirrored band.
//!
//! Sampling is `sampling::map_into` with Catmull-Rom taps, premultiplied
//! accumulation in binary64 as `raw_source_geometry` does, one pass when the
//! chromatic sliders are 0 and three (one per colour channel, alpha from green)
//! otherwise; a point outside the picture drops every tap, so the edge is
//! transparent. The vignette is last: `lens_model::vignette_stops` at the green
//! source point's fraction of `lens_model::vignette_radius`, applied as the
//! sRGB light-linear gain of `adjust_tone::Exposure::gain_only` (RAW's manual
//! vignette uses the same gain in its own basis). The plane holds the corrected
//! straight samples; `ops_spatial`'s combine returns them unchanged.
//!
//! A Lensfun profile (v1-lensfun, the `profile` parameter, a
//! `lens_model::LensProfile`) is composed after the manual stages: per
//! channel the source is `T_c(D(manual_c(p)))`, the manual map on the output
//! side, then the profile's distortion and the channel's TCA (Lensfun's
//! correction order, inverted). A profile with TCA makes three passes. Its
//! vignetting is evaluated at the full green source point and its stops are
//! added to the slider's, applied as one gain. The gain's basis: a float
//! layer multiplies by `2^stops`; a 16-bit plane whose profile has
//! vignetting gets the image profile's own linear basis from
//! `selected_filter` (`plane_with_gain`: decode through the profile's
//! matrix/TRC model, scale, encode with `PrecisionToneEncoder`); otherwise,
//! and always on the 8-bit byte path, the sRGB-linear gain above, the byte
//! filter family's convention. A neutral profile (every model the identity)
//! is treated as absent, and without `profile` the output is bit-identical to
//! the filter before it.
//!
//! A 32-bit float layer runs the same map and taps in
//! `SampleRange::Extended`: Catmull-Rom has negative lobes, so each resampled
//! colour component is clamped to the layer's visible range in place of 0…1
//! (D3 in `float_document`'s module doc), the transparent floor is 2^-24,
//! and the vignette multiplies the stored linear values by `2^stops` (see
//! `vignette`).
use crate::adjust_tone::Exposure;
use crate::filter_kernels::SampleRange;
use crate::job_control::JobContext;
use crate::lens_model::{
    radial_map, radial_monotone, vignette_radius, vignette_stops, LensProfile,
};
use crate::precision_image::{pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use crate::sampling::{map_into, Kernel};

/// `k1 = −distortion · this`: Distortion 100 is `k1 = −0.08`, which moves the
/// farthest corner of a square plate (r² = 2) inward by 16% of `R` — a strong
/// barrel correction, past what ordinary zoom lenses need — while its fold
/// radius (r² = 1/(3·0.08) ≈ 4.2) stays outside the unscaled frame.
pub(crate) const DISTORTION_K1_PER_UNIT: f64 = 0.0008;
/// `red_scale = 1 + red_cyan · this`, blue likewise: ±100 is a ±0.2% lateral
/// scale, ±2 pixels at the edge of a 2000-pixel half-width, which covers the
/// colour fringes real lenses show; RAW's profile bound is ±10%.
pub(crate) const CHROMATIC_SCALE_PER_UNIT: f64 = 0.00002;
/// `h = horizontal · this`, `v = vertical · this`: ±100 makes `d` 1 ± 0.4 at the
/// frame's edge, so the near edge is shown 0.4/1.4 larger than the far one,
/// about the keystone of a building shot from its foot with a normal lens.
pub(crate) const PERSPECTIVE_PER_UNIT: f64 = 0.004;
/// `stops = amount · this` at the farthest corner: ±100 is ±2 stops, enough to
/// lift a fast lens's wide-open falloff or to add a heavy creative vignette.
pub(crate) const VIGNETTE_STOPS_PER_UNIT: f64 = 0.02;
/// Where a point outside the picture is sent: so far out that every tap of the
/// kernel lands out of bounds and drops (`Kernel::weights_f64` saturates).
const OUTSIDE: [f64; 2] = [-1e9, -1e9];
/// The plan's per-pixel work: 16 Catmull-Rom taps per sampling pass, plus a
/// few units for the map, the vignette and the clamp.
const WORK_PER_TAP_PASS: u64 = 16;
const WORK_PER_PIXEL: u64 = 8;
/// [`LensCorrection::plan`]'s per-pixel term: the n×16 plane every pass writes
/// into is the only buffer that grows with the picture (the source is the
/// caller's own; the passes accumulate per pixel).
///
/// Record (all three sampling passes, a perspective and a vignette:
/// distortion 40, chromatic_red_cyan 60, chromatic_blue_yellow −40,
/// vignette_amount −30, vertical_perspective −30, horizontal_perspective 20,
/// angle 3, scale 105; `MallocLargeCache=0`, E = `ri_interval_max_phys_footprint`
/// − pre around the one call, a fresh process per case, 3 runs + a baseline
/// twin at E 0, 6000×4000). This plan is 16n + 2 MiB = 386,097,152 and binds
/// nowhere: every stage above it is a combine or a commit.
/// - Core, uncontrolled `dr_image_filter_op` on opaque splitmix noise (the
///   method of `tests/byte_filter_op_plan.rs`' `MEASURED_24MP`): E
///   480,264,528 ×3, the plane and the n×4 output, against
///   `filter_op_scratch_bytes` 605,242,880 (its 20n combine stage; 1.260×).
/// - The Release app at `DR_WORKING_LIMIT_BYTES` 8 GiB, `apply_filter` on a
///   one-layer document of two-colour Clouds:
///   - 16-bit: E 363,151,600 / 585,384,344 / 363,086,064, and in a second set
///     585,367,960 / 363,151,600 / 359,170,288, against `selected_edit_plan`
///     822,291,456 (1.405×), which its 24n combine stage sets.
///   - 8-bit: E 842,646,080 / 867,451,456 / 867,516,992, and 867,353,152 /
///     867,385,920 / 867,451,480: the peak is the commit's, which the host
///     charges. `ByteFilterAdmission.commitStageBytes` is 8n + 24n + ⌈7 × 24n /
///     16⌉ = 1,020,000,000 (each instance's `resource_usage` reported the core
///     projection estimate 480,000,000, 20n), plus what the edit leaves
///     retained (the smallest of a set: 192,003,778, then 96,003,738): 1.397×
///     and 1.287×, `ProjectionMargin`'s rule (1.176× before the retained term,
///     as in Motion Blur's record).
const PLANE_BYTES_PER_PIXEL: u64 = 16;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LensCorrection {
    pub distortion: f64,
    pub chromatic_red_cyan: f64,
    pub chromatic_blue_yellow: f64,
    pub vignette_amount: f64,
    pub vignette_midpoint: f64,
    pub vertical_perspective: f64,
    pub horizontal_perspective: f64,
    pub angle: f64,
    pub scale: f64,
    /// A resolved Lensfun profile (`lens_model::LensProfile`), composed after
    /// the manual stages; `None` (and a neutral profile) is today's filter.
    pub profile: Option<LensProfile>,
}

/// A vignette gain in a caller's basis: `(encoded rgb, stops) → encoded rgb`.
/// `None` is the filter's own basis (module doc).
pub(crate) type Gain<'a> = Option<&'a dyn Fn([f32; 3], f32) -> [f32; 3]>;

impl LensCorrection {
    /// The profile, unless it is absent or neutral (a neutral profile is the
    /// exact bypass, as if it were absent).
    fn active_profile(&self) -> Option<&LensProfile> {
        self.profile.as_ref().filter(|p| !p.neutral())
    }

    /// Whether the profile moves points (distortion or TCA).
    fn profile_geometry(&self) -> bool {
        self.active_profile()
            .is_some_and(|p| p.distortion.is_some() || p.tca.is_some())
    }

    /// A non-neutral profile with vignetting: the vignette stage runs, and a
    /// 16-bit caller passes its own gain basis (`selected_filter`).
    pub(crate) fn profile_vignetting(&self) -> bool {
        self.active_profile()
            .is_some_and(|p| p.vignetting.is_some())
    }

    /// Whether the vignette stage runs (the manual slider or the profile's).
    fn vignettes(&self) -> bool {
        self.vignette_amount != 0.0 || self.profile_vignetting()
    }

    /// Whether the map leaves every point where it is (the plane is the source).
    fn geometry_identity(&self) -> bool {
        self.manual_geometry_identity() && !self.profile_geometry()
    }

    /// Whether the manual sliders leave every point where it is.
    fn manual_geometry_identity(&self) -> bool {
        self.distortion == 0.0
            && self.chromatic_red_cyan == 0.0
            && self.chromatic_blue_yellow == 0.0
            && self.vertical_perspective == 0.0
            && self.horizontal_perspective == 0.0
            && self.angle == 0.0
            && self.scale == 100.0
    }

    /// Every default: nothing moves and no vignette, the exact bypass.
    pub(crate) fn identity(&self) -> bool {
        self.geometry_identity() && !self.vignettes()
    }

    /// Sampling passes: none for the identity map, one when the colour channels
    /// share it, three when the chromatic sliders give each its own.
    fn passes(&self) -> u64 {
        if self.geometry_identity() {
            0
        } else if self.chromatic_red_cyan == 0.0
            && self.chromatic_blue_yellow == 0.0
            && self.active_profile().is_none_or(|p| p.tca.is_none())
        {
            1
        } else {
            3
        }
    }

    /// The source position, in plane pixels, of output point `(x, y)` for
    /// colour `channel` (0 red, 1 green, 2 blue), or `None` when the point is
    /// outside the corrected picture (behind the perspective's horizon or past
    /// the radial map's or the profile's fold): `T_c(D(manual_c(p)))`, the
    /// manual stages on the output side, then the profile's distortion and
    /// the channel's TCA (`LensProfile::source`).
    pub(crate) fn source_point(
        &self,
        dimensions: (u32, u32),
        x: f64,
        y: f64,
        channel: usize,
    ) -> Option<[f64; 2]> {
        if !self.profile_geometry() {
            return self.manual_point(dimensions, x, y, channel);
        }
        let point = if self.manual_geometry_identity() {
            [x, y]
        } else {
            self.manual_point(dimensions, x, y, channel)?
        };
        self.active_profile()?.source(dimensions, point, channel)
    }

    /// The manual sliders' source position (`source_point` without the
    /// profile).
    fn manual_point(
        &self,
        dimensions: (u32, u32),
        x: f64,
        y: f64,
        channel: usize,
    ) -> Option<[f64; 2]> {
        let (w, h) = (f64::from(dimensions.0), f64::from(dimensions.1));
        let c = [w / 2.0, h / 2.0];
        let radius = w.max(h) / 2.0;
        let s = self.scale / 100.0;
        let n = [(x - c[0]) / radius / s, (y - c[1]) / radius / s];
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let n = [n[0] * cos - n[1] * sin, n[0] * sin + n[1] * cos];
        let d = 1.0
            + self.horizontal_perspective * PERSPECTIVE_PER_UNIT * n[0]
            + self.vertical_perspective * PERSPECTIVE_PER_UNIT * n[1];
        if d.is_nan() || d <= 0.0 {
            return None;
        }
        let n = [n[0] / d, n[1] / d];
        let radial = [-self.distortion * DISTORTION_K1_PER_UNIT, 0.0, 0.0];
        if !radial_monotone(n[0] * n[0] + n[1] * n[1], radial) {
            return None;
        }
        let lateral = match channel {
            0 => 1.0 + self.chromatic_red_cyan * CHROMATIC_SCALE_PER_UNIT,
            2 => 1.0 + self.chromatic_blue_yellow * CHROMATIC_SCALE_PER_UNIT,
            _ => 1.0,
        };
        let point = [c[0] + radius * n[0], c[1] + radius * n[1]];
        Some(radial_map(c, radius, point, radial, lateral))
    }

    /// `(scratch, work)`, refused before any allocation when the work is above
    /// `cap`. The scratch is the one n×16 plane every pass writes into (the
    /// passes accumulate per pixel, holding nothing else that grows with the
    /// picture) plus two bookkeeping allowances.
    pub(crate) fn plan(&self, dimensions: (u32, u32), cap: u64) -> Result<(u64, u64), String> {
        let n = pixel_count(dimensions.0, dimensions.1)? as u64;
        let work = n.saturating_mul(WORK_PER_TAP_PASS * self.passes() + WORK_PER_PIXEL);
        if work > cap {
            return Err("lens correction exceeds the bounded pixel-work limit".into());
        }
        Ok((n * PLANE_BYTES_PER_PIXEL + 2 * BOOKKEEPING_BYTES, work))
    }

    /// The corrected plane of straight samples: alpha in 0…1, and RGB in
    /// 0…1 at `SampleRange::Unit` or within the layer's visible range before
    /// the vignette at `Extended` (`SampleRange::resampled`).
    ///
    /// Below `range.alpha_floor()` the accumulated alpha is transparent black:
    /// half a 16-bit code quantizes to transparent at either integer depth,
    /// and dividing a colour by it would invent one nobody can see
    /// (`ops_spatial::box_plane`'s floor); a float layer's floor is 2^-24.
    pub(crate) fn plane(
        &self,
        read: impl Fn(usize) -> [f32; 4],
        dimensions: (u32, u32),
        cap: u64,
        range: &SampleRange,
        job: JobContext<'_>,
    ) -> Result<Vec<[f32; 4]>, String> {
        self.plane_with_gain(read, dimensions, cap, range, job, None)
    }

    /// [`Self::plane`] with the vignette's gain applied by `gain` (a
    /// 16-bit caller's linear basis, `selected_filter`) instead of the
    /// filter's own basis; `None` is [`Self::plane`].
    pub(crate) fn plane_with_gain(
        &self,
        read: impl Fn(usize) -> [f32; 4],
        dimensions: (u32, u32),
        cap: u64,
        range: &SampleRange,
        job: JobContext<'_>,
        gain: Gain<'_>,
    ) -> Result<Vec<[f32; 4]>, String> {
        self.plan(dimensions, cap)?;
        let floor = range.alpha_floor();
        let n = pixel_count(dimensions.0, dimensions.1)?;
        let passes = self.passes();
        let stages = (passes + u64::from(self.vignettes())).max(1) as f64;
        let mut out: Vec<[f32; 4]> = reserved_vec(n)?;
        if passes == 0 {
            for i in 0..n {
                job.every(i)?;
                out.push(read(i));
            }
        } else {
            out.resize(n, [0.0; 4]);
        }
        let map = |channel: usize| {
            move |x: f64, y: f64, _: JobContext<'_>| -> Result<[f64; 2], String> {
                Ok(self
                    .source_point(dimensions, x, y, channel)
                    .unwrap_or(OUTSIDE))
            }
        };
        let premultiplied = |c: usize| {
            move |acc: &mut [f64; 2], p: [f32; 4], w: f64| {
                let alpha = f64::from(p[3]);
                acc[0] += f64::from(p[c]) * alpha * w;
                acc[1] += alpha * w;
            }
        };
        let unit = |v: f64| v.clamp(0.0, 1.0) as f32;
        if passes == 1 {
            map_into::<_, _, 4>(
                dimensions,
                dimensions,
                Kernel::Bicubic,
                map(1),
                &read,
                |acc, p, w| {
                    let alpha = f64::from(p[3]);
                    for c in 0..3 {
                        acc[c] += f64::from(p[c]) * alpha * w;
                    }
                    acc[3] += alpha * w;
                },
                |acc| {
                    if acc[3] < floor {
                        [0.0; 4]
                    } else {
                        [
                            range.resampled(0, acc[0] / acc[3]) as f32,
                            range.resampled(1, acc[1] / acc[3]) as f32,
                            range.resampled(2, acc[2] / acc[3]) as f32,
                            unit(acc[3]),
                        ]
                    }
                },
                |i, p| out[i] = p,
                job.range(0.0, 1.0 / stages),
            )?;
        } else if passes == 3 {
            // Green first: it carries the alpha, which the other two passes
            // leave alone.
            for (k, channel) in [1_usize, 0, 2].into_iter().enumerate() {
                map_into::<_, _, 2>(
                    dimensions,
                    dimensions,
                    Kernel::Bicubic,
                    map(channel),
                    &read,
                    premultiplied(channel),
                    |acc| acc,
                    |i, acc| {
                        if channel == 1 {
                            out[i][3] = unit(acc[1]);
                        }
                        out[i][channel] = if acc[1] < floor {
                            0.0
                        } else {
                            range.resampled(channel, acc[0] / acc[1]) as f32
                        };
                    },
                    job.range(k as f64 / stages, (k + 1) as f64 / stages),
                )?;
            }
            for (i, p) in out.iter_mut().enumerate() {
                job.every(i)?;
                if f64::from(p[3]) < floor {
                    *p = [0.0; 4];
                }
            }
        }
        if self.vignettes() {
            self.vignette(
                &mut out,
                dimensions,
                range,
                job.range(passes as f64 / stages, 1.0),
                gain,
            )?;
        }
        Ok(out)
    }

    /// The vignette over the corrected plane, at each pixel's green source
    /// point (so it stays with the lens as Scale and Perspective move the
    /// picture): `stops = amount·VIGNETTE_STOPS_PER_UNIT · fraction^falloff`,
    /// `falloff = 4^((midpoint − 50)/50)` — 1 at the default midpoint 50, 4 at
    /// 100 (a clear middle, the darkening kept to the corners) and ¼ at 0.
    /// Reports `(y + 1) / h` at stage 6 after each row, within `job`'s range
    /// (the `sampling_motion` pattern), so a large plate's last stage shows
    /// progress as it runs.
    ///
    /// At `SampleRange::Unit` the samples are display-encoded, so the gain is
    /// applied in sRGB linear light and encoded back (`Exposure::gain_only`),
    /// then clamped to 0…1. A float layer's samples ARE linear light, so at
    /// `Extended` each RGB component is multiplied by `2^stops` directly: no
    /// transfer round trip (it would be the identity at best, and it clips
    /// below 0), and no clamp, because a lifted corner above white is a value
    /// a float layer keeps. It runs after the resample's visible-range clamp,
    /// which bounds the kernel's ringing, not the vignette.
    ///
    /// A profile's vignetting adds its stops (`LensProfile::vignette_stops` at
    /// the full green source point) to the slider's, and the sum is applied
    /// as one gain. `gain`, when given, applies it in the caller's basis and
    /// the result is clamped to 0…1.
    fn vignette(
        &self,
        out: &mut [[f32; 4]],
        dimensions: (u32, u32),
        range: &SampleRange,
        job: JobContext<'_>,
        gain: Gain<'_>,
    ) -> Result<(), String> {
        let extended = matches!(range, SampleRange::Extended { .. });
        let (w, h) = dimensions;
        let c = [f64::from(w) / 2.0, f64::from(h) / 2.0];
        let radius = vignette_radius(c, w, h);
        let manual = self.vignette_amount != 0.0;
        let profile = self.active_profile().filter(|p| p.vignetting.is_some());
        let stops = self.vignette_amount * VIGNETTE_STOPS_PER_UNIT;
        let falloff = 4.0_f64.powf((self.vignette_midpoint - 50.0) / 50.0);
        let width = w as usize;
        if width == 0 {
            return Ok(());
        }
        for (row, line) in out.chunks_mut(width).enumerate() {
            for (column, p) in line.iter_mut().enumerate() {
                job.every(row * width + column)?;
                if p[3] <= 0.0 {
                    continue;
                }
                let (x, y) = (column as f64 + 0.5, row as f64 + 0.5);
                let mut total = 0.0;
                if manual && radius != 0.0 {
                    let Some(s) = self.manual_point(dimensions, x, y, 1) else {
                        continue;
                    };
                    let fraction = (s[0] - c[0]).hypot(s[1] - c[1]) / radius;
                    total = vignette_stops(stops, fraction, falloff);
                }
                if let Some(profile) = profile {
                    let Some(s) = self.source_point(dimensions, x, y, 1) else {
                        continue;
                    };
                    total += profile.vignette_stops(dimensions, s);
                }
                apply_vignette_stops(p, total as f32, extended, gain);
            }
            job.report((row + 1) as f64 / f64::from(h), 6)?;
        }
        Ok(())
    }
}

/// One straight sample's vignette gain of `stops` (0 leaves it alone): with
/// `gain`, the caller's basis, clamped to 0…1; at `extended` (a float
/// layer's linear light) RGB × 2^stops, unclamped; otherwise the sRGB
/// light-linear `Exposure::gain_only`, clamped to 0…1. Alpha is untouched.
/// The ONE per-pixel gain of [`LensCorrection`]'s vignette, which Auto-Align's
/// Vignette Removal (`doc_align_vignette`, `precision_align_vignette`) also
/// applies.
pub(crate) fn apply_vignette_stops(p: &mut [f32; 4], stops: f32, extended: bool, gain: Gain<'_>) {
    if stops == 0.0 {
        return;
    }
    if let Some(gain) = gain {
        let rgb = gain([p[0], p[1], p[2]], stops);
        for (channel, value) in p.iter_mut().zip(rgb) {
            *channel = value.clamp(0.0, 1.0);
        }
        return;
    }
    if extended {
        let gain = f64::from(stops).exp2();
        for channel in p.iter_mut().take(3) {
            *channel = (f64::from(*channel) * gain) as f32;
        }
        return;
    }
    let rgb = Exposure::gain_only(stops).apply([p[0], p[1], p[2]]);
    for (channel, value) in p.iter_mut().zip(rgb) {
        *channel = value.clamp(0.0, 1.0);
    }
}
