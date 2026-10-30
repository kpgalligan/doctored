//! Edit ▸ Stroke… with nothing selected on a CMYK document (#1): a CMYK
//! raster's own alpha edge stroked in ink — the CMYK twin of
//! `precision_fill_stroke::LayerEdgeStroke`, whose module doc (and the byte
//! master's, `doc_fill_stroke::stroke_layer_edge`) is this module's
//! specification: the frame, the band, the growth rule.
//!
//! There is no new band or blend math. The layer's alpha > 0 bounds `B`
//! ([`cmyk_alpha_bounds`], the CMYK twin of `doc_export_layers::alpha_bounds16`)
//! grown by `ceil(width) + 2` on every side is the frame `F`; a u16 plane of
//! `F` holds the layer's alpha over `B` and 0 elsewhere; `PlaneOperation::Stroke`
//! (the selection path's band) turns it into the band; and every band pixel
//! goes through `CmykPaintEdit::PaintBlend`'s own pixel (`cmyk_paint`:
//! straight CMYKA over through the request's blend mode on `1 − ink`, at
//! `ink_alpha × opacity × band`, the backdrop's alpha
//! frozen under Preserve Transparency or a transparency lock). Outside the
//! layer the backdrop is bare paper. When the layer's alpha is free to change
//! the layer grows to its union with the changed pixels (`merged_extent`) —
//! past the canvas if the band reaches there — with comps following, as
//! `cmyk_paint`'s walk grows; otherwise only the layer's own pixels change.
//!
//! The request is `{"op":"stroke_layer_edge","layer","width","location",
//! "ink":[c,m,y,k,a],"opacity","blend_mode","preserve_alpha"}` (unit ink, no
//! `color`/`color_space`) on the pixel-edit request API with no input
//! samples; its plan reports `outline`. The edit runs under the record's
//! `Pixels` locks; a blend mode outside `CMYK_BLEND_MODES` (Pass Through)
//! refuses by name (#1); an edit that
//! changes nothing is `Ok(None)`.
//!
//! The plan is `8·BOOKKEEPING + canvas·cmyk_paint::CANVAS_BYTES_PER_PIXEL` (the
//! host's display preparation, as every CMYK paint plan charges it) `+
//! F·14` (the frame's ink copy, the alpha plane and the band) `+` the band
//! kind's own plan `+ output·10` (the grown or kept layer).

use serde::Deserialize;
use serde_json::{json, Value};

use crate::cmyk_image::{CmykImage, CmykSample, PAPER};
use crate::cmyk_paint::{CmykPaintEdit, CANVAS_BYTES_PER_PIXEL};
use crate::doc_lock::{EditKind, LOCK_TRANSPARENCY};
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{admit, pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use crate::precision_selection_plane::PlaneOperation;
use crate::retouch_transform::{merged_extent, StorageExtent};
use crate::style_model::StrokePosition;

/// One CMYK `stroke_layer_edge` request (module doc).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CmykLayerEdgeStroke {
    op: String,
    layer: usize,
    width: f32,
    location: StrokePosition,
    ink: [f32; 5],
    opacity: f32,
    #[serde(default)]
    blend_mode: u32,
    #[serde(default)]
    preserve_alpha: bool,
}

/// The alpha > 0 bounds of a CMYK raster, `(x, y, w, h)` in layer space, or
/// `None` when nothing is visible: the CMYK twin of
/// `doc_export_layers::alpha_bounds16` (its master), the same extremes over
/// the image's row runs, polled once a row.
pub(crate) fn cmyk_alpha_bounds(
    image: &CmykImage,
    job: JobContext<'_>,
) -> Result<Option<(u32, u32, u32, u32)>, String> {
    let (w, h) = image.dimensions();
    let mut extent: Option<(u32, u32, u32, u32)> = None;
    for y in 0..h {
        job.check()?;
        for (x, run) in image.row_segments(y, 0, w) {
            let Some(first) = run.iter().position(|p| p[4] != 0) else {
                continue;
            };
            let last = run.iter().rposition(|p| p[4] != 0).unwrap_or(first);
            let (a, b) = (x + first as u32, x + last as u32);
            extent = Some(match extent {
                None => (a, y, b, y),
                Some((x0, y0, x1, _)) => (x0.min(a), y0, x1.max(b), y),
            });
        }
    }
    Ok(extent.map(|(x0, y0, x1, y1)| (x0, y0, x1 - x0 + 1, y1 - y0 + 1)))
}

