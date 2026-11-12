//! Edit ▸ Puppet Warp at 8 bits: the request and the byte document's
//! thin adapter over the kernels (`puppet_mesh`, `puppet_arap`,
//! `puppet_render`). The 16-bit and float twin is `precision_puppet`; the
//! selection's float form rides `SelectedPixelsEdit::PuppetSelected`
//! (`selected_pixels_puppet`).
//!
//! The request is `{"op":"puppet_warp","layer":N,"mode":…,"density":…,
//! "expansion":…,"pins":[…]}` in canvas pixels, at most 64 KiB, strict. The
//! edit warps the layer's current pixels through the solved pose with the
//! persisted warp's byte accumulate/finish (`doc_warp`) — on a linear-light
//! document `sampling_linear`'s binary64 pair, so the colour mixes in
//! premultiplied linear light (11E D7) — writes the result at
//! the deformed mesh's outward-rounded box (not clipped to the canvas) and
//! drops any persisted warp binding, as a warp Set rewrites the layer. A
//! LINKED mask warps through the same map, filling outside the mesh with its
//! outside coverage; an unlinked mask stays. An identity request (every pin
//! unmoved and unrotated) plans 0 scratch bytes and edits nothing.
use serde_json::{json, Value};

use crate::doc::{DrDocument, Layer, LayerKind, LayerRaster};
use crate::doc_lock::EditKind;
use crate::doc_warp::{accumulate_mask8, accumulate_rgba8, finish_mask8, finish_rgba8};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_image::admit;
use crate::puppet_arap::{solve, PuppetPose, PuppetSpec, SOLVER_BYTES};
use crate::puppet_mesh::{build, mesh_plan_bytes, outward_bounds, MeshOptions, PuppetMesh, Rect};
use crate::puppet_render::{render, InverseMap, PoseMap, MAP_BYTES, MARGIN_BYTES_PER_PIXEL};
use crate::sampling_linear::{accumulate_linear_f64, straight_linear_f64};

/// The request bound, the one every strict geometry request uses.
pub(crate) const REQUEST_LIMIT: usize = 64 * 1024;
/// Fixed bookkeeping on top of the plan's per-pixel terms.
pub(crate) const PLAN_BOOKKEEPING: u64 = 2 * 1024 * 1024;
pub(crate) const PIXEL_LIMIT_REFUSAL: &str =
    "Puppet Warp would make the layer larger than the pixel limit.";
/// The feature name the shared refusals carry.
const NAME: &str = "Puppet Warp";

/// A parsed `puppet_warp` request.
#[derive(Clone, Debug)]
pub struct PuppetWarpRequest {
    pub layer: usize,
    pub(crate) spec: PuppetSpec,
}

impl PuppetWarpRequest {
    /// Strict parse, bounded before serde sees the text.
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > REQUEST_LIMIT {
            return Err("puppet warp request exceeds 64 KiB".into());
        }
        let value: Value =
            serde_json::from_str(text).map_err(|e| format!("invalid puppet warp request: {e}"))?;
        let object = value
            .as_object()
            .ok_or("invalid puppet warp request: it must be a JSON object")?;
        if let Some(key) = object.keys().find(|k| {
            !matches!(
                k.as_str(),
                "op" | "layer" | "mode" | "density" | "expansion" | "pins"
            )
        }) {
            return Err(format!(
                "invalid puppet warp request: unknown field `{key}`"
            ));
        }
        if object.get("op").and_then(Value::as_str) != Some("puppet_warp") {
            return Err("invalid puppet warp request: op must be puppet_warp".into());
        }
        let layer = object
            .get("layer")
            .and_then(Value::as_u64)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or("invalid puppet warp request: layer must be a layer index")?;
        let spec = PuppetSpec::parse(
            object.get("mode"),
            object.get("density"),
            object.get("expansion"),
            object.get("pins"),
        )?;
        Ok(Self { layer, spec })
    }
}

/// The prepared warp both the plan and the edit start from.
pub(crate) enum Prepared {
    Identity,
    Ready {
        pose: PuppetPose,
        dest: Rect,
        vertices: usize,
    },
}

