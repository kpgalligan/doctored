//! Edit ▸ Auto-Align Layers' Geometric Distortion (phase 2b-4): the
//! request and the byte and typed documents' thin adapters over
//! `align_distortion`. The fit only READS the stack, so both depths live in
//! this one file. The host registers the layers (`AutoAlignReprojection.swift`),
//! sends every registered layer with its anchor and Vision map in one request,
//! takes back the coefficient and the refined maps, and reprojects each layer
//! with the coefficient (`project_layer`'s `distortion`).
//!
//! The request is `{"op":"fit_distortion","layers":[{"layer":N,"anchor":M,
//! "map":[9]},…]}`, at most 64 KiB, strict: at least 2 distinct layers; the
//! FIRST entry is the reference, its own anchor with the identity map; every
//! later anchor is an earlier entry's layer; each `map` is row-major, finite,
//! with a positive determinant, taking the layer's canvas point to the
//! reference's.
//!
//! The layers are read in linear light through Vignette Removal's readers
//! (`doc_align_vignette::byte_linear`, `precision_align_vignette::
//! typed_linear`) and checked by its layer and record checks under this
//! feature's name; a CMYK document refuses by name.
//!
//! THE ANSWER is `{"coefficient","matches","residual","layers":[N…],
//! "maps":[[9]…]}`, the maps in request order, each a layer's CORRECTED
//! canvas point to the reference's corrected point (m[8] = 1). No document
//! changes.
//!
//! THE PLAN is `{"retained_bytes","scratch_bytes"}`: the document as it
//! stands (nothing new is held) and `PLAN_BOOKKEEPING` plus the fit's
//! `FIT_SCRATCH_BYTES` and the level-2 window of the largest listed layer's
//! step (`align_distortion::refine_bytes`). It runs no matching.
use serde_json::{json, Map, Value};

use crate::align_distortion::{fit, refine_bytes, step, DistortionFit, FIT_SCRATCH_BYTES};
use crate::align_vignette::VignetteFrame;
use crate::doc::DrDocument;
use crate::doc_align_vignette::byte_linear;
use crate::doc_puppet::{PLAN_BOOKKEEPING, REQUEST_LIMIT};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_align_vignette::{cmyk_refusal, typed_linear};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::admit;

/// The feature name the refusals carry.
pub(crate) const NAME: &str = "Geometric Distortion";

/// A parsed `fit_distortion` request: each layer, its anchor and its map, in
/// the request's order.
#[derive(Clone, Debug)]
pub struct FitDistortionRequest {
    pub layers: Vec<(usize, usize, [f64; 9])>,
}

fn refuse(what: &str) -> String {
    format!("invalid fit distortion request: {what}")
}

fn index(object: &Map<String, Value>, key: &str) -> Result<usize, String> {
    object
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| refuse(&format!("{key} must be a layer index")))
}

fn entry(value: &Value) -> Result<(usize, usize, [f64; 9]), String> {
    let object: &Map<String, Value> = value
        .as_object()
        .ok_or_else(|| refuse("each layers entry must be an object with layer, anchor and map"))?;
    if let Some(key) = object
        .keys()
        .find(|k| !matches!(k.as_str(), "layer" | "anchor" | "map"))
    {
        return Err(refuse(&format!("unknown field `{key}`")));
    }
    let layer = index(object, "layer")?;
    let anchor = index(object, "anchor")?;
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
    Ok((layer, anchor, map))
}

impl FitDistortionRequest {
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
        if object.get("op").and_then(Value::as_str) != Some("fit_distortion") {
            return Err(refuse("op must be fit_distortion"));
        }
        let list = object
            .get("layers")
            .and_then(Value::as_array)
            .ok_or_else(|| refuse("layers must be an array of {layer, anchor, map} objects"))?;
        let mut layers: Vec<(usize, usize, [f64; 9])> = Vec::with_capacity(list.len());
        for value in list {
            let (layer, anchor, map) = entry(value)?;
            if layers.iter().any(|(l, _, _)| *l == layer) {
                return Err(refuse(&format!("layer {layer} is listed twice")));
            }
            if layers.is_empty() {
                let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
                if anchor != layer || map != identity {
                    return Err(refuse(
                        "the first entry must be the reference: its own anchor, with the identity map",
                    ));
                }
            } else if !layers.iter().any(|(l, _, _)| *l == anchor) {
                return Err(refuse(&format!(
                    "the anchor of layer {layer} must be an earlier entry's layer"
                )));
            }
            layers.push((layer, anchor, map));
        }
        if layers.len() < 2 {
            return Err(refuse("layers must name at least 2 layers"));
        }
        Ok(Self { layers })
    }

    /// Each entry's anchor as an index into the request.
    fn anchors(&self) -> Vec<usize> {
        self.layers
            .iter()
            .map(|(_, anchor, _)| {
                self.layers
                    .iter()
                    .position(|(l, _, _)| l == anchor)
                    .unwrap_or(0)
            })
            .collect()
    }
}

