//! Edit ▸ Auto-Blend Layers on the byte document: the request, the
//! report and the 8-bit adapter over `focus_stack` (Stack Images) and over
//! `panorama_gain` / `panorama_seam` (Panorama). `precision_auto_blend` is
//! the 16-bit and float adapter over the same request, report and helpers.
//!
//! The request's `method` is `stack_images` (the default, so every request
//! written before Panorama means what it meant) or `panorama`.
//!
//! **Stack Images.** Each selected raster layer takes a canvas-sized layer
//! mask revealing the region where it is the sharpest frame; with
//! `seamless`, one merged raster layer built by `focus_stack::blend` through
//! those masks is added.
//!
//! **Panorama.** Each selected raster layer takes a canvas-sized layer mask
//! revealing the region a seam through each overlap gives it
//! (`panorama_seam`). With `seamless` (Seamless Tones and Colors), each
//! frame's RGB is first scaled by its gain (`panorama_gain`), the seams are
//! cut on the gained frames, and the frames are multi-band blended across the
//! seams by `focus_stack::blend` (the one pyramid). The blend is written back
//! INTO the frames: no layer is added, each frame keeps its size, offset and
//! alpha bit for bit, and its RGB is the blend where its own mask reveals it
//! and its gained RGB elsewhere (`doc_panorama_blend`). That is take1's
//! default build row C-PIXELS (S untested there): the masks stay editable,
//! and a pixel a user later reveals by painting a mask shows the frame's own
//! tone-matched picture.
//!
//! # The masks
//!
//! Canvas-sized at (0, 0), binary, linked, enabled, density 1, feather 0,
//! outside coverage 0. Binary because the masks must PARTITION the covered
//! area: stacked, the layers then show exactly one frame per pixel with no
//! hole, whatever their order, opacity or blend mode.
//!
//! # The merged layer (Stack Images)
//!
//! Canvas-sized at (0, 0), Normal, opacity 1, no mask, unclipped, at
//! `insert_slot_above_clip_run(top selected layer)`: above the last layer
//! clipped to the top selected one, so it joins no clip run and the layers
//! clipped to that frame stay clipped to it.
//!
//! # Refusals
//!
//! Each names its layer: fewer than two distinct layers, more than
//! `focus_stack::MAX_FRAMES`, an index out of range, a group, an adjustment
//! layer, a layer that already has a mask, a layer whose locks block a mask
//! edit (and, for Panorama with `seamless`, a pixel edit), growth past native
//! capacity (the masks or the merged layer, by name), and a selection with no
//! pixel inside the canvas. Visibility, opacity, blend mode and style are not
//! consulted. The plan runs the same stage, so it answers every refusal the
//! edit would.
//!
//! The order is validate, plan, begin the run, admit, reserve, then for
//! Stack Images score, masks, blend, insert; for Panorama gains (with
//! `seamless`), seams, masks, then with `seamless` blend and write back;
//! report.

use image::{GrayImage, RgbaImage};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::blend::{LUMA_B, LUMA_G, LUMA_R};
use crate::doc::{DrDocument, Layer, LayerKind, LayerRaster};
use crate::doc_geometry_set::is_adjustment;
use crate::doc_lock::EditKind;
use crate::drdc::MAX_DRDC_NAME_LEN;
use crate::focus_stack::{self, MAX_FRAMES, NO_WINNER};
use crate::job_control::{DrJobControl, JobContext};
use crate::layer_insert::InsertSlot;
use crate::panorama_gain;
use crate::panorama_seam;
use crate::precision_image::{admit, reserved_vec};

/// The whole selection is transparent inside the canvas.
pub(crate) const NOTHING_COVERED: &str =
    "None of the selected layers has a pixel inside the canvas, so there is nothing to blend.";

/// The share of the job the score takes when a blend follows it; the blend
/// runs to [`BLEND_END`] and the insert takes the rest.
pub(crate) const SCORE_SHARE: f64 = 0.4;
pub(crate) const BLEND_END: f64 = 0.95;

/// Panorama with `seamless`: the gains take the job to here and the seams
/// start from it.
pub(crate) const GAIN_END: f64 = 0.1;

