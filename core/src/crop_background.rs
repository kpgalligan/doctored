//! Crop past the canvas: the window rule and the Background
//! pad, as free functions over rectangles and sample slices. A kernel: it
//! names no document type; `doc_crop_extend` is its byte and typed adapter.
//!
//! Every rectangle is `(x, y, width, height)` in CANVAS space. A straighten
//! window may lie anywhere [`window_fits`] admits: its edges inside the
//! signed 32-bit range (so `-x` never wraps), its size inside the pixel
//! limit, and the saved channels inside their budget at the window's size.
//!
//! The Background (Default) fill pads the qualifying record 0 to
//! [`pad_rect`] with the background colour BEFORE the op runs, so the turned
//! crop resamples a plane that already holds the colour where the old canvas
//! had none: its corners are the colour exactly and its edge is the filter
//! over the padded plane, as measured in Photoshop (take1's R5 corners are
//! the colour).

use crate::doc_transform::{Affine, EXACT_EPSILON};
use crate::precision_geometry::{
    CROP_CHANNELS_DO_NOT_FIT, CROP_PAD_TOO_LARGE, CROP_WINDOW_OUT_OF_RANGE,
};

/// How far past the inverse-mapped window the pad reaches on a turned crop.
/// Lanczos3, the widest geometry filter, reads 3 source pixels either side
/// of a sample point, and the point itself can fall up to one pixel past
/// the window's mapped edge after the outward floor/ceil of its corners; 3 +
/// 1 = 4 makes every tap of every destination pixel read padded colour
/// rather than transparent outside, which is what keeps a turned crop's new
/// corners exactly the colour. An identity crop resamples nothing and
/// reaches nothing.
pub(crate) const PAD_REACH: i64 = 4;

/// Whether a straighten window `(x, y, w, h)` with `channels` saved channels
/// may be cropped to: [`CROP_WINDOW_OUT_OF_RANGE`] when `x` or `y` is
/// `i32::MIN` or a far edge passes `i32::MAX`, then the pixel limit
/// (`pixel_count`'s own sentence, a zero size included), then
/// [`CROP_CHANNELS_DO_NOT_FIT`] when the channels break their budget at the
/// window's size.
pub(crate) fn window_fits(x: i32, y: i32, w: u32, h: u32, channels: usize) -> Result<(), String> {
    let max = i64::from(i32::MAX);
    if x == i32::MIN
        || y == i32::MIN
        || i64::from(x) + i64::from(w) > max
        || i64::from(y) + i64::from(h) > max
    {
        return Err(CROP_WINDOW_OUT_OF_RANGE.into());
    }
    crate::precision_image::pixel_count(w, h)?;
    if !crate::doc_channel::channels_fit_at(w, h, channels) {
        return Err(CROP_CHANNELS_DO_NOT_FIT.into());
    }
    Ok(())
}

