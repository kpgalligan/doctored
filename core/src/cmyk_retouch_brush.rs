//! The retouch brushes on a CMYK document's raster records (#1): Dodge and
//! Burn, the Blur, Sharpen and Sponge brushes, Smudge and the History Brush,
//! each the shipped 16-bit RGB walk run on the record's `L` passes
//! (`cmyk_passes`) and joined back into ink. Every entry point is reached
//! from the RGB one it mirrors, which hands a CMYK document here at its top.
//!
//! * **Dodge and Burn** (DB-P4, the take's H0): both passes, so the curve
//!   (`doc_retouch::shaped`) runs on `L` of all four plates. Under Protect
//!   Tones the group rule (`doc_retouch::protected`) holds each pass as one
//!   group: `[L_c, L_m, L_y]` keeps its ratios, and `[L_k; 3]` is a group of
//!   equal values, whose protected curve is the plain one.
//! * **Blur and Sharpen**: both passes.
//! * **Sponge** (SP-C3K, the take's H0): pass A only; the K plate is held
//!   exactly, so the sponge moves the chromatic inks and never the black.
//! * **Smudge**: both passes, the same dabs.
//! * **Vibrance**: the Sponge's Vibrance target runs on pass A's
//!   `[L_c, L_m, L_y]` through the sRGB curve (`retouch_brush::Vibrancy::
//!   Encoded`, the pass edit's `ink_pass`), K held.
//! * **Sample All Layers**: Blur, Sharpen and Smudge read the sampled
//!   ink picture (`retouch_brush_sampling::cmyk_sampled_passes`, rendered once
//!   and split into the two passes), each pass reading its own pass of it;
//!   the passes' alphas agree because both mix the same layer alpha toward
//!   the same picture alpha by the same coverage.
//! * **History Brush**: both passes, the history state's layer split by pass
//!   too. A state that is not a CMYK document with a CMYK raster at the
//!   source index refuses ([`HISTORY_SOURCE_REFUSAL`]).
//!
//! Each runs under the CMYK record's `Pixels` lock. A transparency-locked
//! record's passes carry the lock, so the RGB walk keeps alpha exactly as it
//! does on an RGB layer, and the join keeps the source's alpha besides.
//!
//! The passes run over the stroke's rect on the record, Smudge's over its
//! footprint (13B-9, `cmyk_brush_rect`); each `_in` body's `whole_layer`
//! keeps layer-sized pass documents for that module's test seam.
//!
//! A coverage brush's request (Dodge/Burn, Blur, Sharpen, Sponge, the History
//! Brush) that names its `coverage_rect` plans the passes over that rect
//! (`cmyk_brush_rect`'s plan frame). One without it keeps the by-layer
//! bound: the RGB plan of one pass (on a stand-in pass document with the
//! record's size and offset, which the plan reads for its geometry only)
//! once per pass run, plus layer px × 34 (pass A's input 8 and held output
//! 8, pass B's input 8, the CMYK output 10; the join's rect read, 10 B a
//! rect pixel, sits inside the second pass's idle RGB plan), canvas ×
//! `DISPLAY_BYTES_PER_PIXEL` (the host's display preparation) and
//! bookkeeping, `cmyk_filter`'s accounting; the History Brush adds the
//! history state's two pass images (source px × 16). Smudge plans its
//! footprint (`cmyk_brush_rect`). Sample All Layers adds the picture once
//! (`sampled_plate_plan`: `cmyk_plate_scratch` plus its two passes, canvas ×
//! 16), and each coverage brush's pass is admitted against `other` plus the
//! two passes' bytes. Measured on the by-layer plans by
//! `scripts/cmyk-admission/measure.py` (`dodge-16`, `smudge-16`,
//! `history-brush-16`; `blur-sal-16`, plan 7,366,605,728 against max E
//! 2,281,113,400, and `smudge-sal-16`, plan 5,310,313,888 against max E
//! 1,464,435,960, the 24 MP 16-bit CMYK TIFF, both 1.25 E inside the plan);
//! the rect plans of `dodge-16`, `history-brush-16` and `blur-sal-16` hold
//! their margin by allocator count only (`core/tests/rect_read_memory.rs`).

