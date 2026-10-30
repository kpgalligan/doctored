//! The byte adapter of the Smudge brush (`retouch_smudge`, the kernel) on the
//! layered document.
//!
//! Unlike the overlay brushes, Smudge takes the stroke as its DABS — the
//! `dr_brush_u16` request and packed x / y / diameter / opacity tuples in
//! canvas coordinates — because the result depends on the order and direction
//! of the dabs, which an accumulated coverage overlay no longer holds.
use crate::brush_coverage::{footprint, BrushRequest};
use crate::doc::DrDocument;
use crate::doc_lock::EditKind;
use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;
use crate::retouch_brush_sampling::clip_footprint;
use crate::retouch_smudge::{smudge, smudge_work};

/// A straight byte pixel as a premultiplied unit sample.
fn premultiplied(p: [u8; 4]) -> [f32; 4] {
    let a = f32::from(p[3]) / 255.0;
    [
        f32::from(p[0]) / 255.0 * a,
        f32::from(p[1]) / 255.0 * a,
        f32::from(p[2]) / 255.0 * a,
        a,
    ]
}

/// Round half-up back to a byte, as `dodge_burn_layer` does.
fn byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5).floor() as u8
}

impl DrDocument {
    /// Smudges layer `idx` along a stroke: `request` and `dabs` are
    /// `dr_brush_u16`'s (the dabs in CANVAS coordinates), `strength` is clamped
    /// to [0, 1], and `selection` is canvas-sized u8 coverage confining the
    /// stroke, or `None`. The walk is `retouch_smudge::smudge` over the
    /// stroke's footprint on the layer, in premultiplied samples, quantized
    /// once at the end. The layer keeps its extent (the smear is clipped at its
    /// edges), and under a transparency lock every pixel keeps its alpha.
    ///
    /// `Ok(None)` (a domain refusal or nothing to do): `idx` out of range or a
    /// group, non-finite `strength`, a lock that forbids a PIXELS edit, a
    /// stroke that misses the layer, or no byte changing (fewer than two dabs,
    /// strength 0, a flat area) — an identical copy would register a phantom
    /// undo step. `Err`: the brush request's own refusals (`BrushRequest::
    /// plan`'s dab and work sentences), the sub-stepped walk past
    /// `MAX_SMUDGE_WORK` in Smudge's own sentence
    /// (`retouch_smudge::smudge_work`), a `selection` that is not
    /// canvas-sized, or a failed allocation.
    ///
    /// `sample_all_layers` smears the sampled picture
    /// (`retouch_brush_sampling::SAMPLE_ALL_LAYERS`) into the layer: the
    /// footprint is first clipped to the layer ∩ canvas (empty: `Ok(None)`),
    /// so the layer's pixels outside the canvas are never read or written; the
    /// field starts as the picture there (premultiplied); a sample the walk
    /// left at its starting value keeps the layer's bytes, and a moved sample
    /// is written straight with its own alpha (under a transparency lock the
    /// layer's alpha is kept and alpha-0 pixels stay).
    pub fn smudge_layer(
        &self,
        idx: usize,
        request: &BrushRequest,
        dabs: &[f64],
        strength: f32,
        selection: Option<&[u8]>,
        sample_all_layers: bool,
    ) -> Result<Option<Self>, String> {
        self.under_locks_fallible(idx, EditKind::Pixels, |doc| {
            doc.smudge_layer_unlocked(idx, request, dabs, strength, selection, sample_all_layers)
        })
    }

