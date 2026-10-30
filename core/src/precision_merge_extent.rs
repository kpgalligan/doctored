//! The FRAME a typed Merge Layers / Merge Down renders into: the union of
//! every visible member's extent, at that union's origin — the byte rule of
//! `doc_structure::merged_layer` and `doc::merging_down`, which size their
//! accumulator by `style_composite::merge_extent` and place the merged layer
//! at the union's top-left, so pixels a member holds outside the canvas
//! survive the merge.
//!
//! # The union, member by member
//!
//! Only the members' TOP level is asked, and every VISIBLE member counts,
//! clipped ones included; a hidden record adds nothing.
//!
//! * A RASTER counts as its style's grown rect (`style_extent16_of`, the
//!   bake's own extent), or its pixel rect when it carries no style.
//! * An ADJUSTMENT counts as the canvas rect `(0, 0, w, h)`: the byte twin's
//!   pixels are canvas-sized, so that is its `merge_extent`.
//! * A GROUP or PASS THROUGH group counts as the union of its visible
//!   children's extents, recursively, grown by the group's own style through
//!   `style_grown16` — the shape `doc_group`'s materialized group takes. A
//!   group with nothing visible inside counts as nothing.
//!
//! # The refusals, in the byte order
//!
//! [`frame_of`] refuses an empty union (no member contributes, or a zero
//! width or height), then a union over `MAX_PIXELS`, then a union whose
//! top-left lies below `i32::MIN` — exactly where the byte merges answer
//! `None`, but by name, because a typed edit reaches the host as a sentence.
//!
//! # The handed backdrop
//!
//! A knockout member's `B0` is captured from the document's own projection,
//! which is canvas-sized at (0, 0). [`windowed_seed`] re-indexes those planes
//! onto the frame through `style_knockout_merge::window_plane`, the byte
//! merge's own windowing, so they are transparent outside the canvas there as
//! here.
//!
//! `record_extent16` (a visible raster's `style_extent16_of` or pixel rect, an adjustment's canvas
//! rect, a group's visible children's union grown by `style_grown16`; hidden adds nothing),
//! `frame_of` (refuses "nothing visible", then "covers W×H pixels, over the pixel limit", then
//! "outside the supported coordinate range" — the byte order), `windowed_seed` (a `Handed` seed's
//! canvas planes windowed onto the frame through `style_knockout_merge::window_plane`, the ONE
//! windowing at both depths), and `PrecisionDocument::merge_frame` / `framed_scratch_bytes` (the
//! members document's plan over the frame). Tests: `tests/precision_merge_extent.rs`.

use std::sync::Arc;

use crate::doc::MAX_PIXELS;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_style_knockout::KnockoutSeed;
use crate::precision_style_render::{style_extent16_of, style_grown16};
use crate::style_knockout_merge::window_plane;

/// A merge frame: `(origin, dims)` — the document pixel the frame's pixel
/// (0, 0) is, and the frame's size.
pub(crate) type Frame = ((i32, i32), (u32, u32));

const NOTHING_VISIBLE: &str = "high-depth merge has nothing visible to merge";
const OUT_OF_RANGE: &str = "high-depth merge extends outside the supported coordinate range";

/// The union of two optional rects, `None` the identity.
fn unite(
    a: Option<(i64, i64, i64, i64)>,
    b: Option<(i64, i64, i64, i64)>,
) -> Option<(i64, i64, i64, i64)> {
    match (a, b) {
        (Some(a), Some(b)) => Some((a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))),
        (a, None) => a,
        (None, b) => b,
    }
}

