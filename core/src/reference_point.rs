//! The layer REFERENCE POINT under a whole-canvas edit or a Free Transform:
//! where Photoshop's `fxrp` (`Layer::reference`, the typed record's
//! `reference`) and a linked pattern fill's anchor go when the canvas is
//! resized, padded, cropped, turned, flipped or rotated, or a layer is
//! transformed, and the turned copy of a linked tile a quarter turn or a flip
//! makes. Pure geometry: the adapter is `doc_reference`, which hands each
//! record's point and the edit's map in and writes the answer back.
//!
//! # The measured rule
//!
//! `scripts/photoshop-probe/reference_point_take.py` (take 1, run
//! 20261017T040216Z-8e2c6e46, its RECORDED RESULT) read `fxrp` before and
//! after each op on three rasters and a group and judged five candidates
//! declared before the take (POINT, STAYS, VECTOR, CORNER, CENTRE). POINT —
//! the point mapped by the op's map M itself, `M(r)` — holds for Image Size,
//! Canvas Size and Crop (phase 1), and, in phase 2, for the Crop tool's
//! angled crop ([`ReferenceOp::Straighten`], the + sign: L1 (7, 3) reads
//! (4.230, −11.641)), the quarter turns and flips, and Free Transform
//! ([`ReferenceOp::Transform`]: FT-S (52, 13.5), FT-R (40.058, −36.373),
//! FT-K (−8.287, 3), FT-F (187, 3) about L1's bounds centre) — every record,
//! groups included.
//!
//! A filtered Rotate Canvas ([`ReferenceOp::Rotate`]) maps the point by
//! SWAP ([`rotate_canvas_map`]): the plan's matrix with its translation
//! moved by the rotated old canvas's margins in the new canvas, SWAPPED —
//! the top margin added to x, the left margin to y. On the
//! `ceil(bw) × ceil(bh)` canvas with the content at the FLOOR of the centre
//! (`doc_rotate::rotated_canvas`) it meets every take1 record
//! (TAKE2 run 20261017T063139Z-1fad1d85: 10°, 45°, 120° and −20° on
//! 120 × 80 and 121 × 81, plus R30; 36 records) within 3e-14 px, where
//! POINT on the same canvas misses by up to 0.750 px. SWAP has no fitted
//! parameter, but it was found from these records' own e = fx + fy, so no
//! held-out row confirms it.
//!
//! # The turned tile
//!
//! A quarter turn or a flip also turns each LINKED fill's tile (take1: the
//! unturned tile misses Photoshop's render by up to 220 codes, the turned one
//! — RENDER-TURNED — by 0, live and reopened): [`turned_tile`] turns it
//! through the one pixel implementation of the canvas turn (`ops`). Free
//! Transform keeps the tile (FT-F's reopened render matches the unturned tile
//! within 0 codes and misses the turned one by 220), and so does an unlinked
//! fill, which take1 did not measure.
//!
//! # Rounding
//!
//! Photoshop keeps the exact fraction (Image Size 120 -> 187 maps (7, 3) to
//! (10.908, 4.7625)); the point is stored as i32 (`.rz` word 91's `LREF`
//! table, no new word), so the image is rounded with `f64::round` — the
//! rule the PSD reader already applies to a fractional `fxrp`
//! (`psd_pattern_fill`) — and saturated to i32. A Photoshop file and the same
//! op in Doctored therefore agree to the pixel, and a round trip through
//! PSD is exact. Repeated resizes round once per op, as layer offsets do
//! (`doc::scaled_offset`).

use std::sync::Arc;

use image::RgbaImage;

use crate::pattern_tile::PatternTile;

/// The edit a point is carried through. Every variant is a family a take
/// measured, and every one ships.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReferenceOp {
    /// Image Size: M = (x W'/W, y H'/H).
    Resize,
    /// Canvas Size: M = the translation by where the old canvas lands.
    CanvasSize,
    /// Crop to a window: M = the translation by minus its corner.
    Crop,
    /// The Crop tool's turned window (`GeometryEdit::Straighten`).
    Straighten,
    /// An exact quarter or half turn of the canvas.
    QuarterTurn,
    /// A canvas flip.
    Flip,
    /// A filtered Rotate Canvas.
    Rotate,
    /// Free Transform of a layer (one or a set, the Ruler's Straighten
    /// Layer): M = its matrix.
    Transform,
}

/// The rules the takes chose; only winners are listed (module doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rule {
    /// The point mapped by the canvas map: `M(r)`.
    Point,
}

fn rule(op: ReferenceOp) -> Option<Rule> {
    match op {
        ReferenceOp::Resize
        | ReferenceOp::CanvasSize
        | ReferenceOp::Crop
        | ReferenceOp::Straighten
        | ReferenceOp::QuarterTurn
        | ReferenceOp::Flip
        | ReferenceOp::Rotate
        | ReferenceOp::Transform => Some(Rule::Point),
    }
}