/// Auto-Blend Layers' Blend Method.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutoBlendMethod {
    /// Focus stacking: each pixel from the sharpest frame.
    #[default]
    StackImages,
    /// Seams through the overlaps (module doc).
    Panorama,
}

impl AutoBlendMethod {
    /// The request's spelling.
    pub fn name(self) -> &'static str {
        match self {
            Self::StackImages => "stack_images",
            Self::Panorama => "panorama",
        }
    }
}

fn default_name() -> String {
    "Auto-Blend".into()
}

/// The strict request both depths parse: `{"layers":[…],"seamless":bool,
/// "method":"stack_images"|"panorama","name":string}`, `method` and `name`
/// optional.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutoBlendRequest {
    /// The layers to blend, in any order; a repeat counts once.
    pub layers: Vec<usize>,
    /// Stack Images: whether the merged layer is added. Panorama: Seamless
    /// Tones and Colors (the gains, the blend and its write-back).
    pub seamless: bool,
    /// Absent is `stack_images`.
    #[serde(default)]
    pub method: AutoBlendMethod,
    /// The merged layer's name (Stack Images).
    #[serde(default = "default_name")]
    pub name: String,
}

impl AutoBlendRequest {
    /// `Err` for a request over 64 KiB, malformed JSON, an unknown field or
    /// a name past the native limit.
    pub fn parse(json: &str) -> Result<Self, String> {
        if json.len() > 64 * 1024 {
            return Err("auto-blend request exceeds 64 KiB".into());
        }
        let request: Self =
            serde_json::from_str(json).map_err(|e| format!("invalid auto-blend request: {e}"))?;
        if request.name.len() > MAX_DRDC_NAME_LEN as usize {
            return Err("auto-blend layer name exceeds the native limit".into());
        }
        Ok(request)
    }

    /// Whether the edit adds the merged layer: Stack Images with `seamless`.
    pub(crate) fn adds_layer(&self) -> bool {
        self.seamless && self.method == AutoBlendMethod::StackImages
    }

    /// Whether the edit writes the frames' own pixels: Panorama with
    /// `seamless`.
    pub(crate) fn writes_pixels(&self) -> bool {
        self.seamless && self.method == AutoBlendMethod::Panorama
    }

    /// `(gains end, labels end)` of the job: the gains take `0..gains end`
    /// (Panorama with `seamless`), the score or the seams the rest up to
    /// `labels end`, a blend [`SCORE_SHARE`]`..`[`BLEND_END`].
    pub(crate) fn shares(&self) -> (f64, f64) {
        let gains = if self.writes_pixels() { GAIN_END } else { 0.0 };
        (
            gains,
            if self.seamless {
                SCORE_SHARE
            } else {
                BLEND_END
            },
        )
    }

    /// The frames: the distinct requested indices, ascending (bottom first),
    /// so equal scores keep the lower layer. `Err` for fewer than two, more
    /// than `MAX_FRAMES` or an index at or past `count`.
    pub(crate) fn frames(&self, count: usize) -> Result<Vec<usize>, String> {
        let mut frames = self.layers.clone();
        frames.sort_unstable();
        frames.dedup();
        if let Some(&index) = frames.iter().find(|&&index| index >= count) {
            return Err(format!("Auto-Blend Layers: layer {index} is out of range."));
        }
        if frames.len() < 2 {
            return Err("Auto-Blend Layers needs at least two layers.".into());
        }
        if frames.len() > MAX_FRAMES {
            return Err(format!(
                "Auto-Blend Layers takes at most {MAX_FRAMES} layers."
            ));
        }
        Ok(frames)
    }
}

/// What the edit did, for the host's reply.
#[derive(Clone, Debug, PartialEq)]
pub struct AutoBlendReport {
    pub method: AutoBlendMethod,
    /// `(layer index, pixels its mask reveals)`, one per frame, ascending.
    pub won_pixels: Vec<(usize, u64)>,
    /// Canvas pixels at least one frame covers.
    pub covered_pixels: u64,
    pub canvas_pixels: u64,
    /// The merged layer's index in the result; `None` without `seamless` and
    /// always for Panorama.
    pub merged_layer: Option<usize>,
    /// Panorama with `seamless`: each frame's RGB gain, ascending.
    pub gains: Option<Vec<[f32; 3]>>,
}

