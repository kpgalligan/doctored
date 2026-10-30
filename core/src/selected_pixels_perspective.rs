//! Perspective Warp of the selection's FLOAT: the geometry half of
//! `SelectedPixelsEdit::PerspectiveSelected`, Edit ▸ Perspective Warp with
//! a selection active. The float, the hole `a·(1−c)`, the source-over
//! landing and the single quantize are the selected-pixels walk
//! (`selected_pixels_walk`); this module says where the float lands and what
//! it looks like there, as `selected_pixels_puppet` does for Puppet Warp.
//!
//! The layout, plane mesh and extension are built over the LAYER rect (the
//! subject a layer warp would use, so the float moves exactly as the same
//! pixels of the layer would), and the premultiplied f32 float resamples
//! through that map (`selected_pixels_puppet::resample_through`) onto the
//! map's box ∩ canvas. The selection itself is unchanged.
use crate::job_control::JobContext;
use crate::perspective_plane::{Layout, PerspectiveSpec};
use crate::perspective_warp::{map, prepare, scratch, Prepared};
use crate::selected_pixels::{CanvasRect, SelectedPixelsEdit};
use crate::selected_pixels_puppet::{resample_through, Landed};

pub(crate) const EMPTY_AREA: &str = "The selection holds no pixels to perspective warp.";

/// A resolved, non-identity Perspective Warp of the float.
#[derive(Clone, Debug)]
pub(crate) struct PerspectiveLanding {
    pub(crate) spec: PerspectiveSpec,
    pub(crate) layout: Layout,
}

impl PerspectiveLanding {
    /// The plan's landing term: the whole canvas at 16 bytes a pixel (the
    /// warped box is not known before the solve) plus [`Self::solve_scratch`].
    pub(crate) fn scratch(&self, canvas: (u32, u32)) -> u64 {
        16u64
            .saturating_mul(u64::from(canvas.0) * u64::from(canvas.1))
            .saturating_add(self.solve_scratch())
    }

    /// The warp's own working memory (`perspective_warp::scratch`).
    pub(crate) fn solve_scratch(&self) -> u64 {
        scratch(&self.layout)
    }

    /// Prepare the warp over the layer standing at `layer` and resample the
    /// premultiplied `floated` samples (standing at `float`) onto the
    /// landing. `Ok(None)` when the landing misses the canvas.
    pub(crate) fn resample(
        &self,
        floated: &[[f32; 4]],
        float: CanvasRect,
        layer: CanvasRect,
        canvas: (u32, u32),
        job: JobContext<'_>,
    ) -> Result<Option<Landed>, String> {
        // The plan admitted this warp's scratch already.
        let Prepared::Ready {
            pose,
            layout,
            plane,
            ..
        } = prepare(&self.spec, layer, u64::MAX, 0, job)?
        else {
            return Err("the perspective warp landing is the identity".into());
        };
        let map = map(&layout, &plane, &pose)?;
        resample_through(&map, floated, float, canvas, job)
    }
}

impl SelectedPixelsEdit {
    /// A Perspective Warp's validated quads and straighten entries; `None`
    /// for the other ops.
    pub(crate) fn perspective_spec(&self) -> Result<Option<PerspectiveSpec>, String> {
        match self {
            Self::PerspectiveSelected {
                quads, straighten, ..
            } => PerspectiveSpec::parse(quads.as_ref(), straighten.as_ref(), None).map(Some),
            Self::MoveSelected { .. }
            | Self::TransformSelected { .. }
            | Self::PuppetSelected { .. } => Ok(None),
        }
    }

    /// The float's Perspective Warp landing, `None` for an identity layout
    /// or another op. Refuses bad geometry by name.
    pub(crate) fn perspective_landing(&self) -> Result<Option<PerspectiveLanding>, String> {
        let Some(spec) = self.perspective_spec()? else {
            return Ok(None);
        };
        let layout = spec.resolve()?;
        Ok((!layout.is_identity()).then_some(PerspectiveLanding { spec, layout }))
    }
}
