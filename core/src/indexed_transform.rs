//! The selected pixels' move and transform and Warp on an Indexed Color
//! document (#1 Indexed phases 2d and 3a, `designs/indexed-mode.md`),
//! through the existing kernels (`selected_pixels_with_context`,
//! `warp_edit_with_context`) and 2c's two ways back. An adapter, not a
//! kernel: it names the document types.
//!
//! - **Exact** (`indexed_geometry::carried`): the index plane rides a
//!   carrier layer through the same request, run Nearest. A `move_selected`
//!   (the Move tool with a selection, phase 3a) always moves whole pixels; a
//!   `transform_selected` does when its matrix is a Nearest pick or a
//!   whole-pixel translation (`indexed_geometry::integer_translation`; the
//!   quad form only Nearest). Either is exact when every coverage sample is
//!   also 0 or 65535, so each landed pixel is one source pixel whole; the
//!   hole is new area, `indexed_edge::clear_index` (t, else the entry
//!   nearest the background colour; `duplicate` leaves none). A t pixel
//!   rides the carrier as alpha 0, so a float of t lands nothing and leaves
//!   the destination's index, as the RGB storage does. A warp `set` whose
//!   field is a whole-pixel translation (`WarpField::translation`) is exact
//!   too.
//! - **Resample** (every other request): the op on the RGB storage, then
//!   `indexed_paint::republish` with each pixel's own colour
//!   (`EdgeInk::Own`) against the old plane: an unchanged pixel keeps its
//!   index, a changed one takes `clear_index` below alpha 128, else its
//!   nearest entry.
//!
//! The targets (phase 3a): the layer as above; an alpha channel
//! (`{"channel":n}`) runs the request on the RGB storage, whose channel
//! edit leaves the layer and the plane untouched, and re-attaches the
//! unchanged policy; a mask refuses for good with `transform_mask`'s
//! sentence (an Indexed layer holds no mask); a colour
//! plane (red, green, blue, alpha or an ink plate) refuses for good with
//! `indexed_document::INDEX_CHANNEL_REFUSAL`: an Indexed document's Channels
//! panel shows one channel, Index, so there is no plane to float. A warp
//! keeps no binding (`indexed_paint::rebuild` drops it), so `warp_info`
//! reports `null`, Clear and Bake change nothing, and each set warps the
//! current pixels again (the float, CMYK, Gray and Lab rule).
//!
//! Since phase 3b a request, and a warp `set` through its
//! `indexed_background` field, may name the background colour
//! (`indexed_paint::BACKGROUND_KEY`): with no t, a hole and a resample's
//! clear pixel take its nearest entry, else white's.
//!
//! Each plan is the kernel's plan on the RGB storage plus
//! `indexed_geometry::transform_scratch` over the unchanged canvas, the
//! adapter's term measured by `scripts/indexed-admission/measure.py`'s
//! `transform-nearest-indexed` and `transform-indexed`.

use serde_json::{json, Value};

use crate::indexed_document::{refused_sentence, INDEX_CHANNEL_REFUSAL};
use crate::indexed_edge::{clear_index, EdgeInk};
use crate::indexed_geometry::{carried, integer_translation, transform_scratch};
use crate::indexed_paint::{background_codes, republish, split_background, NOT_INDEXED};
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_geometry::CropBackground;
use crate::precision_image::admit;
use crate::precision_sampling::GeometryFilter;
use crate::retained_source::SourcePlan;
use crate::selected_pixels::{SelectedPixelsEdit, SelectedTarget};
use crate::warp::WarpEdit;

/// A `move_selected` or `transform_selected` request on an Indexed document
/// (module doc).
pub(crate) struct IndexedSelectedEdit {
    edit: SelectedPixelsEdit,
    /// The request's `indexed_paint::BACKGROUND_KEY` (phase 3b).
    background: Option<CropBackground>,
}

