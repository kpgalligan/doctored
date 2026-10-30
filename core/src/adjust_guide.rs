//! Shared alpha-weighted tone guide construction. Byte storage retains the
//! existing blur/rounding policy; typed storage keeps normalized f32 coverage
//! and directly applies the same Gaussian on the shared reduced guide grid.
//!
//! `adjust_guide.rs` and `gaussian_plane.rs` share byte/float tone-guide and Gaussian algorithms
//! while preserving legacy rounding. Since 13A-5b-iii-c-1 `adjust_guide::build` reports its two
//! grid blurs inside their OWN sub-ranges of the caller's job —
//! `adjust_precision_plan::GuideWork::blur_ranges`, the one home of the guide's work terms
//! (`pixels + cells·(2 + 4·taps)`), which `guide()` also reads — at every depth and caller, with no pixel
//! change.
use crate::adjust_math::luma;
use crate::adjust_precision::plan::GuideWork;
use crate::job_control::JobContext;
use crate::style_render::{
    blur_plane_with_job, downsample_factor, feather_radius, reduced_sigma, sigma_for_size,
};

/// The Shadows/Highlights guide, held at the resolution the signal actually
/// carries rather than at the canvas's — a `w` x `h` grid of `factor`-sized
/// cells, read back by bilinear interpolation ([`GuidePlane::at`]).
///
/// The guide is by construction a blur at `sigma = radius / 2`, so above
/// `factor` 1 it has no detail a full-canvas buffer could hold: sampling it
/// at 1/factor and interpolating is the same picture. Holding it small is
/// what keeps the op affordable at the sizes the app supports — the plane is
/// rebuilt on EVERY composite (`doc::composite_adjustment_into` has no
/// cache, deliberately: the plane is a function of the backdrop, which a
/// live stroke changes on every tick, so a cache keyed on anything cheaper
/// than the backdrop's contents would hand back a stale guide). At 10000 x
/// 10000 the reduced grid is 1250 x 1250: each stored plane uses about
/// 1.5 MB with byte coverage or 6 MB with float coverage, and two blurs visit
/// 1.5 M cells instead of 100 M pixels. The typed admission plan also includes
/// the float accumulation and convolution temporaries.
pub(crate) struct GuidePlane {
    plane: Storage,
    w: u32,
    h: u32,
    factor: u32,
}

enum Storage {
    Bytes(Vec<u8>),
    Floats(Vec<f32>),
}
impl Storage {
    fn raw(&self, i: usize) -> f32 {
        match self {
            Self::Bytes(v) => f32::from(v[i]),
            Self::Floats(v) => v[i],
        }
    }
    fn divisor(&self) -> f32 {
        match self {
            Self::Bytes(_) => 255.0,
            Self::Floats(_) => 1.0,
        }
    }
}
impl GuidePlane {
    /// The guide at full-resolution pixel `(x, y)`, as a fraction in
    /// [0, 1]. Cell centres sit at `(i + 0.5) * factor` in full-resolution
    /// coordinates, so the sample point is `(x + 0.5) / factor - 0.5`,
    /// clamped to the grid at the borders (which is what a blur's own edge
    /// handling does anyway). At `factor` 1 this is a plain lookup, so a
    /// small radius keeps exactly the pixel-for-pixel guide it always had.
    ///
    /// `(x, y)` must be inside the `w` x `h` the plane was built for — both
    /// callers walk exactly that buffer — and is read per pixel of a
    /// composite, so it is written for that and not defended twice.
    pub(crate) fn at(&self, x: u32, y: u32) -> f32 {
        let row = self.w as usize;
        if self.factor == 1 {
            let i = (y as usize) * row + (x as usize);
            return self.plane.raw(i) / self.plane.divisor();
        }
        let f = self.factor as f32;
        let (x0, x1, fx) = cell((x as f32 + 0.5) / f - 0.5, self.w);
        let (y0, y1, fy) = cell((y as f32 + 0.5) / f - 0.5, self.h);
        let at = |cx: usize, cy: usize| self.plane.raw(cy * row + cx);
        let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * fx;
        let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * fx;
        (top + (bottom - top) * fy) / self.plane.divisor()
    }
}

/// The two neighbouring cell indices and the fraction between them for a
/// grid coordinate, clamped into `0..n`.
fn cell(t: f32, n: u32) -> (usize, usize, f32) {
    let last = n.saturating_sub(1) as usize;
    if !t.is_finite() || t <= 0.0 {
        return (0, 0, 0.0);
    }
    let floor = t.floor();
    let i = floor as usize;
    if i >= last {
        return (last, last, 0.0);
    }
    (i, i + 1, t - floor)
}

