//! Retouch source mapping shared by native byte/u16/f32 sampling and source ghosts.
//! The matrix maps destination canvas pixel centres to source canvas centres.
//!
//! Source transforms run in native depth before any ghost display reduction.
//!
//! [`transform`] / [`transform_extended`] map a whole canvas plate onto the
//! whole canvas; [`transform_rect`] / [`transform_rect_extended`] (13B-9a, a
//! typed Clone or Heal) produce only a `frame` of that answer from the taps
//! of one source rect ([`tap_rect`]), every kept sample the whole walk's to
//! the bit.
use crate::doc::{DrDocument, Layer, LayerRaster};
use crate::doc_transform::{Affine, Sampler, SourceMap};
use crate::job_control::JobContext;
use crate::sampling::{self, ColorRange, Kernel, Unorm};

/// Canvas rect, exclusive ends: (x0, y0, x1, y1).
pub(crate) type CanvasRect = (i64, i64, i64, i64);

pub(crate) fn validate(matrix: &[f64; 6]) -> Result<(), String> {
    if matrix.iter().any(|v| !v.is_finite() || v.abs() > 1e9)
        || (matrix[0] * matrix[3] - matrix[1] * matrix[2]).abs() < 1e-9
    {
        return Err("retouch source matrix must be finite, bounded and invertible".into());
    }
    Ok(())
}

pub(crate) fn transform<T: Unorm>(
    source: &[[T; 4]],
    dimensions: (u32, u32),
    matrix: &[f64; 6],
    job: JobContext<'_>,
) -> Result<Vec<[T; 4]>, String> {
    resampled(
        source,
        dimensions,
        matrix,
        [T::default(); 4],
        |acc, p, w| sampling::accumulate_unorm(acc, &p, w),
        sampling::straight_unorm,
        job,
    )
}

/// [`transform`] over a 32-bit float document's plate: the same
/// premultiplied bilinear mapping on straight linear samples, through
/// `sampling`'s extended pair. Bilinear weights are non-negative, so no
/// output leaves its taps' range; the `ColorRange` handed to
/// `straight_extended` is the stored range itself, a guard against rounding
/// at the bound and nothing else.
pub(crate) fn transform_extended(
    source: &[[f32; 4]],
    dimensions: (u32, u32),
    matrix: &[f64; 6],
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    let range = extended_range();
    resampled(
        source,
        dimensions,
        matrix,
        [0.0; 4],
        |acc, p, w| sampling::accumulate_extended(acc, &p, w),
        |acc| sampling::straight_extended(acc, &range),
        job,
    )
}

/// The stored range [`transform_extended`] and [`transform_rect_extended`]
/// hand `straight_extended` (its doc says why it is only a guard).
fn extended_range() -> ColorRange {
    let bound = crate::float_document::FLOAT_RANGE_MAX;
    ColorRange {
        min: [-bound; 3],
        max: [bound; 3],
    }
}

/// The matrix as the walk's `SourceMap`: destination canvas centres to
/// source canvas centres, no offsets.
fn source_map(matrix: &[f64; 6]) -> SourceMap {
    let [a, b, c, d, tx, ty] = *matrix;
    SourceMap::new(&Affine { a, b, c, d, tx, ty }, (0, 0), (0, 0))
}

