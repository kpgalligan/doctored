//! The layer-style plane primitives and their numerics, split out of
//! `style_render` (which re-exports every item) in Phase 13A-5b-i: ONE blur,
//! ONE dilate/erode, shift, multiply, subtract and invert, with the
//! `_with_job` twins the cancellable plane render polls through — the same
//! bytes (`doc_select::offset_contour_with_context` is `offset_contour`'s
//! body). The blur, dilate and erode exist only as their `_with_job` forms
//! (`plane_blur::bytes_with_job` is the one byte blur).
//!
//! # Numeric conventions (one home each)
//!
//! - `sigma_for_size(size) = size / 2`: an effect's "size" is the visible
//!   extent of the soft edge; a Gaussian at sigma = size/2 has faded to
//!   ~5 % two sigmas out, i.e. at `size`.
//! - `feather_radius(sigma)` inverts `doc_select::feather_mask`'s
//!   `sigma = 0.3 (r - 1) + 0.8`, floored at 0.5 so any positive size still
//!   gets the 3-tap minimum kernel. `size == 0` means no blur (hard edge).
//! - `blur_plane_with_job` routes every plane through `feather_mask` (the master
//!   Gaussian for scalar coverage; `ops::blur` handles straight RGBA
//!   through the shared premultiplied sampler and is the wrong tool).
//!   `feather_mask` is O(pixels x taps), so a
//!   50 px shadow (sigma 25, ~330 taps per pass) on a 20 MP layer would take
//!   seconds. Above sigma 3 the plane is therefore blurred at a reduced
//!   resolution — `downsample_factor`: 2x above sigma 3, 4x above 6, 8x
//!   above 12 (each step where the reduced-resolution sigma is 1.5, so the
//!   kernel never exceeds ~13 taps and the cost is flat in sigma) — with a
//!   Triangle resample down and back up, which is invisible at those
//!   softnesses. The two resamples blur a little themselves (a triangle of
//!   half-width `f` has variance `f² / 6`; twice that for the round trip),
//!   so `reduced_sigma` subtracts `f² / 3` from the variance before
//!   dividing by `f`: the total softness stays continuous across a step,
//!   and a size slider crossing 6, 12 or 24 px shows no jump. The plane is
//!   zero-extended to a multiple of `f` before the resample so the sampling
//!   lattice is a fixed function of plane coordinates (an exact ratio, the
//!   same weights everywhere) — what keeps a sub-plane whose origin sits on
//!   the [`SUB_PLANE_ALIGN`] lattice bit-identical to the full plane.
//! - `blur_reach(sigma)` is how far the blur reads or writes from any
//!   pixel: the kernel half-width `feather_mask` builds (`taps =
//!   nearest_odd(2r + 1)`, so at most `ceil(r)`) plus 1 for the odd
//!   rounding; at a reduced resolution that half-width plus the two
//!   resamples' support (a Triangle down-sample reads `[-f/2, 3f/2)` around
//!   its cell, the up-sample the neighbouring reduced pixel on each side —
//!   together two reduced pixels), all scaled back by `f`:
//!   `f * (ceil(r_f) + 1 + 2)`. It is the ONE definition `LayerStyle::pad()`
//!   and every effect use — what makes a plane's padding exact.
//! - `dilate_plane_with_job` / `erode_plane_with_job` are `grow_mask` / `shrink_mask`: they
//!   binarize at >= 128 and re-anti-alias with a ~1 px ramp — right for
//!   spread/choke/stroke, never run for size-only effects.
//! - `outside_stroke_band` / `inside_stroke_band` are THE Edit ▸ Stroke… bands
//!   (lifted out of `style_fx_stroke`): `dilate(cov, px) × (255 − cov)` and
//!   `cov − erode(cov, px)` over a bare coverage plane
//!   (`doc_fill_stroke::stroke_mask`). Layer Style ▸ Stroke shares the
//!   inside band; its outside band is `style_stroke_distance::outside_corner_band`
//!   (#578), the measured chamfer distance from an anti-aliased (`1 − c/255`)
//!   seed, so the two outside bands differ at corners and under anti-aliased
//!   pixels.
//!
//! `style_plane_ops.rs` (split out in 13A-5b-i, every item re-exported by `style_render`) — the
//! numerics (`sigma_for_size`, `blur_reach`, `downsample_factor`, `spread_split`, `light_offset`)
//! and the ONE blur (multi-level downsampled above sigma 3) / dilate / erode / shift / multiply
//! primitives (blur, dilate and erode only as their `_with_job` forms), and the stroke bands
//! `outside_stroke_band` / `inside_stroke_band` (Edit ▸ Stroke… uses both, Layer Style ▸ Stroke
//! the inside one).

