//! The Art History Brush on the 8-bit u16-record modes: Grayscale
//! (#1 phase 2e, [`GRAY_ART`]) and Lab Color (#1 Lab 2f, third part,
//! `lab_retouch::LAB_ART`). One walk, the kernel (`art_history_stroke`) over
//! RGBA16 records, modelled on the CMYK arm (`cmyk_art_history`) and painting
//! as the byte master does (`doc_art_history`); a [`U16Art`] names what a
//! mode changes: its two refusals, which source state it paints from, its
//! blend-mode rule and the luma the strokes head along.
//!
//! The record's part within `reach` of the dabs and the source state's
//! raster over the same canvas rect are read once as straight unit samples
//! (`read_rect_rgba16`); the strokes head along the mode's luma
//! ([`U16Art::luma`]: on Gray `doc_art_history::rgb_luma`, which reads the
//! gray value itself on R = G = B; on Lab L\* times alpha), are painted into
//! the f32 copy of the part through `blend::composite_source_into` (or
//! `composite_source_frozen` under a transparency lock), and the part is
//! written back once, each sample `round(clamp(v) · 65535)`, under the byte
//! master's rules: a frozen alpha is kept, and a sample still fully
//! transparent keeps its latent colour.
//! On Gray the tool's modes are Normal, Darken and Lighten
//! (`gray_document::blend_refusal` refuses the others), all separable, so
//! the source's R = G = B and the record's stay
//! equal through every composite; on Lab the composite is per channel on the
//! L\*a\*b\* codes (`lab_retouch`'s module doc). The record never grows. A
//! 16-bit Gray or Lab document refuses as a 16-bit RGB one does
//! (`precision_art_history`).
//!
//! # Memory
//!
//! The plan is the u16 History Brush's plan for the same layer, source and
//! selection (`history_brush_plan`), plus the source part as unit f32
//! (16 B a source pixel, the CMYK arm's term), plus the dab and field terms
//! every Art History plan adds (`precision_art_history::art_stroke_terms`).
use serde_json::{json, Value};

use crate::art_history_stroke::{generate, paint, ArtHistoryRequest};
use crate::blend::{
    blend_kind, composite_source_frozen, composite_source_into, BlendKind, BlendMode,
};
use crate::brush_coverage::BrushRequest;
use crate::doc_art_history::{reach_rect, rgb_luma, Pair, Part};
use crate::doc_lock::LOCK_TRANSPARENCY;
use crate::job_control::JobContext;
use crate::layer_rect::{canvas_rect_on_plane, plane_rect_offset};
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{admit, pixel_count, reserved_vec, PrecisionImage, PrecisionStorage};

/// A target that exists but is not a Gray (u16) raster.
pub(crate) const GRAY_ART_TARGET_REFUSAL: &str =
    "The Art History Brush paints only a Grayscale raster layer";
/// A source state the Art History Brush cannot paint a Gray document from.
pub(crate) const GRAY_ART_SOURCE_REFUSAL: &str =
    "The Art History Brush's source state is not a Grayscale raster";

/// What one u16-record mode changes in the shared walk (module doc).
pub(crate) struct U16Art {
    /// A target that exists but is not a u16 raster.
    pub(crate) target_refusal: &'static str,
    /// A source state that is not this mode's raster.
    pub(crate) source_refusal: &'static str,
    /// True for a source state of this mode (`is_gray`, `is_lab`).
    pub(crate) source_mode: fn(&PrecisionDocument) -> bool,
    /// The mode's refusal of a blend mode, `None` to paint through it.
    pub(crate) blend_refusal: fn(BlendMode) -> Option<String>,
    /// The luma the strokes head along, of a straight unit sample.
    pub(crate) luma: fn([f32; 4]) -> f64,
    /// The mode's blend behaviour (`blend::blend_kind` on Gray, the 8-bit
    /// Lab arms on Lab: `lab_blend::lab_blend_kind8`).
    pub(crate) kind: fn(BlendMode) -> BlendKind,
}

/// Gray's blend rule at 8 bits (`gray_document::blend_refusal`).
fn gray_blend_refusal(mode: BlendMode) -> Option<String> {
    crate::gray_document::blend_refusal(mode, false)
}

