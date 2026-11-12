//! THE selected-pixels walk every depth runs (the move and the transform, f32 included):
//! the canvas-rect helpers, the extent rule ([`move_extent`]), the per-pixel
//! body ([`moved_pixel`]), the depth's stored sample and the walk itself
//! ([`move_samples`] → [`land_samples`]). The request, its plan and the byte
//! kernel live in `selected_pixels`, which re-exports what its consumers use.
//!
//! `Landing`, the `CanvasRect` helpers (`intersect`/`union`/`area`/`contains`), `move_extent`,
//! `moved_pixel`, `StoredSample`, `MovedSamples`, `plan_scratch`, `move_samples` and
//! `land_samples`. It imports only `OUTSIDE_BOUNDS` and `PLAN_BOOKKEEPING` from `selected_pixels`;
//! consumers keep importing through `selected_pixels`'s `pub(crate) use`.

use crate::blend::{blend_kind, composite_source_into, erase_alpha, BlendMode};
use crate::doc_transform::Sampler;
use crate::job_control::JobContext;
use crate::layer_rect::coverage_rect16;
use crate::precision_image::reserved_vec;
use crate::precision_sampling::visible_range;
use crate::sampling_linear::LinearUnit;
use crate::selected_pixels::{OUTSIDE_BOUNDS, PLAN_BOOKKEEPING};
use crate::selected_pixels_float::float_rect;
use crate::selected_pixels_perspective::PerspectiveLanding;
use crate::selected_pixels_puppet::PuppetLanding;
use crate::selected_pixels_transform::{
    float_sample, landing_extended, landing_sample, SelectedTransform,
};

/// Where the float lands: the move's integer offset, the transform's
/// resampling map, or a Puppet Warp pose or Perspective Warp planes.
pub(crate) enum Landing {
    Offset((i64, i64)),
    Transform(SelectedTransform),
    Puppet(PuppetLanding),
    Perspective(PerspectiveLanding),
}

/// What the float leaves behind where it was lifted from: nothing
/// (the ⌥ duplicate), the eraser's `a·(1−c)` (a layer), or the background
/// colour as coverage gray, `lerp(v, bg, c)` with the alpha kept (a mask or
/// alpha channel, a plane without transparency).
#[derive(Clone, Copy)]
pub(crate) enum Hole {
    Keep,
    Erase,
    Fill(f32),
}

/// A canvas-space rect `(x, y, w, h)` with `w, h > 0`.
pub(crate) type CanvasRect = (i64, i64, u32, u32);

