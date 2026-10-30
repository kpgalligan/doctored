//! Edit ▸ Perspective Warp at 16 bits and at 32-bit float: the typed
//! document's thin adapter, the twin of `doc_perspective_warp` with the same
//! request, refusals, plan shape and write-back, through Puppet Warp's typed
//! helpers (`precision_puppet`'s `mesh_warp_*`: the RGBA16 accumulate/finish,
//! and the float arm's binary64 premultiplied extended samples finished
//! against the layer's own visible range, D3). Family
//! `geometry_perspective_warp` (`precision_capability`); on a CMYK document
//! (#547 phase 4b) the same helpers' CMYKA16 arm (`MeshPixels::Cmyk`).
use serde_json::Value;

use crate::doc_perspective_warp::{ready, PerspectiveWarpRequest, NAME};
use crate::doc_puppet::plan_json;
use crate::job_control::{DrJobControl, JobContext};
use crate::perspective_warp::{map, prepare, Prepared};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::admit;
use crate::puppet_mesh::Rect;

impl PrecisionDocument {
    fn perspective_prepare(
        &self,
        request: &PerspectiveWarpRequest,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<(Rect, Prepared, Value), String> {
        let (_, rect) = self.mesh_warp_record(request.layer, true, NAME)?;
        let prepared = prepare(&request.spec, rect, limit, other, job)?;
        let (common, ready) = ready(&prepared);
        let (retained, scratch) =
            self.mesh_warp_plan(request.layer, rect, common, ready.map(|(d, _)| d))?;
        let plan = plan_json(retained, scratch, ready, rect);
        Ok((rect, prepared, plan))
    }

    /// The typed twin of `DrDocument::perspective_warp_plan`.
    pub fn perspective_warp_plan(
        &self,
        request: &PerspectiveWarpRequest,
        limit: u64,
        other: u64,
    ) -> Result<Value, String> {
        Ok(self
            .perspective_prepare(request, limit, other, JobContext::NONE)?
            .2)
    }

    /// The typed twin of `DrDocument::perspective_warp`.
    pub fn perspective_warp(
        &self,
        request: &PerspectiveWarpRequest,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        let (rect, prepared, plan) = self.perspective_prepare(request, limit, other, job)?;
        let Prepared::Ready {
            pose,
            layout,
            plane,
            dest,
            ..
        } = prepared
        else {
            return Ok(None);
        };
        let retained = plan["retained_bytes"].as_u64().unwrap_or(0);
        let scratch = plan["scratch_bytes"].as_u64().unwrap_or(0);
        admit(retained, scratch, other, limit)?;
        job.reserve(scratch)?;
        let map = map(&layout, &plane, &pose)?;
        let next = self.mesh_warp_through(request.layer, rect, &map, dest, job)?;
        job.report(1.0, 5)?;
        Ok(Some(next))
    }
}
