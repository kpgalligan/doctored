//! Move, Transform, Puppet Warp and Perspective Warp of a CMYK layer's
//! SELECTED PIXELS, and their float bounds (#547 phase 4b): the typed RGBA16
//! arm of `precision_selected_pixels` over a CMYK document's straight
//! CMYKA16 ink, through the ONE walk (`selected_pixels_walk::move_samples`,
//! the master, every landing) at `[u16; 4]`.
//!
//! TWO VIEWS. The walk is four-component, so it runs twice over the same
//! request and coverage: once over `[C, M, Y, A]` and once over `[K, 0, 0, A]`
//! (each read through `CmykImage::read_rect`). Every step of the walk is per
//! component with alpha read from the fourth only (`moved_pixel`'s erase and
//! Normal composite, the premultiplied resample of a transform or a mesh),
//! so each view's colour planes are exactly what one five-plane walk would
//! make of them, and the two alphas are the same computation; the join
//! `[c, m, y, k, a]` refuses a disagreeing alpha as an internal error rather
//! than guess. A `Rect` result publishes through `CmykImage::with_rect`, a
//! grown `Plane` through `CmykImage::from_contiguous`; ⌥-drag duplicate is
//! `Hole::Keep` as on RGB. A mask target never reaches here: it keeps the
//! depth-agnostic `selected_plane` path; an ink-plate target is
//! `cmyk_selected_plate`'s, and an RGB colour-plane target refuses by name
//! (`precision_selected_pixels`).
//!
//! PLAN. Twice the u16 arm's scratch (the first view's moved samples are
//! held while the second walks), plus [`CMYK_SELECTED_SAMPLE_BYTES`] per
//! output pixel of the joined plane (`out` of `plan_scratch` /
//! `bounded_scratch`, the hull the u16 arm charges). The views are dropped
//! before the joined plane is tiled, so that adoption copy falls inside the
//! views' share. Declared before measuring; `scripts/cmyk-admission`'s
//! `transform-selected-16` case is its measured record.

use crate::cmyk_image::CmykImage;
use crate::doc_lock::EditKind;
use crate::job_control::JobContext;
use crate::layer_rect::coverage_rect16;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{admit, pixel_count};
use crate::selected_pixels::{
    move_samples, plan_scratch, CanvasRect, MovedSamples, SelectedPixelsEdit, SelectedPixelsPlan,
};
use crate::selected_pixels_float::{bounded_scratch, float_rect};

/// Bytes per output pixel of the joined CMYKA16 plane (module doc, PLAN).
pub(crate) const CMYK_SELECTED_SAMPLE_BYTES: u64 = 10;

/// The u16 arm's per-pixel term (`precision_selected_pixels::sample_bytes`).
const VIEW_SAMPLE_BYTES: u64 = 8;

/// The internal-error sentence of a join whose two alphas differ.
const VIEWS_DISAGREE: &str = "internal error: CMYK selected-pixels views disagree";

/// One view of the ink: `[C, M, Y, A]` or `[K, 0, 0, A]`.
fn view(sample: &[u16; 5], black: bool) -> [u16; 4] {
    if black {
        [sample[3], 0, 0, sample[4]]
    } else {
        [sample[0], sample[1], sample[2], sample[4]]
    }
}

/// The two views joined `[c, m, y, k, a]`; `Err` on a disagreeing alpha.
fn joined(cmy: &[[u16; 4]], k: &[[u16; 4]]) -> Result<Vec<[u16; 5]>, String> {
    if cmy.len() != k.len() {
        return Err(VIEWS_DISAGREE.into());
    }
    cmy.iter()
        .zip(k)
        .map(|(a, b)| {
            if a[3] != b[3] {
                return Err(VIEWS_DISAGREE.to_string());
            }
            Ok([a[0], a[1], a[2], b[0], a[3]])
        })
        .collect()
}

impl PrecisionDocument {
    /// The CMYK raster record and its canvas rect, or the RGB twin's named
    /// refusal (`selected_pixels_target`'s order).
    pub(crate) fn cmyk_selected_target(
        &self,
        edit: &SelectedPixelsEdit,
    ) -> Result<(&CmykImage, CanvasRect), String> {
        let layer = edit.layer();
        let record = self
            .layer(layer)
            .ok_or("high-depth layer index is out of range")?;
        let image = match &record.content {
            Content::CmykRaster { image, .. } => image,
            Content::Raster { .. } => return Err("a CMYK document holds only CMYK records".into()),
            Content::Adjustment(_) | Content::CmykAdjustment(_) => {
                return Err(edit.target_refusal(layer, "an adjustment layer"))
            }
            Content::Group(_) | Content::PassThrough(_) => {
                return Err(edit.target_refusal(layer, "a group"))
            }
        };
        let (w, h) = image.dimensions();
        let offset = record.offset();
        Ok((image, (i64::from(offset.0), i64::from(offset.1), w, h)))
    }

