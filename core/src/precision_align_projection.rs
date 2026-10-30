//! Edit ▸ Auto-Align Layers' Cylindrical and Spherical reprojection at 16
//! bits and at 32-bit float: the typed document's thin
//! adapter, the twin of `doc_align_projection` with the same request,
//! refusals, plan shape and write-back, through the mesh warps' typed helpers
//! (`precision_puppet`'s `mesh_warp_*`: the RGBA16 accumulate/finish, the
//! float arm's binary64 premultiplied extended samples finished against the
//! layer's own visible range, D3, and the CMYKA16 arm, which Auto-Align on a
//! CMYK document reaches).
use serde_json::Value;

use crate::align_projection::LayerProjection;
use crate::doc_align_projection::{projection_margin, ProjectLayerRequest, NAME};
use crate::doc_puppet::{plan_json, PLAN_BOOKKEEPING};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::admit;
use crate::puppet_mesh::Rect;

impl PrecisionDocument {
    fn project_prepare(
        &self,
        request: &ProjectLayerRequest,
    ) -> Result<(Rect, LayerProjection, Rect, Value), String> {
        let (_, rect) = self.mesh_warp_record(request.layer, true, NAME)?;
        let linked = self
            .layer(request.layer)
            .and_then(|r| r.mask.as_ref())
            .is_some_and(|m| m.properties.linked);
        let (projection, dest) = request.prepare(rect)?;
        let (retained, scratch) =
            self.mesh_warp_plan(request.layer, rect, PLAN_BOOKKEEPING, Some(dest))?;
        let scratch = scratch.saturating_add(projection_margin(dest, linked));
        let (held, _) = self.mesh_warp_plan(request.layer, rect, PLAN_BOOKKEEPING, None)?;
        let mut plan = plan_json(retained, scratch, Some((dest, 0)), rect);
        plan["output_bytes"] = retained.saturating_sub(held).into();
        Ok((rect, projection, dest, plan))
    }

    /// The typed twin of `DrDocument::project_layer_plan`.
    pub fn project_layer_plan(
        &self,
        request: &ProjectLayerRequest,
        _limit: u64,
        _other: u64,
    ) -> Result<Value, String> {
        Ok(self.project_prepare(request)?.3)
    }

    /// The typed twin of `DrDocument::project_layer`.
    pub fn project_layer(
        &self,
        request: &ProjectLayerRequest,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        let (rect, projection, dest, plan) = self.project_prepare(request)?;
        let retained = plan["retained_bytes"].as_u64().unwrap_or(0);
        let scratch = plan["scratch_bytes"].as_u64().unwrap_or(0);
        admit(retained, scratch, other, limit)?;
        job.reserve(scratch)?;
        let next = self.mesh_warp_through(request.layer, rect, &projection, dest, job)?;
        job.report(1.0, 5)?;
        Ok(Some(next))
    }
}
