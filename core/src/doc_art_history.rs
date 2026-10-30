//! The byte adapter of the Art History Brush (the kernel is
//! `art_history_stroke`) on the layered document, modelled on `doc_smudge`:
//! the stroke arrives as its DABS (`dr_brush_u16`'s request and packed tuples
//! in canvas coordinates), because the clicks are the dabs themselves.
//!
//! The layer's pixels within `reach` of the dabs and the source state's
//! layer over the same canvas rect are read once, as straight unit samples;
//! the strokes head along the source's luminance (`rgb_luma`). The kernel
//! generates every stroke against them, then paints into an f32
//! copy of the layer's part through `blend::composite_source_into` (or
//! `composite_source_frozen` under a transparency lock), and the part is
//! quantized once, half-up. The layer never grows: strokes are clipped to
//! the layer and the canvas.
use crate::art_history_stroke::{generate, paint, reach, ArtHistoryRequest, ArtSamples};
use crate::blend::{blend_kind, composite_source_frozen, composite_source_into, lum, BlendMode};
use crate::brush_coverage::BrushRequest;
use crate::doc::DrDocument;
use crate::doc_lock::EditKind;
use crate::job_control::JobContext;
use crate::layer_rect::{canvas_rect_on_plane, plane_rect_offset};
use crate::precision_image::reserved_vec;

/// A straight byte pixel as unit samples.
fn unit(p: [u8; 4]) -> [f32; 4] {
    p.map(|v| f32::from(v) / 255.)
}

/// Round half-up back to a byte, as `smudge_layer` does.
fn byte(v: f32) -> u8 {
    (v.clamp(0., 1.) * 255. + 0.5).floor() as u8
}

/// One canvas-positioned part of a layer, read as unit samples.
pub(crate) struct Part<const C: usize> {
    /// The part's canvas rect: left, top, width, height.
    pub rect: (i64, i64, u32, u32),
    pub samples: Vec<[f32; C]>,
}

impl<const C: usize> Part<C> {
    pub(crate) fn at(&self, x: i64, y: i64) -> Option<[f32; C]> {
        let (rx, ry, rw, rh) = self.rect;
        let (dx, dy) = (x - rx, y - ry);
        if dx < 0 || dy < 0 || dx >= i64::from(rw) || dy >= i64::from(rh) {
            return None;
        }
        Some(self.samples[dy as usize * rw as usize + dx as usize])
    }
}

/// The two parts a click reads: the source state's and the layer's own,
/// and the premultiplied luminance a stroke's heading reads on the source.
pub(crate) struct Pair<const C: usize> {
    pub source: Part<C>,
    pub current: Part<C>,
    pub luma: fn([f32; C]) -> f64,
}

impl<const C: usize> ArtSamples<C> for Pair<C> {
    fn source(&self, x: i64, y: i64) -> Option<[f32; C]> {
        self.source.at(x, y)
    }
    fn current(&self, x: i64, y: i64) -> Option<[f32; C]> {
        self.current.at(x, y)
    }
    fn source_luma(&self, x: i64, y: i64) -> Option<f64> {
        self.source.at(x, y).map(self.luma)
    }
}

/// A straight unit RGBA sample's premultiplied luminance: `blend::lum`
/// (the one luminosity) times alpha.
pub(crate) fn rgb_luma(p: [f32; 4]) -> f64 {
    f64::from(lum([p[0], p[1], p[2]])) * f64::from(p[3])
}

/// The canvas rect the strokes of `dabs` can reach (exclusive ends,
/// clipped to the canvas), or `None` for no dabs.
pub(crate) fn reach_rect(
    request: &ArtHistoryRequest,
    dabs: &[f64],
    canvas: (u32, u32),
) -> Option<(i64, i64, i64, i64)> {
    let margin = reach(request, dabs);
    let mut bounds: Option<[f64; 4]> = None;
    for d in dabs.chunks_exact(4) {
        let b = bounds.get_or_insert([d[0], d[1], d[0], d[1]]);
        *b = [
            b[0].min(d[0]),
            b[1].min(d[1]),
            b[2].max(d[0]),
            b[3].max(d[1]),
        ];
    }
    let [x0, y0, x1, y1] = bounds?;
    let clip = |v: f64, end: u32| v.clamp(0., f64::from(end)) as i64;
    let rect = (
        clip((x0 - margin).floor(), canvas.0),
        clip((y0 - margin).floor(), canvas.1),
        clip((x1 + margin).ceil() + 1., canvas.0),
        clip((y1 + margin).ceil() + 1., canvas.1),
    );
    (rect.0 < rect.2 && rect.1 < rect.3).then_some(rect)
}

