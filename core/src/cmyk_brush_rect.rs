//! The RECT path of the CMYK retouch brushes (13B-9; `cmyk_retouch_brush`'s
//! module doc has the two passes): Dodge and Burn, the Blur, Sharpen and
//! Sponge brushes, the History Brush and Smudge run their `L` passes over
//! the stroke's rect on the record, not over the whole layer.
//!
//! The brush FRAME `G` ([`PrecisionDocument::cmyk_brush_frame`]) is the
//! stroke coverage's bounding box on the record (`coverage_rect16`), grown by
//! how far the kind reads past it and clipped to the record: 0 for Dodge,
//! Burn, the Sponge and the History Brush, whose walks read and write only
//! the coverage rect, and `retouch_brush::halo` for Blur and Sharpen, whose
//! Gaussian reads that far past it with reads clamped to the layer (wherever
//! the halo crosses the layer's edge, that edge is `G`'s, so the clamped
//! reads agree). A stroke that misses the record has the frame
//! `(0, 0, 1, 1)`: the RGB walk still validates its request, then answers
//! nothing. Each pass document is the canvas with layer 0 the record's rect
//! `G` (`cmyk_passes::pass_rect_image`) at its canvas position, carrying the
//! record's transparency lock as `cmyk_pass_document` does, so the RGB walk
//! sees the samples, coverage, selection and canvas positions of the
//! whole-layer pass over `G`; the join is `cmyk_passes::join_rect` over `G`.
//! The History Brush's state passes hold the source layer's part under `G`
//! (one paper, α 0 pixel at the canvas origin when the source misses it: the
//! walk's premultiplied mix multiplies a transparent source out, as it does
//! the `[0; 4]` it reads where the source has no pixel). Sample All Layers'
//! plates stay canvas-sized; the walks read them by canvas position.
//!
//! Smudge runs no pass document: its footprint `F` (`brush_coverage::
//! footprint` of the layer-space dabs, clipped to the canvas under Sample
//! All Layers) is read per pass (`pass_rect_image`), walked by the RGB walk
//! itself (`precision_smudge::smudge_rect_walk`) in the layer's own
//! coordinates, never translated (the kernel's sub-steps,
//! `from + (to − from)·j / m`, are not translation-exact for a fractional
//! dab), and joined over `F`.
//!
//! Smudge's plan ([`PrecisionDocument::cmyk_smudge_rect_scratch`]) follows
//! its footprint: [`SMUDGE_RECT_BYTES_PER_PIXEL`] over the tiles `F` can
//! touch, the kernel's stage and the layer-space dabs at the 5/4 margin
//! (`precision_retouch_plan::margin`), plus bookkeeping and the canvas
//! display term.
//!
//! A coverage brush's request that names its `coverage_rect` plans by the
//! rect ([`PrecisionDocument::cmyk_rect_plan`]); one without it keeps the
//! by-layer bound (`cmyk_retouch_brush`'s module doc). The plan frame `G'`
//! ([`PrecisionDocument::cmyk_plan_frame`]) is the rect on the record grown
//! by the kind's reach and clipped to it, as `G` is the coverage's bbox
//! grown the same way, so `G ⊆ G'`: the stroke's coverage lies inside its
//! rect. The plan is one pass's RGB plan on a stand-in pass document over
//! `G'` (`cmyk_pass_stand_in_rect`; its input the request's rect, its output
//! terms `G'`'s) once per pass run, plus
//! [`PrecisionDocument::cmyk_rect_pass_scratch`]'s per-tile term over the
//! tiles `G'` can touch (the two passes' rect images, the join's read and the
//! rewritten ink tiles; `cmyk_retouch_brush::PASS_BYTES_PER_PIXEL`), the
//! canvas display term and bookkeeping; the History Brush adds the state's
//! two pass images under `G'` (16 B a pixel). Sample All Layers still adds
//! its canvas-sized picture once (`sampled_plate_plan`). Its margin over the
//! counted peak is held by `core/tests/rect_read_memory.rs`.
//!
//! [`PrecisionDocument::cmyk_brush_whole_layer`] is the tests' seam: a
//! stroke through layer-sized pass documents, the path before the rect
//! (`core/tests/cmyk_brush_rect.rs` holds the rect path equal to it).

