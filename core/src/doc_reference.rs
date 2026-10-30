//! The adapter of the reference-point rule (`reference_point`): each
//! whole-canvas op and each Free Transform hands its result and its map here,
//! and every record's reference point (`Layer::reference`, the typed
//! record's) and linked pattern anchors are set FROM the record before the op
//! through the kernel; a quarter turn or a flip also gives each linked fill a
//! turned copy of its tile ([`TurnedTiles`]). No geometry of its own beyond
//! naming each op's canvas map.
//!
//! # Byte ([`follow_layer`], `DrDocument::following_references`)
//!
//! The hook OVERWRITES from the entry before the op, so it is idempotent: a
//! plain op called inside another (the Straighten arm's crop and
//! `straighten_layers`' per-layer transforms, `exact_source_geometry`'s and
//! `resize_source_geometry`'s plain ops, `rotate_canvas_with_job` over
//! `straighten_layers`) hooks too, and the outer call supersedes it, tiles
//! included: they turn from the entry before the op, never from the inner
//! result, so the inner Free Transform steps `straighten_layers` makes never
//! leak into a filtered Rotate Canvas, which steps by its own map
//! (`reference_point::rotate_canvas_map`). An image that is not finite
//! copies the point and the anchors over unchanged. Free Transform hooks per
//! entry: `doc_transform`'s `transform_layer_unlocked_with_job` and
//! `doc_align`'s group arm.
//!
//! # Typed (`follow_record`)
//!
//! `precision_geometry` asks [`edit_map`] once per edit and steps every
//! record after its Scale Effects and Delete Cropped Pixels rebase, against
//! the record's FINAL (trimmed) offset — so the trim's anchor rebase is kept
//! (offset − anchor does not depend on the trim) — and a Free Transform
//! record by its plan's `reference_map`. An image that is not finite leaves
//! the record as the plan made it. A style the step replaces (a turned tile,
//! a moved linked anchor) is an edit there; a moved reference alone is not.
//!
//! A group (and any other record without pixels) passes offset (0, 0) both
//! sides, never its children's union (`doc::group_offset`, the typed
//! `PrecisionDocumentLayer::offset`).
use std::cell::RefCell;
use std::sync::Arc;

use crate::doc::{DrDocument, Geometry, Layer, LayerKind};
use crate::pattern_tile::PatternTile;
use crate::precision_document::PrecisionDocumentLayer;
use crate::precision_geometry::GeometryEdit;
use crate::reference_point::{mapped, mapped_anchor, rotate_canvas_map, turned_tile, ReferenceOp};
use crate::style_pattern::{remapped_arc, turned_arc, Pattern, PatternSource};

/// One op's turned copies, memoized by the source tile (`Arc::ptr_eq`, else
/// equal texels: two layers whose styles were set from the same JSON hold
/// equal tiles in separate `Arc`s), so every layer sharing a tile shares one
/// copy (Photoshop's one new `Idnt`).
#[derive(Default)]
pub(crate) struct TurnedTiles(RefCell<Vec<(Arc<PatternTile>, Arc<PatternTile>)>>);

impl TurnedTiles {
    /// `pattern`'s turned copy after `op`: a Custom pattern under `pattern`'s
    /// name holding [`turned_tile`] (take1: same name, new id —
    /// `psd_patterns::pattern_id` hashes the new pixels). `None` when
    /// [`turned_tile`] is.
    pub(crate) fn turned(
        &self,
        op: ReferenceOp,
        matrix: [f64; 6],
        pattern: &Pattern,
    ) -> Option<Pattern> {
        let source = &pattern.tile;
        let known = self
            .0
            .borrow()
            .iter()
            .find(|(from, _)| Arc::ptr_eq(from, source) || **from == **source)
            .map(|(_, to)| Arc::clone(to));
        let tile = match known {
            Some(tile) => tile,
            None => {
                let tile = Arc::new(turned_tile(op, source, matrix)?);
                self.0
                    .borrow_mut()
                    .push((Arc::clone(source), Arc::clone(&tile)));
                tile
            }
        };
        Some(Pattern {
            source: PatternSource::Custom {
                name: pattern.name(),
            },
            tile,
        })
    }
}

