//! The Duotone document mode (#547, `designs/duotone-mode.md`): a Grayscale
//! document whose `GrayPolicy` carries a Duotone description
//! (`duotone_ink`). Its pixels are the Gray document's, 8 or 16 bits, layers
//! and all; every edit a Gray document admits runs unchanged on the gray
//! channel (`gray_document`'s gate). This module is the adapter: whether a
//! document is Duotone, its options, Image ▸ Mode ▸ Duotone…'s plan and edit
//! (set or replace the description, every sample shared), the view and the
//! refusals the mode adds to Gray's. Duotone → Grayscale renders the inks
//! into the gray (`duotone_gray`).
//!
//! # The view
//!
//! `duotone_ink`'s mix, per gray code: a 65 536-entry table by the
//! composite's R code (u16), each entry the mix's PCS XYZ through the twin's
//! matrix (`MatrixTrc::pcs_to_linear`, clamped to [0, 1]) and its encoder
//! (`GrayPolicy::encode_linear`), so the RGBA16 result is in the document's
//! own profile (the twin), alpha kept, and the host draws it as any RGB
//! projection. The table ([`DuotoneViewTable`]) is built once a call and
//! mapped over every image the call covers: the view and an export map one,
//! the render (`duotone_convert`, phase 2a) maps every raster leaf through
//! the one table it builds on its source.
//!
//! # The thumbnail
//!
//! A layer's or the composite's thumbnail (the Layers panel, the Channels
//! row; phase 2a, documented and unmeasured until phase 2b's menu walk) is
//! `layer_thumbnail::fit_u16` at u16 first (the fit-first rule, never
//! upscaled), then the table over the fitted pixels only, quantized once
//! without dither (`lab_rgb::lab_thumbnail_rgba8`'s shape): RGBA8 in the
//! document's profile, so a thumbnail is the view of the same pixels.
//!
//! # Memory
//!
//! Image ▸ Mode ▸ Duotone… shares every sample, so its plan is the host's
//! display preparation of the result: canvas × [`TO_DUOTONE_BYTES_PER_PIXEL`]
//! and bookkeeping, held by `scripts/duotone-admission/measure.py` to
//! ⌈1.25 × the measured peak⌉; a miss raises this term. The term is
//! measured, not Gray's conversion term (41) plus the view's 8: no sample is
//! converted, so the peak is the display preparation alone, 576 MB at 24 MP
//! (24.0 B/px, every case); ⌈1.25 ×⌉ less bookkeeping is 29.98 B/px, and 31
//! keeps a byte a pixel above it for run-to-run spread. The view
//! reserves its output at [`VIEW_BYTES_PER_PIXEL`], the table
//! ([`VIEW_TABLE_BYTES`]) and bookkeeping; the display charges the larger of
//! that and the view held beside its preview, Lab's shape
//! ([`display_scratch_bytes`]).

use std::sync::Arc;

use serde_json::{json, Value};

