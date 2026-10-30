//! Edit ▸ Perspective Warp at 8 bits: the request and the byte
//! document's thin adapter over the kernels (`perspective_plane`,
//! `perspective_mesh`, `perspective_warp`). The 16-bit and float twin is
//! `precision_perspective_warp`; the selection's float form rides
//! `SelectedPixelsEdit::PerspectiveSelected` (`selected_pixels_perspective`).
//!
//! The request is `{"op":"perspective_warp","layer":N,"quads":[{"from":[8],
//! "to":[8]}],"straighten":[{"quad","edge","direction"}]}` in canvas pixels,
//! at most 64 KiB, strict. The target checks, the plan accounting and the
//! edit tail are Puppet Warp's (`doc_puppet`'s `mesh_warp_*`): the layer is
//! resampled through the map onto its destination (not clipped to the
//! canvas), a LINKED mask warps along with its outside coverage, an unlinked
//! mask stays, and the persisted warp binding is dropped. An identity
//! layout plans 0 scratch bytes and edits nothing.
use serde_json::Value;

use crate::doc::DrDocument;
use crate::doc_puppet::{plan_json, REQUEST_LIMIT};
use crate::job_control::{DrJobControl, JobContext};
use crate::perspective_plane::PerspectiveSpec;
use crate::perspective_warp::{map, prepare, scratch, Prepared};
use crate::precision_image::admit;
use crate::puppet_mesh::Rect;

/// The feature name the shared refusals carry.
pub(crate) const NAME: &str = "Perspective Warp";

/// A parsed `perspective_warp` request.
#[derive(Clone, Debug)]
pub struct PerspectiveWarpRequest {
    pub layer: usize,
    pub(crate) spec: PerspectiveSpec,
}

impl PerspectiveWarpRequest {
    /// Strict parse, bounded before serde sees the text.
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > REQUEST_LIMIT {
            return Err("perspective warp request exceeds 64 KiB".into());
        }
        let value: Value = serde_json::from_str(text)
            .map_err(|e| format!("invalid perspective warp request: {e}"))?;
        let object = value
            .as_object()
            .ok_or("invalid perspective warp request: it must be a JSON object")?;
        if let Some(key) = object
            .keys()
            .find(|k| !matches!(k.as_str(), "op" | "layer" | "quads" | "straighten"))
        {
            return Err(format!(
                "invalid perspective warp request: unknown field `{key}`"
            ));
        }
        if object.get("op").and_then(Value::as_str) != Some("perspective_warp") {
            return Err("invalid perspective warp request: op must be perspective_warp".into());
        }
        let layer = object
            .get("layer")
            .and_then(Value::as_u64)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or("invalid perspective warp request: layer must be a layer index")?;
        let spec = PerspectiveSpec::parse(object.get("quads"), object.get("straighten"), None)?;
        Ok(Self { layer, spec })
    }
}

/// The plan's warp scratch and `plan_json`'s `ready` of a prepared warp.
pub(crate) fn ready(prepared: &Prepared) -> (u64, Option<(Rect, usize)>) {
    match prepared {
        Prepared::Identity => (0, None),
        Prepared::Ready {
            layout,
            dest,
            vertices,
            ..
        } => (scratch(layout), Some((*dest, *vertices))),
    }
}

impl DrDocument {
    fn perspective_prepare(
        &self,
        request: &PerspectiveWarpRequest,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<(Rect, Prepared, Value), String> {
        let entry = self.mesh_warp_layer(request.layer, true, NAME)?;
        let (w, h) = entry.pixels.dimensions();
        let rect = (i64::from(entry.offset.0), i64::from(entry.offset.1), w, h);
        let prepared = prepare(&request.spec, rect, limit, other, job)?;
        let (common, ready) = ready(&prepared);
        let (retained, scratch) =
            self.mesh_warp_plan(request.layer, rect, common, ready.map(|(d, _)| d));
        let plan = plan_json(retained, scratch, ready, rect);
        Ok((rect, prepared, plan))
    }

    /// The plan: `{"retained_bytes","scratch_bytes","rect","vertices"}`.
    /// Resolves, meshes and solves (admitting the grid and the solver
    /// first), so a refusal by name answers here as it would from the edit.
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

    /// The edit. `Ok(None)` for an identity layout.
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
