//! Edit ▸ Stroke… with nothing selected at 16 and 32 bits:
//! a typed layer's own alpha edge stroked in LAYER space — the typed twin of
//! `doc_fill_stroke::stroke_layer_edge`, the BYTE MASTER whose module doc is
//! this module's specification (the frame, the growth rule, the refusals).
//!
//! There is no new band or blend math here. The layer's alpha > 0 bounds `B`
//! (`doc_export_layers::alpha_bounds16`, or `alpha_bounds32` in a float
//! document; the mask excluded) grown by `ceil(width) + 2` on every side is
//! the frame `F`; a u16 plane of `F` holds the layer's alpha over `B` (an f32
//! alpha through `unit_to_u16`) and 0 elsewhere; `PlaneOperation::Stroke` (the
//! selection path's band) turns it into the band; and the band is painted
//! through `NativePaintEdit::PaintBlend` laid out over `F`
//! (`precision_paint_tools`' `apply_in`, whose float arm is `float_paint`),
//! the 16-bit brush's one compositing path, under the layer's locks. When the layer's alpha is free to change
//! (no Preserve Transparency, no transparency lock) the layer grows to its
//! union with the changed pixels — past the canvas if the band reaches there
//! — with comps following, as every native paint growth does.
//!
//! The band's canvas-edge ring is inert over `F`: every nonzero sample sits
//! at least `ceil(width) + 2` pixels from `F`'s edge, so the ring subtracts
//! everything there, and every band pixel falls inside `F`.
//!
//! The request is `{"op":"stroke_layer_edge", ...}` on the pixel-edit
//! request API with no input samples; its plan reports `outline` (whether
//! the layer has an alpha > 0 pixel). The plan and the apply each scan the
//! layer's alpha once.
//!
//! No new band or blend math: Fill is native paint's `paint_blend` (`precision_paint_tools`) over
//! the u16 selection coverage on a layer — growing a trimmed layer to the changed pixels like the
//! 16-bit brush, never under `preserve_alpha` (Preserve Transparency, restored through
//! `PrecisionDocument::restoring_alpha`, the restore `freezing_alpha` now calls) or a transparency
//! lock, which clips instead of refusing — and the scalar `paint` kind with a `blend_mode`
//! (`precision_scalar_edit`; Normal keeps the lerp byte-identical, every other mode is
//! `doc_plane::blend_triple` on gray, HSL refused by `precision_plane_math::validate`) on a mask,
//! colour plane or channel (a 32-bit float layer's colour plane is `paint_blend`'s `plane`,
//! `float_paint`). Stroke's band is `PlaneOperation::Stroke` (`precision_selection_plane`,
//! kind `stroke`, width finite in (0, 250]): `doc_fill_stroke::stroke_mask`'s rule at u16 built
//! from the Grow/Shrink arms (Outside `grow·(65535−cov)/65535`, Inside `cov ⊖ shrink` joined with
//! the plane-edge ring, Center the two halves at w/2), support-bounded with reach `ceil(w) + 1` and
//! work `|W|·64`, its exactness argued in `precision_selection_support`'s module doc. With nothing
//! selected, `LayerEdgeStroke` (`{"op":"stroke_layer_edge", …}` on the pixel-edit request API, no
//! input samples, plan reports `outline`) is `stroke_layer_edge` at u16 and f32: `alpha_bounds16`
//! (`alpha_bounds32` at float) grown by `ceil(width) + 2` is the frame `F`, the alpha over `B`
//! copied into a u16 plane of `F` (an f32 alpha through `unit_to_u16`), the `stroke` kind over it,
//! then `paint_blend` laid out over `F` through `NativePaintEdit::apply_in` (`PaintFrame`;
//! `plan`/`apply` are the canvas frame and unchanged; bucket fill and gradient refuse another
//! frame), growing past the canvas with comps following. Refusals in the byte order: raster index,
//! retained source, Pixels lock, width, opacity, group-only mode; `PrecisionDocument::build` ties
//! the leaf storage to the document mode, so there is no storage refusal. Tests:
//! `tests/precision_fill_stroke.rs`, `tests/float_fill_stroke.rs`.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::blend::BlendMode;
use crate::doc_export_layers::{alpha_bounds16, alpha_bounds32};
use crate::doc_lock::EditKind;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{
    admit, pixel_count, reserved_vec, unit_to_u16, PrecisionImage, PrecisionRun, PrecisionStorage,
    BOOKKEEPING_BYTES,
};
use crate::precision_paint::PaintColorSpace;
use crate::precision_paint_tools::{NativePaintEdit, PaintFrame};
use crate::precision_selection_plane::PlaneOperation;
use crate::style_model::StrokePosition;