use serde_json::{json, Value};

use crate::brush_coverage::BrushRequest;
use crate::cmyk_adjust_layer::DISPLAY_BYTES_PER_PIXEL;
use crate::cmyk_brush_rect::dodge_pass;
use crate::cmyk_passes::{pass_document, InkPass, BLANK_PASS};
use crate::doc_lock::EditKind;
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, pixel_count, PrecisionImage, BOOKKEEPING_BYTES};
use crate::precision_paint_tools::NativePaintEdit;
use crate::precision_retouch_brush::RetouchBrushEdit;
use crate::retouch_brush::{halo, RetouchBrushKind};
use crate::retouch_coverage::{brush_rect, BrushCoverage, Rect};
use crate::retouch_transform::StorageExtent;

/// The History Brush's refusal of a state it cannot paint from on CMYK.
pub(crate) const HISTORY_SOURCE_REFUSAL: &str =
    "The History Brush's source state is not a CMYK raster";

/// The per-layer-pixel term of every two-pass plan (module doc), and the
/// per-tile-pixel term of the rect plans (`cmyk_brush_rect`).
pub(crate) const PASS_BYTES_PER_PIXEL: u64 = 34;

/// Whether a retouch brush runs pass B (Sponge holds K; module doc).
fn runs_k(kind: RetouchBrushKind) -> bool {
    matches!(kind, RetouchBrushKind::Blur | RetouchBrushKind::Sharpen)
}

/// Pass `ink`'s picture of Sample All Layers' `[Cmy, K]` pair.
pub(crate) fn plate_of(plates: &[PrecisionImage; 2], ink: InkPass) -> &PrecisionImage {
    match ink {
        InkPass::Cmy => &plates[0],
        InkPass::K => &plates[1],
    }
}

/// A number of a plan, or the plan's own refusal.
pub(crate) fn number(plan: &Value, key: &str) -> Result<u64, String> {
    plan[key]
        .as_u64()
        .ok_or_else(|| "invalid CMYK retouch plan".to_string())
}

impl PrecisionDocument {
    /// CMYK raster `layer`'s image size and offset, refused by name for any
    /// other record.
    pub(crate) fn cmyk_brush_target(&self, layer: usize) -> Result<StorageExtent, String> {
        let record = self
            .layer(layer)
            .ok_or("native paint target is out of range")?;
        let image = record
            .cmyk_source()
            .ok_or("CMYK paint requires a CMYK raster layer")?;
        Ok((image.dimensions(), record.offset()))
    }

    /// A two-pass plan's scratch (module doc): `rgb` is one pass's RGB
    /// scratch, run `passes` times.
    pub(crate) fn cmyk_two_pass_scratch(
        &self,
        layer: usize,
        rgb: u64,
        passes: u64,
    ) -> Result<u64, String> {
        let ((w, h), _) = self.cmyk_brush_target(layer)?;
        let (cw, ch) = self.dimensions();
        let pixels = pixel_count(w, h)? as u64;
        let canvas = pixel_count(cw, ch)? as u64;
        rgb.checked_mul(passes)
            .and_then(|v| v.checked_add(pixels.checked_mul(PASS_BYTES_PER_PIXEL)?))
            .and_then(|v| v.checked_add(canvas.checked_mul(DISPLAY_BYTES_PER_PIXEL)?))
            .and_then(|v| v.checked_add(2 * BOOKKEEPING_BYTES))
            .ok_or_else(|| "CMYK retouch scratch overflow".to_string())
    }

