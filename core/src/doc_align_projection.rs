//! Edit ▸ Auto-Align Layers' Cylindrical and Spherical reprojection (phase
//! 2b-2), and Collage and Reposition's lens-corrected plane (phase
//! 2b-5), at 8 bits: the request and the byte document's thin adapter
//! over `align_projection`. The 16-bit and float twin is
//! `precision_align_projection`. The host registers the layers, estimates
//! the focal and each layer's rotation (`AutoAlignReprojection.swift`) and
//! sends one request per layer, the reference included.
//!
//! The request is `{"op":"project_layer","layer":N,"projection":
//! "cylindrical"|"spherical","focal":f,"rotation":[9],"centre":[x,y],
//! "origin":[x,y],"distortion":k}` or, for Collage and Reposition with
//! Geometric Distortion (phase 2b-5), `{"op":"project_layer","layer":N,
//! "projection":"plane","map":[9],"distortion":k}`, in canvas pixels, at most
//! 64 KiB, strict (the convention is `align_projection`'s module doc). The
//! projection is read first and each has its own key set: a plane request
//! with `focal`, `rotation`, `centre` or `origin`, or a surface request with
//! `map`, is an unknown field. `distortion` (phase 2b-4) is optional, 0
//! when absent: Geometric Distortion's fitted coefficient, which the
//! projection undoes; a k that `align_distortion::folds` the layer refuses on
//! every projection. The target checks, the plan
//! accounting and the edit tail are the mesh warps' (`doc_puppet`'s
//! `mesh_warp_*`): the layer is resampled (Catmull-Rom) onto the projected
//! rect, not clipped to the canvas; a LINKED mask reprojects with it, filling
//! outside the layer with its outside coverage; an unlinked mask stays; the
//! persisted warp binding is dropped. The edit always changes the layer, so
//! it never answers `Ok(None)`.
//!
//! THE PLAN is `mesh_warp_plan(layer, rect, PLAN_BOOKKEEPING, Some(dest))`:
//! the source copy, the output and its shared 4-byte map margin (a linked
//! mask's too) and 2 MiB of bookkeeping, plus [`projection_margin`], which
//! raises each destination plane's margin to the projection's own measured
//! [`PROJECTION_MARGIN_BYTES_PER_PIXEL`], and `output_bytes`, the new layer
//! the result holds, which the host adds to every later layer's charge. The
//! map itself holds a dozen numbers. Puppet Warp's measured ratios do not
//! carry (their plan also holds the mesh, the solver, `MAP_BYTES` and the
//! host's projection), so this plan was measured on its own (the record is
//! [`PROJECTION_MARGIN_BYTES_PER_PIXEL`]'s, and
//! `designs/v1-stacks-status.md`'s "Memory plans are measured").
use serde_json::{Map, Value};

use crate::align_distortion::folds;
use crate::align_projection::{LayerProjection, ProjectionKind, FAR_SIDE};
use crate::doc::DrDocument;
use crate::doc_puppet::{plan_json, PLAN_BOOKKEEPING, REQUEST_LIMIT};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_image::admit;
use crate::puppet_mesh::Rect;
use crate::puppet_render::MARGIN_BYTES_PER_PIXEL;

/// The feature name the shared refusals carry.
pub(crate) const NAME: &str = "Auto-Align Layers";

