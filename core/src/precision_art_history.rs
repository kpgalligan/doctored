//! The Art History Brush on the typed document: its plan, its lock
//! gate and its refusals by depth, dispatching to the CMYK walk
//! (`cmyk_art_history`) and the u16-record walk (`gray_art_history`) of an
//! 8-bit Gray document (#1 phase 2e, `GRAY_ART`) and an 8-bit Lab Color
//! one (#1 Lab 2f, third part, `lab_retouch::LAB_ART`).
//!
//! The tool runs only on a document of 8 bits per channel: the typed
//! document runs only an 8-bit CMYK document's ink, an 8-bit Gray
//! document's R = G = B and an 8-bit Lab document's L\*a\*b\* codes (8-bit
//! RGB is the byte document, `doc_art_history`), and every other depth
//! refuses by name (a 16-bit Gray or Lab document as a 16-bit one); no
//! 16-bit or float arm is built. `scripts/photoshop-probe/art_history_take.py`
//! take1 recorded Photoshop 26.10.0's tool DISABLED on 16-bit RGB, 32-bit
//! float and 16-bit CMYK ("does not work with 16 bit per channel images",
//! "… 32 bit …") and ENABLED on 8-bit RGB and CMYK.
use serde_json::{json, Value};

use crate::art_history_stroke::{dab_bound, max_length, ArtHistoryRequest};
use crate::brush_coverage::BrushRequest;
use crate::cmyk_document::CmykDepth;
use crate::cmyk_retouch_brush::HISTORY_SOURCE_REFUSAL;
use crate::doc_lock::EditKind;
use crate::gray_art_history::GRAY_ART;
use crate::gray_document::GrayDepth;
use crate::job_control::JobContext;
use crate::lab_document::LabDepth;
use crate::lab_retouch::LAB_ART;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::pixel_count;

pub(crate) const RGB16_REFUSAL: &str = "The Art History Brush is unavailable in a 16-bit document.";
pub(crate) const FLOAT32_REFUSAL: &str =
    "The Art History Brush is unavailable in a 32-bit document.";
pub(crate) const CMYK16_REFUSAL: &str =
    "The Art History Brush is unavailable in a 16-bit CMYK document.";

/// A target that exists but is not a CMYK raster.
pub(crate) const ART_TARGET_REFUSAL: &str = "The Art History Brush paints only a CMYK raster layer";
/// A source state the Art History Brush cannot paint from on CMYK.
pub(crate) const ART_SOURCE_REFUSAL: &str =
    "The Art History Brush's source state is not a CMYK raster";

/// The bytes of one generated dab (`[f64; 4]`).
const DAB_BYTES: u64 = 32;

