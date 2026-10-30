//! The FLOAT's own rectangle, shared by the selected-pixels kernel and the
//! host's queries.
//!
//! * [`float_rect`] is THE walk that finds F — the canvas bbox of the layer
//!   pixels with alpha > 0 AND coverage > 0. `selected_pixels::move_samples`
//!   runs it over the samples it has already read; the float-bounds query
//!   ([`DrDocument::selected_pixels_bounds`] and its typed twin) runs it over
//!   the layer's own sample reads, allocating nothing, so ⌘T can open its box
//!   on the opaque bounds and a Move drag can refuse an empty float on its
//!   first tick without waiting for a preview.
//! * [`bounded_scratch`] is the plan of a request that carries `bounds`: the
//!   same terms as `plan_scratch` + `landing_scratch`, charged over the
//!   selection's rect instead of the whole layer, so a small selection on a
//!   large layer no longer reserves the layer's worth of scratch.
//!
//! Both ops take an optional `"bounds":[x,y,w,h]` (`null` is absent, and only a FAILED
//! `from_str` asks its shape, so a non-null wrong shape refuses `BAD_BOUNDS` by name while serde
//! stays the one parse and duplicate keys still refuse): coverage over the layer past it refuses
//! `OUTSIDE_BOUNDS` before any read, and the plan becomes `selected_pixels_float::bounded_scratch`
//! (the selection's rect instead of the layer's; the module doc is the never-exceeded argument);
//! without it every plan is byte-for-byte the old one. `selected_pixels_float.rs` also holds
//! `float_rect`, THE walk that finds F (the kernel's and the query's), and the float-bounds query
//! `dr_doc_selected_pixels_bounds` / `dr_precision_doc_selected_pixels_bounds` (target checks
//! as the move, in-place sample reads, no allocation, no lock check; false with no error = an empty
//! float).

use crate::doc::DrDocument;
use crate::job_control::{DrJobControl, JobContext};
use crate::layer_rect::coverage_rect16;
use crate::precision_document::PrecisionDocument;
use crate::selected_pixels::{
    area, intersect, union, CanvasRect, Landing, SelectedPixelsEdit, SelectedTarget,
    PLAN_BOOKKEEPING,
};

/// F, in canvas space: the bbox of the pixels of `covered` (a LAYER-space
/// rect, normally `coverage_rect16`'s answer) where `opaque(lx, ly)` holds
/// and the canvas coverage is non-zero. `None` when there is none. One job
/// check per row.
pub(crate) fn float_rect(
    layer: CanvasRect,
    canvas: (u32, u32),
    coverage: &[u16],
    covered: (u32, u32, u32, u32),
    opaque: impl Fn(u32, u32) -> bool,
    job: JobContext<'_>,
) -> Result<Option<CanvasRect>, String> {
    let (rx, ry, rw, rh) = covered;
    let (cw, ch) = (i64::from(canvas.0), i64::from(canvas.1));
    // (x0, y0, x1, y1), layer space, inclusive.
    let mut found: Option<(u32, u32, u32, u32)> = None;
    for ly in ry..ry + rh {
        job.check()?;
        let cy = layer.1 + i64::from(ly);
        if cy < 0 || cy >= ch {
            continue;
        }
        let row = cy as usize * canvas.0 as usize;
        let set = |lx: u32| -> bool {
            let cx = layer.0 + i64::from(lx);
            cx >= 0 && cx < cw && coverage[row + cx as usize] > 0 && opaque(lx, ly)
        };
        let Some(first) = (rx..rx + rw).find(|&lx| set(lx)) else {
            continue;
        };
        let last = (first..rx + rw).rev().find(|&lx| set(lx)).unwrap_or(first);
        found = Some(match found {
            None => (first, ly, last, ly),
            Some((x0, y0, x1, _)) => (x0.min(first), y0, x1.max(last), ly),
        });
    }
    Ok(found.map(|(x0, y0, x1, y1)| {
        (
            layer.0 + i64::from(x0),
            layer.1 + i64::from(y0),
            x1 - x0 + 1,
            y1 - y0 + 1,
        )
    }))
}