/// The sentence a retained-source layer's layer-edge stroke refuses with
/// (the byte master's).
const RETAINED_SOURCE: &str = "Rasterize the retained source before stroking its pixels";

/// One `stroke_layer_edge` request.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LayerEdgeStroke {
    op: String,
    layer: usize,
    width: f32,
    location: StrokePosition,
    color: [f32; 4],
    color_space: PaintColorSpace,
    opacity: f32,
    blend_mode: u32,
    #[serde(default)]
    preserve_alpha: bool,
}

/// The layer's edge: its alpha bounds `B` (layer space, `(x, y, w, h)`), the
/// pad and the frame `F`.
struct Edge {
    bounds: (u32, u32, u32, u32),
    pad: u32,
    frame: PaintFrame,
}

impl LayerEdgeStroke {
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let edit: Self = serde_json::from_str(text)
            .map_err(|e| format!("invalid layer-edge stroke request: {e}"))?;
        if edit.op != "stroke_layer_edge" {
            return Err("expected stroke_layer_edge request".into());
        }
        Ok(edit)
    }

    /// The paint the band goes through.
    fn paint(&self) -> NativePaintEdit {
        NativePaintEdit::PaintBlend {
            layer: self.layer,
            color: self.color,
            color_space: self.color_space,
            opacity: self.opacity,
            blend_mode: self.blend_mode,
            selection: false,
            preserve_alpha: self.preserve_alpha,
            plane: None,
            pattern: None,
        }
    }

    /// The band's kind.
    fn band(&self) -> PlaneOperation {
        PlaneOperation::Stroke {
            width: self.width,
            location: self.location,
        }
    }

    /// The refusals, in the byte master's order; the layer's image.
    fn target<'a>(&self, doc: &'a PrecisionDocument) -> Result<&'a PrecisionImage, String> {
        let layer = doc
            .layer(self.layer)
            .filter(|l| l.source().is_some())
            .ok_or("Stroke… outlines a raster layer; this index names none")?;
        let image = layer.source().unwrap();
        if layer.retained_source.is_some() {
            return Err(RETAINED_SOURCE.into());
        }
        doc.lock_refusal(self.layer, EditKind::Pixels)?;
        if !self.width.is_finite() || self.width <= 0. {
            return Err("The stroke width must be a finite number of pixels above 0".into());
        }
        if self.opacity.is_nan() {
            return Err("The stroke opacity must be a number".into());
        }
        if i32::try_from(self.blend_mode)
            .ok()
            .and_then(BlendMode::from_c)
            .is_some_and(BlendMode::is_group_only)
        {
            return Err("Pass Through is a group's mode, not a stroke blend mode".into());
        }
        Ok(image)
    }

    /// The layer's edge, or `None` for a layer with no alpha > 0 pixel. One
    /// alpha scan, polled by `job`.
    fn edge(
        &self,
        doc: &PrecisionDocument,
        image: &PrecisionImage,
        job: JobContext<'_>,
    ) -> Result<Option<Edge>, String> {
        let bounds = match image.storage() {
            PrecisionStorage::Rgba16Unorm => alpha_bounds16(image, job)?,
            PrecisionStorage::Rgba32Float => alpha_bounds32(image, job)?,
        };
        let Some(bounds) = bounds else {
            return Ok(None);
        };
        let too_large = || "the stroke's frame exceeds the supported size".to_string();
        let pad =
            u32::try_from((self.width.ceil() as i64).saturating_add(2)).map_err(|_| too_large())?;
        let (ox, oy) = doc.layer(self.layer).map_or((0, 0), |l| l.offset());
        let side = |span: u32| {
            span.checked_add(pad.checked_mul(2).ok_or_else(too_large)?)
                .ok_or_else(too_large)
        };
        let frame = PaintFrame {
            x: i64::from(ox) + i64::from(bounds.0) - i64::from(pad),
            y: i64::from(oy) + i64::from(bounds.1) - i64::from(pad),
            width: side(bounds.2)?,
            height: side(bounds.3)?,
        };
        pixel_count(frame.width, frame.height).map_err(|_| too_large())?;
        Ok(Some(Edge { bounds, pad, frame }))
    }

    /// The working memory over `frame`: the paint's own figure, the band
    /// kind's, and the alpha plane plus the band.
    fn scratch(&self, doc: &PrecisionDocument, frame: PaintFrame) -> Result<u64, String> {
        let paint = self.paint().plan_in(doc, frame)?;
        let band = self.band().plan((frame.width, frame.height))?;
        let planes = u64::from(frame.width) * u64::from(frame.height) * 4;
        Ok(paint["scratch_bytes"]
            .as_u64()
            .unwrap_or(u64::MAX)
            .saturating_add(band)
            .saturating_add(planes))
    }

    pub(crate) fn plan(&self, doc: &PrecisionDocument) -> Result<Value, String> {
        let image = self.target(doc)?;
        let edge = self.edge(doc, image, JobContext::NONE)?;
        let scratch = match &edge {
            Some(edge) => self.scratch(doc, edge.frame)?,
            None => 8 * BOOKKEEPING_BYTES,
        };
        Ok(json!({
            "retained_bytes": doc.retained_bytes(),
            "scratch_bytes": scratch,
            "input_samples": 0,
            "outline": edge.is_some(),
        }))
    }

    /// The stroke (module doc). `Err`: every refusal [`Self::plan`] names, a
    /// reservation refusal or a cancellation. `Ok(None)`: a layer with no
    /// alpha > 0 pixel, an empty band, or a result identical to the input.
    pub(crate) fn apply(
        &self,
        doc: &PrecisionDocument,
        input: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        if !input.is_empty() {
            return Err("the layer-edge stroke takes no input samples".into());
        }
        let image = self.target(doc)?;
        let Some(edge) = self.edge(doc, image, job)? else {
            job.report(1., 5)?;
            return Ok(None);
        };
        let frame = edge.frame;
        let scratch = self.scratch(doc, frame)?;
        admit(doc.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let job = job.with_reservation_floor(scratch);
        let (fw, fh) = (frame.width as usize, frame.height as usize);
        let mut plane = reserved_vec(fw * fh)?;
        plane.resize(fw * fh, 0u16);
        let (bx, by, bw, bh) = edge.bounds;
        let pad = edge.pad as usize;
        for y in by..by + bh {
            job.check()?;
            let row = (pad + (y - by) as usize) * fw + pad;
            for (x, run) in image.row_segments(y, bx, bx + bw) {
                let at = row + (x - bx) as usize;
                match run {
                    PrecisionRun::Rgba16(run) => {
                        for (dst, p) in plane[at..at + run.len()].iter_mut().zip(run) {
                            *dst = p[3];
                        }
                    }
                    PrecisionRun::Rgba32(run) => {
                        for (dst, p) in plane[at..at + run.len()].iter_mut().zip(run) {
                            *dst = unit_to_u16(p[3]);
                        }
                    }
                }
            }
        }
        let band = self
            .band()
            .apply(&plane, (frame.width, frame.height), job)?;
        drop(plane);
        if band.iter().all(|&v| v == 0) {
            job.report(1., 5)?;
            return Ok(None);
        }
        let held = (band.len() as u64).saturating_mul(2);
        self.paint()
            .apply_in(doc, &band, frame, limit, other.saturating_add(held), job)
    }
}
