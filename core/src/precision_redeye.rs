//! Red-eye correction on native source codes through the existing classifier.
//! Detection uses continuous RGB; the 1px binary component feather is coverage
//! only. Alpha and unselected/out-of-rectangle storage are never quantized.
//!
//! `precision_redeye.rs` shares the continuous classifier and cancellable pupil-component gate with
//! byte red-eye; no RGB preview enters correction.
//!
//! [`apply`] is generic over the sample ([`RedEyeSample`]): u16 codes at 16
//! bits, bit for bit the arithmetic it always had, and f32 at 32 bits. Red Eye
//! at 32 bits (`float_retouch`) runs on the rect's DISPLAY-ENCODED copy
//! (`display_encoding`): the classifier reads the encoded values clamped to
//! 0…1, so the red test means what it means at 16 bits, and the correction
//! (red toward the green/blue mean, then the darken) is applied to the
//! encoded values unclamped and decoded back, so a pupil above white keeps
//! its light ratio instead of being clipped.
use crate::job_control::JobContext;
use crate::precision_paint::canvas_index;

/// One stored sample of a red-eye plane: its code in caller units, the scale
/// of 1.0 in those units, how a corrected code is stored, and whether an
/// alpha sample is visible.
pub(crate) trait RedEyeSample: Copy {
    /// The code of 1.0 (65535 for u16, 1 for an encoded f32).
    const SCALE: f32;
    fn code(self) -> f32;
    fn store(v: f32) -> Self;
    fn visible(self) -> bool;
}

impl RedEyeSample for u16 {
    const SCALE: f32 = 65535.;
    fn code(self) -> f32 {
        f32::from(self)
    }
    fn store(v: f32) -> Self {
        v.clamp(0., 65535.).round() as u16
    }
    fn visible(self) -> bool {
        self > 0
    }
}

/// An encoded float sample: stored as computed (no clamp, no rounding).
impl RedEyeSample for f32 {
    const SCALE: f32 = 1.;
    fn code(self) -> f32 {
        self
    }
    fn store(v: f32) -> Self {
        v
    }
    fn visible(self) -> bool {
        self > 0.
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply<S: RedEyeSample>(
    canvas: (u32, u32),
    pixels: &mut [[S; 4]],
    dimensions: (u32, u32),
    offset: (i32, i32),
    selection: &[u16],
    rect: (i32, i32, u32, u32),
    pupil: f32,
    darken: f32,
    job: JobContext<'_>,
) -> Result<(), String> {
    let (x, y, w, h) = rect;
    let x0 = i64::from(x).max(0);
    let y0 = i64::from(y).max(0);
    let x1 = (i64::from(x) + i64::from(w)).min(i64::from(canvas.0));
    let y1 = (i64::from(y) + i64::from(h)).min(i64::from(canvas.1));
    if x0 >= x1 || y0 >= y1 {
        return Ok(());
    }
    let (rw, rh) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let index = |i: usize| {
        canvas_index(
            x0 + (i % rw) as i64 - i64::from(offset.0),
            y0 + (i / rw) as i64 - i64::from(offset.1),
            dimensions.0,
            dimensions.1,
        )
    };
    // The classifier reads 0…1: a u16 code over 65535 is already inside it,
    // so the clamp changes nothing there.
    let unit = |v: S| (v.code() / S::SCALE).clamp(0., 1.);
    let mut coverage = vec![0.; rw * rh];
    for (i, c) in coverage.iter_mut().enumerate() {
        job.every(i)?;
        if let Some(p) = index(i).map(|j| pixels[j]).filter(|p| p[3].visible()) {
            *c = crate::doc_redeye::redness_normalized(unit(p[0]), unit(p[1]), unit(p[2]));
        }
    }
    let mut mask = crate::doc_redeye::pupil_mask(&coverage, rw, rh, pupil, job)?;
    crate::doc_select::feather_mask_with_context(&mut mask, rw as u32, rh as u32, 1., job)?;
    for (i, c) in coverage.iter().enumerate() {
        job.every(i)?;
        let Some(di) = index(i) else {
            continue;
        };
        let ci = (y0 as usize + i / rw) * canvas.0 as usize + x0 as usize + i % rw;
        // Selection fades the complete result; it must not change detection.
        let amount = f32::from(selection[ci]) / 65535.;
        if amount == 0. || !pixels[di][3].visible() {
            continue;
        }
        let weight = c * f32::from(mask[i]) / 255.;
        let old = pixels[di];
        let neutral = (old[1].code() + old[2].code()) * 0.5;
        for ch in 0..3 {
            let v = old[ch].code();
            let corrected = if ch == 0 {
                v + weight * (neutral - v)
            } else {
                v
            };
            let darkened = corrected * (1. - weight * 0.8 * darken);
            pixels[di][ch] = S::store(v + amount * (darkened - v));
        }
    }
    Ok(())
}