use serde_json::{json, Value};

use crate::brush_coverage::{footprint, BrushRequest};
use crate::cmyk_adjust_layer::DISPLAY_BYTES_PER_PIXEL;
use crate::cmyk_passes::{
    join_rect, pass_document, pass_rect_image, pass_result, InkPass, BLANK_PASS,
};
use crate::cmyk_retouch_brush::{number, plate_of, HISTORY_SOURCE_REFUSAL, PASS_BYTES_PER_PIXEL};
use crate::doc_lock::{EditKind, LOCK_TRANSPARENCY};
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::layer_rect::{canvas_rect_on_plane, plane_rect_offset};
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{pixel_count, PrecisionImage, BOOKKEEPING_BYTES};
use crate::precision_paint_tools::NativePaintEdit;
use crate::precision_retouch_brush::RetouchBrushEdit;
use crate::precision_retouch_plan::{margin, rect_tiles, TILE_PIXELS};
use crate::precision_smudge::{
    check_smudge_strength, layer_dabs, smudge_rect_walk, SMUDGE_SELECTION_REFUSAL,
};
use crate::retouch_brush_sampling::clip_footprint;
use crate::retouch_coverage::{brush_rect, BrushCoverage, Rect};
use crate::retouch_smudge::smudge_stage_pixels;

/// Footprint-tile bytes of Smudge's rect walk: pass A's result 8, the
/// running pass's input 8, samples 8, field 16 and Sample All Layers'
/// starting field 16, the rewritten ink 10.
const SMUDGE_RECT_BYTES_PER_PIXEL: u64 = 66;

/// `rect` on a `(w, h)` record grown by `reach` on every side and clipped to
/// the record: the brush frame and the plan frame alike.
fn grown((x, y, rw, rh): Rect, reach: u32, (w, h): (u32, u32)) -> Rect {
    let (x0, y0) = (x.saturating_sub(reach), y.saturating_sub(reach));
    let x1 = (x + rw).saturating_add(reach).min(w);
    let y1 = (y + rh).saturating_add(reach).min(h);
    (x0, y0, x1 - x0, y1 - y0)
}

/// A stroke for [`PrecisionDocument::cmyk_brush_whole_layer`] (tests only).
#[doc(hidden)]
pub enum CmykBrushStroke<'a> {
    /// A `dodge_burn` or `retouch_brush` request of `dr_precision_doc_edit`
    /// and its input.
    Edit { request: &'a str, input: &'a [u16] },
    /// `dr_precision_doc_smudge`'s arguments, the request parsed here.
    Smudge {
        layer: usize,
        brush_request: &'a str,
        dabs: &'a [f64],
        strength: f64,
        selection: &'a [u16],
        sample_all_layers: bool,
    },
    /// `dr_precision_doc_history_brush`'s arguments.
    History {
        layer: usize,
        source: &'a PrecisionDocument,
        source_layer: usize,
        opacity: f64,
        input: &'a [u16],
        rect: Option<(u32, u32, u32, u32)>,
    },
}

