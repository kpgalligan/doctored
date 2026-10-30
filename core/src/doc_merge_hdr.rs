//! Merge to HDR over a document's layers: the adapters around the
//! `merge_hdr` kernel. An 8-bit document (`DrDocument`) and a 16-bit one
//! (`PrecisionDocument`) answer the same three questions about 2…16 of their
//! raster layers, each layer one exposure of one scene:
//!
//! * `merge_hdr_exposures` — the layers' relative exposures estimated from
//!   their pixels, `{"evs":[…]}` relative to the first listed layer, or
//!   `{"evs":null,"reason":…}` naming the pair that shares too few
//!   well-exposed pixels (a reply, not an error: the caller then asks for the
//!   values);
//! * `merge_hdr_plan` — `{"width","height","frames","output_bytes",
//!   "scratch_bytes"}`, what `merge_hdr` admits and reserves;
//! * `merge_hdr` — a NEW 32-bit float document holding the merge as one
//!   raster layer, "Merged HDR", with the report `{"reference",
//!   "all_clipped_pixels","all_dark_pixels","deghosted_pixels",
//!   "noise_sigma":[number|null…]}`. The source document is not changed.
//!
//! # The request
//!
//! JSON of at most [`MAX_REQUEST_BYTES`], unknown keys refused:
//! `{"version":1,"layers":[int…],"evs":[number…],"reference":int,
//! "deghost":bool}`. `layers` are 2…16 distinct indices of raster leaves.
//! `evs` is one finite number per layer in stops (larger = more light),
//! `reference` is one of `layers`, and every `|ev − ev[reference]|` is at most
//! `merge_hdr::EV_SPAN_MAX`. `merge_hdr_exposures` takes `version` and
//! `layers` alone.
//!
//! # The frames
//!
//! Each listed layer is read on the CANVAS grid: its offset applied, alpha 0
//! outside its own rectangle. Its mask, opacity, blend mode and style are
//! ignored (a frame is a photograph, not a composite). A stored code becomes
//! linear light through the ONE typed transfer of the document's profile,
//! `TypedTransfer::for_profile(profile).decode(code / 65535)`, a byte widened
//! to `v · 257` first (`precision_conversion::promote_runs`' rule), so an
//! 8-bit frame decodes exactly as its 16-bit promotion would. Its `level` is
//! the largest stored RGB code over full scale.
//!
//! # The result
//!
//! `PrecisionImage::from_rgba32_extended` → `PrecisionDocumentLayer::
//! raster_float` → `PrecisionDocument::from_parts_float` with the source's
//! profile and resolution, empty metadata (the frames' capture packets
//! describe one exposure, not the merge) and the `Clip` tone map. Those are
//! `float_document`'s own gates; nothing here admits an f32 leaf. A
//! Grayscale source (#547 phase 6b) gives a 32-bit Grayscale document: the
//! merged image is folded to its gray by the Mode rule
//! (`gray_artwork::fold_artwork` under the source's policy at 32 bits, which
//! keeps a neutral pixel's `Y`), and the document takes that policy.
//!
//! # Refusals
//!
//! `Err`, each naming the layer: a layer that is already 32-bit float, a
//! group or an adjustment layer, a profile `FloatPolicy::for_profile`
//! refuses, an EV farther than the span from the reference; and a malformed
//! request, a working-memory refusal or a cancel.
//!
//! # Memory
//!
//! `output_bytes` is the result's 16 bytes a pixel. `scratch_bytes` is the
//! contiguous-to-tiled adoption copy of it (`float_adoption_copy_bytes`), the
//! kernel's rows and lattice (`merge_hdr::scratch_bytes`), the transfer's
//! tables (`TypedTransfer::table_bytes`) and four bookkeeping units.
//! `merge_hdr` admits the source's retained bytes plus both figures against
//! the limit and reserves both on the job before it reads a row. On a Gray
//! source the fold's float image (16 bytes a pixel) is admitted beside the
//! result before it is built.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::json;