impl AutoBlendReport {
    /// The report over `labels` (`focus_stack::winners`' or
    /// `panorama_seam::seams`' answer for `frames`).
    pub(crate) fn of(
        method: AutoBlendMethod,
        labels: &[u8],
        frames: &[usize],
        merged_layer: Option<usize>,
        gains: Option<Vec<[f32; 3]>>,
    ) -> Self {
        let mut counts = [0u64; 256];
        for &label in labels {
            counts[usize::from(label)] += 1;
        }
        let canvas_pixels = labels.len() as u64;
        Self {
            method,
            won_pixels: frames
                .iter()
                .enumerate()
                .map(|(frame, &layer)| (layer, counts[frame]))
                .collect(),
            covered_pixels: canvas_pixels - counts[usize::from(NO_WINNER)],
            canvas_pixels,
            merged_layer,
            gains,
        }
    }

    /// `{"method","layers":[{"layer","won_pixels"(,"gain":[r,g,b])}],
    /// "covered_pixels","canvas_pixels","merged_layer"}`; `gain` only when
    /// [`Self::gains`] is `Some`.
    pub fn json(&self) -> Value {
        json!({
            "method": self.method.name(),
            "layers": self
                .won_pixels
                .iter()
                .enumerate()
                .map(|(frame, (layer, won))| {
                    let mut row = json!({"layer": layer, "won_pixels": won});
                    if let Some(gain) = self.gains.as_ref().and_then(|g| g.get(frame)) {
                        row["gain"] = json!(gain);
                    }
                    row
                })
                .collect::<Vec<_>>(),
            "covered_pixels": self.covered_pixels,
            "canvas_pixels": self.canvas_pixels,
            "merged_layer": self.merged_layer,
        })
    }
}

/// Why one layer cannot be a frame.
pub(crate) enum Unfit {
    Group,
    Adjustment,
    Masked,
    Locked,
}

impl Unfit {
    pub(crate) fn sentence(&self, name: &str) -> String {
        match self {
            Self::Group => {
                format!("“{name}” is a group. Auto-Blend Layers takes raster layers.")
            }
            Self::Adjustment => {
                format!("“{name}” is an adjustment layer. Auto-Blend Layers takes raster layers.")
            }
            Self::Masked => format!(
                "“{name}” already has a layer mask. Apply or remove it before Auto-Blend Layers."
            ),
            Self::Locked => {
                format!("“{name}” is locked. Unlock it before Auto-Blend Layers.")
            }
        }
    }
}

/// The capacity refusal: `part` is what does not fit ("the layer masks do",
/// "the merged layer does"), `detail` the capacity rule's own sentence when
/// it has one.
pub(crate) fn capacity(part: &str, detail: Option<&str>) -> String {
    let detail = detail.map_or(String::new(), |detail| format!(" ({detail})"));
    format!(
        "Auto-Blend Layers would grow the document past its native capacity: {part} not fit{detail}."
    )
}

/// The kernel's `(working set, work units)`. Stack Images: the score alone,
/// or the larger of the score and the blend (the score's planes are freed
/// before the blend allocates) with both works. Panorama: the larger of the
/// gains (with `seamless`), the seams and `focus_stack::winners_plan`'s
/// bytes, and with `seamless` the blend's too; the works of the gains, the
/// seams and the blend. The winners term is the per-canvas-pixel charge of
/// the masks-only peak outside any kernel (set while the masks are set and
/// the display is prepared, `focus_stack::WINNERS_BYTES_PER_PIXEL`), which
/// the seams' capped analysis grid cannot carry; no scoring runs, so its
/// work is not added.
pub(crate) fn kernel_plan(
    size: (u32, u32),
    frames: usize,
    method: AutoBlendMethod,
    seamless: bool,
) -> Result<(u64, u64), String> {
    let (score, score_work) = focus_stack::winners_plan(size, frames)?;
    let (mut bytes, mut work) = match method {
        AutoBlendMethod::StackImages => (score, score_work),
        AutoBlendMethod::Panorama => {
            let (seams, seams_work) = panorama_seam::seams_plan(size, frames)?;
            let (gains, gains_work) = if seamless {
                panorama_gain::gains_plan(size, frames)?
            } else {
                (0, 0)
            };
            (
                score.max(seams).max(gains),
                seams_work.saturating_add(gains_work),
            )
        }
    };
    if seamless {
        let (blend, blend_work) = focus_stack::blend_plan(size, frames)?;
        bytes = bytes.max(blend);
        work = work.saturating_add(blend_work);
    }
    Ok((bytes, work))
}

