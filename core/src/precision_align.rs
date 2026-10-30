//! Align and Distribute at 16-bit and 32-bit float: the typed twins of
//! the byte `DrDocument::align_layers` / `distribute_layers`, reached as the
//! strict `align_set` / `distribute_set` requests of `dr_precision_doc_edit`.
//!
//! The math is `align_math`'s, shared with the byte ops; the claim semantics
//! (one delta per root, a subtree and link group come along, the last root to
//! claim an entry wins) are byte `doc_align::moved_once`'s, through
//! `align_math::claim_deltas`. The boxes are the typed content box
//! (`precision_content_box`), the twin of the byte `layer_bounds`, with one
//! stated difference: an adjustment layer never has a typed box, while byte
//! `layer_bounds` would box opaque pixels in an adjustment's buffer. Every
//! adjustment the app or a PSD import makes is pixel-less or transparent, so
//! the two depths align the same layers.
//!
//! Every record's translation lands in ONE typed edit
//! (`PrecisionDocument::translate_records`), never a host loop: a loop would
//! pay an edit per root and could leave the document half-aligned.
//!
//! The POSITION lock gate mirrors byte `movable_set`: the whole expanded set
//! is asked UP FRONT, before any delta, so an entry whose delta happens to be
//! zero — or a link-only partner — still refuses the call by name.
//!
//! `precision_align` is the typed `align_set` / `distribute_set` request of
//! `dr_precision_doc_edit`: the POSITION lock gate over the whole expanded set up front (byte
//! `movable_set`), `precision_structure_ops::independent_roots`, the typed content box, then
//! `translate_records`. Byte `dr_doc_align_layers` / `dr_doc_distribute_layers` keep their exports.
//! Tests: `tests/precision_align.rs`.

use serde::Deserialize;

use crate::align_math::{align_deltas, claim_deltas, content_boxes, distribute_deltas, AlignEdge};
use crate::doc_lock::EditKind;
use crate::geometry_set::expand;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_geometry::GeometryPlan;
use crate::precision_structure_ops::{flatten, independent_roots};

const REQUEST_LIMIT: usize = 64 * 1024;

/// A typed align or distribute request.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum AlignEdit {
    /// `{op:"align_set", layers, edge, to_canvas}`: line the roots' content
    /// boxes up on `edge` of their union (or of the canvas).
    AlignSet {
        layers: Vec<usize>,
        edge: AlignEdge,
        to_canvas: bool,
    },
    /// `{op:"distribute_set", layers, vertical}`: space the roots' content
    /// boxes evenly along one axis.
    DistributeSet { layers: Vec<usize>, vertical: bool },
}

impl AlignEdit {
    /// Parses a request, bounded at 64 KiB as `GeometryEdit::parse` is.
    pub fn parse(request: &str) -> Result<Self, String> {
        if request.len() > REQUEST_LIMIT {
            return Err("typed align request exceeds 64 KiB".into());
        }
        serde_json::from_str(request).map_err(|e| format!("invalid typed align request: {e}"))
    }

    fn layers(&self) -> &[usize] {
        match self {
            Self::AlignSet { layers, .. } | Self::DistributeSet { layers, .. } => layers,
        }
    }
}

impl PrecisionDocument {
    /// The plan of the translations `edit` would apply (no lock gate; the
    /// edit asks it).
    pub(crate) fn align_plan(&self, edit: &AlignEdit) -> Result<GeometryPlan, String> {
        self.translation_plan(&self.align_moves(edit)?)
    }

    /// Applies `edit` in one typed edit. `Ok(None)` when nothing would move:
    /// already aligned, fewer than two boxes (three to distribute), or no
    /// content at all.
    pub(crate) fn align_with_context(
        &self,
        edit: &AlignEdit,
        limit: u64,
        other_retained: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        if edit.layers().is_empty() {
            return Err("typed align needs at least one layer".into());
        }
        let mut given = edit.layers().to_vec();
        given.sort_unstable();
        given.dedup();
        for index in expand(&self.set_entries(true), &given)? {
            self.lock_refusal(index, EditKind::Position)?;
        }
        let moves = self.align_moves(edit)?;
        self.translate_records(&moves, limit, other_retained, job)
    }

    /// One translation per record: the given entries reduced to independent
    /// roots, their content boxes, the deltas, then the claims.
    fn align_moves(&self, edit: &AlignEdit) -> Result<Vec<Option<(i32, i32)>>, String> {
        if edit.layers().is_empty() {
            return Err("typed align needs at least one layer".into());
        }
        let roots = independent_roots(&flatten(&self.layers), edit.layers())?;
        let boxes = content_boxes(&roots, |i| self.layer_bounds(i));
        let deltas = match edit {
            AlignEdit::AlignSet {
                edge, to_canvas, ..
            } => align_deltas(&boxes, *edge, to_canvas.then(|| self.dimensions())),
            AlignEdit::DistributeSet { vertical, .. } => distribute_deltas(&boxes, *vertical),
        };
        let Some(deltas) = deltas else {
            return Ok(vec![None; self.records]);
        };
        let entries = self.set_entries(true);
        Ok(claim_deltas(self.records, &deltas, |root| {
            expand(&entries, &[root]).unwrap_or_default()
        }))
    }
}