/// The rectangle a qualifying Background is padded to before a straighten
/// whose `matrix` turns the canvas and whose `window` becomes the new
/// canvas: the bounding box of `extent` and the window mapped back through
/// `matrix`'s inverse (floored and ceiled outward), that mapped part grown by
/// [`PAD_REACH`] unless `matrix` is the identity. `Ok(None)` when it lies
/// inside `extent` (nothing to pad). [`CROP_PAD_TOO_LARGE`] when its size
/// fails the pixel limit or its origin or far edge leaves `i32`; a singular
/// matrix answers the typed op's own sentence.
pub(crate) fn pad_rect(
    extent: (i64, i64, u32, u32),
    matrix: [f64; 6],
    window: (i64, i64, u32, u32),
) -> Result<Option<(i64, i64, u32, u32)>, String> {
    let identity = matrix
        .iter()
        .zip([1., 0., 0., 1., 0., 0.])
        .all(|(a, b)| (*a - b).abs() <= EXACT_EPSILON);
    let (wx, wy) = (window.0 as f64, window.1 as f64);
    let (ww, wh) = (f64::from(window.2), f64::from(window.3));
    let reach = if identity { 0 } else { PAD_REACH };
    let (mut left, mut top, mut right, mut bottom) = if identity {
        (
            window.0,
            window.1,
            window.0 + i64::from(window.2),
            window.1 + i64::from(window.3),
        )
    } else {
        let inverse = Affine::from_array(matrix)
            .inverse()
            .ok_or("typed affine matrix is singular")?;
        let corners = [
            inverse.apply(wx, wy),
            inverse.apply(wx + ww, wy),
            inverse.apply(wx, wy + wh),
            inverse.apply(wx + ww, wy + wh),
        ];
        let min_x = corners.iter().map(|c| c.0).fold(f64::INFINITY, f64::min);
        let min_y = corners.iter().map(|c| c.1).fold(f64::INFINITY, f64::min);
        let max_x = corners
            .iter()
            .map(|c| c.0)
            .fold(f64::NEG_INFINITY, f64::max);
        let max_y = corners
            .iter()
            .map(|c| c.1)
            .fold(f64::NEG_INFINITY, f64::max);
        // A far-off mapped corner is refused below by the i32 check; the
        // clamp only keeps the f64 → i64 casts meaningful.
        let bound = 1e15;
        (
            min_x.floor().clamp(-bound, bound) as i64,
            min_y.floor().clamp(-bound, bound) as i64,
            max_x.ceil().clamp(-bound, bound) as i64,
            max_y.ceil().clamp(-bound, bound) as i64,
        )
    };
    left -= reach;
    top -= reach;
    right += reach;
    bottom += reach;
    let (ex, ey) = (extent.0, extent.1);
    let (er, eb) = (ex + i64::from(extent.2), ey + i64::from(extent.3));
    left = left.min(ex);
    top = top.min(ey);
    right = right.max(er);
    bottom = bottom.max(eb);
    if (left, top, right, bottom) == (ex, ey, er, eb) {
        return Ok(None);
    }
    let range = i64::from(i32::MIN)..=i64::from(i32::MAX);
    if ![left, top, right, bottom].iter().all(|v| range.contains(v)) {
        return Err(CROP_PAD_TOO_LARGE.into());
    }
    let (w, h) = (
        u32::try_from(right - left).map_err(|_| CROP_PAD_TOO_LARGE)?,
        u32::try_from(bottom - top).map_err(|_| CROP_PAD_TOO_LARGE)?,
    );
    crate::precision_image::pixel_count(w, h).map_err(|_| CROP_PAD_TOO_LARGE)?;
    Ok(Some((left, top, w, h)))
}

/// `src` (row-major, `extent.2 × extent.3`) placed at its offset inside
/// `to`, with `fill` everywhere else in `to`. `to` must contain `extent`.
pub(crate) fn padded<P: Copy>(
    src: &[P],
    extent: (i64, i64, u32, u32),
    to: (i64, i64, u32, u32),
    fill: P,
) -> Result<Vec<P>, String> {
    let (sw, sh) = (extent.2 as usize, extent.3 as usize);
    let (tw, th) = (to.2 as usize, to.3 as usize);
    let (dx, dy) = (extent.0 - to.0, extent.1 - to.1);
    if src.len() != sw * sh || dx < 0 || dy < 0 || dx as usize + sw > tw || dy as usize + sh > th {
        return Err("the background pad must contain the layer".into());
    }
    let count = tw.checked_mul(th).ok_or(CROP_PAD_TOO_LARGE)?;
    let mut out = crate::precision_image::reserved_vec(count)?;
    out.resize(count, fill);
    let (dx, dy) = (dx as usize, dy as usize);
    for row in 0..sh {
        let start = (dy + row) * tw + dx;
        out[start..start + sw].copy_from_slice(&src[row * sw..(row + 1) * sw]);
    }
    Ok(out)
}