/// The Shadows/Highlights guide: an ALPHA-WEIGHTED, large-radius blur of the
/// luma.
///
/// Two planes go through the ONE plane blur (`style_render::blur_plane_with_job`) at
/// `sigma = radius / 2` — `style_render::sigma_for_size`, the one sizing
/// rule in this crate, so a Shadows/Highlights radius and a layer-style size
/// of the same number soften by the same amount. The plane is then the RATIO
/// `blur(luma * alpha) / blur(alpha)`, which is what makes the estimate
/// honest at an edge:
///
/// * a transparent pixel contributes NOTHING, so a cut-out subject is not
///   dragged toward black by the empty space around it and gets no halo;
/// * the ratio also normalizes away the blur's own edge handling, so the
///   estimate at the image border is the mean of the pixels that are
///   actually there;
/// * where the blurred alpha is zero the ratio is undefined and the cell's
///   OWN mean luma stands in. The legacy byte path quantizes both planes;
///   the typed path retains float coverage, including values below 1/255.
///
/// **The grid.** `blur_plane_with_job` above sigma 3 already blurs a downsampled copy
/// and resamples back, so the guide never had detail below
/// `style_render::downsample_factor(sigma)` pixels. This builds it at that
/// resolution directly — box-averaging each cell in the ONE pass over the
/// image that reading it requires, blurring the small grid, and letting
/// [`GuidePlane::at`] interpolate — instead of materializing three
/// canvas-sized buffers and putting each through a resample-blur-resample
/// round trip. `style_render::reduced_sigma` is the compensation for that
/// round trip's own softening; a box average down plus a bilinear read back
/// carries variance `f²/4` against the `f²/3` it assumes, which at the
/// factors it selects is under 2 % of the effective sigma — far less than
/// the 8-bit quantization the plane already carries, and worth not having a
/// second sizing rule in the crate.
///
/// At `radius` 0 the blur is a no-op, the factor is 1, and the plane is
/// exactly the per-pixel luma — the honest reading of "no neighbourhood".
///
/// **Progress.** The two grid blurs report inside their own sub-ranges of
/// the caller's `job` — `GuideWork::blur_ranges`, the plan's own terms
/// (Phase 13A-5b-iii-c-1): the pixel pass and per-cell means poll flat at
/// the range's start, the first blur ramps `(p + c)/t → (p + c + 2·c·taps)/t`,
/// the second ramps on to `(t − c)/t`, and the combine polls flat, leaving
/// the caller's advance to land at the range's end.
pub(crate) fn tone_plane(
    w: u32,
    h: u32,
    radius: f32,
    rgba_at: &dyn Fn(usize) -> [f32; 4],
) -> GuidePlane {
    tone_plane_with_job(w, h, radius, rgba_at, JobContext::NONE).expect("uncancellable guide")
}
/// [`tone_plane`] polling `job` — the same byte build, cancellable between
/// cells (the regional compositor's staged guide, Phase 13A-4a). The blur
/// inside polls the job the caller narrowed (Phase 13A-5a: the regional
/// caller hands `checkpoints_only`, so its rows report nothing).
pub(crate) fn tone_plane_with_job(
    w: u32,
    h: u32,
    radius: f32,
    rgba_at: &dyn Fn(usize) -> [f32; 4],
    job: JobContext<'_>,
) -> Result<GuidePlane, String> {
    build::<u8>(w, h, radius, rgba_at, job)
}
pub(crate) fn precision_plane(
    w: u32,
    h: u32,
    radius: f32,
    rgba_at: &dyn Fn(usize) -> [f32; 4],
    job: JobContext<'_>,
) -> Result<GuidePlane, String> {
    build::<f32>(w, h, radius, rgba_at, job)
}
trait GuideSample: crate::gaussian_plane::Scalar {
    fn coverage(v: f32) -> Self;
    fn storage(v: Vec<Self>) -> Storage;
    fn blur(v: &mut [Self], w: u32, h: u32, sigma: f32, job: JobContext<'_>) -> Result<(), String>;
}
impl GuideSample for u8 {
    fn coverage(v: f32) -> Self {
        (v.clamp(0.0, 1.0) * 255.0).round() as u8
    }
    fn storage(v: Vec<Self>) -> Storage {
        Storage::Bytes(v)
    }
    fn blur(v: &mut [Self], w: u32, h: u32, sigma: f32, job: JobContext<'_>) -> Result<(), String> {
        blur_plane_with_job(v, w, h, sigma, job)
    }
}
impl GuideSample for f32 {
    fn coverage(v: f32) -> Self {
        v.clamp(0.0, 1.0)
    }
    fn storage(v: Vec<Self>) -> Storage {
        Storage::Floats(v)
    }
    fn blur(v: &mut [Self], w: u32, h: u32, sigma: f32, job: JobContext<'_>) -> Result<(), String> {
        if sigma <= 0.0 {
            return Ok(());
        }
        crate::gaussian_plane::feather(v, w, h, feather_radius(sigma), job)
    }
}
fn build<T: GuideSample>(
    w: u32,
    h: u32,
    radius: f32,
    rgba_at: &dyn Fn(usize) -> [f32; 4],
    job: JobContext<'_>,
) -> Result<GuidePlane, String> {
    let sigma = sigma_for_size(radius);
    let factor = downsample_factor(sigma);
    let (gw, gh) = (w.div_ceil(factor).max(1), h.div_ceil(factor).max(1));
    let cells = (gw as usize) * (gh as usize);
    let mut weighted_plane = vec![T::coverage(0.0); cells];
    let mut coverage_plane = vec![T::coverage(0.0); cells];
    // Reused as the output plane: it starts as each cell's own mean luma,
    // which is the fallback where the blurred coverage is zero.
    let mut plane = vec![T::coverage(0.0); cells];
    let byte = T::coverage;
    if factor == 1 {
        // The grid IS the image: no cell arithmetic, one pass, straight to
        // the bytes. This is the path every small radius takes, and it is
        // pixel-for-pixel what the guide has always been.
        for i in 0..cells {
            job.every(i)?;
            let px = rgba_at(i);
            let l = luma([px[0], px[1], px[2]]).clamp(0.0, 1.0);
            let a = px[3].clamp(0.0, 1.0);
            plane[i] = byte(l);
            coverage_plane[i] = byte(a);
            weighted_plane[i] = byte(l * a);
        }
    } else {
        let mut weighted = vec![0f32; cells];
        let mut coverage = vec![0f32; cells];
        let mut lumas = vec![0f32; cells];
        // One RUN of a cell's pixels at a time — a cell is `factor`
        // consecutive pixels of a row — so the three sums live in registers
        // across the run and the grid is touched once per run, not once per
        // pixel.
        for y in 0..h {
            job.check()?;
            let row = (y / factor) as usize * gw as usize;
            let base = (y as usize) * (w as usize);
            for cx in 0..gw {
                let (x0, x1) = (cx * factor, ((cx + 1) * factor).min(w));
                let (mut sum_w, mut sum_c, mut sum_l) = (0.0f32, 0.0f32, 0.0f32);
                for x in x0..x1 {
                    job.every(base + x as usize)?;
                    let px = rgba_at(base + x as usize);
                    let l = luma([px[0], px[1], px[2]]).clamp(0.0, 1.0);
                    let a = px[3].clamp(0.0, 1.0);
                    sum_w += l * a;
                    sum_c += a;
                    sum_l += l;
                }
                let c = row + cx as usize;
                weighted[c] += sum_w;
                coverage[c] += sum_c;
                lumas[c] += sum_l;
            }
        }
        // The cell's pixel count, arithmetically: the last row and column
        // are ragged when the dimensions are not a multiple of the factor.
        let span = |i: u32, limit: u32| (((i + 1) * factor).min(limit) - i * factor) as f32;
        for c in 0..cells {
            job.every(c)?;
            let (cx, cy) = ((c % gw as usize) as u32, (c / gw as usize) as u32);
            let n = (span(cx, w) * span(cy, h)).max(1.0);
            weighted_plane[c] = byte(weighted[c] / n);
            coverage_plane[c] = byte(coverage[c] / n);
            plane[c] = byte(lumas[c] / n);
        }
    }
    let grid_sigma = if factor == 1 {
        sigma
    } else {
        reduced_sigma(sigma, factor)
    };
    let r = GuideWork::of(w, h, radius).blur_ranges();
    T::blur(
        &mut weighted_plane,
        gw,
        gh,
        grid_sigma,
        job.range(r[0].0, r[0].1),
    )?;
    T::blur(
        &mut coverage_plane,
        gw,
        gh,
        grid_sigma,
        job.range(r[1].0, r[1].1),
    )?;
    for c in 0..cells {
        job.every(c)?;
        if coverage_plane[c].raw() > 0.0 {
            plane[c] = byte(weighted_plane[c].raw() / coverage_plane[c].raw());
        }
    }
    job.check()?;
    Ok(GuidePlane {
        plane: T::storage(plane),
        w: gw,
        h: gh,
        factor,
    })
}