/// The projection's working bytes per destination pixel, per plane, on top
/// of the stored output. It is its own constant, not the mesh warps'
/// `puppet_render::MARGIN_BYTES_PER_PIXEL` (4): the projection's inverse map
/// is a different algorithm, its scratch was measured on its own, and the
/// mesh warps' plans must not carry its cost.
///
/// Record (phase 2b-2; the Release app at `DR_WORKING_LIMIT_BYTES`
/// 8 GiB, a fresh instance per case, `MallocLargeCache=0`, 3 runs plus a
/// baseline twin; `scripts/stack-admission/measure.py`, cases
/// `project-3-jpeg8-24mp`, `project-3-png16-24mp`, `project-float-24mp`,
/// gate M-register, 2026-10-17): three 6000×4000 yawed views, each
/// reprojected onto a rect of about 22.27 MP, E the whole
/// `auto_align_layers` call (registration, three reprojections whose outputs
/// the job holds, the canvas set to their union, the publish). P is the
/// reply's `working_memory_plan_bytes`, the largest charge, each warp
/// charged its own scratch plus the outputs held before it. At the shared
/// 4 bytes the 16-bit case misses: it needs at least 30.2 bytes (at 30 its
/// plan is about 1,396.5 M against a need of 1,399.3 M), so the margin is
/// 31. At 31: P 1,055,666,226 / 1,418,722,715 / 2,501,639,394 against max E
/// 464,896,360 / 1,119,470,408 / 1,668,302,072 (2.271×, 1.267×, 1.500×;
/// baseline twins ≤ 81,920).
pub(crate) const PROJECTION_MARGIN_BYTES_PER_PIXEL: u64 = 31;

/// What the projection's plan adds to `mesh_warp_plan`'s scratch, which
/// charges each destination plane (the layer, and a `linked` mask) the
/// shared margin: the difference up to [`PROJECTION_MARGIN_BYTES_PER_PIXEL`]
/// for each plane. Shared by both depths.
pub(crate) fn projection_margin(dest: Rect, linked: bool) -> u64 {
    let extra = PROJECTION_MARGIN_BYTES_PER_PIXEL.saturating_sub(MARGIN_BYTES_PER_PIXEL);
    u64::from(dest.2)
        .saturating_mul(u64::from(dest.3))
        .saturating_mul(1 + u64::from(linked))
        .saturating_mul(extra)
}

/// A parsed `project_layer` request.
#[derive(Clone, Debug)]
pub struct ProjectLayerRequest {
    pub layer: usize,
    pub(crate) kind: ProjectionKind,
    /// 0 on the plane.
    pub(crate) focal: f64,
    /// The identity on the plane.
    pub(crate) rotation: [f64; 9],
    /// [0, 0] on the plane.
    pub(crate) centre: [f64; 2],
    /// [0, 0] on the plane.
    pub(crate) origin: [f64; 2],
    pub(crate) distortion: f64,
    /// The plane's map (`align_projection`'s module doc); the identity on
    /// the surfaces.
    pub(crate) map: [f64; 9],
}

/// The 3×3 identity, row-major.
const IDENTITY: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

fn numbers<const N: usize>(object: &Map<String, Value>, key: &str) -> Result<[f64; N], String> {
    let refusal = || format!("invalid project layer request: {key} must be {N} finite numbers");
    let list = object
        .get(key)
        .and_then(Value::as_array)
        .filter(|l| l.len() == N)
        .ok_or_else(refusal)?;
    let mut out = [0.0; N];
    for (slot, value) in out.iter_mut().zip(list) {
        *slot = value
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or_else(refusal)?;
    }
    Ok(out)
}

