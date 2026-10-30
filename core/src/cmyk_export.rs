//! The export workspace on a CMYK document (#1 phase 5): Export As,
//! Quick Export, Save for Web and Batch convert to RGB, or keep CMYK where
//! the format can hold it. `doc_export`'s typed `export_plan` and
//! `export_encoded` hand a CMYK document here.
//!
//! * **Kept CMYK** — profile `document` with JPEG or TIFF: the composite on
//!   paper through `cmyk_export_ink`'s stages (resize on ink, PLATE-L output
//!   sharpen, the depth step), encoded by `cmyk_io::cmyk_tiff_bytes` behind
//!   the request's metadata policy (`doc_color::image_sidecar_with_policy`
//!   at the OUTPUT dimensions), the CMYK profile embedded on request. A TIFF
//!   is final at `bit_depth` 8 or 16 (a 16-bit document's 8-bit TIFF rounds,
//!   an 8-bit document's 16-bit TIFF is its codes ×257); a JPEG is the 8-bit
//!   CMYK TIFF intermediate PENDING the host's CMYK transcode
//!   (`HostImageEncoder.cmykJPEG`, run by `ExportCapture.hostFinish`), and
//!   `info_json` says `"cmyk": true`. A default request is still the CMYK
//!   save's bytes (`cmyk_io`'s `cmyk_encoded`). Two requests refuse by name:
//!   a TIFF `bit_depth` other than 8 or 16, and a dithered 8-bit TIFF of a
//!   16-bit document (the depth step never dithers). A JPEG, and an 8-bit
//!   TIFF of an 8-bit document, ignore `dither` (no step rounds there).
//! * **Converted** — every other format with `document`, and `srgb`,
//!   `display-p3` or `file` with any format: the composite flattened on
//!   paper (`cmyk_composite::on_paper`; with spot channels, the on-paper
//!   image they are overprinted on, `spot_cmyk`) goes through
//!   `cmyk_color::CmykToRgb` at the request's intent and BPC into a u16 RGB
//!   image in the destination (`document` is sRGB), and the ordinary typed
//!   stages (`PrecisionDocument::export_stages`: resample, sharpen, reduce,
//!   encode, every request field honoured) run over it with nothing left to
//!   convert. The plan is that conversion's bytes plus `export_plan_for`'s
//!   own terms.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::cmyk_color::{ink_key, CmykToRgb, ConversionOptions};
use crate::cmyk_composite::{code, on_paper};
use crate::cmyk_document::CmykDepth;
use crate::doc_export::{output_dimensions, ExportRequest, ExportResult, OutputProfile};
use crate::dr_image::Format;
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, pixel_count, PrecisionImage, BOOKKEEPING_BYTES};
use crate::precision_quantize::PrecisionDither;
use crate::soft_proof::memo_peak_bytes_for;
use crate::tile_store::TilePlane;

/// Bytes a canvas pixel a kept-CMYK export charges beside the save's own
/// figure (`cmyk_save_scratch_bytes`): the result's bytes and the host's
/// copy and transcode of them (`export-cmyk-tiff-keep`); a miss raises this
/// term.
pub(crate) const KEEP_BYTES_PER_PIXEL: u64 = 16;

/// Bytes a canvas pixel the conversion to RGB charges beside the stages'
/// own plan: the u16 RGB image (8 B/px) and its document's bookkeeping
/// (`export-cmyk-png`); a miss raises this term.
pub(crate) const TO_RGB_EXPORT_BYTES_PER_PIXEL: u64 = 12;

/// True when `request` keeps CMYK (module doc).
fn keeps_cmyk(request: &ExportRequest) -> bool {
    matches!(request.profile, OutputProfile::Document)
        && matches!(request.format, Format::Jpeg | Format::Tiff)
}