/// A pattern fill LAYER's tile after a canvas turn or flip whose linear part
/// is `linear` (`[a, b, c, d]`: (x, y) -> (a x + c y, b x + d y)), for the
/// host's described layers: `pattern` is a `{"preset"}` or
/// `{"name", "width", "height", "rgba"}` object, parsed by
/// `artwork_render::pattern_fill`. `Ok(Some)` with the canonical pattern
/// object ([`TurnedTiles::turned`]: a Custom pattern under its display
/// name) only when each element is within `doc_transform::EXACT_EPSILON` of
/// one of [`turned_tile`]'s five forms, so a filtered rotation near a quarter
/// turn never relies on `turned_tile`'s own rounding; `Ok(None)` for any
/// other linear part; `Err` for a pattern that does not parse.
pub(crate) fn turn_pattern(
    pattern: &serde_json::Value,
    linear: [f64; 4],
) -> Result<Option<serde_json::Value>, String> {
    let mut holder = serde_json::Map::new();
    holder.insert("pattern".into(), pattern.clone());
    let fill = crate::artwork_render::pattern_fill(&serde_json::Value::Object(holder))?;
    let forms: [[f64; 4]; 5] = [
        [0.0, 1.0, -1.0, 0.0],
        [-1.0, 0.0, 0.0, -1.0],
        [0.0, -1.0, 1.0, 0.0],
        [-1.0, 0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, -1.0],
    ];
    let exact = forms.iter().any(|form| {
        form.iter()
            .zip(linear)
            .all(|(f, v)| (v - f).abs() <= crate::doc_transform::EXACT_EPSILON)
    });
    if !exact {
        return Ok(None);
    }
    let [a, b, c, d] = linear;
    let op = if a * d - b * c > 0.0 {
        ReferenceOp::QuarterTurn
    } else {
        ReferenceOp::Flip
    };
    Ok(TurnedTiles::default()
        .turned(op, [a, b, c, d, 0.0, 0.0], &fill.pattern)
        .map(|turned| crate::style_pattern_fill::pattern_value(&turned)))
}

/// One byte entry's step, from `before` (module doc): the reference, the
/// linked anchors (against `next`'s offset, (0, 0) for a group), then the
/// tiles. An image that is not finite copies `before`'s point and anchors.
pub(crate) fn follow_layer(
    before: &Layer,
    next: &mut Layer,
    op: ReferenceOp,
    matrix: [f64; 6],
    tiles: &TurnedTiles,
) {
    let offset = |layer: &Layer| match layer.kind {
        LayerKind::Group => (0, 0),
        LayerKind::Raster => layer.offset,
    };
    let (from, to) = (offset(before), offset(next));
    next.reference = mapped(op, before.reference, matrix).unwrap_or(before.reference);
    next.style = remapped_arc(&next.style, before.style.as_deref(), |anchor| {
        Some(mapped_anchor(op, anchor, matrix, from, to).unwrap_or(anchor))
    });
    next.style = turned_arc(&next.style, before.style.as_deref(), |pattern| {
        tiles.turned(op, matrix, pattern)
    });
}

impl DrDocument {
    /// Layer `idx`'s reference point (`Layer::reference`, the `fxrp`): where
    /// its linked pattern fill's origin follows. `None` out of range.
    pub fn layer_reference(&self, idx: usize) -> Option<(i32, i32)> {
        Some(self.layers.get(idx)?.reference)
    }

