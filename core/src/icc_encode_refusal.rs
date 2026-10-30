//! The refusals of an encode INTO a matrix/TRC profile whose tone curves fail
//! `MatrixTrc::validate_precision`: the sentences
//! and the checks every entry point routes through, so the export and
//! convert preflights, `dr_icc_describe`'s `export_refusal` /
//! `convert_refusal` keys, the edits that encode into a document's own
//! profile and the Assign entry points cannot word the same refusal
//! differently.
//!
//! Three cases. A DESTINATION (an export's or a convert's
//! target) is a choice the caller can change, so its sentence says "Choose
//! another profile". A document's OWN profile is not chosen per edit, so its
//! sentence names the one command that replaces it, Convert to Profile. An
//! ASSIGN encodes nothing into the profile by itself — it retags the
//! stored numbers, so it is admitted — except where the document then has to
//! encode into it: a `linear_light` document's transfer runs through the
//! profile's curves, its text and shape layers re-render into it, and its
//! calibrated (`document_v2`) Exposure/White Balance layers evaluate through
//! those curves. Each of those three is refused up front in Assign
//! words, at every depth and in that order, naming what to change, before
//! any re-render could answer the own-profile sentence instead. The byte
//! `assign_profile_refusal` and the typed `color_plan` (`!convert`) ask.
//!
//! `destination_sentence` ("Doctored cannot {export|convert} into “name” …", which
//! `describe_json`'s `export_refusal` / `convert_refusal` keys format) and `own_profile_sentence`
//! ("Doctored cannot encode colour into “name” … Convert the document to another profile first
//! (Image ▸ Mode ▸ Convert to Profile…)"), plus `fails_precision`, `encode_into_refusal` and
//! `own_profile_error`. `ExportRequest::destination_refusal` asks it in every export entry point
//! after the bit-depth check (`"document"` never refuses; the document's own bytes under a
//! non-absolute intent with BPC off are exempt), both `conversion_preflight`s
//! ask it right after the own-bytes rule, and the edits that encode
//! into a document's OWN profile (typed paint colours, artwork, both linear-light transfers,
//! calibrated `prepare_precision`, the soft proof's output encoder, `color16`, the converted paste
//! and an abstract Color Lookup look, whose sentence is kept out of the "failed to read {path}:"
//! prefix) reword their error through `own_profile_error`. The calibrated create/edit asker is
//! `adjust_calibrated::calibrated_refusal` (`dr_doc_calibrated_refusal` /
//! `dr_precision_doc_calibrated_refusal`, the host's `calibratedRefusal(op:)`): a NEW or EDITED
//! `document_v2` White Balance answers `own_profile_sentence` at both depths
//! (`PROFILE_SENTENCE` when there is no matrix/TRC model), and so does an Exposure only where
//! `MatrixTrc::exposure_domain` is `None` — Exposure runs on one exponent per channel and no
//! encoder, so a falling branch boundary (ROMM, ITU-709) admits it (#2) — and `adjust_image`
//! refuses through it,
//! while an EXISTING 8-bit layer keeps rendering through the byte `ToneEncoder`, pinned by the
//! stored `tests/fixtures/rz_byte_bad_gamma/calibrated_exposure.rz` (regenerate only with an
//! intentional byte-writer change). There is no fallback encoder. Assign relabels and
//! encodes nothing, so it ADMITS such a profile at both depths, rasterizing or not, except where
//! the document must then encode into it: `linear_light_assign_sentence` ("Doctored cannot assign
//! “name” to a document that uses linear-light evaluation … Switch Image ▸ Evaluation back to
//! Encoded first, or choose another profile."), `artwork_assign_sentence` (a document holding text
//! or shape layers, "Rasterize or delete them first (Layer ▸ Rasterize Artwork)") and
//! `calibrated_assign_sentence` (a document holding a calibrated `document_v2` Exposure/White
//! Balance layer whose op the profile refuses, named, "Merge it down or delete it first, or choose
//! another profile." — refused at both depths, because a 16-bit White Balance only evaluates
//! through the validated `PrecisionToneEncoder`), through `linear_light_assign_refusal` / `artwork_assign_refusal` /
//! `calibrated_assign_refusal`, asked in that order (linear-light, artwork, calibrated) by the byte
//! `assign_profile_refusal` and the typed `color_plan`'s `!convert` branch, ahead of any re-render
//! or rebind. A 32-bit float document (`float_color`) words its two own: `float_model_sentence`
//! (Assign or Convert with a profile that has no matrix/TRC model) and `float_assign_curve_sentence`.

use crate::icc::IccProfile;

/// The refusal of an export or convert INTO `name`; `verb` is "export" or
/// "convert". Moved verbatim from `icc::describe_json`.
pub(crate) fn destination_sentence(verb: &str, name: &str) -> String {
    format!(
        "Doctored cannot {verb} into \u{201c}{name}\u{201d} \u{2014} its matrix/TRC tone \
         curves fail the precision check every conversion into it runs (they must be \
         finite and monotone). Choose another profile."
    )
}