/// What both adapters charge for one new mask plane of `plane_bytes`: the
/// plane at 5/4, as `float_adjust_apply::coverage_scratch` charges the
/// canvas coverage. The kernel's constants are measured against a fixed frame
/// count, and a mask plane is one more canvas-sized buffer per frame, so a
/// plane charged only its own bytes let the margin fall under 1.25 × the
/// projected peak past 21, 18 and 34 frames (Seamless on, 8 bits, 16 bits,
/// float); at 5/4 each plane carries its own quarter and the plan holds the
/// margin at any frame count (`focus_stack`'s records).
pub(crate) fn mask_charge(plane_bytes: u64) -> u64 {
    plane_bytes.saturating_add(plane_bytes.div_ceil(4))
}

/// The plan JSON both depths answer.
pub(crate) fn plan_json(retained: u64, scratch: u64, work: u64, merged: Option<usize>) -> Value {
    json!({
        "retained_bytes": retained,
        "scratch_bytes": scratch,
        "work_units": work,
        "merged_index": merged,
    })
}

/// One frame's rectangle against the canvas, in the frame's own columns.
pub(crate) struct Span {
    /// The frame's columns inside the canvas, `x0..x1`.
    pub(crate) x0: u32,
    pub(crate) x1: u32,
    offset: (i64, i64),
    rows: u32,
}

impl Span {
    /// `None` when the frame has no column inside the canvas.
    pub(crate) fn new(offset: (i32, i32), dims: (u32, u32), canvas: (u32, u32)) -> Option<Self> {
        let offset = (i64::from(offset.0), i64::from(offset.1));
        let x0 = (-offset.0).max(0);
        let x1 = (i64::from(canvas.0) - offset.0).min(i64::from(dims.0));
        (x0 < x1).then_some(Self {
            x0: x0 as u32,
            x1: x1 as u32,
            offset,
            rows: dims.1,
        })
    }

    /// The frame's row under canvas row `y`, when it has one.
    pub(crate) fn row(&self, y: u32) -> Option<u32> {
        let row = i64::from(y) - self.offset.1;
        (0..i64::from(self.rows))
            .contains(&row)
            .then_some(row as u32)
    }

    /// The canvas column of frame column `x` (inside `x0..x1`).
    pub(crate) fn column(&self, x: u32) -> usize {
        (i64::from(x) + self.offset.0) as usize
    }

    /// The canvas rows the frame has, clamped to `height`.
    pub(crate) fn canvas_rows(&self, height: u32) -> std::ops::Range<u32> {
        let top = self.offset.1.clamp(0, i64::from(height));
        let bottom = (self.offset.1 + i64::from(self.rows)).clamp(top, i64::from(height));
        top as u32..bottom as u32
    }
}

/// One frame's mask plane: `full` where the frame won, 0 elsewhere.
pub(crate) fn mask_plane<T: Copy + Default>(
    labels: &[u8],
    frame: usize,
    full: T,
) -> Result<Vec<T>, String> {
    let mut plane: Vec<T> = reserved_vec(labels.len())?;
    plane.extend(labels.iter().map(|&label| {
        if usize::from(label) == frame {
            full
        } else {
            T::default()
        }
    }));
    Ok(plane)
}

/// The validated request: the frames, the merged layer's slot and the plan.
struct Stage {
    frames: Vec<usize>,
    slot: Option<InsertSlot>,
    scratch: u64,
    work: u64,
}

