//! The byte adapter of the abstract-look re-bake: every profile
//! write on a `DrDocument` re-bakes each abstract Color Lookup look
//! through the new profile, so a stored Abstract profile is always
//! evaluated through the document's current one. The rule and the bake are
//! `icc_lookup::rebake_meta`; this module only walks the layers.
//!
//! `doc_evaluation::rebind_evaluation`, which every byte profile write
//! already calls once the new profile lands (Assign, Convert), calls
//! [`DrDocument::rebake_looks`] first; a
//! Don't-Convert paste re-points only the pasted layers, so
//! `doc_paste_layers` calls [`rebake_layer_meta`] on each of them. A `.cube`
//! or device-link meta is never touched.

use std::sync::Arc;

use crate::doc::{DrDocument, Layer};
use crate::icc::IccProfile;
use crate::icc_lookup::rebake_meta;

/// Re-bakes `layer`'s meta through `profile` when it is an abstract look;
/// leaves every other meta as it is. `Err` is `rebake_meta`'s sentence,
/// which fails the whole profile write.
pub(crate) fn rebake_layer_meta(layer: &mut Layer, profile: &IccProfile) -> Result<(), String> {
    let Some(meta) = layer.meta.as_deref() else {
        return Ok(());
    };
    if let Some(next) = rebake_meta(meta, profile)? {
        layer.meta = Some(next);
    }
    Ok(())
}

impl DrDocument {
    /// Re-bakes every abstract look in the stack through the document's
    /// current profile. In place, on a document a profile write is still
    /// building (clone-then-mutate), never on one a caller holds.
    pub(crate) fn rebake_looks(&mut self) -> Result<(), String> {
        let profile = Arc::clone(&self.profile);
        for layer in &mut self.layers {
            rebake_layer_meta(layer, &profile)?;
        }
        Ok(())
    }
}
