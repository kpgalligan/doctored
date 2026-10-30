//! The export of an Indexed Color document that keeps Indexed (#1
//! Indexed phase 1b): profile `document` with PNG, GIF or TIFF writes the
//! document's palette, transparent index and every index as they are
//! (`indexed_encode`), with no render and no quantization, so the file
//! reopens as the same palette and plane. `doc_export`'s typed
//! `export_plan` and `export_encoded` hand a kept request here (the
//! `cmyk_export` dispatch); `gray_export` is the template.
//!
//! * **Kept Indexed** — the sidecar is the RGB one
//!   (`doc_color::image_sidecar_with_policy`) over the document's own
//!   profile, at the canvas size; a GIF carries neither profile nor
//!   metadata (`format_carries`).
//! * **Refused** — a kept request with `bit_depth` other than 8, an output
//!   size other than the canvas, or an active sharpen: each would change
//!   the indices, so each names the RGB export that can do it. Since phase
//!   3a a kept request while the one layer is hidden refuses too
//!   (`HIDDEN_LAYER`): the file would hold every index of a layer the
//!   composite does not show. Export Layers' kept path asks the same.
//! * **RGB** — every other request (another format, or profile `srgb`,
//!   `display-p3` or `file`) runs today's RGB stages over the storage, the
//!   palette's render.
//!
//! Since phase 2d, Export Layers of the one Index layer
//! (`doc_export_layers`' typed arms): an untrimmed kept request is the kept
//! export above with the layer's description; a trimmed one, or any other
//! request, is the typed layer route over the RGB storage.

use serde_json::{json, Value};

use crate::doc_export::{output_dimensions, ExportRequest, ExportResult, OutputProfile};
use crate::doc_export_layers::LayerExportInfo;
use crate::dr_image::Format;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, pixel_count, BOOKKEEPING_BYTES};

/// Bytes a canvas pixel a kept-Indexed export charges, its whole scratch
/// beside bookkeeping: the packed or interleaved plane the encoder takes (at
/// most 2 B/px, a TIFF's index and alpha) and the encoded bytes the result
/// holds. The heaviest case, a TIFF with a transparent index, measured
/// 4.01 B/px (2 B/px of interleaved samples and 2 B/px of uncompressed
/// output), so 1.25 × that needs 5.01 B/px: 6 keeps the rule with margin.
/// Held by `scripts/indexed-admission/measure.py` (`export-indexed-png`,
/// `export-indexed-gif`, `export-indexed-tiff` and their `-alpha` twins) to
/// ⌈1.25 × the measured peak⌉; a miss raises this term.
pub(crate) const EXPORT_BYTES_PER_PIXEL: u64 = 6;

/// The kept-depth refusal (module doc).
const KEPT_DEPTH: &str = "An Indexed Color export with profile document keeps the palette at 8 \
                          bits; choose bit_depth 8, or profile srgb, display-p3 or file to export \
                          RGB.";

/// The kept refusal while the one layer is hidden (module doc, phase 3a).
pub(crate) const HIDDEN_LAYER: &str =
    "The Index layer is hidden: show it before exporting or saving a copy that keeps Indexed Color.";

/// The kept size-or-sharpen refusal (module doc).
const KEPT_SIZE: &str = "An Indexed Color export with profile document keeps every index, so it \
                         takes no size or sharpen; choose profile srgb, display-p3 or file to \
                         export a resized or sharpened RGB copy.";

impl PrecisionDocument {
    /// True when `request` keeps Indexed (module doc).
    pub(crate) fn keeps_indexed(&self, request: &ExportRequest) -> bool {
        self.is_indexed()
            && matches!(request.profile, OutputProfile::Document)
            && matches!(request.format, Format::Png | Format::Gif | Format::Tiff)
    }

    /// Why a kept-Indexed `request` cannot be written (module doc).
    pub(crate) fn indexed_export_refusal(&self, request: &ExportRequest) -> Option<String> {
        if !self.keeps_indexed(request) {
            return None;
        }
        if self.root_layers().iter().any(|layer| !layer.visible) {
            return Some(HIDDEN_LAYER.into());
        }
        if request.bit_depth != 8 {
            return Some(KEPT_DEPTH.into());
        }
        let canvas = self.dimensions();
        let resized = !matches!(output_dimensions(canvas, request), Ok(out) if out == canvas);
        let sharpened = request.sharpen.is_some_and(|s| !s.is_bypass());
        (resized || sharpened).then(|| KEPT_SIZE.into())
    }

