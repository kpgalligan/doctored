//! One typed record's RAW pixels on a transparent canvas:
//! [`PrecisionDocument::layer_canvas_image`], the typed twin of the byte
//! `dr_doc_layer_canvas_image`, behind `dr_precision_doc_layer_canvas_image`.
//!
//! No new compositing. The record is read with the raw-layer scope of the
//! scoped statistics (`analysis_source` for an `AnalysisSource::Layer`
//! request): a raster's exact samples at its offset, a group's RAW
//! projection (`raw_group_document`, the rule Copy of a group uses: the
//! children composited among themselves without the group's own opacity,
//! blend, mask, style or Blend If, visible, Pass Through re-expressed as
//! isolated); an adjustment and an out-of-range
//! index refuse by `scoped_analysis_plan`'s own sentences. The source is
//! then placed on a canvas-sized image in the document's render storage by
//! the clipboard's coverage-free window read (`placed_on_canvas`), so a
//! float record's extended-range samples survive bit for bit. Display stays
//! the caller's: the u16 preview or the float document's tone map.
//!
//! the typed twin of `dr_doc_layer_canvas_image`: the scoped analysis's raw-layer source
//! (`analysis_source`; a group's raw projection, `raw_group_document`; an adjustment or bad
//! index refuses by `scoped_analysis_plan`'s sentence) placed by
//! `precision_clipboard::placed_on_canvas` (the coverage-free `Window` read) in the render storage,
//! so float samples survive bit for bit. Tests: `tests/precision_layer_canvas_image.rs`.

use serde_json::json;

use crate::job_control::{DrJobControl, JobContext};
use crate::precision_clipboard::placed_on_canvas;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, PrecisionImage};
use crate::scoped_statistics::{AnalysisRequest, STATISTICS_SCRATCH};

impl PrecisionDocument {
    /// Record `index`'s raw pixels on a transparent canvas-sized image in
    /// the document's render storage. Admission is the layer source's plan
    /// without the statistics scratch (nothing is counted) plus the canvas
    /// output; the source read runs with that output counted as retained.
    pub fn layer_canvas_image(
        &self,
        index: usize,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<PrecisionImage, String> {
        // A CMYK record's own ink, through the document's view (`cmyk_render`).
        if self.is_cmyk() {
            return self.cmyk_layer_canvas_image(index, limit, other, control);
        }
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let request = AnalysisRequest::parse(
            &json!({"operation":"histogram","source":"layer","layer":index}).to_string(),
        )?;
        let (w, h) = self.dimensions();
        let canvas = u64::from(w) * u64::from(h) * self.render_storage().bytes_per_pixel();
        let scratch = self.scoped_analysis_plan(&request)? - STATISTICS_SCRATCH + canvas;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let (image, offset) =
            self.analysis_source(&request, limit, other + canvas, job.range(0., 0.8))?;
        let placed = placed_on_canvas(
            &image,
            offset,
            self.dimensions(),
            self.profile().clone(),
            job.range(0.8, 1.),
        )?;
        job.report(1., 5)?;
        Ok(placed)
    }
}
