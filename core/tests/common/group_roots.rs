//! Test fixture: every root of a typed document wrapped in one group at an
//! opacity, built through the same `group_layers` edit and opacity edit the
//! app uses. A suite root mounts it once and its files
//! `use crate::group_roots;`; a standalone binary mounts it with
//! `#[path = "common/group_roots.rs"]`. Then `use group_roots::GroupRoots;`.
#![allow(dead_code)]

use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};

/// Records in a layer's subtree, the layer included.
fn records(layer: &PrecisionDocumentLayer) -> usize {
    1 + layer
        .children()
        .map_or(0, |children| children.iter().map(records).sum())
}

/// Flat (children-before-group) indices of the document's roots, bottom first.
pub fn root_indices(document: &PrecisionDocument) -> Vec<usize> {
    let mut next = 0;
    document
        .root_layers()
        .iter()
        .map(|root| {
            next += records(root);
            next - 1
        })
        .collect()
}

pub trait GroupRoots {
    /// Every root in one isolated Normal group at `opacity`.
    fn group_all(&self, opacity: f32) -> Result<Option<PrecisionDocument>, String>;
    /// Every root in one Pass Through group at `opacity`.
    fn group_all_pass_through(&self, opacity: f32) -> Result<Option<PrecisionDocument>, String>;
}

impl GroupRoots for PrecisionDocument {
    fn group_all(&self, opacity: f32) -> Result<Option<PrecisionDocument>, String> {
        group(self, opacity, false)
    }
    fn group_all_pass_through(&self, opacity: f32) -> Result<Option<PrecisionDocument>, String> {
        group(self, opacity, true)
    }
}

fn group(
    document: &PrecisionDocument,
    opacity: f32,
    pass_through: bool,
) -> Result<Option<PrecisionDocument>, String> {
    let Some((grouped, report)) =
        document.group_layers(&root_indices(document), "Group", pass_through)?
    else {
        return Ok(None);
    };
    // An unchanged opacity is a no-op answered with None.
    Ok(Some(
        grouped
            .set_layer_opacity(report.group, opacity)?
            .unwrap_or(grouped),
    ))
}
