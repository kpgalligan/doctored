//! The healing family's covered-set components (a `#[path]` submodule of
//! `doc_heal`): the one 4-connected walk, its per-component window planes,
//! and the pre-walk bound on how many components the walk can find,
//! which the heal and the byte inpaint reserve by; the byte Content-Aware
//! Fill admission also re-admits by it, over the caller's coverage bytes
//! through [`component_count_bound_where`].
//!
//! # The component-count bound
//!
//! [`component_count_bound`] counts `S`, the covered pixels whose left
//! neighbour (only when `x > 0`) and upper neighbour (only when `y > 0`) are
//! both uncovered. The number of 4-connected components is at most `|S|`:
//! take any component `C` and its smallest raster index `p = y·w + x`. If
//! `x > 0` and `p − 1` is covered, `p − 1` is 4-adjacent to `p`, so it lies in
//! `C` at a smaller index, contradicting the choice of `p`; the same holds
//! for `p − w` when `y > 0`. So `p ∈ S`, and `C ↦ min(C)` is injective
//! because components are disjoint. That minimum is exactly the `start` at
//! which [`components_with_context`] discovers `C`.
//!
//! `|S| ≥ 1` exactly when anything is covered. Only a horizontal run's
//! leftmost pixel can qualify, so `|S|` is at most the number of runs, which
//! is at most the covered count: one convex Patch of `H` rows costs at most
//! `H` records rather than one per covered pixel. It is exact for rectangles
//! and diagonal chains, and over-counts where a component's left edge steps
//! left going down or where it has a U-shaped top.
//!
//! The left test is `x > 0 && covered[i − 1]`, never `i > 0`: at `x = 0`,
//! index `i − 1` is the previous row's last pixel, which is not a neighbour,
//! and testing it would drop a real component's first pixel — an undercount.
//!
//! a `#[path]` submodule of `doc_heal` (re-exported there, so callers name `doc_heal::{Component,
//! components_with_context, component_count_bound}`): `Component` with `box_padded` /
//! `window_buffers`, the one 4-connected `components_with_context` walk the healing family shares,
//! and `component_count_bound`, whose module doc proves it never undercounts (the left test is `x >
//! 0`, never `i > 0`). It has a second caller, the byte inpaint, through `doc_heal`'s
//! re-export. The bound's master is `component_count_bound_where` (a predicate on the
//! canvas index; `component_count_bound` delegates), whose third caller is the byte Content-Aware
//! Fill's re-admission over its coverage bytes.

use crate::job_control::JobContext;
use std::collections::VecDeque;

/// One 4-connected run of covered canvas pixels. This is the MASTER copy for
/// the whole healing family: `doc_inpaint` splits its hole into the same
/// components with the same walk rather than keeping a second one.
pub(crate) struct Component {
    /// Canvas pixel indices, in discovery order.
    pub(crate) pixels: Vec<u32>,
    /// The run's inclusive bounding box, `(x0, y0, x1, y1)`.
    pub(crate) bounds: (u32, u32, u32, u32),
}

/// A component's `region`, `write` and straight-RGB `source` window planes.
pub(crate) type WindowPlanes = (Vec<u8>, Vec<u8>, Vec<u8>);

impl Component {
    /// The component's bounding box padded by one pixel and clamped to the
    /// canvas — the ring of destination pixels the solve needs in order to
    /// see an uncovered usable neighbour.
    pub(crate) fn box_padded(&self, w: u32, h: u32) -> (u32, u32, u32, u32) {
        let (x0, y0, x1, y1) = self.bounds;
        (
            x0.saturating_sub(1),
            y0.saturating_sub(1),
            (x1 + 1).min(w - 1),
            (y1 + 1).min(h - 1),
        )
    }

    /// The three window planes the solve takes: the coverage `region` (this
    /// component's overlay alpha only, so a neighbouring blob that happens
    /// to fall inside the same window cannot leak into this solve), the
    /// hard-cut `write` weight, and the un-premultiplied straight-RGB
    /// `source`. Polls `control` every `EVERY_INTERVAL` component pixels and
    /// every `EVERY_INTERVAL` window pixels.
    pub(crate) fn window_buffers(
        &self,
        overlay: &[u8],
        canvas_w: usize,
        origin: (u32, u32),
        size: (usize, usize),
        control: JobContext<'_>,
    ) -> Result<WindowPlanes, String> {
        let (ww, wh) = size;
        let mut region = vec![0u8; ww * wh];
        let mut write = vec![0u8; ww * wh];
        for (k, &p) in self.pixels.iter().enumerate() {
            control.every(k)?;
            let (cx, cy) = (p as usize % canvas_w, p as usize / canvas_w);
            let i = (cy - origin.1 as usize) * ww + (cx - origin.0 as usize);
            region[i] = overlay[p as usize * 4 + 3];
            // The hard cut of `doc_heal`'s module doc: full weight inside the α = 128
            // contour, nothing outside it.
            write[i] = 255;
        }
        let mut source = vec![0u8; ww * wh * 3];
        for wyi in 0..wh {
            for wxi in 0..ww {
                let i = wyi * ww + wxi;
                control.every(i)?;
                let p = (origin.1 as usize + wyi) * canvas_w + (origin.0 as usize + wxi);
                let alpha = overlay[p * 4 + 3];
                if alpha == 0 {
                    continue;
                }
                // Straight source colour: divide the premultiplied bytes by
                // their own alpha, as `doc_paint::painting_layer_blend`
                // (the master copy of this un-premultiply) does; the min
                // guards a malformed colour byte above its alpha.
                for c in 0..3 {
                    let v = f32::from(overlay[p * 4 + c]) * 255.0 / f32::from(alpha);
                    source[i * 3 + c] = v.min(255.0).round() as u8;
                }
            }
        }
        Ok((region, write, source))
    }
}

