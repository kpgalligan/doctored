//! The typed half of the versioned evaluation policy (Phase 11E): the
//! `PrecisionDocument` twin of `doc_evaluation`. The enum, the operator table,
//! the refusal wordings and the byte transfer live there; this module owns the
//! 16-bit transfer tables and the five methods `PrecisionDocument` answers the
//! policy with.
//!
//! [`TypedTransfer`] is the same two questions at typed depth: a 65536-entry
//! decode per channel filled from the profile's own forward TRCs, and the
//! existing continuous [`PrecisionToneEncoder`] on the way back — no second
//! curve formula, no inverter. ≈2.41 MB, built ONCE per document and recorded
//! by `PrecisionDocument::record_resources` and `PrecisionScene::plan`, so the
//! bytes are inside admission rather than beside it.
//!
//! The refusal walk is the byte walk over the typed tree: postorder,
//! bottom-first (the `PrecisionDocument::layer` indexing), refusing a layer
//! style, Blend If, one of the seven unmigrated blend modes and
//! a Shadows/Highlights kernel that reads a guide.

use std::sync::Arc;

use crate::doc_evaluation::{
    linear_light_model, refusal_sentence, stale_transfer_sentence, style_operator, Evaluation,
};
use crate::icc::IccProfile;
use crate::icc_transform::PrecisionToneEncoder;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::resources::Inventory;

/// Entries in each typed decode table: one per representable u16 code.
const DECODE16_ENTRIES: usize = 65536;

/// The typed document's transfer tables: one 65536-entry decode per channel
/// and the shared continuous encoder, both built from the profile's own TRCs.
pub struct TypedTransfer {
    profile: Arc<IccProfile>,
    /// `decode16[c][v]` is channel `c`'s LINEAR value for stored code `v`.
    decode16: [Vec<f32>; 3],
    encoder: PrecisionToneEncoder,
}

impl TypedTransfer {
    /// Builds the tables for `profile`. `Err` — a REFUSAL — for a profile with
    /// no matrix/TRC model and for one `validate_precision` rejects, which is
    /// the gate `PrecisionToneEncoder::new` already applies.
    pub fn for_profile(profile: &Arc<IccProfile>) -> Result<Self, String> {
        let model = linear_light_model(profile)
            .map_err(|e| crate::icc_encode_refusal::own_profile_error(profile, e))?;
        let encoder = PrecisionToneEncoder::new(model)?;
        let mut decode16: [Vec<f32>; 3] = std::array::from_fn(|_| Vec::new());
        for (channel, table) in decode16.iter_mut().enumerate() {
            table
                .try_reserve_exact(DECODE16_ENTRIES)
                .map_err(|_| "could not allocate the linear-light decode table")?;
            for v in 0..DECODE16_ENTRIES {
                let x = v as f32 / (DECODE16_ENTRIES - 1) as f32;
                table.push(model.decode_rgb([x, x, x])[channel] as f32);
            }
        }
        Ok(Self {
            profile: Arc::clone(profile),
            decode16,
            encoder,
        })
    }

    /// The working memory [`Self::for_profile`] builds: the three decode
    /// tables (`3 · 65536 · 4` bytes) plus the encoder's own
    /// (`PrecisionToneEncoder::scratch_bytes`). The ONE statement of that
    /// charge, read by the clipboard's promotion into float.
    pub(crate) fn table_bytes() -> u64 {
        3 * DECODE16_ENTRIES as u64 * 4 + PrecisionToneEncoder::scratch_bytes()
    }

    /// The profile these tables were built from — the staleness check's subject.
    pub fn profile(&self) -> &Arc<IccProfile> {
        &self.profile
    }

    /// One straight encoded RGB triple in [0, 1] as LINEAR components: the
    /// byte rule at 16 bits — the exact table entry on a lattice code, a
    /// linear interpolation between neighbours anywhere else.
    pub fn decode(&self, rgb: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|c| {
            let position = rgb[c].clamp(0.0, 1.0) * (DECODE16_ENTRIES - 1) as f32;
            let low = (position.floor() as usize).min(DECODE16_ENTRIES - 1);
            let high = (position.ceil() as usize).min(DECODE16_ENTRIES - 1);
            let table = &self.decode16[c];
            table[low] + (table[high] - table[low]) * (position - position.floor())
        })
    }

    /// One LINEAR triple back to a CONTINUOUS encoded triple in [0, 1]:
    /// `PrecisionToneEncoder::rgb`, the existing interpolated forward search.
    /// There is no rounding here — `PrecisionImage::from_float_samples` owns
    /// the one `round(v · 65535)` the typed output makes.
    pub fn encode(&self, linear: [f32; 3]) -> [f32; 3] {
        self.encoder.rgb(linear.map(f64::from))
    }

    /// The tables' unique heap payload: three decode vectors plus the shared
    /// encoder's own three. Counted by the document and by the render plan, so
    /// a linear-light typed document is admitted for what it actually holds.
    pub(crate) fn record_resources(&self, inventory: &mut Inventory) {
        for table in &self.decode16 {
            inventory.vec(table, "linear_light_tables", false);
        }
        self.encoder.record_resources(inventory);
    }
}