/// The canvas rect holding every bilinear tap `frame`'s pixels read under
/// `matrix`: the bbox of the frame's four corner centres through the
/// SourceMap, grown 2 px, clamped to `canvas`; an empty frame answers the
/// empty rect.
///
/// Why 2 px: a bilinear tap at `u` reads columns `floor(u − 0.5)` and one
/// more, so the exact reach is the bbox's taps plus one column; the second
/// pixel absorbs the difference between these corners (`row_start + step·i`)
/// and the walk's own running sums (`nu += step`, column by column). That
/// difference is at most `canvas.0 · M · 2⁻⁵²` with `M` the largest corner
/// magnitude over the walked columns `0 … x1 − 1`, far below a pixel for any
/// real matrix; a matrix whose entries or translation make the sums that
/// large grows the margin by it instead, so no tap the walk reads ever falls
/// outside.
pub(crate) fn tap_rect(
    canvas: (u32, u32),
    matrix: &[f64; 6],
    (x0, y0, x1, y1): CanvasRect,
) -> Result<CanvasRect, String> {
    validate(matrix)?;
    if x0 >= x1 || y0 >= y1 {
        return Ok((0, 0, 0, 0));
    }
    let map = source_map(matrix);
    let at = |i: i64, j: i64| {
        let (nu, nv, _) = map.row_start(j as u32);
        let i = i as f64;
        (nu + map.step.0 * i, nv + map.step.1 * i)
    };
    let (mut umin, mut umax, mut vmin, mut vmax) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for (i, j) in [(x0, y0), (x1 - 1, y0), (x0, y1 - 1), (x1 - 1, y1 - 1)] {
        let (u, v) = at(i, j);
        umin = umin.min(u);
        umax = umax.max(u);
        vmin = vmin.min(v);
        vmax = vmax.max(v);
    }
    // The walk starts every row at column 0, so its sums span columns
    // 0 … x1 − 1 (an affine map's extremes over them are at the corners).
    let mut magnitude: f64 = 0.0;
    for (i, j) in [(0, y0), (x1 - 1, y0), (0, y1 - 1), (x1 - 1, y1 - 1)] {
        let (u, v) = at(i, j);
        magnitude = magnitude.max(u.abs()).max(v.abs());
    }
    let drift = f64::from(canvas.0) * magnitude * f64::EPSILON;
    let margin = 2i64.max((drift.ceil() as i64).saturating_add(1));
    let (cw, ch) = (i64::from(canvas.0), i64::from(canvas.1));
    let low = |v: f64| ((v - 0.5).floor() as i64).saturating_sub(margin);
    let high = |v: f64| {
        ((v - 0.5).floor() as i64)
            .saturating_add(margin)
            .saturating_add(1)
    };
    let rect = (
        low(umin).clamp(0, cw),
        low(vmin).clamp(0, ch),
        high(umax).clamp(0, cw),
        high(vmax).clamp(0, ch),
    );
    Ok(if rect.0 >= rect.2 || rect.1 >= rect.3 {
        (0, 0, 0, 0)
    } else {
        rect
    })
}

/// [`transform`] over `frame` only (13B-9a): `taps` is row-major over the
/// canvas rect `taps_at` (every tap the frame reads, [`tap_rect`]); the
/// output is row-major over `frame`, each sample the whole-canvas walk's to
/// the bit. [`transform`] stays the master.
pub(crate) fn transform_rect<T: Unorm>(
    taps: &[[T; 4]],
    taps_at: CanvasRect,
    canvas: (u32, u32),
    matrix: &[f64; 6],
    frame: CanvasRect,
    job: JobContext<'_>,
) -> Result<Vec<[T; 4]>, String> {
    resampled_rect(
        taps,
        taps_at,
        canvas,
        matrix,
        frame,
        [T::default(); 4],
        |acc, p, w| sampling::accumulate_unorm(acc, &p, w),
        sampling::straight_unorm,
        job,
    )
}

/// [`transform_extended`] over `frame` only: [`transform_rect`]'s rules
/// with the extended pair. [`transform_extended`] stays the master.
pub(crate) fn transform_rect_extended(
    taps: &[[f32; 4]],
    taps_at: CanvasRect,
    canvas: (u32, u32),
    matrix: &[f64; 6],
    frame: CanvasRect,
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    let range = extended_range();
    resampled_rect(
        taps,
        taps_at,
        canvas,
        matrix,
        frame,
        [0.0; 4],
        |acc, p, w| sampling::accumulate_extended(acc, &p, w),
        |acc| sampling::straight_extended(acc, &range),
        job,
    )
}