use crate::duotone_ink::DuotoneInks;
use crate::gray_document::{GrayDepth, GrayPolicy};
use crate::job_control::{DrJobControl, JobContext};
use crate::layer_thumbnail::{fit_u16, fit_u16_scratch, fitted_dims};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{
    admit, PrecisionImage, PrecisionRun, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::precision_quantize::PrecisionDither;
use crate::tile_store::TilePlane;

/// Bytes Image ▸ Mode ▸ Duotone… charges a canvas pixel (module doc).
pub(crate) const TO_DUOTONE_BYTES_PER_PIXEL: u64 = 31;

/// Bytes the view charges a pixel: its RGBA16 output.
pub(crate) const VIEW_BYTES_PER_PIXEL: u64 = 8;

/// The view's table: one RGB16 entry per gray code.
pub(crate) const VIEW_TABLE_BYTES: u64 = 65_536 * 6;

/// The refusal of a source that is not an 8- or 16-bit Grayscale document.
const SOURCE_REFUSAL: &str =
    "Duotone converts an 8- or 16-bit Grayscale document; convert to Grayscale first.";

/// The refusal of a 32-bit Grayscale source.
const FLOAT_REFUSAL: &str =
    "Duotone converts an 8- or 16-bit Grayscale document; convert to 8 or 16 Bits/Channel first.";

/// The refusal of the view on any document but a Duotone one.
const NOT_DUOTONE: &str = "the Duotone view takes a Duotone document";

/// The refusal of 32 Bits/Channel on a Duotone document (`gray_profile`),
/// for good, beside [`ASSIGN_REFUSAL`], [`CONVERT_REFUSAL`] and
/// [`DEPTH16_REFUSAL`].
pub(crate) const DEPTH32_REFUSAL: &str =
    "32 Bits/Channel is unavailable in a Duotone document: the 32-bit modes are RGB and Grayscale.";

/// The refusal of Assign Profile on a Duotone document (`gray_profile`).
pub(crate) const ASSIGN_REFUSAL: &str = "Assign Profile is not available in Duotone.";

/// The refusal of Convert to Profile on a Duotone document (`gray_profile`).
pub(crate) const CONVERT_REFUSAL: &str = "Convert to Profile is not available in Duotone.";

/// The refusal of 8 → 16 Bits/Channel on a Duotone document
/// (`gray_profile`); 16 → 8 keeps the inks.
pub(crate) const DEPTH16_REFUSAL: &str = "16 Bits/Channel is not available in Duotone.";

/// Bytes a fitted thumbnail pixel charges after the fit: its view at u16
/// (8) and its quantized RGBA8 (4), `lab_rgb`'s thumbnail shape.
const THUMB_BYTES_PER_PIXEL: u64 = 12;

/// The view's table (module doc): one RGB16 entry per gray code, built once
/// by [`PrecisionDocument::duotone_view_table`] and mapped over any number
/// of images by [`PrecisionDocument::duotone_view_mapped`].
pub(crate) struct DuotoneViewTable(Vec<[u16; 3]>);

/// What the view reserves for a `width` × `height` image.
pub(crate) fn view_scratch_bytes(width: u32, height: u32) -> u64 {
    (u64::from(width) * u64::from(height))
        .saturating_mul(VIEW_BYTES_PER_PIXEL)
        .saturating_add(VIEW_TABLE_BYTES)
        .saturating_add(BOOKKEEPING_BYTES)
}

/// A Duotone document's display working memory
/// (`PrecisionDocument::display_scratch_bytes`): the view, or the view held
/// beside the preview of it, whichever is larger (Lab's shape).
pub(crate) fn display_scratch_bytes(width: u32, height: u32) -> Result<u64, String> {
    let preview = crate::precision_image::rgba8_preview_scratch(width, height)?;
    let held = (u64::from(width) * u64::from(height))
        .checked_mul(VIEW_BYTES_PER_PIXEL)
        .and_then(|bytes| bytes.checked_add(preview))
        .ok_or("display working-memory calculation overflow")?;
    Ok(view_scratch_bytes(width, height).max(held))
}

impl PrecisionDocument {
    /// True for a Duotone document.
    pub fn is_duotone(&self) -> bool {
        self.gray_policy().is_some_and(|p| p.duotone().is_some())
    }

    /// The normalized options (`DuotoneInks::to_json`); `None` for every
    /// other document.
    pub fn duotone_options(&self) -> Option<Value> {
        self.gray_policy()?.duotone().map(|inks| inks.to_json())
    }

    /// The Gray policy `inks` makes, or `Ok(None)` when it changes nothing.
    /// A source that is not an 8- or 16-bit Grayscale document refuses by
    /// name.
    fn duotone_policy(&self, inks: DuotoneInks) -> Result<Option<GrayPolicy>, String> {
        // A Bitmap document is Gray storage too (`bitmap_document`), but not
        // a Grayscale document, so its refusal asks for the Grayscale step first.
        if self.is_bitmap() {
            return Err(SOURCE_REFUSAL.into());
        }
        let policy = self.gray_policy().ok_or(SOURCE_REFUSAL)?;
        if policy.depth() == GrayDepth::ThirtyTwo {
            return Err(FLOAT_REFUSAL.into());
        }
        if policy.duotone().map(|current| &**current) == Some(&inks) {
            return Ok(None);
        }
        Ok(Some(policy.with_duotone(Some(Arc::new(inks)))))
    }

    /// Image ▸ Mode ▸ Duotone…'s plan:
    /// `{"working_memory_plan_bytes","refusal","changes"}`. Malformed
    /// options are the `Err`; a source the mode refuses is the `refusal`.
    /// Duotone → Grayscale is `duotone_gray`'s.
    pub fn set_duotone_plan(&self, options: &str) -> Result<Value, String> {
        let inks = DuotoneInks::parse(options)?;
        let (refusal, changes) = match self.duotone_policy(inks) {
            Ok(next) => (None, next.is_some()),
            Err(reason) => (Some(reason), false),
        };
        let canvas = u64::from(self.width) * u64::from(self.height);
        Ok(json!({
            "working_memory_plan_bytes":
                canvas.saturating_mul(TO_DUOTONE_BYTES_PER_PIXEL).saturating_add(BOOKKEEPING_BYTES),
            "refusal": refusal,
            "changes": changes,
        }))
    }

    /// Image ▸ Mode ▸ Duotone…: set or replace the description, every
    /// sample shared; `Ok(None)` when nothing changes.
    pub fn set_duotone(&self, options: &str) -> Result<Option<Self>, String> {
        let inks = DuotoneInks::parse(options)?;
        let Some(policy) = self.duotone_policy(inks)? else {
            return Ok(None);
        };
        self.clone().with_gray(Arc::new(policy)).map(Some)
    }

    /// The Duotone view of `image` (a projection or layer source of this
    /// document, RGBA16 in its twin): module doc. A per-pixel map, so a
    /// region or a layer works as the whole composite does. Refuses a
    /// document that is not Duotone, an image that is not RGBA16, scratch
    /// past `limit` and a cancel.
    pub fn duotone_view(
        &self,
        image: &PrecisionImage,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<PrecisionImage, String> {
        let policy = self.gray_policy().ok_or(NOT_DUOTONE)?;
        policy.duotone().ok_or(NOT_DUOTONE)?;
        if image.storage() != PrecisionStorage::Rgba16Unorm {
            return Err("the Duotone view takes a 16-bit image".into());
        }
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let (w, h) = image.dimensions();
        let scratch = view_scratch_bytes(w, h);
        admit(image.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        self.duotone_view_job(image, job)
    }

    /// [`Self::duotone_view`]'s body under `job`, for a caller that has
    /// already admitted and reserved [`view_scratch_bytes`] (the exports,
    /// phase 1b): the table, then the map.
    pub(crate) fn duotone_view_job(
        &self,
        image: &PrecisionImage,
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        if image.storage() != PrecisionStorage::Rgba16Unorm {
            return Err("the Duotone view takes a 16-bit image".into());
        }
        let table = self.duotone_view_table(job)?;
        self.duotone_view_mapped(image, &table, job)
    }

    /// The view's table of this Duotone document (module doc): built once a
    /// call, so a caller with many images (the render's raster leaves) maps
    /// them all through one table. Admission belongs to the caller
    /// ([`VIEW_TABLE_BYTES`]).
    pub(crate) fn duotone_view_table(
        &self,
        job: JobContext<'_>,
    ) -> Result<DuotoneViewTable, String> {
        let policy = self.gray_policy().ok_or(NOT_DUOTONE)?;
        let inks = policy.duotone().ok_or(NOT_DUOTONE)?;
        let model = policy
            .twin()
            .model()
            .ok_or("the Duotone view needs a matrix/TRC twin")?;
        let splines = inks.coverages();
        let decode = |code: f64| policy.decode_unit(code);
        let mut table: Vec<[u16; 3]> = Vec::new();
        table
            .try_reserve_exact(65_536)
            .map_err(|_| "could not allocate the Duotone view table")?;
        let wide = |unit: f32| (f64::from(unit).clamp(0.0, 1.0) * 65535.0).round() as u16;
        for g in 0..=u16::MAX {
            let k = 1.0 - f64::from(g) / 65535.0;
            let xyz = inks.mix_xyz(&splines, k, &decode);
            let linear = model.pcs_to_linear(xyz).map(|v| v.clamp(0.0, 1.0));
            table.push(policy.encode_linear(linear).map(wide));
            if g % 4096 == 0 {
                job.check()?;
            }
        }
        Ok(DuotoneViewTable(table))
    }

    /// `image` (RGBA16, a source of this document) through `table`: each
    /// pixel's R code picks its view, alpha kept, into an RGBA16 image in the
    /// document's profile. Admission belongs to the caller
    /// ([`VIEW_BYTES_PER_PIXEL`] a pixel).
    pub(crate) fn duotone_view_mapped(
        &self,
        image: &PrecisionImage,
        table: &DuotoneViewTable,
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        if image.storage() != PrecisionStorage::Rgba16Unorm {
            return Err("the Duotone view takes a 16-bit image".into());
        }
        let table = &table.0;
        let (w, h) = image.dimensions();
        let plane = TilePlane::<[u16; 4]>::from_rows_with_job(
            w,
            h,
            |y, row| {
                let mut x = 0usize;
                for (_, run) in image.row_segments(y, 0, w) {
                    let PrecisionRun::Rgba16(run) = run else {
                        return Err("the Duotone view takes a 16-bit image".into());
                    };
                    for pixel in run {
                        let [r, g, b] = table[usize::from(pixel[0])];
                        row[x] = [r, g, b, pixel[3]];
                        x += 1;
                    }
                }
                job.every(y as usize * w as usize)
            },
            job,
        )?;
        PrecisionImage::from_rgba16_tiles_unseeded(plane, self.profile().clone())
    }

    /// A Duotone layer's (or composite's) thumbnail as RGBA8 in the
    /// document's profile (module doc, "The thumbnail"): `image` fitted by
    /// `layer_thumbnail::fitted_dims` (never upscaled) at u16 first, then
    /// through the view's table, quantized once without dither. Refuses a
    /// document that is not Duotone and an image that is not u16 by the
    /// view's sentences; admits bookkeeping, the fit's scratch
    /// (`fit_u16_scratch`), [`THUMB_BYTES_PER_PIXEL`] a fitted pixel and the
    /// table against `limit` beside the image's retained bytes and `other`,
    /// and reserves them before allocating.
    pub fn duotone_thumbnail_rgba8(
        &self,
        image: &PrecisionImage,
        max_side: u32,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<image::RgbaImage, String> {
        if !self.is_duotone() {
            return Err(NOT_DUOTONE.into());
        }
        if image.storage() != PrecisionStorage::Rgba16Unorm {
            return Err("the Duotone view takes a 16-bit image".into());
        }
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let (w, h) = image.dimensions();
        let (tw, th) = fitted_dims(w, h, max_side);
        let scratch = [
            BOOKKEEPING_BYTES,
            fit_u16_scratch((w, h), max_side)?,
            (u64::from(tw) * u64::from(th)).saturating_mul(THUMB_BYTES_PER_PIXEL),
            VIEW_TABLE_BYTES,
        ]
        .iter()
        .try_fold(0u64, |sum, &term| sum.checked_add(term))
        .ok_or("thumbnail working-memory calculation overflow")?;
        admit(image.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let fitted = fit_u16(image, max_side, job.range(0.0, 0.7))?;
        let table = self.duotone_view_table(job.range(0.7, 0.8))?;
        let shown = self.duotone_view_mapped(&fitted, &table, job.range(0.8, 0.95))?;
        shown.quantize_with_context(PrecisionDither::None, job.range(0.95, 1.0))
    }
}
