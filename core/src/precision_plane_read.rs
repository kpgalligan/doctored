//! Typed scalar reads. Raw planes are exact stored codes; effective masks are
//! evaluated continuously in canvas space and rounded once. FFI publication
//! stages the whole result before touching the caller's destination.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{
    admit, pixel_count, reserved_vec, PrecisionRun, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::tile_store::TILE_EDGE;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadTarget {
    Mask,
    EffectiveMask,
    Channel,
    Red,
    Green,
    Blue,
    Alpha,
    /// One ink plate of a CMYK raster, as its stored `1 − ink`
    /// (`cmyk_plate_scalar`).
    Cyan,
    Magenta,
    Yellow,
    Black,
}

impl ReadTarget {
    /// The CMYK plate a plate target names.
    fn plate(self) -> Option<usize> {
        match self {
            Self::Cyan => Some(0),
            Self::Magenta => Some(1),
            Self::Yellow => Some(2),
            Self::Black => Some(3),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaneRead {
    op: String,
    target: ReadTarget,
    index: usize,
    #[serde(default)]
    canvas: bool,
}

impl PlaneRead {
    pub fn parse(description: &str) -> Result<Self, String> {
        let out: Self = serde_json::from_str(description)
            .map_err(|e| format!("invalid high-depth plane read: {e}"))?;
        if out.op != "read_plane" {
            return Err("expected a high-depth read_plane request".into());
        }
        Ok(out)
    }
    fn stored_dimensions(&self, doc: &PrecisionDocument) -> Result<(u32, u32), String> {
        match self.target {
            ReadTarget::Channel => Ok(doc
                .channels()
                .get(self.index)
                .ok_or("high-depth channel index is out of range")?
                .plane()
                .dimensions()),
            ReadTarget::Mask | ReadTarget::EffectiveMask => {
                let mask = doc
                    .layer(self.index)
                    .and_then(|l| l.mask.as_ref())
                    .ok_or("high-depth target has no mask")?;
                Ok(if matches!(self.target, ReadTarget::Mask) {
                    mask.plane().dimensions()
                } else {
                    doc.dimensions()
                })
            }
            ReadTarget::Cyan | ReadTarget::Magenta | ReadTarget::Yellow | ReadTarget::Black => {
                Ok(doc.cmyk_plate_geometry(self.index)?.0)
            }
            _ => Ok(doc
                .layer_source(self.index)
                .ok_or("high-depth plane read requires a raster source")?
                .dimensions()),
        }
    }
    pub(crate) fn dimensions(&self, doc: &PrecisionDocument) -> Result<(u32, u32), String> {
        let stored = self.stored_dimensions(doc)?;
        Ok(if self.canvas {
            doc.dimensions()
        } else {
            stored
        })
    }
    pub(crate) fn offset(&self, doc: &PrecisionDocument) -> Result<(i32, i32), String> {
        Ok(match self.target {
            ReadTarget::Channel | ReadTarget::EffectiveMask => (0, 0),
            ReadTarget::Mask => {
                doc.layer(self.index)
                    .and_then(|l| l.mask())
                    .ok_or("target has no mask")?
                    .properties()
                    .offset
            }
            ReadTarget::Cyan | ReadTarget::Magenta | ReadTarget::Yellow | ReadTarget::Black => {
                doc.cmyk_plate_geometry(self.index)?.1
            }
            _ => doc.layer(self.index).ok_or("invalid plane layer")?.offset(),
        })
    }
    fn inspection_mask(
        &self,
        doc: &PrecisionDocument,
    ) -> Result<crate::precision_mask::PrecisionMask, String> {
        let mask = doc
            .layer(self.index)
            .and_then(|l| l.mask.as_ref())
            .ok_or("high-depth target has no mask")?;
        mask.with_properties(mask.properties().clone(), true)
    }
    pub fn plan(&self, doc: &PrecisionDocument) -> Result<Value, String> {
        // A float layer's components are f32: refuse before any reservation,
        // with `read_with_context`'s own sentence (the depth-32 `read_plane` row).
        if matches!(
            self.target,
            ReadTarget::Red | ReadTarget::Green | ReadTarget::Blue | ReadTarget::Alpha
        ) && doc
            .layer_source(self.index)
            .is_some_and(|image| image.storage() == PrecisionStorage::Rgba32Float)
        {
            return Err("high-depth plane read requires u16 storage".into());
        }
        let (w, h) = self.dimensions(doc)?;
        let n = pixel_count(w, h)? as u64;
        let extra = if matches!(self.target, ReadTarget::EffectiveMask) {
            self.inspection_mask(doc)?.plan()?.0
        } else {
            0
        };
        let raw = self.stored_dimensions(doc)?;
        let raw_pixels = u64::from(raw.0) * u64::from(raw.1);
        // A plate read holds the layer rect's whole CMYKA16 `read_rect` copy
        // beside its u16 output (`cmyk_plate_samples`), as `ScalarEdit::plan`
        // charges for the same read.
        let plate = if self.target.plate().is_some() {
            raw_pixels * crate::cmyk_image::CMYK_BYTES_PER_PIXEL
        } else {
            0
        };
        let scratch = 4 * BOOKKEEPING_BYTES
            + n * 4
            + extra
            + plate
            + if self.canvas { raw_pixels * 2 } else { 0 };
        Ok(
            json!({"retained_bytes":doc.retained_bytes(),"scratch_bytes":scratch,
            "input_samples":0,"output_samples":n,"width":w,"height":h}),
        )
    }
    pub fn read(
        &self,
        doc: &PrecisionDocument,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Vec<u16>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        self.read_with_context(doc, limit, other, job)
    }
    pub(crate) fn read_with_context(
        &self,
        doc: &PrecisionDocument,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Vec<u16>, String> {
        admit(doc.retained_bytes(), 4 * BOOKKEEPING_BYTES, other, limit)?;
        job.reserve(4 * BOOKKEEPING_BYTES)?;
        let plan = self.plan(doc)?;
        let scratch = plan["scratch_bytes"]
            .as_u64()
            .ok_or("invalid scalar read plan")?;
        admit(doc.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let (w, h) = self.stored_dimensions(doc)?;
        let n = pixel_count(w, h)?;
        let mut out = reserved_vec(n)?;
        match self.target {
            ReadTarget::Mask | ReadTarget::Channel => {
                let values = if matches!(self.target, ReadTarget::Mask) {
                    doc.layer(self.index)
                        .and_then(|l| l.mask.as_ref())
                        .ok_or("high-depth target has no mask")?
                        .plane()
                        .samples()
                } else {
                    doc.channels()
                        .get(self.index)
                        .ok_or("high-depth channel index is out of range")?
                        .plane()
                        .samples()
                };
                for chunk in values.chunks(4096) {
                    job.check()?;
                    out.extend_from_slice(chunk);
                }
            }
            ReadTarget::EffectiveMask => {
                let inspection = self.inspection_mask(doc)?;
                let prepared = inspection.prepare(job)?;
                for y in 0..h {
                    job.check()?;
                    for x in 0..w {
                        job.every(y as usize * w as usize + x as usize)?;
                        out.push(
                            (prepared.at(i64::from(x), i64::from(y)).clamp(0.0, 1.0) * 65535.0)
                                .round() as u16,
                        );
                    }
                }
            }
            ReadTarget::Cyan | ReadTarget::Magenta | ReadTarget::Yellow | ReadTarget::Black => {
                out = doc.cmyk_plate_samples(self.index, self.target.plate().unwrap_or(0), job)?;
            }
            _ => {
                let channel = match self.target {
                    ReadTarget::Red => 0,
                    ReadTarget::Green => 1,
                    ReadTarget::Blue => 2,
                    _ => 3,
                };
                let image = doc
                    .layer_source(self.index)
                    .ok_or("high-depth plane read requires a raster source")?;
                if image.storage() != PrecisionStorage::Rgba16Unorm {
                    return Err("high-depth plane read requires u16 storage".into());
                }
                // TILE-BAND order (each 256-row band, each 256-column span,
                // each row's run) into the pre-sized plane by flat index;
                // `job.every` reads only the visit counter, so the
                // checkpoints are the flat walk's (13B-3e-3a).
                out.resize(n, 0);
                let mut visited = 0_usize;
                for band in (0..h).step_by(TILE_EDGE as usize) {
                    for x0 in (0..w).step_by(TILE_EDGE as usize) {
                        let x1 = x0.saturating_add(TILE_EDGE).min(w);
                        for y in band..band.saturating_add(TILE_EDGE).min(h) {
                            for (x, run) in image.row_segments(y, x0, x1) {
                                let PrecisionRun::Rgba16(run) = run else {
                                    return Err("high-depth plane read requires u16 storage".into());
                                };
                                let row = y as usize * w as usize + x as usize;
                                for (k, pixel) in run.iter().enumerate() {
                                    job.every(visited)?;
                                    visited += 1;
                                    out[row + k] = pixel[channel];
                                }
                            }
                        }
                    }
                }
            }
        }
        if self.canvas && !matches!(self.target, ReadTarget::EffectiveMask | ReadTarget::Channel) {
            let (cw, ch) = doc.dimensions();
            let offset = self.offset(doc)?;
            let outside = if matches!(self.target, ReadTarget::Mask) {
                u16::from(
                    doc.layer(self.index)
                        .and_then(|l| l.mask())
                        .ok_or("target has no mask")?
                        .properties()
                        .outside_coverage,
                ) * 257
            } else if self.target.plate().is_some() {
                // Outside the layer rect there is no ink: paper, `1 − 0`.
                65535
            } else {
                0
            };
            let mut placed = reserved_vec(pixel_count(cw, ch)?)?;
            for y in 0..ch {
                job.check()?;
                for x in 0..cw {
                    job.every(y as usize * cw as usize + x as usize)?;
                    let sx = i64::from(x) - i64::from(offset.0);
                    let sy = i64::from(y) - i64::from(offset.1);
                    placed.push(
                        if sx < 0 || sy < 0 || sx >= i64::from(w) || sy >= i64::from(h) {
                            outside
                        } else {
                            out[sy as usize * w as usize + sx as usize]
                        },
                    );
                }
            }
            out = placed;
        }
        job.report(1.0, 5)?;
        Ok(out)
    }
}
