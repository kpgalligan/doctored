//! The export of a Bitmap document that keeps its 1-bit plane (#1 Bitmap
//! phase 1c): profile `document` with PNG, TIFF or BMP writes the packed
//! plane as it is (`bitmap_codec`, `tiff_encode::encode_bilevel`), with no
//! render and no threshold, so the file reopens as the same plane.
//! `doc_export`'s typed `export_plan` and `export_encoded` hand a Bitmap
//! request here before the Indexed and Gray arms (decision 1: a Bitmap
//! document is Gray storage underneath); `indexed_export` is the template.
//!
//! * **Kept 1-bit** — PNG colour type 0 at depth 1, TIFF BitsPerSample 1
//!   Photometric 1 and BMP 1 bpp. The sidecar is the RGB one
//!   (`doc_color::image_sidecar_with_policy`) over the Gray twin, with the
//!   twin's ICC replaced by the Gray profile's bytes when it embeds one
//!   (`gray_export`'s rule); a BMP carries no profile and no metadata
//!   (`format_carries`), only its pixels per metre. The result's
//!   `bit_depth` is 1.
//! * **Refused** — with profile `document`, any format but those three and
//!   JPEG; and a kept request with `bit_depth` other than 8, an output size
//!   other than the canvas, or an active sharpen: each would change the
//!   plane, so each names the RGB export that can do it.
//! * **Through Gray** — JPEG with profile `document` is today's kept-gray
//!   JPEG of the storage (`gray_export`, 8 bits); profile `srgb`,
//!   `display-p3` or `file` runs today's RGB stages over the storage, the
//!   plane's render.

use serde_json::{json, Value};

use crate::doc_export::{output_dimensions, ExportRequest, ExportResult, OutputProfile};
use crate::dr_image::Format;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, pixel_count, BOOKKEEPING_BYTES};

/// Bytes a canvas pixel a kept-Bitmap export charges, its whole scratch
/// beside bookkeeping: the encoded file the result holds (an eighth of a
/// byte a pixel, uncompressed in a TIFF or BMP) and the encoder's copies of
/// the rows, far below a byte a pixel; 2 leaves the rule's margin over the
/// largest. Held by `scripts/bitmap-admission/measure.py`
/// (`export-bitmap-tiff`) to ⌈1.25 × the measured peak⌉; a miss raises this
/// term.
pub(crate) const EXPORT_BYTES_PER_PIXEL: u64 = 2;

/// The kept-depth refusal (module doc).
const KEPT_DEPTH: &str =
    "A Bitmap export to PNG, TIFF or BMP with profile document writes 1 bit a \
                          pixel, so it takes bit_depth 8; choose profile srgb, display-p3 or file \
                          for a deeper RGB copy.";

/// The kept size-or-sharpen refusal (module doc).
const KEPT_SIZE: &str = "A Bitmap export to PNG, TIFF or BMP with profile document keeps every \
                         pixel, so it takes no size or sharpen; choose profile srgb, display-p3 or \
                         file to export a resized or sharpened RGB copy.";

impl PrecisionDocument {
    /// True when `request` keeps the 1-bit plane (module doc).
    pub(crate) fn keeps_bitmap(&self, request: &ExportRequest) -> bool {
        self.is_bitmap()
            && matches!(request.profile, OutputProfile::Document)
            && matches!(request.format, Format::Png | Format::Tiff | Format::Bmp)
    }

    /// Why a Bitmap `request` cannot be written (module doc); `None` for a
    /// kept request it admits, for JPEG through Gray, for any RGB profile
    /// and for every other document.
    pub(crate) fn bitmap_export_refusal(&self, request: &ExportRequest) -> Option<String> {
        if !self.is_bitmap() || !matches!(request.profile, OutputProfile::Document) {
            return None;
        }
        if !self.keeps_bitmap(request) {
            return (request.format != Format::Jpeg).then(|| {
                format!(
                    "A Bitmap document exports a 1-bit PNG, TIFF or BMP, or JPEG through its \
                     8-bit Grayscale render, with profile document; {} has neither. Choose \
                     profile srgb, display-p3 or file to export RGB.",
                    request.format_name()
                )
            });
        }
        if request.bit_depth != 8 {
            return Some(KEPT_DEPTH.into());
        }
        let canvas = self.dimensions();
        let resized = !matches!(output_dimensions(canvas, request), Ok(out) if out == canvas);
        let sharpened = request.sharpen.is_some_and(|s| !s.is_bypass());
        (resized || sharpened).then(|| KEPT_SIZE.into())
    }

