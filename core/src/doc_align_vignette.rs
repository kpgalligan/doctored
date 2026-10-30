//! Edit ▸ Auto-Align Layers' Vignette Removal (phase 2b-3) at 8 bits:
//! the request and the byte document's thin adapter over `align_vignette`.
//! The 16-bit and float twin is `precision_align_vignette`. The host
//! registers the layers (`AutoAlignReprojection.swift`), sends every listed
//! layer's map in one request, removes the vignette here, then reprojects the
//! result (`project_layer`).
//!
//! The request is `{"op":"remove_vignette","layers":[{"layer":N,"map":[9]},
//! …]}`, at most 64 KiB, strict: at least 2 distinct layers, each `map`
//! row-major, finite, with a positive determinant, taking the layer's canvas
//! point to the reference's. The FIRST entry's exposure is the one the fit
//! pins (the host sends the reference first, with the identity).
//!
//! THE EDIT reads each listed layer's pixels in linear light for the fit —
//! sRGB-decoded (`adjust_math::srgb_to_linear`), a pixel below full alpha
//! read as absent — then writes every listed layer: each pixel with any alpha
//! gets `sampling_lens::apply_vignette_stops` at `align_vignette::stops`, in
//! sRGB linear light (`Exposure::gain_only`), clamped, rounded to its code.
//! Alpha, masks and offsets are unchanged; a transparent pixel is untouched.
//! The write compares every sample it writes, and an edit that changes none
//! (a vignette-free stack fits a coefficient near 0) answers `Ok(None)`,
//! never an identical copy. A layer none of whose samples changed keeps its
//! raster.
//!
//! THE PLAN: `retained_bytes` is the document plus `output_bytes`, every
//! listed layer at 4 bytes a pixel (each new raster the result holds);
//! `scratch_bytes` is [`plan_scratch`]: `PLAN_BOOKKEEPING`, the fit's
//! `FIT_SCRATCH_BYTES` and `system_bytes`, and the write's own allocation
//! for the largest listed layer — at 8 bits its contiguous copy (4 bytes a
//! pixel), the new tiles being the output. The 16-bit and float figures are
//! `precision_align_vignette`'s. The plan runs the fit, so its answer
//! carries `coefficient`, `gain` and `samples`.
//!
//! MEASURED (2026-10-17, `scripts/stack-admission/measure.py`, the
//! `vignette-*` cases, Release, gate M-register): the job's largest charge
//! against the whole call's peak at 8, 16 and float was 2.977 ×, 2.076 × and
//! 1.938 ×, so Vignette Removal needs no margin of its own.
use serde_json::{json, Map, Value};

use crate::adjust_math::srgb_to_linear;
use crate::align_vignette::FIT_SCRATCH_BYTES;
use crate::align_vignette::{fit, system_bytes, Radial, VignetteFit, VignetteFrame};
use crate::doc::{DrDocument, Layer, LayerKind, LayerRaster};
use crate::doc_lock::EditKind;
use crate::doc_puppet::{PLAN_BOOKKEEPING, REQUEST_LIMIT};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_image::admit;
use crate::sampling_lens::apply_vignette_stops;

/// The feature name the refusals carry.
pub(crate) const NAME: &str = "Vignette Removal";

/// A parsed `remove_vignette` request: each layer and its map, in the
/// request's order.
#[derive(Clone, Debug)]
pub struct RemoveVignetteRequest {
    pub layers: Vec<(usize, [f64; 9])>,
}

fn refuse(what: &str) -> String {
    format!("invalid remove vignette request: {what}")
}

fn entry(value: &Value) -> Result<(usize, [f64; 9]), String> {
    let object: &Map<String, Value> = value
        .as_object()
        .ok_or_else(|| refuse("each layers entry must be an object with layer and map"))?;
    if let Some(key) = object
        .keys()
        .find(|k| !matches!(k.as_str(), "layer" | "map"))
    {
        return Err(refuse(&format!("unknown field `{key}`")));
    }
    let layer = object
        .get("layer")
        .and_then(Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| refuse("layer must be a layer index"))?;
    let map_refusal = || refuse("map must be 9 finite numbers with a positive determinant");
    let list = object
        .get("map")
        .and_then(Value::as_array)
        .filter(|l| l.len() == 9)
        .ok_or_else(map_refusal)?;
    let mut map = [0.0; 9];
    for (slot, value) in map.iter_mut().zip(list) {
        *slot = value
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or_else(map_refusal)?;
    }
    let m = &map;
    let det = m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
        + m[2] * (m[3] * m[7] - m[4] * m[6]);
    if !(det > 0.0 && det.is_finite()) {
        return Err(map_refusal());
    }
    Ok((layer, map))
}