impl PrecisionDocument {
    /// The u16-record arm of a Gray or Lab document, else `None` (CMYK).
    fn u16_art(&self) -> Option<&'static crate::gray_art_history::U16Art> {
        if self.is_gray() {
            Some(&GRAY_ART)
        } else if self.is_lab() {
            Some(&LAB_ART)
        } else {
            None
        }
    }

    /// The depth refusal of the Art History Brush (module doc), or `None`
    /// on an 8-bit CMYK, Gray or Lab document.
    fn art_history_refusal(&self) -> Option<&'static str> {
        if self.is_float() {
            return Some(FLOAT32_REFUSAL);
        }
        if let Some(policy) = self.gray_policy() {
            return (policy.depth() != GrayDepth::Eight).then_some(RGB16_REFUSAL);
        }
        if let Some(policy) = self.lab_policy() {
            return (policy.depth() != LabDepth::Eight).then_some(RGB16_REFUSAL);
        }
        match self.require_cmyk_policy("The Art History Brush") {
            Err(_) => Some(RGB16_REFUSAL),
            Ok(policy) if policy.depth == CmykDepth::Sixteen => Some(CMYK16_REFUSAL),
            Ok(_) => None,
        }
    }

    /// The Art History Brush's plan for layer `layer` painting from layer
    /// `source_layer` of `source`: the History Brush's own CMYK plan for the
    /// same layer, source and selection, with `input_samples` the selection
    /// plane alone (the canvas pixel count when `selection`, else 0), plus
    /// 32 B per generated dab (`art_history_stroke::dab_bound`) and one
    /// stroke's f32 field, `4·F²` with `F = ⌈(1.5·L_max/√2 + 1)·D_max⌉ + 2`:
    /// the longest stroke's box at the widest dab (`L_max` the shipped
    /// table's, `max_length`). Refuses by
    /// depth (module doc), an existing target that is not a CMYK raster
    /// ([`ART_TARGET_REFUSAL`], before the source), and a source state that
    /// is not one ([`ART_SOURCE_REFUSAL`]), otherwise as the History Brush
    /// refuses. An 8-bit Gray or Lab document plans the u16-record arm
    /// (`u16_art_history_plan`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn art_history_brush_plan(
        &self,
        layer: usize,
        source: &PrecisionDocument,
        source_layer: usize,
        request: &ArtHistoryRequest,
        brush: &BrushRequest,
        dabs: &[f64],
        selection: bool,
    ) -> Result<Value, String> {
        if let Some(refusal) = self.art_history_refusal() {
            return Err(refusal.into());
        }
        if let Some(art) = self.u16_art() {
            return self.u16_art_history_plan(
                art,
                layer,
                source,
                source_layer,
                request,
                brush,
                dabs,
                selection,
            );
        }
        brush.plan(self.dimensions(), dabs)?;
        // The target is judged before the source, so a non-raster target is
        // refused in its own sentence rather than the source's.
        if self
            .layer(layer)
            .is_some_and(|record| record.cmyk_source().is_none())
        {
            return Err(ART_TARGET_REFUSAL.into());
        }
        let plan = self
            .cmyk_history_brush_plan(layer, source, source_layer, selection)
            .map_err(|e| {
                if e == HISTORY_SOURCE_REFUSAL {
                    ART_SOURCE_REFUSAL.to_string()
                } else {
                    e
                }
            })?;
        self.art_stroke_terms(plan, request, dabs, selection)
    }

    /// `plan` (a History Brush plan) with the terms every Art History plan
    /// adds (`art_history_brush_plan`'s doc): 32 B per generated dab and one
    /// stroke's f32 field added to `scratch_bytes`, and `input_samples` the
    /// selection plane alone.
    pub(crate) fn art_stroke_terms(
        &self,
        mut plan: Value,
        request: &ArtHistoryRequest,
        dabs: &[f64],
        selection: bool,
    ) -> Result<Value, String> {
        let (width, height) = self.dimensions();
        let n = pixel_count(width, height)? as u64;
        let widest = dabs.chunks_exact(4).map(|d| d[2]).fold(0., f64::max);
        let side = ((1.5 * max_length() / std::f64::consts::SQRT_2 + 1.) * widest).ceil() + 2.;
        let field = (side * side * 4.).min(u64::MAX as f64) as u64;
        let scratch = plan["scratch_bytes"]
            .as_u64()
            .unwrap_or(u64::MAX)
            .checked_add(dab_bound(request, dabs).saturating_mul(DAB_BYTES))
            .and_then(|v| v.checked_add(field))
            .ok_or("art history brush scratch overflow")?;
        plan["scratch_bytes"] = json!(scratch);
        plan["input_samples"] = json!(if selection { n } else { 0 });
        plan["input_planes"] = json!(u64::from(selection));
        Ok(plan)
    }

    /// Paints layer `layer` with the Art History Brush from layer
    /// `source_layer` of `source` (an earlier state of this document):
    /// `request` the tool's options, `brush` and `dabs` `dr_brush_u16`'s (the
    /// dabs in canvas coordinates), `selection` the canvas-sized u16
    /// selection plane or empty. Refuses by depth (module doc). `Ok(None)`
    /// when no stored code changes. A PIXELS edit under `precision_lock`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn art_history_brush(
        &self,
        layer: usize,
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
        if let Some(refusal) = self.art_history_refusal() {
            return Err(refusal.into());
        }
        if let Some(art) = self.u16_art() {
            return self.under_locks(layer, EditKind::Pixels, |doc| {
                doc.u16_art_history_unlocked(
                    art,
                    layer,
                    source,
                    source_layer,
                    request,
                    brush,
                    dabs,
                    selection,
                    limit,
                    other,
                    job,
                )
            });
        }
        self.under_locks(layer, EditKind::Pixels, |doc| {
            doc.cmyk_art_history_unlocked(
                layer,
                source,
                source_layer,
                request,
                brush,
                dabs,
                selection,
                limit,
                other,
                job,
            )
        })
    }
}