pub(crate) fn intersect(a: CanvasRect, b: CanvasRect) -> Option<CanvasRect> {
    let x0 = a.0.max(b.0);
    let y0 = a.1.max(b.1);
    let x1 = (a.0 + i64::from(a.2)).min(b.0 + i64::from(b.2));
    let y1 = (a.1 + i64::from(a.3)).min(b.1 + i64::from(b.3));
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    Some((x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
}

pub(crate) fn union(a: CanvasRect, b: CanvasRect) -> CanvasRect {
    let x0 = a.0.min(b.0);
    let y0 = a.1.min(b.1);
    let x1 = (a.0 + i64::from(a.2)).max(b.0 + i64::from(b.2));
    let y1 = (a.1 + i64::from(a.3)).max(b.1 + i64::from(b.3));
    (x0, y0, (x1 - x0) as u32, (y1 - y0) as u32)
}

pub(crate) fn area(rect: CanvasRect) -> u64 {
    u64::from(rect.2) * u64::from(rect.3)
}

fn contains(rect: CanvasRect, (x, y): (i64, i64)) -> bool {
    x >= rect.0 && y >= rect.1 && x < rect.0 + i64::from(rect.2) && y < rect.1 + i64::from(rect.3)
}

/// layer ∪ ((float + offset) ∩ canvas); the layer itself when the landing
/// misses the canvas: the extent measured against Photoshop.
pub(crate) fn move_extent(
    layer: CanvasRect,
    float: CanvasRect,
    offset: (i64, i64),
    canvas: (u32, u32),
) -> CanvasRect {
    let landed = (float.0 + offset.0, float.1 + offset.1, float.2, float.3);
    match intersect(landed, (0, 0, canvas.0, canvas.1)) {
        Some(landing) => union(layer, landing),
        None => layer,
    }
}

/// THE per-pixel body for both depths. `original` is normalized straight RGBA
/// at p (zeros outside the layer), and `c` is the coverage at p (0
/// off-canvas). `float` is the ORIGINAL (rgba, c) at p−offset. The caller
/// passes it ONLY when p is on the canvas AND p−offset ∈ F. A float sample
/// with a·c == 0 is NOT a landing: no composite, because
/// `composite_source_into` requires sa > 0 (blend.rs:400).
/// `fill` None cuts the eraser's hole; `Some(v)` (a plane's hole) lerps
/// the RGB toward `v` by `c` and keeps the alpha.
/// Returns None when c == 0 and there is no landing, so the caller keeps the
/// stored bits.
pub(crate) fn moved_pixel(
    original: [f32; 4],
    c: f32,
    float: Option<([f32; 4], f32)>,
    fill: Option<f32>,
) -> Option<[f32; 4]> {
    let landing = float.filter(|(rgba, fc)| rgba[3] * fc > 0.0);
    if c <= 0.0 && landing.is_none() {
        return None;
    }
    let mut pixel = original;
    match fill {
        None => pixel[3] = erase_alpha(original[3], c),
        Some(v) => {
            for channel in &mut pixel[..3] {
                *channel += (v - *channel) * c;
            }
        }
    }
    if let Some((rgba, fc)) = landing {
        composite_source_into(
            std::slice::from_mut(&mut pixel),
            0,
            [rgba[0], rgba[1], rgba[2]],
            rgba[3] * fc,
            blend_kind(BlendMode::Normal),
            (0, 0),
        );
    }
    Some(pixel)
}

/// One stored sample at any depth: normalize, and quantize ONCE (never at f32).
pub(crate) trait StoredSample: Copy + Default {
    /// f32 storage: no quantize, the transform lands through `landing_extended`.
    const EXTENDED: bool = false;
    fn unit(self) -> [f32; 4];
    fn store(value: [f32; 4]) -> Self;
    fn alpha(self) -> bool;
}

impl StoredSample for [u8; 4] {
    fn unit(self) -> [f32; 4] {
        self.map(|v| f32::from(v) / 255.0)
    }
    fn store(value: [f32; 4]) -> Self {
        value.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
    }
    fn alpha(self) -> bool {
        self[3] > 0
    }
}

impl StoredSample for [u16; 4] {
    fn unit(self) -> [f32; 4] {
        self.map(|v| f32::from(v) / 65535.0)
    }
    fn store(value: [f32; 4]) -> Self {
        value.map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16)
    }
    fn alpha(self) -> bool {
        self[3] > 0
    }
}

/// 32-bit float: identity, nothing quantizes; extended values (above 1, negative) pass
/// through the walk untouched.
impl StoredSample for [f32; 4] {
    const EXTENDED: bool = true;
    fn unit(self) -> [f32; 4] {
        self
    }
    fn store(value: [f32; 4]) -> Self {
        value
    }
    fn alpha(self) -> bool {
        self[3] > 0.0
    }
}

/// The moved layer, as the depth's writer takes it.
pub(crate) enum MovedSamples<S> {
    /// The extent is unchanged: overwrite this LAYER-space rect.
    Rect((u32, u32, u32, u32), Vec<S>),
    /// The extent grew: a whole new plane standing at this canvas offset.
    Plane((i32, i32), (u32, u32), Vec<S>),
}

/// Delta-independent scratch: the float read over L∩C, the output over
/// bbox(L∪C) at `sample_bytes` per pixel, plus bookkeeping.
pub(crate) fn plan_scratch(canvas: (u32, u32), layer: CanvasRect, sample_bytes: u64) -> u64 {
    let canvas_rect = (0, 0, canvas.0, canvas.1);
    let inside = intersect(layer, canvas_rect).map_or(0, area);
    let hull = area(union(layer, canvas_rect));
    inside
        .saturating_mul(16)
        .saturating_add(hull.saturating_mul(sample_bytes))
        .saturating_add(PLAN_BOOKKEEPING)
}

/// THE walk both depths run. `read` answers a LAYER-space rect of original
/// samples. `layer` is the layer's canvas rect. `landing` says where the
/// float lands: the move's integer offset, or the transform's resample
/// (F grown by the kernel reach, see `selected_pixels_transform`). Both reach the same
/// per-pixel walk, [`land_samples`]. `hole` [`Hole::Keep`] is the ⌥-drag duplicate;
/// `bounds` is the request's promise, checked before any read.
///
/// `linear` is a linear-light 8- or 16-bit layer's transfer (`None` for an
/// encoded document, a float layer, a plane and ink). It is read only by a
/// landing that interpolates — a kernel transform, Puppet Warp, Perspective
/// Warp — whose float decodes each original's RGB before premultiplying and
/// encodes each landed sample's RGB once after clamping
/// (`sampling_linear`'s rule). The move and a Nearest transform (every exact
/// form) copy codes. The hole and the source-over landing stay writes into
/// stored samples.
#[allow(clippy::too_many_arguments)]
pub(crate) fn move_samples<S: StoredSample>(
    read: impl Fn(u32, u32, u32, u32) -> Option<Vec<S>>,
    layer: CanvasRect,
    canvas: (u32, u32),
    coverage: &[u16],
    landing: &Landing,
    empty: &'static str,
    hole: Hole,
    bounds: Option<CanvasRect>,
    linear: Option<&dyn LinearUnit>,
    job: JobContext<'_>,
) -> Result<MovedSamples<S>, String> {
    let interpolates = match landing {
        Landing::Offset(_) => false,
        Landing::Transform(transform) => matches!(transform.sampler, Sampler::Kernel(_)),
        Landing::Puppet(_) | Landing::Perspective(_) => true,
    };
    let linear = linear.filter(|_| interpolates && !S::EXTENDED);
    let alloc = || "could not allocate the moved selected pixels".to_string();
    let origin = (
        i32::try_from(layer.0).map_err(|_| "layer offset overflow")?,
        i32::try_from(layer.1).map_err(|_| "layer offset overflow")?,
    );
    let at = |x: i64, y: i64| -> Option<usize> {
        (x >= 0 && y >= 0 && x < i64::from(canvas.0) && y < i64::from(canvas.1))
            .then(|| y as usize * canvas.0 as usize + x as usize)
    };
    let covered = coverage_rect16(coverage, canvas, (layer.2, layer.3), origin).ok_or(empty)?;
    let (rx, ry, rw, rh) = covered;
    if let Some(bounds) = bounds {
        let reach = (layer.0 + i64::from(rx), layer.1 + i64::from(ry), rw, rh);
        if intersect(reach, bounds) != Some(reach) {
            return Err(OUTSIDE_BOUNDS.into());
        }
    }
    let source = read(rx, ry, rw, rh).ok_or_else(alloc)?;
    job.check()?;
    // F, in canvas space.
    let float = float_rect(
        layer,
        canvas,
        coverage,
        covered,
        |lx, ly| source[(ly - ry) as usize * rw as usize + (lx - rx) as usize].alpha(),
        job,
    )?
    .ok_or(empty)?;
    // The ORIGINAL (rgba, c) at an on-canvas q ∈ F.
    let original = |qx: i64, qy: i64, qi: usize| -> ([f32; 4], f32) {
        let (lx, ly) = ((qx - layer.0) as usize, (qy - layer.1) as usize);
        let k = (ly - ry as usize) * rw as usize + (lx - rx as usize);
        (source[k].unit(), f32::from(coverage[qi]) / 65535.0)
    };
    let canvas_rect = (0, 0, canvas.0, canvas.1);
    match landing {
        Landing::Offset(offset) => {
            let landed = (float.0 + offset.0, float.1 + offset.1, float.2, float.3);
            let float_at = |x: i64, y: i64| -> Option<([f32; 4], f32)> {
                at(x, y)?;
                let (qx, qy) = (x - offset.0, y - offset.1);
                if !contains(float, (qx, qy)) {
                    return None;
                }
                let qi = at(qx, qy)?;
                Some(original(qx, qy, qi))
            };
            land_samples(
                &read,
                layer,
                canvas,
                coverage,
                float,
                intersect(landed, canvas_rect),
                float_at,
                hole,
                job,
            )
        }
        Landing::Transform(transform) => {
            let Some(dest) = transform.landing(float, canvas)? else {
                return land_samples(
                    &read,
                    layer,
                    canvas,
                    coverage,
                    float,
                    None,
                    |_, _| None,
                    hole,
                    job,
                );
            };
            let floated = floated_samples(float, &at, &original, linear, job)?;
            let resampled = transform.resample(&floated, float, dest, job)?;
            drop(floated);
            land_resampled(
                &read, layer, canvas, coverage, float, dest, resampled, &at, &original, hole,
                linear, job,
            )
        }
        Landing::Puppet(puppet) => {
            let floated = floated_samples(float, &at, &original, linear, job)?;
            let opaque = |qx: i64, qy: i64| {
                at(qx, qy).is_some_and(|qi| {
                    let (rgba, c) = original(qx, qy, qi);
                    rgba[3] > 0.0 && c > 0.0
                })
            };
            let landed = puppet.resample(&floated, float, canvas, opaque, job)?;
            drop(floated);
            let Some((dest, resampled)) = landed else {
                return land_samples(
                    &read,
                    layer,
                    canvas,
                    coverage,
                    float,
                    None,
                    |_, _| None,
                    hole,
                    job,
                );
            };
            land_resampled(
                &read, layer, canvas, coverage, float, dest, resampled, &at, &original, hole,
                linear, job,
            )
        }
        Landing::Perspective(perspective) => {
            let floated = floated_samples(float, &at, &original, linear, job)?;
            let landed = perspective.resample(&floated, float, layer, canvas, job)?;
            drop(floated);
            let Some((dest, resampled)) = landed else {
                return land_samples(
                    &read,
                    layer,
                    canvas,
                    coverage,
                    float,
                    None,
                    |_, _| None,
                    hole,
                    job,
                );
            };
            land_resampled(
                &read, layer, canvas, coverage, float, dest, resampled, &at, &original, hole,
                linear, job,
            )
        }
    }
}

/// The float's premultiplied f32 samples over `float` (`float_sample` of
/// each on-canvas pixel's original `(rgba, c)`, zero off the canvas). With
/// `linear`, each original's RGB is decoded first.
fn floated_samples(
    float: CanvasRect,
    at: &impl Fn(i64, i64) -> Option<usize>,
    original: &impl Fn(i64, i64, usize) -> ([f32; 4], f32),
    linear: Option<&dyn LinearUnit>,
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    let mut floated = reserved_vec(area(float) as usize)?;
    for row in 0..float.3 {
        job.check()?;
        let qy = float.1 + i64::from(row);
        for col in 0..float.2 {
            let qx = float.0 + i64::from(col);
            let (rgba, c) = at(qx, qy).map_or(([0.0; 4], 0.0), |qi| original(qx, qy, qi));
            let rgba = match linear {
                Some(transfer) => {
                    let rgb = transfer.linear([rgba[0], rgba[1], rgba[2]]);
                    [rgb[0], rgb[1], rgb[2], rgba[3]]
                }
                None => rgba,
            };
            floated.push(float_sample(rgba, c));
        }
    }
    Ok(floated)
}

/// The resampling landings' shared tail (the transform and Puppet
/// Warp): at f32 D3's clamp range (the float's own visible range), each
/// landed premultiplied sample through `landing_sample` / `landing_extended`,
/// then [`land_samples`]. With `linear`, each landed sample's clamped RGB is
/// encoded once before it reaches `moved_pixel`.
#[allow(clippy::too_many_arguments)]
fn land_resampled<S: StoredSample>(
    read: &impl Fn(u32, u32, u32, u32) -> Option<Vec<S>>,
    layer: CanvasRect,
    canvas: (u32, u32),
    coverage: &[u16],
    float: CanvasRect,
    dest: CanvasRect,
    resampled: Vec<[f32; 4]>,
    at: &impl Fn(i64, i64) -> Option<usize>,
    original: &impl Fn(i64, i64, usize) -> ([f32; 4], f32),
    hole: Hole,
    linear: Option<&dyn LinearUnit>,
    job: JobContext<'_>,
) -> Result<MovedSamples<S>, String> {
    // f32: D3's clamp range is the float's own visible range, the RGB of F's
    // pixels with a·c > 0, read in a second pass over F rather than a copy.
    let range = if S::EXTENDED {
        let pixels = (0..float.3).flat_map(move |row| {
            let qy = float.1 + i64::from(row);
            (0..float.2).map(move |col| {
                let qx = float.0 + i64::from(col);
                let (rgba, c) = at(qx, qy).map_or(([0.0; 4], 0.0), |qi| original(qx, qy, qi));
                [rgba[0], rgba[1], rgba[2], rgba[3] * c]
            })
        });
        Some(visible_range(pixels, job)?)
    } else {
        None
    };
    let landed_at = |x: i64, y: i64| -> Option<([f32; 4], f32)> {
        if !contains(dest, (x, y)) {
            return None;
        }
        let k = (y - dest.1) as usize * dest.2 as usize + (x - dest.0) as usize;
        match (&range, linear) {
            (Some(range), _) => landing_extended(resampled[k], range),
            (None, None) => landing_sample(resampled[k]),
            (None, Some(transfer)) => landing_sample(resampled[k]).map(|(rgba, fc)| {
                let rgb = transfer.encoded([rgba[0], rgba[1], rgba[2]]);
                ([rgb[0], rgb[1], rgb[2], rgba[3]], fc)
            }),
        }
    };
    land_samples(
        read,
        layer,
        canvas,
        coverage,
        float,
        Some(dest),
        landed_at,
        hole,
        job,
    )
}

/// The per-pixel walk over the extent `layer ∪ landed` ([`move_extent`]),
/// `landed` already clipped to the canvas. `landing(x, y)` answers the float
/// sample arriving at canvas pixel `(x, y)` — [`moved_pixel`]'s `float`.
/// `hole` [`Hole::Keep`] (the duplicate) hands `moved_pixel` c = 0 for the
/// hole, so the original stays and only the landing composites.
#[allow(clippy::too_many_arguments)]
fn land_samples<S: StoredSample>(
    read: &impl Fn(u32, u32, u32, u32) -> Option<Vec<S>>,
    layer: CanvasRect,
    canvas: (u32, u32),
    coverage: &[u16],
    float: CanvasRect,
    landed: Option<CanvasRect>,
    landing: impl Fn(i64, i64) -> Option<([f32; 4], f32)>,
    hole: Hole,
    job: JobContext<'_>,
) -> Result<MovedSamples<S>, String> {
    let alloc = || "could not allocate the moved selected pixels".to_string();
    let at = |x: i64, y: i64| -> Option<usize> {
        (x >= 0 && y >= 0 && x < i64::from(canvas.0) && y < i64::from(canvas.1))
            .then(|| y as usize * canvas.0 as usize + x as usize)
    };
    let fill = match hole {
        Hole::Fill(v) => Some(v),
        Hole::Keep | Hole::Erase => None,
    };
    let pixel_at = |x: i64, y: i64, stored: S| -> S {
        let c = match at(x, y) {
            Some(i) if !matches!(hole, Hole::Keep) => f32::from(coverage[i]) / 65535.0,
            _ => 0.0,
        };
        moved_pixel(stored.unit(), c, landing(x, y), fill).map_or(stored, S::store)
    };
    let extent = landed.map_or(layer, |landed| move_extent(layer, landed, (0, 0), canvas));
    if extent == layer {
        // The hole is F and the landing is inside the layer here; only their
        // bounding box can change.
        let window = landed.map_or(float, |landing| union(float, landing));
        let (wx, wy) = ((window.0 - layer.0) as u32, (window.1 - layer.1) as u32);
        let mut samples = read(wx, wy, window.2, window.3).ok_or_else(alloc)?;
        for row in 0..window.3 {
            job.check()?;
            let y = window.1 + i64::from(row);
            for col in 0..window.2 {
                let k = row as usize * window.2 as usize + col as usize;
                samples[k] = pixel_at(window.0 + i64::from(col), y, samples[k]);
            }
        }
        return Ok(MovedSamples::Rect((wx, wy, window.2, window.3), samples));
    }
    let plane_origin = (
        i32::try_from(extent.0).map_err(|_| "the moved layer's offset leaves i32")?,
        i32::try_from(extent.1).map_err(|_| "the moved layer's offset leaves i32")?,
    );
    if area(extent) > crate::doc::MAX_PIXELS {
        return Err("the moved layer exceeds the supported pixel limit".into());
    }
    let mut samples = Vec::new();
    samples
        .try_reserve_exact(area(extent) as usize)
        .map_err(|_| alloc())?;
    for row in 0..extent.3 {
        job.check()?;
        let y = extent.1 + i64::from(row);
        let line = if y >= layer.1 && y < layer.1 + i64::from(layer.3) {
            read(0, (y - layer.1) as u32, layer.2, 1).ok_or_else(alloc)?
        } else {
            Vec::new()
        };
        for col in 0..extent.2 {
            let x = extent.0 + i64::from(col);
            let stored = if !line.is_empty() && x >= layer.0 && x < layer.0 + i64::from(layer.2) {
                line[(x - layer.0) as usize]
            } else {
                S::default()
            };
            samples.push(pixel_at(x, y, stored));
        }
    }
    Ok(MovedSamples::Plane(
        plane_origin,
        (extent.2, extent.3),
        samples,
    ))
}