impl RemoveVignetteRequest {
    /// Strict parse, bounded before serde sees the text.
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > REQUEST_LIMIT {
            return Err(refuse("it exceeds 64 KiB"));
        }
        let value: Value = serde_json::from_str(text).map_err(|e| refuse(&e.to_string()))?;
        let object = value
            .as_object()
            .ok_or_else(|| refuse("it must be a JSON object"))?;
        if let Some(key) = object
            .keys()
            .find(|k| !matches!(k.as_str(), "op" | "layers"))
        {
            return Err(refuse(&format!("unknown field `{key}`")));
        }
        if object.get("op").and_then(Value::as_str) != Some("remove_vignette") {
            return Err(refuse("op must be remove_vignette"));
        }
        let list = object
            .get("layers")
            .and_then(Value::as_array)
            .ok_or_else(|| refuse("layers must be an array of {layer, map} objects"))?;
        let mut layers: Vec<(usize, [f64; 9])> = Vec::with_capacity(list.len());
        for value in list {
            let (layer, map) = entry(value)?;
            if layers.iter().any(|(l, _)| *l == layer) {
                return Err(refuse(&format!("layer {layer} is listed twice")));
            }
            layers.push((layer, map));
        }
        if layers.len() < 2 {
            return Err(refuse("layers must name at least 2 layers"));
        }
        Ok(Self { layers })
    }
}

/// `name`'s refusal for listed layer `index` and its `reason` (Vignette
/// Removal's and Geometric Distortion's).
pub(crate) fn layer_refusal(name: &str, index: usize, reason: &str) -> String {
    format!("{name} cannot change layer {index}: {reason}.")
}

/// The lock reason for a layer `owner` blocks (itself, or a locked group).
pub(crate) fn lock_reason(index: usize, owner: Option<usize>) -> String {
    match owner {
        Some(owner) if owner != index => {
            format!("it is inside group {owner}, which is locked (pixels)")
        }
        _ => "it is locked (pixels)".into(),
    }
}

/// [`plan_value`]'s scratch: the bookkeeping, the fit's samples and system
/// for `frames` layers, and `write` (the adapter's own allocation for its
/// largest layer).
pub(crate) fn plan_scratch(write: u64, frames: usize) -> u64 {
    PLAN_BOOKKEEPING
        .saturating_add(FIT_SCRATCH_BYTES)
        .saturating_add(system_bytes(frames))
        .saturating_add(write)
}

/// The plan JSON both depths answer.
pub(crate) fn plan_value(
    retained: u64,
    scratch: u64,
    output: u64,
    fit: &VignetteFit,
    request: &RemoveVignetteRequest,
) -> Value {
    json!({
        "retained_bytes": retained, "scratch_bytes": scratch, "output_bytes": output,
        "coefficient": fit.coefficient, "gain": fit.gain, "samples": fit.samples,
        "layers": request.layers.iter().map(|(l, _)| *l).collect::<Vec<_>>(),
    })
}

/// A canvas pixel (x, y) in the layer standing at `offset`, or `None`
/// before its origin or past u32.
pub(crate) fn local(offset: (i32, i32), x: i64, y: i64) -> Option<(u32, u32)> {
    let lx = u32::try_from(x - i64::from(offset.0)).ok()?;
    let ly = u32::try_from(y - i64::from(offset.1)).ok()?;
    Some((lx, ly))
}

/// The linear-light reader of a byte stack's `layers` (in request order) for
/// `align_vignette::fit` and `align_distortion::fit`: sRGB-decoded, a pixel
/// below full alpha read as absent.
pub(crate) fn byte_linear<'a>(
    layers: &'a [&'a Layer],
) -> impl Fn(usize, i64, i64) -> Option<[f64; 3]> + 'a {
    let table: [f64; 256] = std::array::from_fn(|v| f64::from(srgb_to_linear(v as f32 / 255.0)));
    move |frame, x, y| {
        let layer = layers.get(frame)?;
        let (lx, ly) = local(layer.offset, x, y)?;
        let p = layer.pixels.sample_at(lx, ly)?;
        (p[3] == 255).then(|| {
            [
                table[usize::from(p[0])],
                table[usize::from(p[1])],
                table[usize::from(p[2])],
            ]
        })
    }
}

/// `(retained, scratch, output)` and the frames of a byte request.
struct Prepared<'a> {
    frames: Vec<VignetteFrame>,
    layers: Vec<&'a Layer>,
    retained: u64,
    scratch: u64,
    output: u64,
}

impl DrDocument {
    /// The raster layer a lens correction `name` reads or changes, or its
    /// refusal: index, kind, artwork or retained source, and the Pixels lock.
    pub(crate) fn lens_layer(&self, index: usize, name: &str) -> Result<&Layer, String> {
        let layer = self
            .layers
            .get(index)
            .ok_or_else(|| layer_refusal(name, index, "there is no such layer"))?;
        if layer.kind == LayerKind::Group {
            return Err(layer_refusal(
                name,
                index,
                "it is a group, not a raster layer",
            ));
        }
        if crate::retouch_sampling::is_adjustment(layer) {
            return Err(layer_refusal(
                name,
                index,
                "it is an adjustment layer, not a raster layer",
            ));
        }
        if layer.meta.is_some() || self.has_retained_source(index) {
            return Err(layer_refusal(
                name,
                index,
                "it holds artwork or a placed source; rasterize it first",
            ));
        }
        let blocking = self.lock_block(index, EditKind::Pixels);
        if blocking != 0 {
            let owner = self.lock_owner(index, blocking);
            return Err(layer_refusal(name, index, &lock_reason(index, owner)));
        }
        Ok(layer)
    }

