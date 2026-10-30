//! The 16-bit adapter of the Smudge brush (`retouch_smudge`, the kernel the
//! byte `doc_smudge` shares) on the typed document.
//!
//! The stroke arrives as its DABS (`dr_brush_u16`'s request and packed tuples
//! in canvas coordinates), because the result depends on their order and
//! direction. The walk covers the stroke's footprint on the layer in
//! premultiplied f32, is quantized to u16 once at the end and written back
//! through `with_rect_rgba16`. A target that is not a u16 raster and a
//! retained source refuse in `NativePaintEdit::extent`'s sentences
//! (`precision_retouch_brush::paintable_u16`).
//!
//! A 32-bit float document leaves `smudge_plan` and `smudge_unlocked`
//! at their top for `float_retouch_brush`, the f32 arm: the same walk over
//! the same kernel, with a field that is written back unquantized. The lock
//! gate stays here, and this module is the MASTER of every step but the
//! sample type.
use crate::brush_coverage::{footprint, BrushRequest};
use crate::doc_lock::{EditKind, LOCK_TRANSPARENCY};
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{admit, pixel_count, reserved_vec, PrecisionImage, BOOKKEEPING_BYTES};
use crate::precision_retouch_brush::paintable_u16;
use crate::retouch_brush_sampling::{clip_footprint, sampled_field};
use crate::retouch_smudge::{smudge, smudge_stage_pixels, smudge_work};
use serde_json::{json, Value};

/// Smudge's per-pixel plan term, over the stroke's FOOTPRINT on the layer
/// (known from the dabs before anything is allocated): the premultiplied f32
/// field (16 B) and the u16 rect read and rewritten (8 B). The stage (16 B for
/// each pixel of the largest sub-step box) and the layer-space dabs are
/// charged beside it.
///
/// Record (`precision_retouch_brush`'s `RETOUCH_BRUSH_BYTES_PER_PIXEL`
/// harness and document, one size-200 hard `smudge_stroke` from (1000,1000) to
/// (1800,1800) at strength 50): E 576,291,224 / 576,307,608 / 576,340,376
/// against `smudge_plan` 800,044,792 (1.388×, the plan by arithmetic). That
/// peak is the publication's: an invert on the same document reaches
/// 577,061,272.
///
/// Sample All Layers (`scripts/cmyk-admission/measure.py`
/// `smudge-sal-rgb16`, the Release app, a size-20 `smudge_stroke` along
/// (100, 100)–(5900, 3900), a 5820 × 3820 footprint, with
/// `sample_all_layers`): the plan, this term plus the sampled picture's, is
/// 2,082,706,960 against E 1,100,907,312 / 1,100,792,624 / 1,101,579,056
/// (1.891×).
const SMUDGE_BYTES_PER_PIXEL: u64 = 24;

/// `dabs` (canvas coordinates) in the pixels of a layer at `offset`. A ragged
/// list keeps its length, so `BrushRequest::plan` refuses it by name.
pub(crate) fn layer_dabs(dabs: &[f64], offset: (i32, i32)) -> Result<Vec<f64>, String> {
    let mut local = reserved_vec(dabs.len())?;
    for (i, value) in dabs.iter().enumerate() {
        local.push(match i % 4 {
            0 => value - f64::from(offset.0),
            1 => value - f64::from(offset.1),
            _ => *value,
        });
    }
    Ok(local)
}

fn premultiplied(p: [u16; 4]) -> [f32; 4] {
    let a = f32::from(p[3]) / 65535.0;
    [
        f32::from(p[0]) / 65535.0 * a,
        f32::from(p[1]) / 65535.0 * a,
        f32::from(p[2]) / 65535.0 * a,
        a,
    ]
}

impl PrecisionDocument {
    /// Smudge's plan for layer `layer` along `dabs` (`dr_brush_u16`'s tuples,
    /// canvas coordinates): `retained_bytes`, `scratch_bytes` and
    /// `input_samples` (the canvas-sized selection plane when `selection`, else
    /// 0). Refuses, before allocating, a target that is not a paintable u16
    /// raster, the brush request's own dab and work refusals, and a sub-stepped
    /// walk past `MAX_SMUDGE_WORK` (`retouch_smudge::smudge_work`, its own
    /// sentence). `sample_all_layers` adds the sampled picture's plan
    /// (`sampled_plate_plan`); the walk keeps the unclipped footprint, an
    /// upper bound on the canvas-clipped one it walks.
    pub(crate) fn smudge_plan(
        &self,
        layer: usize,
        request: &BrushRequest,
        dabs: &[f64],
        selection: bool,
        sample_all_layers: bool,
    ) -> Result<Value, String> {
        if self.is_cmyk() {
            return self.cmyk_smudge_plan(layer, request, dabs, selection, sample_all_layers);
        }
        let mut plan = if self.is_float() {
            self.float_smudge_plan(layer, request, dabs, selection)?
        } else {
            self.smudge_walk_plan(layer, request, dabs, selection)?
        };
        if sample_all_layers {
            let walk = plan["scratch_bytes"].as_u64().unwrap_or(u64::MAX);
            let scratch = walk
                .checked_add(self.sampled_plate_plan(layer)?)
                .ok_or("smudge scratch overflow")?;
            plan["scratch_bytes"] = json!(scratch);
        }
        Ok(plan)
    }

