//! Layer-panel thumbnails FITTED BEFORE any whole-layer copy.
//!
//! A thumbnail used to copy the whole layer first — the byte depth a
//! transient `to_contiguous()` for the image crate's resize, the typed depth
//! a whole-layer RGBA8 quantization in the host before its resize — so a
//! cache miss on a 6000 × 4000 layer paid a 24 MP copy and a kernel whose
//! cost is the SOURCE's (~360 ms at 16 bits). Here a layer at least 16× the
//! thumbnail per axis takes the Channels panel's two-step reduction instead
//! (`ffi_channel::plane_image`): box means read straight out of the tiles
//! through `row_segments` (never a whole-plane copy), then the same kernel
//! over the small intermediate. The factor is
//! `ffi_channel::thumb_box_factor` at [`LAYER_THUMB_BOX_GUARD`], so the box
//! leaves at least 8 samples per output pixel per axis for the kernel to
//! weight, and the result stays within 2 codes of the single full-kernel
//! pass on alpha and every premultiplied channel (`tests/layer_thumbnail.rs`,
//! hard edges included). Every smaller layer keeps today's path and today's
//! bytes exactly.
//!
//! The box mean is the RGBA, AREA-weighted relative of `thumbnail_reduce::box_reduced`
//! (the master copy of the plain box mean) in PREMULTIPLIED space, because
//! alpha is coverage: per sample it is `sampling::accumulate_unorm`'s formula
//! (the master of the premultiplied accumulation) carried in f64, and each
//! box ends through `sampling::straight_unorm` — hidden RGB never leaks into
//! a visible mean, and a mean alpha that rounds to zero has no surviving
//! colour. Its boxes are the equal real-valued areas `box_reduced` uses
//! (both split by `thumbnail_reduce::area_split`); why is on [`box_reduced_rgba`].
//!
//! The typed DOCUMENT thumbnails — a float layer through its tone map, a u16
//! mask plane — reuse this file's fit ([`fitted_dims`]), guard and box-arm
//! tail ([`box_arm_scratch`] / [`finish_box_arm`]) in `precision_thumbnail`.
//!
//! The 8-bit MASK thumbnail ([`DrDocument::layer_mask_thumbnail`]) is
//! the byte twin of the u16 one: the same fit and guard, the plain box mean
//! `thumbnail_reduce::box_reduced` over the stored plane (a group's canvas view
//! through `thumbnail_reduce::box_reduced_rows` over `doc_channel::placed_row`),
//! then `thumbnail_reduce::gray_resized` — no whole-mask RGBA image.
//!
//! `dr_doc_layer_thumbnail`'s raster arm (`fitted_rgba8`) and `PrecisionImage::thumbnail_rgba8`
//! (`dr_precision_image_thumbnail_rgba8`). A layer at least 16× the thumbnail per axis
//! (`ffi_channel::thumb_box_factor` at `LAYER_THUMB_BOX_GUARD` 8, measured: the plane thumbnails' 4
//! drifts 3 codes on an RGBA layer) is box-reduced out of its rows (`box_reduced_rgba`: EQUAL
//! real-valued area boxes tiling the image exactly, a straddling sample split by area through
//! `thumbnail_reduce::area_split`, each stored sample read through a per-sample map (`|p| *p`, or
//! the float thumbnail's tone map), premultiplied in f64 by `accumulate_unorm`'s formula, ended by
//! `straight_unorm`, polled every `EVERY_INTERVAL` samples — the whole-cell boxes with a partial
//! last cell that `thumbnail_reduce::box_reduced` once used stretched the kernel's grid and
//! drifted 54 codes at a hard edge) and then resized with the same kernel, within 2 codes of the
//! single pass on alpha and every premultiplied channel (`tests/layer_thumbnail.rs`). The typed
//! thumbnail keeps the host's former `RasterDocument.fitted` sizing (never upscales; its tie
//! rounding, restated in `fitted_dims`), admits and reserves its arm's scratch (the u16 box arm's
//! terms are `box_arm_scratch`, its tail `finish_box_arm`), and refuses f32 with
//! `FLOAT_RGBA8_REFUSAL` (`precision_image`, shared with `to_rgba8`).
//! `DrDocument::layer_mask_thumbnail` (`dr_doc_layer_mask_thumbnail`) is the 8-bit mask
//! thumbnail fitted before any whole-mask image: `mask_image`'s picture (a group's canvas view, any
//! other layer's raw mask), `fitted_dims`, `LAYER_THUMB_BOX_GUARD`, then `box_reduced` (a group's
//! placed rows through `box_reduced_rows` over `doc_channel::placed_row`, the row form of
//! `padded_plane_with_coverage`) and `gray_resized`; byte-identical to `dr_doc_layer_mask_image` +
//! `dr_image_resize` below the threshold, within 2 codes above it.

