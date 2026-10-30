//! PATHS at high depth — the typed twin of `doc_path`'s
//! `impl DrDocument` block (the `precision_sampler` pattern), the typed
//! document's path state and the one dispatcher over a `GeometryEdit`.
//! Every rule — the list rules, the caps, the identity and the point maps —
//! is `doc_path`'s; a path holds no sample, so nothing here depends on the
//! document's depth.

use std::sync::Arc;

use crate::doc::Geometry;
use crate::doc_path::{
    accepted, cropped_paths, geometry_paths, matrix_paths, padded_paths, resized_paths, PathEntry,
};
use crate::precision_document::PrecisionDocument;
use crate::precision_geometry::GeometryEdit;

/// The typed document's path state: ONE field of `PrecisionDocument`, the
/// twin of `doc_path::DocPaths`.
#[derive(Clone, Debug, Default)]
pub(crate) struct TypedPaths {
    /// The list, the work path first when there is one; `Arc`-shared for
    /// `DocPaths::entries`' reason, and with it across a depth conversion.
    pub(crate) entries: Arc<Vec<PathEntry>>,
    /// The typed VECTOR MASK records (`precision_vector_mask`), the twin of
    /// `DocPaths::vector_masks` and live by the same rule.
    pub(crate) vector_masks: Arc<Vec<crate::precision_vector_mask::TypedVectorMaskRecord>>,
}

impl TypedPaths {
    /// Replaces the entries in place — for a result document still being
    /// built.
    pub(crate) fn set_entries(&mut self, entries: Vec<PathEntry>) {
        self.entries = Arc::new(entries);
    }
}

/// The paths after `edit` on a `w × h` canvas — the typed geometry fold's
/// one call, and the byte Straighten arm's. The arms are
/// `doc_sampler::edited_samplers`' with each sampler map replaced by its
/// path map: a per-layer transform leaves the paths alone, and Rotate Canvas
/// delegates a quarter turn to [`geometry_paths`] and is the identity for a
/// whole turn.
pub(crate) fn edited_paths(
    list: &[PathEntry],
    edit: &GeometryEdit,
    w: u32,
    h: u32,
) -> Result<Vec<PathEntry>, String> {
    use crate::doc_rotate::{rotated_canvas, rotation_kind, RotationKind};
    Ok(match edit {
        GeometryEdit::Rotate90 => geometry_paths(list, Geometry::Rotate90, w, h),
        GeometryEdit::Rotate180 => geometry_paths(list, Geometry::Rotate180, w, h),
        GeometryEdit::Rotate270 => geometry_paths(list, Geometry::Rotate270, w, h),
        GeometryEdit::FlipHorizontal => geometry_paths(list, Geometry::FlipH, w, h),
        GeometryEdit::FlipVertical => geometry_paths(list, Geometry::FlipV, w, h),
        GeometryEdit::Resize { width, height, .. } => resized_paths(
            list,
            f64::from(*width) / f64::from(w),
            f64::from(*height) / f64::from(h),
        ),
        GeometryEdit::Crop { x, y, .. } => cropped_paths(list, *x, *y),
        GeometryEdit::CanvasSize { x, y, .. } => padded_paths(list, (*x, *y)),
        GeometryEdit::Straighten { matrix, x, y, .. } => matrix_paths(list, *matrix, (*x, *y)),
        GeometryEdit::RotateCanvas { degrees, .. } => {
            let (kind, normalized) =
                rotation_kind(*degrees).ok_or("rotation degrees must be finite")?;
            match kind {
                RotationKind::Identity => list.to_vec(),
                RotationKind::Rotate90 => geometry_paths(list, Geometry::Rotate90, w, h),
                RotationKind::Rotate180 => geometry_paths(list, Geometry::Rotate180, w, h),
                RotationKind::Rotate270 => geometry_paths(list, Geometry::Rotate270, w, h),
                RotationKind::Filtered => {
                    let plan = rotated_canvas(w, h, normalized)
                        .ok_or("the rotated canvas would exceed the supported canvas size")?;
                    matrix_paths(list, plan.matrix, (0, 0))
                }
            }
        }
        GeometryEdit::Affine { .. }
        | GeometryEdit::Perspective { .. }
        | GeometryEdit::AffineSet { .. }
        | GeometryEdit::PerspectiveSet { .. } => list.to_vec(),
    })
}

impl PrecisionDocument {
    /// The document's paths, the work path first when there is one.
    pub fn paths(&self) -> &[PathEntry] {
        &self.paths.entries
    }

    /// `DrDocument::with_paths` at high depth: `None` for a list
    /// `doc_path::accepted` refuses or the list the document already holds.
    pub fn with_paths(&self, list: Vec<PathEntry>) -> Option<Self> {
        let entries = accepted(list, &self.paths.entries, self.vector_mask_anchor_count())?;
        let mut next = self.clone();
        next.paths.set_entries(entries);
        // A path write is where dead vector-mask records are dropped.
        next.prune_vector_masks();
        Some(next)
    }
}
