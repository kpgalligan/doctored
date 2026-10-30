//! The neural and generative tools on a Lab Color document (#1 Lab phase
//! 6b-iii): the model's input is the document's Display P3 VIEW and its
//! result returns to L\*a\*b\* codes by Mode ▸ Lab Color's rule.
//!
//! * **In** ([`view_samples`]): each plate pixel's codes through
//!   `lab_convert::view_pipeline` (the twin into the built-in Display P3,
//!   relative colorimetric, no BPC), clamped to 0…1, alpha kept: the rule
//!   Remove's model reads (`doc_remove_model`, 2f's third part), which routes
//!   through this function. Super Zoom, JPEG Artifacts Removal and Neural
//!   Colorize take the view from the host (`lab_convert::lab_display`);
//!   Generative Fill's plate takes it here (`doc_gen_fill`).
//! * **Out** ([`published`]): a model's or provider's picture, tagged the
//!   built-in Display P3 ([`result_profile`]), runs through
//!   `lab_convert::from_view_pipeline` into the twin and is quantized by
//!   `LabPolicy::code` — relative colorimetric, no BPC, ×257 at 8 bits,
//!   alpha on the same grid: the rule `convert_to_lab` uses, so a Lab result
//!   equals Mode ▸ Lab Color of the same picture on an RGB Display P3
//!   document. An image tagged anything else refuses ([`NOT_VIEW`]): its
//!   samples are not view samples. On a Gray document [`published`] is
//!   Gray's fold (`gray_artwork::folded_result`), and on any other document
//!   the image passes unchanged, so Gray's and RGB's results and plans stay
//!   byte-identical.
//! * **Plans**: [`published_bytes`] is the result's image (8 B/px of u16
//!   RGBA, `lab_convert::VIEW_BYTES_PER_PIXEL`) plus the way back's tables
//!   ([`published_tables`]), Gray's fold image on Gray, 0 elsewhere;
//!   [`view_tables`] is the view pipeline's tables a plate read prepares.
//!   Both pipelines are between built-in profiles, so their tables are
//!   computed once. [`neural_scratch_bytes`] is what the host's Lab leg of a
//!   neural run adds to the model's own plan: the view of the input and the
//!   Lab image of the output.
//!
//! Not a kernel: it names `PrecisionDocument` and `PrecisionImage`.

use std::sync::{Arc, OnceLock};

use crate::icc::IccProfile;
use crate::icc_intent::ColorPipeline;
use crate::job_control::JobContext;
use crate::lab_convert::{
    converted_image, from_view_pipeline, view_pipeline, view_scratch_bytes, VIEW_BYTES_PER_PIXEL,
};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{PrecisionImage, BOOKKEEPING_BYTES};

/// The refusal for a result not tagged the built-in Display P3 (module doc).
pub(crate) const NOT_VIEW: &str =
    "a Lab Color document takes a model's result in its Display P3 view";

/// Each RGB of `samples` (unit Lab codes) through `view`, clamped to 0…1,
/// alpha kept (module doc, "In").
pub(crate) fn view_samples(view: &ColorPipeline, samples: &mut [[f32; 4]]) {
    for p in samples {
        let rgb = view.color([p[0], p[1], p[2]]).map(|v| v.clamp(0.0, 1.0));
        *p = [rgb[0], rgb[1], rgb[2], p[3]];
    }
}

/// `image`, a model's picture in the built-in Display P3, as the Lab codes
/// of `doc`'s policy (module doc, "Out"). Refuses [`NOT_VIEW`] for an image
/// in any other profile, a non-Lab `doc`, and a cancel.
pub(crate) fn from_view(
    doc: &PrecisionDocument,
    image: PrecisionImage,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    let policy = doc
        .lab_policy()
        .ok_or("the Lab Color result needs a Lab Color document")?;
    if image.profile().bytes() != IccProfile::display_p3().bytes() {
        return Err(NOT_VIEW.into());
    }
    converted_image(
        &image,
        &from_view_pipeline()?,
        policy.twin(),
        &|unit| policy.code(unit),
        job,
    )
}

/// `image`, a result `doc` is about to publish: Gray's fold on a Gray
/// document, [`from_view`] on a Lab one, else `image` unchanged.
pub(crate) fn published(
    doc: &PrecisionDocument,
    image: PrecisionImage,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    if doc.is_lab() {
        return from_view(doc, image, job);
    }
    crate::gray_artwork::folded_result(doc, image, job)
}

/// The bytes [`published`] may allocate for an image of `pixels` pixels on
/// `doc` (module doc, "Plans").
pub(crate) fn published_bytes(doc: &PrecisionDocument, pixels: u64) -> u64 {
    if doc.is_lab() {
        return pixels
            .saturating_mul(VIEW_BYTES_PER_PIXEL)
            .saturating_add(published_tables(doc));
    }
    crate::gray_artwork::fold_bytes(doc, pixels)
}

/// The prepared tables of a pipeline between built-in profiles, computed
/// once; a pipeline that cannot be built charges `u64::MAX`, so the plan
/// refuses rather than under-charging.
fn tables(cell: &OnceLock<u64>, pipeline: fn() -> Result<ColorPipeline, String>) -> u64 {
    *cell.get_or_init(|| pipeline().map_or(u64::MAX, |p| p.scratch_bytes()))
}

/// The way back's tables (`lab_convert::from_view_pipeline`).
fn from_view_tables() -> u64 {
    static TABLES: OnceLock<u64> = OnceLock::new();
    tables(&TABLES, from_view_pipeline)
}

/// The tables [`from_view`] prepares; 0 off Lab.
pub(crate) fn published_tables(doc: &PrecisionDocument) -> u64 {
    if doc.is_lab() {
        from_view_tables()
    } else {
        0
    }
}

/// The tables a plate read through the view prepares
/// (`lab_convert::view_pipeline`); 0 off Lab.
pub(crate) fn view_tables(doc: &PrecisionDocument) -> u64 {
    static TABLES: OnceLock<u64> = OnceLock::new();
    if doc.is_lab() {
        tables(&TABLES, view_pipeline)
    } else {
        0
    }
}

/// The profile a provider's picture is rendered in before [`published`]:
/// the built-in Display P3 on a Lab document, else the document's own.
pub(crate) fn result_profile(doc: &PrecisionDocument) -> Arc<IccProfile> {
    if doc.is_lab() {
        IccProfile::display_p3()
    } else {
        doc.profile().clone()
    }
}

/// What the host's Lab leg of a neural run reserves beside the model's own
/// plan (module doc, "Plans"): the view of an `input`-sized image
/// (`lab_convert::view_scratch_bytes`), the Lab image of an `output`-sized
/// result at 8 B/px, the way back's tables and bookkeeping.
pub fn neural_scratch_bytes(input: (u32, u32), output: (u32, u32)) -> Result<u64, String> {
    let out = u64::from(output.0) * u64::from(output.1);
    Ok(view_scratch_bytes(input.0, input.1)?
        .saturating_add(out.saturating_mul(VIEW_BYTES_PER_PIXEL))
        .saturating_add(from_view_tables())
        .saturating_add(BOOKKEEPING_BYTES))
}
