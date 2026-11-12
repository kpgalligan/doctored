//! A Lab Color document's TIFF save (#1 phase 1b): the flattened composite
//! as a TIFF 6.0 CIELab file at the document's own depth, its request and
//! its plan (`dr_precision_doc_save_lab_plan` / `dr_precision_doc_save_lab`;
//! the CMYK save's shape, `cmyk_io`).
//!
//! The composite is the host's projection when it passes one (checked by
//! `PrecisionDocument::check_projection`, the export's rule), else a fresh
//! render. Its samples are `lab_encode`'s (L\* unsigned, a\*/b\* signed),
//! three a pixel, or four with straight alpha only when some pixel is not
//! opaque. The sidecar is the TIFF one every save writes
//! (`doc_color::image_sidecar`: resolution, EXIF, XMP, IPTC, and the paths,
//! `tiff_paths`) with no ICC
//! profile: a CIELab TIFF's samples are CIE L\*a\*b\* (D50) by definition, and
//! the twin describes Doctored's storage, not the file's encoding. Other
//! formats refuse: a Lab PSD, PSB or layered TIFF is Export as PSD's
//! (`psd_export_lab`, phase 3b; phase 3a opens Lab files).

use std::io::Cursor;

use serde_json::{json, Value};

use crate::job_control::{DrJobControl, JobContext};
use crate::lab_document::LabDepth;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{
    admit, pixel_count, reserved_vec, PrecisionImage, PrecisionRun, BOOKKEEPING_BYTES,
};
use crate::resources::Inventory;

/// Bytes a canvas pixel the save charges: the TIFF samples (up to 8 B/px
/// at 16 bits with alpha), the encoder's buffer and the file (each up to
/// the samples' size). Held by `scripts/lab-admission` `save-lab-tiff16` to
/// ⌈1.25 × the measured peak⌉; a miss raises this term.
pub(crate) const SAVE_BYTES_PER_PIXEL: u64 = 24;

/// The save's request: `{"format":"tiff","strip_metadata"?:bool}`.
struct SaveRequest {
    strip_metadata: bool,
}

impl SaveRequest {
    fn parse(text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(text)
            .map_err(|e| format!("Lab save request is not valid JSON: {e}"))?;
        let object = value
            .as_object()
            .ok_or("Lab save request must be a JSON object")?;
        for key in object.keys() {
            if !matches!(key.as_str(), "format" | "strip_metadata") {
                return Err(format!("Lab save request has an unknown key \"{key}\""));
            }
        }
        match object.get("format").and_then(Value::as_str) {
            Some("tiff") => {}
            Some(other) => {
                return Err(format!(
                    "Saving as {other} is unavailable in a Lab Color document: it saves as a Lab TIFF, and Export as PSD writes its PSD."
                ))
            }
            None => return Err("Lab save request needs a \"format\"".into()),
        }
        let strip_metadata = match object.get("strip_metadata") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(v)) => *v,
            Some(_) => return Err("Lab save request strip_metadata must be a boolean".into()),
        };
        Ok(Self { strip_metadata })
    }
}

impl PrecisionDocument {
    /// The save's refusal before any work: not a Lab document, spot
    /// channels (`spot_channel::TIFF_REFUSAL`), or a projection that is not
    /// this canvas's.
    fn lab_save_refusal(&self, projection: Option<&PrecisionImage>) -> Result<(), String> {
        if !self.is_lab() {
            return Err("Saving a Lab TIFF requires a Lab Color document".into());
        }
        // A Lab TIFF writes no spot channels (#1 1c-2a, `tiff_spot`).
        if self.has_spot_channel() {
            return Err(crate::spot_channel::TIFF_REFUSAL.into());
        }
        if let Some(image) = projection {
            self.check_projection(image)?;
        }
        Ok(())
    }

    /// The save's working memory: [`SAVE_BYTES_PER_PIXEL`] a canvas pixel
    /// and bookkeeping, plus the render's scratch and output when the host
    /// passes no projection.
    fn lab_save_scratch(&self, projection: Option<&PrecisionImage>) -> Result<u64, String> {
        let (w, h) = self.dimensions();
        let pixels = pixel_count(w, h)? as u64;
        let render = match projection {
            Some(_) => 0,
            None => {
                let plan = self.render_plan()?;
                plan.scratch_bytes.saturating_add(plan.output_bytes)
            }
        };
        Ok(pixels
            .saturating_mul(SAVE_BYTES_PER_PIXEL)
            .saturating_add(render)
            .saturating_add(BOOKKEEPING_BYTES))
    }

