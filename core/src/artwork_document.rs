//! Bounded opaque artwork descriptions attached atomically to native rasters.
//! The host owns text/layout semantics; destructive pixel writes clear the
//! description, while structural edits share its immutable allocation.
//!
//! Typed publication rebinds equal-byte ICC to the document's captured profile handle.
//! `ArtworkPublication.position` (12E-2b) routes a NEW record through the slot rule at both depths;
//! `None` is the pre-12E-2b code path verbatim, so `dr_doc_artwork` and `dr_precision_doc_artwork`
//! are unchanged and `dr_doc_artwork_at` / `dr_precision_doc_artwork_at` are ADDITIONS.
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, PrecisionImage, BOOKKEEPING_BYTES};
use serde::Deserialize;
use std::sync::Arc;

/// The word-less description slot: a description at most this long needs no
/// `.rz` word, and the Generative tags keep it as their cap.
pub const MAX_DESCRIPTION: usize = 64 * 1024;
/// The largest description: the 16 MiB meta cap. One over
/// [`MAX_DESCRIPTION`] (a pattern fill's custom tile) makes the `.rz` writers
/// claim word 102 (`large_description_native`).
pub const MAX_LARGE_DESCRIPTION: usize = crate::drdc::MAX_DRDC_META_LEN as usize;
pub const NATIVE_VERSION: u32 = 20;
pub(crate) struct ArtworkPublication<'a> {
    pub index: usize,
    pub replacing: bool,
    pub offset: (i32, i32),
    pub name: &'a str,
    pub description: &'a str,
    /// Where a NEW record lands (`layer_insert`), `index` being its anchor.
    /// `None` is the pre-12E-2b code path verbatim — byte artwork above the
    /// anchor's subtree, typed artwork pushed onto the root level — so the two
    /// original exports are unchanged. Ignored when `replacing`.
    pub position: Option<crate::layer_insert::InsertPosition>,
}

pub fn validate(text: &str) -> Result<(), String> {
    #[derive(Deserialize)]
    struct Kind {
        #[serde(rename = "type")]
        kind: String,
        version: u32,
    }
    if text.len() > MAX_LARGE_DESCRIPTION || text.contains('\0') {
        return Err("artwork description exceeds its bounded UTF-8 slot".into());
    }
    let kind: Kind =
        serde_json::from_str(text).map_err(|e| format!("invalid artwork description: {e}"))?;
    if !matches!(kind.kind.as_str(), "text" | "shape" | "fill" | "generated") || kind.version == 0 {
        return Err(
            "artwork description requires text, shape, fill or generated with a positive version"
                .into(),
        );
    }
    Ok(())
}