/// The refusal of an edit that encodes colour into a document's own profile
/// `name`. There is no fallback encoder: the answer is to convert.
pub(crate) fn own_profile_sentence(name: &str) -> String {
    format!(
        "Doctored cannot encode colour into \u{201c}{name}\u{201d} \u{2014} its matrix/TRC \
         tone curves fail the precision check (they must be finite and monotone). Convert \
         the document to another profile first (Image \u{25b8} Mode \u{25b8} Convert to \
         Profile\u{2026})."
    )
}

/// The refusal of an Assign of `name` to a `linear_light` document:
/// its transfer would have to encode through the failing curves.
pub(crate) fn linear_light_assign_sentence(name: &str) -> String {
    format!(
        "Doctored cannot assign \u{201c}{name}\u{201d} to a document that uses linear-light \
         evaluation \u{2014} its matrix/TRC tone curves fail the precision check (they must be \
         finite and monotone). Switch Image \u{25b8} Evaluation back to Encoded first, or \
         choose another profile."
    )
}

/// The refusal of an Assign of `name` to a document holding text or shape
/// artwork: Assign re-renders that artwork into the profile.
pub(crate) fn artwork_assign_sentence(name: &str) -> String {
    format!(
        "Doctored cannot assign \u{201c}{name}\u{201d} to a document that holds text or shape \
         layers \u{2014} its matrix/TRC tone curves fail the precision check (they must be \
         finite and monotone), and those layers would have to render into it. Rasterize or \
         delete them first (Layer \u{25b8} Rasterize Artwork), or choose another profile."
    )
}

/// The refusal of an Assign of `name` to a document holding the calibrated
/// (`document_v2`) Exposure/White Balance layer `layer`: that layer
/// would have to evaluate through the failing curves.
pub(crate) fn calibrated_assign_sentence(name: &str, layer: &str) -> String {
    format!(
        "Doctored cannot assign \u{201c}{name}\u{201d} to a document whose layer \u{201c}{layer}\u{201d} \
         uses calibrated Exposure/White Balance (document_v2) \u{2014} its matrix/TRC tone curves \
         fail the precision check (they must be finite and monotone), and that layer would have \
         to evaluate through them. Merge it down or delete it first, or choose another profile."
    )
}

/// The refusal of an Assign (`convert` false) or a Convert (`convert` true)
/// of a 32-bit float document with the profile `name`, which has no
/// matrix/TRC model: a float document's linear values are defined by its
/// profile's primaries, and a LUT has none.
pub(crate) fn float_model_sentence(convert: bool, name: &str) -> String {
    let action = if convert {
        format!("convert a 32-bit float document into \u{201c}{name}\u{201d}")
    } else {
        format!("assign \u{201c}{name}\u{201d} to a 32-bit float document")
    };
    format!(
        "Doctored cannot {action} \u{2014} it has no matrix/TRC model, and a float document's \
         linear values are defined by its profile's primaries. Choose a matrix/TRC RGB profile."
    )
}

/// The refusal of an Assign of `name`, whose tone curves fail the precision
/// check, to a 32-bit float document: Assign encodes nothing, but the
/// document's display and SDR export then run those curves.
pub(crate) fn float_assign_curve_sentence(name: &str) -> String {
    format!(
        "Doctored cannot assign \u{201c}{name}\u{201d} to a 32-bit float document \u{2014} its \
         matrix/TRC tone curves fail the precision check (they must be finite and monotone), and \
         a float document's display and 8/16-bit export encode through them. Choose another \
         profile."
    )
}

/// True when `profile` has a matrix/TRC model whose curves fail the
/// precision check. A LUT-only or modelless profile is not this question.
pub(crate) fn fails_precision(profile: &IccProfile) -> bool {
    profile
        .model()
        .is_some_and(|model| model.validate_precision().is_err())
}

/// The destination refusal for an encode into `profile`, or `None` when the
/// profile passes (or has no matrix/TRC model).
pub(crate) fn encode_into_refusal(profile: &IccProfile, verb: &str) -> Option<String> {
    fails_precision(profile).then(|| destination_sentence(verb, profile.name()))
}

/// `error` reworded as the own-profile sentence when it came from encoding
/// into `profile` and that profile fails the precision check; any other
/// error passes through unchanged.
pub(crate) fn own_profile_error(profile: &IccProfile, error: String) -> String {
    if fails_precision(profile) {
        own_profile_sentence(profile.name())
    } else {
        error
    }
}

/// The Assign refusal for a `linear_light` document and `profile`, or `None`
/// when the profile passes (or has no matrix/TRC model, which
/// `linear_light_model` words itself).
pub(crate) fn linear_light_assign_refusal(profile: &IccProfile) -> Option<String> {
    fails_precision(profile).then(|| linear_light_assign_sentence(profile.name()))
}

/// The Assign refusal for a document holding text or shape artwork and
/// `profile`, or `None` when the profile passes.
pub(crate) fn artwork_assign_refusal(profile: &IccProfile) -> Option<String> {
    fails_precision(profile).then(|| artwork_assign_sentence(profile.name()))
}

/// The Assign refusal for a document holding the calibrated layer `layer`
/// and `profile`, or `None` when the profile passes.
pub(crate) fn calibrated_assign_refusal(profile: &IccProfile, layer: &str) -> Option<String> {
    fails_precision(profile).then(|| calibrated_assign_sentence(profile.name(), layer))
}
