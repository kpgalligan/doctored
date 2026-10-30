//! Per-entry queries over the layer forest: the CONTENT bounds align,
//! distribute, `get_document` and the layer-boundary overlay all speak; the
//! Move tool's auto-select hit test; and the link-group helpers the set ops
//! expand a selection with. `doc_group` owns the layout, the invariant and the
//! compositor these read; this module only asks questions of them and is where
//! that file's ~600-line budget sends its second half.
//!
//! The one per-pixel answer here — the content box — is MEMOIZED on the `Arc`
//! allocations it is a function of (the foot of this file says why that is
//! exact), because every caller re-asks after each settled edit and an edit
//! that touched one layer leaves every other layer's box unchanged. The same
//! memo answers Edit ▸ Stroke…'s mask-less alpha bounds
//! ([`alpha_edge_bounds`]), so the host's scratch queries and the stroke share
//! one scan. Its scan, [`edge_bounds`], is the crate's ONE
//! content-bounds scan: it reads ROWS through a caller's row-extent closure
//! (over a layer's tile runs, [`runs_extent`] the one run walker) and polls an
//! optional job once per row; the per-layer export trim, the typed masked
//! content box and the stroke's band call it too.

use std::sync::{Arc, Mutex, OnceLock, Weak};

use image::GrayImage;

use crate::adjust::Adjustment;
use crate::doc::{DrDocument, Layer, LayerKind};
use crate::doc_group::{parent, subtree, top_level, units, Unit};
use crate::job_control::JobContext;
use crate::layer_raster::LayerRasterKey;

/// The coverage a pixel needs before the hit test counts it as HIT: the
/// half-code contour the rest of the core already cuts a selection at, so
/// "what auto-select picks" and "what a selection would enclose" agree.
const HIT_COVERAGE: u8 = 128;

impl DrDocument {
    /// The canvas box `(x, y, width, height)` of entry `idx`'s visible
    /// CONTENT: for a raster entry the box of its non-transparent pixels with
    /// its enabled mask applied, for a group the union over its raster
    /// descendants. `None` when nothing is opaque — align and distribute skip
    /// such an entry rather than treating its pixel rect as content.
    ///
    /// This is the rect `align_layers`, `distribute_layers`,
    /// `get_document`'s `content_*` keys and the layer-boundary overlay all
    /// speak, which is why every one of them gets the SAME answer: a raster
    /// layer's `offset`/`width`/`height` are its PIXEL BUFFER rect, and for a
    /// photo-shaped layer on a canvas-sized buffer — what every paste, text
    /// layer and shape layer produces — that is a different rectangle.
    ///
    /// Deliberately NOT clipped to the canvas, so content hanging off the edge
    /// still aligns by where it really is; and deliberately blind to the
    /// `visible` flag, exactly like [`DrDocument::layer_canvas_image`], since
    /// this says what an entry HOLDS and not what it currently shows.
    pub fn layer_bounds(&self, idx: usize) -> Option<(i32, i32, u32, u32)> {
        let entry = self.layers.get(idx)?;
        let mut box_ = None;
        match entry.kind {
            LayerKind::Raster => union(&mut box_, content_box(entry)),
            LayerKind::Group => {
                for i in subtree(&self.layers, idx) {
                    let descendant = &self.layers[i];
                    if descendant.kind == LayerKind::Raster {
                        union(&mut box_, content_box(descendant));
                    }
                }
            }
        }
        let (x0, y0, x1, y1) = box_?;
        Some((
            clamp_i32(x0),
            clamp_i32(y0),
            (x1 - x0).clamp(0, i64::from(u32::MAX)) as u32,
            (y1 - y0).clamp(0, i64::from(u32::MAX)) as u32,
        ))
    }