use std::sync::Arc;

use image::imageops::FilterType;
use image::RgbaImage;

use crate::doc::{DrDocument, LayerKind};
use crate::doc_channel::{padded_plane_with_coverage, placed_row};
use crate::doc_plane::gray_image;
use crate::ffi_channel::thumb_box_factor;
use crate::ffi_util::thumb_dims;
use crate::icc::IccProfile;
use crate::job_control::{DrJobControl, JobContext};
use crate::layer_raster::LayerRaster;
use crate::precision_image::{
    admit, pixel_count, reserved_vec, PrecisionImage, PrecisionRun, PrecisionStorage,
    BOOKKEEPING_BYTES, FLOAT_RGBA8_REFUSAL,
};
use crate::precision_quantize::PrecisionDither;
use crate::precision_sampling::{
    adoption_copy_bytes, resize_cost, resize_image, source_copy_bytes, GeometryFilter,
};
use crate::sampling::{straight_unorm, Unorm};
use crate::thumbnail_reduce::{area_split, box_reduced, box_reduced_rows, gray_resized};

/// How many box samples per OUTPUT pixel per axis the layer thumbnail's box
/// leaves for the Triangle pass (`ffi_channel::THUMB_BOX_GUARD`'s role, at a
/// layer's own measured value). The plane thumbnails' 4 is not enough for
/// RGBA layers: at 4 a 24 MP 16-bit layer with fine ringing detail (a 10×
/// Lanczos upscale of a hashed pattern, the jobs probe's fixture) drifts 3
/// codes from the single Triangle pass — the box ⊗ tent kernel is wider
/// than the tent alone. At 5, 6 and 8 it measures 2; the 2400 × 1600
/// gradient/noise/hard-edge fixtures measure 2 at 4 and 1 at 8. 8 keeps that
/// margin for the price of a 4× larger intermediate (285 × 190 for 6000 ×
/// 4000 at 34), which the kernel crosses in well under a millisecond; the box
/// pass over the layer is the cost either way.
pub(crate) const LAYER_THUMB_BOX_GUARD: u32 = 8;

/// A reduced image: its straight samples, row-major, and its dimensions.
pub(crate) type Reduced<T> = (Vec<[T; 4]>, u32, u32);