    /// `out` (this document after a whole-canvas op; the same layer list)
    /// with every entry stepped FROM `self`'s by [`follow_layer`], one
    /// [`TurnedTiles`] for the op (module doc). `out` unchanged when the layer
    /// counts differ.
    pub(crate) fn following_references(
        &self,
        mut out: Self,
        op: ReferenceOp,
        matrix: [f64; 6],
    ) -> Self {
        if out.layers.len() != self.layers.len() {
            return out;
        }
        let tiles = TurnedTiles::default();
        for (before, next) in self.layers.iter().zip(out.layers.iter_mut()) {
            follow_layer(before, next, op, matrix, &tiles);
        }
        out
    }

    /// [`Self::following_references`] for a translation by `(dx, dy)`: Crop
    /// (minus the window corner) and Canvas Size (where the old canvas lands).
    pub(crate) fn following_translation(
        &self,
        out: Self,
        op: ReferenceOp,
        dx: i64,
        dy: i64,
    ) -> Self {
        self.following_references(out, op, [1.0, 0.0, 0.0, 1.0, dx as f64, dy as f64])
    }

    /// [`Self::following_references`] for Image Size from this canvas to
    /// `out`'s.
    pub(crate) fn following_resize(&self, out: Self) -> Self {
        let (sx, sy) = (
            f64::from(out.width) / f64::from(self.width),
            f64::from(out.height) / f64::from(self.height),
        );
        self.following_references(out, ReferenceOp::Resize, [sx, 0.0, 0.0, sy, 0.0, 0.0])
    }

    /// [`Self::following_references`] for an exact turn or flip of this
    /// canvas ([`geometry_map`]).
    pub(crate) fn following_geometry(&self, out: Self, geom: Geometry) -> Self {
        let (op, matrix) = geometry_map(geom, self.width, self.height);
        self.following_references(out, op, matrix)
    }
}

/// An exact turn or flip of a `w × h` canvas as its family and canvas map:
/// the byte `geometry`'s own point maps (R90 (h − y, x), R180 (w − x, h − y),
/// R270 (y, w − x), FlipH (w − x, y), FlipV (x, h − y)).
pub(crate) fn geometry_map(geom: Geometry, w: u32, h: u32) -> (ReferenceOp, [f64; 6]) {
    let (w, h) = (f64::from(w), f64::from(h));
    match geom {
        Geometry::Rotate90 => (ReferenceOp::QuarterTurn, [0.0, 1.0, -1.0, 0.0, h, 0.0]),
        Geometry::Rotate180 => (ReferenceOp::QuarterTurn, [-1.0, 0.0, 0.0, -1.0, w, h]),
        Geometry::Rotate270 => (ReferenceOp::QuarterTurn, [0.0, -1.0, 1.0, 0.0, 0.0, w]),
        Geometry::FlipH => (ReferenceOp::Flip, [-1.0, 0.0, 0.0, 1.0, w, 0.0]),
        Geometry::FlipV => (ReferenceOp::Flip, [1.0, 0.0, 0.0, -1.0, 0.0, h]),
    }
}

/// The window corner folded into a Straighten matrix: `T(−x, −y) ∘ m`.
pub(crate) fn windowed(matrix: [f64; 6], x: i32, y: i32) -> [f64; 6] {
    [
        matrix[0],
        matrix[1],
        matrix[2],
        matrix[3],
        matrix[4] - f64::from(x),
        matrix[5] - f64::from(y),
    ]
}

