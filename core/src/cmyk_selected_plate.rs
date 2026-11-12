//! MOVE AND FREE TRANSFORM OF ONE INK PLATE'S SELECTED PIXELS on a CMYK
//! document (#1): the Move tool, its nudge, ⌘T and `move_selected_pixels` /
//! `transform_selected_pixels` with `"target":"cyan"|"magenta"|"yellow"|"black"`
//! on a CMYK raster. An adapter like `selected_color_plane`: it reads the
//! layer's straight CMYKA16 over the layer rect R, runs THE walk for one
//! stored component (`selected_color_plane::walk_component`) on plate k at u16
//! and writes plate k back.
//!
//! Decisions (measured against Photoshop 26.10.0,
//! `scripts/photoshop-probe/cmyk_channels_take.py` take1,
//! `doctored-fixtures/photoshop/cmyk-channels-547/take1`): one plate's
//! selected pixels MOVE and ROTATE in CMYK.
//!
//! * ONE PLATE MOVES. The other three plates and the alpha are untouched on
//!   every pixel (take1: exact), the moved plate equals the source shifted
//!   (exact), and every write is clipped to R.
//! * THE VACATED AREA is `Hole::Fill(1 − color_plane_hole(swatch))`: the
//!   RGB channel rule (`plane_swatch`: the background swatch's
//!   30/59/11 luma rounded to an 8-bit code) read as ink, so a plate's hole
//!   is the swatch's gray as an RGB channel's is, on the 8-bit grid at every
//!   depth; the request's `"swatch"` is the swatch's sRGB 8-bit codes,
//!   absent = black (full ink) as for R/G/B. This is HG, selected after take1
//!   from the three declared candidates: the vacated Cyan was 156 on every
//!   pixel (translate and both turns) for the swatch RGB (220, 30, 170), where
//!   HG read 153 (residual 3 codes, not exact), HP (Photoshop's own
//!   separation of the swatch, 17.7 %) 45 and H0 0. The residual is disclosed
//!   in `designs/cmyk-mode.md`. A duplicate leaves no hole.
//! * THE LIFT. Coverage is scaled by the stored alpha (0 outside R), so an
//!   all-transparent lift is the walk's empty float and refuses with
//!   `selected_color_plane::COLOR_EMPTY_AREA`'s sentence (take1's `lift`
//!   case).
//! * LOCKS. `EditKind::Pixels` (Lock Pixels and Lock All refuse); a group or
//!   an adjustment record refuses with the layer path's `target_refusal`.
//!
//! PLAN. `selected_color_plane`'s formula at S = 2 (the u16 plate) and W =
//! `CMYK_SELECTED_SAMPLE_BYTES` (the CMYKA16 read and the rebuilt tiles):
//! `plane_scratch(edit, canvas, R, 2) + |R|·(2 + W) + 2·|canvas|` (the
//! alpha-scaled coverage copy). `scripts/cmyk-admission`'s `move-plate-16`
//! and `transform-plate-16` cases are its measured record.

use crate::cmyk_selected_pixels::CMYK_SELECTED_SAMPLE_BYTES;
use crate::doc_lock::EditKind;
use crate::job_control::JobContext;
use crate::plane_swatch::color_plane_hole;
use crate::precision_document::{Content, PrecisionDocument};
use crate::selected_color_plane::{walk_component, ComponentPlane};
use crate::selected_pixels::{area, CanvasRect, Hole, SelectedPixelsEdit, SelectedPixelsPlan};
use crate::selected_plane::plane_scratch;

const NO_PLATE: &str = "could not build the transformed ink plate";

/// The plate k an ink target names.
fn plate(edit: &SelectedPixelsEdit) -> Result<usize, String> {
    edit.target().ink_plate().ok_or_else(|| NO_PLATE.into())
}

/// The plate's hole (module doc): none for a duplicate, else HG — one
/// minus the swatch's RGB channel hole (black when absent).
fn hole(edit: &SelectedPixelsEdit) -> Hole {
    if edit.duplicate() {
        return edit.hole();
    }
    Hole::Fill(1.0 - color_plane_hole(edit.swatch().unwrap_or([0, 0, 0])))
}

/// The plan's scratch (module doc, PLAN).
fn scratch(edit: &SelectedPixelsEdit, canvas: (u32, u32), rect: CanvasRect) -> Result<u64, String> {
    let copy = 2u64.saturating_mul(u64::from(canvas.0) * u64::from(canvas.1));
    Ok(plane_scratch(edit, canvas, rect, 2)?
        .saturating_add(area(rect).saturating_mul(2 + CMYK_SELECTED_SAMPLE_BYTES))
        .saturating_add(copy))
}

impl PrecisionDocument {
    /// The byte plan of an ink-plate target: sample-free; validates the CMYK
    /// layer target and a transform's geometry. No lock check.
    pub(crate) fn cmyk_selected_plate_plan(
        &self,
        edit: &SelectedPixelsEdit,
    ) -> Result<SelectedPixelsPlan, String> {
        plate(edit)?;
        let (_, rect) = self.cmyk_selected_target(edit)?;
        let canvas = self.dimensions();
        Ok(SelectedPixelsPlan {
            retained_bytes: self.retained_bytes(),
            scratch_bytes: scratch(edit, canvas, rect)?,
            input_samples: u64::from(canvas.0) * u64::from(canvas.1),
        })
    }

    /// Move or transform plate k of the CMYK layer's selected pixels.
    /// Refuses, in order: the coverage length, the target, a retained
    /// source, a lock; then the identity is `Ok(None)`; then the plan's
    /// admission and reservation, and an empty float (`COLOR_EMPTY_AREA`).
    /// `Ok(None)` when no stored sample changed.
    pub(crate) fn cmyk_selected_plate_with_context(
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
        let k = plate(edit)?;
        let (image, rect) = self.cmyk_selected_target(edit)?;
        if self.has_retained_source(index) {
            return Err(edit.retained_refusal());
        }
        self.lock_refusal(index, EditKind::Pixels)?;
        let Some(landing) = edit.landing(canvas)? else {
            return Ok(None);
        };
        let plan = self.cmyk_selected_plate_plan(edit)?;
        crate::precision_image::admit(plan.retained_bytes, plan.scratch_bytes, other, limit)?;
        job.reserve(plan.scratch_bytes)?;
        job.check()?;
        let (w, h) = (rect.2, rect.3);
        self.require_cmyk_policy("a CMYK ink plate")?;
        let read = || image.read_rect((0, 0, w, h), JobContext::NONE);
        let Some(samples) = walk_component::<[u16; 5], u16>(
            edit,
            canvas,
            rect,
            coverage,
            &landing,
            ComponentPlane {
                k,
                hole: hole(edit),
                scaled: true,
            },
            read,
            |p, k| p[k],
            |p, k, v| p[k] = v,
            |p| f32::from(p[4]) / 65535.0,
            job,
        )?
        else {
            job.report(1.0, 5)?;
            return Ok(None);
        };
        let Some(image) = image.with_rect((0, 0, w, h), &samples, job)? else {
            job.report(1.0, 5)?;
            return Ok(None);
        };
        let offset = (rect.0 as i32, rect.1 as i32);
        let result = self.replacing_content(index, Content::CmykRaster { image, offset })?;
        job.report(1.0, 5)?;
        Ok(result)
    }
}