/// The image reduced by about the whole-number factor `k` in both axes, read
/// row by row through `row(y)` — the row's runs `(x, samples)`, left to right
/// and adjoining, as `LayerRaster::row_segments` /
/// `PrecisionImage::row_segments` answer them — each sample read as straight
/// RGBA through `sample` (`|p| *p` for a stored plane; the float thumbnail's
/// tone map), so nothing is copied but the `floor(w / k)` x `floor(h / k)`
/// result and two rows of accumulators. Every output sample is the
/// AREA-weighted premultiplied mean of its box, returned straight through
/// `straight_unorm`.
///
/// The RGBA relative of `thumbnail_reduce::box_reduced`, the master copy of the
/// plain box mean: the same equal boxes, tiling the image exactly — `w / sw`
/// by `h / sh` samples each, so between `k` and `k + 1` — with a sample
/// straddling a box edge shared between the two boxes by the area on either
/// side (`thumbnail_reduce::area_split`). That keeps every box centre ON the
/// uniform grid the kernel after it assumes. The whole `k`-sample boxes
/// with a partial last one that `box_reduced` once used stretched
/// that grid instead (a 2400-wide layer boxed by 17 was 142 cells of 17 =
/// 2414 samples, a hard edge moved by up to 14 source pixels in a 34-pixel
/// thumbnail: 54 codes from the single pass), and boxes of mixed whole
/// widths jittered their centres by half a sample (5 codes at a hard edge);
/// exact area boxes measure within 2.
///
/// `Ok(None)` for `k < 2` or an image smaller than `k`: nothing to reduce.
/// Polled every `EVERY_INTERVAL` samples (`JobContext::every` over the flat
/// visit count); a cancel answers the control's sentence and drops the
/// buffers.
pub(crate) fn box_reduced_rgba<'a, S: 'a, T: Unorm, R: Iterator<Item = (u32, &'a [S])>>(
    width: u32,
    height: u32,
    k: u32,
    row: impl Fn(u32) -> R,
    sample: impl Fn(&S) -> [T; 4],
    job: JobContext<'_>,
) -> Result<Option<Reduced<T>>, String> {
    if k < 2 || width < k || height < k {
        return Ok(None);
    }
    let (sw, sh) = (width / k, height / k);
    let mut out = reserved_vec((sw as usize) * (sh as usize))?;
    // The current band's accumulators and the next band's (which a row
    // straddling the band edge already feeds): premultiplied RGB and alpha
    // in the storage's raw units, area-weighted, f64 because one box may
    // cover most of the layer.
    let mut current: Vec<[f64; 4]> = reserved_vec(sw as usize)?;
    current.resize(sw as usize, [0.0; 4]);
    let mut next: Vec<[f64; 4]> = reserved_vec(sw as usize)?;
    next.resize(sw as usize, [0.0; 4]);
    let unit = 1.0 / f64::from(T::MAX);
    let area = f64::from(width) * f64::from(height) / (f64::from(sw) * f64::from(sh));
    let emit = |sums: &mut Vec<[f64; 4]>, out: &mut Vec<[T; 4]>| {
        for acc in sums.iter_mut() {
            out.push(straight_unorm::<T>([
                (acc[0] / area) as f32,
                (acc[1] / area) as f32,
                (acc[2] / area) as f32,
                (acc[3] / area) as f32,
            ]));
            *acc = [0.0; 4];
        }
    };
    let add = |acc: &mut [f64; 4], cell: &[f64; 4], weight: f64| {
        for c in 0..4 {
            acc[c] += cell[c] * weight;
        }
    };
    let mut band = 0usize;
    let mut visited = 0usize;
    for y in 0..height {
        let (by, row_in, row_out) = area_split(y, height, sh);
        while band < by {
            emit(&mut current, &mut out);
            std::mem::swap(&mut current, &mut next);
            band += 1;
        }
        for (x0, run) in row(y) {
            let mut x = x0;
            let mut rest = run;
            while !rest.is_empty() {
                let (bx, col_in, col_out) = area_split(x, width, sw);
                // Whole samples of box `bx` up to its edge, else the one
                // sample straddling it.
                let take = if col_out == 0.0 {
                    let end = (u64::from(bx as u32 + 1) * u64::from(width) / u64::from(sw)) as u32;
                    ((end - x) as usize).min(rest.len()).max(1)
                } else {
                    1
                };
                let (cell, tail) = rest.split_at(take);
                // `accumulate_unorm` at weight 1, in f64.
                let mut sum = [0.0f64; 4];
                for stored in cell {
                    job.every(visited)?;
                    visited += 1;
                    let sample = sample(stored);
                    let a = f64::from(sample[3].raw());
                    let pm = a * unit;
                    sum[0] += pm * f64::from(sample[0].raw());
                    sum[1] += pm * f64::from(sample[1].raw());
                    sum[2] += pm * f64::from(sample[2].raw());
                    sum[3] += a;
                }
                add(&mut current[bx], &sum, row_in * col_in);
                if col_out > 0.0 {
                    add(&mut current[bx + 1], &sum, row_in * col_out);
                }
                if row_out > 0.0 {
                    add(&mut next[bx], &sum, row_out * col_in);
                    if col_out > 0.0 {
                        add(&mut next[bx + 1], &sum, row_out * col_out);
                    }
                }
                x += take as u32;
                rest = tail;
            }
        }
    }
    emit(&mut current, &mut out);
    Ok(Some((out, sw, sh)))
}

/// A byte layer's thumbnail, aspect-fit to `thumb_dims(max_side)` with the
/// Triangle filter (tiny layers upscale, as `dr_doc_layer_thumbnail` always
/// has). A layer at least 16× the thumbnail per axis is box-reduced out of
/// its tiles first (module doc); every smaller one keeps today's transient
/// `to_contiguous()` copy and its exact bytes. `None` for an empty layer, an
/// absurd target or an allocation failure.
pub(crate) fn fitted_rgba8(raster: &LayerRaster, max_side: u32) -> Option<RgbaImage> {
    let (w, h) = raster.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let (tw, th) = thumb_dims(w, h, max_side);
    let k = thumb_box_factor(w, h, tw, th, LAYER_THUMB_BOX_GUARD);
    if k < 2 {
        let source = raster.to_contiguous().ok()?;
        return crate::ops::resize(&source, tw, th, FilterType::Triangle);
    }
    let (samples, sw, sh) = box_reduced_rgba(
        w,
        h,
        k,
        |y| raster.row_segments(y, 0, w),
        |p| *p,
        JobContext::NONE,
    )
    .ok()??;
    let source = RgbaImage::from_raw(sw, sh, samples.into_flattened())?;
    crate::ops::resize(&source, tw, th, FilterType::Triangle)
}