/// The scratch of a request carrying `bounds`, at `sample_bytes` per stored
/// pixel. With B = bounds ∩ layer ∩ canvas and D the landing of B (a move:
/// (B + d) ∩ canvas, none for d = 0; a transform: `landing_box(B)`, the whole
/// canvas when that errs, the move form for an integer translation):
/// `16·|B| + S·out + 16·|D| (a resampling transform only) + PLAN_BOOKKEEPING`,
/// where `out` is |bbox(B ∪ D)| when D ⊆ layer (or there is no D) and
/// |bbox(layer ∪ D)| otherwise. An empty B charges the bookkeeping alone.
///
/// Why the kernel never exceeds it: the kernel refuses a request whose
/// coverage over the layer reaches past `bounds` before reading anything
/// ([`crate::selected_pixels::OUTSIDE_BOUNDS`]), so the read rect and F lie
/// inside B. Landing is monotone (a translation trivially; a transform by
/// `landing_scratch`'s argument: grow, project, snap, floor/ceil and clip
/// each preserve ⊆), so the kernel's landing lies inside D and its resample
/// inside 16·|D|. When D ⊆ layer the landing cannot grow the extent, which
/// forces the kernel's Rect arm, whose window is bbox(F ∪ landing) ⊆
/// bbox(B ∪ D); otherwise the grown plane is bbox(layer ∪ landing) ⊆
/// bbox(layer ∪ D). Every term is at most its unbounded twin, because
/// B ⊆ layer ∩ canvas and D ⊆ canvas.
pub(crate) fn bounded_scratch(
    edit: &SelectedPixelsEdit,
    canvas: (u32, u32),
    layer: CanvasRect,
    bounds: CanvasRect,
    sample_bytes: u64,
) -> Result<u64, String> {
    let canvas_rect = (0, 0, canvas.0, canvas.1);
    // Resolved first, so an empty B still refuses bad geometry exactly as
    // the unbounded plan does.
    let resolved = edit.landing(canvas)?;
    let Some(b) = intersect(bounds, layer).and_then(|r| intersect(r, canvas_rect)) else {
        return Ok(PLAN_BOOKKEEPING);
    };
    let mut solver = 0;
    let (dest, resampled) = match resolved {
        None => (None, false),
        Some(Landing::Offset(d)) => (
            intersect((b.0 + d.0, b.1 + d.1, b.2, b.3), canvas_rect),
            false,
        ),
        Some(Landing::Transform(transform)) => match transform.landing_box(b, canvas) {
            Ok(dest) => (dest, true),
            Err(_) => (Some(canvas_rect), true),
        },
        // The deformed box is not known before the solve.
        Some(Landing::Puppet(puppet)) => {
            solver = puppet.solve_scratch(Some(b))?;
            (Some(canvas_rect), true)
        }
        Some(Landing::Perspective(perspective)) => {
            solver = perspective.solve_scratch();
            (Some(canvas_rect), true)
        }
    };
    let out = match dest {
        None => area(b),
        Some(d) if intersect(d, layer) == Some(d) => area(union(b, d)),
        Some(d) => area(union(layer, d)),
    };
    let landing = if resampled { dest.map_or(0, area) } else { 0 };
    Ok(area(b)
        .saturating_mul(16)
        .saturating_add(out.saturating_mul(sample_bytes))
        .saturating_add(landing.saturating_mul(16))
        .saturating_add(solver)
        .saturating_add(PLAN_BOOKKEEPING))
}

/// The request the query validates its target as: a move of `layer`, so the
/// refusals are the move's own sentences.
pub(crate) fn probe(layer: usize) -> SelectedPixelsEdit {
    SelectedPixelsEdit::MoveSelected {
        layer,
        dx: 0,
        dy: 0,
        duplicate: false,
        bounds: None,
        target: SelectedTarget::Layer,
        background: None,
        swatch: None,
    }
}

fn coverage_length(canvas: (u32, u32), coverage: &[u16]) -> Result<(), String> {
    if coverage.len() as u64 != u64::from(canvas.0) * u64::from(canvas.1) {
        return Err("selected pixels coverage must be canvas-sized u16".into());
    }
    Ok(())
}

impl DrDocument {
    /// The float's canvas rect for `layer` × `coverage` (canvas-sized u16):
    /// `Ok(None)` when the float is empty. Validates the length and the
    /// target as the move does; reads samples in place and allocates nothing.
    /// No lock or retained-source check — the edit still asks those.
    pub fn selected_pixels_bounds(
        &self,
        layer: usize,
        coverage: &[u16],
        control: Option<&DrJobControl>,
    ) -> Result<Option<CanvasRect>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let canvas = (self.width, self.height);
        coverage_length(canvas, coverage)?;
        let entry = self.selected_pixels_target(&probe(layer))?;
        let (w, h) = entry.pixels.dimensions();
        let rect = (i64::from(entry.offset.0), i64::from(entry.offset.1), w, h);
        let Some(covered) = coverage_rect16(coverage, canvas, (w, h), entry.offset) else {
            return Ok(None);
        };
        let pixels = &entry.pixels;
        float_rect(
            rect,
            canvas,
            coverage,
            covered,
            |x, y| pixels.sample_at(x, y).is_some_and(|p| p[3] > 0),
            job,
        )
    }
}

impl PrecisionDocument {
    /// The typed twin of [`DrDocument::selected_pixels_bounds`], at 16 bits
    /// and at 32-bit float, as the edit admits both.
    pub fn selected_pixels_bounds(
        &self,
        layer: usize,
        coverage: &[u16],
        control: Option<&DrJobControl>,
    ) -> Result<Option<CanvasRect>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let canvas = self.dimensions();
        coverage_length(canvas, coverage)?;
        if self.is_cmyk() {
            return self.cmyk_selected_pixels_bounds(layer, coverage, job);
        }
        let edit = probe(layer);
        let (image, rect) = self.selected_pixels_target(&edit)?;
        let origin = (rect.0 as i32, rect.1 as i32);
        let Some(covered) = coverage_rect16(coverage, canvas, (rect.2, rect.3), origin) else {
            return Ok(None);
        };
        float_rect(
            rect,
            canvas,
            coverage,
            covered,
            |x, y| image.sample_at(x, y).is_some_and(|p| p[3] > 0.0),
            job,
        )
    }
}
