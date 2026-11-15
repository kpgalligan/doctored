//! The Background Eraser on a byte layer: `background_eraser`'s coverage over the stroke's
//! footprint, erased through the byte paint walk's Clear (`doc_paint`, the Eraser's and
//! Edit ▸ Fill…'s Clear rule `alpha' = alpha × (1 − coverage)`, colour kept). A thin
//! adapter: the layer's bytes read as ×257 codes over the footprint, the kernel's field
//! quantized once (`BrushRequest::quantize`), each u16 code rounded to a byte and
//! multiplied by the selection's byte, then one walk over the footprint frame, so a stroke
//! costs its footprint, never the canvas.
use crate::background_eraser::{
    field, placed, plan, BackgroundEraserRequest, LikeRule, Reference, ReferenceCodes,
};
use crate::brush_coverage::BrushRequest;
use crate::doc::DrDocument;
use crate::doc_lock::EditKind;
use crate::doc_paint::{Ink, OverlayFrame, PaintMode};
use crate::job_control::JobContext;
use crate::precision_paint::PaintColorSpace;

/// A byte document's reference colour as ×257 codes: the request's straight
/// colour in the document's own space, each component rounded to its byte. An
/// sRGB, linear or ink colour refuses: the host converts a swatch into the
/// document's bytes before it asks.
fn byte_codes(reference: &Reference) -> Result<[u16; 4], String> {
    let color = match (reference.color, reference.color_space, reference.ink) {
        (Some(color), None | Some(PaintColorSpace::Document), None)
            if color
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v)) =>
        {
            color
        }
        _ => {
            return Err(
                "a byte document's background eraser colours are document-space RGBA in 0…1".into(),
            )
        }
    };
    Ok(color.map(|v| u16::from((v * 255.).round() as u8) * 257))
}

impl DrDocument {
    /// The Background Eraser along `dabs` (`dr_brush_u16`'s packed x / y /
    /// diameter / opacity tuples) on raster layer `idx` (module doc of
    /// `background_eraser`), confined by `selection`, canvas-sized u8 coverage.
    /// `Err`: a bad request, dab list or swatch, the stroke past 256 million
    /// work units, a selection of another length. `Ok(None)`: an out-of-range
    /// or group `idx`, no dab reaching the canvas, a pixels lock or a
    /// transparency lock (the walk's Clear changes nothing with alpha frozen),
    /// or no byte changing.
    pub(crate) fn background_eraser_layer(
        &self,
        idx: usize,
        request: &BackgroundEraserRequest,
        brush: &BrushRequest,
        dabs: &[f64],
        selection: Option<&[u8]>,
    ) -> Result<Option<Self>, String> {
        let dimensions = (self.width, self.height);
        let planned = plan(brush, dimensions, dabs)?;
        let references = ReferenceCodes {
            swatch: request.swatch.as_ref().map(byte_codes).transpose()?,
            protect: request.protect.as_ref().map(byte_codes).transpose()?,
        };
        let width = self.width as usize;
        if selection.is_some_and(|s| s.len() != width * self.height as usize) {
            return Err("background eraser selection length differs from the canvas".into());
        }
        let Some(layer) = self.raster_layer(idx) else {
            return Ok(None);
        };
        let Some(footprint) = planned.footprint else {
            return Ok(None);
        };
        let samples = placed(
            footprint,
            layer.pixels.dimensions(),
            layer.offset,
            |ly, x0, x1, out| {
                for (_, run) in layer.pixels.row_segments(ly, x0, x1) {
                    out.extend(run.iter().map(|p| p.map(|v| u16::from(v) * 257)));
                }
                Ok(())
            },
            JobContext::NONE,
        )?;
        let coverage = field(
            request,
            brush,
            dimensions,
            dabs,
            footprint,
            &samples,
            LikeRule::Rgb,
            references,
            JobContext::NONE,
        )?;
        let [fx0, fy0, fx1, fy1] = footprint;
        let fw = fx1 - fx0;
        let mask: Vec<u8> = coverage
            .iter()
            .enumerate()
            .map(|(k, &value)| {
                let byte = (u32::from(brush.quantize(value)) + 128) / 257;
                let selected =
                    selection.map_or(255, |s| u32::from(s[(fy0 + k / fw) * width + fx0 + k % fw]));
                ((byte * selected + 127) / 255) as u8
            })
            .collect();
        if mask.iter().all(|&m| m == 0) {
            return Ok(None);
        }
        let frame = OverlayFrame {
            x: fx0 as i64,
            y: fy0 as i64,
            width: fw as u32,
            height: (fy1 - fy0) as u32,
        };
        Ok(self.under_locks(idx, EditKind::Pixels, |doc| {
            doc.paint_walk(
                idx,
                frame,
                PaintMode::Clear,
                1.0,
                doc.alpha_frozen(idx),
                |i| Ink::Straight {
                    rgb: [0; 3],
                    alpha: mask[i],
                },
            )
        }))
    }
}
