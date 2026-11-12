//! Box Blur, Surface Blur, Add Grain, Radial Blur, Clouds / Difference Clouds
//! and Offset, Motion Blur, Lens Blur and Tilt-Shift, and Lens
//! Correction: ten destructive spatial filters with ONE arithmetic at
//! both depths. `SpatialKernel` is planned, run as a plane and combined per
//! pixel exactly as `selected_filter` runs it for a 16-bit layer, mask or
//! colour/channel plane, and `filter_op` — the byte twin behind
//! `dr_image_filter_op` — runs the same three calls over straight byte
//! samples. There is deliberately no second byte path. `filter_op` is also the
//! one byte dispatch for Unsharp Mask, High Pass, Dust & Scratches and Reduce
//! Noise, which it hands to `ops_photographic::apply`, for Gaussian Blur
//! (`blur`), which it hands to `sampling::fitted_blur`, and for Liquify
//! (`liquify`), which it hands to `liquify::plane`; it dispatches
//! no other op name. `filter_op_scratch_bytes` is its byte plan, which
//! the host admits before `dr_image_filter_op` allocates.
//! `filter_op` takes `sampling_gaussian::destructive_work_cap(job)`: under a
//! host job control (cancellable) it has no work ceiling and is admitted by
//! memory alone; without one it keeps `MAX_DESTRUCTIVE_WORK`. The plan is made
//! at `CONTROLLED_JOB_WORK`, so it carries no work refusal.
//!
//! Box Blur is a premultiplied mean over a `(2r+1)²` clamped-edge window,
//! through `sampling_box::box_sum`'s sliding passes, one channel's sum at a
//! time, alpha first (one binary64 sum held, never four); like
//! Gaussian Blur it filters alpha. Surface Blur (`sampling_surface`) is colour only. Add Grain
//! is colour only, positional (a value depends on where it is asked for, not on
//! a draw order) and luminance-weighted, so pure black and pure white are left
//! exactly alone. Radial Blur (`sampling_radial`) filters alpha through Box
//! Blur's combine; Clouds (`render_clouds`) replaces the pixel and Difference
//! Clouds is colour only; Offset (`sampling_offset`) moves samples exactly.
//! Motion Blur (`sampling_motion`) is the mean along a line, Lens Blur and
//! Tilt-Shift (`sampling_disc`) a disc mean — highlight-weighted for Lens Blur,
//! with a radius that grows away from a focus line for Tilt-Shift. All three
//! filter alpha through Box Blur's combine and take no frame. Lens Correction
//! (`sampling_lens`, the radial model shared with RAW development through
//! `lens_model`) resamples the whole plane through an inverse lens map, with a
//! transparent edge, and its plane is the corrected straight sample; it takes no
//! frame either.
//!
//! Radial Blur and Offset act within a [`Frame`]: the bounding box of the
//! selection in plane pixels, or the whole plane. At 16 bits the core
//! derives it from the coverage at run time ([`coverage_frame`],
//! `SelectedFilter::with_frame`); at 8 bits the host passes it as
//! `region_x`/`region_y`/`region_width`/`region_height`.
//!
//! ONE `SpatialKernel` (`plan` refuses before allocating, `plane`, `combine`) serves
//! `SelectedFilter::Spatial` (16-bit layers, masks, colour/channel planes) and the byte `filter_op`
//! behind `dr_image_filter_op`. `filter_op` is the ONE byte dispatch for seven ops —
//! those three plus Unsharp Mask, High Pass, Dust & Scratches and Reduce Noise
//! (`SelectedFilter::Photo` → `ops_photographic::apply`), parsed with the 16-bit filter's keys,
//! ranges and defaults — and takes an optional job: `dr_image_filter_op`'s single-use `control`
//! (the cancellable slider previews and the cancellable Apply job; controlled output is
//! byte-identical). `filter_op_scratch_bytes` / `dr_image_filter_op_scratch_bytes` is its ONE byte
//! plan, refused by the same private `dispatch` in the same words: max(kernel plan, 20n) + 4
//! BK, plus a quarter. Gaussian Blur is the fifteenth op, `blur` (key `sigma`, up to
//! 1000): `filter_op` returns `sampling::fitted_blur`'s banded output under the job (the
//! kernel measured against Photoshop, `gaussian_blur_filter`), with no combine stage and no
//! work ceiling, and its plan is `sampling::fitted_blur_scratch_bytes` + 4 BK, plus a quarter.
//! `dr_image_blur` is gone, so no filter has a second byte path.
//! Liquify is the sixteenth op, `liquify` (key `strokes`): not a `SpatialKernel` arm (the
//! kernel stays `Copy`) but `ByteFilter::Liquify`, the stroke list `SelectedFilter::Liquify` parsed.
//! `filter_op` quantizes `liquify::plane`'s samples as it does a spatial arm's, with no combine and
//! no work ceiling (`liquify::MAX_WORK` bounds the strokes instead, and `filter_op_scratch_bytes`
//! makes that refusal too); an identity field is the exact bypass.
//! Lens Correction is the seventeenth, `lens_correction`, a `SpatialKernel` arm; its
//! all-default parameters are the exact bypass.
//! Presence is the eighteenth op, `presence`, a `SpatialKernel` arm too (`sampling_presence`
//! and `sampling_dehaze`: Texture, Clarity and Dehaze, colour only, no frame); its plane is the
//! final straight sample and all three amounts at 0 are the exact bypass.
//! Develop Filter is the nineteenth op, `develop_filter`, a `SpatialKernel` arm composing
//! existing ops and Presence in one fixed order (`sampling_develop_filter`; colour only, no frame); its
//! plane is the final straight sample and all thirteen controls at 0 are the exact bypass.
//! Depth Blur is the twenty-first op, `depth_blur`: not a `SpatialKernel` arm (its depth map
//! is not `Copy`) but `ByteFilter::DepthBlur`, the request `SelectedFilter::DepthBlur` parsed
//! (`depth_blur::DepthBlur`: Lens Blur's disc at a radius that follows the host's depth map, plus
//! Haze, the Develop Filter's grade and Add Grain). `filter_op` quantizes its plane, the final
//! sample, as it does Liquify's; it has no work ceiling (the disc's radius bounds it) and
//! `filter_op_scratch_bytes` charges `DepthBlur::plan`.
//! A `develop_filter` request with masks is `ByteFilter::DevelopFilterMasked`, the request
//! `SelectedFilter::DevelopFilterMasked` parsed (`develop_filter_masks`: ordered local adjustments over
//! the global controls); `filter_op` quantizes its plane, the final sample, as it does Depth
//! Blur's, and `filter_op_scratch_bytes` charges `DevelopFilterMasked::plan` as a spatial arm.
//! Smart Sharpen is the twentieth op, `smart_sharpen`, a `SpatialKernel` arm too
//! (`sampling_smart_sharpen`: Reduce Noise, the low-pass of the blur to remove, Unsharp Mask's
//! combine with the tonal fades and Lens Blur's projection; colour only, no frame); its plane is the
//! final straight sample and it is never an identity. [`spatial_bytes`] is `filter_op`'s spatial
//! tail, which the export workspace's Smart Sharpen output sharpening (`export_sharpen`) runs too.
//! Box Blur is the premultiplied mean through `sampling_box::box_sum` (the whole-plane wrapper of
//! the streamed `BoxRows`; it holds one channel's sum at a time,
//! alpha first, and its plan is its own 24n stage + the ring + 2 BK — `BOX_BLUR_BYTES_PER_PIXEL`
//! (the commit and first projection after it is charged by the host,
//! `ByteFilterAdmission.commitStageBytes`) — where it was the plane plus four sums, 48n + 2 BK,
//! once the ring's n×8 was dropped) and filters alpha. Add Grain is a positional Box–Muller field
//! blurred by `sampling_gaussian::gaussian_taps`, unit-variance at every roughness, scaled by
//! `GRAIN_RMS_PER_AMOUNT` (0.001) × 4Y(1−Y). Both colour-only filters leave alpha exact and alpha-0
//! pixels untouched. Tests and the `#[ignore]`d calibration probe: `tests/spatial_filters.rs`; the
//! measurements are in `designs/phase-9f-selected-tone-status.md`.
use crate::blend::{LUMA_B, LUMA_G, LUMA_R};
use crate::depth_blur::DepthBlur;
use crate::develop_filter_masks::DevelopFilterMasked;
use crate::filter_kernels::{neighborhood_plan, straight};
use crate::filter_kernels::{FilterKind, Neighborhood, SampleRange};
use crate::job_control::JobContext;
use crate::liquify::LiquifyStroke;
use crate::precision_image::{pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use crate::render_clouds::{clouds_combine, clouds_plan, clouds_plane};
use crate::rng::SplitMix64;
use crate::sampling::Unorm;
use crate::sampling_box::{box_sum, BoxRows};
use crate::sampling_develop_filter::DevelopFilter;
use crate::sampling_disc::{disc_plan, disc_plane};
use crate::sampling_gaussian::{destructive_work_cap, gaussian_taps, CONTROLLED_JOB_WORK};
use crate::sampling_gaussian_fast::MAX_SIGMA;
use crate::sampling_lens::LensCorrection;
use crate::sampling_motion::{motion_plan, motion_plane};
use crate::sampling_offset::{offset_plan, offset_plane, OffsetEdge};
use crate::sampling_presence::Presence;
use crate::sampling_radial::{radial_plan, radial_plane, RadialMethod, RadialQuality};
use crate::sampling_smart_sharpen::SmartSharpen;
use crate::selected_filter::SelectedFilter;
use image::RgbaImage;
use serde_json::{Map, Value};

/// Normalized RMS of the grain at mid-grey (Y = 0.5) per Amount unit. Frozen
/// at 0.001, so Amount 100 is an RMS of 0.1 — 25.5 8-bit levels — which is the
/// heavy end of film grain, and the default 25 (≈ 6.4 levels) a visible but
/// ordinary one. The plane is unit-variance by construction, so this constant
/// IS the RMS and needs no measured calibration.
pub(crate) const GRAIN_RMS_PER_AMOUNT: f64 = 0.001;
/// The `params_json` ceiling of `dr_image_filter_op`: every op but `liquify`
/// and `depth_blur` takes at most thirteen short values, so this only bounds a hostile caller.
pub(crate) const FILTER_OP_PARAMS_MAX_BYTES: usize = 65_536;
/// Box Blur's per-pixel plan term. The kernel holds the n×16
/// plane and one binary64 sum (n×8), 24 B/px, plus the `BoxRows` ring; that
/// is its own stage and all the plan charges. The 8-bit edit's commit and
/// first projection, which an `apply_filter` invert peaks at too, is charged
/// by the host for every byte filter (`ByteFilterAdmission.commitStageBytes`,
/// a sequential stage taken as max() against this plan); batch 164 had folded
/// it into this term as 33 B/px.
///
/// Record (one case per process, `MallocLargeCache=0`, E =
/// `ri_interval_max_phys_footprint` − pre, 3 runs + a baseline twin at E 0):
/// - G1, uncontrolled `dr_image_filter_op` box_blur r5 on opaque splitmix
///   noise: 6000×4000 E 576,323,992 ×3; 8256×5504 E 1,091,158,808 ×3 (24.0
///   B/px; HEAD's four held sums peaked at 1,152,680,776 at 24 MP). The
///   `filter_op_scratch_bytes` plan at 24 is 728,644,320 (1.264×) and
///   1,372,168,320 (1.258×).
/// - G2, the Release app at 8 GiB, `open_document` photo45.png (8256×5504)
///   then `apply_filter` box_blur r5: E 1,633,059,992 / 1,633,141,912 /
///   1,632,830,640; an `apply_filter` invert on the same document peaks at
///   1,633,698,968. That peak is the commit's, not the kernel's.
/// - G3, 16-bit (the same kernel through `SelectedFilter::scratch`): P16.png
///   (6000×4000) `apply_filter` box_blur r10, E 764,117,544 / 764,215,824 /
///   763,544,104 against `selected_edit_plan` 825,492,608 (1.080×, by
///   arithmetic; the 9f standard is ≥ 1.0×).
const BOX_BLUR_BYTES_PER_PIXEL: u64 = 24;

/// The rectangle, in the plane's own pixels, that Offset shifts and wraps
/// within and that Radial Blur's centre is relative to. Never recorded: a
/// replayed step derives it from the selection and placement at run time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Frame {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// The bounding box of the plane pixels whose `weight` (the selection's
/// coverage at that pixel) is nonzero, or `None` when no pixel is covered —
/// the whole plane then, as with no selection.
pub(crate) fn coverage_frame(
    dimensions: (u32, u32),
    weight: impl Fn(usize) -> u16,
) -> Option<Frame> {
    let (w, h) = dimensions;
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0_u32, 0_u32);
    let mut i = 0_usize;
    for y in 0..h {
        for x in 0..w {
            if weight(i) > 0 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
            i += 1;
        }
    }
    (x0 <= x1 && y0 <= y1).then(|| Frame {
        x: x0,
        y: y0,
        width: x1 - x0 + 1,
        height: y1 - y0 + 1,
    })
}