/// Splits the covered canvas plane into 4-connected components — the ONE
/// component walk the healing brush, spot healing and content-aware fill all
/// use, so they cannot disagree about what a region is. The BFS mirrors
/// `doc_select::grow_region` (`doc_select.rs`), the master copy of this walk;
/// it differs only in its predicate (a coverage flag instead of colour
/// similarity) and in collecting the run rather than painting a mask.
pub(crate) fn components_with_context(
    covered: &[bool],
    w: usize,
    h: usize,
    control: JobContext<'_>,
) -> Result<Vec<Component>, String> {
    control.check()?;
    let mut seen = vec![false; w * h];
    let mut out = Vec::new();
    let mut queue: VecDeque<u32> = VecDeque::new();
    for start in 0..w * h {
        control.every(start)?;
        if seen[start] || !covered[start] {
            continue;
        }
        seen[start] = true;
        queue.push_back(start as u32);
        let mut pixels = Vec::new();
        let (mut x0, mut y0) = (u32::MAX, u32::MAX);
        let (mut x1, mut y1) = (0u32, 0u32);
        while let Some(p) = queue.pop_front() {
            control.every(pixels.len())?;
            pixels.push(p);
            let (x, y) = (p as usize % w, p as usize / w);
            x0 = x0.min(x as u32);
            y0 = y0.min(y as u32);
            x1 = x1.max(x as u32);
            y1 = y1.max(y as u32);
            let neighbors = [
                (x > 0, p.wrapping_sub(1)),
                (x + 1 < w, p + 1),
                (y > 0, p.wrapping_sub(w as u32)),
                (y + 1 < h, p + w as u32),
            ];
            for (inside, q) in neighbors {
                let q = q as usize;
                if !inside || seen[q] || !covered[q] {
                    continue;
                }
                seen[q] = true;
                queue.push_back(q as u32);
            }
        }
        out.push(Component {
            pixels,
            bounds: (x0, y0, x1, y1),
        });
    }
    Ok(out)
}

/// An upper bound on the number of components [`components_with_context`]
/// finds in `covered` (`w × h`), counted without walking them: the covered
/// pixels whose left neighbour (when `x > 0`) and upper neighbour (when
/// `y > 0`) are both uncovered — the module doc's `S`, with its proof. Polls
/// `control` every `EVERY_INTERVAL` pixels. Its two callers reserve their
/// pre-walk component records by it: `doc_heal`'s heal and
/// `doc_inpaint`'s byte inpaint, through `doc_heal`'s re-export.
/// It is [`component_count_bound_where`] over the plane; the content-aware
/// fill reaches the same count through that predicate form directly.
pub(crate) fn component_count_bound(
    covered: &[bool],
    w: usize,
    h: usize,
    control: JobContext<'_>,
) -> Result<u64, String> {
    component_count_bound_where(w, h, |i| covered[i], control)
}

/// [`component_count_bound`]'s count over a predicate on the canvas index —
/// the MASTER copy of the bound. `doc_content_aware`'s byte fill asks
/// it over its coverage bytes at `Caller::Fill`'s hole threshold without
/// building the `bool` plane the kernel builds from the same bytes, so its
/// count equals the kernel's pre-walk count.
pub(crate) fn component_count_bound_where(
    w: usize,
    h: usize,
    covered: impl Fn(usize) -> bool,
    control: JobContext<'_>,
) -> Result<u64, String> {
    let mut starts = 0u64;
    for i in 0..w * h {
        control.every(i)?;
        if !covered(i) {
            continue;
        }
        let (x, y) = (i % w, i / w);
        let left = x > 0 && covered(i - 1);
        let up = y > 0 && covered(i - w);
        if !left && !up {
            starts += 1;
        }
    }
    Ok(starts)
}