    /// `selected_pixels_plan`'s CMYK arm (module doc, PLAN).
    pub(crate) fn cmyk_selected_pixels_plan(
        &self,
        edit: &SelectedPixelsEdit,
    ) -> Result<SelectedPixelsPlan, String> {
        let (_, rect) = self.cmyk_selected_target(edit)?;
        let (w, h) = self.dimensions();
        let arm = |bytes: u64| -> Result<u64, String> {
            Ok(match edit.bounds() {
                Some(bounds) => bounded_scratch(edit, (w, h), rect, bounds, bytes)?,
                None => plan_scratch((w, h), rect, bytes)
                    .saturating_add(edit.landing_scratch((w, h), rect)?),
            })
        };
        let view = arm(VIEW_SAMPLE_BYTES)?;
        // `out × CMYK_SELECTED_SAMPLE_BYTES`: the arm at a wider sample less
        // the arm itself, so the hull is the u16 arm's own.
        let join = arm(VIEW_SAMPLE_BYTES + CMYK_SELECTED_SAMPLE_BYTES)?.saturating_sub(view);
        Ok(SelectedPixelsPlan {
            retained_bytes: self.retained_bytes(),
            scratch_bytes: view.saturating_mul(2).saturating_add(join),
            input_samples: u64::from(w) * u64::from(h),
        })
    }

    /// `selected_pixels_with_context`'s CMYK arm: the same refusals in the
    /// same order, the walk over two views (module doc).
    pub(crate) fn cmyk_selected_pixels_with_context(
        &self,
        edit: &SelectedPixelsEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let canvas = self.dimensions();
        if coverage.len() as u64 != u64::from(canvas.0) * u64::from(canvas.1) {
            return Err("selected pixels coverage must be canvas-sized u16".into());
        }
        let index = edit.layer();
        let (image, rect) = self.cmyk_selected_target(edit)?;
        if self.has_retained_source(index) {
            return Err(edit.retained_refusal());
        }
        self.lock_refusal(index, EditKind::Pixels)?;
        let Some(landing) = edit.landing(canvas)? else {
            return Ok(None);
        };
        let plan = self.cmyk_selected_pixels_plan(edit)?;
        admit(plan.retained_bytes, plan.scratch_bytes, other, limit)?;
        job.reserve(plan.scratch_bytes)?;
        job.check()?;
        let walk = |black: bool| {
            move_samples::<[u16; 4]>(
                |x, y, w, h| {
                    // Read without the job, as the RGB arm's `read_rect_rgba16`
                    // does: `move_samples` polls `job` right after this read, so
                    // a cancel reports `operation cancelled`, never this `None`'s
                    // allocation refusal.
                    let samples = image.read_rect((x, y, w, h), JobContext::NONE).ok()?;
                    Some(samples.iter().map(|p| view(p, black)).collect())
                },
                rect,
                canvas,
                coverage,
                &landing,
                edit.empty_area(),
                edit.hole(),
                edit.bounds(),
                job,
            )
        };
        let cmy = walk(false)?;
        let k = walk(true)?;
        let mut grown_comps = None;
        let published = match (cmy, k) {
            (MovedSamples::Rect(window, a), MovedSamples::Rect(other_window, b))
                if window == other_window =>
            {
                let samples = joined(&a, &b)?;
                drop((a, b));
                image
                    .with_rect(window, &samples, job)?
                    .map(|image| (image, (rect.0 as i32, rect.1 as i32)))
            }
            (MovedSamples::Plane(offset, size, a), MovedSamples::Plane(o, s, b))
                if (offset, size) == (o, s) =>
            {
                pixel_count(size.0, size.1)?;
                let samples = joined(&a, &b)?;
                drop((a, b));
                grown_comps = self.comps_after_growth(index, offset);
                Some((
                    CmykImage::from_contiguous(size.0, size.1, &samples, job)?,
                    offset,
                ))
            }
            _ => return Err(VIEWS_DISAGREE.into()),
        };
        let Some((image, offset)) = published else {
            return Ok(None);
        };
        let mut result = self.replacing_content(index, Content::CmykRaster { image, offset })?;
        if let (Some(next), Some(comps)) = (result.as_mut(), grown_comps) {
            next.comps = comps;
        }
        job.report(1.0, 5)?;
        Ok(result)
    }

    /// The typed float-bounds query's CMYK arm: F over the ink's alpha
    /// (`[4]`) through the one `float_rect` walk, allocating nothing.
    pub(crate) fn cmyk_selected_pixels_bounds(
        &self,
        layer: usize,
        coverage: &[u16],
        job: JobContext<'_>,
    ) -> Result<Option<CanvasRect>, String> {
        let canvas = self.dimensions();
        let (image, rect) =
            self.cmyk_selected_target(&crate::selected_pixels_float::probe(layer))?;
        let origin = (rect.0 as i32, rect.1 as i32);
        let Some(covered) = coverage_rect16(coverage, canvas, (rect.2, rect.3), origin) else {
            return Ok(None);
        };
        float_rect(
            rect,
            canvas,
            coverage,
            covered,
            |x, y| image.sample_at(x, y).is_some_and(|p| p[4] > 0),
            job,
        )
    }
}