    /// The 16-bit walk's plan alone (`smudge_plan` without the picture).
    fn smudge_walk_plan(
        &self,
        layer: usize,
        request: &BrushRequest,
        dabs: &[f64],
        selection: bool,
    ) -> Result<Value, String> {
        let (image, offset) = paintable_u16(self, layer)?;
        let (width, height) = self.dimensions();
        let n = pixel_count(width, height)? as u64;
        let dimensions = image.dimensions();
        let output = pixel_count(dimensions.0, dimensions.1)? as u64;
        let local = layer_dabs(dabs, offset)?;
        smudge_work(request, dimensions, &local)?;
        let field = footprint(dimensions, &local)
            .map_or(0, |f| (f[2] - f[0]) as u64 * (f[3] - f[1]) as u64);
        let scratch = 8 * BOOKKEEPING_BYTES
            + n * 24
            + output * 8
            + field * SMUDGE_BYTES_PER_PIXEL
            + smudge_stage_pixels(field, &local) * 16
            + dabs.len() as u64 * 8;
        Ok(json!({
            "retained_bytes": self.retained_bytes(),
            "scratch_bytes": scratch,
            "input_samples": if selection { n } else { 0 },
        }))
    }

    /// Smudges layer `layer` along the stroke at `strength` in [0, 1],
    /// confined by `selection` — canvas-sized u16 coverage, or empty for none.
    /// The layer keeps its extent, and under a transparency lock every pixel
    /// keeps its alpha. `Ok(None)` when no stored code changes (fewer than two
    /// dabs, strength 0, a stroke off the layer, a flat area). A PIXELS edit
    /// under `precision_lock`.
    ///
    /// `sample_all_layers` smears the sampled picture into the layer,
    /// by `doc_smudge`'s rule (its master): the exported plan is admitted
    /// whole, the picture (`sampled_plate`) admits and reserves its own term,
    /// then the walk's term is admitted against `other` plus the picture's
    /// bytes, so the picture is charged once.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn smudge(
        &self,
        layer: usize,
        request: &BrushRequest,
        dabs: &[f64],
        strength: f64,
        selection: &[u16],
        sample_all_layers: bool,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        self.under_locks(layer, EditKind::Pixels, |doc| {
            if !sample_all_layers || doc.is_cmyk() {
                return doc.smudge_unlocked(
                    layer,
                    request,
                    dabs,
                    strength,
                    selection,
                    (sample_all_layers, None),
                    limit,
                    other,
                    job,
                );
            }
            if !strength.is_finite() || !(0. ..=1.).contains(&strength) {
                return Err("smudge strength is invalid".into());
            }
            let plan = doc.smudge_plan(layer, request, dabs, !selection.is_empty(), true)?;
            let scratch = plan["scratch_bytes"].as_u64().unwrap_or(u64::MAX);
            admit(doc.retained_bytes(), scratch, other, limit)?;
            let sampled = doc.sampled_plate(layer, limit, other, job.range(0., 0.3))?;
            let held = sampled.retained_bytes();
            doc.smudge_unlocked(
                layer,
                request,
                dabs,
                strength,
                selection,
                (true, Some(&sampled)),
                limit,
                other.saturating_add(held),
                job.with_held_bytes(held).range(0.3, 1.),
            )
        })
    }

    /// The lock gate, then the walk smearing `sampled`, a canvas-sized
    /// picture at the origin (a CMYK record's pass of it).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn smudge_sampled(
        &self,
        layer: usize,
        request: &BrushRequest,
        dabs: &[f64],
        strength: f64,
        selection: &[u16],
        sampled: &PrecisionImage,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        self.under_locks(layer, EditKind::Pixels, |doc| {
            doc.smudge_unlocked(
                layer,
                request,
                dabs,
                strength,
                selection,
                (true, Some(sampled)),
                limit,
                other,
                job,
            )
        })
    }

    /// `sampling` is Sample All Layers and, once rendered, its picture
    /// (`None` on CMYK, which samples per pass).
    #[allow(clippy::too_many_arguments)]
    fn smudge_unlocked(
        &self,
        index: usize,
        request: &BrushRequest,
        dabs: &[f64],
        strength: f64,
        selection: &[u16],
        (sample_all_layers, sampled): (bool, Option<&PrecisionImage>),
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        if self.is_cmyk() {
            return self.cmyk_smudge_unlocked(
                index,
                request,
                dabs,
                strength,
                selection,
                sample_all_layers,
                limit,
                other,
                job,
            );
        }
        if self.is_float() {
            return self.float_smudge_unlocked(
                index, request, dabs, strength, selection, sampled, limit, other, job,
            );
        }
        if !strength.is_finite() || !(0. ..=1.).contains(&strength) {
            return Err("smudge strength is invalid".into());
        }
        let plan = self.smudge_plan(index, request, dabs, !selection.is_empty(), false)?;
        if selection.len() as u64 != plan["input_samples"].as_u64().unwrap_or(u64::MAX) {
            return Err("smudge selection length differs from the canvas".into());
        }
        let scratch = plan["scratch_bytes"].as_u64().unwrap_or(u64::MAX);
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let (image, (ox, oy)) = paintable_u16(self, index)?;
        let dimensions = image.dimensions();
        let local = layer_dabs(dabs, (ox, oy))?;
        let canvas = self.dimensions();
        // Under Sample All Layers the footprint is clipped to the layer ∩
        // canvas, so no pixel outside the canvas is read or written.
        let rect = footprint(dimensions, &local).and_then(|f| match sampled {
            Some(_) => clip_footprint(f, (ox, oy), canvas),
            None => Some(f),
        });
        let Some(rect) = rect else {
            job.report(1., 5)?;
            return Ok(None);
        };
        if strength == 0. {
            job.report(1., 5)?;
            return Ok(None);
        }
        let (rx, ry) = (rect[0] as u32, rect[1] as u32);
        let (rw, rh) = ((rect[2] - rect[0]) as u32, (rect[3] - rect[1]) as u32);
        let mut px = image
            .read_rect_rgba16(rx, ry, rw, rh)
            .ok_or("could not allocate precision output")?;
        let start = sampled
            .map(|plate| sampled_field(plate, rect, (ox, oy)))
            .transpose()?;
        let mut field: Vec<[f32; 4]> = reserved_vec(px.len())?;
        match &start {
            Some(start) => field.extend_from_slice(start),
            None => field.extend(px.iter().map(|p| premultiplied(*p))),
        }
        let weight = |x: usize, y: usize| -> f64 {
            if selection.is_empty() {
                return 1.0;
            }
            let (cx, cy) = (x as i64 + i64::from(ox), y as i64 + i64::from(oy));
            if cx < 0 || cy < 0 || cx >= i64::from(canvas.0) || cy >= i64::from(canvas.1) {
                return 0.0;
            }
            f64::from(selection[cy as usize * canvas.0 as usize + cx as usize]) / 65535.0
        };
        smudge(&mut field, rect, request, &local, strength, weight, job)?;
        let frozen = self.lock_owner(index, LOCK_TRANSPARENCY).is_some();
        let code = |v: f32| (f64::from(v).clamp(0., 1.) * 65535.).round() as u16;
        let mut changed = false;
        for (at, (old, sample)) in px.iter_mut().zip(&field).enumerate() {
            job.every(at)?;
            let first = start
                .as_ref()
                .map_or_else(|| premultiplied(*old), |s| s[at]);
            // A sample the walk left alone keeps its stored codes exactly.
            if first == *sample || (frozen && old[3] == 0) {
                continue;
            }
            // The quantize-back rule is `doc_smudge.rs`'s (the master copy),
            // at u16 codes.
            let alpha = if frozen { old[3] } else { code(sample[3]) };
            let new = if alpha == 0 && old[3] == 0 {
                *old
            } else if alpha == 0 {
                [0; 4]
            } else if sample[3] > 0.0 {
                [
                    code(sample[0] / sample[3]),
                    code(sample[1] / sample[3]),
                    code(sample[2] / sample[3]),
                    alpha,
                ]
            } else {
                // Frozen alpha over a sample smeared to nothing: the colour
                // has no source, so it stays.
                *old
            };
            if new != *old {
                *old = new;
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