    /// The canvas box `(x, y, width, height)` of entry `idx`'s PIXEL BUFFER:
    /// a raster entry's `offset` plus its dimensions, and for a group the
    /// UNION of its raster descendants' same rects. `None` for an
    /// out-of-range index and for a group with no raster descendant.
    ///
    /// The cheap positional twin of [`DrDocument::layer_bounds`]: O(number of
    /// descendants), never a per-pixel read. It is what the four geometry
    /// getters answer with, so a layers-panel reload and a `get_document` row
    /// cost the same on a group as on a raster entry, and it keeps ONE
    /// meaning across both kinds — "the rectangle this entry's pixels live
    /// in". The CONTENT box, which has to look at the pixels, stays behind
    /// `layer_bounds`, which every caller that needs it asks for by name.
    pub fn layer_pixel_rect(&self, idx: usize) -> Option<(i32, i32, u32, u32)> {
        let entry = self.layers.get(idx)?;
        let mut box_ = None;
        match entry.kind {
            LayerKind::Raster => union(&mut box_, Some(pixel_rect(entry))),
            LayerKind::Group => {
                for i in subtree(&self.layers, idx) {
                    let descendant = &self.layers[i];
                    if descendant.kind == LayerKind::Raster {
                        union(&mut box_, Some(pixel_rect(descendant)));
                    }
                }
            }
        }
        let (x0, y0, x1, y1) = box_?;
        Some((
            clamp_i32(x0),
            clamp_i32(y0),
            (x1 - x0).clamp(0, i64::from(u32::MAX)) as u32,
            (y1 - y0).clamp(0, i64::from(u32::MAX)) as u32,
        ))
    }

    /// The entry a click at canvas position `(x, y)` activates: the TOPMOST
    /// one whose own coverage there — alpha scaled by an enabled mask — is at
    /// least [`HIT_COVERAGE`]. Invisible entries and their whole subtrees are
    /// skipped, as are adjustment layers (they have no footprint of their own
    /// and clicking one would never be what the user meant). A group HITS when
    /// any descendant hits.
    ///
    /// With `top_level` the answer is the hit entry's top-level ancestor —
    /// "Auto-Select: Group" — otherwise the leaf itself. `None`
    /// when nothing is hit; the caller leaves the selection alone rather than
    /// emptying it.
    pub fn layer_at(&self, x: i32, y: i32, top_level_ancestor: bool) -> Option<usize> {
        let level = top_level(&self.layers);
        let hit = hit_in_level(&self.layers, &level, i64::from(x), i64::from(y))?;
        if !top_level_ancestor {
            return Some(hit);
        }
        let mut current = hit;
        while let Some(above) = parent(&self.layers, current) {
            current = above;
        }
        Some(current)
    }
}

/// The smallest unused non-zero link id in `layers`. Deterministic — unlike an
/// atomic counter, which is right for `Channel::id` precisely because that one
/// is never persisted — so a `.rz` round trip is byte-identical.
pub fn next_link_id(layers: &[Layer]) -> u32 {
    let mut used: Vec<u32> = layers.iter().map(|l| l.link).filter(|&v| v != 0).collect();
    used.sort_unstable();
    used.dedup();
    let mut candidate = 1u32;
    for value in used {
        if value == candidate {
            candidate = candidate.saturating_add(1);
        } else if value > candidate {
            break;
        }
    }
    candidate
}

/// The given entries expanded the way every "move this" op expands them: by
/// SUBTREE (moving a group moves its children) and then by LINK GROUP (every
/// entry carrying a member's non-zero `link`, with its own subtree), to a
/// fixed point, deduplicated and ascending. Out-of-range indices are dropped.
///
/// A fixed point rather than one pass, because a linked entry's subtree can
/// contain an entry that is itself linked into a further group; the loop is
/// bounded by the entry count, which no growth step can exceed.
pub fn expanded_set(layers: &[Layer], idx: &[usize]) -> Vec<usize> {
    let mut included = vec![false; layers.len()];
    for &i in idx {
        if i >= layers.len() {
            continue;
        }
        for member in subtree(layers, i) {
            included[member] = true;
        }
    }
    for _ in 0..layers.len() {
        let links: Vec<u32> = layers
            .iter()
            .enumerate()
            .filter(|(i, l)| included[*i] && l.link != 0)
            .map(|(_, l)| l.link)
            .collect();
        let mut grew = false;
        for (i, layer) in layers.iter().enumerate() {
            if layer.link == 0 || included[i] || !links.contains(&layer.link) {
                continue;
            }
            for member in subtree(layers, i) {
                grew |= !included[member];
                included[member] = true;
            }
        }
        if !grew {
            break;
        }
    }
    included
        .iter()
        .enumerate()
        .filter_map(|(i, &keep)| keep.then_some(i))
        .collect()
}

