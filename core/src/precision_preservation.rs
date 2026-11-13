//! What a flattened file of a typed document (RGB at 16 or 32 bits,
//! Grayscale, Lab Color, CMYK, Indexed Color, Bitmap) has no place for: the
//! editable state of `flat_state`'s inventory, read off the typed records,
//! then the two pixel lines of the byte list (`doc_preservation`) read over
//! the typed storage's own rows. What a format carries of the profile, the
//! metadata, the depth and a 32-bit document's tone map is the host's, per
//! mode, beside this list (`GrayDocument.saveLosses`, `LabDocument.saveLosses`,
//! `CmykDocument.saveLosses`, `PrecisionExportCapture.losses`).
//!
//! The one layer a flat file reopens with is named "Background", except in
//! Indexed Color ("Index") and Bitmap ("Bitmap"), whose openers name it
//! after the mode; any other name is a layer property the file drops.
//! Indexed and Bitmap files are written from the policy's plane, never the
//! layer's samples, so the pixel lines are not theirs: what their formats
//! keep of transparency is the host's mode list.
//!
//! The pixel lines read every sample of an opaque plane, and the host asks
//! for this list about 25 times per `get_document` and up to 9 times per
//! File ▸ Save. So each plane scans once: its two facts live in the plane
//! itself (`PrecisionImage::alpha_facts_once`, `CmykImage`'s twin), shared by
//! every clone, which is every later handle that kept the layer's pixels. A
//! plane's samples never change after it is built and every writer builds a
//! new plane, whose cell starts empty, so a kept answer is always the
//! samples' own; after a pixel edit only the planes it wrote scan again.

use crate::blend::BlendMode;
use crate::cmyk_image::CmykImage;
use crate::flat_state::{self, EditableState};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::PrecisionImage;

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
/// pixel record, scanned once per plane (module doc); `(false, false)` for
/// every other record. The exact float TIFF (`doc_export_float`) asks the
/// second for the one layer it writes.
pub(crate) fn alpha_facts(layer: &PrecisionDocumentLayer) -> (bool, bool) {
    match &layer.content {
        Content::Raster { image, .. } => image.alpha_facts_once(|| scan(image)),
        Content::CmykRaster { image, .. } => image.alpha_facts_once(|| scan_cmyk(image)),
        _ => (false, false),
    }
}

/// [`alpha_facts`] of an RGB plane, read whole unless both facts turn up.
/// Each run is folded without branching (an AND of the alphas, an OR of the
/// colours at alpha 0), so the compiler can vectorize it, and the early exit
/// is tested once per run: the facts only ever turn true, so the answer is
/// the per-sample walk's.
fn scan(image: &PrecisionImage) -> (bool, bool) {
    let (mut translucent, mut hidden) = (false, false);
    if let Some(runs) = image.runs_rgba16() {
        for run in runs {
            let (mut alpha, mut colour) = (u16::MAX, 0u16);
            for p in run {
                alpha &= p[3];
                colour |= if p[3] == 0 { p[0] | p[1] | p[2] } else { 0 };
            }
            translucent |= alpha != u16::MAX;
            hidden |= colour != 0;
            if translucent && hidden {
                break;
            }
        }
    } else if let Some(runs) = image.runs_rgba32() {
        for run in runs {
            let (mut not_opaque, mut coloured) = (false, false);
            for p in run {
                not_opaque |= p[3] != 1.0;
                coloured |= (p[3] == 0.0) & ((p[0] != 0.0) | (p[1] != 0.0) | (p[2] != 0.0));
            }
            translucent |= not_opaque;
            hidden |= coloured;
            if translucent && hidden {
                break;
            }
        }
    }
    (translucent, hidden)
}

/// [`alpha_facts`] of a CMYK plane, folded per run as [`scan`] is.
fn scan_cmyk(image: &CmykImage) -> (bool, bool) {
    let (mut translucent, mut hidden) = (false, false);
    let (w, h) = image.dimensions();
    'rows: for y in 0..h {
        for (_, run) in image.row_segments(y, 0, w) {
            let (mut alpha, mut ink) = (u16::MAX, 0u16);
            for p in run {
                alpha &= p[4];
                ink |= if p[4] == 0 {
                    p[0] | p[1] | p[2] | p[3]
                } else {
                    0
                };
            }
            translucent |= alpha != u16::MAX;
            hidden |= ink != 0;
            if translucent && hidden {
                break 'rows;
            }
        }
    }
    (translucent, hidden)
}

impl PrecisionDocument {
    /// The pixel records whose plane has not yet answered [`alpha_facts`]:
    /// the scans the next list would run (module doc). A test seam.
    #[doc(hidden)]
    pub fn alpha_scans_pending(&self) -> usize {
        records(self.root_layers())
            .into_iter()
            .filter(|layer| match &layer.content {
                Content::Raster { image, .. } => !image.alpha_facts_known(),
                Content::CmykRaster { image, .. } => !image.alpha_facts_known(),
                _ => false,
            })
            .count()
    }

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
    /// here: whether a format keeps one is the host's (`saveLosses`). `tiff`
    /// says the file is a flat TIFF, which keeps the paths of a document
    /// kind whose TIFF carries them (`tiff_paths`). `[]` when the file
    /// reopens as this document.
    pub(crate) fn flat_state_losses(&self, alpha_kept: bool, tiff: bool) -> Vec<&'static str> {
        let roots = self.root_layers();
        let all = records(roots);
        let canvas = (self.width, self.height);
        let name = self.flat_layer_name();
        let paths_kept = self.paths.entries.is_empty() || (tiff && self.tiff_keeps_paths());
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
            paths: !paths_kept,
            comps: !self.comps.is_empty(),
            global_light: self.global_light != crate::style::GlobalLight::default(),
            // A float document's policy is its own (`linear_light` is
            // always `None` there), and its file reopens linear.
            evaluation: self.linear_light.is_some(),
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
