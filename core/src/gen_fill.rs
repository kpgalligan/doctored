//! Generative Fill's compositing kernel (M3b): the pixel math around a
//! cloud image-edit provider that answers a picture at one of its native
//! sizes. The adapters (`doc_gen_fill`) read the plate and build the layer;
//! the host sends the request between the two halves:
//!
//! 1. FRAME. The native size whose aspect is nearest the covered rect's
//!    ([`pick_native`]) and the crop around the rect at that aspect
//!    ([`crop`]).
//! 2. INPUT. The crop resampled to the native size by
//!    `remove_model::native_mean` (the exact area-weighted mean, and a cell
//!    is in the hole when any covered pixel lies in its footprint), then
//!    encoded as the two RGBA8 pictures the provider takes ([`encode_native`]):
//!    the image is transparent on the hole, so an API that reads its edit
//!    area from the image's alpha (OpenAI's GPT Image) generates there.
//! 3. APPLY. The provider's picture read back at each pixel of the covered
//!    rect ([`fill_at`]), opaque; the selection's own coverage becomes the
//!    new layer's mask, so nothing outside it changes.
//!
//! Generative Expand reuses all three steps: its coverage is
//! [`transparency_coverage`] of the composite's alpha, so the hole is the
//! area Canvas Size added and the frame, input and apply are Fill's.
//!
//! Every value stays in the document's encoding: the provider sees and
//! answers display-encoded pictures, so there is no colour conversion either
//! way.

use std::ops::RangeInclusive;

use crate::job_control::JobContext;
use crate::remove_model::Rect;
use crate::sampling_radial::sample_exact;

/// The native sides a provider may declare: 64 is the smallest picture an
/// image model still has context in, 2048 the largest side any provider the
/// ADR surveys answers.
pub(crate) const NATIVE_SIDES: RangeInclusive<u32> = 64..=2048;
/// The sides of a provider's answer the apply accepts: it is resampled onto
/// the crop whatever its size, so any picture up to twice the largest native
/// side reads back.
pub(crate) const RESULT_SIDES: RangeInclusive<u32> = 1..=4096;

/// `native` inside [`NATIVE_SIDES`] on both axes, else the refusal.
pub(crate) fn check_native(native: (u32, u32)) -> Result<(), String> {
    if NATIVE_SIDES.contains(&native.0) && NATIVE_SIDES.contains(&native.1) {
        Ok(())
    } else {
        Err(format!(
            "generative fill native size must be {}…{} on each side",
            NATIVE_SIDES.start(),
            NATIVE_SIDES.end()
        ))
    }
}

/// The native size among `sizes` whose aspect ratio is nearest `covered`'s:
/// least `|ln(aspect(size) / aspect(covered))|`, the first winning a tie, so
/// a wide rect is sent wide and a tall one tall. `Err` for no size or a size
/// outside [`NATIVE_SIDES`].
pub(crate) fn pick_native(covered: Rect, sizes: &[(u32, u32)]) -> Result<(u32, u32), String> {
    if sizes.is_empty() {
        return Err("generative fill needs at least one native size".into());
    }
    let aspect = f64::from(covered.2.max(1)) / f64::from(covered.3.max(1));
    let mut best: Option<((u32, u32), f64)> = None;
    for &size in sizes {
        check_native(size)?;
        let score = (f64::from(size.0) / f64::from(size.1) / aspect).ln().abs();
        if best.is_none_or(|(_, b)| score < b) {
            best = Some((size, score));
        }
    }
    Ok(best.map(|(size, _)| size).unwrap_or(sizes[0]))
}

