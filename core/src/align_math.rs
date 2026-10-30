//! The align / distribute MATH both depths share: the edge vocabulary,
//! the content boxes, the per-root deltas and the claim rule that turns them
//! into one translation per entry. Pure functions over indices and boxes —
//! no document model, no pixels — so the byte ops (`doc_align`) and the typed
//! ones (`precision_align`) cannot word one rule two ways.
//!
//! The byte `doc_align` module doc is the master of the SEMANTICS: content
//! boxes rather than pixel rects, an entry with nothing opaque skipped,
//! "distribute spacing", and [`claim_deltas`]' last-root-wins
//! rule for entries two roots reach (`DrDocument::moved_once`).
//!
//! `align_math` is the shared math: `AlignEdge` (serde `align_layers` vocabulary), `content_boxes`,
//! `align_deltas`, `distribute_deltas` and `claim_deltas` (one delta per root, subtree and link
//! group follow, the last root to claim an entry wins — byte `doc_align::moved_once`'s rule, which
//! now calls it).

use serde::Deserialize;

use crate::doc::saturating_i32;

/// Which edge of the reference rect an alignment lines entries up on. Mirrors
/// `DrAlign` in the C header; the serde names are `align_layers`' MCP
/// vocabulary, which the typed `align_set` request takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
#[repr(i32)]
#[serde(rename_all = "snake_case")]
pub enum AlignEdge {
    /// Left edges together.
    Left = 0,
    /// Horizontal centres together.
    #[serde(rename = "horizontal_center")]
    CenterX = 1,
    /// Right edges together.
    Right = 2,
    /// Top edges together.
    Top = 3,
    /// Vertical centres together.
    #[serde(rename = "vertical_center")]
    CenterY = 4,
    /// Bottom edges together.
    Bottom = 5,
}

impl AlignEdge {
    /// Maps a raw `DrAlign` value coming across the FFI.
    pub fn from_c(value: i32) -> Option<Self> {
        match value {
            0 => Some(AlignEdge::Left),
            1 => Some(AlignEdge::CenterX),
            2 => Some(AlignEdge::Right),
            3 => Some(AlignEdge::Top),
            4 => Some(AlignEdge::CenterY),
            5 => Some(AlignEdge::Bottom),
            _ => None,
        }
    }

    /// Whether this edge moves an entry along the Y axis.
    pub(crate) fn vertical(self) -> bool {
        matches!(
            self,
            AlignEdge::Top | AlignEdge::CenterY | AlignEdge::Bottom
        )
    }
}

/// A CONTENT box as `(x0, y0, x1, y1)`, exclusive, in canvas coordinates.
pub(crate) type ContentBox = (i64, i64, i64, i64);

/// The given roots paired with their content boxes, skipping every root
/// `bounds` answers `None` for (nothing opaque). `bounds` is the depth's
/// `layer_bounds` — `(x, y, w, h)`.
pub(crate) fn content_boxes(
    roots: &[usize],
    bounds: impl Fn(usize) -> Option<(i32, i32, u32, u32)>,
) -> Vec<(usize, ContentBox)> {
    roots
        .iter()
        .filter_map(|&i| {
            let (x, y, w, h) = bounds(i)?;
            Some((
                i,
                (
                    i64::from(x),
                    i64::from(y),
                    i64::from(x) + i64::from(w),
                    i64::from(y) + i64::from(h),
                ),
            ))
        })
        .collect()
}

/// One delta per box, lining every box up on `edge` of the reference rect:
/// the canvas when `canvas` is given, else the UNION of the boxes.
///
/// `None` for no boxes, and for fewer than two when `canvas` is `None` —
/// aligning one layer to itself is a no-op by definition.
pub(crate) fn align_deltas(
    boxes: &[(usize, ContentBox)],
    edge: AlignEdge,
    canvas: Option<(u32, u32)>,
) -> Option<Vec<(usize, i32, i32)>> {
    let first = boxes.first()?.1;
    let reference = match canvas {
        Some((w, h)) => (0, 0, i64::from(w), i64::from(h)),
        None if boxes.len() < 2 => return None,
        None => boxes.iter().skip(1).fold(first, |acc, b| {
            (
                acc.0.min(b.1 .0),
                acc.1.min(b.1 .1),
                acc.2.max(b.1 .2),
                acc.3.max(b.1 .3),
            )
        }),
    };
    Some(
        boxes
            .iter()
            .map(|&(i, rect)| {
                let delta = align_delta(edge, rect, reference);
                if edge.vertical() {
                    (i, 0, saturating_i32(delta))
                } else {
                    (i, saturating_i32(delta), 0)
                }
            })
            .collect(),
    )
}