/// The RGB destination of a converting export (module doc).
fn destination(request: &ExportRequest) -> Arc<IccProfile> {
    match &request.profile {
        OutputProfile::Document | OutputProfile::Srgb => IccProfile::srgb(),
        OutputProfile::DisplayP3 => IccProfile::display_p3(),
        OutputProfile::File { profile, .. } => profile.clone(),
    }
}

/// `request` with its convert stage made a no-op: the pixels the stages
/// receive are already in the destination.
fn staged(request: &ExportRequest) -> ExportRequest {
    ExportRequest {
        format: request.format,
        jpeg_quality: request.jpeg_quality,
        bit_depth: request.bit_depth,
        dither: request.dither,
        size: request.size,
        filter: request.filter,
        profile: OutputProfile::Document,
        intent: request.intent,
        black_point_compensation: request.black_point_compensation,
        sharpen: request.sharpen,
        embed_profile: request.embed_profile,
        strip_metadata: request.strip_metadata,
        metadata: request.metadata.clone(),
        float_samples: request.float_samples,
        hdr_encoding: request.hdr_encoding,
        // The converted and kept CMYK exports composite (`tiff_spot`).
        keep_spots: false,
    }
}

impl PrecisionDocument {
    /// Why a kept-CMYK `request` cannot be written (module doc).
    fn cmyk_keep_refusal(&self, request: &ExportRequest) -> Option<String> {
        let bits = self.cmyk_policy()?.depth().bits();
        if request.format != Format::Tiff {
            return None;
        }
        if !matches!(request.bit_depth, 8 | 16) {
            return Some("a CMYK TIFF is written at 8 or 16 bits; set bit_depth to 8 or 16".into());
        }
        if request.bit_depth == 8 && bits == 16 && request.dither != PrecisionDither::None {
            return Some(
                "a CMYK TIFF that keeps CMYK rounds 16 bits to 8 without dithering; set dither to none"
                    .into(),
            );
        }
        None
    }

    /// The typed `export_plan` of a CMYK document (module doc):
    /// `{"retained_bytes","scratch_bytes","width","height"}`.
    pub(crate) fn cmyk_export_plan(&self, request: &ExportRequest) -> Result<Value, String> {
        let (w, h) = self.dimensions();
        let canvas = pixel_count(w, h)? as u64;
        if keeps_cmyk(request) {
            if let Some(reason) = self.cmyk_keep_refusal(request) {
                return Err(reason);
            }
            let (ow, oh) = output_dimensions((w, h), request)?;
            let scratch = self.cmyk_kept_scratch_bytes(request)?;
            return Ok(
                json!({"retained_bytes":self.retained_bytes(),"scratch_bytes":scratch,
                "width":ow,"height":oh}),
            );
        }
        if let Some(refusal) = request.destination_refusal(self.profile()) {
            return Err(refusal);
        }
        let mut plan = self.export_plan_for((w, h), None, &staged(request))?;
        let conversion = canvas * TO_RGB_EXPORT_BYTES_PER_PIXEL
            + u64::from(w) * 20
            + self.cmyk_stack_scratch_bytes()?
            + memo_peak_bytes_for::<[f32; 3]>(canvas)
            + BOOKKEEPING_BYTES
            // The on-paper image the spot overprint rows from (`spot_cmyk`).
            + if self.has_spot_channel() {
                self.cmyk_spot_scratch_bytes()
            } else {
                0
            };
        let scratch = plan["scratch_bytes"].as_u64().unwrap_or(0) + conversion;
        plan["scratch_bytes"] = json!(scratch);
        Ok(plan)
    }

