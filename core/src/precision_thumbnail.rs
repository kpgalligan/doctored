//! The typed DOCUMENT's layer-panel thumbnails, FITTED BEFORE any whole-layer
//! work: a 32-bit float layer through its document's tone map
//! ([`PrecisionDocument::display_thumbnail_rgba8`]) and a u16 layer mask
//! plane ([`PrecisionDocument::layer_mask_thumbnail_rgba8`]).
//!
//! Both follow `PrecisionImage::thumbnail_rgba8` (`layer_thumbnail`) and
//! state none of its rules again: the fit is `layer_thumbnail::fitted_dims`
//! (the host's former `RasterDocument.fitted`, never upscaled), the box
//! threshold `ffi_channel::thumb_box_factor` at
//! `layer_thumbnail::LAYER_THUMB_BOX_GUARD` (the Layers panel's one guard),
//! and below it each is its former path byte for byte. The Channels panel's
//! two plane thumbnails (last bullet) share the fit and the guard.
//!
//! * FLOAT: below the threshold, the whole layer through
//!   `display_rgba8_with_context` and the Triangle resize — exactly the
//!   host's former `dr_precision_doc_display_rgba8` + `dr_image_resize`
//!   pair. Above it, `box_reduced_rgba` reads the f32 tiles through
//!   `FloatPolicy::display_sample` and `unit_to_u16` (the per-pixel rules
//!   `tone_mapped_u16` stores by), so its result is the u16 box arm over
//!   `tone_mapped_u16(image)` without that whole-layer copy, and
//!   `layer_thumbnail::finish_box_arm` ends it.
//! * MASK: the raw stored plane at its own size (no density, feather or
//!   enabled flag — the former `read_plane(canvas: false)` read), each code
//!   reduced by `precision_quantize::nearest` (`(v + 128) / 257`, the host's
//!   former display rule). No resize: the quantized plane as opaque gray;
//!   below the threshold: the quantized plane through
//!   `thumbnail_reduce::gray_resized`; above it: `thumbnail_reduce::box_reduced`
//!   (the master plain box mean) over the codes, then `gray_resized`. A
//!   GROUP's mask (isolated or pass-through) is the exception: it answers the
//!   CANVAS view of the Channels row below, the picture the 8-bit mask
//!   thumbnail shows for a byte group.
//!
//! * CHANNELS rows: a u16 alpha channel
//!   ([`PrecisionDocument::channel_thumbnail_rgba8`]) and a layer mask's
//!   CANVAS view ([`PrecisionDocument::layer_mask_canvas_thumbnail_rgba8`]:
//!   the plane at its offset, its outside coverage beyond it — the
//!   `read_plane canvas: true` placement) for the 16-bit Channels panel,
//!   which converted the whole canvas plane in Swift before. One arm serves
//!   both: the plane's canvas rows come from `doc_channel::placed_row`, codes
//!   by `nearest`, and above the box threshold `thumbnail_reduce::box_reduced_rows`
//!   reduces them before `gray_resized`. Below the box threshold each is
//!   byte for byte the former `(v + 128) / 257` canvas gray image +
//!   `dr_image_resize`. The guard is the Layers panel's
//!   `LAYER_THUMB_BOX_GUARD` (8), not the 8-bit Channels plane thumbnails'
//!   `ffi_channel::THUMB_BOX_GUARD` (4): at 4 a 2400 × 1600 noisy mask
//!   moved to (-37, -23) with outside coverage 255 measured 3 codes from the
//!   former pair (a one-off measurement), at 8 it measures
//!   within 2 (`tests/layer_thumbnail.rs`).
//!
//! Each arm admits its own scratch against `limit` beside the retained bytes
//! and `other`, then reserves it on the job before allocating; the box
//! passes and the mask quantize poll.
//!
//! `display_thumbnail_rgba8` (`dr_precision_doc_display_thumbnail_rgba8`, a float layer through its
//! document's tone map; the box arm reads the f32 tiles through `FloatPolicy::display_sample` and
//! `unit_to_u16`, the per-pixel rules `tone_mapped_u16` stores by, so it equals the u16 box arm
//! over `tone_mapped_u16(image)` without that copy) and `layer_mask_thumbnail_rgba8`
//! (`dr_precision_doc_layer_mask_thumbnail_rgba8`, the raw u16 mask plane at its stored size, codes
//! by `precision_quantize::nearest`, the box arm `thumbnail_reduce::box_reduced`; a GROUP's mask —
//! isolated or pass-through — answers `layer_mask_canvas_thumbnail_rgba8`'s canvas view instead,
//! the picture the 8-bit `layer_mask_thumbnail` shows for a byte group, handed off before the
//! single-use control begins). Both share `layer_thumbnail`'s fit, `LAYER_THUMB_BOX_GUARD` and
//! box-arm tail, are byte-identical to their former host paths below the box threshold
//! (`display_rgba8` / the `(v + 128) / 257` gray image, then `dr_image_resize` Bilinear) and
//! measure within 2 codes above it, and admit and reserve their arm's scratch (the float display's
//! is `float_document::display_scratch`). The 16-bit Channels panel's rows fit here too:
//! `channel_thumbnail_rgba8` (`dr_precision_doc_channel_thumbnail_rgba8`) and
//! `layer_mask_canvas_thumbnail_rgba8` (`dr_precision_doc_layer_mask_canvas_thumbnail_rgba8`, the
//! mask's `read_plane canvas: true` placement) share one placed-plane arm — canvas rows from
//! `doc_channel::placed_row`, codes by `nearest`, `box_reduced_rows` above the threshold — at
//! `LAYER_THUMB_BOX_GUARD`, because the Channels guard 4 measured 3 codes on a moved noisy mask.