/// The 16-bit thumbnail's dimensions: `RasterDocument.fitted`'s rule
/// (app/Sources/Channel.swift, the master), which the typed thumbnail used
/// before it moved into the core — never upscale (`max_side` 0 or a longest
/// side within it keeps the size), else `round(side · (max_side / longest))`
/// per axis, at least 1. Restated rather than `thumb_dims` on purpose: the
/// two round differently at exact ties (`thumb_dims` divides once, `fitted`
/// multiplies by a rounded scale — 440 × 55 at 28 is 28 × 4 against 28 × 3),
/// and a 16-bit thumbnail keeps the dimensions it had.
pub(crate) fn fitted_dims(w: u32, h: u32, max_side: u32) -> (u32, u32) {
    let longest = w.max(h);
    if max_side == 0 || longest <= max_side {
        return (w, h);
    }
    let scale = f64::from(max_side) / f64::from(longest);
    let side = |v: u32| ((f64::from(v) * scale).round() as u32).max(1);
    (side(w), side(h))
}

impl DrDocument {
    /// Layer `idx`'s mask as its Layers-panel thumbnail: opaque gray
    /// RGBA8, aspect-fit by [`fitted_dims`] (the host's former fit: never
    /// upscaled, `max_side` 0 = full size). The picture is
    /// [`DrDocument::mask_image`]'s — a GROUP's canvas-aligned view (canvas
    /// size, the mask's offset, its outside coverage beyond the plane), every
    /// other kind the raw mask at its stored size — without building it.
    ///
    /// Below 16× the thumbnail per axis ([`LAYER_THUMB_BOX_GUARD`], the u16
    /// mask thumbnail's guard) it is byte for byte the former host path,
    /// `mask_image` then the Triangle resize (`dr_image_resize`
    /// FILTER_BILINEAR) when it shrinks: the gray plane through
    /// `thumbnail_reduce::gray_resized`, whose kernel over a plane equals the
    /// kernel over its gray image (`ffi_channel::plane_image`). Above it the
    /// plane is box-reduced first (`thumbnail_reduce::box_reduced`; a group's
    /// placed rows through `box_reduced_rows`), within 2/255 of that path.
    ///
    /// `None` without the layer or a mask, for a zero-size mask and on an
    /// allocation failure.
    pub fn layer_mask_thumbnail(&self, idx: usize, max_side: u32) -> Option<RgbaImage> {
        let layer = self.layers.get(idx)?;
        let raw = layer.mask.as_deref()?;
        let group = layer.kind == LayerKind::Group;
        let (w, h) = if group {
            (self.width, self.height)
        } else {
            raw.dimensions()
        };
        if w == 0 || h == 0 || raw.width() == 0 || raw.height() == 0 {
            return None;
        }
        let (tw, th) = fitted_dims(w, h, max_side);
        let resizing = (tw, th) != (w, h);
        let k = if resizing {
            thumb_box_factor(w, h, tw, th, LAYER_THUMB_BOX_GUARD)
        } else {
            1
        };
        let properties = &layer.mask_properties;
        if k < 2 {
            let plane = if group {
                padded_plane_with_coverage(
                    raw,
                    w,
                    h,
                    properties.offset,
                    properties.outside_coverage,
                )
                .into_raw()
            } else if resizing {
                raw.as_raw().clone()
            } else {
                return gray_image(raw.as_raw(), w, h);
            };
            return if resizing {
                gray_resized(plane, w, h, tw, th)
            } else {
                gray_image(&plane, w, h)
            };
        }
        let reduced = if group {
            let mut fill = reserved_vec(w as usize).ok()?;
            fill.resize(w as usize, properties.outside_coverage);
            box_reduced_rows(
                w,
                h,
                k,
                |y| {
                    placed_row(
                        raw.as_raw(),
                        raw.dimensions(),
                        properties.offset,
                        &fill,
                        w,
                        y,
                    )
                },
                |v| v,
                JobContext::NONE,
            )
        } else {
            box_reduced(raw.as_raw(), w, h, k, |v| v, JobContext::NONE)
        };
        let (codes, sw, sh) = reduced.ok()??;
        gray_resized(codes, sw, sh, tw, th)
    }
}