/// SWAP: `matrix` with (tx, ty) += (fy, fx), where fx = (a·W/2 + c·H/2 + tx) − (W|a| + H|b|)/2
/// and fy = (b·W/2 + d·H/2 + ty) − (W|b| + H|a|)/2 are the rotated old canvas's left and top
/// margins in the new canvas; `before` = (W, H). `matrix` is
/// `doc_rotate::rotated_canvas`'s, `[a, b, c, d, tx, ty]`; the map Photoshop's
/// fxrp follows under a filtered Rotate Canvas (module doc).
pub(crate) fn rotate_canvas_map(matrix: [f64; 6], before: (u32, u32)) -> [f64; 6] {
    let [a, b, c, d, tx, ty] = matrix;
    let (w, h) = (f64::from(before.0), f64::from(before.1));
    let fx = (a * w / 2.0 + c * h / 2.0 + tx) - (w * a.abs() + h * b.abs()) / 2.0;
    let fy = (b * w / 2.0 + d * h / 2.0 + ty) - (w * b.abs() + h * a.abs()) / 2.0;
    [a, b, c, d, tx + fy, ty + fx]
}

/// `point` after `op`, whose canvas map is `matrix` (`[a, b, c, d, tx, ty]`:
/// (x, y) -> (a x + c y + tx, b x + d y + ty)), rounded (module doc) and
/// saturated to i32. `None` when the image is not finite: the caller keeps
/// the point it had.
pub(crate) fn mapped(op: ReferenceOp, point: (i32, i32), matrix: [f64; 6]) -> Option<(i32, i32)> {
    match rule(op)? {
        Rule::Point => {
            let (x, y) = (f64::from(point.0), f64::from(point.1));
            let mx = matrix[0] * x + matrix[2] * y + matrix[4];
            let my = matrix[1] * x + matrix[3] * y + matrix[5];
            (mx.is_finite() && my.is_finite())
                .then(|| (saturated(mx.round()), saturated(my.round())))
        }
    }
}

/// A linked fill's anchor after `op`: `after_offset − mapped(before_offset −
/// anchor)`, so that offset − anchor (what the tile follows, and what the PSD
/// export writes as `fxrp`) moves by the same rule as the reference point.
/// `before_offset` and `after_offset` are the record's offset before and
/// after the op ((0, 0) for a group or another record without pixels).
/// `None` exactly when [`mapped`] is.
pub(crate) fn mapped_anchor(
    op: ReferenceOp,
    anchor: [i32; 2],
    matrix: [f64; 6],
    before_offset: (i32, i32),
    after_offset: (i32, i32),
) -> Option<[i32; 2]> {
    let followed = (
        difference(before_offset.0, anchor[0]),
        difference(before_offset.1, anchor[1]),
    );
    let moved = mapped(op, followed, matrix)?;
    Some([
        difference(after_offset.0, moved.0),
        difference(after_offset.1, moved.1),
    ])
}

/// A linked fill's tile after `op` (module doc): QuarterTurn and Flip only.
/// `matrix`'s linear part, rounded, must be one of R90 [0, 1, −1, 0], R180
/// [−1, 0, 0, −1], R270 [0, −1, 1, 0], FlipH [−1, 0, 0, 1] or FlipV
/// [1, 0, 0, −1], turned through `ops::rotate90/180/270` and
/// `ops::flip_horizontal/vertical` (the one pixel implementation). `None` for
/// every other op or linear part, and for a tile whose bytes do not match its
/// size.
pub(crate) fn turned_tile(
    op: ReferenceOp,
    tile: &PatternTile,
    matrix: [f64; 6],
) -> Option<PatternTile> {
    if !matches!(op, ReferenceOp::QuarterTurn | ReferenceOp::Flip) {
        return None;
    }
    let linear = [matrix[0], matrix[1], matrix[2], matrix[3]].map(f64::round);
    let forms: [([f64; 4], Turn); 5] = [
        ([0.0, 1.0, -1.0, 0.0], crate::ops::rotate90),
        ([-1.0, 0.0, 0.0, -1.0], crate::ops::rotate180),
        ([0.0, -1.0, 1.0, 0.0], crate::ops::rotate270),
        ([-1.0, 0.0, 0.0, 1.0], crate::ops::flip_horizontal),
        ([1.0, 0.0, 0.0, -1.0], crate::ops::flip_vertical),
    ];
    let (_, turn) = forms.iter().find(|(form, _)| *form == linear)?;
    let image = RgbaImage::from_raw(tile.width, tile.height, tile.rgba.to_vec())?;
    let turned = turn(&image);
    Some(PatternTile {
        width: turned.width(),
        height: turned.height(),
        rgba: Arc::from(turned.into_raw()),
    })
}

/// One exact turn of an image (`ops`).
type Turn = fn(&RgbaImage) -> RgbaImage;

fn saturated(v: f64) -> i32 {
    v.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

fn difference(a: i32, b: i32) -> i32 {
    (i64::from(a) - i64::from(b)).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}
