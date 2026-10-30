//! The Art History Brush on an 8-bit CMYK document's ink: the
//! kernel (`art_history_stroke`) over CMYKA16 records.
//!
//! The record's part within `reach` of the dabs and the source state's CMYK
//! raster over the same canvas rect are read once as straight unit ink
//! (`[c, m, y, k, alpha]`); the strokes are generated against them (their
//! colour is the source's ink, the tolerance compares premultiplied ink,
//! the heading reads the ink's luminance, `ink_luma`),
//! then painted into an f32 copy of the part through
//! `cmyk_composite::over_mode` (or `over_mode_frozen` under a transparency
//! lock: the blend modes on `1 − ink`), and written back as
//! `CmykPaintEdit`'s walk writes its samples (`cmyk_composite::codes`, the
//! part through `CmykImage::with_rect`). The record never grows.
use crate::art_history_stroke::{generate, paint, ArtHistoryRequest};
use crate::blend::{lum, BlendMode};
use crate::brush_coverage::BrushRequest;
use crate::cmyk_composite::{codes, over_mode, over_mode_frozen, unit, InkPixel};
use crate::cmyk_image::{CmykImage, CmykSample};
use crate::doc_art_history::{reach_rect, Pair, Part};
use crate::doc_lock::LOCK_TRANSPARENCY;
use crate::job_control::JobContext;
use crate::layer_rect::{canvas_rect_on_plane, plane_rect_offset};
use crate::precision_art_history::{ART_SOURCE_REFUSAL, ART_TARGET_REFUSAL};
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{admit, pixel_count, reserved_vec};

/// A straight unit ink sample's premultiplied luminance: `blend::lum` of
/// the ink's naive RGB `(1 − C)(1 − K), (1 − M)(1 − K), (1 − Y)(1 − K)`,
/// times alpha — the reading `style_blend_if`'s Gray takes of CMYK ink.
fn ink_luma(p: [f32; 5]) -> f64 {
    let paper = 1. - p[3];
    let rgb = [
        (1. - p[0]) * paper,
        (1. - p[1]) * paper,
        (1. - p[2]) * paper,
    ];
    f64::from(lum(rgb)) * f64::from(p[4])
}

/// A record's part as read: its plane rect, its stored samples and the
/// samples as unit ink on the canvas.
type ReadPart = ((u32, u32, u32, u32), Vec<CmykSample>, Part<5>);

/// `image`'s part over canvas rect `reach` as unit ink, with the samples it
/// was read from; `None` when the rect misses the record.
fn read_part(
    image: &CmykImage,
    offset: (i32, i32),
    canvas: (u32, u32),
    reach: (i64, i64, i64, i64),
    job: JobContext<'_>,
) -> Result<Option<ReadPart>, String> {
    let Some(part) = canvas_rect_on_plane(canvas, image.dimensions(), offset, reach) else {
        return Ok(None);
    };
    let raw = image.read_rect(part, job)?;
    let (cx, cy) = plane_rect_offset(offset, part);
    let mut samples = reserved_vec(raw.len())?;
    samples.extend(raw.iter().map(|p| p.map(unit)));
    let rect = (i64::from(cx), i64::from(cy), part.2, part.3);
    Ok(Some((part, raw, Part { rect, samples })))
}

impl PrecisionDocument {
    /// The Art History Brush on CMYK record `index`, inside the lock gate
    /// (`PrecisionDocument::art_history_brush` holds it; module doc).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cmyk_art_history_unlocked(
        &self,
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
        let plan = self.art_history_brush_plan(
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
        // The plan has refused a target or source that is not a CMYK raster.
        let record = self
            .layer(index)
            .ok_or("CMYK paint target is out of range")?;
        let image = record.cmyk_source().ok_or(ART_TARGET_REFUSAL)?;
        let offset = record.offset();
        let from = source
            .layer(source_layer)
            .ok_or("history brush source layer is out of range")?;
        let from_image = from.cmyk_source().ok_or(ART_SOURCE_REFUSAL)?;
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
            luma: ink_luma,
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
        let composite = |px: &mut InkPixel, ink: InkPixel, alpha: f32, xy: (i64, i64)| {
            let src = [ink[0], ink[1], ink[2], ink[3]];
            if frozen {
                over_mode_frozen(px, src, alpha, mode, xy);
            } else {
                over_mode(px, src, alpha, mode, xy);
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
        for (slot, sample) in out.iter_mut().zip(&acc) {
            let result = codes(*sample);
            // Sub-quantum alpha leaves a transparent sample's ink as it was
            // (`CmykPaintEdit::pixel`'s rule), so it changes no stored code.
            if !(result[4] == 0 && slot[4] == 0) {
                *slot = result;
            }
        }
        let Some(image) = image.with_rect(part, &out, job)? else {
            job.report(1., 5)?;
            return Ok(None);
        };
        job.check()?;
        let next = self.replacing_content(index, Content::CmykRaster { image, offset })?;
        job.report(1., 5)?;
        Ok(next)
    }
}