    /// The typed `export_plan` of a kept-Bitmap request:
    /// `{"retained_bytes","scratch_bytes","width","height"}`.
    pub(crate) fn bitmap_export_plan(&self, request: &ExportRequest) -> Result<Value, String> {
        if let Some(reason) = self.bitmap_export_refusal(request) {
            return Err(reason);
        }
        if !self.keeps_bitmap(request) {
            return Err("a kept-Bitmap export is PNG, TIFF or BMP with profile document".into());
        }
        let (w, h) = self.dimensions();
        let scratch = pixel_count(w, h)? as u64 * EXPORT_BYTES_PER_PIXEL + BOOKKEEPING_BYTES;
        Ok(
            json!({"retained_bytes":self.retained_bytes(),"scratch_bytes":scratch,
            "width":w,"height":h}),
        )
    }

    /// The kept-Bitmap export (module doc), admitted by
    /// [`Self::bitmap_export_plan`].
    pub(crate) fn bitmap_export_encoded(
        &self,
        request: &ExportRequest,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<ExportResult, String> {
        let plan = self.bitmap_export_plan(request)?;
        let retained = plan["retained_bytes"].as_u64().unwrap_or(0);
        let scratch = plan["scratch_bytes"].as_u64().unwrap_or(0);
        admit(retained, scratch, other, limit)?;
        job.check()?;
        job.reserve(scratch)?;
        let bitmap = self
            .bitmap_policy()
            .ok_or("a kept-Bitmap export requires a Bitmap document")?
            .clone();
        let gray = self
            .gray_policy()
            .ok_or("a kept-Bitmap export requires a Bitmap document")?
            .clone();
        let dims = self.dimensions();
        if bitmap.dimensions() != dims {
            return Err("the Bitmap plane does not match the canvas".into());
        }
        let format = request.format;
        let (mut side, carried, outcome) = crate::doc_color::image_sidecar_with_policy(
            self.profile(),
            self.metadata(),
            self.resolution(),
            format,
            request.embed_profile,
            request.strip_metadata,
            dims,
            &request.metadata,
        );
        // The sidecar decided WHETHER a profile is embedded (and reported
        // it); the Gray profile's bytes are what is embedded, not the twin's.
        if side.icc.is_some() {
            side.icc = Some(gray.bytes());
        }
        // A TIFF carries the paths in its 8BIM run (`tiff_paths`).
        if let Some(paths) = self.tiff_path_run().filter(|_| format == Format::Tiff) {
            crate::tiff_paths::attach(&mut side, paths);
        }
        let packed = bitmap.packed();
        // The encode is the last stage; its codec polls as it writes.
        job.report(0.9, 1)?;
        let bytes = match format {
            Format::Png => crate::bitmap_codec::encode_png(packed, dims, &side, job)?,
            Format::Tiff => {
                let mut injected = crate::gray_encode::injector(Format::Tiff, &side);
                let mut sink = crate::cancel_io::Polled::new(&mut injected, job);
                let written = crate::tiff_encode::encode_bilevel(packed, dims, &mut sink, &side);
                crate::cancel_io::settle(job, written)?;
                injected.finish()?.into_inner()
            }
            Format::Bmp => crate::bitmap_codec::encode_bmp(packed, dims, self.resolution(), job)?,
            _ => return Err("a kept-Bitmap export is PNG, TIFF or BMP".into()),
        };
        job.report(1.0, 5)?;
        ExportResult::gray_kept(
            bytes,
            dims,
            carried,
            1,
            request,
            gray.name().to_string(),
            outcome,
        )
    }
}
