//! Retouch source mapping shared by native byte/u16/f32 sampling and source ghosts.
//! The matrix maps destination canvas pixel centres to source canvas centres.
//!
//! Source transforms run in native depth before any ghost display reduction.
use crate::doc::{DrDocument, Layer, LayerRaster};
use crate::doc_transform::{Affine, Sampler, SourceMap};
use crate::job_control::JobContext;
use crate::sampling::{self, ColorRange, Kernel, Unorm};

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
    let bound = crate::float_document::FLOAT_RANGE_MAX;
    let range = ColorRange {
        min: [-bound; 3],
        max: [bound; 3],
    };
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
    let [a, b, c, d, tx, ty] = *matrix;
    let map = SourceMap::new(&Affine { a, b, c, d, tx, ty }, (0, 0), (0, 0));
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
