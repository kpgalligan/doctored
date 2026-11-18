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
//! into: two n×16 buffers in the difference form
//! ([`DIFFERENCE_BYTES_PER_PIXEL`]), two n×8 in the decoded form
//! ([`DECODED_BYTES_PER_PIXEL`]). The transfer tables are the document's own
//! and already retained. Measured 2026-11-18 (Apple M2 Max, release, one case
//! per process, three runs, `MallocLargeCache=0`, E =
//! `ri_interval_max_phys_footprint` − pre, a baseline twin per form reading
//! E 0; `tests/layer_detail_24mp.rs::measure`): a 6000×4000 sRGB document of
//! two opaque canvas-sized layers, layer 1 detailed against layer 0, every
//! pixel changed; the difference form in a float document (the target's
//! ramps over [−0.25, 3.75] and [0.02, 2.02] and a hashed blue in [0, 2]),
//! the decoded form in a 16-bit document evaluated in linear light (the same
//! pictures as codes over the full range).
//!
//! | case | E (three runs) | retained | scratch | scratch / max E |
//! |---|---|---|---|---|
//! | difference | 769,933,888 / 769,933,888 / 769,901,120 | 769,641,201 | 966,291,456 | 1.255 |
//! | decoded | 385,024,264 / 385,024,264 / 385,024,264 | 386,425,913 | 486,291,456 | 1.263 |
//!
//! E is 32 and 16 bytes a pixel plus about 2 MB and 1 MB. Each figure is the
//! least whole one whose 24 MP plan covers ⌈1.25 × max E⌉: 962,417,360 needs
//! 39.84 bytes a pixel beside the bookkeeping, so 40; 481,280,330 needs 19.79,
//! so 20. `layer_detail_24mp` pins both floors. The difference case's
//! retained + scratch, 1,735,932,657, and the decoded case's, 872,717,369, are
//! within the 2 GiB test pin (2,147,483,648), so both are admitted there.
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::doc_lock::EditKind;
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, reserved_vec, PrecisionImage, BOOKKEEPING_BYTES};
use crate::sampling_linear::LinearCodes;

/// Scratch bytes per target pixel of the difference form (module doc's `# Admission`).
const DIFFERENCE_BYTES_PER_PIXEL: u64 = 40;
/// Scratch bytes per target pixel of the decoded form (module doc's `# Admission`).
const DECODED_BYTES_PER_PIXEL: u64 = 20;

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
            Form::Difference => DIFFERENCE_BYTES_PER_PIXEL,
            Form::Decoded(_) => DECODED_BYTES_PER_PIXEL,
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
