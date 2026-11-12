//! The export of a Grayscale document that keeps gray (#1 phase 1b):
//! profile `document` with PNG, TIFF or JPEG writes ONE channel (two with
//! alpha) at 8 or 16 bits, JPEG at 8, with the Gray ICC embedded; a 32-bit
//! Gray document's TIFF at 32 bits is the float stage's
//! (`doc_export_float::export_float`, #1 phase 3c).
//! `doc_export`'s typed plan and stages hand a Gray document here (the
//! `cmyk_export` dispatch).
//!
//! * **Kept gray** — the ordinary stages run first (the float pre-stage's
//!   tone map on a 32-bit document, resample, sharpen) on the R = G = B
//!   storage; then [`PrecisionDocument::gray_export_stages`] takes R (and
//!   alpha) of the u16 composite at 16 bits, or of its 8-bit reduction
//!   (`quantize_with_context`, the request's dither) at 8. Alpha is written
//!   only when some pixel is not opaque. A JPEG flattens over white first
//!   (`dr_image::composite_over_white`, the RGB JPEG's rule). The sidecar is
//!   the RGB one (`doc_color::image_sidecar_with_policy`) with the twin's
//!   ICC replaced by the Gray profile's bytes, verbatim; `gray_encode`
//!   writes the file.
//! * **Refused** — a kept request in any other format has no one-channel
//!   export ([`PrecisionDocument::gray_export_refusal`]); PNG and JPEG at 32
//!   bits are refused by the request's own validation.
//! * **RGB** — profile `srgb`, `display-p3` or `file` runs the RGB stages
//!   unchanged from the twin: an RGB file.

use crate::doc_export::{output_dimensions, ExportRequest, ExportResult, OutputProfile};
use crate::dr_image::Format;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{reserved_vec, PrecisionImage, PrecisionRun};

/// Bytes an output pixel a kept-gray export charges beside the RGB stages'
/// own plan, measured on the opaque cases: the gray buffer the encoder takes
/// (2 B/px at L16) and a PNG's native-endian copy of it (2 B/px). A
/// transparent 16-bit PNG (LA16, 4 + 4 B/px) rides on the shared convert and
/// codec terms, which cover it many times over. Held by `scripts/gray-admission/measure.py` to ⌈1.25 × the measured
/// peak⌉ (`export-gray-png16`, `export-gray-tiff8`, `export-gray-jpeg8`); a
/// miss raises this term.
pub(crate) const EXPORT_BYTES_PER_PIXEL: u64 = 4;

impl PrecisionDocument {
    /// True when `request` keeps gray (module doc).
    pub(crate) fn keeps_gray(&self, request: &ExportRequest) -> bool {
        self.is_gray() && matches!(request.profile, OutputProfile::Document)
    }

    /// Why a kept-gray `request` cannot be written (module doc).
    pub(crate) fn gray_export_refusal(&self, request: &ExportRequest) -> Option<String> {
        if !self.keeps_gray(request) {
            return None;
        }
        let one_channel = matches!(request.format, Format::Png | Format::Tiff | Format::Jpeg);
        (!one_channel).then(|| {
            format!(
                "A Grayscale document exports one-channel PNG, TIFF or JPEG with profile \
                 document; {} has no one-channel export in Doctored. Choose profile srgb, \
                 display-p3 or file to export RGB.",
                request.format_name()
            )
        })
    }

    /// The last stages of a kept-gray export (module doc) over `prepared`,
    /// the resampled and sharpened u16 composite.
    pub(crate) fn gray_export_stages(
        &self,
        prepared: &PrecisionImage,
        request: &ExportRequest,
        job: JobContext<'_>,
    ) -> Result<ExportResult, String> {
        let policy = self
            .gray_policy()
            .ok_or("a kept-gray export requires a Grayscale document")?
            .clone();
        let (w, h) = output_dimensions(prepared.dimensions(), request)?;
        if prepared.dimensions() != (w, h) {
            return Err("a kept-gray export's composite is not at the output size".into());
        }
        let format = request.format;
        let (mut side, carried, outcome) = crate::doc_color::image_sidecar_with_policy(
            self.profile(),
            self.metadata(),
            self.resolution(),
            format,
            request.embed_profile,
            request.strip_metadata,
            (w, h),
            &request.metadata,
        );
        // The sidecar decided WHETHER a profile is embedded (and reported
        // it); the Gray profile's bytes are what is embedded, not the twin's.
        if side.icc.is_some() {
            side.icc = Some(policy.bytes());
        }
        // A TIFF carries the paths in its 8BIM run (`tiff_paths`).
        if let Some(paths) = self.tiff_path_run().filter(|_| format == Format::Tiff) {
            crate::tiff_paths::attach(&mut side, paths);
        }
        let bytes = if request.bit_depth == 16 {
            let (samples, alpha) = gray16_samples(prepared, job)?;
            crate::gray_encode::encode_gray16(&samples, alpha, (w, h), format, &side)?
        } else {
            let reduced = prepared.quantize_with_context(request.dither, job)?;
            job.check()?;
            if format == Format::Jpeg {
                let flat = crate::dr_image::composite_over_white(&reduced);
                drop(reduced);
                let mut samples = reserved_vec(w as usize * h as usize)?;
                for (i, pixel) in flat.pixels().enumerate() {
                    job.every(i)?;
                    samples.push(pixel[0]);
                }
                drop(flat);
                crate::gray_encode::encode_gray8(
                    &samples,
                    false,
                    (w, h),
                    format,
                    request.jpeg_quality,
                    &side,
                )?
            } else {
                let alpha = reduced.pixels().any(|pixel| pixel[3] < u8::MAX);
                let per = if alpha { 2 } else { 1 };
                let mut samples = reserved_vec(w as usize * h as usize * per)?;
                for (i, pixel) in reduced.pixels().enumerate() {
                    job.every(i)?;
                    samples.push(pixel[0]);
                    if alpha {
                        samples.push(pixel[3]);
                    }
                }
                drop(reduced);
                crate::gray_encode::encode_gray8(
                    &samples,
                    alpha,
                    (w, h),
                    format,
                    request.jpeg_quality,
                    &side,
                )?
            }
        };
        job.report(1.0, 5)?;
        ExportResult::gray_kept(
            bytes,
            (w, h),
            carried,
            request.bit_depth,
            request,
            policy.name().to_string(),
            outcome,
        )
    }
}

/// R (and alpha, when some pixel is not opaque) of a u16 composite.
fn gray16_samples(image: &PrecisionImage, job: JobContext<'_>) -> Result<(Vec<u16>, bool), String> {
    let (w, h) = image.dimensions();
    let not_rgba16 = || "a kept-gray export requires RGBA16 UNORM storage".to_string();
    let mut alpha = false;
    for y in 0..h {
        for (_, run) in image.row_segments(y, 0, w) {
            let PrecisionRun::Rgba16(run) = run else {
                return Err(not_rgba16());
            };
            alpha |= run.iter().any(|pixel| pixel[3] < u16::MAX);
        }
        if alpha {
            break;
        }
    }
    let per = if alpha { 2 } else { 1 };
    let mut samples = reserved_vec(w as usize * h as usize * per)?;
    for y in 0..h {
        for (_, run) in image.row_segments(y, 0, w) {
            let PrecisionRun::Rgba16(run) = run else {
                return Err(not_rgba16());
            };
            for pixel in run {
                samples.push(pixel[0]);
                if alpha {
                    samples.push(pixel[3]);
                }
            }
        }
        job.every(y as usize * w as usize)?;
    }
    Ok((samples, alpha))
}