/// `frame`, refused when it is empty or not inside the plane, or the whole
/// plane for `None`.
pub(crate) fn frame_or_plane(
    frame: Option<Frame>,
    dimensions: (u32, u32),
) -> Result<Frame, String> {
    let Some(f) = frame else {
        return Ok(Frame {
            x: 0,
            y: 0,
            width: dimensions.0,
            height: dimensions.1,
        });
    };
    if f.width == 0
        || f.height == 0
        || u64::from(f.x) + u64::from(f.width) > u64::from(dimensions.0)
        || u64::from(f.y) + u64::from(f.height) > u64::from(dimensions.1)
    {
        return Err("region must be a non-empty rectangle inside the image".into());
    }
    Ok(f)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SpatialKernel {
    BoxBlur {
        radius: u32,
    },
    SurfaceBlur {
        radius: u32,
        threshold: f64,
    },
    AddGrain {
        amount: f64,
        size: f64,
        roughness: f64,
        seed: u64,
    },
    RadialBlur {
        amount: u32,
        method: RadialMethod,
        quality: RadialQuality,
        center: (f64, f64),
        frame: Option<Frame>,
    },
    Clouds {
        seed: u64,
        foreground: [u8; 3],
        background: [u8; 3],
        high_contrast: bool,
        difference: bool,
    },
    Offset {
        horizontal: i64,
        vertical: i64,
        edge: OffsetEdge,
        frame: Option<Frame>,
    },
    /// The mean along a line of `distance` pixels at `angle` degrees,
    /// counterclockwise from the x axis.
    MotionBlur {
        angle: f64,
        distance: u32,
    },
    /// A disc mean of `radius` pixels; `brightness` (0..=100) weighs pixels
    /// whose luma is above `threshold` (8-bit levels) more heavily.
    LensBlur {
        radius: u32,
        brightness: f64,
        threshold: f64,
    },
    /// A disc mean whose radius is 0 within `focus` (a fraction of the plane's
    /// height) of the line through `center` (plane fractions) at `angle`
    /// degrees, and grows to `radius` over `transition` (the same unit).
    TiltShift {
        radius: u32,
        center: (f64, f64),
        angle: f64,
        focus: f64,
        transition: f64,
    },
    /// Filters ▸ Lens Correction: distortion, chromatic aberration,
    /// vignette and perspective / angle / scale (`sampling_lens`).
    LensCorrection(LensCorrection),
    /// Filters ▸ Presence: Texture, Clarity and Dehaze
    /// (`sampling_presence`, `sampling_dehaze`).
    Presence(Presence),
    /// Filters ▸ Develop Filter: the Develop panel's global
    /// controls in one fixed order (`sampling_develop_filter`).
    DevelopFilter(DevelopFilter),
    /// Filter ▸ Sharpen ▸ Smart Sharpen: `sampling_smart_sharpen`.
    SmartSharpen(SmartSharpen),
}

/// Tilt-Shift's blur radius at pixel `(x, y)`: `radius · clamp((d − focus·H) /
/// (transition·H), 0, 1)`, `d` the distance in pixels from the pixel's centre
/// to the focus line and `H` the plane's height; a step at `transition` 0. The
/// line's direction is `(cos θ, −sin θ)` (0 is horizontal, positive
/// counterclockwise on the y-down plane), so its normal is `(sin θ, cos θ)`.
fn tilt_shift_radius(
    dimensions: (u32, u32),
    kernel: (u32, (f64, f64), f64, f64, f64),
) -> impl Fn(usize, usize) -> f64 {
    let (radius, center, angle, focus, transition) = kernel;
    let (w, h) = (f64::from(dimensions.0), f64::from(dimensions.1));
    let (cx, cy) = (center.0 * w, center.1 * h);
    let (sin, cos) = angle.to_radians().sin_cos();
    let (clear, ramp) = (focus * h, transition * h);
    move |x, y| {
        let d = ((x as f64 + 0.5 - cx) * sin + (y as f64 + 0.5 - cy) * cos).abs() - clear;
        let t = if ramp > 0.0 {
            (d / ramp).clamp(0.0, 1.0)
        } else if d > 0.0 {
            1.0
        } else {
            0.0
        };
        f64::from(radius) * t
    }
}

/// The Gaussian tap count `gaussian_taps(size)` will return, without allocating.
fn grain_taps(size: f64) -> u64 {
    2 * (3.0 * size).ceil() as u64 + 1
}

impl SpatialKernel {
    /// Only a zero grain amount, a zero offset and an all-default lens
    /// correction change nothing; every
    /// admitted radius or amount moves a non-constant picture, and clouds
    /// always render.
    pub(crate) fn identity(&self) -> bool {
        match *self {
            Self::AddGrain { amount, .. } => amount == 0.0,
            Self::Offset {
                horizontal,
                vertical,
                ..
            } => horizontal == 0 && vertical == 0,
            Self::LensCorrection(lens) => lens.identity(),
            Self::Presence(presence) => presence.identity(),
            Self::DevelopFilter(develop_filter) => develop_filter.identity(),
            _ => false,
        }
    }

    /// `(scratch, work)` for one plane, refused before anything is allocated.
    pub(crate) fn plan(&self, dimensions: (u32, u32), cap: u64) -> Result<(u64, u64), String> {
        let n = pixel_count(dimensions.0, dimensions.1)? as u64;
        let edges = u64::from(dimensions.0) + u64::from(dimensions.1);
        match *self {
            Self::BoxBlur { radius } => {
                if !(1..=1000).contains(&radius) {
                    return Err("box blur radius must be an integer in [1,1000]".into());
                }
                // Four sliding box passes of a few units each, plus the
                // `2r+1` re-seeding reads per row and column that
                // `sampling_denoise::plan_bounded` charges for the same passes.
                let work = n * 24 + 4 * (2 * u64::from(radius) + 1) * edges;
                if work > cap {
                    return Err("box blur exceeds the bounded pixel-work limit".into());
                }
                // `box_plane` holds the n×16 plane, ONE binary64 sum (n×8)
                // and `sampling_box::BoxRows`' ring while it builds the next
                // sum, so the kernel's peak is 24n + ring, which is
                // what the plan charges (`BOX_BLUR_BYTES_PER_PIXEL`, its record).
                let ring = BoxRows::bytes(
                    u64::from(dimensions.0),
                    u64::from(dimensions.1),
                    u64::from(radius),
                );
                Ok((
                    n * BOX_BLUR_BYTES_PER_PIXEL + ring + 2 * BOOKKEEPING_BYTES,
                    work,
                ))
            }
            Self::SurfaceBlur { radius, threshold } => {
                crate::sampling_surface::surface_plan(dimensions, radius, threshold, cap)
            }
            Self::AddGrain { size, .. } => {
                if !size.is_finite() || !(0.5..=4.0).contains(&size) {
                    return Err("grain size must be in [0.5,4] pixels".into());
                }
                let taps = grain_taps(size);
                let work = n * (16 + 4 * taps);
                if work > cap {
                    return Err("add grain exceeds the bounded pixel-work limit".into());
                }
                // The plane, the noise field and its horizontal pass, the taps.
                Ok((n * 16 + n * 8 + taps * 8 + 2 * BOOKKEEPING_BYTES, work))
            }
            Self::RadialBlur {
                amount,
                method,
                quality,
                frame,
                ..
            } => radial_plan(dimensions, amount, method, quality, frame, cap),
            Self::Clouds { .. } => clouds_plan(dimensions, cap),
            Self::Offset { frame, .. } => offset_plan(dimensions, frame, cap),
            Self::MotionBlur { distance, .. } => motion_plan(dimensions, distance, cap),
            Self::LensBlur { radius, .. } | Self::TiltShift { radius, .. } => {
                disc_plan(dimensions, radius, cap)
            }
            Self::LensCorrection(lens) => lens.plan(dimensions, cap),
            Self::Presence(presence) => presence.plan(dimensions, cap),
            Self::DevelopFilter(develop_filter) => develop_filter.plan(dimensions, cap),
            Self::SmartSharpen(smart) => smart.plan(dimensions, cap),
        }
    }

    /// The filter's plane over straight samples. Box Blur, Radial Blur, Motion
    /// Blur, Lens Blur and Tilt-Shift answer the premultiplied mean, Surface Blur `[r, g, b, source alpha]`,
    /// Add Grain `[g, 0, 0, 0]` with `g` unit-variance grain, Clouds
    /// `[t, 0, 0, 0]` with `t` the cloud value, Offset the moved straight sample,
    /// Lens Correction the corrected straight sample. The 8- and 16-bit form:
    /// [`Self::plane_in`] at `SampleRange::Unit`.
    pub(crate) fn plane(
        &self,
        read: impl Fn(usize) -> [f32; 4],
        dimensions: (u32, u32),
        cap: u64,
        job: JobContext<'_>,
    ) -> Result<Vec<[f32; 4]>, String> {
        self.plane_in(read, dimensions, cap, &SampleRange::Unit, job)
    }

    /// [`Self::plane`] in `range`: the transparent floor of the five means
    /// and of Lens Correction is `range.alpha_floor()`, Motion and Radial
    /// Blur place their samples in binary64 in a float layer
    /// (`sampling_radial::sample_exact`), and Lens Correction finishes its
    /// resample and its vignette in `range`
    /// (`sampling_lens::LensCorrection::plane`). Surface Blur maps its
    /// histogram over the visible range (`sampling_surface::surface_plane_in`),
    /// and Presence and the Develop Filter run their guided bases unclamped
    /// (`Presence::plane_in`, `DevelopFilter::plane_in`), and Smart Sharpen its
    /// Reduce Noise, low-pass and combine (`SmartSharpen::plane_in`). Add
    /// Grain, Clouds and Offset do not read it.
    pub(crate) fn plane_in(
        &self,
        read: impl Fn(usize) -> [f32; 4],
        dimensions: (u32, u32),
        cap: u64,
        range: &SampleRange,
        job: JobContext<'_>,
    ) -> Result<Vec<[f32; 4]>, String> {
        self.plan(dimensions, cap)?;
        let alpha_floor = range.alpha_floor();
        match *self {
            Self::BoxBlur { radius } => box_plane(read, dimensions, radius, alpha_floor, job),
            Self::SurfaceBlur { radius, threshold } => crate::sampling_surface::surface_plane_in(
                read, dimensions, radius, threshold, cap, range, job,
            ),
            Self::AddGrain {
                size,
                roughness,
                seed,
                ..
            } => grain_plane(dimensions, size, roughness, seed, job),
            Self::RadialBlur {
                amount,
                method,
                quality,
                center,
                frame,
            } => radial_plane(
                read, dimensions, amount, method, quality, center, frame, range, job,
            ),
            Self::Clouds {
                seed,
                high_contrast,
                ..
            } => clouds_plane(dimensions, seed, high_contrast, job),
            Self::Offset {
                horizontal,
                vertical,
                edge,
                frame,
            } => offset_plane(read, dimensions, horizontal, vertical, edge, frame, job),
            Self::MotionBlur { angle, distance } => {
                motion_plane(read, dimensions, angle, distance, range, job)
            }
            Self::LensBlur {
                radius,
                brightness,
                threshold,
            } => disc_plane(
                read,
                dimensions,
                |_, _| f64::from(radius),
                radius,
                (brightness > 0.0).then_some((brightness, threshold)),
                alpha_floor,
                job,
            ),
            Self::TiltShift {
                radius,
                center,
                angle,
                focus,
                transition,
            } => disc_plane(
                read,
                dimensions,
                tilt_shift_radius(dimensions, (radius, center, angle, focus, transition)),
                radius,
                None,
                alpha_floor,
                job,
            ),
            Self::LensCorrection(lens) => lens.plane(read, dimensions, cap, range, job),
            Self::Presence(presence) => presence.plane_in(read, dimensions, cap, range, job),
            Self::DevelopFilter(develop_filter) => {
                develop_filter.plane_in(read, dimensions, cap, range, job)
            }
            Self::SmartSharpen(smart) => smart.plane_in(read, dimensions, cap, range, job),
        }
    }

    /// One output pixel from its straight source and its plane sample. The
    /// 8- and 16-bit form: [`Self::combine_in`] at `SampleRange::Unit`.
    pub(crate) fn combine(&self, source: [f32; 4], sample: [f32; 4]) -> [f32; 4] {
        self.combine_in(source, sample, &SampleRange::Unit)
    }

    /// [`Self::combine`] in `range`. The five means are non-negative weights
    /// over the layer's own colours, so their straight colour is finished
    /// with `range.resampled`: clamped to 0…1 at `Unit`, and in a float layer
    /// to the layer's own visible range, which is never 0…1 there (a mean
    /// beside a pixel far above white is meant to rise above 1). An exact
    /// mean cannot leave that range, but this one is an f32 quotient of a
    /// rounded premultiplied mean and a rounded alpha, which can land an ulp
    /// past the layer's brightest or darkest colour; at ±65504 that ulp is
    /// outside what a float layer stores and the write would refuse a legal
    /// picture. The clamp changes nothing else. A sample with no alpha stays
    /// transparent black: it has no colour to bring into the range. The
    /// mean's alpha is a sum of alphas at most 1 that rounding can leave a
    /// few ulps above it; a float layer stores no alpha above 1, so
    /// `Extended` takes the smaller (the `Unit` quantizer clamps it later).
    ///
    /// Surface Blur, Add Grain and Clouds run in a float layer on its
    /// DISPLAY-ENCODED copy (`display_encoding`), so their parameters are
    /// the 16-bit ones. Add Grain at 32 bits: the grain is weighted by
    /// `4Y(1 − Y)` on the encoded luma clamped to 0…1, so a pixel whose luma
    /// is at or above white gets none and keeps its light, and the sum is not clamped
    /// at `Extended` (`range.color`), so a value above white is never pulled
    /// down to it. Clouds mixes its hex colours in encoded units.
    pub(crate) fn combine_in(
        &self,
        source: [f32; 4],
        sample: [f32; 4],
        range: &SampleRange,
    ) -> [f32; 4] {
        match *self {
            Self::BoxBlur { .. }
            | Self::RadialBlur { .. }
            | Self::MotionBlur { .. }
            | Self::LensBlur { .. }
            | Self::TiltShift { .. } => {
                let mut out = straight(sample);
                if out[3] > 0.0 {
                    for (c, v) in out.iter_mut().take(3).enumerate() {
                        *v = range.resampled(c, f64::from(*v)) as f32;
                    }
                }
                if matches!(range, SampleRange::Extended { .. }) {
                    out[3] = out[3].min(1.0);
                }
                out
            }
            Self::SurfaceBlur { .. } => {
                if source[3] <= 0.0 {
                    return source;
                }
                [sample[0], sample[1], sample[2], source[3]]
            }
            Self::AddGrain { amount, .. } => {
                if source[3] <= 0.0 {
                    return source;
                }
                let y = f64::from(LUMA_R) * f64::from(source[0])
                    + f64::from(LUMA_G) * f64::from(source[1])
                    + f64::from(LUMA_B) * f64::from(source[2]);
                // 4Y(1−Y) is 1 at mid-grey, where the RMS constant is stated,
                // and 0 at black and white: grain lives in the midtones, as on
                // film, and never lifts a black or dents a white.
                let delta = amount
                    * GRAIN_RMS_PER_AMOUNT
                    * f64::from(sample[0])
                    * 4.0
                    * y.clamp(0.0, 1.0)
                    * (1.0 - y.clamp(0.0, 1.0));
                let mut out = source;
                for c in out.iter_mut().take(3) {
                    *c = range.color(f64::from(*c) + delta) as f32;
                }
                out
            }
            Self::Clouds {
                foreground,
                background,
                difference,
                ..
            } => clouds_combine(source, sample[0], foreground, background, difference),
            Self::Offset { .. } | Self::LensCorrection(_) => sample,
            Self::Presence(_) | Self::DevelopFilter(_) | Self::SmartSharpen(_) => sample,
        }
    }
}

fn box_plane(
    read: impl Fn(usize) -> [f32; 4],
    dimensions: (u32, u32),
    radius: u32,
    floor: f64,
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    let (w, h) = (dimensions.0 as usize, dimensions.1 as usize);
    let r = radius as usize;
    let premultiplied = |i: usize, c: usize| {
        let p = read(i);
        if c == 3 {
            f64::from(p[3])
        } else {
            f64::from(p[c]) * f64::from(p[3])
        }
    };
    let count = ((2 * r + 1) * (2 * r + 1)) as f64;
    // Below half a 16-bit code the mean alpha quantizes to transparent at
    // either depth, and that is also where the sliding sums' rounding drift
    // can be the size of the signal: dividing drift by drift would invent a
    // colour for a pixel nobody can see, so it is stored as transparent black.
    // `floor` is that half code (`SampleRange::alpha_floor`); a float layer
    // passes its own 2^-24, which is still far above the drift.
    // One binary64 sum lives at a time: alpha first, straight into the
    // output, then each colour channel written where the stored alpha is not
    // 0.0. That test is the floor test, because a mean at or above the floor
    // rounds to a positive f32, so the output is bit-identical to holding all
    // four sums at once.
    let alpha = box_sum(|i| premultiplied(i, 3), w, h, r, job)?;
    job.report(1.0 / 5.0, 6)?;
    let mut out = reserved_vec(w * h)?;
    for (i, a) in alpha.iter().enumerate() {
        job.every(i)?;
        let mean = a / count;
        out.push(if mean < floor {
            [0.0; 4]
        } else {
            [0.0, 0.0, 0.0, mean as f32]
        });
    }
    drop(alpha);
    for c in 0..3 {
        let sum = box_sum(|i| premultiplied(i, c), w, h, r, job)?;
        for (i, (pixel, s)) in out.iter_mut().zip(&sum).enumerate() {
            job.every(i)?;
            if pixel[3] != 0.0 {
                pixel[c] = (s / count) as f32;
            }
        }
        job.report((c + 2) as f64 / 5.0, 6)?;
    }
    Ok(out)
}

/// One positional standard-normal draw (Box–Muller). `u1` is in (0, 1] so the
/// logarithm is finite; 53 bits is the binary64 mantissa.
fn normal_at(seed: u64, x: usize, y: usize) -> f64 {
    let mut rng = SplitMix64::at(seed, x as u64, y as u64, 0);
    let scale = (1_u64 << 53) as f64;
    let u1 = ((rng.next_u64() >> 11) + 1) as f64 / scale;
    let u2 = (rng.next_u64() >> 11) as f64 / scale;
    (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
}

fn grain_plane(
    dimensions: (u32, u32),
    size: f64,
    roughness: f64,
    seed: u64,
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    let (w, h) = (dimensions.0 as usize, dimensions.1 as usize);
    let n = w * h;
    let taps = gaussian_taps(size)?;
    let half = (taps.len() / 2) as i64;
    let mut noise: Vec<f32> = reserved_vec(n)?;
    for i in 0..n {
        job.every(i)?;
        noise.push(normal_at(seed, i % w, i / w) as f32);
    }
    job.report(0.3, 6)?;
    let mut horizontal: Vec<f32> = reserved_vec(n)?;
    for y in 0..h {
        job.check()?;
        for x in 0..w {
            let mut acc = 0.0_f64;
            for (j, weight) in taps.iter().enumerate() {
                let sx = (x as i64 + j as i64 - half).clamp(0, w as i64 - 1) as usize;
                acc += weight * f64::from(noise[y * w + sx]);
            }
            horizontal.push(acc as f32);
        }
    }
    job.report(0.6, 6)?;
    // A separable blur of unit white noise has variance (Σk²)², so dividing by
    // Σk² restores unit variance. The rough field is the white noise itself,
    // whose covariance with the blurred field is k[mid]²/Σk²; the mix is
    // renormalized by that, so every roughness has the same RMS and Amount
    // means one thing.
    let energy: f64 = taps.iter().map(|k| k * k).sum();
    let rough = roughness / 100.0;
    let smooth = 1.0 - rough;
    let mid = taps[half as usize];
    let norm = (smooth * smooth + rough * rough + 2.0 * rough * smooth * mid * mid / energy).sqrt();
    let mut out = reserved_vec(n)?;
    for y in 0..h {
        job.check()?;
        for x in 0..w {
            let mut acc = 0.0_f64;
            for (j, weight) in taps.iter().enumerate() {
                let sy = (y as i64 + j as i64 - half).clamp(0, h as i64 - 1) as usize;
                acc += weight * f64::from(horizontal[sy * w + x]);
            }
            let s = acc / energy;
            let z = f64::from(noise[y * w + x]);
            out.push([((smooth * s + rough * z) / norm) as f32, 0.0, 0.0, 0.0]);
        }
    }
    Ok(out)
}

/// One `dr_image_filter_op` call after its parameters are parsed: a spatial
/// kernel, a photographic neighborhood and combine step, the legacy
/// Gaussian Blur (`sampling::fitted_blur`), or a Liquify stroke list
/// (`liquify::plane`), or a Depth Blur request (`depth_blur`).
enum ByteFilter {
    Spatial(SpatialKernel),
    Photo {
        neighborhood: Neighborhood,
        kind: FilterKind,
    },
    Gaussian {
        sigma: f32,
    },
    Liquify(Vec<LiquifyStroke>),
    DepthBlur(Box<DepthBlur>),
    DevelopFilterMasked(Box<DevelopFilterMasked>),
}

/// `blur`'s one key, `sigma` (default 4): finite, above 0 and at most
/// `sampling_gaussian_fast::MAX_SIGMA`. It is parsed here, not by
/// `SelectedFilter::parse`, whose 16-bit `blur` stops at σ64. A σ that is
/// positive in binary64 but rounds to 0 as the f32 the kernel takes is refused
/// too, so no σ is run as another.
fn gaussian_sigma(params: &Map<String, Value>) -> Result<f32, String> {
    if let Some(key) = params.keys().find(|k| k.as_str() != "sigma") {
        return Err(format!("unexpected blur parameter {key}"));
    }
    let n = params.get("sigma").map_or(Ok(4.0), |v| {
        v.as_f64()
            .ok_or_else(|| "sigma must be numeric".to_string())
    })?;
    let sigma = n as f32;
    if !n.is_finite() || n <= 0.0 || n > f64::from(MAX_SIGMA) || sigma <= 0.0 {
        return Err("sigma must be finite in (0,1000]".into());
    }
    Ok(sigma)
}

/// The refusals [`filter_op`] and [`filter_op_scratch_bytes`] share, in one
/// order: the op name, then `blur`'s own `sigma` or `SelectedFilter::parse`,
/// then any variant the byte path does not run. `None` is an exact bypass (a
/// photographic `bypass`) or identity (a zero grain amount, a zero offset);
/// `blur` is never one, and `liquify` is one only once its strokes have run
/// (`filter_op` answers it then); `depth_blur` is one when it would change
/// nothing (`DepthBlur::identity`).
fn dispatch(op: &str, params: &Map<String, Value>) -> Result<Option<ByteFilter>, String> {
    const REFUSAL: &str = "dr_image_filter_op supports blur, box_blur, surface_blur, add_grain, \
        radial_blur, clouds, difference_clouds, offset, motion_blur, lens_blur, tilt_shift, \
        lens_correction, presence, develop_filter, smart_sharpen, liquify, depth_blur, unsharp_mask, \
        high_pass, dust_and_scratches and reduce_noise only";
    if op == "blur" {
        return gaussian_sigma(params).map(|sigma| Some(ByteFilter::Gaussian { sigma }));
    }
    if !matches!(
        op,
        "box_blur"
            | "surface_blur"
            | "add_grain"
            | "radial_blur"
            | "clouds"
            | "difference_clouds"
            | "offset"
            | "motion_blur"
            | "lens_blur"
            | "tilt_shift"
            | "lens_correction"
            | "presence"
            | "develop_filter"
            | "smart_sharpen"
            | "liquify"
            | "depth_blur"
            | "unsharp_mask"
            | "high_pass"
            | "dust_and_scratches"
            | "reduce_noise"
    ) {
        return Err(REFUSAL.into());
    }
    match SelectedFilter::parse(op, params)? {
        SelectedFilter::Spatial(kernel) => {
            Ok((!kernel.identity()).then_some(ByteFilter::Spatial(kernel)))
        }
        SelectedFilter::Photo {
            neighborhood,
            kind,
            bypass,
        } => Ok((!bypass).then_some(ByteFilter::Photo { neighborhood, kind })),
        SelectedFilter::Liquify(strokes) => Ok(Some(ByteFilter::Liquify(strokes))),
        SelectedFilter::DepthBlur(depth) => {
            Ok((!depth.identity()).then_some(ByteFilter::DepthBlur(depth)))
        }
        SelectedFilter::DevelopFilterMasked(masked) => {
            Ok((!masked.identity()).then_some(ByteFilter::DevelopFilterMasked(masked)))
        }
        _ => Err(REFUSAL.into()),
    }
}

/// [`filter_op`]'s planned peak scratch for `op` and `params` over an image of
/// `dimensions`, refused exactly as a controlled `filter_op` refuses before it
/// allocates, in the same words and order: planned at `CONTROLLED_JOB_WORK`,
/// so no work refusal. It allocates no pixels. `Ok(0)` is an exact
/// bypass or identity.
///
/// The run has two phases. First the kernel builds its plane, charged by the
/// kernel's own plan (`neighborhood_plan` or `SpatialKernel::plan`, the one
/// home of each formula). Then the combine holds that n×16 plane beside the
/// n×4 output. The plan is the larger of the two plus `4 · BOOKKEEPING_BYTES`,
/// plus a quarter, so it stays at least 1.25× the measured peak (the gate F
/// record in `designs/phase-9f-selected-tone-status.md`; Box Blur's kernel
/// term and its record are `BOX_BLUR_BYTES_PER_PIXEL`). The edit's commit and
/// first projection after the filter is a later stage the host charges, not
/// this plan. Checked arithmetic.
///
/// Gaussian Blur (`blur`) has no combine stage: `filter_op` returns
/// `sampling::fitted_blur`'s n×4 output and holds no n×16 plane. Its plan is
/// `sampling::fitted_blur_scratch_bytes` (the output plus one band) plus
/// `4 · BOOKKEEPING_BYTES`, plus a quarter, with no 20n maximum; the record is
/// on `gaussian_blur_filter::plan`.
///
/// Liquify (`liquify`) is planned by `liquify::plan` (the output plane
/// and the field) and then as any spatial arm: the 20n maximum, 4 BK and the
/// quarter. `liquify::plan` refuses a stroke list past `liquify::MAX_WORK`, so
/// this plan makes that refusal, in `filter_op`'s words: it is a parameter
/// refusal (the strokes ask for too much), not a work ceiling, and holds
/// controlled or not. A strength-0 list plans above 0 although its run is the
/// bypass, because only running the strokes shows the field is the identity.
///
/// Depth Blur (`depth_blur`) is planned by `DepthBlur::plan` (its output
/// plane, the depth field and its guided filter, the grade and the grain) and
/// then as any spatial arm; that plan refuses a depth frame outside the image.
///
/// Develop Filter with masks (`develop_filter` with `masks`) is planned
/// by `DevelopFilterMasked::plan` and then as any spatial arm.
pub(crate) fn filter_op_scratch_bytes(
    dimensions: (u32, u32),
    op: &str,
    params: &Map<String, Value>,
) -> Result<u64, String> {
    let overflow = || "filter plan overflow".to_string();
    let kernel = match dispatch(op, params)? {
        None => return Ok(0),
        Some(ByteFilter::Photo { neighborhood, .. }) => {
            neighborhood_plan(neighborhood, dimensions, CONTROLLED_JOB_WORK)?.0
        }
        Some(ByteFilter::Spatial(kernel)) => kernel.plan(dimensions, CONTROLLED_JOB_WORK)?.0,
        Some(ByteFilter::Liquify(strokes)) => crate::liquify::plan(dimensions, &strokes)?,
        Some(ByteFilter::DepthBlur(depth)) => depth.plan(dimensions)?.0,
        Some(ByteFilter::DevelopFilterMasked(masked)) => {
            masked.plan(dimensions, CONTROLLED_JOB_WORK)?.0
        }
        Some(ByteFilter::Gaussian { sigma }) => {
            let base = crate::sampling::fitted_blur_scratch_bytes(dimensions, sigma)?
                .checked_add(4 * BOOKKEEPING_BYTES)
                .ok_or_else(overflow)?;
            return base.checked_add(base.div_ceil(4)).ok_or_else(overflow);
        }
    };
    let n = u64::from(dimensions.0) * u64::from(dimensions.1);
    let combine = n.checked_mul(16 + 4).ok_or_else(overflow)?;
    let base = kernel
        .max(combine)
        .checked_add(4 * BOOKKEEPING_BYTES)
        .ok_or_else(overflow)?;
    base.checked_add(base.div_ceil(4)).ok_or_else(overflow)
}

/// The byte twin behind `dr_image_filter_op`, for the twenty-one ops it names,
/// under the caller's `job`. Twenty take `SelectedFilter::parse`'s parameters,
/// so the keys, ranges and defaults are the 16-bit selected filter's (plus the
/// `region_*` frame of Radial Blur and Offset, which the 16-bit path derives
/// from its coverage instead); `blur` takes its own `sigma` and runs
/// `sampling::fitted_blur` (measured against Photoshop; `dr_image_sharpen`
/// keeps `sampling::blur`), with no work ceiling, controlled or not. `liquify` runs `liquify::plane`, which
/// refuses as its plan does and has no work ceiling either, and `depth_blur`
/// runs `DepthBlur::plane_in` at `Unit`, its plane the final sample, as a masked
/// `develop_filter` runs `DevelopFilterMasked::plane_in`. `Ok(None)` is an
/// exact bypass (a zero grain amount, a zero offset, an all-default lens
/// correction, Presence or Develop Filter with every control at 0, a photographic filter's
/// `bypass`, or liquify strokes that leave the field the identity), reported
/// as "nothing changed" rather than a copy. An encoded document's form: the
/// `None` of [`filter_op_in`].
pub(crate) fn filter_op(
    img: &RgbaImage,
    op: &str,
    params: &Map<String, Value>,
    job: JobContext<'_>,
) -> Result<Option<RgbaImage>, String> {
    filter_op_in(img, op, params, None, job)
}

/// [`filter_op`] under a document's evaluation policy (`dr_doc_filter_op`):
/// `linear` is a linear-light document's transfer, which only `liquify`
/// reads — its resample decodes each tap and encodes once
/// (`LiquifyField::render`). Every other op, and `liquify` with `None`, is
/// [`filter_op`]'s arm byte for byte: the other filters still edit the stored
/// codes (11E D9).
pub(crate) fn filter_op_in(
    img: &RgbaImage,
    op: &str,
    params: &Map<String, Value>,
    linear: Option<&dyn crate::sampling_linear::LinearUnit>,
    job: JobContext<'_>,
) -> Result<Option<RgbaImage>, String> {
    let kernel = match dispatch(op, params)? {
        None => return Ok(None),
        Some(ByteFilter::Photo { neighborhood, kind }) => {
            return crate::ops_photographic::apply(img, neighborhood, kind, job);
        }
        Some(ByteFilter::Gaussian { sigma }) => {
            return crate::sampling::fitted_blur(img, sigma, job).map(Some);
        }
        Some(
            filter @ (ByteFilter::Liquify(_)
            | ByteFilter::DepthBlur(_)
            | ByteFilter::DevelopFilterMasked(_)),
        ) => {
            let dimensions = img.dimensions();
            let raw = img.as_raw().as_chunks::<4>().0;
            let read = |i: usize| raw[i].map(|v| f32::from(v) / 255.0);
            let plane = match filter {
                ByteFilter::DepthBlur(depth) => {
                    depth.plan(dimensions)?;
                    Some(depth.plane_in(read, dimensions, &SampleRange::Unit, None, job)?)
                }
                ByteFilter::Liquify(strokes) => {
                    crate::liquify::plane(read, dimensions, &strokes, linear, job)?
                }
                ByteFilter::DevelopFilterMasked(masked) => {
                    let cap = destructive_work_cap(job);
                    masked.plan(dimensions, cap)?;
                    Some(masked.plane_in(read, dimensions, cap, job)?)
                }
                _ => None,
            };
            let Some(plane) = plane else {
                return Ok(None);
            };
            // Quantized as the spatial arms below are; there is no combine.
            let mut out: Vec<u8> = reserved_vec(raw.len() * 4)?;
            for sample in &plane {
                if sample.iter().any(|v| !v.is_finite()) {
                    return Err("filter produced an invalid component".into());
                }
                out.extend_from_slice(&sample.map(|v| u8::quantize(v * 255.0)));
            }
            return RgbaImage::from_raw(dimensions.0, dimensions.1, out)
                .map(Some)
                .ok_or_else(|| "invalid filtered image dimensions".into());
        }
        Some(ByteFilter::Spatial(kernel)) => kernel,
    };
    spatial_bytes(img, kernel, job).map(Some)
}

/// One spatial kernel over straight byte samples under `job`: planned at
/// `destructive_work_cap(job)` before any pixel allocation, its plane, its
/// combine, each component quantized. `filter_op`'s spatial tail, and the
/// export workspace's Smart Sharpen stage (`export_sharpen`).
pub(crate) fn spatial_bytes(
    img: &RgbaImage,
    kernel: SpatialKernel,
    job: JobContext<'_>,
) -> Result<RgbaImage, String> {
    let dimensions = img.dimensions();
    // Refuse an over-budget plan BEFORE any pixel allocation.
    let cap = destructive_work_cap(job);
    kernel.plan(dimensions, cap)?;
    let raw = img.as_raw().as_chunks::<4>().0;
    let unit = |p: &[u8; 4]| p.map(|v| f32::from(v) / 255.0);
    let plane = kernel.plane(|i| unit(&raw[i]), dimensions, cap, job)?;
    let mut out: Vec<u8> = reserved_vec(raw.len() * 4)?;
    for (pixel, sample) in raw.iter().zip(&plane) {
        let value = kernel.combine(unit(pixel), *sample);
        if value.iter().any(|v| !v.is_finite()) {
            return Err("filter produced an invalid component".into());
        }
        out.extend_from_slice(&value.map(|v| u8::quantize(v * 255.0)));
    }
    RgbaImage::from_raw(dimensions.0, dimensions.1, out)
        .ok_or_else(|| "invalid filtered image dimensions".into())
}