impl PrecisionDocument {
    /// The whole-layer seam (tests only): `stroke` through layer-sized pass
    /// documents, the path before the rect, under the record's `Pixels`
    /// lock as its export takes it; no other bytes held, no job.
    #[doc(hidden)]
    pub fn cmyk_brush_whole_layer(
        &self,
        stroke: CmykBrushStroke<'_>,
        limit: u64,
    ) -> Result<Option<Self>, String> {
        let job = JobContext::NONE;
        match stroke {
            CmykBrushStroke::Edit { request, input } => {
                let op = serde_json::from_str::<Value>(request)
                    .ok()
                    .and_then(|v| v.get("op").and_then(Value::as_str).map(str::to_owned));
                if op.as_deref() == Some("retouch_brush") {
                    let edit = RetouchBrushEdit::parse(request)?;
                    self.under_locks(edit.layer, EditKind::Pixels, |doc| {
                        doc.cmyk_retouch_brush_in(&edit, input, limit, 0, job, true)
                    })
                } else {
                    // Dodge/Burn holds its own lock gate.
                    let edit = NativePaintEdit::parse(request)?;
                    self.cmyk_dodge_burn_in(&edit, input, limit, 0, job, true)
                }
            }
            CmykBrushStroke::Smudge {
                layer,
                brush_request,
                dabs,
                strength,
                selection,
                sample_all_layers,
            } => {
                let request = BrushRequest::parse(brush_request)?;
                self.under_locks(layer, EditKind::Pixels, |doc| {
                    doc.cmyk_smudge_in(
                        layer,
                        &request,
                        dabs,
                        strength,
                        selection,
                        sample_all_layers,
                        limit,
                        0,
                        job,
                        true,
                    )
                })
            }
            CmykBrushStroke::History {
                layer,
                source,
                source_layer,
                opacity,
                input,
                rect,
            } => self.under_locks(layer, EditKind::Pixels, |doc| {
                doc.cmyk_history_brush_in(
                    layer,
                    source,
                    source_layer,
                    opacity,
                    input,
                    rect,
                    limit,
                    0,
                    job,
                    true,
                )
            }),
        }
    }

    /// The brush frame `G` of CMYK raster `layer` (module doc): the bounding
    /// box of the stroke's `coverage` on the record, grown by `reach` on
    /// every side and clipped to the record; `(0, 0, 1, 1)` when the stroke
    /// misses the record.
    pub(crate) fn cmyk_brush_frame(
        &self,
        layer: usize,
        coverage: &BrushCoverage<'_>,
        reach: u32,
    ) -> Result<Rect, String> {
        let ((w, h), offset) = self.cmyk_brush_target(layer)?;
        Ok(coverage
            .layer_rect((w, h), offset)
            .map_or((0, 0, 1, 1), |bbox| grown(bbox, reach, (w, h))))
    }

    /// The plan frame `G'` of CMYK raster `layer` (module doc): the canvas
    /// `rect` on the record (and the canvas), grown by `reach` and clipped to
    /// the record; `(0, 0, 1, 1)` when it misses the record.
    pub(crate) fn cmyk_plan_frame(
        &self,
        layer: usize,
        rect: Rect,
        reach: u32,
    ) -> Result<Rect, String> {
        let ((w, h), offset) = self.cmyk_brush_target(layer)?;
        let (x, y) = (i64::from(rect.0), i64::from(rect.1));
        let ends = (x, y, x + i64::from(rect.2), y + i64::from(rect.3));
        Ok(
            canvas_rect_on_plane(self.dimensions(), (w, h), offset, ends)
                .map_or((0, 0, 1, 1), |part| grown(part, reach, (w, h))),
        )
    }

    /// A stand-in pass document of CMYK raster `layer` over its rect `frame`
    /// for a plan: the frame's size at its canvas position, the record's
    /// transparency lock, no samples.
    pub(crate) fn cmyk_pass_stand_in_rect(
        &self,
        layer: usize,
        frame: Rect,
    ) -> Result<PrecisionDocument, String> {
        let (_, offset) = self.cmyk_brush_target(layer)?;
        let locks = if self.lock_owner(layer, LOCK_TRANSPARENCY).is_some() {
            LOCK_TRANSPARENCY
        } else {
            0
        };
        let image =
            PrecisionImage::filled_rgba16(frame.2, frame.3, BLANK_PASS, IccProfile::srgb())?;
        pass_document(
            self.dimensions(),
            image,
            plane_rect_offset(offset, frame),
            locks,
        )
    }

