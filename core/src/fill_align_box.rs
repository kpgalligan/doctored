//! KERNEL: a Gradient Fill layer's Align with Layer box, the ONE rule
//! the PSD import (`psd_raster_masks::MaskRecord::align_box`) and the host
//! (`doc_fill_align`, the `dr_*_fill_align_box` exports) both call.
//!
//! An aligned Gradient Fill layer's gradient spans the layer's bounds (so an
//! imported PSD's fill layer renders as it does in Photoshop), which for a
//! fill layer (no pixels of its own) are its mask's: the bounds of the mask's
//! coverage. So the box is the bounding box of the mask samples above zero,
//! moved by the mask's canvas offset and clipped to the canvas
//! ([`align_box`]). Every other case spans the canvas: no mask, a disabled
//! one (it shapes nothing), one whose outside coverage is not 0 (its
//! coverage reaches past its plane, to the canvas edge), one whose sample
//! count does not match its dimensions, and one with no coverage on the
//! canvas. The rule reads the RAW stored coverage only: a mask's density and
//! feather change how much of the fill shows, never the box (a feathered
//! mask's bounds would be a blur's, which no take has measured).
//!
//! Why the coverage bounds and not the stored rectangle: a mask Doctored
//! makes from a selection is canvas-sized, so its stored rectangle is the
//! canvas, while Photoshop's own tight rectangle of the same selection is the
//! selection's bounds; the coverage bounds are the same box for both, so an
//! imported mask and an in-app one align alike. A mask edit does not move an
//! already rendered fill: the box is read at the fill's next render.

use crate::layer_rect::nonzero_rect;

/// A mask's raw coverage as the rule reads it: `samples` row-major over
/// `dimensions`, its top-left at canvas `offset`, `outside` its raw outside
/// coverage (0 or 255) and `enabled` its enabled state.
pub(crate) struct MaskCoverage<'a, T> {
    pub samples: &'a [T],
    pub dimensions: (u32, u32),
    pub offset: (i32, i32),
    pub outside: u8,
    pub enabled: bool,
}

/// The Align with Layer box `[x, y, w, h]` in canvas pixels (module doc): the
/// bounds of `mask`'s samples above zero, moved by its offset and clipped to
/// `canvas`, when the mask is enabled, its outside coverage is 0 and its
/// sample count is its width × height; otherwise, or when that leaves
/// nothing, the canvas `[0, 0, canvas.0, canvas.1]`.
pub(crate) fn align_box<T: Copy + Default + PartialOrd>(
    canvas: (u32, u32),
    mask: Option<MaskCoverage<'_, T>>,
) -> [i32; 4] {
    let whole = [0, 0, clamp_i32(canvas.0), clamp_i32(canvas.1)];
    let Some(mask) = mask else {
        return whole;
    };
    let (w, h) = mask.dimensions;
    let count = u64::from(w) * u64::from(h);
    if !mask.enabled || mask.outside != 0 || mask.samples.len() as u64 != count {
        return whole;
    }
    let zero = T::default();
    let Some((x, y, bw, bh)) = nonzero_rect((w, h), |i| mask.samples[i] > zero) else {
        return whole;
    };
    // In i64: a mask offset near the i32 range plus a plane side cannot wrap.
    let left = i64::from(mask.offset.0) + i64::from(x);
    let top = i64::from(mask.offset.1) + i64::from(y);
    let (right, bottom) = (left + i64::from(bw), top + i64::from(bh));
    let (x0, y0) = (left.max(0), top.max(0));
    let (x1, y1) = (
        right.min(i64::from(canvas.0)),
        bottom.min(i64::from(canvas.1)),
    );
    if x1 <= x0 || y1 <= y0 {
        return whole;
    }
    // Every value now lies within the canvas, which is at most `u32`; the
    // core's canvases are far inside `i32`.
    [x0, y0, x1 - x0, y1 - y0].map(|v| i32::try_from(v).unwrap_or(i32::MAX))
}

fn clamp_i32(v: u32) -> i32 {
    i32::try_from(v).unwrap_or(i32::MAX)
}
