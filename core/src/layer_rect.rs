//! Layer-space rects for the byte and typed rect writers (13B-2c-2a,
//! 13B-3d-1).
//!
//! A writer whose edit has a natural bound — a stroke's nonzero overlay, a
//! selection's coverage, a region grown from a seed, a heal's windows, a
//! red-eye rect — reads only that rect (`LayerRaster::read_rect`), runs its
//! existing per-pixel body over the rect buffer with rect-relative indices and
//! writes it back through `LayerRaster::with_rect`, so a small edit on a large
//! layer neither copies the whole plane nor seeds a whole-plane view. The rect
//! bounds the READ; `with_rect` shares every tile whose bytes the walk left
//! bitwise unchanged, so the diff bounds the copies.
//!
//! Every rect here is LAYER-space `(x, y, w, h)` with `w, h > 0`; `None` means
//! empty. The rect arithmetic is allocation-free; [`rect_image`] is the one
//! copy (the rect's own samples). The typed paint writers (13B-3d-1) take the
//! PLANE forms (`plane_clip`, `canvas_rect_on_plane`, `coverage_rect16`,
//! `plane_rect_offset`: a plane's dimensions and canvas offset instead of a
//! byte `Layer`) over `PrecisionImage::read_rect_rgba16/32` +
//! `with_rect_rgba16/32`; the byte forms delegate to them.
//!
//! The heal's and the byte inpaint's union reads go through
//! [`rect_image_with_job`], which polls once per visited tile.

use image::RgbaImage;

use crate::doc::Layer;
use crate::job_control::JobContext;
use crate::layer_raster::LayerRaster;

/// The layer's canvas intersection, in layer space: the clip
/// `doc_paint::source_rect_in` makes (the master copy of this clip, which the
/// paint walk and `overlay_rect` use), in i64 so an extreme offset cannot wrap. `None` when
/// the layer misses the canvas. [`plane_clip`] over the layer's plane.
pub(crate) fn canvas_clip(canvas: (u32, u32), layer: &Layer) -> Option<(u32, u32, u32, u32)> {
    plane_clip(canvas, layer.pixels.dimensions(), layer.offset)
}

/// The canvas intersection of a `dimensions` plane standing at canvas
/// `offset`, in plane space (the ONE implementation of [`canvas_clip`], so
/// the typed writers share it). `None` when the plane misses the canvas.
pub(crate) fn plane_clip(
    canvas: (u32, u32),
    (lw, lh): (u32, u32),
    offset: (i32, i32),
) -> Option<(u32, u32, u32, u32)> {
    let (off_x, off_y) = (i64::from(offset.0), i64::from(offset.1));
    let lx0 = (-off_x).max(0);
    let ly0 = (-off_y).max(0);
    let lx1 = (i64::from(canvas.0) - off_x).min(i64::from(lw));
    let ly1 = (i64::from(canvas.1) - off_y).min(i64::from(lh));
    if lx0 >= lx1 || ly0 >= ly1 {
        return None;
    }
    Some((
        lx0 as u32,
        ly0 as u32,
        (lx1 - lx0) as u32,
        (ly1 - ly0) as u32,
    ))
}

/// The canvas rect `(x0, y0, x1, y1)` (exclusive ends) intersected with
/// [`canvas_clip`], in layer space. `None` when the intersection is empty.
pub(crate) fn canvas_rect_on_layer(
    canvas: (u32, u32),
    layer: &Layer,
    rect: (i64, i64, i64, i64),
) -> Option<(u32, u32, u32, u32)> {
    canvas_rect_on_plane(canvas, layer.pixels.dimensions(), layer.offset, rect)
}

/// [`canvas_rect_on_layer`] over a `dimensions` plane at `offset`: the
/// canvas rect (exclusive ends) intersected with [`plane_clip`], in plane
/// space.
pub(crate) fn canvas_rect_on_plane(
    canvas: (u32, u32),
    dimensions: (u32, u32),
    offset: (i32, i32),
    (x0, y0, x1, y1): (i64, i64, i64, i64),
) -> Option<(u32, u32, u32, u32)> {
    let (cx, cy, cw, ch) = plane_clip(canvas, dimensions, offset)?;
    let (off_x, off_y) = (i64::from(offset.0), i64::from(offset.1));
    let lx0 = x0.saturating_sub(off_x).max(i64::from(cx));
    let ly0 = y0.saturating_sub(off_y).max(i64::from(cy));
    let lx1 = x1.saturating_sub(off_x).min(i64::from(cx) + i64::from(cw));
    let ly1 = y1.saturating_sub(off_y).min(i64::from(cy) + i64::from(ch));
    if lx0 >= lx1 || ly0 >= ly1 {
        return None;
    }
    Some((
        lx0 as u32,
        ly0 as u32,
        (lx1 - lx0) as u32,
        (ly1 - ly0) as u32,
    ))
}

