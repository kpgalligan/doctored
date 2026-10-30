//! Liquify and the History Brush on a Bitmap document (#547 Bitmap phase
//! 3, `designs/bitmap-mode.md`). An adapter, not a kernel, in
//! `bitmap_edit`'s shape: each runs on the document's 8-bit Gray storage
//! (`PrecisionDocument::without_bitmap`) through its existing kernel, then
//! `bitmap_edit::republish` puts every pixel back on the 1-bit plane by the
//! one rule, `bitmap_method::white_over`, over a white background.
//!
//! - **Liquify** is the selected edit's (`selected_adjustment`, admitted by
//!   `bitmap_document::admit_selected`): the strokes resample the gray, and
//!   the result is thresholded, as phase 2a-ii's resampled geometry is.
//! - **The History Brush** paints the Gray storage back toward a source
//!   state's layer (a Bitmap snapshot's layer is the plane's black-and-white
//!   render), then thresholds it, so full coverage gives the source's bits
//!   back.
//!
//! Each plan is the Gray storage's plan plus the output canvas ×
//! `BITMAP_EDIT_BYTES_PER_PIXEL` and bookkeeping (the republish, measured
//! for `bitmap_edit`); each result is `None` when no bit moves.

use serde_json::{json, Value};

use crate::bitmap_edit::{republish, BITMAP_EDIT_BYTES_PER_PIXEL, NOT_BITMAP};
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, BOOKKEEPING_BYTES};
use crate::selected_adjustment::SelectedEdit;

/// `value` (the Gray storage's plan) with the republish's bytes over
/// `doc`'s canvas added to `scratch_bytes`.
fn with_republish(doc: &PrecisionDocument, mut value: Value) -> Result<Value, String> {
    let scratch = value["scratch_bytes"]
        .as_u64()
        .ok_or("bitmap tool plan has no scratch_bytes")?;
    let pixels = u64::from(doc.width) * u64::from(doc.height);
    value["scratch_bytes"] = json!(scratch
        .saturating_add(pixels.saturating_mul(BITMAP_EDIT_BYTES_PER_PIXEL))
        .saturating_add(BOOKKEEPING_BYTES));
    Ok(value)
}

/// The scratch bytes of a plan from [`with_republish`].
fn scratch(plan: &Value) -> Result<u64, String> {
    plan["scratch_bytes"]
        .as_u64()
        .ok_or_else(|| "bitmap tool plan has no scratch_bytes".to_string())
}

/// The selected edit's plan on a Bitmap document (module doc).
pub(crate) fn selected_plan(doc: &PrecisionDocument, edit: &SelectedEdit) -> Result<Value, String> {
    doc.bitmap_policy().ok_or(NOT_BITMAP)?;
    with_republish(doc, doc.without_bitmap().selected_edit_plan(edit)?)
}

/// The selected edit (Liquify, module doc) on a Bitmap document; `Ok(None)`
/// when no bit moves.
pub(crate) fn selected_edit(
    doc: &PrecisionDocument,
    edit: &SelectedEdit,
    coverage: &[u16],
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<Option<PrecisionDocument>, String> {
    doc.bitmap_policy().ok_or(NOT_BITMAP)?;
    let scratch = scratch(&selected_plan(doc, edit)?)?;
    admit(doc.retained_bytes(), scratch, other, limit)?;
    job.reserve(scratch)?;
    let gray = doc.without_bitmap();
    let inner = job.range(0.0, 0.8).with_reservation_floor(scratch);
    let Some(result) = gray.selected_edit_with_context(edit, coverage, limit, other, inner)? else {
        return Ok(None);
    };
    republish(doc, &result, true, true, job.range(0.8, 1.0))
}

/// The History Brush's plan on a Bitmap document (module doc).
pub(crate) fn history_brush_plan(
    doc: &PrecisionDocument,
    layer: usize,
    source: &PrecisionDocument,
    source_layer: usize,
    selection: bool,
) -> Result<Value, String> {
    doc.bitmap_policy().ok_or(NOT_BITMAP)?;
    let value = doc
        .without_bitmap()
        .history_brush_plan(layer, source, source_layer, selection)?;
    with_republish(doc, value)
}

/// The History Brush on a Bitmap document (module doc): the Gray storage's
/// stroke, which takes the locks itself, then the result thresholded.
/// `Ok(None)` when no bit moves.
#[allow(clippy::too_many_arguments)]
pub(crate) fn history_brush(
    doc: &PrecisionDocument,
    layer: usize,
    source: &PrecisionDocument,
    source_layer: usize,
    opacity: f64,
    samples: &[u16],
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<Option<PrecisionDocument>, String> {
    doc.bitmap_policy().ok_or(NOT_BITMAP)?;
    let canvas = crate::precision_image::pixel_count(doc.width, doc.height)?;
    let selection = samples.len() == 2 * canvas;
    let scratch = scratch(&history_brush_plan(
        doc,
        layer,
        source,
        source_layer,
        selection,
    )?)?;
    admit(doc.retained_bytes(), scratch, other, limit)?;
    job.reserve(scratch)?;
    let gray = doc.without_bitmap();
    let inner = job.range(0.0, 0.8).with_reservation_floor(scratch);
    let Some(result) = gray.history_brush(
        layer,
        source,
        source_layer,
        opacity,
        samples,
        limit,
        other,
        inner,
    )?
    else {
        return Ok(None);
    };
    republish(doc, &result, true, true, job.range(0.8, 1.0))
}