/// The typed twin of `doc_evaluation::guide_view`: under `linear_light` the
/// backdrop sample's RGB re-encoded CONTINUOUSLY ([`TypedTransfer::encode`],
/// nothing quantized at this depth), alpha kept; `None` is the sample itself.
pub(crate) fn guide_view16(px: [f32; 4], transfer: Option<&TypedTransfer>) -> [f32; 4] {
    match transfer {
        None => px,
        Some(transfer) => {
            let rgb = transfer.encode([px[0], px[1], px[2]]);
            [rgb[0], rgb[1], rgb[2], px[3]]
        }
    }
}

/// The operator this typed ENTRY carries that linear-light evaluation does not
/// evaluate yet, or `None` — the byte `refused_operator`, over the typed
/// fields.
fn refused_operator(layer: &PrecisionDocumentLayer) -> Option<&'static str> {
    if layer.style.is_some() || layer.blend_if.is_some() {
        return Some(style_operator());
    }
    None
}

/// The first refused entry in postorder, bottom-first — the same numbering
/// `PrecisionDocument::layer` uses, so the sentence names the row the host's
/// layer panel shows.
fn walk(layers: &[PrecisionDocumentLayer], index: &mut usize) -> Option<String> {
    for layer in layers {
        if let Content::Group(children) | Content::PassThrough(children) = &layer.content {
            if let Some(reason) = walk(children, index) {
                return Some(reason);
            }
        }
        let here = *index;
        *index += 1;
        if let Some(operator) = refused_operator(layer) {
            return Some(refusal_sentence(here, &layer.name, operator));
        }
    }
    None
}

impl PrecisionDocument {
    /// This document's evaluation policy. A FLOAT document is linear light
    /// BY CONSTRUCTION and holds no transfer at all, so the float policy is
    /// asked first; for every other document the transfer's presence IS the
    /// policy, exactly as before.
    pub fn evaluation(&self) -> Evaluation {
        if self.float.is_some() || self.linear_light.is_some() {
            Evaluation::LinearLight
        } else {
            Evaluation::Encoded
        }
    }

    /// The document under `policy`; `Ok(None)` when it is already there and
    /// `Err` for a refusal. The byte contract, at typed depth. On a float
    /// document `linear_light` is therefore a no-op and `encoded` is a
    /// refusal: there is no encoding a scene-linear extended-range document
    /// could composite in.
    pub fn with_evaluation(&self, policy: Evaluation) -> Result<Option<Self>, String> {
        if self.float.is_some() {
            return match policy {
                Evaluation::LinearLight => Ok(None),
                Evaluation::Encoded => {
                    Err("evaluation is fixed to linear light in a 32-bit float document".into())
                }
            };
        }
        if self.evaluation() == policy {
            return Ok(None);
        }
        let mut out = self.clone();
        match policy {
            Evaluation::Encoded => out.linear_light = None,
            Evaluation::LinearLight => {
                out.linear_light = Some(Arc::new(TypedTransfer::for_profile(&self.profile)?));
                if let Some(reason) = out.linear_light_refusal() {
                    return Err(reason);
                }
            }
        }
        Ok(Some(out))
    }

    /// The first operator this stack carries that linear-light evaluation
    /// cannot evaluate; `None`, before any walk, for an encoded document.
    ///
    /// A FLOAT document answers with its OWN, narrower table
    /// (`float_document::float_refusal`) — which is what makes the two host
    /// guards 11E installed refuse an unadmitted float edit with no new
    /// guard anywhere.
    pub fn linear_light_refusal(&self) -> Option<String> {
        if self.float.is_some() {
            return self.float_refusal();
        }
        let transfer = self.linear_light.as_ref()?;
        if !Arc::ptr_eq(transfer.profile(), &self.profile) {
            return Some(stale_transfer_sentence());
        }
        walk(&self.layers, &mut 0)
    }

    /// The transfer this document's render composites through — the typed
    /// backstop, obtained by `scene_cut` exactly as every byte projection
    /// obtains the byte one.
    pub(crate) fn transfer(&self) -> Result<Option<&Arc<TypedTransfer>>, String> {
        // A float document's samples are ALREADY linear, so it has no
        // transfer to decode through — but the refusal walk is still the
        // backstop nothing routes around.
        if self.float.is_some() {
            return match self.float_refusal() {
                Some(reason) => Err(reason),
                None => Ok(None),
            };
        }
        let Some(transfer) = self.linear_light.as_ref() else {
            return Ok(None);
        };
        match self.linear_light_refusal() {
            Some(reason) => Err(reason),
            None => Ok(Some(transfer)),
        }
    }

    /// Rebuilds the transfer when the document's profile has been replaced.
    pub(crate) fn rebind_evaluation(&mut self) -> Result<(), String> {
        let Some(transfer) = self.linear_light.as_ref() else {
            return Ok(());
        };
        if Arc::ptr_eq(transfer.profile(), &self.profile) {
            return Ok(());
        }
        self.linear_light = Some(Arc::new(TypedTransfer::for_profile(&self.profile)?));
        Ok(())
    }
}
