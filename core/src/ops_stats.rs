//! Image STATISTICS: the histogram behind the Info panel and the Levels /
//! Curves plots, and the block sample behind the eyedropper. Pure reads —
//! nothing here produces an image.
//!
//! # The one counting rule
//!
//! A pixel counts when its **alpha is non-zero** and, when a coverage mask
//! is given, its **coverage is at least 128**. Both halves matter:
//!
//! * every alpha-0 pixel of an `RgbaImage` is `(0, 0, 0, 0)`, so on any
//!   layer with transparency — a cut-out subject, anything padded to the
//!   canvas by a canvas-space layer image — an alpha-blind scan would put a
//!   huge spike in bin 0 of all three channels, pin the black point at 0 and
//!   drag every mean toward zero;
//! * binarizing coverage at 128 is the existing 50 %-contour rule this crate
//!   already uses for mask morphology (`doc_select`), so a feathered
//!   selection means the same thing to the histogram as it does to a grow or
//!   a shrink.
//!
//! [`counted`] is that rule, written once; `ops_auto` calls it too, so
//! "the image's own histogram" has exactly one definition in this build.

use image::{Rgba, RgbaImage};

use crate::blend::{LUMA_B, LUMA_G, LUMA_R};
use crate::selection_tools::SelectionSample;

/// Whether pixel `i` (row-major) takes part in a statistic — the module
/// doc's one counting rule. `mask` is a coverage buffer of exactly
/// width*height bytes; a short one (which the FFI's length derivation makes
/// impossible) reads as "not covered" rather than panicking.
pub(crate) fn counted(px: &Rgba<u8>, mask: Option<&[u8]>, i: usize) -> bool {
    if px[3] == 0 {
        return false;
    }
    match mask {
        None => true,
        Some(m) => m.get(i).copied().unwrap_or(0) >= 128,
    }
}

/// 4 x 256 counts — red, then green, then blue, then Rec. 709 luma
/// (`bin = round(luma * 255)`) — plus the number of pixels counted.
///
/// `stride` counts every `stride`-th pixel in row-major order, with 0 and 1
/// both meaning every pixel, and the returned total is how many were
/// ACTUALLY counted, so every proportion and every clipping count stays
/// comparable at any stride. Only the live panel passes more than 1.
pub(crate) fn histogram(img: &RgbaImage, mask: Option<&[u8]>, stride: u32) -> ([u32; 1024], u64) {
    let step = stride.max(1) as usize;
    let mut bins = [0u32; 1024];
    let mut total = 0u64;
    let raw = img.as_raw();
    let count = (img.width() as usize) * (img.height() as usize);
    let mut i = 0;
    while i < count {
        let px = Rgba([raw[i * 4], raw[i * 4 + 1], raw[i * 4 + 2], raw[i * 4 + 3]]);
        if counted(&px, mask, i) {
            bins[usize::from(px[0])] += 1;
            bins[256 + usize::from(px[1])] += 1;
            bins[512 + usize::from(px[2])] += 1;
            let luma =
                LUMA_R * f32::from(px[0]) + LUMA_G * f32::from(px[1]) + LUMA_B * f32::from(px[2]);
            bins[768 + (luma.round().max(0.0) as usize).min(255)] += 1;
            total += 1;
        }
        i += step;
    }
    (bins, total)
}

/// The straight RGBA at (x, y): the plain mean of the (2*reach + 1) square
/// about it with out-of-bounds pixels DROPPED, `reach` 0/1/2 = point / 3x3 /
/// 5x5. Integer division, truncating — byte for byte what the eyedropper
/// this replaces has always done. The caller of [`block_mean`], which owns
/// the arithmetic; this keeps the eyedropper's 1/3/5 limit.
///
/// The CENTRE need not be inside the image: the eyedropper hands over an
/// unclamped point and takes whatever falls inside, so a 5x5 sample at
/// x = -1 is the mean of the two in-bounds columns. `None` only when NO
/// pixel of the block is inside, or `reach` is above 2.
pub(crate) fn sample_mean(img: &RgbaImage, x: i32, y: i32, reach: u32) -> Option<[u8; 4]> {
    if reach > 2 {
        return None;
    }
    let raw = img.as_raw();
    block_mean(
        img.dimensions(),
        |i| [raw[i * 4], raw[i * 4 + 1], raw[i * 4 + 2], raw[i * 4 + 3]],
        i64::from(x),
        i64::from(y),
        reach,
    )
}

/// THE box mean: per channel, the truncating integer mean of the
/// `(2*reach + 1)` square about (x, y) over the pixels of a `dimensions`
/// plane that lie inside it (out-of-bounds pixels are dropped, never
/// replicated). `pixel` answers the row-major sample `i`. The sums are u64,
/// so a 101x101 box of 16-bit codes cannot overflow. `None` when no pixel of
/// the box is inside. [`sample_mean`] (the eyedropper and the samplers) and
/// the Magic Wand's Sample Size (`selection_region::grow`) are its callers.
pub(crate) fn block_mean<T: SelectionSample>(
    dimensions: (u32, u32),
    pixel: impl Fn(usize) -> [T; 4],
    x: i64,
    y: i64,
    reach: u32,
) -> Option<[T; 4]> {
    let (w, h) = (i64::from(dimensions.0), i64::from(dimensions.1));
    let r = i64::from(reach);
    let (x0, x1) = ((x - r).max(0), (x + r).min(w - 1));
    let (y0, y1) = ((y - r).max(0), (y + r).min(h - 1));
    if x0 > x1 || y0 > y1 {
        return None;
    }
    let mut sum = [0u64; 4];
    for py in y0..=y1 {
        for px in x0..=x1 {
            let sample = pixel((py * w + px) as usize);
            for (slot, v) in sum.iter_mut().zip(sample) {
                *slot += v.code() as u64;
            }
        }
    }
    let count = ((x1 - x0 + 1) * (y1 - y0 + 1)) as u64;
    Some(sum.map(|v| T::quantize((v / count) as f32)))
}