impl DrDocument {
    fn auto_blend_stage(&self, request: &AutoBlendRequest) -> Result<Stage, String> {
        let frames = request.frames(self.layers.len())?;
        for &index in &frames {
            let layer = &self.layers[index];
            let unfit = if layer.kind == LayerKind::Group {
                Some(Unfit::Group)
            } else if is_adjustment(layer.meta.as_deref()) {
                Some(Unfit::Adjustment)
            } else if layer.mask.is_some() {
                Some(Unfit::Masked)
            } else if self.lock_block(index, EditKind::Mask) != 0
                || (request.writes_pixels() && self.lock_block(index, EditKind::Pixels) != 0)
            {
                Some(Unfit::Locked)
            } else {
                None
            };
            if let Some(unfit) = unfit {
                return Err(unfit.sentence(&layer.name));
            }
        }
        let canvas = (self.width, self.height);
        let masks = self
            .layers
            .iter()
            .filter_map(|l| l.mask.as_ref().map(|m| m.dimensions()))
            .chain(frames.iter().map(|_| canvas));
        if !self.native_masks_fit_dimensions(masks) {
            return Err(capacity("the layer masks do", None));
        }
        let slot = if request.adds_layer() {
            // The check `adding_layer_at` makes before it allocates.
            if !self.native_layers_fit_dimensions(
                self.layers
                    .iter()
                    .map(|l| l.pixels.dimensions())
                    .chain(std::iter::once(canvas)),
            ) {
                return Err(capacity("the merged layer does", None));
            }
            let top = frames[frames.len() - 1];
            Some(self.insert_slot_above_clip_run(top)?)
        } else {
            None
        };
        if !frames.iter().any(|&index| self.auto_blend_covers(index)) {
            return Err(NOTHING_COVERED.into());
        }
        let (kernel, work) = kernel_plan(canvas, frames.len(), request.method, request.seamless)?;
        let pixels = u64::from(self.width) * u64::from(self.height);
        // The new pixels: the merged layer, or each frame's written-back
        // raster at its own size.
        let written = if request.adds_layer() {
            pixels.saturating_mul(4)
        } else if request.writes_pixels() {
            frames
                .iter()
                .map(|&index| {
                    let (w, h) = self.layers[index].pixels.dimensions();
                    (u64::from(w) * u64::from(h)).saturating_mul(4)
                })
                .fold(0u64, u64::saturating_add)
        } else {
            0
        };
        let scratch = kernel
            .saturating_add(mask_charge(pixels).saturating_mul(frames.len() as u64))
            .saturating_add(written);
        Ok(Stage {
            frames,
            slot,
            scratch,
            work,
        })
    }

    /// Whether layer `index` has a pixel with alpha above 0 inside the
    /// canvas. Stops at the first one.
    fn auto_blend_covers(&self, index: usize) -> bool {
        let layer = &self.layers[index];
        let Some(span) = Span::new(
            layer.offset,
            layer.pixels.dimensions(),
            (self.width, self.height),
        ) else {
            return false;
        };
        span.canvas_rows(self.height).any(|y| {
            span.row(y).is_some_and(|row| {
                layer
                    .pixels
                    .row_segments(row, span.x0, span.x1)
                    .any(|(_, run)| run.iter().any(|p| p[3] != 0))
            })
        })
    }

    fn auto_blend_retained(&self) -> u64 {
        let mut inventory = crate::resources::Inventory::default();
        inventory.document(self);
        inventory.unique_bytes()
    }

    /// The plan of [`Self::auto_blend`]: `retained_bytes`, `scratch_bytes`
    /// (the kernel's working set, the new mask planes and the merged layer
    /// or the frames' written-back rasters), `work_units` and `merged_index`
    /// (where the merged layer would land, `null` without `seamless` and for
    /// Panorama). `Err` with the sentence of every refusal
    /// the edit would give (module doc).
    pub fn auto_blend_plan(&self, request: &AutoBlendRequest) -> Result<Value, String> {
        let stage = self.auto_blend_stage(request)?;
        Ok(plan_json(
            self.auto_blend_retained(),
            stage.scratch,
            stage.work,
            stage.slot.map(|slot| slot.at),
        ))
    }