/// What record `layer` of `document` adds to a merge's union (module doc);
/// `None` for a hidden record and for a group with nothing visible inside.
pub(crate) fn record_extent16(
    layer: &PrecisionDocumentLayer,
    document: &PrecisionDocument,
) -> Option<(i64, i64, i64, i64)> {
    if !layer.visible() {
        return None;
    }
    let canvas = document.dimensions();
    match &layer.content {
        // A CMYK record (#1): its style's grown rect, or its pixel rect
        // without one; an adjustment the canvas.
        Content::CmykRaster { image, offset } => Some(
            style_extent16_of(layer, &document.profile, canvas, document.global_light)
                .unwrap_or_else(|| {
                    let (w, h) = image.dimensions();
                    let (x, y) = (i64::from(offset.0), i64::from(offset.1));
                    (x, y, x + i64::from(w), y + i64::from(h))
                }),
        ),
        Content::CmykAdjustment(_) => Some((0, 0, i64::from(canvas.0), i64::from(canvas.1))),
        Content::Raster { image, offset } => Some(
            style_extent16_of(layer, &document.profile, canvas, document.global_light)
                .unwrap_or_else(|| {
                    let (w, h) = image.dimensions();
                    let (x, y) = (i64::from(offset.0), i64::from(offset.1));
                    (x, y, x + i64::from(w), y + i64::from(h))
                }),
        ),
        Content::Adjustment(_) => Some((0, 0, i64::from(canvas.0), i64::from(canvas.1))),
        Content::Group(children) | Content::PassThrough(children) => {
            let inner = children
                .iter()
                .map(|child| record_extent16(child, document))
                .fold(None, unite)?;
            Some(match layer.style() {
                Some(style) => style_grown16(
                    inner,
                    style,
                    &document.profile,
                    canvas,
                    document.global_light,
                ),
                None => inner,
            })
        }
    }
}

/// The frame of a union (module doc), refused by name in the byte order.
pub(crate) fn frame_of(union: Option<(i64, i64, i64, i64)>) -> Result<Frame, String> {
    let (x0, y0, x1, y1) = union.ok_or(NOTHING_VISIBLE)?;
    let (uw, uh) = ((x1 - x0).max(0) as u64, (y1 - y0).max(0) as u64);
    if uw == 0 || uh == 0 {
        return Err(NOTHING_VISIBLE.into());
    }
    if uw.saturating_mul(uh) > MAX_PIXELS {
        return Err(format!(
            "high-depth merge covers {uw}×{uh} pixels, over the pixel limit; crop, trim or scale the layers first"
        ));
    }
    if x0 < i64::from(i32::MIN) || y0 < i64::from(i32::MIN) {
        return Err(OUT_OF_RANGE.into());
    }
    // Both sides are at most `MAX_PIXELS` here, far inside `u32`.
    Ok(((x0 as i32, y0 as i32), (uw as u32, uh as u32)))
}

/// `seed` with a `Handed` seed's planes — captured over the `canvas` at
/// (0, 0) — windowed onto `frame` (module doc). A `Projection` seed carries
/// no plane and passes through unchanged.
pub(crate) fn windowed_seed(seed: KnockoutSeed, canvas: (u32, u32), frame: Frame) -> KnockoutSeed {
    match seed {
        KnockoutSeed::Handed { shallow, deep } => {
            let window = |plane: Option<crate::precision_style_knockout::Plane>| {
                plane.map(|p| Arc::new(window_plane(&p, (0, 0), canvas, frame.0, frame.1)))
            };
            KnockoutSeed::Handed {
                shallow: window(shallow),
                deep: window(deep),
            }
        }
        projection => projection,
    }
}

impl PrecisionDocument {
    /// The frame a merge of `members` (postorder indices, one level) renders
    /// into: the union of their extents, refused by name (module doc).
    pub(crate) fn merge_frame(&self, members: &[usize]) -> Result<Frame, String> {
        let mut union = None;
        for &index in members {
            let layer = self
                .layer(index)
                .ok_or("high-depth layer index is out of range")?;
            union = unite(union, record_extent16(layer, self));
        }
        frame_of(union)
    }

    /// The scratch this document's own render plan charges over `frame`
    /// rather than the canvas — a members document's render in a merge.
    pub(crate) fn framed_scratch_bytes(&self, frame: Frame) -> Result<u64, String> {
        let (scene, _) = self.scene()?;
        Ok(scene
            .with_frame(frame.0, frame.1)?
            .plan(self.render_storage())?
            .scratch_bytes)
    }
}