    fn vignette_prepare(&self, request: &RemoveVignetteRequest) -> Result<Prepared<'_>, String> {
        let mut frames = Vec::with_capacity(request.layers.len());
        let mut layers = Vec::with_capacity(request.layers.len());
        let (mut output, mut largest) = (0u64, 0u64);
        for (index, map) in &request.layers {
            let layer = self.lens_layer(*index, NAME)?;
            let (w, h) = layer.pixels.dimensions();
            let pixels = u64::from(w) * u64::from(h);
            output = output.saturating_add(pixels.saturating_mul(4));
            largest = largest.max(pixels);
            frames.push(VignetteFrame {
                rect: (i64::from(layer.offset.0), i64::from(layer.offset.1), w, h),
                map: *map,
            });
            layers.push(layer);
        }
        let mut inventory = crate::resources::Inventory::default();
        inventory.document(self);
        let retained = inventory.unique_bytes().saturating_add(output);
        let scratch = plan_scratch(largest.saturating_mul(4), frames.len());
        Ok(Prepared {
            frames,
            layers,
            retained,
            scratch,
            output,
        })
    }

    fn vignette_fit(
        &self,
        prepared: &Prepared<'_>,
        job: JobContext<'_>,
    ) -> Result<VignetteFit, String> {
        fit(&prepared.frames, byte_linear(&prepared.layers), job)
    }

    /// The plan: `{"retained_bytes","scratch_bytes","output_bytes",
    /// "coefficient","gain","samples","layers"}` (module doc). Runs the fit,
    /// so every refusal the edit would answer answers here; nothing is
    /// admitted (`limit` and `other` are the edit's, taken for one shape).
    pub fn remove_vignette_plan(
        &self,
        request: &RemoveVignetteRequest,
        _limit: u64,
        _other: u64,
    ) -> Result<Value, String> {
        let prepared = self.vignette_prepare(request)?;
        let fit = self.vignette_fit(&prepared, JobContext::NONE)?;
        Ok(plan_value(
            prepared.retained,
            prepared.scratch,
            prepared.output,
            &fit,
            request,
        ))
    }

    /// The edit: every listed layer with the fitted vignette divided out;
    /// `Ok(None)` when no sample changed.
    pub fn remove_vignette(
        &self,
        request: &RemoveVignetteRequest,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        let prepared = self.vignette_prepare(request)?;
        admit(prepared.retained, prepared.scratch, other, limit)?;
        job.reserve(prepared.scratch)?;
        let fit = self.vignette_fit(&prepared, job.range(0.0, 0.5))?;
        let mut next: Option<Self> = None;
        let count = request.layers.len() as f64;
        for (k, ((index, _), frame)) in request.layers.iter().zip(&prepared.frames).enumerate() {
            let part = job.range(
                0.5 + 0.5 * k as f64 / count,
                0.5 + 0.5 * (k + 1) as f64 / count,
            );
            let mut image = prepared.layers[k].pixels.to_contiguous_with_job(part)?;
            let width = frame.rect.2 as usize;
            let radial = Radial::of(frame.rect);
            let mut changed = false;
            if width > 0 {
                for (row, line) in image.chunks_exact_mut(width * 4).enumerate() {
                    let y = frame.rect.1 as f64 + row as f64 + 0.5;
                    for (column, px) in line.chunks_exact_mut(4).enumerate() {
                        part.every(row * width + column)?;
                        if px[3] == 0 {
                            continue;
                        }
                        let x = frame.rect.0 as f64 + column as f64 + 0.5;
                        let mut p = [
                            f32::from(px[0]) / 255.0,
                            f32::from(px[1]) / 255.0,
                            f32::from(px[2]) / 255.0,
                            f32::from(px[3]) / 255.0,
                        ];
                        apply_vignette_stops(&mut p, radial.stops(&fit, x, y), false, None);
                        for (code, value) in px.iter_mut().zip(p).take(3) {
                            let written = (value * 255.0).round() as u8;
                            if written != *code {
                                *code = written;
                                changed = true;
                            }
                        }
                    }
                    part.report((row + 1) as f64 / f64::from(frame.rect.3), 4)?;
                }
            }
            if changed {
                let doc = next.get_or_insert_with(|| self.clone());
                doc.layers[*index].pixels = LayerRaster::unseeded(image);
            }
        }
        job.report(1.0, 5)?;
        Ok(next)
    }
}