    /// Auto-Blend Layers (module doc): the document with a mask on each
    /// requested layer and, with `seamless`, the merged layer (Stack Images)
    /// or the blended frames (Panorama), plus the report. `Err` for every refusal, a plan over `limit` beside
    /// `other_retained`, and a cancel; the input is never changed.
    pub fn auto_blend(
        &self,
        request: &AutoBlendRequest,
        limit: u64,
        other_retained: u64,
        control: Option<&DrJobControl>,
    ) -> Result<(Self, AutoBlendReport), String> {
        let stage = self.auto_blend_stage(request)?;
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        admit(
            self.auto_blend_retained(),
            stage.scratch,
            other_retained,
            limit,
        )?;
        job.reserve(stage.scratch)?;
        let size = (self.width, self.height);
        let frames = &stage.frames;
        let spans: Vec<Option<Span>> = frames
            .iter()
            .map(|&index| {
                let layer = &self.layers[index];
                Span::new(layer.offset, layer.pixels.dimensions(), size)
            })
            .collect();
        let read = |frame: usize, y: u32, out: &mut [[f32; 4]]| {
            out.fill([0.0; 4]);
            let Some(span) = &spans[frame] else {
                return;
            };
            let Some(row) = span.row(y) else {
                return;
            };
            let pixels = &self.layers[frames[frame]].pixels;
            for (x, run) in pixels.row_segments(row, span.x0, span.x1) {
                let at = span.column(x);
                for (sample, p) in out[at..at + run.len()].iter_mut().zip(run) {
                    *sample = p.map(|v| f32::from(v) / 255.0);
                }
            }
        };
        let (gain_end, score_end) = request.shares();
        let gains = if request.writes_pixels() {
            Some(panorama_gain::gains(
                size,
                frames.len(),
                &read,
                job.range(0.0, gain_end),
            )?)
        } else {
            None
        };
        let shown = |frame: usize, y: u32, out: &mut [[f32; 4]]| {
            read(frame, y, out);
            if let Some(gains) = &gains {
                panorama_gain::gained(gains[frame], out);
            }
        };
        let labels = match request.method {
            AutoBlendMethod::StackImages => focus_stack::winners(
                size,
                frames.len(),
                &read,
                [LUMA_R, LUMA_G, LUMA_B],
                job.range(0.0, score_end),
            )?,
            AutoBlendMethod::Panorama => panorama_seam::seams(
                size,
                frames.len(),
                &shown,
                [LUMA_R, LUMA_G, LUMA_B],
                job.range(gain_end, score_end),
            )?,
        };
        let mut doc = self.clone();
        for (frame, &index) in frames.iter().enumerate() {
            job.check()?;
            let plane = mask_plane(&labels, frame, 255u8)?;
            let mask = GrayImage::from_raw(size.0, size.1, plane)
                .ok_or("Auto-Blend Layers could not build a layer mask")?;
            doc = doc.set_mask(index, mask, (0, 0), true, 0).ok_or_else(|| {
                format!(
                    "Auto-Blend Layers could not set the layer mask of “{}”.",
                    self.layers[index].name
                )
            })?;
        }
        let mut merged = None;
        if let Some(slot) = stage.slot {
            let blended = focus_stack::blend(
                size,
                frames.len(),
                &labels,
                &read,
                job.range(SCORE_SHARE, BLEND_END),
            )?;
            let mut bytes: Vec<u8> = reserved_vec(blended.len() * 4)?;
            for (i, pixel) in blended.iter().enumerate() {
                job.every(i)?;
                bytes.extend(pixel.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
            }
            drop(blended);
            let image = RgbaImage::from_raw(size.0, size.1, bytes)
                .ok_or("Auto-Blend Layers could not build the merged layer")?;
            let layer = Layer::from_raster(LayerRaster::unseeded(image), &request.name);
            doc = doc
                .inserting_layer(layer, slot)
                .ok_or("Auto-Blend Layers could not insert the merged layer.")?;
            merged = Some(slot.at);
        } else if let Some(gains) = &gains {
            let blended = focus_stack::blend(
                size,
                frames.len(),
                &labels,
                &shown,
                job.range(SCORE_SHARE, BLEND_END),
            )?;
            doc =
                doc.panorama_frames(frames, &labels, &blended, gains, job.range(BLEND_END, 1.0))?;
        }
        // The net `fallible_doc_op` applies to every byte edit; this export
        // answers a report beside the document, so it runs the check itself.
        if !self.native_edit_fits(&doc) {
            return Err("the edited document exceeds native capacity".into());
        }
        let report = AutoBlendReport::of(request.method, &labels, frames, merged, gains);
        job.report(1.0, 5)?;
        Ok((doc, report))
    }
}