/// The 8-bit Grayscale document's arm (#1 phase 2e).
pub(crate) const GRAY_ART: U16Art = U16Art {
    target_refusal: GRAY_ART_TARGET_REFUSAL,
    source_refusal: GRAY_ART_SOURCE_REFUSAL,
    source_mode: PrecisionDocument::is_gray,
    blend_refusal: gray_blend_refusal,
    luma: rgb_luma,
    kind: blend_kind,
};

/// A record's part as read: its plane rect, its stored samples and the
/// samples as unit values on the canvas.
type ReadPart = ((u32, u32, u32, u32), Vec<[u16; 4]>, Part<4>);

/// `image`'s part over canvas rect `reach` as unit samples, with the codes
/// it was read from; `None` when the rect misses the record.
fn read_part(
    image: &PrecisionImage,
    offset: (i32, i32),
    canvas: (u32, u32),
    reach: (i64, i64, i64, i64),
    job: JobContext<'_>,
) -> Result<Option<ReadPart>, String> {
    let Some(part) = canvas_rect_on_plane(canvas, image.dimensions(), offset, reach) else {
        return Ok(None);
    };
    let raw = image
        .read_rect_rgba16(part.0, part.1, part.2, part.3)
        .ok_or("could not read the art history brush rect")?;
    job.check()?;
    let (cx, cy) = plane_rect_offset(offset, part);
    let mut samples = reserved_vec(raw.len())?;
    samples.extend(raw.iter().map(|p| p.map(|v| f32::from(v) / 65535.)));
    let rect = (i64::from(cx), i64::from(cy), part.2, part.3);
    Ok(Some((part, raw, Part { rect, samples })))
}

/// One unit sample as its u16 code.
fn code(v: f32) -> u16 {
    (v.clamp(0., 1.) * 65535.).round() as u16
}