impl ProjectLayerRequest {
    /// Strict parse, bounded before serde sees the text.
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > REQUEST_LIMIT {
            return Err("project layer request exceeds 64 KiB".into());
        }
        let value: Value = serde_json::from_str(text)
            .map_err(|e| format!("invalid project layer request: {e}"))?;
        let object = value
            .as_object()
            .ok_or("invalid project layer request: it must be a JSON object")?;
        let kind = match object.get("projection").and_then(Value::as_str) {
            Some("cylindrical") => ProjectionKind::Cylindrical,
            Some("spherical") => ProjectionKind::Spherical,
            Some("plane") => ProjectionKind::Plane,
            _ => {
                return Err(
                    "invalid project layer request: projection must be cylindrical, \
                     spherical or plane"
                        .into(),
                )
            }
        };
        let plane = kind == ProjectionKind::Plane;
        if let Some(key) = object.keys().find(|k| {
            let shared = matches!(k.as_str(), "op" | "layer" | "projection" | "distortion");
            let own = if plane {
                k.as_str() == "map"
            } else {
                matches!(k.as_str(), "focal" | "rotation" | "centre" | "origin")
            };
            !shared && !own
        }) {
            return Err(format!(
                "invalid project layer request: unknown field `{key}`"
            ));
        }
        if object.get("op").and_then(Value::as_str) != Some("project_layer") {
            return Err("invalid project layer request: op must be project_layer".into());
        }
        let layer = object
            .get("layer")
            .and_then(Value::as_u64)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or("invalid project layer request: layer must be a layer index")?;
        let distortion = match object.get("distortion") {
            None => 0.0,
            Some(value) => value
                .as_f64()
                .filter(|v| v.is_finite())
                .ok_or("invalid project layer request: distortion must be a finite number")?,
        };
        if plane {
            return Ok(Self {
                layer,
                kind,
                focal: 0.0,
                rotation: IDENTITY,
                centre: [0.0, 0.0],
                origin: [0.0, 0.0],
                distortion,
                map: numbers(object, "map")?,
            });
        }
        let focal = object
            .get("focal")
            .and_then(Value::as_f64)
            .ok_or("invalid project layer request: focal must be a number")?;
        Ok(Self {
            layer,
            kind,
            focal,
            rotation: numbers(object, "rotation")?,
            centre: numbers(object, "centre")?,
            origin: numbers(object, "origin")?,
            distortion,
            map: IDENTITY,
        })
    }

    /// The projection of the layer standing at `rect` and its destination,
    /// or the kernel's refusal (the far-side one naming the layer). Shared by
    /// both depths.
    pub(crate) fn prepare(&self, rect: Rect) -> Result<(LayerProjection, Rect), String> {
        if self.distortion != 0.0 && folds(rect, self.distortion) {
            return Err("invalid project layer request: distortion folds the layer".into());
        }
        let projection = if self.kind == ProjectionKind::Plane {
            LayerProjection::plane(self.map, rect, self.distortion)?
        } else {
            LayerProjection::new(
                self.kind,
                self.focal,
                self.rotation,
                self.centre,
                self.origin,
                rect,
                self.distortion,
            )?
        };
        let dest = projection.dest_rect().map_err(|e| {
            if e == FAR_SIDE {
                format!("{NAME} cannot project layer {}: {FAR_SIDE}", self.layer)
            } else {
                e
            }
        })?;
        Ok((projection, dest))
    }
}

impl DrDocument {
    fn project_prepare(
        &self,
        request: &ProjectLayerRequest,
    ) -> Result<(Rect, LayerProjection, Rect, Value), String> {
        let entry = self.mesh_warp_layer(request.layer, true, NAME)?;
        let (w, h) = entry.pixels.dimensions();
        let rect = (i64::from(entry.offset.0), i64::from(entry.offset.1), w, h);
        let linked = entry.mask.is_some() && entry.mask_properties.linked;
        let (projection, dest) = request.prepare(rect)?;
        let (retained, scratch) =
            self.mesh_warp_plan(request.layer, rect, PLAN_BOOKKEEPING, Some(dest));
        let scratch = scratch.saturating_add(projection_margin(dest, linked));
        let (held, _) = self.mesh_warp_plan(request.layer, rect, PLAN_BOOKKEEPING, None);
        let mut plan = plan_json(retained, scratch, Some((dest, 0)), rect);
        plan["output_bytes"] = retained.saturating_sub(held).into();
        Ok((rect, projection, dest, plan))
    }

    /// The plan: `{"retained_bytes","scratch_bytes","rect","vertices":0,
    /// "output_bytes"}`, `rect` the projected layer's and `output_bytes` the
    /// part of `retained_bytes` that is the new layer (and linked mask):
    /// what a caller that keeps the result while it projects the next layer
    /// holds. Resolves the projection and its rect, so a refusal by name
    /// answers here as it would from the edit; nothing is admitted (`limit`
    /// and `other` are the edit's, taken for one shape).
    pub fn project_layer_plan(
        &self,
        request: &ProjectLayerRequest,
        _limit: u64,
        _other: u64,
    ) -> Result<Value, String> {
        Ok(self.project_prepare(request)?.3)
    }

    /// The edit: the layer reprojected. Never `Ok(None)`.
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
