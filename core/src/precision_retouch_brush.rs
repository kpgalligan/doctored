//! The 16-bit adapters of the Blur, Sharpen and Sponge brushes
//! (`retouch_brush`, the kernel the byte `doc_retouch_brush` shares) and of
//! the History Brush.
//!
//! [`RetouchBrushEdit`] is the `retouch_brush` op of `dr_precision_doc_edit`:
//! its input is laid out as `dodge_burn`'s — the canvas-sized u16 stroke
//! coverage, then the canvas-sized selection plane when `selection` is set —
//! and the two planes multiply per pixel. The History Brush has its own export
//! because it takes a second document, the history state it paints from.
//!
//! Both walk only the layer rect the stroke coverage reaches
//! (`layer_rect::coverage_rect16`), write it back through `with_rect_rgba16`,
//! never grow the layer, and answer `Ok(None)` when no stored code changes. A
//! target that is not a u16 raster and a retained source refuse in
//! `NativePaintEdit::extent`'s sentences.
//!
//! A 32-bit float document leaves each entry point at its top
//! (`plan`, `applying_unlocked`, `history_brush_plan`,
//! `history_brush_unlocked`) for `float_retouch_brush`, the f32 arm, which
//! mirrors these walks step for step; the lock gate stays here. This module
//! is the MASTER of every step but the sample type.
use crate::adjust_vibrance::BoundVibrance;
use crate::doc_lock::{EditKind, LOCK_TRANSPARENCY};
use crate::icc_transform::PrecisionToneEncoder;
use crate::job_control::JobContext;
use crate::layer_rect::coverage_rect16;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{
    admit, pixel_count, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::retouch_brush::{
    halo, mix, mix_over, refuse_options, sponge_slider, targets, RetouchBrushKind, Vibrancy,
};
use crate::retouch_brush_sampling::{canvas_reach, plate_samples};
use serde::Deserialize;
use serde_json::{json, Value};

/// The retouch brushes' per-pixel plan term, charged over the LAYER (a bound
/// on the walk rect, which the plan cannot know before it sees the coverage):
/// the rect read with its halo (8 B), the Gaussian's binary64 row strip
/// (32 B), the targets (12 B) and the rect being rewritten (8 B).
///
/// Record (the Release app at `DR_WORKING_LIMIT_BYTES` 8 GiB, a fresh
/// instance per case, `MallocLargeCache=0`, E =
/// `ri_interval_max_phys_footprint` − pre around the one stroke call, 3 runs +
/// a baseline twin at E ≤ 16,384; a 6000×4000 16-bit layer of two-colour
/// Clouds and one size-200 stroke along (100,100)–(5900,3900), whose walk rect
/// is nearly the whole layer; the plan is 2,216,388,608):
/// - `blur_stroke`: E 1,336,198,056 / 1,336,411,048 / 1,336,296,360 (1.658×).
/// - `sharpen_stroke`: E 1,334,903,720 / 1,334,805,416 / 1,334,985,640
///   (1.660×).
/// - `sponge_stroke` (desaturate): E 763,003,456 / 762,429,968 / 762,544,656
///   (2.905×; no Gaussian strip).
///
/// Sample All Layers (`scripts/cmyk-admission/measure.py`
/// `blur-sal-rgb16`, the Release app, the same method, a 24 MP 16-bit RGB
/// PNG, `blur_stroke` along (100, 100)–(5900, 3900) at size 200 with
/// `sample_all_layers`): the plan, this term plus the sampled picture's
/// (`sampled_plate_plan`) and the stroke's footprint, is 3,232,074,272
/// against E 1,908,409,712 / 1,907,623,256 / 1,908,393,352 (1.694×).
const RETOUCH_BRUSH_BYTES_PER_PIXEL: u64 = 60;

/// The History Brush's per-pixel plan term, over the layer: the rect being
/// rewritten and the source layer's part of it, 8 B each.
///
/// Record ([`RETOUCH_BRUSH_BYTES_PER_PIXEL`]'s harness and stroke, painting the
/// Clouds state back over the inverted layer): E 577,094,040 / 577,094,040 /
/// 577,077,656 against `history_brush_plan` 1,160,388,608 (2.011×). That peak
/// is the publication's: an invert on the same document reaches 577,061,272.
pub(crate) const HISTORY_BRUSH_BYTES_PER_PIXEL: u64 = 16;

/// `{"op":"retouch_brush","layer":…,"kind":"blur"|"sharpen"|"saturate"|
/// "desaturate","strength":0..1,"selection":bool,"vibrance":bool,
/// "sample_all_layers":bool}`. `vibrance` (Sponge only) takes the
/// Vibrance target bound to the document's profile
/// (`BoundVibrance::prepare_precision`, the adjustment's own 16-bit binding);
/// `sample_all_layers` (Blur and Sharpen only) reads the sampled
/// picture (`retouch_brush_sampling`) and writes `retouch_brush::mix_over`.
/// Both default to false, the shipped behaviour. `ink_pass` is never read from
/// JSON: `cmyk_retouch_brush` sets it on the edit it runs on a CMYK record's
/// pass, whose Vibrance target runs through the sRGB curve
/// (`retouch_brush::Vibrancy::Encoded`).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RetouchBrushEdit {
    #[serde(rename = "op")]
    _op: String,
    pub(crate) layer: usize,
    pub(crate) kind: RetouchBrushKind,
    pub(crate) strength: f32,
    #[serde(default)]
    pub(crate) selection: bool,
    #[serde(default)]
    pub(crate) vibrance: bool,
    #[serde(default)]
    pub(crate) sample_all_layers: bool,
    #[serde(skip)]
    pub(crate) ink_pass: bool,
}

