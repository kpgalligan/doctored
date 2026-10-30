//! The TYPED layer hit test: [`PrecisionDocument::layer_at`], the twin
//! of `DrDocument::layer_at` (`doc_group_query::layer_at`, the byte master)
//! at 16-bit and 32-bit float. The record a click at canvas `(x, y)`
//! activates, as a postorder record index (`PrecisionDocument::layer`'s).
//!
//! THE WALK. Top-down over each level (`root_layers()`, children in reverse),
//! as `doc_group_query::hit_in_level` walks the byte composite order: a
//! hidden record is skipped with its whole subtree; an adjustment record is
//! skipped; a group (isolated or Pass Through) hits when a descendant hits,
//! its own mask ignored; `clipped` is ignored, as the byte walk ignores it.
//! With `top_level` the answer is the hit record's top-level ancestor
//! (Auto-Select: Group), else the hit record itself.
//!
//! THE HIT RULE. A raster record hits when the canvas point lies inside its
//! image and `alpha * coverage >= 0.5`, evaluated in f64: alpha is the
//! normalized sample (`PrecisionImage::sample_at`: u16 `/ 65535`, f32
//! verbatim) clamped to `[0, 1]`, a NaN alpha missing; coverage is 1 with no
//! mask or a disabled one, else the prepared mask's effective coverage at the
//! canvas point (feather, density and outside coverage applied). A mask whose
//! feather is not prepared here (its planned work is over the interactive
//! budget `precision_render::MAX_WORK_UNITS`; a hit test has no job control)
//! counts as no mask, the fallback `precision_content_box` takes
//! (`prepared_within_budget`, the one check). An alpha below one half misses
//! without preparing the mask. This equals the byte `round(a·m/255) >= 128`
//! for a 16-bit promotion at density 1 with no feather; density and feather
//! are evaluated at typed precision and may flip an edge pixel by design.
//!
//! No memo and no feather cache: the byte hit test has no memo, and the typed
//! render re-prepares a mask per use too.
//!
//! `PrecisionDocument::layer_at`, the 16/32-bit twin of the byte `DrDocument::layer_at`
//! (`doc_group_query.rs`, the master) behind `dr_precision_doc_layer_at` — Move Auto-select and
//! `auto_select_layer` at depth. Same top-down walk (hidden subtrees and adjustments skipped, a
//! group hit through its descendants, its own mask ignored; `top_level` answers the top-level
//! ancestor).

use crate::precision_content_box::prepared_within_budget;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::PrecisionImage;

/// The product `alpha * coverage` at or above which a raster record hits.
const HIT_COVERAGE: f64 = 0.5;

impl PrecisionDocument {
    /// `DrDocument::layer_at`'s typed twin: the postorder index of the
    /// topmost visible raster record whose `alpha * coverage` at canvas
    /// `(x, y)` is at least one half (module doc for the exact rule), or with
    /// `top_level` its top-level ancestor. `None` when nothing hits.
    pub fn layer_at(&self, x: i32, y: i32, top_level: bool) -> Option<usize> {
        hit_in_level(self.root_layers(), 0, i64::from(x), i64::from(y), top_level)
    }
}

/// Top-down over one level whose first record's subtree starts at postorder
/// index `base`, recursing into groups. With `top_level` a hit inside a
/// group answers the group's own index (only ever set at the root level).
fn hit_in_level(
    level: &[PrecisionDocumentLayer],
    base: usize,
    x: i64,
    y: i64,
    top_level: bool,
) -> Option<usize> {
    let mut end = base + level.iter().map(|record| record.records).sum::<usize>();
    for record in level.iter().rev() {
        let start = end - record.records;
        let own = end - 1;
        end = start;
        if !record.visible {
            continue;
        }
        match &record.content {
            Content::CmykRaster { image, offset } => {
                let (lx, ly) = (x - i64::from(offset.0), y - i64::from(offset.1));
                if lx >= 0
                    && ly >= 0
                    && u32::try_from(lx)
                        .ok()
                        .zip(u32::try_from(ly).ok())
                        .and_then(|(lx, ly)| image.sample_at(lx, ly))
                        .is_some_and(|s| s[4] > 0)
                {
                    return Some(own);
                }
            }
            Content::Adjustment(_) | Content::CmykAdjustment(_) => {}
            Content::Group(children) | Content::PassThrough(children) => {
                if let Some(hit) = hit_in_level(children, start, x, y, false) {
                    return Some(if top_level { own } else { hit });
                }
            }
            Content::Raster { image, offset } => {
                if covers(record, image, *offset, x, y) {
                    return Some(own);
                }
            }
        }
    }
    None
}

/// Whether a raster record's own `alpha * coverage` at canvas `(x, y)`
/// reaches [`HIT_COVERAGE`] (module doc).
fn covers(
    record: &PrecisionDocumentLayer,
    image: &PrecisionImage,
    offset: (i32, i32),
    x: i64,
    y: i64,
) -> bool {
    let (w, h) = image.dimensions();
    let lx = x - i64::from(offset.0);
    let ly = y - i64::from(offset.1);
    if lx < 0 || ly < 0 || lx >= i64::from(w) || ly >= i64::from(h) {
        return false;
    }
    let Some(sample) = image.sample_at(lx as u32, ly as u32) else {
        return false;
    };
    let alpha = sample[3];
    if alpha.is_nan() {
        return false;
    }
    let alpha = f64::from(alpha.clamp(0.0, 1.0));
    if alpha < HIT_COVERAGE {
        return false;
    }
    let coverage = match record
        .mask
        .as_ref()
        .filter(|mask| mask.enabled())
        .and_then(prepared_within_budget)
    {
        // No enabled mask, or one whose feather is not prepared here.
        None => 1.0,
        Some(prepared) => f64::from(prepared.at(x, y)),
    };
    alpha * coverage >= HIT_COVERAGE
}