use crate::job_control::JobContext;

// ------------------------------------------------------------- numerics --

/// Gaussian sigma for an effect "size" in px (module doc).
pub(crate) fn sigma_for_size(size_px: f32) -> f32 {
    if size_px.is_finite() {
        (size_px / 2.0).max(0.0)
    } else {
        0.0
    }
}

/// The `feather_mask` radius that yields `sigma` (inverse of
/// `sigma = 0.3 (r - 1) + 0.8`), floored at 0.5 so any positive sigma still
/// blurs with the 3-tap minimum kernel.
pub(crate) fn feather_radius(sigma: f32) -> f32 {
    ((sigma - 0.8) / 0.3 + 1.0).max(0.5)
}

/// The lattice a sub-plane's origin must sit on for `blur_plane_with_job` to be
/// bit-identical between the sub-plane and the full plane (module doc):
/// the largest `downsample_factor`.
pub(crate) const SUB_PLANE_ALIGN: u32 = 8;

/// The resolution reduction `blur_plane_with_job` works at for `sigma` (module
/// doc): 1 (direct) up to sigma 3, then 2, 4 and 8 at each doubling.
pub(crate) fn downsample_factor(sigma: f32) -> u32 {
    if sigma.is_nan() || sigma <= 3.0 {
        1
    } else if sigma <= 6.0 {
        2
    } else if sigma <= 12.0 {
        4
    } else {
        8
    }
}

/// The sigma blurred at 1/`f` resolution so that, with the two Triangle
/// resamples' own softening (`f² / 3` of variance), the result matches
/// `sigma` at full resolution (module doc). `pub(crate)` for
/// `adjust_tone::tone_plane`, which reduces a plane itself rather than
/// letting `blur_plane_with_job` do it, so that the crate keeps ONE such rule.
pub(crate) fn reduced_sigma(sigma: f32, f: u32) -> f32 {
    let f = f as f32;
    (sigma * sigma - f * f / 3.0).max(0.0).sqrt() / f
}

/// How far, in px, `blur_plane_with_job` at `sigma` reads or writes from any pixel
/// (module doc). 0 for no blur.
pub(crate) fn blur_reach(sigma: f32) -> u32 {
    if sigma.is_nan() || sigma <= 0.0 {
        return 0;
    }
    let f = downsample_factor(sigma);
    if f == 1 {
        feather_radius(sigma).ceil() as u32 + 1
    } else {
        f * (feather_radius(reduced_sigma(sigma, f)).ceil() as u32 + 1 + 2)
    }
}

/// Splits a size and its spread/choke fraction into `(dilate_px, sigma)`:
/// dilate by `size * spread`, then blur by `size * (1 - spread)`.
pub(crate) fn spread_split(size: f32, spread: f32) -> (f32, f32) {
    let size = if size.is_finite() { size.max(0.0) } else { 0.0 };
    let spread = if spread.is_finite() {
        spread.clamp(0.0, 1.0)
    } else {
        0.0
    };
    (size * spread, sigma_for_size(size * (1.0 - spread)))
}

/// The integer offset a shadow at `angle_deg` and `distance` lands at:
/// `(round(-d cos a), round(+d sin a))` in y-down pixel space, so 120°
/// (the default light, upper-left) casts down-right. Rounded to
/// whole pixels — planes are shifted, never resampled. The products are
/// snapped to a millionth before rounding: the trig identities produce
/// exact halves (`cos 120° = -1/2`, so distance 3 at 120° is 1.5 px) that
/// `f64::cos` returns a few ulps off, and without the snap the same style
/// could land a pixel apart between platforms.
pub(crate) fn light_offset(angle_deg: f32, distance: f32) -> (i32, i32) {
    if !angle_deg.is_finite() || !distance.is_finite() {
        return (0, 0);
    }
    let a = f64::from(angle_deg).to_radians();
    let d = f64::from(distance);
    let snap = |v: f64| {
        let snapped = (v * 1e6).round() / 1e6;
        snapped
            .round()
            .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
    };
    (snap(-d * a.cos()), snap(d * a.sin()))
}

/// THE plane blur (module doc): `sigma <= 0` is a no-op; up to sigma 3 a
/// direct `feather_mask`; above it the plane is zero-extended to a multiple
/// of the `downsample_factor`, resampled down, blurred at `reduced_sigma`,
/// and resampled back. Polls `job` between cells, rows and resamples
/// (Phase 13A-5a).
pub(crate) fn blur_plane_with_job(
    plane: &mut [u8],
    w: u32,
    h: u32,
    sigma: f32,
    job: crate::job_control::JobContext<'_>,
) -> Result<(), String> {
    crate::plane_blur::bytes_with_job(plane, w, h, sigma, job)
}