/// Layer `index` as a paintable u16 raster and its canvas offset, refused in
/// `NativePaintEdit::extent`'s sentences. A float document never reaches it:
/// its entry points branch to `float_retouch_brush` first.
pub(crate) fn paintable_u16(
    doc: &PrecisionDocument,
    index: usize,
) -> Result<(&PrecisionImage, (i32, i32)), String> {
    let layer = doc
        .layer(index)
        .ok_or("native paint target is out of range")?;
    let image = layer
        .source()
        .ok_or("native paint requires a raster target")?;
    if doc.is_float() || image.storage() != PrecisionStorage::Rgba16Unorm {
        return Err("native paint requires u16 storage".into());
    }
    if layer.retained_source.is_some() {
        return Err("Rasterize the retained source before painting its pixels".into());
    }
    Ok((image, layer.offset()))
}

/// A canvas-coverage plan in `NativePaintEdit::plan_in`'s shape: the paint
/// walk's `8·BOOKKEEPING_BYTES + n·24 + output·8`, plus `kernel` bytes for
/// each layer pixel.
pub(crate) fn coverage_plan(
    doc: &PrecisionDocument,
    image: &PrecisionImage,
    kernel: u64,
    selection: bool,
) -> Result<Value, String> {
    let (width, height) = doc.dimensions();
    let n = pixel_count(width, height)? as u64;
    let (w, h) = image.dimensions();
    let output = pixel_count(w, h)? as u64;
    let scratch = 8 * BOOKKEEPING_BYTES + n * 24 + output * 8 + output * kernel;
    let planes = if selection { 2 } else { 1 };
    Ok(json!({
        "retained_bytes": doc.retained_bytes(),
        "scratch_bytes": scratch,
        "input_samples": n * planes,
        "input_width": width,
        "input_height": height,
        "input_planes": planes,
    }))
}

/// The stroke's coverage of canvas sample `i` in [0, 1]: the coverage plane,
/// times the selection plane when the input carries one.
pub(crate) fn coverage_at(input: &[u16], n: usize, selected: bool, i: usize) -> f64 {
    let stroke = f64::from(input[i]) / 65535.0;
    if selected {
        stroke * f64::from(input[n + i]) / 65535.0
    } else {
        stroke
    }
}

fn unit(p: [u16; 4]) -> [f32; 4] {
    p.map(|v| f32::from(v) / 65535.0)
}

