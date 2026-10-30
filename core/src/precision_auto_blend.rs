//! Edit ▸ Auto-Blend Layers at 16 bits and 32-bit float: the typed
//! adapter over `focus_stack` (Stack Images) and `panorama_gain` /
//! `panorama_seam` (Panorama). `doc_auto_blend` holds the request, the
//! report, the refusal sentences and the rules for the masks, the merged
//! layer and Panorama's write-back; this file states only what differs at
//! high depth.
//!
//! The masks are u16 planes (0 or 65535) and the merged record is a u16
//! raster, or an f32 raster in a float document; Panorama writes each frame
//! back in its own storage (`precision_panorama_blend`).
//!
//! # A float document blends in the retouch encoding
//!
//! The reader hands the kernel `retouch_encoding::encode` codes over 65535
//! for RGB, and the merged record is `retouch_encoding::decode`d once at the
//! write, so the score, Panorama's gains and seams, and the pyramid all run
//! on the compressed encoding and values above white survive. In linear light a few very bright pixels
//! would decide every region they touch (the score is a squared difference,
//! so one 5000-valued highlight outweighs all the texture of a focused
//! subject) and a pyramid sum would ring around them. The encoding is the
//! sRGB curve up to white and a logarithm above it, so an SDR picture held in
//! a float document blends as its 16-bit twin does.
//!
//! # Panorama reads display units
//!
//! Panorama's reader divides the code by the code of white (`encode(1)`,
//! about 11,155) instead of 65535, so an SDR picture held in a float document
//! reads 0…1 as its 16-bit twin does. The seam's ε and λ, the gains' σ_N and
//! clip bounds are all constants in those units: over 65535, white reads
//! about 0.17, every colour difference and gradient is six times smaller
//! against ε, and Brown and Lowe's prior outweighs the overlap's evidence (a
//! 0.8 exposure step came out at gain ratio 0.973). A gain is a ratio, so it
//! means the same on either scale; `precision_panorama_blend` decodes with
//! the same unit.
//!
//! # Lab Color
//!
//! A Lab document (#547 phase 6b-ii) reads its L\*a\*b\* codes over 65535.
//! Stack Images scores, and Panorama's seams take their gradient, on L\*
//! alone (`lab_stack::LIGHTNESS_LUMA`). Seamless Tones fits each frame's
//! gains as on RGB, then keeps only the Lightness gain
//! (`lab_stack::lightness_gains`: `[g, 1, 1]`), so the seams, the
//! write-back and the report all see a\* and b\* unchanged. The masks and the
//! pyramid blend run per channel on the codes, with no Lab arm of their own.

use serde_json::Value;

use crate::blend::{LUMA_B, LUMA_G, LUMA_R};
use crate::doc_auto_blend::{
    capacity, kernel_plan, mask_charge, mask_plane, plan_json, AutoBlendMethod, AutoBlendReport,
    AutoBlendRequest, Span, Unfit, BLEND_END, NOTHING_COVERED, SCORE_SHARE,
};
use crate::doc_lock::EditKind;
use crate::doc_mask::MaskProperties;
use crate::focus_stack;
use crate::job_control::{DrJobControl, JobContext};
use crate::lab_stack;
use crate::layer_insert::InsertSlot;
use crate::panorama_gain;
use crate::panorama_seam;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, reserved_vec, unit_to_u16, PrecisionImage};
use crate::precision_mask::{PrecisionMask, PrecisionPlane};
use crate::precision_panorama_blend::float_white_code;
use crate::retouch_encoding::{decode, encode};

/// The validated request: the frames, the merged record's slot and the plan.
struct Stage {
    frames: Vec<usize>,
    slot: Option<InsertSlot>,
    scratch: u64,
    work: u64,
}