    /// The body of [`Self::smudge_layer`], outside the lock gate.
    fn smudge_layer_unlocked(
        &self,
        idx: usize,
        request: &BrushRequest,
        dabs: &[f64],
        strength: f32,
        selection: Option<&[u8]>,
        sample_all_layers: bool,
    ) -> Result<Option<Self>, String> {
        let Some(layer) = self.raster_layer(idx) else {
            return Ok(None);
        };
        if !strength.is_finite() {
            return Ok(None);
        }
        let canvas = (self.width as usize, self.height as usize);
        if selection.is_some_and(|s| s.len() != canvas.0 * canvas.1) {
            return Err("smudge selection length differs from the canvas".into());
        }
        // The dabs in the layer's own pixels; a ragged list keeps its length
        // so `plan` refuses it by name.
        let offset = [f64::from(layer.offset.0), f64::from(layer.offset.1)];
        let mut local: Vec<f64> = reserved_vec(dabs.len())?;
        for (i, value) in dabs.iter().enumerate() {
            local.push(match i % 4 {
                0 => value - offset[0],
                1 => value - offset[1],
                _ => *value,
            });
        }
        let dimensions = layer.pixels.dimensions();
        smudge_work(request, dimensions, &local)?;
        let strength = strength.clamp(0.0, 1.0);
        let Some(rect) = footprint(dimensions, &local) else {
            return Ok(None);
        };
        let rect = if sample_all_layers {
            let Some(rect) = clip_footprint(rect, layer.offset, (self.width, self.height)) else {
                return Ok(None);
            };
            rect
        } else {
            rect
        };
        if strength == 0.0 {
            return Ok(None);
        }
        let (rx, ry) = (rect[0] as u32, rect[1] as u32);
        let (rw, rh) = ((rect[2] - rect[0]) as u32, (rect[3] - rect[1]) as u32);
        let Some(mut px) = layer.pixels.read_rect(rx, ry, rw, rh) else {
            return Err("could not allocate the smudge field".into());
        };
        let (ox, oy) = (i64::from(layer.offset.0), i64::from(layer.offset.1));
        // The field's starting samples: the layer's own, or the sampled
        // picture over the clipped footprint (whose canvas rect it is).
        let start: Option<Vec<[f32; 4]>> = if sample_all_layers {
            let canvas_rect = (
                (i64::from(rx) + ox) as u32,
                (i64::from(ry) + oy) as u32,
                rw,
                rh,
            );
            let picture = self.sampled_region(idx, canvas_rect)?;
            let mut start = reserved_vec(px.len())?;
            start.extend(picture.pixels().map(|p| premultiplied(p.0)));
            Some(start)
        } else {
            None
        };
        let mut field: Vec<[f32; 4]> = reserved_vec(px.len())?;
        match &start {
            Some(start) => field.extend_from_slice(start),
            None => field.extend(px.iter().map(|p| premultiplied(*p))),
        }
        let weight = |x: usize, y: usize| -> f64 {
            let Some(selection) = selection else {
                return 1.0;
            };
            let (cx, cy) = (x as i64 + ox, y as i64 + oy);
            if cx < 0 || cy < 0 || cx >= canvas.0 as i64 || cy >= canvas.1 as i64 {
                return 0.0;
            }
            f64::from(selection[cy as usize * canvas.0 + cx as usize]) / 255.0
        };
        smudge(
            &mut field,
            rect,
            request,
            &local,
            f64::from(strength),
            weight,
            JobContext::NONE,
        )?;
        let frozen = self.alpha_frozen(idx);
        let mut changed = false;
        for (at, (old, sample)) in px.iter_mut().zip(&field).enumerate() {
            let first = start
                .as_ref()
                .map_or_else(|| premultiplied(*old), |s| s[at]);
            // A sample the walk left alone keeps its stored bytes exactly.
            if first == *sample || (frozen && old[3] == 0) {
                continue;
            }
            let alpha = if frozen { old[3] } else { byte(sample[3]) };
            let new = if alpha == 0 && old[3] == 0 {
                // Still fully transparent: the pixel's latent colour stays, so
                // a smear that deposits nothing there changes no byte.
                *old
            } else if alpha == 0 {
                [0; 4]
            } else if sample[3] > 0.0 {
                [
                    byte(sample[0] / sample[3]),
                    byte(sample[1] / sample[3]),
                    byte(sample[2] / sample[3]),
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
            return Ok(None);
        }
        let Some(pixels) = layer.pixels.with_rect(rx, ry, rw, rh, &px)? else {
            return Ok(None);
        };
        Ok(self.with_layer_raster(idx, pixels))
    }
}