impl DrDocument {
    /// Paints layer `idx` with the Art History Brush from layer `source_idx`
    /// of `source` (an earlier state of this document): `request` is the
    /// tool's options, `brush` and `dabs` `dr_brush_u16`'s (the dabs in
    /// CANVAS coordinates), `selection` canvas-sized u8 coverage confining
    /// the strokes, or `None`. Under a transparency lock every pixel keeps
    /// its alpha.
    ///
    /// `Ok(None)` (a domain refusal or nothing to do), as
    /// `history_brush_layer` refuses: `idx` or `source_idx` out of range or a
    /// group, a source of another canvas size, a lock that forbids a PIXELS
    /// edit, no dab, a stroke that misses the layer, or no byte changing (an
    /// identical copy would register a phantom undo step). `Err`: the brush
    /// request's refusals (`BrushRequest::plan`), the kernel's work refusal,
    /// a `selection` that is not canvas-sized, or a failed allocation.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn art_history_brush_layer(
        &self,
        idx: usize,
        source: &DrDocument,
        source_idx: usize,
        request: &ArtHistoryRequest,
        brush: &BrushRequest,
        dabs: &[f64],
        selection: Option<&[u8]>,
    ) -> Result<Option<Self>, String> {
        self.under_locks_fallible(idx, EditKind::Pixels, |doc| {
            doc.art_history_unlocked(idx, source, source_idx, request, brush, dabs, selection)
        })
    }

    /// The body of [`Self::art_history_brush_layer`], outside the lock gate.
    #[allow(clippy::too_many_arguments)]
    fn art_history_unlocked(
        &self,
        idx: usize,
        source: &DrDocument,
        source_idx: usize,
        request: &ArtHistoryRequest,
        brush: &BrushRequest,
        dabs: &[f64],
        selection: Option<&[u8]>,
    ) -> Result<Option<Self>, String> {
        let canvas = (self.width, self.height);
        if selection.is_some_and(|s| s.len() != canvas.0 as usize * canvas.1 as usize) {
            return Err("art history brush selection length differs from the canvas".into());
        }
        brush.plan(canvas, dabs)?;
        let (Some(layer), Some(from)) = (self.raster_layer(idx), source.raster_layer(source_idx))
        else {
            return Ok(None);
        };
        if (source.width, source.height) != canvas {
            return Ok(None);
        }
        let Some(reach) = reach_rect(request, dabs, canvas) else {
            return Ok(None);
        };
        let Some(part) =
            canvas_rect_on_plane(canvas, layer.pixels.dimensions(), layer.offset, reach)
        else {
            return Ok(None);
        };
        let read = |pixels: &crate::layer_raster::LayerRaster, offset, r: (u32, u32, u32, u32)| {
            let raw = pixels
                .read_rect(r.0, r.1, r.2, r.3)
                .ok_or("could not allocate the art history brush rect")?;
            let (cx, cy) = plane_rect_offset(offset, r);
            let mut samples = reserved_vec(raw.len())?;
            samples.extend(raw.iter().map(|p| unit(*p)));
            Ok::<_, String>((
                raw,
                Part {
                    rect: (i64::from(cx), i64::from(cy), r.2, r.3),
                    samples,
                },
            ))
        };
        let (old, current) = read(&layer.pixels, layer.offset, part)?;
        let source_part =
            match canvas_rect_on_plane(canvas, from.pixels.dimensions(), from.offset, reach) {
                Some(r) => read(&from.pixels, from.offset, r)?.1,
                None => Part {
                    rect: (0, 0, 0, 0),
                    samples: Vec::new(),
                },
            };
        let samples = Pair {
            source: source_part,
            current,
            luma: rgb_luma,
        };
        let strokes = generate(request, dabs, &samples)?;
        if strokes.is_empty() {
            return Ok(None);
        }
        let Some(mode) = BlendMode::from_c(request.blend_mode as i32) else {
            return Ok(None);
        };
        let frozen = self.alpha_frozen(idx);
        let rect = samples.current.rect;
        let mut acc = samples.current.samples;
        let gate = |x: i64, y: i64| {
            selection.map_or(1., |s| {
                f32::from(s[y as usize * canvas.0 as usize + x as usize]) / 255.
            })
        };
        let composite = |px: &mut [f32; 4], colour: [f32; 4], alpha: f32, xy: (i64, i64)| {
            let cs = [colour[0], colour[1], colour[2]];
            if frozen {
                composite_source_frozen(px, cs, alpha, blend_kind(mode), xy);
            } else {
                composite_source_into(std::slice::from_mut(px), 0, cs, alpha, blend_kind(mode), xy);
            }
        };
        if !paint(
            &strokes,
            brush,
            rect,
            &mut acc,
            gate,
            composite,
            JobContext::NONE,
        )? {
            return Ok(None);
        }
        let mut px = old;
        let mut changed = false;
        for (slot, sample) in px.iter_mut().zip(&acc) {
            let mut new = sample.map(byte);
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
            return Ok(None);
        }
        let Some(pixels) = layer
            .pixels
            .with_rect(part.0, part.1, part.2, part.3, &px)?
        else {
            return Ok(None);
        };
        Ok(self.with_layer_raster(idx, pixels))
    }
}