impl RetouchBrushEdit {
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| format!("invalid retouch brush request: {e}"))
    }

    /// `retained_bytes`, `scratch_bytes`, `input_samples` (and the input's
    /// layout), refused before anything is allocated: the walk's plan, plus
    /// the sampled picture's (`PrecisionDocument::sampled_plate_plan`) under
    /// `sample_all_layers`. A 32-bit float document plans its own walk
    /// (`float_retouch_brush`), a CMYK one its passes (`cmyk_retouch_brush`).
    pub(crate) fn plan(&self, doc: &PrecisionDocument) -> Result<Value, String> {
        refuse_options(self.kind, self.vibrance, self.sample_all_layers)?;
        let mut plan = self.walk_plan(doc)?;
        if self.sample_all_layers {
            let walk = plan["scratch_bytes"].as_u64().unwrap_or(u64::MAX);
            let scratch = walk
                .checked_add(doc.sampled_plate_plan(self.layer)?)
                .ok_or("retouch brush scratch overflow")?;
            plan["scratch_bytes"] = json!(scratch);
        }
        Ok(plan)
    }

    /// The walk's plan alone, without the sampled picture.
    pub(crate) fn walk_plan(&self, doc: &PrecisionDocument) -> Result<Value, String> {
        if doc.is_cmyk() {
            return doc.cmyk_retouch_brush_plan(self);
        }
        if doc.is_float() {
            return crate::float_retouch_brush::retouch_brush_plan(self, doc);
        }
        if !self.strength.is_finite() || !(0. ..=1.).contains(&self.strength) {
            return Err("retouch brush strength is invalid".into());
        }
        let (image, _) = paintable_u16(doc, self.layer)?;
        let mut plan = coverage_plan(doc, image, RETOUCH_BRUSH_BYTES_PER_PIXEL, self.selection)?;
        if self.vibrance && !self.ink_pass && !doc.is_gray() && !doc.is_lab() {
            // The Vibrance target binds the document's profile at 16 bits
            // (`BoundVibrance::prepare_precision`), whose continuous encoder
            // tables the walk holds for the whole stroke.
            let walk = plan["scratch_bytes"].as_u64().unwrap_or(u64::MAX);
            let scratch = walk
                .checked_add(PrecisionToneEncoder::scratch_bytes())
                .ok_or("retouch brush scratch overflow")?;
            plan["scratch_bytes"] = json!(scratch);
        }
        Ok(plan)
    }

    /// The LOCK gate (`precision_lock`): the brush writes its target's
    /// pixels, so the whole body runs under a `Pixels` edit. Under
    /// `sample_all_layers` an RGB document renders its sampled picture first
    /// (`sampled_plate`, which admits and reserves its own term), then admits
    /// and reserves the walk's term against `other` plus the picture's bytes,
    /// so the picture is charged once; a CMYK document samples per pass.
    pub(crate) fn apply(
        &self,
        doc: &PrecisionDocument,
        input: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        doc.under_locks(self.layer, EditKind::Pixels, |doc| {
            if !self.sample_all_layers || doc.is_cmyk() {
                return self.applying_unlocked(doc, input, limit, other, job, None);
            }
            self.plan(doc)?;
            let sampled = doc.sampled_plate(self.layer, limit, other, job.range(0., 0.3))?;
            let held = sampled.retained_bytes();
            self.applying_unlocked(
                doc,
                input,
                limit,
                other.saturating_add(held),
                job.with_held_bytes(held).range(0.3, 1.),
                Some(&sampled),
            )
        })
    }

    /// The lock gate, then the walk reading `sampled`, a canvas-sized
    /// picture at the origin (a CMYK record's pass of it).
    pub(crate) fn apply_sampled(
        &self,
        doc: &PrecisionDocument,
        input: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
        sampled: &PrecisionImage,
    ) -> Result<Option<PrecisionDocument>, String> {
        doc.under_locks(self.layer, EditKind::Pixels, |doc| {
            self.applying_unlocked(doc, input, limit, other, job, Some(sampled))
        })
    }

    fn applying_unlocked(
        &self,
        doc: &PrecisionDocument,
        input: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
        sampled: Option<&PrecisionImage>,
    ) -> Result<Option<PrecisionDocument>, String> {
        if doc.is_cmyk() {
            return doc.cmyk_retouch_brush_unlocked(self, input, limit, other, job);
        }
        if doc.is_float() {
            return crate::float_retouch_brush::retouch_brush_unlocked(
                self, doc, input, limit, other, job, sampled,
            );
        }
        refuse_options(self.kind, self.vibrance, self.sample_all_layers)?;
        let plan = self.walk_plan(doc)?;
        let scratch = plan["scratch_bytes"].as_u64().unwrap_or(u64::MAX);
        admit(doc.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        if input.len() as u64 != plan["input_samples"].as_u64().unwrap_or(u64::MAX) {
            return Err("native paint input length differs from plan".into());
        }
        if self.strength == 0. {
            job.report(1., 5)?;
            return Ok(None);
        }
        let (image, (ox, oy)) = paintable_u16(doc, self.layer)?;
        let (lw, lh) = image.dimensions();
        let canvas = doc.dimensions();
        let n = pixel_count(canvas.0, canvas.1)?;
        let Some((rx, ry, rw, rh)) = coverage_rect16(&input[..n], canvas, (lw, lh), (ox, oy))
        else {
            job.report(1., 5)?;
            return Ok(None);
        };
        let alloc = || "could not allocate precision output".to_string();
        let reach = halo(self.kind);
        // A Grayscale document's Sponge is the middle-gray rule, Vibrance
        // ignored (`retouch_brush`'s module doc).
        let pivot = crate::gray_filter::sponge_pivot(doc);
        // A Lab Color document's Sponge is the chroma rule about a* = b* = 0
        // (`lab_retouch`), Vibrance ignored as on Gray: no binding is built.
        let neutral = crate::lab_retouch::sponge_neutral(doc);
        let bound = match (self.vibrance, self.ink_pass, pivot, neutral) {
            (true, false, None, None) => Some(
                BoundVibrance::srgb(sponge_slider(self.kind)).prepare_precision(doc.profile())?,
            ),
            _ => None,
        };
        let vibrancy = match (neutral, pivot, &bound, self.vibrance) {
            (Some(n), _, _, _) => Vibrancy::Lab(n),
            (None, Some(m), _, _) => Vibrancy::Gray(m),
            (None, None, Some(bound), _) => Vibrancy::Bound(bound),
            (None, None, None, true) => Vibrancy::Encoded,
            (None, None, None, false) => Vibrancy::Off,
        };
        let coverage_of = |at: usize| -> f64 {
            let cx = (i64::from(rx) + (at % rw as usize) as i64 + i64::from(ox)) as usize;
            let cy = (i64::from(ry) + (at / rw as usize) as i64 + i64::from(oy)) as usize;
            coverage_at(input, n, self.selection, cy * canvas.0 as usize + cx)
        };
        let code = |v: f32| (v.clamp(0., 1.) * 65535.).round() as u16;
        let mut px = image.read_rect_rgba16(rx, ry, rw, rh).ok_or_else(alloc)?;
        let mut changed = false;
        if let Some(plate) = sampled {
            // The walk rect and its halo on the canvas, read from the picture
            // with reads clamped to that rect (`doc_retouch_brush`'s byte
            // twin is the master of this walk).
            let Some((sx, sy, sw, sh)) = canvas_reach((rx, ry, rw, rh), reach, (ox, oy), canvas)
            else {
                job.report(1., 5)?;
                return Ok(None);
            };
            let picture = plate_samples(plate, (sx, sy, sw, sh))?;
            let read = |x: i64, y: i64| -> [f32; 4] {
                let cx = (i64::from(rx) + x + i64::from(ox))
                    .clamp(i64::from(sx), i64::from(sx + sw) - 1)
                    - i64::from(sx);
                let cy = (i64::from(ry) + y + i64::from(oy))
                    .clamp(i64::from(sy), i64::from(sy + sh) - 1)
                    - i64::from(sy);
                picture[cy as usize * sw as usize + cx as usize]
            };
            let target = targets(self.kind, vibrancy, (rw, rh), read, job.range(0., 0.6))?;
            job.report(0.6, 3)?;
            let frozen = doc.lock_owner(self.layer, LOCK_TRANSPARENCY).is_some();
            for (at, slot) in px.iter_mut().enumerate() {
                job.every(at)?;
                let old = *slot;
                if frozen && old[3] == 0 {
                    continue;
                }
                let coverage = coverage_of(at);
                if coverage == 0. {
                    continue;
                }
                let amount = (coverage * f64::from(self.strength)) as f32;
                let (column, row) = ((at % rw as usize) as i64, (at / rw as usize) as i64);
                let t = target[at];
                let wanted = [t[0], t[1], t[2], read(column, row)[3]];
                let out = mix_over(unit(old), wanted, amount, frozen);
                let mut new = [code(out[0]), code(out[1]), code(out[2]), old[3]];
                if !frozen {
                    new[3] = code(out[3]);
                    if new[3] == 0 {
                        // Nothing visible is left: transparent black, or the
                        // pixel as it was when it already held nothing.
                        new = if old[3] == 0 { old } else { [0; 4] };
                    }
                }
                if new != old {
                    *slot = new;
                    changed = true;
                }
            }
        } else {
            // The rect plus the kind's halo, clipped to the layer: what the
            // targets read (`doc_retouch_brush`'s byte twin does the same).
            let (hx, hy) = (rx.saturating_sub(reach), ry.saturating_sub(reach));
            let hw = (rx + rw).saturating_add(reach).min(lw) - hx;
            let hh = (ry + rh).saturating_add(reach).min(lh) - hy;
            let source = image.read_rect_rgba16(hx, hy, hw, hh).ok_or_else(alloc)?;
            let read = |x: i64, y: i64| -> [f32; 4] {
                let lx = (i64::from(rx) + x).clamp(0, i64::from(lw) - 1) - i64::from(hx);
                let ly = (i64::from(ry) + y).clamp(0, i64::from(lh) - 1) - i64::from(hy);
                unit(source[ly as usize * hw as usize + lx as usize])
            };
            let target = targets(self.kind, vibrancy, (rw, rh), read, job.range(0., 0.6))?;
            job.report(0.6, 3)?;
            for (at, slot) in px.iter_mut().enumerate() {
                job.every(at)?;
                let old = *slot;
                if old[3] == 0 {
                    continue;
                }
                let coverage = coverage_of(at);
                if coverage == 0. {
                    continue;
                }
                let amount = (coverage * f64::from(self.strength)) as f32;
                let out = mix(unit(old), target[at], amount);
                let new = [code(out[0]), code(out[1]), code(out[2]), old[3]];
                if new != old {
                    *slot = new;
                    changed = true;
                }
            }
        }
        if !changed {
            job.report(1., 5)?;
            return Ok(None);
        }
        let Some(image) = image.with_rect_rgba16(rx, ry, rw, rh, &px)? else {
            job.report(1., 5)?;
            return Ok(None);
        };
        job.check()?;
        let next = doc.replacing_content(
            self.layer,
            Content::Raster {
                image,
                offset: (ox, oy),
            },
        )?;
        job.report(1., 5)?;
        Ok(next)
    }
}