/// `text` is a valid description of artwork that re-renders from it: text,
/// shape or fill. A `generated` description (Generative Fill's tag)
/// only records where the pixels came from, so nothing about it depends on
/// the document's profile.
pub(crate) fn rerenders(text: &str) -> bool {
    #[derive(Deserialize)]
    struct Kind {
        #[serde(rename = "type")]
        kind: String,
    }
    validate(text).is_ok()
        && serde_json::from_str::<Kind>(text).is_ok_and(|kind| kind.kind != "generated")
}
impl crate::doc::DrDocument {
    pub(crate) fn artwork(
        &self,
        request: &ArtworkPublication<'_>,
        image: &PrecisionImage,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let ArtworkPublication {
            index,
            replacing,
            offset,
            name,
            description,
            position,
        } = *request;
        job.check()?;
        let mut inventory = crate::resources::Inventory::default();
        inventory.document(self);
        image.record_resources(&mut inventory);
        let pixels = u64::from(image.dimensions().0) * u64::from(image.dimensions().1);
        let scratch = 8 * BOOKKEEPING_BYTES
            + self.cloned_structure_bytes() * 3
            + pixels * 4
            + description.len() as u64 * 2;
        admit(inventory.unique_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        validate(description)?;
        if image.profile().bytes() != self.profile.bytes()
            || image.storage() != crate::precision_image::PrecisionStorage::Rgba16Unorm
        {
            return Err("artwork pixels require exact document ICC and native u16 source".into());
        }
        let old = self
            .layers
            .get(index)
            .ok_or("artwork layer index is out of range")?;
        if replacing
            && (old.retained_source.is_some() || !matches!(old.kind, crate::doc::LayerKind::Raster))
        {
            return Err("artwork replacement requires an ordinary raster".into());
        }
        let bytes =
            image.quantize_with_context(crate::precision_quantize::PrecisionDither::None, job)?;
        let (mut next, at) = if replacing {
            if old.pixels.equals_image(&bytes) && old.offset == offset {
                if old.meta.as_deref() == Some(description) {
                    return Ok(None);
                }
                (self.clone(), index)
            } else {
                (
                    self.set_layer_content_preserving_mask(index, bytes, offset)
                        .ok_or(
                            "artwork replacement was refused by layer locks or native capacity",
                        )?,
                    index,
                )
            }
        } else if let Some(position) = position {
            // The ONE slot rule (`layer_insert`): `index` is the anchor, and
            // the slot is resolved ONCE here and spliced at, exactly as the
            // typed twin below does. The capacity check is separate so the
            // capacity sentence can only ever be about capacity; a splice that
            // `doc_group::validated` refuses gets its own sentence.
            let slot = self.insert_slot(index, position)?;
            if !self.native_layers_fit_dimensions(
                self.layers
                    .iter()
                    .map(|l| l.pixels.dimensions())
                    .chain(std::iter::once(bytes.dimensions())),
            ) {
                return Err("artwork insertion exceeds native capacity".into());
            }
            let mut next = self
                .inserting_layer(crate::doc::Layer::new(bytes, name), slot)
                .ok_or("artwork insertion would leave a malformed layer structure")?;
            next.layers[slot.at].offset = offset;
            (next, slot.at)
        } else {
            let at = crate::doc_group::subtree(&self.layers, index).end;
            let mut next = self
                .adding_image_layer(index, bytes, name)
                .ok_or("artwork insertion exceeds native capacity")?;
            next.layers[at].offset = offset;
            (next, at)
        };
        next.layers[at].meta = Some(description.into());
        next.native_save_preflight()?;
        job.check()?;
        Ok(Some(next))
    }
}
impl PrecisionDocument {
    pub fn artwork_description(&self, index: usize) -> Option<&str> {
        self.layer(index)?
            .description
            .as_deref()
            .map(String::as_str)
    }
    /// The LOCK gate (`precision_lock`): REPLACING a record's artwork
    /// rewrites its pixels, so that branch runs under a `Pixels` edit.
    /// Publishing to a NEW record names no target and is ungated, exactly as
    /// every other new-layer path is. On a Gray document (#1 phase 6a-i)
    /// the published image is folded to its gray first
    /// (`gray_artwork::fold_artwork`): its coloured pixels by the Mode rule,
    /// on the ×257 grid at 8 bits. On an 8-bit Lab document (#1 Lab phase
    /// 6a-i) it is put on the ×257 grid (`lab_artwork::on_document_grid`).
    pub(crate) fn artwork(
        &self,
        request: &ArtworkPublication<'_>,
        image: &PrecisionImage,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        // A CMYK document's artwork is ink (`cmyk_artwork`); an RGB image
        // has no place in it.
        if self.is_cmyk() {
            return Err("RGB artwork is unavailable in a CMYK document".into());
        }
        if !request.replacing {
            return self.artwork_unlocked(request, image, limit, other, job);
        }
        self.under_locks(request.index, crate::doc_lock::EditKind::Pixels, |doc| {
            doc.artwork_unlocked(request, image, limit, other, job)
        })
    }