impl PrecisionDocument {
    fn auto_blend_stage(&self, request: &AutoBlendRequest) -> Result<Stage, String> {
        // #547 Indexed phase 3b: an Indexed document holds one layer, and
        // Auto-Blend Layers needs more, for good.
        if self.is_indexed() {
            return Err(crate::indexed_document::refused_sentence(
                "auto_blend_layers",
            ));
        }
        let frames = request.frames(self.layer_count())?;
        for &index in &frames {
            let layer = self.layer(index).ok_or("invalid typed layer")?;
            let unfit = if layer.children().is_some() {
                Some(Unfit::Group)
            } else if layer.source().is_none() {
                Some(Unfit::Adjustment)
            } else if layer.mask().is_some() {
                Some(Unfit::Masked)
            } else if self.lock_block(index, EditKind::Mask) != 0
                || (request.writes_pixels() && self.lock_block(index, EditKind::Pixels) != 0)
            {
                Some(Unfit::Locked)
            } else {
                None
            };
            if let Some(unfit) = unfit {
                return Err(unfit.sentence(layer.name()));
            }
        }
        let canvas = self.dimensions();
        let (mut rasters, mut masks) = self.native_dimensions();
        masks.extend(frames.iter().map(|_| canvas));
        self.native_fits_geometry(canvas, &rasters, &masks, self.channels.len())
            .map_err(|e| capacity("the layer masks do", Some(&e)))?;
        let slot = if request.adds_layer() {
            rasters.push(canvas);
            let merged = |e: String| capacity("the merged layer does", Some(&e));
            self.native_fits_geometry(canvas, &rasters, &masks, self.channels.len())
                .map_err(merged)?;
            // The record cap and the name, as every `inserting_record` caller
            // checks before it builds its record.
            self.preflight_raster_addition(canvas, &request.name)
                .map_err(merged)?;
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
        let merged_bytes = if self.is_float() { 16 } else { 8 };
        // The new pixels: the merged record, or each frame's written-back
        // raster at its own size.
        let written = if request.adds_layer() {
            pixels.saturating_mul(merged_bytes)
        } else if request.writes_pixels() {
            frames
                .iter()
                .filter_map(|&index| self.layer(index).and_then(|layer| layer.source()))
                .map(|image| {
                    let (w, h) = image.dimensions();
                    (u64::from(w) * u64::from(h)).saturating_mul(merged_bytes)
                })
                .fold(0u64, u64::saturating_add)
        } else {
            0
        };
        let scratch = kernel
            .saturating_add(
                mask_charge(pixels.saturating_mul(2)).saturating_mul(frames.len() as u64),
            )
            .saturating_add(written);
        Ok(Stage {
            frames,
            slot,
            scratch,
            work,
        })
    }

    /// Whether record `index` has a pixel with alpha above 0 inside the
    /// canvas. Stops at the first one.
    fn auto_blend_covers(&self, index: usize) -> bool {
        let Some(layer) = self.layer(index) else {
            return false;
        };
        let Some(image) = layer.source() else {
            return false;
        };
        let Some(span) = Span::new(layer.offset(), image.dimensions(), self.dimensions()) else {
            return false;
        };
        span.canvas_rows(self.height).any(|y| {
            span.row(y).is_some_and(|row| {
                image
                    .row_segments(row, span.x0, span.x1)
                    .any(|(_, run)| (0..run.len()).any(|i| run.pixel(i)[3] > 0.0))
            })
        })
    }

    /// The plan of [`Self::auto_blend`]: `doc_auto_blend`'s plan JSON, with
    /// the masks at 2 bytes a pixel (charged at 5/4, `mask_charge`) and the
    /// merged record or Panorama's written-back frames at 8 bytes a pixel, or
    /// 16 in a float document. `Err` with the sentence of every refusal the edit
    /// would give.
    pub fn auto_blend_plan(&self, request: &AutoBlendRequest) -> Result<Value, String> {
        let stage = self.auto_blend_stage(request)?;
        Ok(plan_json(
            self.retained_bytes(),
            stage.scratch,
            stage.work,
            stage.slot.map(|slot| slot.at),
        ))
    }

    /// Auto-Blend Layers at high depth (module doc; the rules are
    /// `doc_auto_blend`'s): the document with a mask on each requested record
    /// and, with `seamless`, the merged record (Stack Images) or the blended
    /// frames (Panorama), plus the report. `Err` for
    /// every refusal, a plan over `limit` beside `other_retained`, and a
    /// cancel; the input is never changed.
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
        admit(self.retained_bytes(), stage.scratch, other_retained, limit)?;
        job.reserve(stage.scratch)?;
        let size = self.dimensions();
        let float = self.is_float();
        let frames = &stage.frames;
        let mut sources: Vec<(&PrecisionImage, Option<Span>)> = Vec::with_capacity(frames.len());
        for &index in frames {
            let layer = self.layer(index).ok_or("invalid typed layer")?;
            let image = layer.source().ok_or("invalid typed layer")?;
            sources.push((image, Span::new(layer.offset(), image.dimensions(), size)));
        }
        // Stack Images reads codes over 65535; Panorama over the code of
        // white (module doc, "Panorama reads display units").
        let unit = match request.method {
            AutoBlendMethod::StackImages => 65535.0,
            AutoBlendMethod::Panorama => float_white_code(),
        };
        let code = |linear: f32| encode(linear) / unit;
        let read = |frame: usize, y: u32, out: &mut [[f32; 4]]| {
            out.fill([0.0; 4]);
            let (image, Some(span)) = &sources[frame] else {
                return;
            };
            let Some(row) = span.row(y) else {
                return;
            };
            for (x, run) in image.row_segments(row, span.x0, span.x1) {
                let at = span.column(x);
                for (i, sample) in out[at..at + run.len()].iter_mut().enumerate() {
                    let p = run.pixel(i);
                    *sample = if float {
                        [code(p[0]), code(p[1]), code(p[2]), p[3]]
                    } else {
                        p
                    };
                }
            }
        };
        let (gain_end, score_end) = request.shares();
        let lab = self.is_lab();
        let gains = if request.writes_pixels() {
            let mut gains =
                panorama_gain::gains(size, frames.len(), &read, job.range(0.0, gain_end))?;
            // Seamless Tones on L* alone (module doc, "Lab Color").
            if lab {
                lab_stack::lightness_gains(&mut gains);
            }
            Some(gains)
        } else {
            None
        };
        let luma = if lab {
            lab_stack::LIGHTNESS_LUMA
        } else {
            [LUMA_R, LUMA_G, LUMA_B]
        };
        let shown = |frame: usize, y: u32, out: &mut [[f32; 4]]| {
            read(frame, y, out);
            if let Some(gains) = &gains {
                panorama_gain::gained(gains[frame], out);
            }
        };
        let labels = match request.method {
            AutoBlendMethod::StackImages => {
                focus_stack::winners(size, frames.len(), &read, luma, job.range(0.0, score_end))?
            }
            AutoBlendMethod::Panorama => panorama_seam::seams(
                size,
                frames.len(),
                &shown,
                luma,
                job.range(gain_end, score_end),
            )?,
        };
        let mut doc = self.clone();
        for (frame, &index) in frames.iter().enumerate() {
            job.check()?;
            let plane =
                PrecisionPlane::adopt(size.0, size.1, mask_plane(&labels, frame, 65535u16)?)?;
            let mask = PrecisionMask::new(plane, MaskProperties::at((0, 0)), true)?;
            doc = doc
                .set_mask_with_context(index, Some(mask), job)?
                .ok_or_else(|| {
                    format!(
                        "Auto-Blend Layers could not set the layer mask of “{}”.",
                        self.layer(index).map_or("", |layer| layer.name())
                    )
                })?;
        }
        let mut merged = None;
        if let Some(slot) = stage.slot {
            let mut blended = focus_stack::blend(
                size,
                frames.len(),
                &labels,
                &read,
                job.range(SCORE_SHARE, BLEND_END),
            )?;
            let record = if float {
                for (i, pixel) in blended.iter_mut().enumerate() {
                    job.every(i)?;
                    *pixel = [
                        decode(pixel[0] * 65535.0),
                        decode(pixel[1] * 65535.0),
                        decode(pixel[2] * 65535.0),
                        pixel[3].clamp(0.0, 1.0),
                    ];
                }
                let image = PrecisionImage::from_rgba32_extended(
                    size.0,
                    size.1,
                    blended,
                    self.profile.clone(),
                    None,
                )?;
                PrecisionDocumentLayer::raster_float(&request.name, image, (0, 0), 1.0, true)?
            } else {
                let mut codes: Vec<[u16; 4]> = reserved_vec(blended.len())?;
                for (i, pixel) in blended.iter().enumerate() {
                    job.every(i)?;
                    codes.push(pixel.map(unit_to_u16));
                }
                drop(blended);
                let image = PrecisionImage::from_rgba16_unseeded(
                    size.0,
                    size.1,
                    codes,
                    self.profile.clone(),
                    None,
                )?;
                PrecisionDocumentLayer::raster(&request.name, image, (0, 0), 1.0, true)?
            };
            doc = doc.inserting_record(record, slot)?;
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
        let report = AutoBlendReport::of(request.method, &labels, frames, merged, gains);
        job.report(1.0, 5)?;
        Ok((doc, report))
    }
}
