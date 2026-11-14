//! `layer_detail`, the typed edit behind Filters ▸ Other ▸ Frequency
//! Separation… wherever Apply Image's stored-code detail would not rebuild the
//! layer. The request `{"op":"layer_detail","version":1,"layer":T,"source":S}`
//! (through `dr_precision_doc_edit` and its plan; no samples) replaces raster
//! `T`'s colour with its detail against raster `S`, which is read at the same
//! CANVAS position and whose colour is 0 outside its bounds. `T`'s alpha,
//! size, offset, mask and every other property are kept, and a pixel of `T` at
//! alpha 0 is left as it is. The arithmetic is the kernel's
//! (`layer_detail`, whose module doc gives both forms and their bounds):
//!
//! - a 32-bit float document, RGB or Grayscale, takes the DIFFERENCE form
//!   (`t − s`, recombined by Linear Dodge (Add)); its pixels go through
//!   `PrecisionImage::rebuilt_rgba32_extended`, so a difference outside
//!   ±65504 refuses with that sentence and nothing is clamped;
//! - a 16-bit RGB document evaluated in linear light takes the DECODED form
//!   through the transfer its render composites with (`transfer`, so a stale
//!   transfer refuses as the render does).
//!
//! Every other document refuses with [`ENCODED_REFUSAL`]: there the detail is
//! Apply Image's Subtract at Scale 2 and Offset 127.5 levels, whose stored
//! codes the compositor's own Linear Light undoes. A layer holding a retained
//! source or a description (text, shape, fill, placed) refuses: its
//! description would no longer describe the pixels. The edit is a Pixels edit
//! under the layer's locks. A bitwise-unchanged result is `None`.
//!
//! # Admission
//! `scratch = 6·BOOKKEEPING + n·per_pixel` over `T`'s `n` pixels, where the
//! per-pixel term is the output vector beside the tiles the rebuild copies it
//! into. The figures are borrowed, not measured here: the float form holds the
//! same two n×16 buffers as `float_adjust_apply`'s apply stage, so it charges
//! that module's measured `APPLY_BYTES_PER_PIXEL` (40, 32 bytes at 5/4); the
//! decoded form holds two n×8 buffers, half of it (20). The transfer tables
//! are the document's own and already retained.
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::doc_lock::EditKind;
use crate::float_adjust_apply::APPLY_BYTES_PER_PIXEL;
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, reserved_vec, PrecisionImage, BOOKKEEPING_BYTES};
use crate::sampling_linear::LinearCodes;

/// The refusal of every document but a float one and a 16-bit RGB one
/// evaluated in linear light (module doc).
pub(crate) const ENCODED_REFUSAL: &str = "layer_detail takes a 32-bit float document or a 16-bit RGB document evaluated in linear light: in an encoded document the detail is Apply Image's Subtract of the low layer at scale 2 and offset 127.5 levels";

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum LayerDetailEdit {
    LayerDetail {
        version: u32,
        layer: usize,
        source: usize,
    },
}

/// Which form the document takes (module doc).
enum Form<'a> {
    Difference,
    Decoded(&'a dyn LinearCodes<u16>),
}

/// The validated request against one document.
struct Target<'a> {
    index: usize,
    layer: &'a PrecisionDocumentLayer,
    image: &'a PrecisionImage,
    source: &'a PrecisionImage,
    source_offset: (i32, i32),
    form: Form<'a>,
}

