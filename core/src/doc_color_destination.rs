//! Whether a byte document can LIVE in a destination profile after a Convert
//! or an Assign. Some of what a document holds is
//! evaluated through its own profile's matrix/TRC model rather than on its
//! stored numbers — calibrated (`document_v2`) Exposure and White Balance,
//! and the `linear_light` evaluation policy's transfer — and a profile with
//! no such model (a LUT-only RGB profile) cannot evaluate either. Carrying
//! such a document into one would produce a document that refuses to render,
//! so the Convert preflight and the Assign entry points refuse FIRST, naming
//! the layer or the policy, rather than landing that document.
//!
//! The rule, stated once:
//!
//! - a `linear_light` document refuses a destination
//!   [`linear_light_model`] refuses, with that function's sentence, for
//!   Convert and Assign alike ([`DrDocument::destination_refusal`]);
//! - Assign of a profile whose matrix/TRC curves fail the precision check is
//!   otherwise admitted (it retags the stored numbers), but a `linear_light`
//!   document refuses it with `icc_encode_refusal`'s linear-light Assign
//!   sentence, a document holding text or shape artwork with its artwork
//!   Assign sentence, and a document holding a `document_v2`
//!   Exposure/White Balance layer with its calibrated Assign sentence,
//!   all three checked first, in that order;
//! - Assign also refuses, naming the first such layer bottom-first, while any
//!   `document_v2` Exposure/White Balance layer exists and the destination
//!   has no matrix/TRC model ([`DrDocument::assign_profile_refusal`]) —
//!   hidden, grouped and clipped layers included, because the render
//!   preflight (`adjust_calibrated::validate_context`) refuses them all;
//! - Convert's per-layer half lives in `doc_color_policy`'s preflight, which
//!   refuses under keep-parameters ([`calibrated_sentence`]) and bakes under
//!   preserve-appearance ([`calibrated_bake_reason`]).
//!
//! Assigning the document's own profile bytes is never refused here: it is
//! already the no-op `assign_profile` answers `Ok(None)` for. Open-time tagging
//! runs on a fresh encoded document with no adjustment layer, so a LUT
//! profile is still accepted there.
//!
//! `doc_color_destination.rs` (byte depth) is the rule for a destination with NO
//! matrix/TRC model (`adjust_calibrated::evaluates_in`, the one statement of that gate): a
//! `linear_light` document refuses it (`destination_refusal`,
//! `doc_evaluation::linear_light_model`'s sentence, asked by the preflight right after
//! `source_refusal`); keep-parameters refuses a `document_v2` Exposure/White Balance layer by name,
//! offering the bake only when it would succeed, and preserve-appearance bakes a visible one under
//! the ordinary bake rules; Assign refuses while such a layer exists (`assign_profile_refusal`,
//! `dr_doc_assign_profile_refusal`, which first asks `icc_encode_refusal`'s three Assign checks —
//! linear-light, then text/shape artwork, then the first calibrated layer
//! (`first_calibrated_layer`, shared with the no-model sentence) — for a failing-curves profile),
//! and `assign_profile` / `assign_profile_rasterizing` refuse with its sentence whenever it is
//! `Some`.

use crate::adjust_calibrated::{calibrated_refusal, evaluates_in};
use crate::artwork_document;
use crate::doc::{DrDocument, LayerKind};
use crate::doc_color_policy::{adjustment_op, appearance_invariant};
use crate::doc_evaluation::linear_light_model;
use crate::icc::IccProfile;
use crate::icc_encode_refusal::{
    artwork_assign_refusal, calibrated_assign_refusal, linear_light_assign_refusal,
};

/// The refusal for a calibrated (`document_v2`) Exposure/White Balance layer
/// named `layer` carried into `profile`, which has no matrix/TRC model.
/// `verb` is `"assign"` or `"convert to"`; `offer_bake` adds the
/// preserve-appearance route, offered only where that bake would succeed.
pub(crate) fn calibrated_sentence(
    layer: &str,
    profile: &str,
    verb: &str,
    offer_bake: bool,
) -> String {
    let mut sentence = format!(
        "\"{layer}\" uses calibrated Exposure/White Balance (document_v2), which only a \
         matrix/TRC profile can evaluate, and \"{profile}\" is not one. Merge it down or delete \
         it, or {verb} a matrix/TRC profile."
    );
    if offer_bake {
        sentence.push_str(" Or choose preserve appearance to bake it.");
    }
    sentence
}

