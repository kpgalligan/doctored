//! Bounded application inspection without copying pixel storage or adjustment
//! payloads. Layer order is postorder, identical to the public layer indices.

use serde_json::{json, Value};

use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::PrecisionImage;

pub(crate) fn inspection_bytes(document: &PrecisionDocument) -> u64 {
    fn text(layers: &[PrecisionDocumentLayer]) -> u64 {
        layers
            .iter()
            .map(|layer| layer.name().len() as u64 + layer.children().map(text).unwrap_or(0))
            .sum()
    }
    let text = text(&document.layers)
        + document
            .channels
            .iter()
            .map(|c| {
                c.name().len() as u64
                    + c.metadata()
                        .provenance
                        .as_ref()
                        .map_or(0, |s| s.len() as u64)
            })
            .sum::<u64>()
        + document
            .import_warnings
            .iter()
            .map(|s| s.len() as u64)
            .sum::<u64>()
        + document.profile.name().len() as u64;
    // Covers UTF-8/UTF-16 strings, worst-case JSON escaping, scalar dictionaries,
    // arrays and a copied ICC profile. Host also reserves temporary parse space.
    1024 * 1024
        + text * 16
        + document.records as u64 * 8192
        + document.channels.len() as u64 * 1024
        + document.profile.bytes().len() as u64 * 2
}

pub(crate) fn layer_info(document: &PrecisionDocument) -> Vec<Value> {
    fn walk(
        layers: &[PrecisionDocumentLayer],
        depth: usize,
        out: &mut Vec<Value>,
    ) -> Option<[i64; 4]> {
        let mut union: Option<[i64; 4]> = None;
        for layer in layers {
            let start = out.len();
            let bounds = if let Some(children) = layer.children() {
                walk(children, depth + 1, out)
            } else {
                // A CMYK record (`cmyk_document`) reports its rectangle too.
                let dimensions = layer
                    .source()
                    .map(|image| image.dimensions())
                    .or_else(|| layer.cmyk_source().map(|image| image.dimensions()));
                dimensions.map(|(w, h)| {
                    let (x, y) = layer.offset();
                    [i64::from(x), i64::from(y), i64::from(w), i64::from(h)]
                })
            };
            if let Some(b) = bounds {
                union = Some(match union {
                    None => b,
                    Some(a) => {
                        let x = a[0].min(b[0]);
                        let y = a[1].min(b[1]);
                        [
                            x,
                            y,
                            (a[0] + a[2]).max(b[0] + b[2]) - x,
                            (a[1] + a[3]).max(b[1] + b[3]) - y,
                        ]
                    }
                });
            }
            let adjustment = match &layer.content {
                Content::Adjustment(state) => Some(json!({"operation":state.kernel.operation(),
                    "evaluation":"precision_v1", "description_bytes":state.description().len()})),
                // A CMYK Levels/Curves record (`cmyk_adjust_layer`) reports as
                // an adjustment, its ink domain named.
                Content::CmykAdjustment(state) => Some(json!({"operation":state.operation(),
                    "evaluation":"precision_v1", "domain":"cmyk_ink",
                    "description_bytes":state.description().len()})),
                _ => None,
            };
            out.push(json!({"index":out.len(), "depth":depth, "subtree_start":start,
                // The STABLE identity beside the renumberable index
                // (`doc_identity`): this is the typed depth's answer to
                // `dr_doc_layer_id`, carried in the one bounded metadata read
                // a host already makes per document change.
                "id":layer.id(),
                "bounds":bounds, "name":layer.name(),
                "kind":if layer.source().is_some() || layer.cmyk_source().is_some(){"raster"}else if adjustment.is_some(){"adjustment"}
                    else if layer.is_pass_through(){"pass_through_group"}
                    else if layer.blend_mode() != crate::blend::BlendMode::Normal {"isolated_blend_group"}else{"isolated_normal_group"},
                "blend_mode":layer.blend_mode().to_c(), "blend_evaluation":"encoded_rgb_f32_v1",
                "opacity":layer.opacity(), "visible":layer.visible(), "clipped":layer.clipped(), "offset":layer.offset(),
                // The colour LABEL, 0..=7 (`doc_label`); the typed depth's
                // answer to `dr_doc_layer_label`.
                "label":layer.label(),
                // The REFERENCE POINT (`fxrp`), the typed depth's
                // answer to `dr_doc_layer_reference_x` / `_y`.
                "reference":[layer.reference.0, layer.reference.1],
                // The four LOCK bits (`doc_lock::LOCK_*`) and the LINK-GROUP
                // id (0 = unlinked), the typed depth's answers to
                // `dr_doc_layer_locks` and `dr_doc_layer_link`
                // (`precision_lock`).
                "locks":layer.locks(), "link":layer.link(),
                // The group DISCLOSURE, the typed depth's answer to
                // `dr_doc_layer_open`; `true` on every non-group row.
                "open":layer.open(),
                "blend_if":layer.blend_if().as_ref().map_or(Value::Null, crate::style_json_write::blend_if_value),
                "style":layer.reported_style().map_or(Value::Null, Value::String),
                "dimensions":layer.source().map(PrecisionImage::dimensions).or_else(|| layer.cmyk_source().map(|image| image.dimensions())), "adjustment":adjustment,
                "description_bytes":layer.description.as_ref().map(|s|s.len()),
                "mask":layer.mask().map(|m|json!({"dimensions":m.plane().dimensions(),
                    "offset":m.properties().offset, "enabled":m.enabled(), "linked":m.properties().linked,
                    "density":m.properties().density, "feather":m.properties().feather,
                    "outside_coverage":m.properties().outside_coverage}))}));
        }
        union
    }
    let mut result = Vec::with_capacity(document.records);
    walk(&document.layers, 0, &mut result);
    result
}