    /// `plan` (one pass's RGB plan) restated for the CMYK record: the
    /// document's retained bytes and the two-pass scratch.
    fn cmyk_two_pass_plan(
        &self,
        layer: usize,
        mut plan: Value,
        passes: u64,
        extra: u64,
    ) -> Result<Value, String> {
        let scratch = self.cmyk_two_pass_scratch(layer, number(&plan, "scratch_bytes")?, passes)?;
        plan["retained_bytes"] = json!(self.retained_bytes());
        plan["scratch_bytes"] = json!(scratch
            .checked_add(extra)
            .ok_or("CMYK retouch scratch overflow")?);
        Ok(plan)
    }

    /// Runs `run` on CMYK raster `layer`'s pass A and, when `k`, pass B,
    /// then joins them (module doc). `run(pass, document, other, job)` is the
    /// RGB walk on a pass document. `frame` is the stroke's rect on the
    /// record (`cmyk_brush_rect`'s pass documents and join); `None` runs
    /// whole-layer pass documents. Admission is the caller's.
    fn cmyk_two_pass(
        &self,
        layer: usize,
        k: bool,
        frame: Option<Rect>,
        other: u64,
        job: JobContext<'_>,
        run: impl Fn(
            InkPass,
            &PrecisionDocument,
            u64,
            JobContext<'_>,
        ) -> Result<Option<PrecisionDocument>, String>,
    ) -> Result<Option<Self>, String> {
        if let Some(frame) = frame {
            return self.cmyk_two_pass_rect(layer, k, frame, other, job, run);
        }
        let ((w, h), _) = self.cmyk_brush_target(layer)?;
        let n = pixel_count(w, h)? as u64;
        let held = self.retained_bytes().saturating_add(other);
        let a_doc = self.cmyk_pass_document(layer, InkPass::Cmy, false)?;
        let a_out = run(
            InkPass::Cmy,
            &a_doc,
            held + n * 8,
            job.range(0.0, if k { 0.45 } else { 0.9 }),
        )?;
        let b = if k {
            let b_doc = self.cmyk_pass_document(layer, InkPass::K, false)?;
            let b_out = run(InkPass::K, &b_doc, held + n * 16, job.range(0.45, 0.9))?;
            Some((b_doc, b_out))
        } else {
            None
        };
        if a_out.is_none() && b.as_ref().is_none_or(|(_, out)| out.is_none()) {
            job.report(1.0, 5)?;
            return Ok(None);
        }
        let a = a_out.as_ref().unwrap_or(&a_doc);
        let b = b.as_ref().map(|(doc, out)| out.as_ref().unwrap_or(doc));
        let out = self.cmyk_joined(layer, a, b, job.range(0.9, 1.0))?;
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// Dodge/Burn's plan on CMYK (module doc).
    pub(crate) fn cmyk_dodge_burn_plan(&self, edit: &NativePaintEdit) -> Result<Value, String> {
        let (pass, rect) = dodge_pass(edit)?;
        let layer = edit.target().0;
        if let Some(rect) = rect {
            return self.cmyk_rect_plan(layer, rect, 0, 2, 0, |stand| pass.plan(stand));
        }
        let plan = pass.plan(&self.cmyk_pass_stand_in(layer)?)?;
        self.cmyk_two_pass_plan(layer, plan, 2, 0)
    }

    /// Dodge/Burn on CMYK raster `edit`'s layer (module doc).
    pub(crate) fn cmyk_dodge_burn(
        &self,
        edit: &NativePaintEdit,
        input: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        self.cmyk_dodge_burn_in(edit, input, limit, other, job, false)
    }

    /// [`Self::cmyk_dodge_burn`] over the stroke's rect, or over whole-layer
    /// pass documents when `whole_layer` (`cmyk_brush_rect`'s seam): admits
    /// the plan and runs the passes under the record's `Pixels` lock.
    pub(crate) fn cmyk_dodge_burn_in(
        &self,
        edit: &NativePaintEdit,
        input: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
        whole_layer: bool,
    ) -> Result<Option<Self>, String> {
        let plan = self.cmyk_dodge_burn_plan(edit)?;
        if input.len() as u64 != number(&plan, "input_samples")? {
            return Err("native paint input length differs from plan".into());
        }
        let (pass, rect) = dodge_pass(edit)?;
        let (layer, .., selected) = edit.target();
        let scratch = number(&plan, "scratch_bytes")?;
        let canvas = self.dimensions();
        let coverage = BrushCoverage::new(input, brush_rect(rect, canvas)?, canvas, selected)?;
        self.under_locks(layer, EditKind::Pixels, |doc| {
            admit(doc.retained_bytes(), scratch, other, limit)?;
            job.reserve(scratch)?;
            let job = job.with_reservation_floor(scratch);
            let frame = (!whole_layer)
                .then(|| doc.cmyk_brush_frame(layer, &coverage, 0))
                .transpose()?;
            doc.cmyk_two_pass(layer, true, frame, other, job, |_, doc, other, job| {
                pass.apply(doc, input, limit, other, job)
            })
        })
    }

    /// Blur, Sharpen or Sponge's walk plan on CMYK (module doc): the passes'
    /// plans, taken without Sample All Layers (`RetouchBrushEdit::plan` adds
    /// the sampled picture once).
    pub(crate) fn cmyk_retouch_brush_plan(&self, edit: &RetouchBrushEdit) -> Result<Value, String> {
        let mut pass = edit.clone();
        pass.layer = 0;
        pass.sample_all_layers = false;
        pass.ink_pass = true;
        let passes = if runs_k(edit.kind) { 2 } else { 1 };
        if let Some(rect) = edit.coverage_rect {
            let reach = halo(edit.kind);
            return self.cmyk_rect_plan(edit.layer, rect, reach, passes, 0, |s| pass.plan(s));
        }
        let plan = pass.plan(&self.cmyk_pass_stand_in(edit.layer)?)?;
        self.cmyk_two_pass_plan(edit.layer, plan, passes, 0)
    }

    /// Blur, Sharpen or Sponge on CMYK, inside the record's lock gate
    /// (`RetouchBrushEdit::apply` holds it).
    pub(crate) fn cmyk_retouch_brush_unlocked(
        &self,
        edit: &RetouchBrushEdit,
        input: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        self.cmyk_retouch_brush_in(edit, input, limit, other, job, false)
    }

    /// [`Self::cmyk_retouch_brush_unlocked`] over the stroke's rect grown by
    /// the kind's halo, or over whole-layer pass documents when
    /// `whole_layer` (`cmyk_brush_rect`'s seam).
    pub(crate) fn cmyk_retouch_brush_in(
        &self,
        edit: &RetouchBrushEdit,
        input: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
        whole_layer: bool,
    ) -> Result<Option<Self>, String> {
        let plan = edit.plan(self)?;
        if input.len() as u64 != number(&plan, "input_samples")? {
            return Err("native paint input length differs from plan".into());
        }
        let scratch = number(&plan, "scratch_bytes")?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let job = job.with_reservation_floor(scratch);
        let mut pass = edit.clone();
        pass.layer = 0;
        pass.sample_all_layers = false;
        pass.ink_pass = true;
        let plates = self.cmyk_plates(edit.layer, edit.sample_all_layers, job)?;
        let frame = if whole_layer {
            None
        } else {
            let canvas = self.dimensions();
            let rect = brush_rect(edit.coverage_rect, canvas)?;
            let coverage = BrushCoverage::new(input, rect, canvas, edit.selection)?;
            Some(self.cmyk_brush_frame(edit.layer, &coverage, halo(edit.kind))?)
        };
        let Some((plates, held)) = plates else {
            return self.cmyk_two_pass(
                edit.layer,
                runs_k(edit.kind),
                frame,
                other,
                job,
                |_, doc, other, job| pass.apply(doc, input, limit, other, job),
            );
        };
        self.cmyk_two_pass(
            edit.layer,
            runs_k(edit.kind),
            frame,
            other.saturating_add(held),
            job.with_held_bytes(held),
            |ink, doc, other, job| {
                pass.apply_sampled(doc, input, limit, other, job, plate_of(&plates, ink))
            },
        )
    }

    /// Sample All Layers' two pass pictures (`cmyk_sampled_passes`) and
    /// their bytes when `sample_all_layers`, else `None`; the caller has
    /// admitted their term (`sampled_plate_plan`).
    fn cmyk_plates(
        &self,
        layer: usize,
        sample_all_layers: bool,
        job: JobContext<'_>,
    ) -> Result<Option<([PrecisionImage; 2], u64)>, String> {
        if !sample_all_layers {
            return Ok(None);
        }
        let plates = self.cmyk_sampled_passes(layer, job.checkpoints_only())?;
        let held = plates[0]
            .retained_bytes()
            .saturating_add(plates[1].retained_bytes());
        Ok(Some((plates, held)))
    }

    /// Smudge's plan on CMYK (module doc).
    pub(crate) fn cmyk_smudge_plan(
        &self,
        layer: usize,
        request: &BrushRequest,
        dabs: &[f64],
        selection: bool,
        sample_all_layers: bool,
    ) -> Result<Value, String> {
        // The stand-in's plan for its refusals and `input_samples`; the
        // scratch is the rect walk's (`cmyk_brush_rect`).
        let mut plan = self
            .cmyk_pass_stand_in(layer)?
            .smudge_plan(0, request, dabs, selection, false)?;
        let plate = if sample_all_layers {
            self.sampled_plate_plan(layer)?
        } else {
            0
        };
        let scratch = self
            .cmyk_smudge_rect_scratch(layer, dabs)?
            .checked_add(plate)
            .ok_or("CMYK retouch scratch overflow")?;
        plan["retained_bytes"] = json!(self.retained_bytes());
        plan["scratch_bytes"] = json!(scratch);
        Ok(plan)
    }

    /// Smudge on CMYK, inside the record's lock gate
    /// (`PrecisionDocument::smudge` holds it).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cmyk_smudge_unlocked(
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
        self.cmyk_smudge_in(
            layer,
            request,
            dabs,
            strength,
            selection,
            sample_all_layers,
            limit,
            other,
            job,
            false,
        )
    }