impl IndexedSelectedEdit {
    /// A `move_selected` or `transform_selected` request of the layer or an
    /// alpha channel; a mask target refuses with `transform_mask`'s
    /// sentence, a colour plane with [`INDEX_CHANNEL_REFUSAL`] and every
    /// other request with the mode's sentence (module doc).
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let (stripped, background) = split_background(text)?;
        let text = stripped.as_str();
        let edit = SelectedPixelsEdit::parse(text)?;
        if !matches!(
            edit,
            SelectedPixelsEdit::MoveSelected { .. } | SelectedPixelsEdit::TransformSelected { .. }
        ) {
            let value: Value = serde_json::from_str(text)
                .map_err(|e| format!("invalid high-depth edit request: {e}"))?;
            return Err(refused_sentence(value["op"].as_str().unwrap_or("edit")));
        }
        match edit.target() {
            SelectedTarget::Layer | SelectedTarget::Channel(_) => Ok(Self { edit, background }),
            SelectedTarget::Mask => Err(refused_sentence("transform_mask")),
            SelectedTarget::Red
            | SelectedTarget::Green
            | SelectedTarget::Blue
            | SelectedTarget::Alpha
            | SelectedTarget::Cyan
            | SelectedTarget::Magenta
            | SelectedTarget::Yellow
            | SelectedTarget::Black => Err(INDEX_CHANNEL_REFUSAL.into()),
        }
    }

    /// Whether the request alone moves whole pixels (module doc); the
    /// coverage decides the rest.
    fn exact(&self) -> bool {
        match &self.edit {
            SelectedPixelsEdit::MoveSelected { .. } => true,
            SelectedPixelsEdit::TransformSelected { filter, matrix, .. } => {
                matches!(filter, GeometryFilter::Nearest)
                    || matrix.as_ref().is_some_and(integer_translation)
            }
            _ => false,
        }
    }

    /// The kernel's plan on the RGB storage with the adapter's bytes added
    /// to `scratch_bytes`; the input is the canvas coverage, one plane.
    pub(crate) fn plan(&self, doc: &PrecisionDocument) -> Result<Value, String> {
        doc.indexed_policy().ok_or(NOT_INDEXED)?;
        let p = doc.without_indexed().selected_pixels_plan(&self.edit)?;
        let canvas = doc.dimensions();
        Ok(json!({
            "retained_bytes": p.retained_bytes,
            "scratch_bytes": transform_scratch(doc, p.scratch_bytes, canvas),
            "input_samples": u64::from(canvas.0) * u64::from(canvas.1),
        }))
    }

    /// The move or transform (module doc); `Ok(None)` when no index and no
    /// channel sample moves.
    pub(crate) fn apply(
        &self,
        doc: &PrecisionDocument,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        let policy = doc.indexed_policy().ok_or(NOT_INDEXED)?;
        let scratch = self.plan(doc)?["scratch_bytes"]
            .as_u64()
            .ok_or("indexed selected pixels plan has no scratch_bytes")?;
        admit(doc.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        if let SelectedTarget::Channel(_) = self.edit.target() {
            // An alpha channel: the layer and the plane stay as they are.
            let Some(moved) = doc.without_indexed().selected_pixels_with_context(
                &self.edit,
                coverage,
                limit,
                other,
                job.with_reservation_floor(scratch),
            )?
            else {
                return Ok(None);
            };
            return moved.with_indexed(policy.clone()).map(Some);
        }
        let background = background_codes(&doc.without_indexed(), self.background.as_ref())?;
        let hard = coverage.iter().all(|&c| c == 0 || c == 65535);
        if !(self.exact() && hard) {
            let inner = job.range(0.0, 0.7).with_reservation_floor(scratch);
            let Some(moved) = doc
                .without_indexed()
                .selected_pixels_with_context(&self.edit, coverage, limit, other, inner)?
            else {
                return Ok(None);
            };
            return republish(
                doc,
                &moved,
                EdgeInk::Own,
                true,
                background,
                job.range(0.7, 1.0),
            );
        }
        let mut edit = self.edit.clone();
        if let SelectedPixelsEdit::TransformSelected { filter, .. } = &mut edit {
            *filter = GeometryFilter::Nearest;
        }
        carried(
            doc,
            clear_index(policy.palette(), policy.transparent(), background),
            |carrier, inner| {
                carrier.selected_pixels_with_context(&edit, coverage, limit, other, inner)
            },
            job.with_reservation_floor(scratch),
        )
    }
}

/// The plan nothing spends: Clear, Bake and a set that changes nothing.
const NOTHING: SourcePlan = SourcePlan {
    retained_bytes: 0,
    scratch_bytes: 0,
    work_units: 0,
};

/// `warp_plan` on an Indexed document (module doc): Clear and Bake spend
/// nothing; a set is the RGB storage's plan plus the adapter's bytes, or
/// nothing when that plan is empty (an identity field).
pub(crate) fn warp_plan(doc: &PrecisionDocument, edit: &WarpEdit) -> Result<SourcePlan, String> {
    doc.indexed_policy().ok_or(NOT_INDEXED)?;
    if !matches!(edit, WarpEdit::Set { .. }) {
        doc.layer(edit.layer()).ok_or("invalid warp layer index")?;
        return Ok(NOTHING);
    }
    let p = doc.without_indexed().warp_plan(edit)?;
    if p.scratch_bytes == 0 {
        return Ok(NOTHING);
    }
    Ok(SourcePlan {
        retained_bytes: p.retained_bytes,
        scratch_bytes: transform_scratch(doc, p.scratch_bytes, doc.dimensions()),
        work_units: p.work_units,
    })
}

/// `warp_edit` on an Indexed document (module doc); `Ok(None)` for Clear,
/// Bake and a set that moves no index.
pub(crate) fn warp_edit(
    doc: &PrecisionDocument,
    edit: &WarpEdit,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<Option<PrecisionDocument>, String> {
    let policy = doc.indexed_policy().ok_or(NOT_INDEXED)?;
    let plan = warp_plan(doc, edit)?;
    let WarpEdit::Set {
        field,
        indexed_background,
        ..
    } = edit
    else {
        return Ok(None);
    };
    if plan.scratch_bytes == 0 {
        return Ok(None);
    }
    let background = background_codes(&doc.without_indexed(), indexed_background.as_ref())?;
    admit(plan.retained_bytes, plan.scratch_bytes, other, limit)?;
    job.reserve(plan.scratch_bytes)?;
    let floor = job.with_reservation_floor(plan.scratch_bytes);
    if field.translation().is_some() {
        return carried(
            doc,
            clear_index(policy.palette(), policy.transparent(), background),
            |carrier, inner| carrier.warp_edit_with_context(edit, limit, other, inner),
            floor,
        );
    }
    let Some(moved) =
        doc.without_indexed()
            .warp_edit_with_context(edit, limit, other, floor.range(0.0, 0.7))?
    else {
        return Ok(None);
    };
    republish(
        doc,
        &moved,
        EdgeInk::Own,
        true,
        background,
        job.range(0.7, 1.0),
    )
}
