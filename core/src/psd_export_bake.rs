//! The adjustment stamp of a layered export: the pixels a baked adjustment
//! layer is written as, for the byte and the typed document.
//!
//! An adjustment layer this build writes no Photoshop record for
//! (`psd_adjustment::encode` answers `None`: `gradient_map`, `color_lookup`, a
//! `bcs` with saturation, `hue_rotate`, `grayscale`, `sepia`,
//! `shadows_highlights`, `white_balance`, `replace_color`) cannot travel as a
//! record, and a pixel-less record with no body would silently drop the
//! adjustment. It is written instead as an ordinary pixel layer whose pixels
//! are the STAMP: the canvas-sized picture of the document with every entry
//! above the adjustment contributing nothing, its enclosing groups kept, and
//! the adjustment and every group enclosing it forced visible (a hidden
//! layer's or group's record stays hidden, but the pixels must be what showing
//! it would show). That is a Stamp Visible of everything up to that layer.
//!
//! The stamp is exact where the picture beneath is opaque and the layer sits
//! in plain structure; where it is clipped, or inside a group with its own
//! blend, opacity, mask, style or clipping, a Normal pixel layer cannot follow
//! it, and the preflight says so per layer (`psd_export_preflight::bake_inexact`).
//!
//! Both stamps are thin adapters over renders that already exist — the retouch
//! sampling plate's cut (`retouch_sampling::excluded` /
//! `precision_retouch_source::retouch_cut16`, mode Current & Below) into the
//! ordinary compositor (`flattened_cut` / `render_cut_with_context`) — so the
//! stamp is the document's own arithmetic and no pixel math lives here. The
//! typed stamp is admitted by `render_cut_with_context`'s own `limit`/`other`.

use std::sync::Arc;

use image::RgbaImage;

use crate::blend::BlendMode;
use crate::doc::{DrDocument, LayerKind};
use crate::doc_group;
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::PrecisionImage;
use crate::precision_retouch_source::retouch_cut16;
use crate::retouch_sampling::{cut_of, excluded, RetouchSampling};
use crate::style_composite::Cut;

impl DrDocument {
    /// The stamp of the adjustment layer at `index`: this document's
    /// canvas-sized projection up to and including it (module doc).
    pub(crate) fn adjustment_stamp(&self, index: usize) -> Result<RgbaImage, String> {
        if index >= self.layers.len() {
            return Err("this adjustment layer cannot be shown for its export stamp".into());
        }
        let cut = excluded(&self.layers, index, RetouchSampling::CurrentAndBelow, false);
        // The layer and every group enclosing it: a hidden group renders
        // nothing at all, so a stamp taken inside one would be the picture
        // beneath the group, without the adjustment or the group's lower
        // members, and would cover them once the group is shown in the file.
        let mut hidden = Vec::new();
        let mut cursor = Some(index);
        while let Some(at) = cursor {
            if !self.layers[at].visible {
                hidden.push(at);
            }
            cursor = doc_group::parent(&self.layers, at);
        }
        if hidden.is_empty() {
            return self.flattened_cut(doc_group::MAX_GROUP_BUFFER_PIXELS, cut_of(cut.as_ref()));
        }
        let mut shown = self.clone();
        for at in hidden {
            shown.layers[at].visible = true;
        }
        shown.flattened_cut(doc_group::MAX_GROUP_BUFFER_PIXELS, cut_of(cut.as_ref()))
    }

    /// The name of the nearest group enclosing `index` that a baked layer
    /// cannot follow exactly: one that is not Normal or Pass Through at 100 %
    /// with no enabled mask, no rendered style and unclipped.
    pub(crate) fn bake_inexact_group(&self, index: usize) -> Option<&str> {
        let mut cursor = doc_group::parent(&self.layers, index);
        while let Some(group) = cursor {
            let layer = &self.layers[group];
            if layer.kind == LayerKind::Group
                && (!matches!(layer.blend, BlendMode::Normal | BlendMode::PassThrough)
                    || layer.opacity != 1.0
                    || (layer.mask.is_some() && layer.mask_enabled)
                    || layer.renders_style().is_some()
                    || layer.clipped)
            {
                return Some(&layer.name);
            }
            cursor = doc_group::parent(&self.layers, group);
        }
        None
    }
}

/// The typed twin of [`DrDocument::bake_inexact_group`]'s test for ONE group.
pub(crate) fn inexact_group16(layer: &PrecisionDocumentLayer) -> bool {
    !matches!(
        layer.blend_mode(),
        BlendMode::Normal | BlendMode::PassThrough
    ) || layer.opacity() != 1.0
        || layer.mask().is_some_and(|mask| mask.enabled())
        || layer.style().is_some()
        || layer.blend_if().is_some()
        || layer.clipped()
}

/// `layers` with the record at postorder index `target` and every group
/// enclosing it made visible, every other node shared. `start` is the postorder index of the first record in
/// `layers`.
fn shown(
    layers: &[PrecisionDocumentLayer],
    start: usize,
    target: usize,
) -> Vec<PrecisionDocumentLayer> {
    let mut offset = start;
    layers
        .iter()
        .map(|layer| {
            let node_start = offset;
            let index = offset + layer.records - 1;
            offset += layer.records;
            if target < node_start || target > index {
                return layer.clone();
            }
            let mut layer = layer.clone();
            // The target, or a group enclosing it (see `adjustment_stamp`).
            layer.visible = true;
            if index == target {
                return layer;
            }
            layer.content = match &layer.content {
                Content::Group(children) => {
                    Content::Group(Arc::new(shown(children, node_start, target)))
                }
                Content::PassThrough(children) => {
                    Content::PassThrough(Arc::new(shown(children, node_start, target)))
                }
                other => other.clone(),
            };
            layer
        })
        .collect()
}

impl PrecisionDocument {
    /// The typed stamp of the adjustment record at postorder index
    /// `postorder`, in the document's own storage (RGBA16 for every document
    /// the export admits). `limit` and `other` are the caller's working limit
    /// and the bytes it retains elsewhere; the render admits itself.
    pub(crate) fn adjustment_stamp16(
        &self,
        postorder: usize,
        limit: u64,
        other: u64,
    ) -> Result<PrecisionImage, String> {
        let cut = retouch_cut16(
            &self.layers,
            postorder,
            RetouchSampling::CurrentAndBelow,
            false,
            self.records,
        );
        let mut document = self.clone();
        document.layers = Arc::new(shown(&self.layers, 0, postorder));
        document.render_cut_with_context(Cut::new(&cut), limit, other, JobContext::NONE)
    }
}