/// Mesh, solve and destination for `spec` over the plane standing at `rect`,
/// admitting the mesh and the solver first. Shared by both depths.
pub(crate) fn prepare(
    spec: &PuppetSpec,
    rect: Rect,
    opaque: impl Fn(u32, u32) -> bool,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<Prepared, String> {
    if spec.is_identity() {
        return Ok(Prepared::Identity);
    }
    admit(
        0,
        mesh_plan_bytes(rect, &spec.options)?.saturating_add(SOLVER_BYTES),
        other,
        limit,
    )?;
    let mesh = build(rect, opaque, &spec.options, job)?;
    let vertices = mesh.vertices().len();
    let pose = solve(&mesh, &spec.pins, spec.mode, job)?;
    // `PoseMap::dest_rect`, read before the map's buckets are built.
    let dest = outward_bounds(pose.positions());
    if u64::from(dest.2) * u64::from(dest.3) > crate::doc::MAX_PIXELS
        || i32::try_from(dest.0).is_err()
        || i32::try_from(dest.1).is_err()
    {
        return Err(PIXEL_LIMIT_REFUSAL.into());
    }
    Ok(Prepared::Ready {
        pose,
        dest,
        vertices,
    })
}

/// The scratch every depth charges besides its own source copy and output:
/// the mesh, the solver, the inverse map and bookkeeping.
pub(crate) fn common_scratch(rect: Rect, options: &MeshOptions) -> Result<u64, String> {
    Ok(mesh_plan_bytes(rect, options)?
        .saturating_add(SOLVER_BYTES)
        .saturating_add(MAP_BYTES)
        .saturating_add(PLAN_BOOKKEEPING))
}

/// The plan JSON both depths answer (Puppet Warp's and Perspective Warp's):
/// `ready` is the destination and the mesh's vertex count, `None` for an
/// identity request.
pub(crate) fn plan_json(
    retained: u64,
    scratch: u64,
    ready: Option<(Rect, usize)>,
    layer: Rect,
) -> Value {
    match ready {
        None => json!({
            "retained_bytes": retained, "scratch_bytes": 0,
            "rect": [layer.0, layer.1, layer.2, layer.3], "vertices": 0,
        }),
        Some((dest, vertices)) => json!({
            "retained_bytes": retained, "scratch_bytes": scratch,
            "rect": [dest.0, dest.1, dest.2, dest.3], "vertices": vertices,
        }),
    }
}

impl Prepared {
    /// [`plan_json`]'s `ready`.
    pub(crate) fn ready(&self) -> Option<(Rect, usize)> {
        match self {
            Prepared::Identity => None,
            Prepared::Ready { dest, vertices, .. } => Some((*dest, *vertices)),
        }
    }
}

/// The destination area at `bytes` stored plus the map margin per pixel.
pub(crate) fn dest_bytes(dest: Rect, bytes: u64) -> u64 {
    u64::from(dest.2)
        .saturating_mul(u64::from(dest.3))
        .saturating_mul(bytes + MARGIN_BYTES_PER_PIXEL)
}

impl DrDocument {
    /// The raster layer a mesh warp (`name`: Puppet Warp, Perspective Warp)
    /// edits, or the named refusal: index, kind, artwork or retained source,
    /// and (for the layer form) the Pixels and Position locks.
    pub(crate) fn mesh_warp_layer(
        &self,
        index: usize,
        position: bool,
        name: &str,
    ) -> Result<&Layer, String> {
        let layer = self
            .layers
            .get(index)
            .ok_or_else(|| format!("{name} layer index is out of range"))?;
        if layer.kind == LayerKind::Group {
            return Err(format!(
                "{name} needs a pixel layer, and layer {index} is a group"
            ));
        }
        if crate::retouch_sampling::is_adjustment(layer) {
            return Err(format!(
                "{name} needs a pixel layer, and layer {index} is an adjustment layer"
            ));
        }
        if layer.meta.is_some() || self.has_retained_source(index) {
            return Err(format!("Rasterize layer {index} before {name}"));
        }
        let kinds: &[(EditKind, &str)] = if position {
            &[
                (EditKind::Pixels, "pixels"),
                (EditKind::Position, "position"),
            ]
        } else {
            &[(EditKind::Pixels, "pixels")]
        };
        for &(kind, which) in kinds {
            let blocking = self.lock_block(index, kind);
            if blocking != 0 {
                return Err(match self.lock_owner(index, blocking) {
                    Some(owner) if owner != index => {
                        format!("layer {index} is inside group {owner}, which is locked ({which})")
                    }
                    _ => format!("layer {index} is locked ({which})"),
                });
            }
        }
        Ok(layer)
    }

    /// A mesh warp's `(retained, scratch)` for layer `index` standing at
    /// `rect`: the document, `common` (the warp's own working memory), the
    /// source copy, and with a `dest` the output and its map margin (and a
    /// linked mask's).
    pub(crate) fn mesh_warp_plan(
        &self,
        index: usize,
        rect: Rect,
        common: u64,
        dest: Option<Rect>,
    ) -> (u64, u64) {
        let mut inventory = crate::resources::Inventory::default();
        inventory.document(self);
        let mut retained = inventory.unique_bytes();
        let mut scratch = common.saturating_add(u64::from(rect.2) * u64::from(rect.3) * 4);
        if let Some(dest) = dest {
            let mut output = u64::from(dest.2) * u64::from(dest.3) * 4;
            scratch = scratch.saturating_add(dest_bytes(dest, 4));
            let linked = self
                .layers
                .get(index)
                .is_some_and(|l| l.mask.is_some() && l.mask_properties.linked);
            if linked {
                output = output.saturating_add(u64::from(dest.2) * u64::from(dest.3));
                scratch = scratch.saturating_add(dest_bytes(dest, 1));
            }
            retained = retained.saturating_add(output);
        }
        (retained, scratch)
    }

    /// The mesh warp's edit tail: layer `index` (standing at `rect`)
    /// resampled through `map` onto `dest`, a linked mask along with it, the
    /// persisted warp binding dropped.
    pub(crate) fn mesh_warp_through<M: InverseMap + ?Sized>(
        &self,
        index: usize,
        rect: Rect,
        map: &M,
        dest: Rect,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let entry = self
            .layers
            .get(index)
            .ok_or("mesh warp layer index is out of range")?;
        let image = entry.pixels.to_contiguous_with_job(job)?;
        let raw = image.as_raw();
        let mut out = vec![0u8; dest.2 as usize * dest.3 as usize * 4];
        let read = |i: usize| [raw[i * 4], raw[i * 4 + 1], raw[i * 4 + 2], raw[i * 4 + 3]];
        let write = |i: usize, p: [u8; 4]| out[i * 4..i * 4 + 4].copy_from_slice(&p);
        match self.linear_light.as_deref() {
            None => render(
                map,
                rect,
                dest,
                read,
                accumulate_rgba8,
                finish_rgba8,
                write,
                job,
            )?,
            Some(codes) => render(
                map,
                rect,
                dest,
                read,
                |acc, p, weight| accumulate_linear_f64(acc, p, weight, codes),
                |acc| straight_linear_f64(acc, codes),
                write,
                job,
            )?,
        }
        drop(image);
        let origin = (dest.0 as i32, dest.1 as i32);
        let pixels = LayerRaster::unseeded(
            image::RgbaImage::from_raw(dest.2, dest.3, out)
                .ok_or("puppet warp output dimensions are invalid")?,
        );
        let mask = match &entry.mask {
            Some(mask) if entry.mask_properties.linked => {
                let outside = entry.mask_properties.outside_coverage;
                let (mw, mh) = mask.dimensions();
                let mo = entry.mask_properties.offset;
                let source = (i64::from(mo.0), i64::from(mo.1), mw, mh);
                let samples = mask.as_raw();
                let mut warped = vec![outside; dest.2 as usize * dest.3 as usize];
                render(
                    map,
                    source,
                    dest,
                    |i| samples[i],
                    |acc: &mut [f64; 1], p, w| accumulate_mask8(acc, p, w, outside),
                    |acc| finish_mask8(acc, outside),
                    |i, p| warped[i] = p,
                    job,
                )?;
                Some(std::sync::Arc::new(
                    image::GrayImage::from_raw(dest.2, dest.3, warped)
                        .ok_or("puppet warp mask dimensions are invalid")?,
                ))
            }
            _ => None,
        };
        let mut next = self.clone();
        let layer = &mut next.layers[index];
        layer.pixels = pixels;
        layer.offset = origin;
        if let Some(mask) = mask {
            layer.mask = Some(mask);
            layer.mask_properties.offset = origin;
        }
        layer.warp = None;
        Ok(next)
    }

    /// The mesh of layer `layer`'s opaque pixels, or with a canvas-sized
    /// `coverage` of its opaque AND selected pixels (the float form), after
    /// admitting `mesh_plan_bytes`.
    pub(crate) fn puppet_mesh(
        &self,
        layer: usize,
        coverage: Option<&[u16]>,
        options: &MeshOptions,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<PuppetMesh, String> {
        let entry = self.mesh_warp_layer(layer, coverage.is_none(), NAME)?;
        let (w, h) = entry.pixels.dimensions();
        let rect = (i64::from(entry.offset.0), i64::from(entry.offset.1), w, h);
        let pixels = &entry.pixels;
        match coverage {
            None => {
                admit(0, mesh_plan_bytes(rect, options)?, other, limit)?;
                build(
                    rect,
                    |x, y| pixels.sample_at(x, y).is_some_and(|p| p[3] > 0),
                    options,
                    job,
                )
            }
            Some(coverage) => {
                let canvas = (self.width, self.height);
                crate::selected_pixels_puppet::float_mesh(
                    rect,
                    canvas,
                    coverage,
                    |x, y| pixels.sample_at(x, y).is_some_and(|p| p[3] > 0),
                    options,
                    limit,
                    other,
                    job,
                )
            }
        }
    }

    fn puppet_prepare(
        &self,
        request: &PuppetWarpRequest,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<(Rect, Prepared, Value), String> {
        let entry = self.mesh_warp_layer(request.layer, true, NAME)?;
        let (w, h) = entry.pixels.dimensions();
        let rect = (i64::from(entry.offset.0), i64::from(entry.offset.1), w, h);
        let pixels = &entry.pixels;
        let prepared = prepare(
            &request.spec,
            rect,
            |x, y| pixels.sample_at(x, y).is_some_and(|p| p[3] > 0),
            limit,
            other,
            job,
        )?;
        let common = common_scratch(rect, &request.spec.options)?;
        let ready = prepared.ready();
        let (retained, scratch) =
            self.mesh_warp_plan(request.layer, rect, common, ready.map(|(d, _)| d));
        let plan = plan_json(retained, scratch, ready, rect);
        Ok((rect, prepared, plan))
    }

    /// The plan: `{"retained_bytes","scratch_bytes","rect","vertices"}`.
    /// Builds the mesh and solves (admitting both first), so a refusal by
    /// name answers here as it would from the edit.
    pub fn puppet_warp_plan(
        &self,
        request: &PuppetWarpRequest,
        limit: u64,
        other: u64,
    ) -> Result<Value, String> {
        Ok(self
            .puppet_prepare(request, limit, other, JobContext::NONE)?
            .2)
    }

    /// The edit. `Ok(None)` for an identity request.
    pub fn puppet_warp(
        &self,
        request: &PuppetWarpRequest,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        let (rect, prepared, plan) = self.puppet_prepare(request, limit, other, job)?;
        let Prepared::Ready { pose, dest, .. } = prepared else {
            return Ok(None);
        };
        let retained = plan["retained_bytes"].as_u64().unwrap_or(0);
        let scratch = plan["scratch_bytes"].as_u64().unwrap_or(0);
        admit(retained, scratch, other, limit)?;
        job.reserve(scratch)?;
        let map = PoseMap::new(&pose)?;
        let next = self.mesh_warp_through(request.layer, rect, &map, dest, job)?;
        job.report(1.0, 5)?;
        Ok(Some(next))
    }
}