/// The plan's scratch for `frames`: the bookkeeping, the fit's records and
/// the largest frame's level-2 window.
fn plan_scratch(frames: &[VignetteFrame]) -> u64 {
    let largest = frames.iter().map(|f| step(f.rect)).max().unwrap_or(1);
    PLAN_BOOKKEEPING
        .saturating_add(FIT_SCRATCH_BYTES)
        .saturating_add(refine_bytes(largest))
}

/// The answer JSON both depths give.
fn answer(fit: &DistortionFit, request: &FitDistortionRequest) -> Value {
    json!({
        "coefficient": fit.coefficient, "matches": fit.matches, "residual": fit.residual,
        "layers": request.layers.iter().map(|(l, _, _)| *l).collect::<Vec<_>>(),
        "maps": fit.maps,
    })
}

fn frame(rect: (i64, i64, u32, u32), map: [f64; 9]) -> VignetteFrame {
    VignetteFrame { rect, map }
}

impl DrDocument {
    fn distortion_frames(
        &self,
        request: &FitDistortionRequest,
    ) -> Result<(Vec<VignetteFrame>, Vec<&crate::doc::Layer>), String> {
        let mut frames = Vec::with_capacity(request.layers.len());
        let mut layers = Vec::with_capacity(request.layers.len());
        for (index, _, map) in &request.layers {
            let layer = self.lens_layer(*index, NAME)?;
            let (w, h) = layer.pixels.dimensions();
            frames.push(frame(
                (i64::from(layer.offset.0), i64::from(layer.offset.1), w, h),
                *map,
            ));
            layers.push(layer);
        }
        Ok((frames, layers))
    }

    fn distortion_retained(&self) -> u64 {
        let mut inventory = crate::resources::Inventory::default();
        inventory.document(self);
        inventory.unique_bytes()
    }

    /// The plan: `{"retained_bytes","scratch_bytes"}` (module doc). Checks
    /// every listed layer, so a refusal by name answers here; runs no
    /// matching and admits nothing (`limit` and `other` are the fit's).
    pub fn fit_distortion_plan(
        &self,
        request: &FitDistortionRequest,
        _limit: u64,
        _other: u64,
    ) -> Result<Value, String> {
        let (frames, _) = self.distortion_frames(request)?;
        Ok(json!({
            "retained_bytes": self.distortion_retained(),
            "scratch_bytes": plan_scratch(&frames),
        }))
    }

    /// The fit (module doc): admits its scratch, changes no document.
    pub fn fit_distortion(
        &self,
        request: &FitDistortionRequest,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Value, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        let (frames, layers) = self.distortion_frames(request)?;
        let scratch = plan_scratch(&frames);
        admit(self.distortion_retained(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let fitted = fit(&frames, &request.anchors(), byte_linear(&layers), job)?;
        job.report(1.0, 5)?;
        Ok(answer(&fitted, request))
    }
}

impl PrecisionDocument {
    fn distortion_frames(
        &self,
        request: &FitDistortionRequest,
    ) -> Result<DistortionStack<'_>, String> {
        if self.is_cmyk() {
            return Err(cmyk_refusal(NAME));
        }
        let mut frames = Vec::with_capacity(request.layers.len());
        let mut images = Vec::with_capacity(request.layers.len());
        for (index, _, map) in &request.layers {
            let (image, offset) = self.lens_record(*index, NAME)?;
            let (w, h) = image.dimensions();
            frames.push(frame(
                (i64::from(offset.0), i64::from(offset.1), w, h),
                *map,
            ));
            images.push((image, offset));
        }
        Ok((frames, images))
    }

    /// The typed twin of `DrDocument::fit_distortion_plan`.
    pub fn fit_distortion_plan(
        &self,
        request: &FitDistortionRequest,
        _limit: u64,
        _other: u64,
    ) -> Result<Value, String> {
        let (frames, _) = self.distortion_frames(request)?;
        Ok(json!({
            "retained_bytes": self.retained_bytes(),
            "scratch_bytes": plan_scratch(&frames),
        }))
    }

    /// The typed twin of `DrDocument::fit_distortion`.
    pub fn fit_distortion(
        &self,
        request: &FitDistortionRequest,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Value, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        let (frames, images) = self.distortion_frames(request)?;
        let scratch = plan_scratch(&frames);
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let fitted = fit(&frames, &request.anchors(), typed_linear(&images)?, job)?;
        job.report(1.0, 5)?;
        Ok(answer(&fitted, request))
    }
}

/// A typed stack's frames and its images with their offsets.
type DistortionStack<'a> = (
    Vec<VignetteFrame>,
    Vec<(&'a crate::precision_image::PrecisionImage, (i32, i32))>,
);