/// The crop sent at `native` around `covered` on a `canvas`: `k = max(1,
/// 2w/nw, 2h/nh)`, the scale that makes the crop at least twice the rect on
/// each axis (so the provider sees at least as much context as hole); then
/// `k' = max(w/nw, h/nh, min(k, W/nw, H/nh))`, the largest scale at or under
/// `k` that the canvas holds on both axes, but never one that no longer
/// covers the rect; `cw = min(W, ⌈k'·nw⌉)`, `ch = min(H, ⌈k'·nh⌉)`, centred
/// on the rect and slid inside the canvas (`remove_model::crop`'s rule).
/// Shrinking the scale rather than clipping one axis keeps the crop at the
/// native aspect, so the provider's picture is not stretched back onto it
/// (a tall rect against a short canvas, Generative Expand's frame); only a
/// rect wider (or taller) than the canvas allows at that aspect forces a
/// clip. `k' ≥ w/nw` and `k' ≥ h/nh` give `cw ≥ w` and `ch ≥ h`, and the
/// slide is clamped, so the crop always contains the rect and never leaves
/// the canvas (the GVAR reader requires `inside(crop, raster)`). A crop
/// smaller than the native size (`k = 1` on a small hole) is resampled UP.
pub(crate) fn crop(covered: Rect, canvas: (u32, u32), native: (u32, u32)) -> Rect {
    let (x0, y0, w, h) = covered;
    let (nw, nh) = (f64::from(native.0), f64::from(native.1));
    let (cw_max, ch_max) = (f64::from(canvas.0), f64::from(canvas.1));
    let k = 1f64
        .max(2.0 * f64::from(w) / nw)
        .max(2.0 * f64::from(h) / nh);
    let fits = k.min(cw_max / nw).min(ch_max / nh);
    let k = (f64::from(w) / nw).max(f64::from(h) / nh).max(fits);
    let cw = ((k * nw).ceil() as i64).min(i64::from(canvas.0));
    let ch = ((k * nh).ceil() as i64).min(i64::from(canvas.1));
    let place = |start: u32, extent: u32, size: i64, total: u32| -> u32 {
        let centred = i64::from(start) + i64::from(extent) / 2 - size / 2;
        centred.clamp(0, i64::from(total) - size) as u32
    };
    (
        place(x0, w, cw, canvas.0),
        place(y0, h, ch, canvas.1),
        cw as u32,
        ch as u32,
    )
}

/// Generative Expand's coverage of a pixel whose composite alpha is `alpha`
/// (u16, a byte alpha × 257): `65535 − α`, so a pixel the picture leaves
/// fully transparent is wholly in the hole, an opaque one wholly kept, and an
/// anti-aliased edge partly both. The new layer's mask is this coverage, so
/// the composite over an opaque pixel is the picture unchanged.
pub(crate) fn transparency_coverage(alpha: u16) -> u16 {
    u16::MAX - alpha
}

/// A unit sample as a byte, rounded.
fn byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The two pictures the provider takes, from `native_mean`'s planes over
/// `cells` cells (`image` channel-major `3·cells`, `mask` `cells`):
/// `image_out` RGBA8 (the mean RGB rounded to bytes, alpha 0 on a hole cell
/// and 255 on a kept cell) and `mask_out` RGBA8, where a hole cell is
/// (255, 255, 255, 0) — the transparent pixels an image-edit API fills — and
/// a kept cell (0, 0, 0, 255). The image's alpha follows the hole because
/// GPT Image masking is prompt-based and reads transparent image pixels as
/// the edit area too: an opaque black Expand border came back black. The RGB
/// stays the mean, for a provider that is sent the image opaque. Polls `job`
/// every few thousand cells.
pub(crate) fn encode_native(
    image: &[f32],
    mask: &[f32],
    cells: usize,
    image_out: &mut [u8],
    mask_out: &mut [u8],
    job: JobContext<'_>,
) -> Result<(), String> {
    if image.len() != 3 * cells
        || mask.len() != cells
        || image_out.len() != 4 * cells
        || mask_out.len() != 4 * cells
    {
        return Err("invalid generative fill input buffers".into());
    }
    for i in 0..cells {
        job.every(i)?;
        let at = 4 * i;
        image_out[at] = byte(image[i]);
        image_out[at + 1] = byte(image[cells + i]);
        image_out[at + 2] = byte(image[2 * cells + i]);
        let hole = mask[i] > 0.0;
        image_out[at + 3] = if hole { 0 } else { 255 };
        mask_out[at..at + 4].copy_from_slice(if hole {
            &[255, 255, 255, 0]
        } else {
            &[0, 0, 0, 255]
        });
    }
    Ok(())
}