/// The rect body of [`transform_rect`] and [`transform_rect_extended`]:
/// [`resampled`]'s walk (`sampling::resample_rows_into`, the same map, kernel
/// and pair) over the frame's ROWS only. Each row is walked whole from
/// column 0, as the whole-canvas walk walks it, because the walk advances
/// its source position by a running sum (`nu += step`): a row started at the
/// frame's first column would sum differently for a non-integer matrix.
/// Writes outside the frame's columns are dropped and reads outside
/// `taps_at` are `blank` (none happens: `taps_at` holds every tap). Bytes
/// O(frame + taps), time O(frame rows × canvas width).
#[allow(clippy::too_many_arguments)]
fn resampled_rect<P: Copy>(
    taps: &[P],
    (tx0, ty0, tx1, ty1): CanvasRect,
    canvas: (u32, u32),
    matrix: &[f64; 6],
    (x0, y0, x1, y1): CanvasRect,
    blank: P,
    accumulate: impl Fn(&mut [f32; 4], P, f32),
    finish: impl Fn([f32; 4]) -> P,
    job: JobContext<'_>,
) -> Result<Vec<P>, String> {
    validate(matrix)?;
    let span = |a: i64, b: i64| usize::try_from(b - a).unwrap_or(0);
    let tw = span(tx0, tx1);
    if taps.len() != tw * span(ty0, ty1) {
        return Err("retouch source extent mismatch".into());
    }
    let (fw, fh) = (span(x0, x1), span(y0, y1));
    let mut output = crate::precision_image::reserved_vec(fw * fh)?;
    output.resize(fw * fh, blank);
    if fw == 0 || fh == 0 {
        return Ok(output);
    }
    let (Ok(top), Ok(bottom)) = (u32::try_from(y0), u32::try_from(y1)) else {
        return Err("retouch frame lies outside the canvas".into());
    };
    let stride = canvas.0 as usize;
    sampling::resample_rows_into(
        canvas,
        canvas,
        top..bottom,
        &source_map(matrix),
        Sampler::Kernel(Kernel::Bilinear),
        |si| {
            let (sx, sy) = ((si % stride) as i64, (si / stride) as i64);
            if sx >= tx0 && sx < tx1 && sy >= ty0 && sy < ty1 {
                taps[(sy - ty0) as usize * tw + (sx - tx0) as usize]
            } else {
                blank
            }
        },
        accumulate,
        finish,
        |di, p| {
            let (dx, dy) = ((di % stride) as i64, (di / stride) as i64);
            if dx >= x0 && dx < x1 {
                output[(dy - y0) as usize * fw + (dx - x0) as usize] = p;
            }
        },
        false,
        true,
        job,
    )?;
    Ok(output)
}

/// The one resample body of [`transform`] and [`transform_extended`]: the
/// source plate mapped through `matrix` with bilinear taps; `accumulate` and
/// `finish` are the sample type's premultiplied pair and `blank` its
/// transparent pixel.
fn resampled<P: Copy>(
    source: &[P],
    dimensions: (u32, u32),
    matrix: &[f64; 6],
    blank: P,
    accumulate: impl Fn(&mut [f32; 4], P, f32),
    finish: impl Fn([f32; 4]) -> P,
    job: JobContext<'_>,
) -> Result<Vec<P>, String> {
    validate(matrix)?;
    let n = crate::precision_image::pixel_count(dimensions.0, dimensions.1)?;
    if source.len() != n {
        return Err("retouch source extent mismatch".into());
    }
    let mut output = crate::precision_image::reserved_vec(n)?;
    output.resize(n, blank);
    let map = source_map(matrix);
    sampling::resample_into(
        dimensions,
        dimensions,
        &map,
        Sampler::Kernel(Kernel::Bilinear),
        |i| source[i],
        accumulate,
        finish,
        |i, p| output[i] = p,
        false,
        true,
        job,
    )?;
    Ok(output)
}

/// Explicit storage expansion preserves every old sample and independently
/// placed mask. It never changes the document canvas or moves existing content.
pub(crate) type StorageExtent = ((u32, u32), (i32, i32));