use crate::doc::{DrDocument, LayerKind};
use crate::doc_geometry_set::is_adjustment;
use crate::float_document::{FloatPolicy, ToneMap};
use crate::gray_document::{GrayDepth, GrayPolicy};
use crate::icc::IccProfile;
use crate::job_control::{DrJobControl, JobContext};
use crate::layer_raster::LayerRaster;
use crate::merge_hdr::{self, Estimate, FrameRows, FrameSample};
use crate::metadata::{Metadata, Resolution};
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_evaluation::TypedTransfer;
use crate::precision_image::{
    admit, pixel_count, PrecisionImage, PrecisionRun, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::precision_sampling::float_adoption_copy_bytes;
use crate::resources::Inventory;

/// The largest request the parser reads: sixteen indices and sixteen numbers
/// are a few hundred bytes, so 64 KiB is far past any honest request and
/// small enough that a hostile one costs nothing to refuse.
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;

/// The one request version this build reads.
const VERSION: u32 = 1;

/// The merged layer's name.
const MERGED_NAME: &str = "Merged HDR";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExposuresRequest {
    version: u32,
    layers: Vec<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MergeRequest {
    version: u32,
    layers: Vec<usize>,
    evs: Vec<f64>,
    reference: usize,
    deghost: bool,
}

fn parsed<'a, T: Deserialize<'a>>(request: &'a str) -> Result<T, String> {
    if request.len() > MAX_REQUEST_BYTES {
        return Err("merge to hdr request exceeds the supported byte limit".into());
    }
    serde_json::from_str(request).map_err(|e| format!("merge to hdr request is not valid: {e}"))
}

/// The version and the layer list's count and distinctness.
fn checked_layers(version: u32, layers: &[usize]) -> Result<(), String> {
    if version != VERSION {
        return Err(format!(
            "merge to hdr request version {version} is not supported"
        ));
    }
    if !(merge_hdr::MIN_FRAMES..=merge_hdr::MAX_FRAMES).contains(&layers.len()) {
        return Err(format!(
            "Merge to HDR takes {} to {} layers; {} were given",
            merge_hdr::MIN_FRAMES,
            merge_hdr::MAX_FRAMES,
            layers.len()
        ));
    }
    for (position, index) in layers.iter().enumerate() {
        if layers[..position].contains(index) {
            return Err(format!("merge to hdr layer {index} is listed twice"));
        }
    }
    Ok(())
}

/// One frame's stored pixels.
enum Pixels<'a> {
    Byte(&'a LayerRaster),
    Typed(&'a PrecisionImage),
}

/// One listed layer as a frame: its name for a refusal, its pixels and where
/// they sit on the canvas.
struct Frame<'a> {
    name: &'a str,
    pixels: Pixels<'a>,
    offset: (i32, i32),
}

impl Frame<'_> {
    fn dimensions(&self) -> (u32, u32) {
        match self.pixels {
            Pixels::Byte(raster) => raster.dimensions(),
            Pixels::Typed(image) => image.dimensions(),
        }
    }
}

fn group_refusal(name: &str) -> String {
    format!("“{name}” is a group; Merge to HDR takes raster layers")
}

fn adjustment_refusal(name: &str) -> String {
    format!("“{name}” is an adjustment layer; Merge to HDR takes raster layers")
}

fn range_refusal(index: usize) -> String {
    format!("merge to hdr layer {index} is out of range")
}

/// The validated frames of one request with what decodes them.
struct Stack<'a> {
    size: (u32, u32),
    frames: Vec<Frame<'a>>,
    transfer: Option<Decode>,
}

/// The document profile's linear decode: the typed transfer, and its values
/// at the 256 widened byte codes (`v · 257`), read once so a byte frame costs
/// a table lookup a channel.
struct Decode {
    transfer: TypedTransfer,
    byte: Box<[[f32; 3]; 256]>,
}

