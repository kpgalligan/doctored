//! Image ▸ Trim's kept rectangle: the one bounding-box scan behind
//! `dr_image_trim_rect` (8-bit composite) and `dr_precision_image_trim_rect`
//! (16-bit and float projection). A kernel: it names no document type and
//! reads samples only through the caller's two closures.
//!
//! A pixel is TRIMMABLE when, for basis
//! * [`TRIM_TRANSPARENT`] — its alpha sample is exactly zero;
//! * [`TRIM_TOP_LEFT`] / [`TRIM_BOTTOM_RIGHT`] — all of its samples (four, or a
//!   CMYK composite's five, `cmyk_trim`) equal that corner pixel's exactly. Exact, not within a tolerance: Trim removes
//!   a border of one colour, and a tolerance would eat into soft content. The
//!   float adapter compares bit patterns (it hands this module `u32` bits), so
//!   `-0.0` and `0.0` differ there as they do on disk.
//!
//! The kept rectangle is the bounding box of the non-trimmable pixels, with
//! every edge the caller did not check left where the canvas has it.

/// Basis: fully transparent pixels.
pub const TRIM_TRANSPARENT: u32 = 0;
/// Basis: pixels equal to the top-left pixel.
pub const TRIM_TOP_LEFT: u32 = 1;
/// Basis: pixels equal to the bottom-right pixel.
pub const TRIM_BOTTOM_RIGHT: u32 = 2;

/// Edge bits of the `edges` mask.
pub const TRIM_TOP: u32 = 1;
pub const TRIM_BOTTOM: u32 = 2;
pub const TRIM_LEFT: u32 = 4;
pub const TRIM_RIGHT: u32 = 8;

/// The kept rectangle `(x, y, width, height)` of a `w`×`h` row-major image.
/// `alpha_zero(i)` answers whether pixel `i`'s alpha sample is zero and
/// `pixel(i)` gives its samples (`P`: four RGBA samples, or a CMYK
/// document's five ink and alpha codes, #547 phase 4b). `None` for an empty image, an unknown
/// `basis`, or an image whose every pixel is trimmable (there is nothing to
/// keep). Edges whose bit is clear in `edges` never move; a result equal to
/// the whole image is still `Some` — the host decides that is no change.
pub fn trim_rect<P: Copy + PartialEq>(
    w: u32,
    h: u32,
    alpha_zero: impl Fn(usize) -> bool,
    pixel: impl Fn(usize) -> P,
    basis: u32,
    edges: u32,
) -> Option<(u32, u32, u32, u32)> {
    if w == 0 || h == 0 {
        return None;
    }
    let (wu, hu) = (w as usize, h as usize);
    let corner = match basis {
        TRIM_TRANSPARENT => None,
        TRIM_TOP_LEFT => Some(pixel(0)),
        TRIM_BOTTOM_RIGHT => Some(pixel(wu * hu - 1)),
        _ => return None,
    };
    let keeps = |i: usize| match corner {
        None => !alpha_zero(i),
        Some(c) => pixel(i) != c,
    };
    // One pass: the bounding box of every kept pixel.
    let mut min = (usize::MAX, usize::MAX);
    let mut max = (0usize, 0usize);
    for y in 0..hu {
        let row = y * wu;
        // The first and last kept pixel of the row bound it horizontally;
        // scanning inward from both ends stops early on wide clear margins.
        let Some(first) = (0..wu).find(|&x| keeps(row + x)) else {
            continue;
        };
        let last = (first..wu).rev().find(|&x| keeps(row + x)).unwrap_or(first);
        min = (min.0.min(first), min.1.min(y));
        max = (max.0.max(last), max.1.max(y));
    }
    if min.0 == usize::MAX {
        return None;
    }
    let left = if edges & TRIM_LEFT != 0 { min.0 } else { 0 };
    let top = if edges & TRIM_TOP != 0 { min.1 } else { 0 };
    let right = if edges & TRIM_RIGHT != 0 {
        max.0
    } else {
        wu - 1
    };
    let bottom = if edges & TRIM_BOTTOM != 0 {
        max.1
    } else {
        hu - 1
    };
    Some((
        left as u32,
        top as u32,
        (right - left + 1) as u32,
        (bottom - top + 1) as u32,
    ))
}