/// The box `(x0, y0, x1, y1)` (exclusive, canvas coordinates) a raster
/// entry's pixel BUFFER occupies, whatever is in it.
fn pixel_rect(layer: &Layer) -> (i64, i64, i64, i64) {
    let (lw, lh) = layer.pixels.dimensions();
    let (ox, oy) = (i64::from(layer.offset.0), i64::from(layer.offset.1));
    (ox, oy, ox + i64::from(lw), oy + i64::from(lh))
}

/// The box `(x0, y0, x1, y1)` (exclusive, canvas coordinates) of a raster
/// entry's non-transparent pixels — its enabled mask applied when `masked`,
/// ignored otherwise (`active_mask` is then never asked); `Ok(None)` when none
/// are. [`memoized_box`] fronts this with the memo — call that, not this.
///
/// EDGE searches with early exits, not one full sweep, because this is on the
/// hot path: [`DrDocument::layer_bounds`] answers the layers panel, the
/// layer-boundary overlay and `get_document`'s `content_*` keys, i.e. once per
/// row on every panel reload and every agent read. The layers this app makes
/// are canvas-sized and mostly opaque — `adding_layer` allocates width x
/// height — and for a fully covered buffer the search reads its first and last
/// rows and nothing else. A sparse layer degrades gracefully towards a full
/// read and an entirely empty one still pays it once, which is the honest
/// price of the answer. Each row is read as the layer's own tile runs
/// (`row_segments`), never pixel by pixel.
fn scan_content_box(
    layer: &Layer,
    masked: bool,
    job: JobContext<'_>,
) -> Result<Option<(i64, i64, i64, i64)>, String> {
    let (lw, lh) = layer.pixels.dimensions();
    let active_mask = if masked { layer.active_mask() } else { None };
    let mask = active_mask.as_deref().map(|m| m.as_raw().as_slice());
    let bounds = edge_bounds(lw, lh, job, |y, x0, x1| {
        let row = y as usize * lw as usize;
        runs_extent(layer.pixels.row_segments(y, x0, x1), |x, p| {
            p[3] != 0 && mask.is_none_or(|m| m[row + x as usize] != 0)
        })
    })?;
    let Some((x0, y0, x1, y1)) = bounds else {
        return Ok(None);
    };
    let (ox, oy) = (i64::from(layer.offset.0), i64::from(layer.offset.1));
    Ok(Some((
        i64::from(x0) + ox,
        i64::from(y0) + oy,
        i64::from(x1) + ox,
        i64::from(y1) + oy,
    )))
}