impl Decode {
    fn for_profile(profile: &Arc<IccProfile>) -> Result<Self, String> {
        let transfer = TypedTransfer::for_profile(profile)?;
        let mut byte = Box::new([[0.0f32; 3]; 256]);
        for (value, entry) in byte.iter_mut().enumerate() {
            let encoded = (value as u16 * 257) as f32 / 65535.0;
            *entry = transfer.decode([encoded; 3]);
        }
        Ok(Self { transfer, byte })
    }
}

impl<'a> Stack<'a> {
    /// The frames after every refusal that needs no pixel: the profile first
    /// (it is the document's, so it is one answer for every layer).
    fn validated(
        size: (u32, u32),
        profile: &Arc<IccProfile>,
        frames: Vec<Frame<'a>>,
    ) -> Result<Self, String> {
        pixel_count(size.0, size.1)?;
        FloatPolicy::for_profile(profile, ToneMap::Clip)?;
        Ok(Self {
            size,
            frames,
            transfer: None,
        })
    }

    /// The same stack with the decode tables built, ready to read rows.
    fn decoding(mut self, profile: &Arc<IccProfile>) -> Result<Self, String> {
        self.transfer = Some(Decode::for_profile(profile)?);
        Ok(self)
    }
}

impl FrameRows for Stack<'_> {
    fn frames(&self) -> usize {
        self.frames.len()
    }

    fn size(&self) -> (u32, u32) {
        self.size
    }

    fn row(&self, frame: usize, y: u32, out: &mut [FrameSample]) {
        out.fill(FrameSample::default());
        let (Some(frame), Some(decode)) = (self.frames.get(frame), self.transfer.as_ref()) else {
            return;
        };
        let (width, height) = frame.dimensions();
        let line = i64::from(y) - i64::from(frame.offset.1);
        if line < 0 || line >= i64::from(height) {
            return;
        }
        // The layer columns that land on the canvas: canvas x = layer x + offset.
        let left = i64::from(frame.offset.0);
        let first = (-left).clamp(0, i64::from(width));
        let last = (out.len() as i64 - left).clamp(0, i64::from(width));
        if first >= last {
            return;
        }
        let (line, first, last) = (line as u32, first as u32, last as u32);
        let place = |x: u32| (i64::from(x) + left) as usize;
        match frame.pixels {
            Pixels::Byte(raster) => {
                for (x, run) in raster.row_segments(line, first, last) {
                    for (sample, code) in out[place(x)..].iter_mut().zip(run) {
                        *sample = FrameSample {
                            linear: [
                                decode.byte[usize::from(code[0])][0],
                                decode.byte[usize::from(code[1])][1],
                                decode.byte[usize::from(code[2])][2],
                            ],
                            level: f32::from(code[0].max(code[1]).max(code[2])) / 255.0,
                            alpha: f32::from(code[3]) / 255.0,
                        };
                    }
                }
            }
            Pixels::Typed(image) => {
                for (x, run) in image.row_segments(line, first, last) {
                    let PrecisionRun::Rgba16(run) = run else {
                        continue;
                    };
                    for (sample, code) in out[place(x)..].iter_mut().zip(run) {
                        let encoded = [code[0], code[1], code[2]].map(|v| f32::from(v) / 65535.0);
                        *sample = FrameSample {
                            linear: decode.transfer.decode(encoded),
                            level: f32::from(code[0].max(code[1]).max(code[2])) / 65535.0,
                            alpha: f32::from(code[3]) / 65535.0,
                        };
                    }
                }
            }
        }
    }
}