impl PrecisionDocument {
    /// The History Brush's source layer: a u16 raster of a document with this
    /// one's canvas size.
    fn history_source<'a>(
        &self,
        source: &'a PrecisionDocument,
        source_layer: usize,
    ) -> Result<(&'a PrecisionImage, (i32, i32)), String> {
        if source.dimensions() != self.dimensions() {
            return Err("history brush source has a different canvas size".into());
        }
        let layer = source
            .layer(source_layer)
            .ok_or("history brush source layer is out of range")?;
        let image = layer
            .source()
            .ok_or("history brush source is not a raster layer")?;
        if source.is_float() || image.storage() != PrecisionStorage::Rgba16Unorm {
            return Err("native paint requires u16 storage".into());
        }
        Ok((image, layer.offset()))
    }

    /// The History Brush's plan for layer `layer` painting from layer
    /// `source_layer` of `source`: `retained_bytes`, `scratch_bytes`,
    /// `input_samples` (the canvas coverage plane, then the selection plane
    /// when `selection`). Refuses, before allocating, a target or source that
    /// is not a u16 raster and a source of another canvas size. A 32-bit
    /// float document plans its own arm (`float_retouch_brush`), whose source
    /// is a float document's f32 raster.
    pub(crate) fn history_brush_plan(
        &self,
        layer: usize,
        source: &PrecisionDocument,
        source_layer: usize,
        selection: bool,
    ) -> Result<Value, String> {
        // A Bitmap document's arm (#547 Bitmap phase 3, `bitmap_tools`).
        if self.is_bitmap() {
            return crate::bitmap_tools::history_brush_plan(
                self,
                layer,
                source,
                source_layer,
                selection,
            );
        }
        if self.is_indexed() {
            return crate::indexed_paint::history_brush_plan(
                self,
                layer,
                source,
                source_layer,
                selection,
            );
        }
        if self.is_cmyk() {
            return self.cmyk_history_brush_plan(layer, source, source_layer, selection);
        }
        if self.is_float() {
            return self.float_history_brush_plan(layer, source, source_layer, selection);
        }
        let (image, _) = paintable_u16(self, layer)?;
        self.history_source(source, source_layer)?;
        coverage_plan(self, image, HISTORY_BRUSH_BYTES_PER_PIXEL, selection)
    }

    /// Paints layer `layer` back toward layer `source_layer` of `source` (an
    /// earlier state of this document) where the stroke covers it: the
    /// premultiplied mix of all four channels (`doc_retouch_brush.rs` holds
    /// the master copy of that mix; this is its u16 twin) toward the source
    /// layer's pixel at the same CANVAS position (transparent where the source
    /// layer has none) by `coverage · opacity`. `samples` is the canvas-sized u16 stroke
    /// coverage, followed by the canvas-sized selection plane when there is
    /// one (length n or 2n). The layer does not grow; under a transparency
    /// lock each pixel keeps its alpha and alpha-0 pixels stay. `Ok(None)`
    /// when no stored code changes. A PIXELS edit under `precision_lock`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn history_brush(
        &self,
        layer: usize,
        source: &PrecisionDocument,
        source_layer: usize,
        opacity: f64,
        samples: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        // A Bitmap document's arm runs the Gray storage's stroke, which takes
        // the locks itself, then thresholds it (`bitmap_tools`).
        if self.is_bitmap() {
            return crate::bitmap_tools::history_brush(
                self,
                layer,
                source,
                source_layer,
                opacity,
                samples,
                limit,
                other,
                job,
            );
        }
        // An Indexed document's arm runs the RGB storage's stroke, which
        // takes the locks itself, then re-indexes (`indexed_paint`).
        if self.is_indexed() {
            return crate::indexed_paint::history_brush(
                self,
                layer,
                source,
                source_layer,
                opacity,
                samples,
                limit,
                other,
                job,
            );
        }
        self.under_locks(layer, EditKind::Pixels, |doc| {
            doc.history_brush_unlocked(
                layer,
                source,
                source_layer,
                opacity,
                samples,
                limit,
                other,
                job,
            )
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn history_brush_unlocked(
        &self,
        index: usize,
        source: &PrecisionDocument,
        source_layer: usize,
        opacity: f64,
        input: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        if self.is_cmyk() {
            return self.cmyk_history_brush_unlocked(
                index,
                source,
                source_layer,
                opacity,
                input,
                limit,
                other,
                job,
            );
        }
        if self.is_float() {
            return self.float_history_brush_unlocked(
                index,
                source,
                source_layer,
                opacity,
                input,
                limit,
                other,
                job,
            );
        }
        if !opacity.is_finite() || !(0. ..=1.).contains(&opacity) {
            return Err("history brush opacity is invalid".into());
        }
        let canvas = self.dimensions();
        let n = pixel_count(canvas.0, canvas.1)?;
        let selected = match input.len() {
            len if len == n => false,
            len if len == 2 * n => true,
            _ => return Err("native paint input length differs from plan".into()),
        };
        let plan = self.history_brush_plan(index, source, source_layer, selected)?;
        let scratch = plan["scratch_bytes"].as_u64().unwrap_or(u64::MAX);
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        if opacity == 0. {
            job.report(1., 5)?;
            return Ok(None);
        }
        let (image, (ox, oy)) = paintable_u16(self, index)?;
        let (from, (sox, soy)) = self.history_source(source, source_layer)?;
        let (lw, lh) = image.dimensions();
        let Some((rx, ry, rw, rh)) = coverage_rect16(&input[..n], canvas, (lw, lh), (ox, oy))
        else {
            job.report(1., 5)?;
            return Ok(None);
        };
        let alloc = || "could not allocate precision output".to_string();
        // The rect's canvas position in the source layer's own pixels, and the
        // part of it that layer holds, read once.
        let (sx, sy) = (
            i64::from(rx) + i64::from(ox) - i64::from(sox),
            i64::from(ry) + i64::from(oy) - i64::from(soy),
        );
        let (sw, sh) = from.dimensions();
        let (ix0, iy0) = (sx.max(0), sy.max(0));
        let (ix1, iy1) = (
            (sx + i64::from(rw)).min(i64::from(sw)),
            (sy + i64::from(rh)).min(i64::from(sh)),
        );
        let held = if ix0 < ix1 && iy0 < iy1 {
            from.read_rect_rgba16(
                ix0 as u32,
                iy0 as u32,
                (ix1 - ix0) as u32,
                (iy1 - iy0) as u32,
            )
            .ok_or_else(alloc)?
        } else {
            Vec::new()
        };
        let frozen = self.lock_owner(index, LOCK_TRANSPARENCY).is_some();
        let mut px = image.read_rect_rgba16(rx, ry, rw, rh).ok_or_else(alloc)?;
        let mut changed = false;
        let area = px.len();
        for (at, slot) in px.iter_mut().enumerate() {
            job.every(at)?;
            if at & 65535 == 0 {
                job.report(0.8 * at as f64 / area as f64, 3)?;
            }
            let (column, row) = ((at % rw as usize) as i64, (at / rw as usize) as i64);
            let cx = (i64::from(rx) + column + i64::from(ox)) as usize;
            let cy = (i64::from(ry) + row + i64::from(oy)) as usize;
            let coverage = coverage_at(input, n, selected, cy * canvas.0 as usize + cx);
            let old = *slot;
            if coverage == 0. || (frozen && old[3] == 0) {
                continue;
            }
            let (source_x, source_y) = (sx + column, sy + row);
            let wanted = if (ix0..ix1).contains(&source_x) && (iy0..iy1).contains(&source_y) {
                held[(source_y - iy0) as usize * (ix1 - ix0) as usize + (source_x - ix0) as usize]
            } else {
                [0; 4]
            };
            let t = coverage * opacity;
            let (a, b) = (
                old.map(|v| f64::from(v) / 65535.),
                wanted.map(|v| f64::from(v) / 65535.),
            );
            let code = |v: f64| (v.clamp(0., 1.) * 65535.).round() as u16;
            // Premultiplied mix: a transparent side contributes no colour.
            let alpha = (1. - t) * a[3] + t * b[3];
            let mut new = old;
            if alpha > 0. {
                for c in 0..3 {
                    new[c] = code(((1. - t) * a[c] * a[3] + t * b[c] * b[3]) / alpha);
                }
            }
            if !frozen {
                new[3] = code(alpha);
                if new[3] == 0 {
                    // Nothing visible is left: transparent black, or the pixel
                    // as it was when it already held nothing.
                    new = if old[3] == 0 { old } else { [0; 4] };
                }
            }
            if new != old {
                *slot = new;
                changed = true;
            }
        }
        if !changed {
            job.report(1., 5)?;
            return Ok(None);
        }
        let Some(image) = image.with_rect_rgba16(rx, ry, rw, rh, &px)? else {
            job.report(1., 5)?;
            return Ok(None);
        };
        job.check()?;
        let next = self.replacing_content(
            index,
            Content::Raster {
                image,
                offset: (ox, oy),
            },
        )?;
        job.report(1., 5)?;
        Ok(next)
    }
}