/// The typed whole-canvas edit's family and canvas map on a `w × h` canvas,
/// in `precision_path::edited_paths`' dispatch: the turns and flips by
/// [`geometry_map`] (as edits and as quarter-turn Rotate Canvas kinds),
/// Resize its scale, Crop and Canvas Size their translation, Straighten its
/// windowed matrix (an identity matrix is a Crop), a filtered Rotate Canvas
/// `rotate_canvas_map` of `rotated_canvas`' matrix; `None` for the per-layer
/// edits and a whole-turn Rotate Canvas.
pub(crate) fn edit_map(
    edit: &GeometryEdit,
    w: u32,
    h: u32,
) -> Result<Option<(ReferenceOp, [f64; 6])>, String> {
    use crate::doc_rotate::{rotated_canvas, rotation_kind, RotationKind};
    let translation = |dx: f64, dy: f64| [1.0, 0.0, 0.0, 1.0, dx, dy];
    Ok(match edit {
        GeometryEdit::Rotate90 => Some(geometry_map(Geometry::Rotate90, w, h)),
        GeometryEdit::Rotate180 => Some(geometry_map(Geometry::Rotate180, w, h)),
        GeometryEdit::Rotate270 => Some(geometry_map(Geometry::Rotate270, w, h)),
        GeometryEdit::FlipHorizontal => Some(geometry_map(Geometry::FlipH, w, h)),
        GeometryEdit::FlipVertical => Some(geometry_map(Geometry::FlipV, w, h)),
        GeometryEdit::Resize { width, height, .. } => Some((
            ReferenceOp::Resize,
            [
                f64::from(*width) / f64::from(w),
                0.0,
                0.0,
                f64::from(*height) / f64::from(h),
                0.0,
                0.0,
            ],
        )),
        GeometryEdit::Crop { x, y, .. } => Some((
            ReferenceOp::Crop,
            translation(-f64::from(*x), -f64::from(*y)),
        )),
        GeometryEdit::CanvasSize { x, y, .. } => Some((
            ReferenceOp::CanvasSize,
            translation(f64::from(*x), f64::from(*y)),
        )),
        GeometryEdit::Straighten { matrix, x, y, .. } => {
            let identity = matrix
                .iter()
                .zip([1., 0., 0., 1., 0., 0.])
                .all(|(a, b)| (*a - b).abs() <= crate::doc_transform::EXACT_EPSILON);
            Some(if identity {
                (
                    ReferenceOp::Crop,
                    translation(-f64::from(*x), -f64::from(*y)),
                )
            } else {
                (ReferenceOp::Straighten, windowed(*matrix, *x, *y))
            })
        }
        GeometryEdit::RotateCanvas { degrees, .. } => {
            let (kind, normalized) =
                rotation_kind(*degrees).ok_or("rotation degrees must be finite")?;
            match kind {
                RotationKind::Identity => None,
                RotationKind::Rotate90 => Some(geometry_map(Geometry::Rotate90, w, h)),
                RotationKind::Rotate180 => Some(geometry_map(Geometry::Rotate180, w, h)),
                RotationKind::Rotate270 => Some(geometry_map(Geometry::Rotate270, w, h)),
                RotationKind::Filtered => {
                    let plan = rotated_canvas(w, h, normalized)
                        .ok_or("the rotated canvas would exceed the supported canvas size")?;
                    Some((ReferenceOp::Rotate, rotate_canvas_map(plan.matrix, (w, h))))
                }
            }
        }
        GeometryEdit::Affine { .. }
        | GeometryEdit::Perspective { .. }
        | GeometryEdit::AffineSet { .. }
        | GeometryEdit::PerspectiveSet { .. } => None,
    })
}

/// One typed record's step (module doc): `next.reference` and `next.style`'s
/// linked anchors from `before` by the kernel, against `next`'s final
/// offset, then its linked tiles from `before`'s by `tiles`; `next`
/// unchanged where the image is not finite.
pub(crate) fn follow_record(
    before: &PrecisionDocumentLayer,
    next: &mut PrecisionDocumentLayer,
    op: ReferenceOp,
    matrix: [f64; 6],
    tiles: &TurnedTiles,
) {
    if let Some(reference) = mapped(op, before.reference, matrix) {
        next.reference = reference;
    }
    let (from, to) = (before.offset(), next.offset());
    next.style = remapped_arc(&next.style, before.style.as_deref(), |anchor| {
        mapped_anchor(op, anchor, matrix, from, to)
    });
    next.style = turned_arc(&next.style, before.style.as_deref(), |pattern| {
        tiles.turned(op, matrix, pattern)
    });
}
