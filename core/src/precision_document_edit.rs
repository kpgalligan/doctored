//! Immutable typed structure edits preserve attached masks and document channels.
//!
//! `precision_document_edit.rs` preserves masks/channels through immutable structural changes.
use crate::precision_document::{
    scalar_and_name, Content, PrecisionDocument, PrecisionDocumentLayer,
};
/// `layer` with `link = 0` on itself and every record of its subtree — the
/// byte `duplicating_layer`'s per-copied-entry `link: 0`.
fn unlinked(mut layer: PrecisionDocumentLayer) -> PrecisionDocumentLayer {
    layer.link = 0;
    if let Content::Group(children) | Content::PassThrough(children) = &mut layer.content {
        let children = std::sync::Arc::make_mut(children);
        for child in children.iter_mut() {
            *child = unlinked(child.clone());
        }
    }
    layer
}

impl PrecisionDocument {
    fn edit(
        &self,
        index: usize,
        duplicate: bool,
        opacity: Option<f32>,
        replacement: Option<&PrecisionDocumentLayer>,
        preflight: bool,
    ) -> Result<Option<Self>, String> {
        let Some(target) = self.layer(index) else {
            return Err("high-depth layer index is out of range".into());
        };
        if opacity.is_some_and(|value| value == target.opacity) {
            return Ok(None);
        }
        if let Some(value) = opacity {
            scalar_and_name(target.name(), value)?;
        }
        fn change(
            layers: &[PrecisionDocumentLayer],
            index: &mut usize,
            duplicate: bool,
            opacity: Option<f32>,
            replacement: Option<&PrecisionDocumentLayer>,
        ) -> Result<Vec<PrecisionDocumentLayer>, String> {
            let mut out = Vec::with_capacity(layers.len() + usize::from(duplicate));
            let mut changed = false;
            for layer in layers {
                if changed || *index >= layer.records {
                    if !changed {
                        *index -= layer.records;
                    }
                    out.push(layer.clone());
                    continue;
                }
                let mut next = layer.clone();
                if let Content::Group(children) | Content::PassThrough(children) = &layer.content {
                    if *index < layer.records - 1 {
                        next = PrecisionDocumentLayer::group(
                            layer.name(),
                            change(children, index, duplicate, opacity, replacement)?,
                            layer.opacity,
                            layer.visible,
                            layer.is_pass_through(),
                        )?;
                        // The ENCLOSING group is the same group with a
                        // changed child, so it keeps its identity; `group`
                        // minted a fresh one for the rebuilt container
                        // (`doc_identity`).
                        next.id = layer.id;
                        next.mask = layer.mask.clone();
                        next.blend_mode = layer.blend_mode;
                        next.clipped = layer.clipped;
                        next.blend_if = layer.blend_if;
                        next.style = layer.style.clone();
                        next.label = layer.label;
                        next.reference = layer.reference;
                        next.locks = layer.locks;
                        next.link = layer.link;
                        next.open = layer.open;
                        out.push(next);
                        changed = true;
                        continue;
                    }
                }
                if duplicate {
                    // The original stays; the SECOND entry is the copy, and a
                    // copy is a new object — fresh ids through its whole
                    // subtree (`doc_identity`). The byte
                    // `DrDocument::duplicating_layer` is the specification:
                    // every copied entry is unlinked, and only the copied
                    // ROOT gains " copy" (no length cap there either).
                    out.push(layer.clone());
                    next = unlinked(next.freshly_identified());
                    std::sync::Arc::make_mut(&mut next.name).push_str(" copy");
                } else if let Some(value) = opacity {
                    next.opacity = value;
                } else if let Some(replacement) = replacement {
                    next = replacement.clone();
                }
                out.push(next);
                changed = true;
            }
            Ok(out)
        }
        let layers = change(
            &self.layers,
            &mut { index },
            duplicate,
            opacity,
            replacement,
        )?;
        let next = if preflight {
            self.rebuilt(
                self.width,
                self.height,
                layers,
                self.channels.as_ref().clone(),
            )?
        } else {
            // A planner supplies the eventual raster extents separately. A
            // master-sized placeholder is never a published document and must
            // not be checked as though it were the final fallback allocation.
            let mut next = self.clone();
            next.records = crate::precision_document::shape(&layers)?.0;
            next.layers = std::sync::Arc::new(layers);
            next
        };
        Ok(Some(next))
    }
    pub(crate) fn replacing_content(
        &self,
        index: usize,
        content: Content,
    ) -> Result<Option<Self>, String> {
        let mut layer = self
            .layer(index)
            .ok_or("high-depth layer index is out of range")?
            .clone();
        layer.content = content;
        layer.description = None;
        self.replacing_layer(index, layer)
    }
    pub(crate) fn replacing_layer(
        &self,
        index: usize,
        layer: PrecisionDocumentLayer,
    ) -> Result<Option<Self>, String> {
        self.edit(index, false, None, Some(&layer), true)
    }
    pub(crate) fn replacing_layer_for_plan(
        &self,
        index: usize,
        layer: PrecisionDocumentLayer,
    ) -> Result<Self, String> {
        self.edit(index, false, None, Some(&layer), false)?
            .ok_or_else(|| "failed to prepare hypothetical layer".into())
    }
    pub fn duplicate_layer(&self, index: usize) -> Result<Option<Self>, String> {
        // #547 Bitmap phase 2a-ii: a Bitmap document holds one layer too.
        if self.is_bitmap() {
            return Err(crate::bitmap_document::refused_sentence("duplicate_layer"));
        }
        // #547 Indexed phase 2d: an Indexed document holds one layer.
        if self.is_indexed() {
            return Err(crate::indexed_document::refused_sentence("duplicate_layer"));
        }
        self.edit(index, true, None, None, true)
    }
    pub fn set_layer_opacity(&self, index: usize, opacity: f32) -> Result<Option<Self>, String> {
        self.edit(index, false, Some(opacity), None, true)
    }
}