/// The preflight row's reason when preserve-appearance bakes a calibrated
/// layer because the destination cannot evaluate it.
pub(crate) fn calibrated_bake_reason(op: &str, layer: &str, profile: &str) -> String {
    format!(
        "{op} with document_v2 evaluation cannot be evaluated in \"{profile}\", so it is baked \
         into \"{layer} (baked)\""
    )
}

impl DrDocument {
    /// Why this document's evaluation POLICY cannot run in `p`: `Some` only
    /// for a `linear_light` document and a destination with no matrix/TRC
    /// model (or one whose curves fail `validate_precision`), carrying
    /// [`linear_light_model`]'s sentence. `None` for an encoded document.
    /// Convert's preflights ask `encode_into_refusal` first and Assign asks
    /// `linear_light_assign_refusal` first, so the failing-curves branch
    /// answers neither today; the no-model sentence still does.
    pub(crate) fn destination_refusal(&self, p: &IccProfile) -> Option<String> {
        if self.linear_light.is_some() {
            linear_light_model(p).err()
        } else {
            None
        }
    }

    /// Why assigning `p` would land a document that cannot render, or `None`
    /// when it would not (module doc). `None` for the document's own profile
    /// bytes. This is the sentence the Assign sheet shows beside a profile
    /// choice and `assign_profile` returns; `assign_profile` and
    /// `assign_profile_rasterizing` refuse with it whenever it is `Some`.
    /// A failing-curves `p` refuses here in Assign words for a `linear_light`
    /// document, then for one holding text or shape artwork, then for
    /// one holding a calibrated layer.
    pub fn assign_profile_refusal(&self, p: &IccProfile) -> Option<String> {
        if self.profile.bytes() == p.bytes() {
            return None;
        }
        if self.linear_light.is_some() {
            if let Some(refusal) = linear_light_assign_refusal(p) {
                return Some(refusal);
            }
        }
        if self.layers.iter().any(|layer| {
            layer
                .meta
                .as_deref()
                .is_some_and(artwork_document::rerenders)
        }) {
            if let Some(refusal) = artwork_assign_refusal(p) {
                return Some(refusal);
            }
        }
        if let Some(layer) = self.first_calibrated_layer(Some(p)) {
            if let Some(refusal) = calibrated_assign_refusal(p, layer) {
                return Some(refusal);
            }
        }
        if let Some(refusal) = self.destination_refusal(p) {
            return Some(refusal);
        }
        if evaluates_in(p) {
            return None;
        }
        self.first_calibrated_layer(None)
            .map(|layer| calibrated_sentence(layer, p.name(), "assign", false))
    }

    /// Name of the first non-group layer, bottom-first, whose meta is a
    /// `document_v2` Exposure/White Balance (`adjustment_op` +
    /// `appearance_invariant`); hidden, grouped and clipped layers included.
    /// With `refused_in`, only a layer whose op that profile refuses
    /// (`calibrated_refusal`: an Exposure is admitted by a falling-boundary
    /// profile a White Balance is not, #578); `None` for any such layer (the
    /// no-model branch, which refuses both ops).
    fn first_calibrated_layer(&self, refused_in: Option<&IccProfile>) -> Option<&str> {
        self.layers
            .iter()
            .filter(|layer| layer.kind != LayerKind::Group)
            .find_map(|layer| {
                let (op, evaluation) = adjustment_op(layer.meta.as_ref())?;
                let refused = refused_in.is_none_or(|p| calibrated_refusal(p, &op).is_some());
                (appearance_invariant(&op, evaluation.as_deref()) && refused)
                    .then_some(layer.name.as_str())
            })
    }
}