    /// The typed `export_plan` of a kept-Indexed request:
    /// `{"retained_bytes","scratch_bytes","width","height"}`.
    pub(crate) fn indexed_export_plan(&self, request: &ExportRequest) -> Result<Value, String> {
        if let Some(reason) = self.indexed_export_refusal(request) {
            return Err(reason);
        }
        let (w, h) = self.dimensions();
        let canvas = pixel_count(w, h)? as u64;
        // The palette encoders never run the 16-bit RGBA codec, so its
        // encode scratch is not charged.
        let scratch = canvas * EXPORT_BYTES_PER_PIXEL + BOOKKEEPING_BYTES;
        Ok(
            json!({"retained_bytes":self.retained_bytes(),"scratch_bytes":scratch,
            "width":w,"height":h}),
        )
    }

    /// The kept-Indexed export (module doc), admitted by
    /// [`Self::indexed_export_plan`].
    pub(crate) fn indexed_export_encoded(
        &self,
        request: &ExportRequest,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<ExportResult, String> {
        let plan = self.indexed_export_plan(request)?;
        let retained = plan["retained_bytes"].as_u64().unwrap_or(0);
        let scratch = plan["scratch_bytes"].as_u64().unwrap_or(0);
        admit(retained, scratch, other, limit)?;
        job.check()?;
        job.reserve(scratch)?;
        let policy = self
            .indexed_policy()
            .ok_or("a kept-Indexed export requires an Indexed Color document")?
            .clone();
        let dims = self.dimensions();
        if policy.dimensions() != dims {
            return Err("the Indexed Color index plane does not match the canvas".into());
        }
        let format = request.format;
        let (side, carried, outcome) = crate::doc_color::image_sidecar_with_policy(
            self.profile(),
            self.metadata(),
            self.resolution(),
            format,
            request.embed_profile,
            request.strip_metadata,
            dims,
            &request.metadata,
        );
        let (palette, transparent, indices) =
            (policy.palette(), policy.transparent(), policy.indices());
        let bytes = match format {
            Format::Png => {
                crate::indexed_encode::encode_png(palette, transparent, indices, dims, &side)?
            }
            Format::Gif => crate::indexed_encode::encode_gif(palette, transparent, indices, dims)?,
            Format::Tiff => {
                crate::indexed_encode::encode_tiff(palette, transparent, indices, dims, &side)?
            }
            _ => return Err("a kept-Indexed export is PNG, GIF or TIFF".into()),
        };
        job.report(1.0, 5)?;
        ExportResult::indexed_kept(
            bytes,
            dims,
            carried,
            request,
            self.profile().name().to_string(),
            outcome,
        )
    }

    /// Export Layers' plan of the Index layer (`idx`, #1 Indexed phase
    /// 2d): an untrimmed kept-Indexed request is the document's own kept
    /// plan (the one layer IS the document); every other request is the
    /// typed layer route's plan over the RGB storage.
    pub(crate) fn indexed_layer_export_plan(
        &self,
        idx: usize,
        trimmed: bool,
        request: &ExportRequest,
    ) -> Result<Value, String> {
        if trimmed || !self.keeps_indexed(request) {
            return self
                .without_indexed()
                .export_layer_plan(idx, trimmed, request);
        }
        self.isolated_root(idx)?;
        self.indexed_export_plan(request)
    }

    /// Export Layers of the Index layer (module doc,
    /// [`Self::indexed_layer_export_plan`]): an untrimmed kept-Indexed
    /// request writes the palette and every index with the layer's
    /// description; every other request runs the typed layer route over the
    /// RGB storage.
    pub(crate) fn indexed_layer_export_encoded(
        &self,
        idx: usize,
        trimmed: bool,
        request: &ExportRequest,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<ExportResult, String> {
        if trimmed || !self.keeps_indexed(request) {
            return self
                .without_indexed()
                .export_layer_encoded(idx, trimmed, request, limit, other, control);
        }
        let isolated = self.isolated_root(idx)?;
        let name = isolated
            .root_layers()
            .first()
            .map(|layer| (*layer.name).clone())
            .unwrap_or_default();
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        Ok(self
            .indexed_export_encoded(request, limit, other, job)?
            .with_layer(LayerExportInfo {
                index: idx,
                name,
                origin: (0, 0),
                size: self.dimensions(),
            }))
    }
}