/// Dilates a plane by `px` (`grow_mask`'s bytes; no-op at `px <= 0`),
/// polling `job` inside the distance transform and every 4096 cells of the
/// mapping (Phase 13A-5b-i): `grow_mask`'s no-op guard, then
/// `offset_contour`'s body with the job.
pub(crate) fn dilate_plane_with_job(
    plane: &mut [u8],
    w: u32,
    h: u32,
    px: f32,
    job: JobContext<'_>,
) -> Result<(), String> {
    if !px.is_finite() || px <= 0.0 || w == 0 || h == 0 || plane.len() != w as usize * h as usize {
        return Ok(());
    }
    crate::doc_select::offset_contour_with_context(plane, w as usize, h as usize, px, job)
}

/// Erodes a plane by `px` (`shrink_mask`'s bytes; no-op at `px <= 0`),
/// polling `job` — `shrink_mask`'s guard, then the contour offset inward
/// (see [`dilate_plane_with_job`]).
pub(crate) fn erode_plane_with_job(
    plane: &mut [u8],
    w: u32,
    h: u32,
    px: f32,
    job: JobContext<'_>,
) -> Result<(), String> {
    if !px.is_finite() || px <= 0.0 || w == 0 || h == 0 || plane.len() != w as usize * h as usize {
        return Ok(());
    }
    crate::doc_select::offset_contour_with_context(plane, w as usize, h as usize, -px, job)
}

/// `out(x, y) = plane(x - dx, y - dy)`, `fill` where that falls outside.
pub(crate) fn shift_plane(plane: &[u8], w: u32, h: u32, dx: i32, dy: i32, fill: u8) -> Vec<u8> {
    let (wi, hi) = (i64::from(w), i64::from(h));
    let mut out = vec![fill; plane.len()];
    if plane.len() != w as usize * h as usize {
        return out;
    }
    for y in 0..hi {
        let sy = y - i64::from(dy);
        if sy < 0 || sy >= hi {
            continue;
        }
        for x in 0..wi {
            let sx = x - i64::from(dx);
            if sx < 0 || sx >= wi {
                continue;
            }
            out[(y * wi + x) as usize] = plane[(sy * wi + sx) as usize];
        }
    }
    out
}

/// `a = round(a * b / 255)` per pixel (integer form; 255 is odd, so no
/// product of two bytes lands on a .5 tie).
pub(crate) fn multiply_planes(a: &mut [u8], b: &[u8]) {
    for (x, &y) in a.iter_mut().zip(b) {
        *x = ((u32::from(*x) * u32::from(y) + 127) / 255) as u8;
    }
}

/// `a = a.saturating_sub(b)` per pixel.
pub(crate) fn subtract_planes(a: &mut [u8], b: &[u8]) {
    for (x, &y) in a.iter_mut().zip(b) {
        *x = x.saturating_sub(y);
    }
}

/// `a = 255 - a` per pixel.
pub(crate) fn invert_plane(a: &mut [u8]) {
    for x in a.iter_mut() {
        *x = 255 - *x;
    }
}

/// `dilate(coverage, px)` knocked out by the coverage (`× (255 − coverage)`):
/// the stroke band outside the 50 % contour (module doc).
pub(crate) fn outside_stroke_band(
    coverage: &[u8],
    w: u32,
    h: u32,
    px: f32,
    job: JobContext<'_>,
) -> Result<Vec<u8>, String> {
    job.check()?;
    let mut grown = coverage.to_vec();
    job.check()?;
    dilate_plane_with_job(&mut grown, w, h, px, job)?;
    job.check()?;
    let inverted: Vec<u8> = coverage.iter().map(|&v| 255 - v).collect();
    job.check()?;
    multiply_planes(&mut grown, &inverted);
    Ok(grown)
}

/// `coverage − erode(coverage, px)`: the stroke band inside the 50 %
/// contour (module doc).
pub(crate) fn inside_stroke_band(
    coverage: &[u8],
    w: u32,
    h: u32,
    px: f32,
    job: JobContext<'_>,
) -> Result<Vec<u8>, String> {
    job.check()?;
    let mut shrunk = coverage.to_vec();
    job.check()?;
    erode_plane_with_job(&mut shrunk, w, h, px, job)?;
    job.check()?;
    let mut band = coverage.to_vec();
    job.check()?;
    subtract_planes(&mut band, &shrunk);
    Ok(band)
}