impl PrecisionDocument {
    /// The Art History Brush's plan on an 8-bit Gray or Lab document under
    /// `art` (module doc, "Memory"). Refuses an existing target that is not
    /// a u16 raster (`art.target_refusal`, judged before the source), a
    /// source state that is not the mode's raster (`art.source_refusal`), a
    /// mode `art.blend_refusal` refuses, otherwise as the History Brush
    /// refuses.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn u16_art_history_plan(
        &self,
        art: &U16Art,
        layer: usize,
        source: &PrecisionDocument,
        source_layer: usize,
        request: &ArtHistoryRequest,
        brush: &BrushRequest,
        dabs: &[f64],
        selection: bool,
    ) -> Result<Value, String> {
        brush.plan(self.dimensions(), dabs)?;
        let not_u16 = |record: &crate::precision_document::PrecisionDocumentLayer| {
            record
                .source()
                .is_none_or(|image| image.storage() != PrecisionStorage::Rgba16Unorm)
        };
        if self.layer(layer).is_some_and(not_u16) {
            return Err(art.target_refusal.into());
        }
        if !(art.source_mode)(source) || source.layer(source_layer).is_some_and(not_u16) {
            return Err(art.source_refusal.into());
        }
        if let Some(refusal) =
            BlendMode::from_c(request.blend_mode as i32).and_then(art.blend_refusal)
        {
            return Err(refusal);
        }
        let mut plan = self.history_brush_plan(layer, source, source_layer, selection)?;
        let (sw, sh) = source
            .layer(source_layer)
            .and_then(|record| record.source())
            .map(PrecisionImage::dimensions)
            .ok_or(art.source_refusal)?;
        let source_part = (pixel_count(sw, sh)? as u64).saturating_mul(16);
        let scratch = plan["scratch_bytes"]
            .as_u64()
            .unwrap_or(u64::MAX)
            .checked_add(source_part)
            .ok_or("art history brush scratch overflow")?;
        plan["scratch_bytes"] = json!(scratch);
        self.art_stroke_terms(plan, request, dabs, selection)
    }

    /// The Art History Brush on Gray or Lab record `index` under `art`,
    /// inside the lock gate (`PrecisionDocument::art_history_brush` holds
    /// it; module doc). `Ok(None)` when no stored code changes.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn u16_art_history_unlocked(
        &self,
        art: &U16Art,
        index: usize,
        source: &PrecisionDocument,
        source_layer: usize,
        request: &ArtHistoryRequest,
        brush: &BrushRequest,
        dabs: &[f64],
        selection: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let canvas = self.dimensions();
        let n = pixel_count(canvas.0, canvas.1)?;
        let selected = match selection.len() {
            0 => false,
            len if len == n => true,
            _ => return Err("native paint input length differs from plan".into()),
        };
        let plan = self.u16_art_history_plan(
            art,
            index,
            source,
            source_layer,
            request,
            brush,
            dabs,
            selected,
        )?;
        let scratch = plan["scratch_bytes"].as_u64().unwrap_or(u64::MAX);
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let job = job.with_reservation_floor(scratch);
        // The plan has refused a target or source that is not the mode's raster.
        let record = self
            .layer(index)
            .ok_or("art history brush target is out of range")?;
        let image = record.source().ok_or(art.target_refusal)?;
        let offset = record.offset();
        let from = source
            .layer(source_layer)
            .ok_or("history brush source layer is out of range")?;
        let from_image = from.source().ok_or(art.source_refusal)?;
        let Some(reach) = reach_rect(request, dabs, canvas) else {
            job.report(1., 5)?;
            return Ok(None);
        };
        let Some((part, old, current)) = read_part(image, offset, canvas, reach, job)? else {
            job.report(1., 5)?;
            return Ok(None);
        };
        let source_part = read_part(from_image, from.offset(), canvas, reach, job)?.map_or_else(
            || Part {
                rect: (0, 0, 0, 0),
                samples: Vec::new(),
            },
            |(_, _, p)| p,
        );
        let samples = Pair {
            source: source_part,
            current,
            luma: art.luma,
        };
        let strokes = generate(request, dabs, &samples)?;
        job.report(0.1, 3)?;
        let Some(mode) = BlendMode::from_c(request.blend_mode as i32) else {
            return Err(crate::art_history_stroke::BLEND_MODE_REFUSAL.into());
        };
        if strokes.is_empty() {
            job.report(1., 5)?;
            return Ok(None);
        }
        let frozen = self.lock_owner(index, LOCK_TRANSPARENCY).is_some();
        let rect = samples.current.rect;
        let mut acc = samples.current.samples;
        let gate = |x: i64, y: i64| {
            if selected {
                (f64::from(selection[y as usize * canvas.0 as usize + x as usize]) / 65535.) as f32
            } else {
                1.
            }
        };
        let composite = |px: &mut [f32; 4], colour: [f32; 4], alpha: f32, xy: (i64, i64)| {
            let cs = [colour[0], colour[1], colour[2]];
            if frozen {
                composite_source_frozen(px, cs, alpha, (art.kind)(mode), xy);
            } else {
                composite_source_into(std::slice::from_mut(px), 0, cs, alpha, (art.kind)(mode), xy);
            }
        };
        if !paint(
            &strokes,
            brush,
            rect,
            &mut acc,
            gate,
            composite,
            job.range(0.1, 0.9),
        )? {
            job.report(1., 5)?;
            return Ok(None);
        }
        let mut out = old;
        let mut changed = false;
        for (slot, sample) in out.iter_mut().zip(&acc) {
            let mut new = sample.map(code);
            if frozen {
                new[3] = slot[3];
            }
            if new[3] == 0 {
                // Still fully transparent: the latent colour stays.
                continue;
            }
            if new != *slot {
                *slot = new;
                changed = true;
            }
        }
        if !changed {
            job.report(1., 5)?;
            return Ok(None);
        }
        let Some(image) = image.with_rect_rgba16(part.0, part.1, part.2, part.3, &out)? else {
            job.report(1., 5)?;
            return Ok(None);
        };
        drop(out);
        job.check()?;
        let next = self.replacing_content(index, Content::Raster { image, offset })?;
        job.report(1., 5)?;
        Ok(next)
    }
}