/// `{"evs":[…]}` or the `reason` reply, for a validated stack.
fn exposures(
    stack: Stack<'_>,
    profile: &Arc<IccProfile>,
    job: JobContext<'_>,
) -> Result<String, String> {
    let (width, height) = stack.size;
    let scratch = merge_hdr::scratch_bytes(width, height, stack.frames.len())
        .and_then(|bytes| bytes.checked_add(TypedTransfer::table_bytes()))
        .and_then(|bytes| bytes.checked_add(BOOKKEEPING_BYTES))
        .ok_or("merge to hdr working-memory calculation overflow")?;
    job.reserve(scratch)?;
    let stack = stack.decoding(profile)?;
    let reply = match merge_hdr::estimate_evs(&stack, job)? {
        Estimate::Evs(evs) => json!({ "evs": evs }),
        Estimate::Unpaired { a, b } => json!({
            "evs": null,
            "reason": format!(
                "the exposures of “{}” and “{}” could not be estimated (too few pixels are well exposed in both); give each frame's EV",
                stack.frames[a].name, stack.frames[b].name
            ),
        }),
    };
    job.report(1.0, 5)?;
    Ok(reply.to_string())
}

/// A merge request's values after the checks that need the frames' names:
/// one finite EV per layer, the reference among the layers, every EV within
/// the span of it. Answers the reference's POSITION in `layers`.
fn checked_exposures(request: &MergeRequest, frames: &[Frame<'_>]) -> Result<usize, String> {
    if request.evs.len() != request.layers.len() {
        return Err(format!(
            "Merge to HDR takes one EV per layer; {} were given for {} layers",
            request.evs.len(),
            request.layers.len()
        ));
    }
    if let Some(position) = request.evs.iter().position(|ev| !ev.is_finite()) {
        return Err(format!(
            "the EV of “{}” is not a finite number",
            frames[position].name
        ));
    }
    let reference = request
        .layers
        .iter()
        .position(|index| *index == request.reference)
        .ok_or_else(|| {
            format!(
                "merge to hdr reference {} is not one of the listed layers",
                request.reference
            )
        })?;
    for (frame, ev) in frames.iter().zip(&request.evs) {
        let distance = (ev - request.evs[reference]).abs();
        if distance > merge_hdr::EV_SPAN_MAX {
            return Err(format!(
                "“{}” is {distance:.1} EV from the reference “{}”; Merge to HDR takes exposures within {} EV of it",
                frame.name,
                frames[reference].name,
                merge_hdr::EV_SPAN_MAX
            ));
        }
    }
    Ok(reference)
}

/// `(output_bytes, scratch_bytes)` of a merge (module doc, "Memory").
fn plan_bytes(size: (u32, u32), frames: usize) -> Result<(u64, u64), String> {
    let overflow = || "merge to hdr working-memory calculation overflow".to_string();
    let output = (pixel_count(size.0, size.1)? as u64)
        .checked_mul(PrecisionStorage::Rgba32Float.bytes_per_pixel())
        .ok_or_else(overflow)?;
    let scratch = float_adoption_copy_bytes(size)?
        .checked_add(merge_hdr::scratch_bytes(size.0, size.1, frames).ok_or_else(overflow)?)
        .and_then(|bytes| bytes.checked_add(TypedTransfer::table_bytes()))
        .and_then(|bytes| bytes.checked_add(4 * BOOKKEEPING_BYTES))
        .ok_or_else(overflow)?;
    Ok((output, scratch))
}

fn plan_json(size: (u32, u32), frames: usize) -> Result<String, String> {
    let (output, scratch) = plan_bytes(size, frames)?;
    Ok(json!({
        "width": size.0, "height": size.1, "frames": frames,
        "output_bytes": output, "scratch_bytes": scratch,
    })
    .to_string())
}

/// What a merge needs of its source document beside the frames.
struct Source<'a> {
    profile: &'a Arc<IccProfile>,
    resolution: Resolution,
    retained: u64,
    /// A Grayscale source's policy: the result is a 32-bit Gray document.
    gray: Option<&'a Arc<GrayPolicy>>,
}

