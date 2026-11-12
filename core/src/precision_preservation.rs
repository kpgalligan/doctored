//! What a flattened file of a typed document (Grayscale, CMYK, Indexed
//! Color, Bitmap) has no place for: the editable state of `flat_state`'s
//! inventory, read off the typed records, then the two pixel lines of the
//! byte list (`doc_preservation`) read over the typed storage's own rows.
//! What a format carries of the profile, the metadata and the depth is the
//! host's, per mode, beside this list (`GrayDocument.saveLosses`,
//! `CmykDocument.saveLosses`).
//!
//! The one layer a flat file reopens with is named "Background", except in
//! Indexed Color ("Index") and Bitmap ("Bitmap"), whose openers name it
//! after the mode; any other name is a layer property the file drops.
//! Indexed and Bitmap files are written from the policy's plane, never the
//! layer's samples, so the pixel lines are not theirs: what their formats
//! keep of transparency is the host's mode list.

use crate::blend::BlendMode;
use crate::flat_state::{self, EditableState};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};

/// Every record of `layers`, children after their group.
fn records(layers: &[PrecisionDocumentLayer]) -> Vec<&PrecisionDocumentLayer> {
    let mut out = Vec::new();
    let mut stack: Vec<&PrecisionDocumentLayer> = layers.iter().rev().collect();
    while let Some(layer) = stack.pop() {
        out.push(layer);
        if let Some(children) = layer.children() {
            stack.extend(children.iter().rev());
        }
    }
    out
}

/// `(any sample not opaque, any sample at alpha 0 with colour or ink)` of a
/// pixel record; `(false, false)` for every other record.
fn alpha_facts(layer: &PrecisionDocumentLayer) -> (bool, bool) {
    let (mut translucent, mut hidden) = (false, false);
    match &layer.content {
        Content::Raster { image, .. } => {
            if let Some(runs) = image.runs_rgba16() {
                for p in runs.flatten() {
                    translucent |= p[3] != u16::MAX;
                    hidden |= p[3] == 0 && p[..3].iter().any(|&c| c != 0);
                    if translucent && hidden {
                        break;
                    }
                }
            } else if let Some(runs) = image.runs_rgba32() {
                for p in runs.flatten() {
                    translucent |= p[3] != 1.0;
                    hidden |= p[3] == 0.0 && p[..3].iter().any(|&c| c != 0.0);
                    if translucent && hidden {
                        break;
                    }
                }
            }
        }
        Content::CmykRaster { image, .. } => {
            let (w, h) = image.dimensions();
            'rows: for y in 0..h {
                for (_, run) in image.row_segments(y, 0, w) {
                    for p in run {
                        translucent |= p[4] != u16::MAX;
                        hidden |= p[4] == 0 && p[..4].iter().any(|&c| c != 0);
                        if translucent && hidden {
                            break 'rows;
                        }
                    }
                }
            }
        }
        _ => {}
    }
    (translucent, hidden)
}

impl PrecisionDocument {
    /// The name of the one layer a flat file of this document reopens with
    /// (module doc).
    fn flat_layer_name(&self) -> &'static str {
        if self.is_indexed() {
            crate::indexed_document::LAYER_NAME
        } else if self.is_bitmap() {
            crate::bitmap_document::LAYER_NAME
        } else {
            "Background"
        }
    }

    /// The lines a flattened file of this document loses (module doc), in
    /// `flat_state`'s order, then "pixel transparency" when the file has no
    /// alpha sample (`alpha_kept` false) and a pixel is not opaque, then
    /// "hidden pixel colors" when a pixel at alpha 0 holds a colour or ink
    /// the flattened composite writes as zero. A spot channel is never named
    /// here: whether a format keeps one is the host's (`saveLosses`). `[]`
    /// when the file reopens as this document.
    pub(crate) fn flat_state_losses(&self, alpha_kept: bool) -> Vec<&'static str> {
        let roots = self.root_layers();
        let all = records(roots);
        let canvas = (self.width, self.height);
        let name = self.flat_layer_name();
        let mut losses = flat_state::losses(&EditableState {
            sources: all.iter().any(|l| l.retained_source.is_some()),
            warps: all.iter().any(|l| l.warp.is_some()),
            layers: roots.len() != 1 || !roots[0].is_pixel_leaf(),
            masks: all.iter().any(|l| l.mask.is_some()),
            descriptions: all
                .iter()
                .any(|l| l.description.is_some() || l.generated.is_some()),
            styles: all
                .iter()
                .any(|l| l.style.is_some() || l.blend_if.is_some()),
            properties: all.iter().any(|l| {
                l.name.as_str() != name
                    || !l.visible
                    || l.opacity != 1.0
                    || l.blend_mode != BlendMode::Normal
                    || l.clipped
                    || l.locks != 0
                    || l.link != 0
                    || l.label != 0
                    || !l.open
            }),
            bounds: all.iter().any(|l| match &l.content {
                Content::Raster { image, offset } => {
                    *offset != (0, 0) || image.dimensions() != canvas
                }
                Content::CmykRaster { image, offset } => {
                    *offset != (0, 0) || image.dimensions() != canvas
                }
                _ => false,
            }),
            channels: self.channels.iter().any(|c| c.metadata.spot.is_none()),
            spot_channels: false,
            guides: !self.guides.is_empty() || self.ruler_origin != (0.0, 0.0),
            samplers: !self.color_samplers.is_empty(),
            paths: !self.paths.entries.is_empty(),
            comps: !self.comps.is_empty(),
            global_light: self.global_light != crate::style::GlobalLight::default(),
        });
        if self.is_indexed() || self.is_bitmap() {
            return losses;
        }
        let (mut translucent, mut hidden) = (false, false);
        for layer in all.iter().filter(|l| l.is_pixel_leaf()) {
            let (t, h) = alpha_facts(layer);
            translucent |= t;
            hidden |= h;
        }
        if translucent && !alpha_kept {
            losses.push("pixel transparency");
        }
        if hidden {
            losses.push("hidden pixel colors");
        }
        losses
    }
}