    /// The rect plans' scratch (module doc): `rgb` (one pass's RGB plan)
    /// run `passes` times, plus `t·PASS_BYTES_PER_PIXEL`,
    /// canvas·`DISPLAY_BYTES_PER_PIXEL` and `2·BOOKKEEPING_BYTES`, with `t`
    /// the pixels of the tiles `frame` can touch (`rect_tiles`).
    pub(crate) fn cmyk_rect_pass_scratch(
        &self,
        frame: Rect,
        rgb: u64,
        passes: u64,
    ) -> Result<u64, String> {
        let (cw, ch) = self.dimensions();
        let canvas = pixel_count(cw, ch)? as u64;
        let tiles = rect_tiles(frame.2, frame.3) * TILE_PIXELS;
        rgb.checked_mul(passes)
            .and_then(|v| v.checked_add(tiles.checked_mul(PASS_BYTES_PER_PIXEL)?))
            .and_then(|v| v.checked_add(canvas.checked_mul(DISPLAY_BYTES_PER_PIXEL)?))
            .and_then(|v| v.checked_add(2 * BOOKKEEPING_BYTES))
            .ok_or_else(|| "CMYK retouch scratch overflow".to_string())
    }

    /// A coverage brush's plan on CMYK raster `layer` over its request's
    /// `coverage_rect` (module doc): `pass_plan` (one pass's RGB plan) on the
    /// stand-in over the plan frame of `rect` grown by `reach`, run `passes`
    /// times, restated by [`Self::cmyk_rect_pass_scratch`], plus `per_frame`
    /// bytes a frame pixel; the document's retained bytes.
    pub(crate) fn cmyk_rect_plan(
        &self,
        layer: usize,
        rect: [u32; 4],
        reach: u32,
        passes: u64,
        per_frame: u64,
        pass_plan: impl FnOnce(&PrecisionDocument) -> Result<Value, String>,
    ) -> Result<Value, String> {
        let rect = brush_rect(Some(rect), self.dimensions())?;
        let frame = self.cmyk_plan_frame(layer, rect, reach)?;
        let mut plan = pass_plan(&self.cmyk_pass_stand_in_rect(layer, frame)?)?;
        let rgb = number(&plan, "scratch_bytes")?;
        let held = (u64::from(frame.2) * u64::from(frame.3))
            .checked_mul(per_frame)
            .ok_or("CMYK retouch scratch overflow")?;
        let scratch = self
            .cmyk_rect_pass_scratch(frame, rgb, passes)?
            .checked_add(held)
            .ok_or("CMYK retouch scratch overflow")?;
        plan["retained_bytes"] = json!(self.retained_bytes());
        plan["scratch_bytes"] = json!(scratch);
        Ok(plan)
    }