    fn artwork_unlocked(
        &self,
        request: &ArtworkPublication<'_>,
        image: &PrecisionImage,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let ArtworkPublication {
            index,
            replacing,
            offset,
            name,
            description,
            position,
        } = *request;
        let anchor = index;
        let index = replacing.then_some(index);
        // A Gray document's fold (`gray_artwork`) may write a new image:
        // 8 B/px at u16, 16 at float.
        let fold = if self.is_gray() {
            let (w, h) = image.dimensions();
            let per_pixel = if self.is_float() { 16 } else { 8 };
            u64::from(w) * u64::from(h) * per_pixel
        } else {
            0
        };
        // An 8-bit Lab document's grid (`lab_artwork`) may write one too.
        let grid = {
            let (w, h) = image.dimensions();
            crate::lab_artwork::grid_bytes(self, u64::from(w) * u64::from(h))
        };
        let scratch = 8 * BOOKKEEPING_BYTES
            + self.layer_count() as u64 * 4096
            + description.len() as u64 * 2
            + fold
            + grid;
        admit(
            self.retained_bytes() + image.retained_bytes(),
            scratch,
            other,
            limit,
        )?;
        job.reserve(scratch)?;
        job.check()?;
        validate(description)?;
        // A 32-bit float document takes `artwork_render::render_float`'s f32
        // image (187b); every other document keeps the u16 rule.
        let float = self.is_float();
        if float
            && (image.profile().bytes() != self.profile().bytes()
                || image.storage() != crate::precision_image::PrecisionStorage::Rgba32Float)
        {
            return Err(
                "artwork pixels require the exact document profile and the document's own storage"
                    .into(),
            );
        }
        if !float
            && (image.profile().bytes() != self.profile().bytes()
                || image.storage() != crate::precision_image::PrecisionStorage::Rgba16Unorm)
        {
            return Err(
                "artwork pixels require the exact document profile and native u16 storage".into(),
            );
        }
        let image = image.with_profile(self.profile.clone());
        // A Gray document stores the image's gray (`gray_artwork`); `None`
        // means it already is, and the no-op compare below runs on it.
        let image = match self.gray_policy() {
            Some(policy) => {
                crate::gray_artwork::fold_artwork(&image, policy, job)?.unwrap_or(image)
            }
            None => image,
        };
        // An 8-bit Lab document stores the image on the ×257 grid
        // (`lab_artwork`); `None` keeps the image for the no-op compare.
        let image = match self.lab_policy() {
            Some(policy) => {
                crate::lab_artwork::on_document_grid(&image, policy, job)?.unwrap_or(image)
            }
            None => image,
        };
        let leaf = |name: &str, image: PrecisionImage, offset: (i32, i32)| {
            if float {
                PrecisionDocumentLayer::raster_float(name, image, offset, 1.0, true)
            } else {
                PrecisionDocumentLayer::raster(name, image, offset, 1.0, true)
            }
        };
        let next = if let Some(index) = index {
            let old = self
                .layer(index)
                .ok_or("artwork layer index is out of range")?;
            let old_image = old
                .source()
                .ok_or("artwork replacement requires a raster layer")?;
            if old.retained_source.is_some() {
                return Err("artwork cannot replace a retained image source".into());
            }
            if old_image.dimensions() == image.dimensions()
                && old_image.same_samples(&image)
                && old.offset() == offset
                && self.artwork_description(index) == Some(description)
            {
                return Ok(None);
            }
            let mut layer = old.clone();
            layer.content = Content::Raster {
                image: image.clone(),
                offset,
            };
            layer.description = Some(Arc::new(description.into()));
            self.replacing_layer(index, layer)?
        } else if let Some(position) = position {
            // The ONE slot rule (`layer_insert`): `request.index` is the
            // anchor, resolved before anything is admitted.
            let slot = self.insert_slot(anchor, position)?;
            self.preflight_raster_addition(image.dimensions(), name)?;
            let mut layer = leaf(name, image.clone(), offset)?;
            layer.description = Some(Arc::new(description.into()));
            Some(self.inserting_record(layer, slot)?)
        } else {
            self.preflight_raster_addition(image.dimensions(), name)?;
            let mut layer = leaf(name, image.clone(), offset)?;
            layer.description = Some(Arc::new(description.into()));
            let mut layers = self.layers.as_ref().clone();
            layers.push(layer);
            Some(self.rebuilt(
                self.width,
                self.height,
                layers,
                self.channels.as_ref().clone(),
            )?)
        };
        job.check()?;
        Ok(next)
    }
    pub(crate) fn artwork_metadata(
        &self,
        index: usize,
        description: Option<&str>,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let old = self
            .layer(index)
            .ok_or("artwork layer index is out of range")?;
        // A CMYK raster carries a description as an RGB one does (#1).
        if (old.source().is_none() && old.cmyk_source().is_none()) || old.retained_source.is_some()
        {
            return Err("artwork metadata requires an ordinary raster".into());
        }
        if self.artwork_description(index) == description {
            return Ok(None);
        }
        // The LOCK gate (`precision_lock`), CONSULTED rather than wrapped: a
        // description is not pixels, so there is no alpha to freeze — and
        // `under_locks`' no-op latch, which compares pixels and mask alone,
        // would throw a description-only change away.
        self.lock_refusal(index, crate::doc_lock::EditKind::Pixels)?;
        admit(
            self.retained_bytes(),
            8 * BOOKKEEPING_BYTES + self.layer_count() as u64 * 4096,
            other,
            limit,
        )?;
        job.check()?;
        if let Some(text) = description {
            validate(text)?;
        }
        let mut layer = old.clone();
        layer.description = description.map(|s| Arc::new(s.into()));
        self.replacing_layer(index, layer)
    }
}