/// The provider's fill at canvas pixel `(px, py)`: its straight RGBA8
/// `result` of `size` read as opaque `[r, g, b, 1]` with
/// `sampling_radial::sample_exact` (bilinear, the pixel centre mapped into
/// the crop's frame at the result's size), in 0…1. The master copy of this
/// read is `remove_model::fill_at`, which reads the model's f32 plane; this
/// twin reads bytes and maps through the result's own size, since a provider
/// may answer a size other than the one asked for. Bilinear for the same
/// reason as the master: a nearest read would print the cells as blocks
/// across the hole when the crop was resampled down.
pub(crate) fn fill_at(result: &[u8], size: (u32, u32), crop: Rect, px: u32, py: u32) -> [f64; 3] {
    let read = |i: usize| {
        let at = 4 * i;
        [
            f32::from(result[at]) / 255.0,
            f32::from(result[at + 1]) / 255.0,
            f32::from(result[at + 2]) / 255.0,
            1.0,
        ]
    };
    let (cx, cy, cw, ch) = crop;
    let qx = (f64::from(px) + 0.5 - f64::from(cx)) * f64::from(size.0) / f64::from(cw);
    let qy = (f64::from(py) + 0.5 - f64::from(cy)) * f64::from(size.1) / f64::from(ch);
    let v = sample_exact(&read, (size.0 as usize, size.1 as usize), (qx, qy), true);
    [v[0], v[1], v[2]].map(|c| c.clamp(0.0, 1.0))
}

/// The kernel's own working set on a `width` × `height` canvas at `native`,
/// writing `bytes_per_pixel` bytes a layer pixel (4 byte, 8 u16) and
/// `mask_bytes` a mask sample (1 byte, 2 u16), beside the plate (the
/// adapters add it):
///
/// - `native_mean`'s planes, image `3·cells` f32 and mask `cells` f32 — 16
///   bytes a cell;
/// - its accumulator, four f64 per cell, and its mask flags — 33 bytes a cell;
/// - the two RGBA8 pictures `encode_native` writes — 8 bytes a cell;
/// - the new canvas-sized layer at `bytes_per_pixel` and its mask at
///   `mask_bytes`;
/// - per stored variation (`variations`), its PNG
///   (`drdc_save_plan::byte_png_bound` at the native size, the master
///   bound) and its decoded RGBA8 copy, `4·cells`.
///
/// The crop's own rows and the per-axis footprint tables are O(width +
/// height + nw + nh) and ride in the 1 KiB per-side slack.
pub(crate) fn scratch_bytes(
    width: u32,
    height: u32,
    native: (u32, u32),
    bytes_per_pixel: u64,
    mask_bytes: u64,
    variations: u64,
) -> u64 {
    let cells = u64::from(native.0) * u64::from(native.1);
    let n = u64::from(width) * u64::from(height);
    let planes = 16 * cells;
    let accumulate = 33 * cells;
    let outputs = 8 * cells;
    let layer = n.saturating_mul(bytes_per_pixel);
    let mask = n.saturating_mul(mask_bytes);
    let slack =
        1024 * (u64::from(native.0) + u64::from(native.1) + u64::from(width) + u64::from(height));
    let stored = crate::drdc_save_plan::byte_png_bound(native, 4)
        .unwrap_or(u64::MAX)
        .saturating_add(4 * cells)
        .saturating_mul(variations);
    (planes + accumulate + outputs + layer + mask + slack).saturating_add(stored)
}