    /// The unique bytes of the document and its projection.
    fn lab_save_retained(&self, projection: Option<&PrecisionImage>) -> u64 {
        let mut inventory = Inventory::default();
        self.record_resources(&mut inventory);
        if let Some(image) = projection {
            image.record_resources(&mut inventory);
        }
        inventory.unique_bytes()
    }

    /// `dr_precision_doc_save_lab_plan`'s answer:
    /// `{"working_memory_plan_bytes","refusal"}`.
    pub fn save_lab_plan(
        &self,
        projection: Option<&PrecisionImage>,
        request: &str,
    ) -> Result<Value, String> {
        let bytes = self.lab_save_scratch(projection)?;
        let refusal = SaveRequest::parse(request)
            .and_then(|_| self.lab_save_refusal(projection))
            .err();
        Ok(json!({"working_memory_plan_bytes":bytes,"refusal":refusal}))
    }

    /// Writes the composite as a CIELab TIFF to `path`, atomically (module
    /// doc).
    pub fn save_lab(
        &self,
        projection: Option<&PrecisionImage>,
        path: &str,
        request: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<(), String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let request = SaveRequest::parse(request)?;
        self.lab_save_refusal(projection)?;
        let depth = self
            .lab_policy()
            .map(|policy| policy.depth())
            .ok_or("Saving a Lab TIFF requires a Lab Color document")?;
        let scratch = self.lab_save_scratch(projection)?;
        admit(self.lab_save_retained(projection), scratch, other, limit)?;
        job.reserve(scratch)?;
        let rendered;
        let composite = match projection {
            Some(image) => image,
            None => {
                rendered = self.render_with_context(limit, other, job.range(0.0, 0.5))?;
                &rendered
            }
        };
        let bytes = self.lab_tiff_bytes(composite, depth, request.strip_metadata, job)?;
        job.check()?;
        crate::dr_image::save_atomically(path, |tmp| {
            std::fs::write(tmp, &bytes).map_err(|e| format!("failed to write {path}: {e}"))
        })?;
        job.report(1.0, 5)?;
        Ok(())
    }

    /// The CIELab TIFF of `composite` (module doc). Admission belongs to the
    /// caller.
    fn lab_tiff_bytes(
        &self,
        composite: &PrecisionImage,
        depth: LabDepth,
        strip_metadata: bool,
        job: JobContext<'_>,
    ) -> Result<Vec<u8>, String> {
        let (w, h) = composite.dimensions();
        let rows = |y: u32| {
            composite
                .row_segments(y, 0, w)
                .map(|(_, run)| match run {
                    PrecisionRun::Rgba16(run) => Ok(run),
                    PrecisionRun::Rgba32(_) => Err("a Lab TIFF requires u16 storage".to_string()),
                })
                .collect::<Result<Vec<_>, String>>()
        };
        let mut alpha = false;
        for y in 0..h {
            job.check()?;
            if rows(y)?
                .iter()
                .flat_map(|run| run.iter())
                .any(|p| p[3] < 65535)
            {
                alpha = true;
                break;
            }
        }
        let per_pixel = if alpha { 4 } else { 3 };
        let count = pixel_count(w, h)? * per_pixel;
        let (mut side, _) = crate::doc_color::image_sidecar(
            self.profile(),
            self.metadata(),
            self.resolution(),
            crate::dr_image::Format::Tiff,
            false,
            strip_metadata,
            (w, h),
        );
        side.icc = None;
        if let Some(paths) = self.tiff_path_run() {
            crate::tiff_paths::attach(&mut side, paths);
        }
        let mut out = Cursor::new(Vec::new());
        match depth {
            LabDepth::Eight => {
                let mut samples = reserved_vec(count)?;
                for y in 0..h {
                    job.check()?;
                    for run in rows(y)? {
                        crate::lab_encode::tiff_samples8(run, alpha, &mut samples);
                    }
                }
                crate::tiff_encode::encode_cielab8(&samples, alpha, (w, h), &mut out, &side)?;
            }
            LabDepth::Sixteen => {
                let mut samples = reserved_vec(count)?;
                for y in 0..h {
                    job.check()?;
                    for run in rows(y)? {
                        crate::lab_encode::tiff_samples16(run, alpha, &mut samples);
                    }
                }
                crate::tiff_encode::encode_cielab16(&samples, alpha, (w, h), &mut out, &side)?;
            }
        }
        Ok(out.into_inner())
    }
}