/// One delta per box spacing them evenly along one axis: the two OUTERMOST
/// keep their positions and the gaps between adjacent boxes are made equal.
/// A negative gap (content wider than its span) is arithmetic, not an error.
///
/// `None` for fewer than three boxes.
pub(crate) fn distribute_deltas(
    boxes: &[(usize, ContentBox)],
    vertical: bool,
) -> Option<Vec<(usize, i32, i32)>> {
    if boxes.len() < 3 {
        return None;
    }
    // (entry, start along the axis, size along the axis), ordered by
    // start; the index breaks a tie so the walk is deterministic.
    let mut spans: Vec<(usize, i64, i64)> = boxes
        .iter()
        .map(|&(i, (x0, y0, x1, y1))| {
            if vertical {
                (i, y0, y1 - y0)
            } else {
                (i, x0, x1 - x0)
            }
        })
        .collect();
    spans.sort_by_key(|&(i, start, _)| (start, i));
    let first = spans[0].1;
    let last = spans[spans.len() - 1];
    let span = last.1 + last.2 - first;
    let content: i64 = spans.iter().map(|&(_, _, size)| size).sum();
    let total_gap = span - content;
    let steps = (spans.len() - 1) as f64;
    let mut deltas: Vec<(usize, i32, i32)> = Vec::with_capacity(spans.len());
    let mut consumed = 0i64;
    for (k, &(i, start, size)) in spans.iter().enumerate() {
        // The k-th entry sits after every earlier entry's content plus its
        // exact share of the gap. Rounding the SHARE rather than the gap
        // keeps the last entry's target exactly where it already is, so
        // the two outermost never move by a rounding residue.
        let share = ((k as f64) * (total_gap as f64) / steps).round() as i64;
        let target = first + consumed + share;
        consumed += size;
        let delta = target - start;
        deltas.push(if vertical {
            (i, 0, saturating_i32(delta))
        } else {
            (i, saturating_i32(delta), 0)
        });
    }
    Some(deltas)
}

/// The translation each of `records` entries takes: every root's delta is
/// claimed by every entry `expand` reaches from it (subtree, then link
/// group), and the LAST root to claim an entry wins — the byte
/// `DrDocument::moved_once` rule, so each entry moves exactly once. A zero
/// delta claims nothing; an index past `records` is ignored.
pub(crate) fn claim_deltas(
    records: usize,
    deltas: &[(usize, i32, i32)],
    mut expand: impl FnMut(usize) -> Vec<usize>,
) -> Vec<Option<(i32, i32)>> {
    let mut claimed: Vec<Option<(i32, i32)>> = vec![None; records];
    for &(root, dx, dy) in deltas {
        if dx == 0 && dy == 0 {
            continue;
        }
        for i in expand(root) {
            if let Some(slot) = claimed.get_mut(i) {
                *slot = Some((dx, dy));
            }
        }
    }
    claimed
}

/// How far entry `rect` must travel along `edge`'s axis to line up with
/// `reference`. Both rects are `(x0, y0, x1, y1)`, exclusive.
fn align_delta(edge: AlignEdge, rect: ContentBox, reference: ContentBox) -> i64 {
    match edge {
        AlignEdge::Left => reference.0 - rect.0,
        AlignEdge::Right => reference.2 - rect.2,
        AlignEdge::Top => reference.1 - rect.1,
        AlignEdge::Bottom => reference.3 - rect.3,
        // Centres in doubled coordinates, so an odd width does not lose half
        // a pixel before the subtraction; the halving rounds toward zero,
        // which is what an integer offset can express.
        AlignEdge::CenterX => ((reference.0 + reference.2) - (rect.0 + rect.2)) / 2,
        AlignEdge::CenterY => ((reference.1 + reference.3) - (rect.1 + rect.3)) / 2,
    }
}
