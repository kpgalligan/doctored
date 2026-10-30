//! Shared native retouch output admission and immutable insertion. Gated groups
//! (opacity, mask, or a clip base above it) refuse because a sampled destination
//! plate already carries their contribution; the new layer inherits the
//! target's clipped flag, as the byte `doc_retouch_output.rs` rule does, and
//! NOT its Blend If or style (the byte `retouch_layer_above` copies only
//! `clipped`).
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{PrecisionImage, PrecisionStorage};
impl PrecisionDocument {
    pub(crate) fn retouch_output_refusal(&self, index: usize) -> Result<(), String> {
        fn walk(nodes: &[PrecisionDocumentLayer], mut index: usize) -> Result<(), String> {
            for node in nodes {
                if index >= node.records {
                    index -= node.records;
                    continue;
                }
                if let Some(children) = node.children() {
                    if index == node.records - 1 {
                        return Err("retouch output requires a raster target".into());
                    }
                    if node.isolates()
                        || node.opacity != 1.
                        || node.mask.is_some()
                        || !node.visible
                        || node.clipped
                    {
                        return Err("new retouch layer inside a gated group would apply its opacity, mask or clip twice; use in-place output".into());
                    }
                    return walk(children, index);
                }
                return Ok(());
            }
            Err("retouch output target missing".into())
        }
        walk(&self.layers, index)
    }
    pub(crate) fn retouch_insert(
        &self,
        index: usize,
        image: PrecisionImage,
        name: &str,
    ) -> Result<Self, String> {
        let clipped = self
            .layer(index)
            .ok_or("retouch output target missing")?
            .clipped;
        // The leaf is built by the image's own storage: a float document's
        // new layer goes through the one f32 leaf constructor.
        let repair = match image.storage() {
            PrecisionStorage::Rgba16Unorm => {
                PrecisionDocumentLayer::raster(name, image, (0, 0), 1., true)?
            }
            PrecisionStorage::Rgba32Float => {
                PrecisionDocumentLayer::raster_float(name, image, (0, 0), 1., true)?
            }
        }
        .with_clipped(clipped);
        self.retouch_insert_leaf(index, repair)
    }

    /// Inserts the new leaf `repair` directly above record `index` (a CMYK
    /// document's retouch builds its own CMYK leaf, #1).
    pub(crate) fn retouch_insert_leaf(
        &self,
        index: usize,
        repair: PrecisionDocumentLayer,
    ) -> Result<Self, String> {
        fn insert(
            nodes: &[PrecisionDocumentLayer],
            mut index: usize,
            repair: &PrecisionDocumentLayer,
        ) -> Result<Vec<PrecisionDocumentLayer>, String> {
            let mut out = Vec::with_capacity(nodes.len() + 1);
            let mut done = false;
            for node in nodes {
                if done || index >= node.records {
                    out.push(node.clone());
                    if !done {
                        index -= node.records;
                    }
                    continue;
                }
                if let Some(children) = node.children() {
                    let children = insert(children, index, repair)?;
                    let mut next = PrecisionDocumentLayer::group(
                        node.name(),
                        children,
                        node.opacity,
                        node.visible,
                        node.is_pass_through(),
                    )?;
                    // Same group, rebuilt around a new child: it keeps its
                    // own identity (`doc_identity`).
                    next.id = node.id();
                    next.mask = node.mask.clone();
                    next.blend_mode = node.blend_mode;
                    next.clipped = node.clipped;
                    next.blend_if = node.blend_if;
                    next.style = node.style.clone();
                    next.label = node.label;
                    next.reference = node.reference;
                    next.locks = node.locks;
                    next.link = node.link;
                    next.open = node.open;
                    next.description = node.description.clone();
                    out.push(next);
                } else {
                    out.push(node.clone());
                    out.push(repair.clone());
                }
                done = true;
            }
            Ok(out)
        }
        self.rebuilt(
            self.width,
            self.height,
            insert(&self.layers, index, &repair)?,
            self.channels.as_ref().clone(),
        )
    }
}