impl PrecisionImage {
    /// The u16 image's layer-panel thumbnail as RGBA8 (no dither), aspect-fit
    /// by `RasterDocument.fitted`'s rule ([`fitted_dims`]: never upscaled).
    /// Below the box threshold it is today's path exactly — the whole image
    /// quantized (`to_rgba8`'s bytes), then resized at 8 bits with the
    /// Triangle filter (`dr_image_resize`'s bytes). A layer at least 16× the
    /// thumbnail per axis is box-reduced out of its tiles at u16, resized
    /// Bilinear at u16 and quantized once, within 2/255 of that path; no
    /// whole-layer copy is made.
    ///
    /// Refuses f32 storage with `to_rgba8`'s sentence (a float layer is
    /// displayed through its document's tone map). Admits the scratch of the
    /// arm it runs — bookkeeping, the box buffers and their adoption, the
    /// resize's source copy, output and temporaries, the quantized bytes —
    /// against `limit` beside the image's retained bytes and `other`, and
    /// reserves it on the job before allocating; the box pass polls every
    /// `EVERY_INTERVAL` samples.
    pub fn thumbnail_rgba8(
        &self,
        max_side: u32,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<RgbaImage, String> {
        if self.storage() != PrecisionStorage::Rgba16Unorm {
            return Err(FLOAT_RGBA8_REFUSAL.into());
        }
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let (w, h) = self.dimensions();
        let count = pixel_count(w, h)? as u64;
        let (tw, th) = fitted_dims(w, h, max_side);
        let resizing = (tw, th) != (w, h);
        let k = if resizing {
            thumb_box_factor(w, h, tw, th, LAYER_THUMB_BOX_GUARD)
        } else {
            1
        };
        let overflow = || "thumbnail working-memory calculation overflow".to_string();
        let scratch = if k < 2 {
            // The whole image quantized, then (when it shrinks) the 8-bit
            // separable resize: its output and temporaries, charged at
            // `resize_cost`'s u16 figures, which cover the byte ones.
            let resize = if resizing {
                let (output, temporary, _) =
                    resize_cost((w, h), (tw, th), GeometryFilter::Bilinear, true)?;
                output + temporary
            } else {
                0
            };
            BOOKKEEPING_BYTES
                .checked_add(count.checked_mul(4).ok_or_else(overflow)?)
                .and_then(|v| v.checked_add(resize))
                .ok_or_else(overflow)?
        } else {
            BOOKKEEPING_BYTES
                .checked_add(box_arm_scratch((w / k, h / k), (tw, th))?)
                .ok_or_else(overflow)?
        };
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        if k < 2 {
            let quantized = self.quantize_with_context(PrecisionDither::None, job)?;
            if !resizing {
                return Ok(quantized);
            }
            return crate::ops::resize(&quantized, tw, th, FilterType::Triangle)
                .ok_or_else(|| "thumbnail dimensions exceed the supported pixel limit".into());
        }
        let rows = |y: u32| {
            self.row_segments(y, 0, w).filter_map(|(x, run)| match run {
                PrecisionRun::Rgba16(run) => Some((x, run)),
                PrecisionRun::Rgba32(_) => None,
            })
        };
        let reduced = box_reduced_rgba(w, h, k, rows, |p| *p, job.range(0.0, 0.8))?
            .ok_or("thumbnail box reduction found nothing to reduce")?;
        finish_box_arm(reduced, self.profile().clone(), (tw, th), job)
    }
}

/// The working memory of a u16 box arm after its box pass, without the
/// bookkeeping: the `reduced` box buffers and their adoption, the resize's
/// source copy, output and temporaries to `fitted`, the resized image's
/// adoption and its quantized bytes. The ONE statement of the arm's terms,
/// shared by [`PrecisionImage::thumbnail_rgba8`] and the typed document
/// thumbnails (`precision_thumbnail`).
pub(crate) fn box_arm_scratch(reduced: (u32, u32), fitted: (u32, u32)) -> Result<u64, String> {
    let (sw, sh) = reduced;
    let (tw, th) = fitted;
    let boxed = u64::from(sw) * u64::from(sh) * 8 + u64::from(sw) * 64;
    let (output, temporary, _) = resize_cost((sw, sh), (tw, th), GeometryFilter::Bilinear, true)?;
    [
        boxed,
        adoption_copy_bytes((sw, sh))?,
        source_copy_bytes((sw, sh))?,
        output,
        temporary,
        adoption_copy_bytes((tw, th))?,
        u64::from(tw) * u64::from(th) * 4,
    ]
    .iter()
    .try_fold(0u64, |sum, &term| sum.checked_add(term))
    .ok_or_else(|| "thumbnail working-memory calculation overflow".to_string())
}

/// The tail of a u16 box arm: the box means adopted as an image in
/// `profile`, resized Bilinear at u16 to `fitted` on `job.range(0.8, 0.95)`
/// and quantized once without dither on `job.range(0.95, 1.0)`. Its scratch
/// is [`box_arm_scratch`]'s, admitted and reserved by the caller.
pub(crate) fn finish_box_arm(
    reduced: Reduced<u16>,
    profile: Arc<IccProfile>,
    fitted: (u32, u32),
    job: JobContext<'_>,
) -> Result<RgbaImage, String> {
    let (samples, sw, sh) = reduced;
    let (tw, th) = fitted;
    let reduced = PrecisionImage::from_rgba16_unseeded(sw, sh, samples, profile, None)?;
    let small = resize_image(
        &reduced,
        tw,
        th,
        GeometryFilter::Bilinear,
        None,
        job.range(0.8, 0.95),
    )?;
    small.quantize_with_context(PrecisionDither::None, job.range(0.95, 1.0))
}

/// `image` (u16) fitted to `max_side` at u16, before any conversion: the
/// fit both u16 arms of [`PrecisionImage::thumbnail_rgba8`] run, without
/// its quantize, for a caller that converts the small image next (the Lab
/// thumbnail, `lab_rgb`; the fit-first rule). [`fitted_dims`] decides
/// the size, and an unchanged size returns the image itself. Below the box
/// threshold ([`LAYER_THUMB_BOX_GUARD`]) one Bilinear resize at u16; above
/// it the box means out of the tiles, then the same resize
/// ([`finish_box_arm`]'s steps). Its scratch is [`fit_u16_scratch`]'s,
/// admitted and reserved by the caller.
pub(crate) fn fit_u16(
    image: &PrecisionImage,
    max_side: u32,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    if image.storage() != PrecisionStorage::Rgba16Unorm {
        return Err(FLOAT_RGBA8_REFUSAL.into());
    }
    let (w, h) = image.dimensions();
    let (tw, th) = fitted_dims(w, h, max_side);
    if (tw, th) == (w, h) {
        return Ok(image.clone());
    }
    let k = thumb_box_factor(w, h, tw, th, LAYER_THUMB_BOX_GUARD);
    if k < 2 {
        return resize_image(image, tw, th, GeometryFilter::Bilinear, None, job);
    }
    let rows = |y: u32| {
        image
            .row_segments(y, 0, w)
            .filter_map(|(x, run)| match run {
                PrecisionRun::Rgba16(run) => Some((x, run)),
                PrecisionRun::Rgba32(_) => None,
            })
    };
    let (samples, sw, sh) = box_reduced_rgba(w, h, k, rows, |p| *p, job.range(0.0, 0.8))?
        .ok_or("thumbnail box reduction found nothing to reduce")?;
    let reduced =
        PrecisionImage::from_rgba16_unseeded(sw, sh, samples, image.profile().clone(), None)?;
    resize_image(
        &reduced,
        tw,
        th,
        GeometryFilter::Bilinear,
        None,
        job.range(0.8, 1.0),
    )
}

/// The working memory of [`fit_u16`] for an image of `dimensions`, without
/// the bookkeeping: the box arm's terms ([`box_arm_scratch`], whose
/// quantized bytes cover the fitted image's), else the resize's source
/// copy, output and temporaries and the output's adoption; nothing for an
/// unchanged size.
pub(crate) fn fit_u16_scratch(dimensions: (u32, u32), max_side: u32) -> Result<u64, String> {
    let (w, h) = dimensions;
    let (tw, th) = fitted_dims(w, h, max_side);
    if (tw, th) == (w, h) {
        return Ok(0);
    }
    let k = thumb_box_factor(w, h, tw, th, LAYER_THUMB_BOX_GUARD);
    if k >= 2 {
        return box_arm_scratch((w / k, h / k), (tw, th));
    }
    let (output, temporary, _) = resize_cost((w, h), (tw, th), GeometryFilter::Bilinear, true)?;
    [
        source_copy_bytes((w, h))?,
        output,
        temporary,
        adoption_copy_bytes((tw, th))?,
    ]
    .iter()
    .try_fold(0u64, |sum, &term| sum.checked_add(term))
    .ok_or_else(|| "thumbnail working-memory calculation overflow".to_string())
}