/// The merge of a validated stack into a new float document, with its report.
fn merged(
    stack: Stack<'_>,
    request: &MergeRequest,
    source: Source<'_>,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<(PrecisionDocument, String), String> {
    let reference = checked_exposures(request, &stack.frames)?;
    let size = stack.size;
    let (output, scratch) = plan_bytes(size, stack.frames.len())?;
    let working = output
        .checked_add(scratch)
        .ok_or("merge to hdr working-memory calculation overflow")?;
    admit(source.retained, working, other, limit)?;
    job.reserve(working)?;
    let stack = stack.decoding(source.profile)?;
    let result = merge_hdr::merge(
        &stack,
        &request.evs,
        reference,
        request.deghost,
        job.range(0.0, 0.9),
    )?;
    let image = PrecisionImage::from_rgba32_extended(
        size.0,
        size.1,
        result.pixels,
        source.profile.clone(),
        None,
    )?;
    let gray = source
        .gray
        .map(|policy| Arc::new(policy.with_depth(GrayDepth::ThirtyTwo)));
    let image = match &gray {
        Some(policy) => {
            // The fold's float image beside the result (module doc, "Memory").
            let fold = output;
            admit(source.retained.saturating_add(output), fold, other, limit)?;
            job.reserve(output.saturating_add(fold))?;
            crate::gray_artwork::fold_artwork(&image, policy, job.range(0.9, 0.95))?
                .unwrap_or(image)
        }
        None => image,
    };
    let layer = PrecisionDocumentLayer::raster_float(MERGED_NAME, image, (0, 0), 1.0, true)?;
    let document = PrecisionDocument::from_parts_float(
        size.0,
        size.1,
        source.profile.clone(),
        Metadata::default(),
        source.resolution,
        vec![layer],
        Vec::new(),
        ToneMap::Clip,
    )?;
    let document = match gray {
        Some(policy) => document.with_gray(policy)?,
        None => document,
    };
    let report = json!({
        "reference": request.reference,
        "all_clipped_pixels": result.all_clipped,
        "all_dark_pixels": result.all_dark,
        "deghosted_pixels": result.deghosted,
        "noise_sigma": result.noise_sigma,
    });
    job.report(1.0, 5)?;
    Ok((document, report.to_string()))
}

/// The opening every controlled entry shares: claim the token, poll once.
fn begun(
    control: Option<&DrJobControl>,
) -> Result<(Option<crate::job_control::JobRun<'_>>, JobContext<'_>), String> {
    let run = control.map(DrJobControl::begin).transpose()?;
    let job = control.map_or(JobContext::NONE, JobContext::new);
    job.check()?;
    Ok((run, job))
}

impl DrDocument {
    /// The listed layers as frames; refuses an index out of range, a group
    /// and an adjustment layer, each by name.
    fn merge_hdr_stack(&self, layers: &[usize]) -> Result<Stack<'_>, String> {
        let mut frames = Vec::with_capacity(layers.len());
        for &index in layers {
            let layer = self.layers.get(index).ok_or_else(|| range_refusal(index))?;
            if layer.kind == LayerKind::Group {
                return Err(group_refusal(&layer.name));
            }
            if is_adjustment(layer.meta.as_deref()) {
                return Err(adjustment_refusal(&layer.name));
            }
            frames.push(Frame {
                name: &layer.name,
                pixels: Pixels::Byte(&layer.pixels),
                offset: layer.offset,
            });
        }
        Stack::validated((self.width, self.height), &self.profile, frames)
    }

    /// The listed layers' relative exposures (module doc). `request` is
    /// `{"version":1,"layers":[…]}`; `Err` for every refusal the merge itself
    /// would give for those layers, and for a cancel.
    pub fn merge_hdr_exposures(
        &self,
        request: &str,
        control: Option<&DrJobControl>,
    ) -> Result<String, String> {
        let (_run, job) = begun(control)?;
        let request: ExposuresRequest = parsed(request)?;
        checked_layers(request.version, &request.layers)?;
        let stack = self.merge_hdr_stack(&request.layers)?;
        exposures(stack, &self.profile, job)
    }

    /// The merge's plan JSON (module doc), after every refusal the merge
    /// gives before it allocates.
    pub fn merge_hdr_plan(&self, request: &str) -> Result<String, String> {
        let request: MergeRequest = parsed(request)?;
        checked_layers(request.version, &request.layers)?;
        let stack = self.merge_hdr_stack(&request.layers)?;
        checked_exposures(&request, &stack.frames)?;
        plan_json(stack.size, stack.frames.len())
    }

    /// The listed layers merged into a NEW 32-bit float document and the
    /// report JSON (module doc). `self` is unchanged. `Err` names the layer a
    /// refusal is about; a working-memory refusal and a cancel are `Err` too.
    pub fn merge_hdr(
        &self,
        request: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<(PrecisionDocument, String), String> {
        let (_run, job) = begun(control)?;
        admit(0, 4 * BOOKKEEPING_BYTES, other, limit)?;
        job.reserve(4 * BOOKKEEPING_BYTES)?;
        let request: MergeRequest = parsed(request)?;
        checked_layers(request.version, &request.layers)?;
        let stack = self.merge_hdr_stack(&request.layers)?;
        let mut inventory = Inventory::default();
        inventory.document(self);
        let source = Source {
            profile: &self.profile,
            resolution: self.resolution,
            retained: inventory.unique_bytes(),
            gray: None,
        };
        merged(stack, &request, source, limit, other, job)
    }
}