impl LayerDetailEdit {
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| format!("invalid layer_detail request: {e}"))
    }

    fn target<'a>(&self, doc: &'a PrecisionDocument) -> Result<Target<'a>, String> {
        let Self::LayerDetail {
            version,
            layer,
            source,
        } = *self;
        if version != 1 {
            return Err("unsupported layer_detail request version".into());
        }
        let form = if doc.is_float() {
            Form::Difference
        } else if doc.is_gray() || doc.is_lab() || doc.is_cmyk() || doc.is_indexed() {
            return Err(ENCODED_REFUSAL.into());
        } else {
            match doc.transfer()? {
                Some(transfer) => Form::Decoded(&**transfer),
                None => return Err(ENCODED_REFUSAL.into()),
            }
        };
        if layer == source {
            return Err("layer_detail reads its source from another layer".into());
        }
        let raster = |index: usize, what: &str| -> Result<_, String> {
            let record = doc
                .layer(index)
                .ok_or_else(|| format!("layer_detail {what} index is out of range"))?;
            let image = record
                .source()
                .ok_or_else(|| format!("layer_detail {what} {index} has no pixels of its own"))?;
            Ok((record, image))
        };
        let (record, image) = raster(layer, "layer")?;
        let (source_record, source_image) = raster(source, "source")?;
        if record.retained_source.is_some() || record.description.is_some() {
            return Err(format!(
                "layer_detail layer {layer} is a text, shape, fill or placed layer: rasterize it first"
            ));
        }
        Ok(Target {
            index: layer,
            layer: record,
            image,
            source: source_image,
            source_offset: source_record.offset(),
            form,
        })
    }

    /// The plan (module doc, `# Admission`); no samples.
    pub(crate) fn plan(&self, doc: &PrecisionDocument) -> Result<Value, String> {
        let target = self.target(doc)?;
        let (w, h) = target.image.dimensions();
        let per_pixel = match target.form {
            Form::Difference => APPLY_BYTES_PER_PIXEL,
            Form::Decoded(_) => APPLY_BYTES_PER_PIXEL / 2,
        };
        let scratch = (u64::from(w) * u64::from(h))
            .checked_mul(per_pixel)
            .and_then(|bytes| bytes.checked_add(6 * BOOKKEEPING_BYTES))
            .ok_or("layer_detail scratch overflow")?;
        Ok(json!({"retained_bytes":doc.retained_bytes(),"scratch_bytes":scratch,"input_samples":0}))
    }

    pub(crate) fn apply(
        &self,
        doc: &PrecisionDocument,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        let scratch = self.plan(doc)?["scratch_bytes"]
            .as_u64()
            .unwrap_or(u64::MAX);
        admit(doc.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let job = job.with_reservation_floor(scratch);
        let Self::LayerDetail { layer, .. } = *self;
        let out = doc.under_locks(layer, EditKind::Pixels, |doc| self.unlocked(doc, job))?;
        job.report(1., 5)?;
        Ok(out)
    }

    fn unlocked(
        &self,
        doc: &PrecisionDocument,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        let target = self.target(doc)?;
        let (w, h) = target.image.dimensions();
        let offset = target.layer.offset();
        let (sw, sh) = target.source.dimensions();
        let width = w as usize;
        // `T`'s pixel `i` read in `S`'s raster at the same canvas position,
        // or `None` outside it.
        let at = |i: usize| -> Option<(u32, u32)> {
            let x = i64::from(offset.0) + (i % width) as i64 - i64::from(target.source_offset.0);
            let y = i64::from(offset.1) + (i / width) as i64 - i64::from(target.source_offset.1);
            (x >= 0 && y >= 0 && x < i64::from(sw) && y < i64::from(sh))
                .then_some((x as u32, y as u32))
        };
        let n = width * h as usize;
        let image = match target.form {
            Form::Difference => {
                let subtract = crate::layer_detail::float_subtract()
                    .ok_or("internal error: the float Subtract is not separable")?;
                let runs = target
                    .image
                    .runs_rgba32()
                    .ok_or("layer_detail requires f32 storage in a 32-bit float document")?;
                let mut pixels = reserved_vec(n)?;
                let mut changed = false;
                for t in runs.flatten() {
                    let i = pixels.len();
                    job.every(i)?;
                    let out = if t[3] == 0.0 {
                        *t
                    } else {
                        let s = at(i)
                            .and_then(|(x, y)| target.source.sample_at(x, y))
                            .unwrap_or([0.0; 4]);
                        crate::layer_detail::difference(subtract, *t, s)
                    };
                    changed |= out.map(f32::to_bits) != t.map(f32::to_bits);
                    pixels.push(out);
                }
                job.check()?;
                if !changed {
                    return Ok(None);
                }
                target
                    .image
                    .rebuilt_rgba32_extended(w, h, pixels, Arc::clone(doc.profile()))?
            }
            Form::Decoded(codes) => {
                let blend = crate::layer_detail::decoded_blend()?;
                let runs = target
                    .image
                    .runs_rgba16()
                    .ok_or("layer_detail requires u16 storage in a 16-bit document")?;
                let mut pixels = reserved_vec(n)?;
                let mut changed = false;
                for t in runs.flatten() {
                    let i = pixels.len();
                    job.every(i)?;
                    let out = if t[3] == 0 {
                        *t
                    } else {
                        let s = at(i)
                            .and_then(|(x, y)| target.source.rgba16_at(x, y))
                            .unwrap_or([0; 4]);
                        crate::layer_detail::decoded(blend, codes, *t, s)
                    };
                    changed |= out != *t;
                    pixels.push(out);
                }
                job.check()?;
                if !changed {
                    return Ok(None);
                }
                target
                    .image
                    .rebuilt_rgba16(w, h, pixels, Arc::clone(doc.profile()))?
            }
        };
        doc.replacing_content(target.index, Content::Raster { image, offset })
    }
}
