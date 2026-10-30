//! Image ▸ Mode ▸ CMYK Color from a Lab Color document (#547 Lab phase 5a):
//! the ProPhoto stage, its plan terms and the Lab-named blocker.
//!
//! The rule is CMYK → Lab's reversed. That route runs CMYK → 16-bit
//! ProPhoto RGB → Lab with the RGB stage relative colorimetric with black
//! point compensation (the host's `rgbStageOptions`), so Lab → CMYK runs
//! the twin into 16-bit ProPhoto RGB under the same options
//! ([`stage_options`], `lab_convert`'s Lab → RGB body, never narrowed: an
//! 8-bit Lab document's codes stay on their ×257 grid and the stage keeps
//! every converted sample at u16), then `cmyk_convert`'s RGB → CMYK body
//! from ProPhoto at the request's depth, intent and BPC. There is no 8-bit
//! RGB stop. ProPhoto's gamut is far wider than a press profile's, so the
//! stage clips practically nothing the ink could have kept.
//!
//! What it refuses mirrors CMYK → Lab ([`first_blocker`]): an adjustment
//! layer, a Blend If on Lightness, a or b (the stored Red, Green and Blue,
//! named by `lab_style::channel_name`), a blend mode CMYK does not offer, a
//! placed image or a warped layer, each with `flatten_required`; `flatten:
//! true` flattens the Lab document first (its Lab arms composite it).
//!
//! # Memory
//!
//! The plan is RGB → CMYK's (`cmyk_convert::to_cmyk_scratch` over the same
//! pixels) plus the retained stage: [`STAGE_BYTES_PER_PIXEL`] a converted
//! pixel, the stage pipeline's tables and the ProPhoto ICC
//! ([`stage_scratch_bytes`]). Held by `scripts/lab-admission/measure.py`
//! (`to-cmyk-lab16`); a miss raises [`STAGE_BYTES_PER_PIXEL`].

use std::sync::OnceLock;

use crate::cmyk_convert_tree::first_blocker_named;
use crate::doc_color_policy::ConversionOptions;
use crate::icc::IccProfile;
use crate::icc_intent::ColorPipeline;
use crate::job_control::JobContext;
use crate::lab_document::lab_twin;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};

/// Bytes a staged pixel the plan charges: the retained u16 ProPhoto RGBA
/// stage (module doc); a miss raises this term.
pub(crate) const STAGE_BYTES_PER_PIXEL: u64 = 8;

/// The stage's options: relative colorimetric with black point
/// compensation, CMYK → Lab's RGB stage reversed (module doc).
pub(crate) fn stage_options() -> ConversionOptions {
    ConversionOptions {
        black_point_compensation: true,
        ..ConversionOptions::DEFAULT
    }
}

/// The stage's charge over `pixels` raster pixels (module doc, "Memory"):
/// the retained stage, the twin → ProPhoto pipeline's tables (both profiles
/// built in, so computed once) and the ProPhoto ICC's bytes.
pub(crate) fn stage_scratch_bytes(pixels: u64) -> u64 {
    static TABLES: OnceLock<u64> = OnceLock::new();
    let tables = match TABLES.get() {
        Some(&bytes) => bytes,
        None => match stage_pipeline() {
            Ok(pipeline) => *TABLES.get_or_init(|| pipeline.scratch_bytes()),
            Err(_) => 0,
        },
    };
    pixels
        .saturating_mul(STAGE_BYTES_PER_PIXEL)
        .saturating_add(tables)
        .saturating_add(IccProfile::prophoto_rgb().bytes().len() as u64)
}

/// The twin into the built-in ProPhoto RGB under [`stage_options`].
fn stage_pipeline() -> Result<ColorPipeline, String> {
    let options = stage_options();
    ColorPipeline::new(
        &lab_twin(),
        &IccProfile::prophoto_rgb(),
        options.intent,
        options.black_point_compensation,
    )
}

/// The first record (postorder) Lab → CMYK cannot carry, as RGB → CMYK's
/// sentence with a Blend If channel named as a Lab document names it
/// (module doc).
pub(crate) fn first_blocker(layers: &[PrecisionDocumentLayer]) -> Option<String> {
    first_blocker_named(layers, &crate::lab_style::channel_name)
}

impl PrecisionDocument {
    /// This Lab document as a 16-bit ProPhoto RGB document (module doc):
    /// the stage RGB → CMYK converts from. The caller has admitted and
    /// reserved [`stage_scratch_bytes`].
    pub(crate) fn lab_cmyk_stage(
        &self,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        self.lab_to_rgb_with_context(
            &IccProfile::prophoto_rgb(),
            &stage_options(),
            limit,
            other,
            job,
        )
    }
}