/// The ONE content-bounds scan: the box `(x0, y0, x1, y1)` (exclusive,
/// buffer coordinates) of every covered pixel of a `w` x `h` buffer; `None`
/// when there is none. `extent(y, x0, x1)` answers the first and last covered
/// x (inclusive) in columns `x0..x1` of row `y`, or `None`; it is only called
/// with `y < h` and `x0 < x1 <= w`. Shared by the content box, Stroke…'s
/// alpha bounds and band, the typed masked content box and the per-layer
/// export trim (`doc_export_layers::alpha_bounds8`), so none of them can
/// disagree about where a layer's pixels are.
///
/// Row-wise edge searches (see [`scan_content_box`] for why): the first
/// covered row from the top gives `y0` and a first `x` span, the first from
/// the bottom widens it and gives the last row, and each row between them is
/// read only OUTSIDE the span found so far — left of it for a smaller first
/// x, right of it for a larger last x. `job` is polled exactly once per
/// buffer row (`h` checks in all, empty buffer or not), before that row's
/// reads, so a cancel lands within one row; `JobContext::NONE` counts
/// nothing, which keeps every uncontrolled caller's checkpoint count as it was.
pub(crate) fn edge_bounds(
    w: u32,
    h: u32,
    job: JobContext<'_>,
    mut extent: impl FnMut(u32, u32, u32) -> Option<(u32, u32)>,
) -> Result<Option<(u32, u32, u32, u32)>, String> {
    let mut top = None;
    for y in 0..h {
        job.check()?;
        if let Some(span) = (w > 0).then(|| extent(y, 0, w)).flatten() {
            top = Some((y, span));
            break;
        }
    }
    let Some((y0, (mut lx, mut rx))) = top else {
        return Ok(None);
    };
    let mut last = y0;
    for y in (y0 + 1..h).rev() {
        job.check()?;
        if let Some((l, r)) = extent(y, 0, w) {
            (lx, rx, last) = (lx.min(l), rx.max(r), y);
            break;
        }
    }
    for y in y0 + 1..last {
        job.check()?;
        if lx > 0 {
            if let Some((l, _)) = extent(y, 0, lx) {
                lx = l;
            }
        }
        if rx + 1 < w {
            if let Some((_, r)) = extent(y, rx + 1, w) {
                rx = r;
            }
        }
    }
    Ok(Some((lx, y0, rx + 1, last + 1)))
}