use image::imageops::FilterType;
use image::RgbaImage;

use crate::doc_channel::placed_row;
use crate::doc_plane::gray_image;
use crate::ffi_channel::thumb_box_factor;
use crate::float_document::display_scratch;
use crate::job_control::{DrJobControl, JobContext, EVERY_INTERVAL};
use crate::layer_thumbnail::{
    box_arm_scratch, box_reduced_rgba, finish_box_arm, fitted_dims, LAYER_THUMB_BOX_GUARD,
};
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{
    admit, pixel_count, reserved_vec, unit_to_u16, PrecisionImage, PrecisionRun, PrecisionStorage,
    BOOKKEEPING_BYTES,
};
use crate::precision_quantize::nearest;
use crate::precision_sampling::{resize_cost, GeometryFilter};
use crate::thumbnail_reduce::{box_reduced, box_reduced_rows, gray_resized};

const OVERFLOW: &str = "thumbnail working-memory calculation overflow";
const TOO_LARGE: &str = "thumbnail dimensions exceed the supported pixel limit";
const NOTHING_TO_REDUCE: &str = "thumbnail box reduction found nothing to reduce";

/// The fitted size of a `w` x `h` source and its box factor: 1 when the
/// thumbnail keeps the source's size or the source is under 16× the
/// thumbnail per axis.
fn fit(w: u32, h: u32, max_side: u32) -> ((u32, u32), bool, u32) {
    let (tw, th) = fitted_dims(w, h, max_side);
    let resizing = (tw, th) != (w, h);
    let k = if resizing {
        thumb_box_factor(w, h, tw, th, LAYER_THUMB_BOX_GUARD)
    } else {
        1
    };
    ((tw, th), resizing, k)
}

/// `terms` summed, checked.
fn total(terms: &[u64]) -> Result<u64, String> {
    terms
        .iter()
        .try_fold(0u64, |sum, &term| sum.checked_add(term))
        .ok_or_else(|| OVERFLOW.to_string())
}

/// The output and temporaries of the resize from `source` to `fitted`, at
/// `resize_cost`'s u16 RGBA figures, which cover the byte and gray ones.
fn resize_terms(source: (u32, u32), fitted: (u32, u32)) -> Result<u64, String> {
    let (output, temporary, _) = resize_cost(source, fitted, GeometryFilter::Bilinear, true)?;
    total(&[output, temporary])
}