/// The bounding box, in layer space, of the canvas-frame u8 samples of
/// `mask` above 0 that fall inside [`canvas_clip`]. `None` when none reaches
/// the layer.
pub(crate) fn coverage_rect(
    mask: &[u8],
    canvas: (u32, u32),
    layer: &Layer,
) -> Option<(u32, u32, u32, u32)> {
    coverage_bounds(mask, canvas, layer.pixels.dimensions(), layer.offset)
}

/// The u16 twin of [`coverage_rect`] over a `dimensions` plane at `offset`:
/// the bounding box, in plane space, of the canvas-frame samples above 0
/// inside [`plane_clip`]. With `dimensions = canvas, offset = (0, 0)` the
/// answer is canvas-space.
pub(crate) fn coverage_rect16(
    coverage: &[u16],
    canvas: (u32, u32),
    dimensions: (u32, u32),
    offset: (i32, i32),
) -> Option<(u32, u32, u32, u32)> {
    coverage_bounds(coverage, canvas, dimensions, offset)
}

/// The ONE row scan behind [`coverage_rect`] and [`coverage_rect16`]: the
/// first and last sample above zero of each clipped canvas row.
fn coverage_bounds<T: Copy + Default + PartialOrd>(
    mask: &[T],
    canvas: (u32, u32),
    dimensions: (u32, u32),
    offset: (i32, i32),
) -> Option<(u32, u32, u32, u32)> {
    let (x, y, w, h) = plane_clip(canvas, dimensions, offset)?;
    let (off_x, off_y) = (i64::from(offset.0), i64::from(offset.1));
    let cx0 = (i64::from(x) + off_x) as usize;
    let cx1 = cx0 + w as usize;
    let zero = T::default();
    let mut bounds: Option<(usize, usize, u32, u32)> = None;
    for ly in y..y + h {
        let at = (i64::from(ly) + off_y) as usize * canvas.0 as usize;
        let row = &mask[at + cx0..at + cx1];
        let Some(first) = row.iter().position(|c| *c > zero) else {
            continue;
        };
        let last = row.iter().rposition(|c| *c > zero).unwrap_or(first);
        bounds = Some(match bounds {
            None => (first, last, ly, ly),
            Some((a, b, top, _)) => (a.min(first), b.max(last), top, ly),
        });
    }
    let (first, last, top, bottom) = bounds?;
    Some((
        x + first as u32,
        top,
        (last - first + 1) as u32,
        bottom - top + 1,
    ))
}

/// The bounding box of the row-major `w × h` plane samples `set(i)` answers
/// true for. `None` when there is none.
pub(crate) fn nonzero_rect(
    (w, h): (u32, u32),
    set: impl Fn(usize) -> bool,
) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = (w as usize, h as usize);
    let mut bounds: Option<(usize, usize, usize, usize)> = None;
    for y in 0..h {
        let at = y * w;
        let Some(first) = (0..w).find(|x| set(at + x)) else {
            continue;
        };
        let last = (first..w).rev().find(|x| set(at + x)).unwrap_or(first);
        bounds = Some(match bounds {
            None => (first, last, y, y),
            Some((a, b, top, _)) => (a.min(first), b.max(last), top, y),
        });
    }
    let (first, last, top, bottom) = bounds?;
    Some((
        first as u32,
        top as u32,
        (last - first + 1) as u32,
        (bottom - top + 1) as u32,
    ))
}

/// The rect's samples as an image (`read_rect`'s buffer, flattened in place:
/// no second copy). `None` when the copy cannot be allocated.
pub(crate) fn rect_image(pixels: &LayerRaster, rect: (u32, u32, u32, u32)) -> Option<RgbaImage> {
    rect_image_with_job(pixels, rect, JobContext::NONE)
        .ok()
        .flatten()
}

/// [`rect_image`] polling `job` once per visited tile of the read
/// (`LayerRaster::read_rect_with_job`). `Ok(None)` is `rect_image`'s `None`.
pub(crate) fn rect_image_with_job(
    pixels: &LayerRaster,
    (x, y, w, h): (u32, u32, u32, u32),
    job: JobContext<'_>,
) -> Result<Option<RgbaImage>, String> {
    let Some(samples) = pixels.read_rect_with_job(x, y, w, h, job)? else {
        return Ok(None);
    };
    Ok(RgbaImage::from_raw(w, h, samples.into_flattened()))
}

/// The canvas position a rect image stands at, `offset + (x, y)`. It cannot
/// wrap: a rect inside [`canvas_clip`] has `offset + x` below the canvas
/// width.
pub(crate) fn rect_offset(layer: &Layer, rect: (u32, u32, u32, u32)) -> (i32, i32) {
    plane_rect_offset(layer.offset, rect)
}

/// [`rect_offset`] for a plane at `offset`: `offset + (x, y)`.
pub(crate) fn plane_rect_offset(
    offset: (i32, i32),
    (x, y, _, _): (u32, u32, u32, u32),
) -> (i32, i32) {
    (
        (i64::from(offset.0) + i64::from(x)) as i32,
        (i64::from(offset.1) + i64::from(y)) as i32,
    )
}