impl PrecisionDocument {
    /// The listed records as frames; refuses an index out of range, a group,
    /// an adjustment layer and a 32-bit float raster, each by name.
    fn merge_hdr_stack(&self, layers: &[usize]) -> Result<Stack<'_>, String> {
        let mut frames = Vec::with_capacity(layers.len());
        for &index in layers {
            let layer = self.layer(index).ok_or_else(|| range_refusal(index))?;
            if layer.children().is_some() {
                return Err(group_refusal(layer.name()));
            }
            let image = layer
                .source()
                .ok_or_else(|| adjustment_refusal(layer.name()))?;
            if image.storage() == PrecisionStorage::Rgba32Float {
                return Err(format!(
                    "“{}” is already 32-bit float; Merge to HDR takes 8- or 16-bit exposures",
                    layer.name()
                ));
            }
            frames.push(Frame {
                name: layer.name(),
                pixels: Pixels::Typed(image),
                offset: layer.offset(),
            });
        }
        Stack::validated(self.dimensions(), self.profile(), frames)
    }

    /// `DrDocument::merge_hdr_exposures` over 16-bit records.
    pub fn merge_hdr_exposures(
        &self,
        request: &str,
        control: Option<&DrJobControl>,
    ) -> Result<String, String> {
        let (_run, job) = begun(control)?;
        let request: ExposuresRequest = parsed(request)?;
        checked_layers(request.version, &request.layers)?;
        let stack = self.merge_hdr_stack(&request.layers)?;
        exposures(stack, self.profile(), job)
    }

    /// `DrDocument::merge_hdr_plan` over 16-bit records.
    pub fn merge_hdr_plan(&self, request: &str) -> Result<String, String> {
        let request: MergeRequest = parsed(request)?;
        checked_layers(request.version, &request.layers)?;
        let stack = self.merge_hdr_stack(&request.layers)?;
        checked_exposures(&request, &stack.frames)?;
        plan_json(stack.size, stack.frames.len())
    }

    /// `DrDocument::merge_hdr` over 16-bit records. A 32-bit float document
    /// refuses, naming the first listed raster.
    pub fn merge_hdr(
        &self,
        request: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<(PrecisionDocument, String), String> {
        let (_run, job) = begun(control)?;
        admit(0, 4 * BOOKKEEPING_BYTES, other, limit)?;
        job.reserve(4 * BOOKKEEPING_BYTES)?;
        let request: MergeRequest = parsed(request)?;
        checked_layers(request.version, &request.layers)?;
        let stack = self.merge_hdr_stack(&request.layers)?;
        let source = Source {
            profile: self.profile(),
            resolution: self.resolution(),
            retained: self.retained_bytes(),
            gray: self.gray_policy(),
        };
        merged(stack, &request, source, limit, other, job)
    }
}