/// The edge's frame `F` in canvas coordinates and the pad.
struct Frame {
    x: i64,
    y: i64,
    width: u32,
    height: u32,
    pad: u32,
    bounds: (u32, u32, u32, u32),
}

impl CmykLayerEdgeStroke {
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(text)
            .map_err(|e| format!("invalid layer-edge stroke request: {e}"))?;
        if ["color", "color_space"]
            .iter()
            .any(|key| value.get(*key).is_some())
        {
            return Err(
                "a CMYK document strokes in ink: pass ink as [c, m, y, k, alpha] in 0…1, not an \
                 RGB color"
                    .into(),
            );
        }
        let edit: Self = serde_json::from_value(value)
            .map_err(|e| format!("invalid layer-edge stroke request: {e}"))?;
        if edit.op != "stroke_layer_edge" {
            return Err("expected stroke_layer_edge request".into());
        }
        let mode = crate::blend::BlendMode::from_c(edit.blend_mode as i32)
            .ok_or("native paint opacity or blend mode is invalid")?;
        if let Some(refusal) = crate::cmyk_document::blend_refusal(mode) {
            return Err(refusal);
        }
        Ok(edit)
    }

    /// The paint each band pixel goes through.
    fn paint(&self) -> CmykPaintEdit {
        CmykPaintEdit::PaintBlend {
            layer: self.layer,
            ink: self.ink,
            opacity: self.opacity,
            blend_mode: self.blend_mode,
            selection: false,
            preserve_alpha: self.preserve_alpha,
        }
    }

    fn band(&self) -> PlaneOperation {
        PlaneOperation::Stroke {
            width: self.width,
            location: self.location,
        }
    }

    /// The refusals; the layer's image and offset.
    fn target<'a>(
        &self,
        doc: &'a PrecisionDocument,
    ) -> Result<(&'a CmykImage, (i32, i32)), String> {
        doc.require_cmyk_policy("a CMYK stroke")?;
        let layer = doc
            .layer(self.layer)
            .filter(|l| l.cmyk_source().is_some())
            .ok_or("Stroke… outlines a raster layer; this index names none")?;
        doc.lock_refusal(self.layer, EditKind::Pixels)?;
        if !self.width.is_finite() || self.width <= 0. {
            return Err("The stroke width must be a finite number of pixels above 0".into());
        }
        if !self.opacity.is_finite() || !(0. ..=1.).contains(&self.opacity) {
            return Err("The stroke opacity must be a number in [0, 1]".into());
        }
        if self
            .ink
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        {
            return Err("CMYK ink must be finite in [0,1]".into());
        }
        let image = layer
            .cmyk_source()
            .ok_or("CMYK stroke requires a CMYK raster")?;
        Ok((image, layer.offset()))
    }

    /// Whether the stroke may grow the layer.
    fn expands(&self, doc: &PrecisionDocument) -> bool {
        !self.preserve_alpha && doc.lock_owner(self.layer, LOCK_TRANSPARENCY).is_none()
    }

    fn frame(
        &self,
        image: &CmykImage,
        offset: (i32, i32),
        job: JobContext<'_>,
    ) -> Result<Option<Frame>, String> {
        match cmyk_alpha_bounds(image, job)? {
            Some(bounds) => self.frame_over(bounds, offset).map(Some),
            None => Ok(None),
        }
    }

    /// The frame `F` around `bounds` (layer space).
    fn frame_over(
        &self,
        bounds: (u32, u32, u32, u32),
        offset: (i32, i32),
    ) -> Result<Frame, String> {
        let too_large = || "the stroke's frame exceeds the supported size".to_string();
        let pad =
            u32::try_from((self.width.ceil() as i64).saturating_add(2)).map_err(|_| too_large())?;
        let side = |span: u32| {
            span.checked_add(pad.checked_mul(2).ok_or_else(too_large)?)
                .ok_or_else(too_large)
        };
        let frame = Frame {
            x: i64::from(offset.0) + i64::from(bounds.0) - i64::from(pad),
            y: i64::from(offset.1) + i64::from(bounds.1) - i64::from(pad),
            width: side(bounds.2)?,
            height: side(bounds.3)?,
            pad,
            bounds,
        };
        pixel_count(frame.width, frame.height).map_err(|_| too_large())?;
        Ok(frame)
    }

    /// The layer's extent after the stroke, at most.
    fn extent(
        &self,
        doc: &PrecisionDocument,
        image: &CmykImage,
        offset: (i32, i32),
        frame: &Frame,
    ) -> Result<StorageExtent, String> {
        let kept = (image.dimensions(), offset);
        if !self.expands(doc) {
            return Ok(kept);
        }
        let origin = (
            i32::try_from(frame.x).map_err(|_| "the stroke's frame is out of range")?,
            i32::try_from(frame.y).map_err(|_| "the stroke's frame is out of range")?,
        );
        merged_extent(kept, ((frame.width, frame.height), origin))
    }

    fn scratch(
        &self,
        doc: &PrecisionDocument,
        image: &CmykImage,
        offset: (i32, i32),
        frame: &Frame,
    ) -> Result<u64, String> {
        let canvas = pixel_count(doc.width, doc.height)? as u64;
        let area = u64::from(frame.width) * u64::from(frame.height);
        let band = self.band().plan((frame.width, frame.height))?;
        let ((ew, eh), _) = self.extent(doc, image, offset, frame)?;
        Ok(8 * BOOKKEEPING_BYTES
            + canvas * CANVAS_BYTES_PER_PIXEL
            + area * 14
            + band
            + u64::from(ew) * u64::from(eh) * 10)
    }

    pub(crate) fn plan(&self, doc: &PrecisionDocument) -> Result<Value, String> {
        let (image, offset) = self.target(doc)?;
        let frame = self.frame(image, offset, JobContext::NONE)?;
        let scratch = match &frame {
            Some(frame) => self.scratch(doc, image, offset, frame)?,
            None => 8 * BOOKKEEPING_BYTES,
        };
        Ok(json!({
            "retained_bytes": doc.retained_bytes(),
            "scratch_bytes": scratch,
            "input_samples": 0,
            "outline": frame.is_some(),
        }))
    }

    /// The plan's scratch bound without scanning the layer's alpha: the
    /// layer's stored extent (never smaller than its alpha > 0 bounds) as
    /// `B`, so it is at least [`Self::plan`]'s figure and costs O(1). The
    /// document descriptor reports it, and the descriptor is read on every
    /// new document state, so it must not pay a pass over the pixels.
    pub(crate) fn scratch_bound(&self, doc: &PrecisionDocument) -> Result<u64, String> {
        let (image, offset) = self.target(doc)?;
        let (w, h) = image.dimensions();
        if w == 0 || h == 0 {
            return Ok(8 * BOOKKEEPING_BYTES);
        }
        let frame = self.frame_over((0, 0, w, h), offset)?;
        self.scratch(doc, image, offset, &frame)
    }

    /// The stroke (module doc) under the record's `Pixels` locks.
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
        doc.under_locks(self.layer, EditKind::Pixels, |doc| {
            self.applying_unlocked(doc, limit, other, job)
        })
    }

    fn applying_unlocked(
        &self,
        doc: &PrecisionDocument,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        let (image, (ox, oy)) = self.target(doc)?;
        let Some(frame) = self.frame(image, (ox, oy), job)? else {
            job.report(1., 5)?;
            return Ok(None);
        };
        let scratch = self.scratch(doc, image, (ox, oy), &frame)?;
        admit(doc.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let job = job.with_reservation_floor(scratch);
        if self.opacity == 0. {
            job.report(1., 5)?;
            return Ok(None);
        }
        let (fw, fh) = (frame.width as usize, frame.height as usize);
        let (lw, lh) = image.dimensions();
        // The alpha plane over F, then the band.
        let mut plane = reserved_vec(fw * fh)?;
        plane.resize(fw * fh, 0u16);
        let (bx, by, bw, bh) = frame.bounds;
        let pad = frame.pad as usize;
        for y in by..by + bh {
            job.check()?;
            let row = (pad + (y - by) as usize) * fw + pad;
            for (x, run) in image.row_segments(y, bx, bx + bw) {
                let at = row + (x - bx) as usize;
                for (dst, p) in plane[at..at + run.len()].iter_mut().zip(run) {
                    *dst = p[4];
                }
            }
        }
        let band = self
            .band()
            .apply(&plane, (frame.width, frame.height), job.range(0., 0.5))?;
        drop(plane);
        if band.iter().all(|&v| v == 0) {
            job.report(1., 5)?;
            return Ok(None);
        }
        // F ∩ the layer, in layer space; F's ink copy (paper outside the layer).
        let (fx, fy) = (frame.x, frame.y);
        let lx0 = (fx - i64::from(ox)).max(0);
        let ly0 = (fy - i64::from(oy)).max(0);
        let lx1 = (fx + fw as i64 - i64::from(ox)).min(i64::from(lw));
        let ly1 = (fy + fh as i64 - i64::from(oy)).min(i64::from(lh));
        let part = (lx0 < lx1 && ly0 < ly1).then(|| {
            (
                lx0 as u32,
                ly0 as u32,
                (lx1 - lx0) as u32,
                (ly1 - ly0) as u32,
            )
        });
        let mut rect: Vec<CmykSample> = reserved_vec(fw * fh)?;
        rect.resize(fw * fh, PAPER);
        if let Some(part) = part {
            let samples = image.read_rect(part, job)?;
            let dx = (i64::from(part.0) + i64::from(ox) - fx) as usize;
            let dy = (i64::from(part.1) + i64::from(oy) - fy) as usize;
            for (row, line) in samples.chunks_exact(part.2 as usize).enumerate() {
                let at = (dy + row) * fw + dx;
                rect[at..at + line.len()].copy_from_slice(line);
            }
        }
        let frozen = !self.expands(doc);
        let paint = self.paint();
        let mut bounds: Option<(i64, i64, i64, i64)> = None;
        for (k, slot) in rect.iter_mut().enumerate() {
            job.every(k)?;
            if band[k] == 0 {
                continue;
            }
            let original = *slot;
            let s = f64::from(band[k]) / 65535.;
            let x = fx + (k % fw) as i64;
            let y = fy + (k / fw) as i64;
            let result = paint.pixel(original, s, self.opacity, frozen, (x, y));
            if result != original {
                *slot = result;
                bounds = Some(match bounds {
                    None => (x, y, x + 1, y + 1),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)),
                });
            }
        }
        drop(band);
        let Some((cx0, cy0, cx1, cy1)) = bounds else {
            job.report(1., 5)?;
            return Ok(None);
        };
        let kept = ((lw, lh), (ox, oy));
        let extent = if frozen {
            kept
        } else {
            let origin = (
                i32::try_from(cx0).map_err(|_| "the stroke's frame is out of range")?,
                i32::try_from(cy0).map_err(|_| "the stroke's frame is out of range")?,
            );
            merged_extent(kept, (((cx1 - cx0) as u32, (cy1 - cy0) as u32), origin))?
        };
        let out = match part {
            Some(part) if extent == kept => {
                // F ∩ layer back through `with_rect` (untouched tiles shared).
                let dx = (i64::from(part.0) + i64::from(ox) - fx) as usize;
                let dy = (i64::from(part.1) + i64::from(oy) - fy) as usize;
                let mut samples = reserved_vec(part.2 as usize * part.3 as usize)?;
                for row in 0..part.3 as usize {
                    let at = (dy + row) * fw + dx;
                    samples.extend_from_slice(&rect[at..at + part.2 as usize]);
                }
                let Some(out) = image.with_rect(part, &samples, job)? else {
                    job.report(1., 5)?;
                    return Ok(None);
                };
                out
            }
            None if extent == kept => {
                job.report(1., 5)?;
                return Ok(None);
            }
            _ => {
                // The grown extent: paper, the old layer's rows, then F's
                // samples over F ∩ extent (`cmyk_paint`'s grown walk).
                let ((nw, nh), (nx, ny)) = extent;
                let (left, top) = (i64::from(nx), i64::from(ny));
                let (r0, r1) = (fx.max(left), (fx + fw as i64).min(left + i64::from(nw)));
                CmykImage::from_rows_with_job(
                    nw,
                    nh,
                    |y, row| {
                        row.fill(PAPER);
                        let cy = i64::from(y) + top;
                        let ly = cy - i64::from(oy);
                        if (0..i64::from(lh)).contains(&ly) {
                            let at = (i64::from(ox) - left) as usize;
                            for (x, run) in image.row_segments(ly as u32, 0, lw) {
                                let at = at + x as usize;
                                row[at..at + run.len()].copy_from_slice(run);
                            }
                        }
                        if (fy..fy + fh as i64).contains(&cy) && r0 < r1 {
                            let from = (cy - fy) as usize * fw + (r0 - fx) as usize;
                            let at = (r0 - left) as usize;
                            let len = (r1 - r0) as usize;
                            row[at..at + len].copy_from_slice(&rect[from..from + len]);
                        }
                        job.check()
                    },
                    job,
                )?
            }
        };
        job.check()?;
        let (_, (nx, ny)) = extent;
        let mut next = doc.replacing_content(
            self.layer,
            Content::CmykRaster {
                image: out,
                offset: (nx, ny),
            },
        )?;
        if let Some(next) = next.as_mut() {
            if let Some(comps) = doc.comps_after_growth(self.layer, (nx, ny)) {
                next.comps = comps;
            }
        }
        job.report(1., 5)?;
        Ok(next)
    }
}