    /// [`Self::cmyk_smudge_unlocked`] over the stroke's footprint
    /// (`cmyk_brush_rect`), or over whole-layer pass documents when
    /// `whole_layer` (its seam).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cmyk_smudge_in(
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
        whole_layer: bool,
    ) -> Result<Option<Self>, String> {
        let plan = self.cmyk_smudge_plan(
            layer,
            request,
            dabs,
            !selection.is_empty(),
            sample_all_layers,
        )?;
        let scratch = number(&plan, "scratch_bytes")?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let job = job.with_reservation_floor(scratch);
        let plates = self.cmyk_plates(layer, sample_all_layers, job)?;
        if !whole_layer {
            let plates = plates.as_ref().map(|(plates, _)| plates);
            return self.cmyk_smudge_rect(layer, request, dabs, strength, selection, plates, job);
        }
        let Some((plates, held)) = plates else {
            return self.cmyk_two_pass(layer, true, None, other, job, |_, doc, other, job| {
                doc.smudge(
                    0, request, dabs, strength, selection, false, limit, other, job,
                )
            });
        };
        self.cmyk_two_pass(
            layer,
            true,
            None,
            other.saturating_add(held),
            job.with_held_bytes(held),
            |ink, doc, other, job| {
                let plate = plate_of(&plates, ink);
                doc.smudge_sampled(
                    0, request, dabs, strength, selection, plate, limit, other, job,
                )
            },
        )
    }

    /// The history state's CMYK raster `source_layer` (module doc): its
    /// size and offset.
    fn cmyk_history_source(
        &self,
        source: &PrecisionDocument,
        source_layer: usize,
    ) -> Result<StorageExtent, String> {
        if !source.is_cmyk() {
            return Err(HISTORY_SOURCE_REFUSAL.into());
        }
        if source.dimensions() != self.dimensions() {
            return Err("history brush source has a different canvas size".into());
        }
        let record = source
            .layer(source_layer)
            .ok_or("history brush source layer is out of range")?;
        let image = record.cmyk_source().ok_or(HISTORY_SOURCE_REFUSAL)?;
        Ok((image.dimensions(), record.offset()))
    }

    /// The History Brush's plan on CMYK (module doc); over `rect`, the state's
    /// two pass images are the frame's (16 B a frame pixel).
    pub(crate) fn cmyk_history_brush_plan(
        &self,
        layer: usize,
        source: &PrecisionDocument,
        source_layer: usize,
        selection: bool,
        rect: Option<Rect>,
    ) -> Result<Value, String> {
        let ((sw, sh), soffset) = self.cmyk_history_source(source, source_layer)?;
        if let Some(rect) = rect {
            // The stand-in is its own state (the plan reads only its size).
            return self.cmyk_rect_plan(layer, rect.into(), 0, 2, 16, |stand| {
                stand.history_brush_plan(0, stand, 0, selection, Some(rect))
            });
        }
        let image = PrecisionImage::filled_rgba16(sw, sh, BLANK_PASS, IccProfile::srgb())?;
        let stand_source = pass_document(self.dimensions(), image, soffset, 0)?;
        let stand = self.cmyk_pass_stand_in(layer)?;
        let plan = stand.history_brush_plan(0, &stand_source, 0, selection, None)?;
        let source_pixels = pixel_count(sw, sh)? as u64;
        self.cmyk_two_pass_plan(layer, plan, 2, source_pixels * 16)
    }

    /// The History Brush on CMYK, inside the record's lock gate
    /// (`PrecisionDocument::history_brush` holds it).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cmyk_history_brush_unlocked(
        &self,
        index: usize,
        source: &PrecisionDocument,
        source_layer: usize,
        opacity: f64,
        input: &[u16],
        rect: Option<Rect>,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        self.cmyk_history_brush_in(
            index,
            source,
            source_layer,
            opacity,
            input,
            rect,
            limit,
            other,
            job,
            false,
        )
    }

    /// [`Self::cmyk_history_brush_unlocked`] over the stroke's rect and the
    /// state's part under it (`cmyk_brush_rect`), or over whole-layer pass
    /// documents when `whole_layer` (its seam). `input` is laid out over
    /// `rect` (the canvas when `None`), which every pass keeps.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cmyk_history_brush_in(
        &self,
        index: usize,
        source: &PrecisionDocument,
        source_layer: usize,
        opacity: f64,
        input: &[u16],
        rect: Option<Rect>,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
        whole_layer: bool,
    ) -> Result<Option<Self>, String> {
        let area = crate::precision_retouch_brush::history_rect(self, rect)?;
        let selected = crate::precision_retouch_brush::history_selected(input, area)?;
        let plan = self.cmyk_history_brush_plan(index, source, source_layer, selected, rect)?;
        let scratch = number(&plan, "scratch_bytes")?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let job = job.with_reservation_floor(scratch);
        let frame = if whole_layer {
            None
        } else {
            let coverage = BrushCoverage::new(input, area, self.dimensions(), selected)?;
            Some(self.cmyk_brush_frame(index, &coverage, 0)?)
        };
        let sources = self.cmyk_history_sources(index, source, source_layer, frame, job)?;
        self.cmyk_two_pass(index, true, frame, other, job, |pass, doc, other, job| {
            let from = match pass {
                InkPass::Cmy => &sources[0],
                InkPass::K => &sources[1],
            };
            doc.history_brush(0, from, 0, opacity, input, rect, limit, other, job)
        })
    }
}