pub(crate) fn expanded_extent(
    canvas: (u32, u32),
    dimensions: (u32, u32),
    offset: (i32, i32),
) -> Result<StorageExtent, String> {
    merged_extent((dimensions, offset), (canvas, (0, 0)))
}

/// Shared checked union, also used by native paint's exact changed-ink bounds.
pub(crate) fn merged_extent(a: StorageExtent, b: StorageExtent) -> Result<StorageExtent, String> {
    let (dimensions, offset) = a;
    let (other, origin) = b;
    let left = i64::from(offset.0).min(i64::from(origin.0));
    let top = i64::from(offset.1).min(i64::from(origin.1));
    let right = (i64::from(offset.0) + i64::from(dimensions.0))
        .max(i64::from(origin.0) + i64::from(other.0));
    let bottom = (i64::from(offset.1) + i64::from(dimensions.1))
        .max(i64::from(origin.1) + i64::from(other.1));
    let width = u32::try_from(right - left).map_err(|_| "retouch expansion width overflow")?;
    let height = u32::try_from(bottom - top).map_err(|_| "retouch expansion height overflow")?;
    crate::precision_image::pixel_count(width, height)?;
    Ok(((width, height), (left as i32, top as i32)))
}

/// The layer's samples copied into `extent` (`((w, h), (x, y))`, which must
/// contain the layer's own rect), transparent everywhere else: the row-segment
/// copy the explicit storage growth makes. `LayerRaster::unseeded`, so no
/// view is seeded. `job` is polled once per row.
pub(crate) fn expanded_raster(
    layer: &Layer,
    extent: StorageExtent,
    job: JobContext<'_>,
) -> Result<LayerRaster, String> {
    let (dimensions, offset) = extent;
    let (lw, lh) = layer.pixels.dimensions();
    let dx = i64::from(layer.offset.0) - i64::from(offset.0);
    let dy = i64::from(layer.offset.1) - i64::from(offset.1);
    if dx < 0
        || dy < 0
        || dx + i64::from(lw) > i64::from(dimensions.0)
        || dy + i64::from(lh) > i64::from(dimensions.1)
    {
        return Err("the grown extent must contain the layer".into());
    }
    let (dx, dy) = (dx as u32, dy as u32);
    let mut pixels = image::RgbaImage::new(dimensions.0, dimensions.1);
    let stride = dimensions.0 as usize * 4;
    let target: &mut [u8] = &mut pixels;
    for y in 0..lh {
        job.check()?;
        let row = (y + dy) as usize * stride;
        for (x, run) in layer.pixels.row_segments(y, 0, lw) {
            let at = row + (x + dx) as usize * 4;
            target[at..at + run.len() * 4].copy_from_slice(run.as_flattened());
        }
    }
    Ok(LayerRaster::unseeded(pixels))
}

/// `doc` with layer `index` grown to `extent` through [`expanded_raster`] —
/// the one storage growth both the retouch expansion and Edit ▸ Stroke…'s
/// layer edge use. A growth moves the buffer's ORIGIN without moving its
/// content, so every comp entry bound to that layer that recorded an offset
/// (an absolute buffer origin, `doc_comps`) shifts by the same delta through
/// `comps_after_growth` (`comps_geometry`, over `remapped_comps`), the one
/// place every growth site writes that shift; otherwise the next Apply would
/// misplace the layer.
pub(crate) fn grown_layer(
    doc: &DrDocument,
    index: usize,
    extent: StorageExtent,
    job: JobContext<'_>,
) -> Result<DrDocument, String> {
    let layer = doc
        .raster_layer(index)
        .ok_or("storage growth requires a raster layer")?;
    let pixels = expanded_raster(layer, extent, job)?;
    let mut out = doc.clone();
    if let Some(comps) = doc.comps_after_growth(index, extent.1) {
        out.comps = comps;
    }
    out.layers[index].pixels = pixels;
    out.layers[index].offset = extent.1;
    Ok(out)
}