    /// `run` on CMYK raster `layer`'s pass A and, when `k`, pass B over its
    /// rect `frame` (module doc), then the join over `frame`: the rect twin of
    /// `cmyk_two_pass`, with its job ranges and `other` terms (the pass
    /// documents' bytes are the frame's). Admission is the caller's.
    pub(crate) fn cmyk_two_pass_rect(
        &self,
        layer: usize,
        k: bool,
        frame: Rect,
        other: u64,
        job: JobContext<'_>,
        run: impl Fn(
            InkPass,
            &PrecisionDocument,
            u64,
            JobContext<'_>,
        ) -> Result<Option<PrecisionDocument>, String>,
    ) -> Result<Option<Self>, String> {
        let record = self
            .layer(layer)
            .ok_or("high-depth layer index is out of range")?;
        let image = record
            .cmyk_source()
            .ok_or("a CMYK edit requires a CMYK raster layer")?;
        let offset = record.offset();
        let frozen = self.lock_owner(layer, LOCK_TRANSPARENCY).is_some();
        let locks = if frozen { LOCK_TRANSPARENCY } else { 0 };
        let canvas = self.dimensions();
        let at = plane_rect_offset(offset, frame);
        let g = u64::from(frame.2) * u64::from(frame.3);
        let held = self.retained_bytes().saturating_add(other);
        let pass = |ink: InkPass, range: JobContext<'_>| -> Result<PrecisionDocument, String> {
            pass_document(
                canvas,
                pass_rect_image(image, ink, false, frame, range)?,
                at,
                locks,
            )
        };
        let a_range = job.range(0.0, if k { 0.45 } else { 0.9 });
        let a_doc = pass(InkPass::Cmy, a_range)?;
        let a_out = run(InkPass::Cmy, &a_doc, held + g * 8, a_range)?;
        let b = if k {
            let b_range = job.range(0.45, 0.9);
            let b_doc = pass(InkPass::K, b_range)?;
            let b_out = run(InkPass::K, &b_doc, held + g * 16, b_range)?;
            Some((b_doc, b_out))
        } else {
            None
        };
        if a_out.is_none() && b.as_ref().is_none_or(|(_, out)| out.is_none()) {
            job.report(1.0, 5)?;
            return Ok(None);
        }
        let a = pass_result(a_out.as_ref().unwrap_or(&a_doc))?;
        let b = b
            .as_ref()
            .map(|(doc, out)| pass_result(out.as_ref().unwrap_or(doc)))
            .transpose()?;
        let out = match join_rect(image, frame, a, b, frozen, job.range(0.9, 1.0))? {
            Some(image) => self.replacing_content(layer, Content::CmykRaster { image, offset })?,
            None => None,
        };
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// The history state's two pass documents `[Cmy, K]` for target `layer`:
    /// the whole source layer (`frame` `None`, the whole-layer path), or its
    /// part under the target's rect `frame` (module doc; one `BLANK_PASS`
    /// pixel at the canvas origin when the source misses it).
    pub(crate) fn cmyk_history_sources(
        &self,
        layer: usize,
        source: &PrecisionDocument,
        source_layer: usize,
        frame: Option<Rect>,
        job: JobContext<'_>,
    ) -> Result<[PrecisionDocument; 2], String> {
        let Some(frame) = frame else {
            return Ok([
                source.cmyk_pass_document(source_layer, InkPass::Cmy, false)?,
                source.cmyk_pass_document(source_layer, InkPass::K, false)?,
            ]);
        };
        let (_, offset) = self.cmyk_brush_target(layer)?;
        let record = source
            .layer(source_layer)
            .ok_or("history brush source layer is out of range")?;
        let image = record.cmyk_source().ok_or(HISTORY_SOURCE_REFUSAL)?;
        let canvas = self.dimensions();
        let (x, y) = plane_rect_offset(offset, frame);
        let (x, y) = (i64::from(x), i64::from(y));
        let under = (x, y, x + i64::from(frame.2), y + i64::from(frame.3));
        let part = canvas_rect_on_plane(canvas, image.dimensions(), record.offset(), under);
        let pass = |ink: InkPass| -> Result<PrecisionDocument, String> {
            match part {
                Some(part) => pass_document(
                    canvas,
                    pass_rect_image(image, ink, false, part, job)?,
                    plane_rect_offset(record.offset(), part),
                    0,
                ),
                None => pass_document(
                    canvas,
                    PrecisionImage::filled_rgba16(1, 1, BLANK_PASS, IccProfile::srgb())?,
                    (0, 0),
                    0,
                ),
            }
        };
        Ok([pass(InkPass::Cmy)?, pass(InkPass::K)?])
    }