    /// The typed `export_encoded` of a CMYK document (module doc), admitted
    /// by [`Self::cmyk_export_plan`].
    pub(crate) fn cmyk_export_encoded(
        &self,
        request: &ExportRequest,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<ExportResult, String> {
        let plan = self.cmyk_export_plan(request)?;
        let retained = plan["retained_bytes"].as_u64().unwrap_or(0);
        let scratch = plan["scratch_bytes"].as_u64().unwrap_or(0);
        admit(retained, scratch, other, limit)?;
        job.check()?;
        job.reserve(scratch)?;
        let policy = self.require_cmyk_policy("A CMYK export")?.clone();
        let (w, h) = self.dimensions();
        if keeps_cmyk(request) {
            let jpeg = request.format == Format::Jpeg;
            let depth = if jpeg || request.bit_depth == 8 {
                CmykDepth::Eight
            } else {
                CmykDepth::Sixteen
            };
            let (dimensions, samples) =
                self.cmyk_kept_samples(request, depth, job.range(0.0, 0.8))?;
            // The save's sidecar (`cmyk_io`) under the request's policy, at
            // the output size, with the CMYK profile in place of the view's.
            let (mut side, mut carried, mut outcome) = crate::doc_color::image_sidecar_with_policy(
                self.profile(),
                self.metadata(),
                self.resolution(),
                Format::Tiff,
                false,
                request.strip_metadata,
                dimensions,
                &request.metadata,
            );
            side.icc = request.embed_profile.then_some(policy.profile_bytes());
            if request.embed_profile {
                carried |= crate::doc_color::CARRIES_PROFILE;
                outcome.profile = true;
            }
            let bytes = crate::cmyk_io::cmyk_tiff_bytes(&samples, dimensions, depth, &side)?;
            drop(samples);
            job.report(1.0, 5)?;
            return ExportResult::cmyk_kept(
                bytes,
                dimensions,
                carried,
                depth.bits(),
                request,
                policy.name().to_string(),
                outcome,
                jpeg,
            );
        }
        let to = destination(request);
        let mut leg = CmykToRgb::new(
            &policy.profile,
            &to,
            ConversionOptions {
                intent: request.intent,
                black_point_compensation: request.black_point_compensation,
            },
        )?;
        let c = |v: f32| (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
        let part = job.range(0.0, 0.4);
        let spots = self.spot_indices();
        let plane = if spots.is_empty() {
            let mut stack = self.cmyk_stack(None, job.range(0.0, 0.4))?;
            let mut acc = vec![[0f32; 5]; w as usize];
            TilePlane::<[u16; 4]>::from_rows_with_job(
                w,
                h,
                |y, row| {
                    stack.row(i64::from(y), 0, &mut acc);
                    for (dst, pixel) in row.iter_mut().zip(acc.iter()) {
                        let ink = on_paper(*pixel).map(code);
                        let rgb = leg.rgb(ink_key(ink), ink.map(|v| f64::from(v) / 65535.0));
                        *dst = [c(rgb[0]), c(rgb[1]), c(rgb[2]), 65535];
                    }
                    part.every(y as usize * w as usize)
                },
                part,
            )?
        } else {
            // The spot inks overprinted on paper (`spot_cmyk`), then the
            // same conversion row by row.
            let printed = self.cmyk_spot_on_paper(&spots, job.range(0.0, 0.2))?;
            let part = job.range(0.2, 0.4);
            TilePlane::<[u16; 4]>::from_rows_with_job(
                w,
                h,
                |y, row| {
                    let mut x = 0usize;
                    for (_, run) in printed.row_segments(y, 0, w) {
                        for sample in run {
                            let ink = [sample[0], sample[1], sample[2], sample[3]];
                            let rgb = leg.rgb(ink_key(ink), ink.map(|v| f64::from(v) / 65535.0));
                            row[x] = [c(rgb[0]), c(rgb[1]), c(rgb[2]), 65535];
                            x += 1;
                        }
                    }
                    part.every(y as usize * w as usize)
                },
                part,
            )?
        };
        let image = PrecisionImage::from_rgba16_tiles_unseeded(plane, to)?;
        let rgb =
            PrecisionDocument::from_image(image.clone(), self.metadata.clone(), self.resolution)?;
        Ok(rgb
            .export_stages(&image, &staged(request), job.range(0.4, 1.0))?
            .reported_as(request))
    }
}