impl PrecisionDocument {
    /// Float layer `image`'s layer-panel thumbnail as RGBA8 through this
    /// document's tone map (module doc): aspect-fit by `fitted_dims` (never
    /// upscaled); below 16× the thumbnail per axis byte for byte
    /// `display_rgba8` then the Triangle resize, above it box-reduced out of
    /// the tiles through the tone map, resized Bilinear at u16 and quantized
    /// once, with no whole-layer copy.
    ///
    /// Refuses a document that is not a float document and an image that is
    /// not `rgba32_float` (`tone_mapped_u16`'s sentences), scratch past
    /// `limit` and a cancel.
    pub fn display_thumbnail_rgba8(
        &self,
        image: &PrecisionImage,
        max_side: u32,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<RgbaImage, String> {
        let policy = self
            .float
            .as_ref()
            .ok_or("tone mapping requires a 32-bit float document")?;
        if image.storage() != PrecisionStorage::Rgba32Float {
            return Err("tone mapping requires rgba32_float storage".into());
        }
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let (w, h) = image.dimensions();
        pixel_count(w, h)?;
        let ((tw, th), resizing, k) = fit(w, h, max_side);
        let scratch = if k < 2 {
            let resize = if resizing {
                resize_terms((w, h), (tw, th))?
            } else {
                0
            };
            total(&[display_scratch(w, h)?, resize])?
        } else {
            total(&[
                BOOKKEEPING_BYTES,
                box_arm_scratch((w / k, h / k), (tw, th))?,
            ])?
        };
        admit(image.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        if k < 2 {
            let display = self.display_rgba8_with_context(image, job)?;
            if !resizing {
                return Ok(display);
            }
            return crate::ops::resize(&display, tw, th, FilterType::Triangle)
                .ok_or_else(|| TOO_LARGE.into());
        }
        let rows = |y: u32| {
            image
                .row_segments(y, 0, w)
                .filter_map(|(x, run)| match run {
                    PrecisionRun::Rgba32(run) => Some((x, run)),
                    PrecisionRun::Rgba16(_) => None,
                })
        };
        let sample = |p: &[f32; 4]| policy.display_sample(*p).map(unit_to_u16);
        let reduced = box_reduced_rgba(w, h, k, rows, sample, job.range(0.0, 0.8))?
            .ok_or(NOTHING_TO_REDUCE)?;
        finish_box_arm(reduced, image.profile().clone(), (tw, th), job)
    }

    /// Record `index`'s raw mask plane as a layer-panel thumbnail: opaque
    /// gray RGBA8, aspect-fit by `fitted_dims` (never upscaled). Each stored
    /// code reads as `(v + 128) / 257`; below 16× the thumbnail per axis the
    /// result is byte for byte that gray image through the Triangle resize,
    /// above it the codes are box-reduced first (module doc), with no
    /// whole-plane image built.
    ///
    /// A GROUP's mask (isolated or pass-through) answers its CANVAS view
    /// instead — exactly [`Self::layer_mask_canvas_thumbnail_rgba8`], the
    /// picture the 8-bit `layer_mask_thumbnail` shows for a byte group;
    /// every other record keeps the raw plane at its stored size.
    ///
    /// Refuses an out-of-range index, a record with no mask, scratch past
    /// `limit` and a cancel.
    pub fn layer_mask_thumbnail_rgba8(
        &self,
        index: usize,
        max_side: u32,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<RgbaImage, String> {
        let layer = self.layer(index).ok_or("layer index is out of range")?;
        let mask = layer.mask.as_ref().ok_or("high-depth target has no mask")?;
        // A group's mask answers its canvas view, as the 8-bit
        // `layer_mask_thumbnail` does. Handed off before `begin`: the
        // control is single-use and the canvas arm begins it.
        if matches!(layer.content, Content::Group(_) | Content::PassThrough(_)) {
            return self.layer_mask_canvas_thumbnail_rgba8(index, max_side, limit, other, control);
        }
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let (w, h) = mask.plane.dimensions();
        let n = pixel_count(w, h)? as u64;
        let ((tw, th), resizing, k) = fit(w, h, max_side);
        let gray_out = u64::from(tw) * u64::from(th) * 4;
        let scratch = if !resizing {
            // The codes and their gray image.
            total(&[BOOKKEEPING_BYTES, n.checked_mul(5).ok_or(OVERFLOW)?])?
        } else if k < 2 {
            total(&[
                BOOKKEEPING_BYTES,
                n,
                resize_terms((w, h), (tw, th))?,
                gray_out,
            ])?
        } else {
            let (sw, sh) = (w / k, h / k);
            total(&[
                BOOKKEEPING_BYTES,
                u64::from(sw) * u64::from(sh),
                u64::from(sw) * 16,
                resize_terms((sw, sh), (tw, th))?,
                gray_out,
            ])?
        };
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let samples = mask.plane.samples();
        if k < 2 {
            let mut codes = reserved_vec(samples.len())?;
            for chunk in samples.chunks(EVERY_INTERVAL) {
                job.check()?;
                codes.extend(chunk.iter().map(|&v| nearest(v)));
            }
            let out = if resizing {
                gray_resized(codes, w, h, tw, th)
            } else {
                gray_image(&codes, w, h)
            };
            return out.ok_or_else(|| TOO_LARGE.into());
        }
        let (codes, sw, sh) = box_reduced(samples, w, h, k, nearest, job.range(0.0, 0.9))?
            .ok_or(NOTHING_TO_REDUCE)?;
        gray_resized(codes, sw, sh, tw, th).ok_or_else(|| TOO_LARGE.into())
    }

    /// Alpha channel `index` as the 16-bit Channels panel's row thumbnail:
    /// opaque gray RGBA8 over the canvas, aspect-fit by `fitted_dims`
    /// (never upscaled; `max_side` 0 = full size), each code read as
    /// `(v + 128) / 257`. Below 16× the thumbnail per axis (the module doc's
    /// guard) it is byte for byte that canvas gray image through the
    /// Triangle resize; above it the codes are box-reduced first, with no
    /// canvas-sized copy.
    ///
    /// Refuses an out-of-range index, scratch past `limit` and a cancel.
    pub fn channel_thumbnail_rgba8(
        &self,
        index: usize,
        max_side: u32,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<RgbaImage, String> {
        let plane = self
            .channels()
            .get(index)
            .ok_or("high-depth channel index is out of range")?
            .plane();
        let size = plane.dimensions();
        self.placed_plane_thumbnail(
            (plane.samples(), size, (0, 0), 0),
            max_side,
            limit,
            other,
            control,
        )
    }

    /// Record `index`'s mask as the 16-bit Channels panel's row thumbnail:
    /// the CANVAS view — the raw plane at its offset, its outside
    /// coverage (`· 257`) beyond it, no density, feather or enabled flag,
    /// exactly `read_plane canvas: true` — as opaque gray RGBA8, fitted and
    /// reduced as [`Self::channel_thumbnail_rgba8`].
    ///
    /// Refuses an out-of-range index, a record with no mask, scratch past
    /// `limit` and a cancel.
    pub fn layer_mask_canvas_thumbnail_rgba8(
        &self,
        index: usize,
        max_side: u32,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<RgbaImage, String> {
        let layer = self.layer(index).ok_or("layer index is out of range")?;
        let mask = layer.mask.as_ref().ok_or("high-depth target has no mask")?;
        let properties = mask.properties();
        let outside = u16::from(properties.outside_coverage) * 257;
        self.placed_plane_thumbnail(
            (
                mask.plane.samples(),
                mask.plane.dimensions(),
                properties.offset,
                outside,
            ),
            max_side,
            limit,
            other,
            control,
        )
    }

    /// The Channels row arm: `placed` = `(samples, size, origin, outside)`,
    /// a u16 plane placed on the canvas (`doc_channel::placed_row`), as an
    /// opaque gray thumbnail. Scratch, n = canvas pixels: unresized, the
    /// codes and their gray image (`5n`); below the box threshold, the codes,
    /// the resize and its gray output; above it, the box output and its two
    /// accumulator rows, the fill row, the resize and its gray output — all
    /// beside `BOOKKEEPING_BYTES`, admitted against `self.retained_bytes()`
    /// and reserved before allocating.
    fn placed_plane_thumbnail(
        &self,
        placed: (&[u16], (u32, u32), (i32, i32), u16),
        max_side: u32,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<RgbaImage, String> {
        let (samples, size, origin, outside) = placed;
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let (w, h) = self.dimensions();
        let n = pixel_count(w, h)? as u64;
        let ((tw, th), resizing, k) = fit(w, h, max_side);
        let gray_out = u64::from(tw) * u64::from(th) * 4;
        let scratch = if !resizing {
            total(&[BOOKKEEPING_BYTES, n.checked_mul(5).ok_or(OVERFLOW)?])?
        } else if k < 2 {
            total(&[
                BOOKKEEPING_BYTES,
                n,
                resize_terms((w, h), (tw, th))?,
                gray_out,
            ])?
        } else {
            let (sw, sh) = (w / k, h / k);
            total(&[
                BOOKKEEPING_BYTES,
                u64::from(sw) * u64::from(sh),
                u64::from(sw) * 16,
                u64::from(w) * 2,
                resize_terms((sw, sh), (tw, th))?,
                gray_out,
            ])?
        };
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let mut fill = reserved_vec(w as usize)?;
        fill.resize(w as usize, outside);
        let rows = |y: u32| placed_row(samples, size, origin, &fill, w, y);
        if k < 2 {
            let mut codes = reserved_vec(n as usize)?;
            for y in 0..h {
                job.check()?;
                for (_, run) in rows(y) {
                    codes.extend(run.iter().map(|&v| nearest(v)));
                }
            }
            let out = if resizing {
                gray_resized(codes, w, h, tw, th)
            } else {
                gray_image(&codes, w, h)
            };
            return out.ok_or_else(|| TOO_LARGE.into());
        }
        let (codes, sw, sh) = box_reduced_rows(w, h, k, rows, nearest, job.range(0.0, 0.9))?
            .ok_or(NOTHING_TO_REDUCE)?;
        gray_resized(codes, sw, sh, tw, th).ok_or_else(|| TOO_LARGE.into())
    }
}