/// The first and last covered ABSOLUTE x over adjoining `(x, run)` runs,
/// left to right, as `row_segments` yields them; `None` when nothing is
/// covered. `covered(x, sample)` gets the sample's absolute column. The one
/// run walker every [`edge_bounds`] row extent goes through.
pub(crate) fn runs_extent<'a, S: 'a>(
    runs: impl IntoIterator<Item = (u32, &'a [S])>,
    covered: impl Fn(u32, &S) -> bool,
) -> Option<(u32, u32)> {
    let mut found: Option<(u32, u32)> = None;
    for (x, run) in runs {
        let at = |i: usize| x + i as u32;
        let hit = |(i, s): (usize, &S)| covered(at(i), s);
        let Some(first) = run.iter().enumerate().position(hit) else {
            continue;
        };
        let last = run.iter().enumerate().rposition(hit).unwrap_or(first);
        found = Some((found.map_or(at(first), |(f, _)| f), at(last)));
    }
    found
}

// ------------------------------------------------------------- the memo --
//
// [`scan_content_box`] reads every row it needs, and everything that asks
// where an entry's content is re-asks after every settled edit: the layers
// panel, the boundary overlay, `get_document`, align and distribute — and,
// since the snapping phase, the host's fold of the WHOLE stack at the start of
// every drag (`DragSnapping.foldedSnapBoxes`) — and Edit ▸ Stroke…'s host
// asks for its scratch figure twice before the stroke scans the same alpha.
// An edit that changed one layer
// leaves every other layer's answer identical, so without a memo that fold
// re-scans the untouched ones every time: measured at 58 ms on a 3000 x 2000
// document with 20 sparse canvas-sized layers, paid again after each brush
// stroke, on the main thread, inside a mouse-down.
//
// KEYED ON THE `Arc` ALLOCATIONS, exactly as `style_cache` is and for the
// same reason. The box is a function of the pixel buffer, the ACTIVE mask and
// the offset alone; every op in this crate rebuilds a changed buffer into a
// FRESH `Arc` (nothing mutates one in place); and a `Weak` keeps the
// allocation alive — only the payload is dropped when the last strong
// reference goes — so a stored address can never be reused by a different
// buffer while its entry lives. Pointer equality is therefore EXACT, not a
// guess, and an entry whose buffer has died can match nothing.
//
// Bounded and least-recently-used, the `adjust_lut::memoized` shape. An entry
// is two `Weak`s, an offset and a rect — it pins no pixels — so the capacity
// answers "how many layers might a document being dragged have", not a memory
// budget.
//
// TWO CALLERS, ONE KEY SPACE. The content box keys the mask it applies; the
// stroke's alpha bounds apply none and key `None`. A `None` mask key means
// "no mask applied" for both, so a mask-less layer's two answers are one
// entry, and a layer with an enabled mask holds one entry of each. Only a
// COMPLETED scan is stored: a cancelled one returns before the store, so
// nothing partial is cached.

/// Entries kept. Comfortably past any real document's layer count, because a
/// fold of the whole stack only pays off if every entry hits — and at some
/// 70 bytes an entry, holding this many costs about 36 KB and pins no pixels.
const CONTENT_BOX_MEMO_CAPACITY: usize = 512;

/// One memo slot: the pixel allocation, the APPLIED mask allocation (`None`
/// when the layer has none, it is disabled, or the caller applies no mask —
/// which is why neither flag needs a field of its own), the offset the box
/// was measured at, and the box.
type ContentBoxEntry = (
    LayerRasterKey,
    Option<Weak<GrayImage>>,
    (i32, i32),
    Option<(i64, i64, i64, i64)>,
    ((i32, i32), u32, u32, u8),
);

static CONTENT_BOX_MEMO: OnceLock<Mutex<Vec<ContentBoxEntry>>> = OnceLock::new();

/// Clears the bounded geometry memo and its weak Arc shells. It owns no pixel
/// buffers; its small entry bookkeeping is disclosed separately in diagnostics.
pub(crate) fn evict_content_box_cache() {
    if let Some(memo) = CONTENT_BOX_MEMO.get() {
        memo.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
}

/// [`scan_content_box`] behind the memo above. Transparent: the same inputs
/// always give the same box, and a poisoned lock simply scans again. `masked`
/// says whether the enabled mask is applied (and keyed); `job` polls the scan
/// on a miss, and an `Err` (a cancel) stores nothing.
fn memoized_box(
    layer: &Layer,
    masked: bool,
    job: JobContext<'_>,
) -> Result<Option<(i64, i64, i64, i64)>, String> {
    // The mask the scan will actually read, as an `Arc` to key on: none when
    // the caller applies no mask, and otherwise taken as `active_mask` takes
    // it, so a disabled or wrongly sized mask keys as "none" the same way it
    // reads as none.
    let mask = (masked && layer.mask_enabled)
        .then(|| layer.mask.clone())
        .flatten();
    let properties = (
        layer.mask_properties.offset,
        layer.mask_properties.density.to_bits(),
        layer.mask_properties.feather.to_bits(),
        layer.mask_properties.outside_coverage,
    );
    let key = layer.pixels.key();
    let matches = |entry: &ContentBoxEntry| {
        entry.0.same(&key)
            && match (&entry.1, &mask) {
                (None, None) => true,
                (Some(weak), Some(arc)) => std::ptr::eq(weak.as_ptr(), Arc::as_ptr(arc)),
                _ => false,
            }
            && entry.2 == layer.offset
            && entry.4 == properties
    };
    let memo = CONTENT_BOX_MEMO.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(mut entries) = memo.lock() {
        if let Some(i) = entries.iter().position(&matches) {
            // Moved to the back, so eviction is genuinely least recently
            // USED: a document with more layers than the memo holds then
            // keeps the ones it is walking rather than the ones it happened
            // to measure first.
            let hit = entries.remove(i);
            let box_ = hit.3;
            entries.push(hit);
            return Ok(box_);
        }
    }
    // Scanned OUTSIDE the lock: an agent read on the server thread and a
    // redraw on the main one should not serialize on each other. A cancelled
    // scan returns here, before anything is stored.
    let box_ = scan_content_box(layer, masked, job)?;
    if let Ok(mut entries) = memo.lock() {
        // A store is the moment to drop the dead: nothing can match them by
        // pointer any more, and each still holds an (empty) allocation.
        entries.retain(|entry| !entry.0.is_dead());
        if !entries.iter().any(&matches) {
            while entries.len() >= CONTENT_BOX_MEMO_CAPACITY {
                entries.remove(0);
            }
            entries.push((
                key.clone(),
                mask.as_ref().map(Arc::downgrade),
                layer.offset,
                box_,
                properties,
            ));
        }
    }
    Ok(box_)
}

/// The content box: [`memoized_box`] with the enabled mask applied and no
/// job (so it cannot fail).
fn content_box(layer: &Layer) -> Option<(i64, i64, i64, i64)> {
    memoized_box(layer, true, JobContext::NONE).ok().flatten()
}

/// The box `(x0, y0, x1, y1)` (exclusive, BUFFER coordinates) of a raster
/// layer's alpha > 0 pixels, its mask ignored — Edit ▸ Stroke…'s bounds `B`.
/// The content-box memo with a mask-less key, minus `layer.offset`, so the
/// host's scratch queries and the stroke share one scan. `job` polls the
/// scan on a miss (once per row); a cancel is `Err` and caches nothing.
pub(crate) fn alpha_edge_bounds(
    layer: &Layer,
    job: JobContext<'_>,
) -> Result<Option<(u32, u32, u32, u32)>, String> {
    let Some((x0, y0, x1, y1)) = memoized_box(layer, false, job)? else {
        return Ok(None);
    };
    let (ox, oy) = (i64::from(layer.offset.0), i64::from(layer.offset.1));
    // The box was measured in buffer space and offset into canvas space with
    // these same two numbers, so each subtraction lands back inside the
    // buffer's u32 range.
    let back = |v: i64, o: i64| (v - o) as u32;
    Ok(Some((
        back(x0, ox),
        back(y0, oy),
        back(x1, ox),
        back(y1, oy),
    )))
}

pub(crate) fn union(box_: &mut Option<(i64, i64, i64, i64)>, rect: Option<(i64, i64, i64, i64)>) {
    let Some(rect) = rect else { return };
    *box_ = Some(match *box_ {
        None => rect,
        Some(o) => (
            o.0.min(rect.0),
            o.1.min(rect.1),
            o.2.max(rect.2),
            o.3.max(rect.3),
        ),
    });
}

pub(crate) fn clamp_i32(v: i64) -> i32 {
    v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// Top-down over one level's composite order, recursing into groups.
fn hit_in_level(layers: &[Layer], level: &[Unit], x: i64, y: i64) -> Option<usize> {
    for unit in level.iter().rev() {
        let entry = &layers[unit.idx];
        if !entry.visible {
            continue;
        }
        match entry.kind {
            LayerKind::Group => {
                let children = units(layers, unit.start..unit.idx, entry.depth.saturating_add(1));
                if let Some(hit) = hit_in_level(layers, &children, x, y) {
                    return Some(hit);
                }
            }
            LayerKind::Raster => {
                if entry
                    .meta
                    .as_deref()
                    .and_then(Adjustment::from_meta)
                    .is_some()
                {
                    continue;
                }
                if covers(entry, x, y) {
                    return Some(unit.idx);
                }
            }
        }
    }
    None
}

/// Whether a raster entry's own coverage at canvas position `(x, y)` reaches
/// [`HIT_COVERAGE`].
fn covers(layer: &Layer, x: i64, y: i64) -> bool {
    let (lw, lh) = layer.pixels.dimensions();
    let lx = x - i64::from(layer.offset.0);
    let ly = y - i64::from(layer.offset.1);
    if lx < 0 || ly < 0 || lx >= i64::from(lw) || ly >= i64::from(lh) {
        return false;
    }
    let Some(sample) = layer.pixels.read_rect(lx as u32, ly as u32, 1, 1) else {
        return false;
    };
    let alpha = f32::from(sample[0][3]);
    let coverage = if layer.mask_enabled {
        layer
            .effective_mask_rect((x as i32, y as i32), 1, 1)
            .map_or(255.0, |m| f32::from(m.as_raw()[0]))
    } else {
        255.0
    };
    (alpha * coverage / 255.0).round() as u32 >= u32::from(HIT_COVERAGE)
}