    /// Smudge's rect path on CMYK raster `layer` (module doc), after the
    /// caller's plan, admission, reservation and `plates` (Sample All
    /// Layers' `[Cmy, K]` pictures): refuses the strength, then the
    /// selection's length, in the RGB walk's sentences; `Ok(None)` when no
    /// stored code changes.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cmyk_smudge_rect(
        &self,
        layer: usize,
        request: &BrushRequest,
        dabs: &[f64],
        strength: f64,
        selection: &[u16],
        plates: Option<&[PrecisionImage; 2]>,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        check_smudge_strength(strength)?;
        let canvas = self.dimensions();
        if !selection.is_empty() && selection.len() != pixel_count(canvas.0, canvas.1)? {
            return Err(SMUDGE_SELECTION_REFUSAL.into());
        }
        let record = self
            .layer(layer)
            .ok_or("high-depth layer index is out of range")?;
        let image = record
            .cmyk_source()
            .ok_or("a CMYK edit requires a CMYK raster layer")?;
        let offset = record.offset();
        let local = layer_dabs(dabs, offset)?;
        // Under Sample All Layers the footprint is clipped to the layer ∩
        // canvas, as the RGB walk clips it.
        let rect = footprint(image.dimensions(), &local).and_then(|f| match plates {
            Some(_) => clip_footprint(f, offset, canvas),
            None => Some(f),
        });
        let Some(f) = rect else {
            job.report(1., 5)?;
            return Ok(None);
        };
        if strength == 0. {
            job.report(1., 5)?;
            return Ok(None);
        }
        let r = (
            f[0] as u32,
            f[1] as u32,
            (f[2] - f[0]) as u32,
            (f[3] - f[1]) as u32,
        );
        let frozen = self.lock_owner(layer, LOCK_TRANSPARENCY).is_some();
        let mut passes = Vec::with_capacity(2);
        let mut changed = false;
        for (k, ink) in [InkPass::Cmy, InkPass::K].into_iter().enumerate() {
            let range = job.range(0.45 * k as f64, 0.45 * (k + 1) as f64);
            let pass = pass_rect_image(image, ink, false, r, range)?;
            let mut px = pass
                .read_rect_rgba16(0, 0, r.2, r.3)
                .ok_or("could not allocate precision output")?;
            let plate = plates.map(|p| plate_of(p, ink));
            let moved = smudge_rect_walk(
                &mut px,
                f,
                (offset, canvas),
                request,
                &local,
                strength,
                selection,
                plate,
                frozen,
                range,
            )?;
            let result = if moved {
                pass.with_rect_rgba16(0, 0, r.2, r.3, &px)?
            } else {
                None
            };
            changed |= result.is_some();
            passes.push(result.unwrap_or(pass));
        }
        if !changed {
            job.report(1., 5)?;
            return Ok(None);
        }
        let b = passes
            .pop()
            .ok_or("internal error: a CMYK pass is missing")?;
        let a = passes
            .pop()
            .ok_or("internal error: a CMYK pass is missing")?;
        let out = match join_rect(image, r, &a, Some(&b), frozen, job.range(0.9, 1.0))? {
            Some(image) => self.replacing_content(layer, Content::CmykRaster { image, offset })?,
            None => None,
        };
        job.report(1., 5)?;
        Ok(out)
    }

    /// Smudge's rect-path scratch (module doc): `2·BOOKKEEPING_BYTES` +
    /// margin(`t`·[`SMUDGE_RECT_BYTES_PER_PIXEL`] + stage·16 + dabs·8) +
    /// canvas·`DISPLAY_BYTES_PER_PIXEL`, with `F` the footprint on the layer
    /// (unclipped, an upper bound on the clipped one), `t` the pixels of the
    /// tiles `F` can touch (`rect_tiles`) and the stage
    /// `smudge_stage_pixels` of `F`.
    pub(crate) fn cmyk_smudge_rect_scratch(
        &self,
        layer: usize,
        dabs: &[f64],
    ) -> Result<u64, String> {
        let ((w, h), offset) = self.cmyk_brush_target(layer)?;
        let (cw, ch) = self.dimensions();
        let canvas = pixel_count(cw, ch)? as u64;
        let local = layer_dabs(dabs, offset)?;
        let (fw, fh) = footprint((w, h), &local)
            .map_or((0, 0), |f| ((f[2] - f[0]) as u32, (f[3] - f[1]) as u32));
        let tiles = rect_tiles(fw, fh) * TILE_PIXELS;
        let stage = smudge_stage_pixels(u64::from(fw) * u64::from(fh), &local);
        let overflow = || "CMYK retouch scratch overflow".to_string();
        let walk = tiles
            .checked_mul(SMUDGE_RECT_BYTES_PER_PIXEL)
            .and_then(|v| v.checked_add(stage.checked_mul(16)?))
            .and_then(|v| v.checked_add((dabs.len() as u64).checked_mul(8)?))
            .ok_or_else(overflow)?;
        margin(walk)
            .checked_add(2 * BOOKKEEPING_BYTES)
            .and_then(|v| v.checked_add(canvas.checked_mul(DISPLAY_BYTES_PER_PIXEL)?))
            .ok_or_else(overflow)
    }
}
