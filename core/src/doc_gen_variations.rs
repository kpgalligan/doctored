//! A Generative Layer's VARIATIONS (M3b phase 2) at 8 bits
//! (`DrDocument`) and 16 bits (`PrecisionDocument`, RGB and 8-bit CMYK):
//! the two bindings, their live rule, and the four ops the host calls —
//! info, select, the input of a new set ("more") and its add.
//!
//! A binding keeps the table (`gen_variations::GeneratedVariations`) beside
//! the raster it rendered, the `doc_warp` / `retained_source` pattern. It is
//! LIVE (`effective_generated` answers `Some`) only while all of these hold:
//! the layer's tag (byte `meta`, typed description) is a `generated` one
//! (`doc_gen_fill::check_tag`); its pixels are the very raster apply, select,
//! add or the reader bound (`ptr_eq`) at the bound offset, so a paint, a
//! filter, a move or a transform detaches it; the document profile's bytes
//! are the bound one's, so Convert and Assign detach it; the layer has a
//! mask whose plane is `raster`-sized at (0, 0) — a mask edit keeps it
//! live, deleting or moving the mask detaches it, and more-input reads its
//! coverage there; and `ink` matches the document's model. Every reader,
//! op and writer asks the rule, so a non-live entry is absent everywhere,
//! and `retained_source::detach_invalid_sources` / `clean_typed_layers`
//! drop one so it pins no stale pixels. Only the depth conversions rebind
//! (`precision_conversion_structure`, `precision_conversion_narrow`).
//!
//! Select re-renders the whole raster from one picture (transparent outside
//! the covered rect, `gen_fill::fill_at` inside, the mask kept) under the
//! layer's Pixels lock, swaps the tag for that picture's and moves
//! `selected`; selecting the current one changes nothing (`None`). Add
//! appends pictures under the caps and selects the first new one. Each op
//! builds its binding on the FINAL document, after the lock wrapper's alpha
//! restore, so the restored raster is the one bound.

use std::sync::Arc;

use crate::cmyk_image::CmykImage;
use crate::doc::{DrDocument, Layer, LayerKind, LayerRaster};
use crate::doc_gen_fill::{
    begin, check_items, check_results, check_tag, encode_results, out_of_range, rect, render_byte,
    render_rgb16, GenResult,
};
use crate::doc_lock::EditKind;
use crate::gen_fill::scratch_bytes;
use crate::gen_variations::{check_document, decode, GeneratedVariations};
use crate::icc::IccProfile;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, reserved_vec, PrecisionImage};
use crate::resources::Inventory;

/// A byte Generative Layer's binding (module doc).
#[derive(Clone)]
pub struct GeneratedBinding8 {
    pub table: Arc<GeneratedVariations>,
    pub rendered: LayerRaster,
    pub rendered_offset: (i32, i32),
    pub profile: Arc<IccProfile>,
}

/// The raster a typed binding rendered: RGB u16 or CMYK ink.
#[derive(Clone)]
pub(crate) enum GeneratedPlane16 {
    Rgb(PrecisionImage),
    Cmyk(CmykImage),
}

/// A typed Generative Layer's binding (module doc).
#[derive(Clone)]
pub(crate) struct GeneratedBinding16 {
    pub(crate) table: Arc<GeneratedVariations>,
    pub(crate) rendered: GeneratedPlane16,
    pub(crate) rendered_offset: (i32, i32),
    pub(crate) profile: Arc<IccProfile>,
}

/// The refusal for a layer holding no live variations.
pub(crate) fn no_variations(layer: usize) -> String {
    format!("generative layer {layer} holds no variations")
}

/// `index` as a variation of `table`, else its refusal.
fn variation_index(table: &GeneratedVariations, index: u32) -> Result<usize, String> {
    let n = table.items.len();
    let k = index as usize;
    if k >= n {
        return Err(format!(
            "generative variation {index} is out of range (the layer holds {n})"
        ));
    }
    Ok(k)
}

/// `[count, selected, native w, native h, crop x, y, w, h]`.
fn info(table: &GeneratedVariations) -> [u32; 8] {
    [
        table.items.len() as u32,
        table.selected as u32,
        table.native.0,
        table.native.1,
        table.crop[0],
        table.crop[1],
        table.crop[2],
        table.crop[3],
    ]
}

fn generated_tag(tag: Option<&str>) -> bool {
    tag.is_some_and(|t| check_tag(t).is_ok())
}

/// What a select holds: the kernel's own terms with no stored picture, plus
/// the decoded picture.
fn select_plan(table: &GeneratedVariations, size: (u32, u32), bpp: u64, mask: u64) -> u64 {
    let (w, h) = table.raster;
    scratch_bytes(w, h, table.native, bpp, mask, 0)
        .saturating_add(4 * u64::from(size.0) * u64::from(size.1))
}

fn record_table(table: &Arc<GeneratedVariations>, inventory: &mut Inventory) {
    inventory.object(&**table, "generated_variations", false);
    inventory.vec(&table.items, "generated_variations", false);
    for v in table.items.iter() {
        inventory.record(
            v.png.as_ptr() as usize,
            v.png.len(),
            "generated_variations",
            false,
        );
        inventory.record(
            v.tag.as_ptr() as usize,
            v.tag.len(),
            "generated_variations",
            false,
        );
    }
}

impl GeneratedBinding8 {
    pub(crate) fn record_resources(&self, inventory: &mut Inventory) {
        record_table(&self.table, inventory);
        self.rendered
            .record_resources(inventory, "layer_pixels", false);
        inventory.object(&*self.profile, "profile", false);
        self.profile.record_resources(inventory);
    }
}

impl GeneratedBinding16 {
    pub(crate) fn record_resources(&self, inventory: &mut Inventory) {
        record_table(&self.table, inventory);
        match &self.rendered {
            GeneratedPlane16::Rgb(image) => image.record_resources(inventory),
            GeneratedPlane16::Cmyk(image) => image.record_resources(inventory),
        }
    }
}

impl Layer {
    /// The binding when it is live (module doc), else `None`.
    pub(crate) fn effective_generated(
        &self,
        profile: &Arc<IccProfile>,
    ) -> Option<GeneratedBinding8> {
        let binding = self.generated.as_ref()?;
        let mask = self.mask.as_ref()?;
        if self.kind != LayerKind::Raster
            || binding.table.ink
            || !LayerRaster::ptr_eq(&self.pixels, &binding.rendered)
            || self.offset != binding.rendered_offset
            || profile.bytes() != binding.profile.bytes()
            || mask.dimensions() != binding.table.raster
            || self.mask_properties.offset != (0, 0)
            || !generated_tag(self.meta.as_deref())
        {
            return None;
        }
        Some(binding.clone())
    }
}

impl PrecisionDocumentLayer {
    /// The binding when it is live (module doc) in a document of `profile`
    /// that is CMYK when `cmyk`, else `None`.
    pub(crate) fn effective_generated(
        &self,
        profile: &Arc<IccProfile>,
        cmyk: bool,
    ) -> Option<GeneratedBinding16> {
        let binding = self.generated.as_ref()?;
        let mask = self.mask.as_ref()?;
        let same = match (&self.content, &binding.rendered) {
            (Content::Raster { image, offset }, GeneratedPlane16::Rgb(rendered)) => {
                PrecisionImage::ptr_eq(image, rendered) && *offset == binding.rendered_offset
            }
            (Content::CmykRaster { image, offset }, GeneratedPlane16::Cmyk(rendered)) => {
                CmykImage::ptr_eq(image, rendered) && *offset == binding.rendered_offset
            }
            _ => false,
        };
        if !same
            || binding.table.ink != cmyk
            || profile.bytes() != binding.profile.bytes()
            || mask.plane().dimensions() != binding.table.raster
            || mask.properties().offset != (0, 0)
            || !generated_tag(self.description.as_deref().map(String::as_str))
        {
            return None;
        }
        Some(binding.clone())
    }
}

impl DrDocument {
    /// Layer `layer`'s live binding, else the refusal by name.
    fn live_generated(&self, layer: usize) -> Result<GeneratedBinding8, String> {
        let entry = self.layers.get(layer).ok_or_else(|| out_of_range(layer))?;
        entry
            .effective_generated(&self.profile)
            .ok_or_else(|| no_variations(layer))
    }

    /// Every live table's stored picture bytes.
    pub(crate) fn variation_bytes(&self) -> u64 {
        self.layers
            .iter()
            .filter_map(|l| l.effective_generated(&self.profile))
            .map(|b| b.table.png_bytes())
            .sum()
    }

    /// The document cap with `added` more stored bytes.
    pub(crate) fn check_variation_room(&self, added: u64) -> Result<(), String> {
        check_document(self.variation_bytes().saturating_add(added))
    }

    /// `[count, selected, native w, native h, crop x, y, w, h]` of layer
    /// `layer`'s live variations; `None` for a layer holding none. `Err` for
    /// an out-of-range index.
    pub fn gen_fill_variations(&self, layer: usize) -> Result<Option<[u32; 8]>, String> {
        let entry = self.layers.get(layer).ok_or_else(|| out_of_range(layer))?;
        Ok(entry
            .effective_generated(&self.profile)
            .map(|b| info(&b.table)))
    }

    fn refuse_locked(&self, layer: usize) -> Result<(), String> {
        if self.lock_block(layer, EditKind::Pixels) != 0 {
            return Err(format!("generative layer {layer} is locked"));
        }
        Ok(())
    }

    /// Layer `layer` re-rendered from variation `index` (module doc); `None`
    /// when it is already selected. `Err` for an out-of-range index or
    /// variation, a layer holding no variations, a lock, a refused plan, a
    /// picture that does not decode and a cancel.
    pub fn gen_fill_select_variation(
        &self,
        layer: usize,
        index: u32,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<DrDocument>, String> {
        let binding = self.live_generated(layer)?;
        let table = &binding.table;
        let k = variation_index(table, index)?;
        if k == table.selected {
            return Ok(None);
        }
        self.refuse_locked(layer)?;
        let (_run, job) = begin(control)?;
        let picked = &table.items[k];
        let plan = select_plan(table, picked.size, 4, 1);
        admit(self.gen_fill_retained(), plan, other, limit)?;
        job.reserve(plan)?;
        let rgba = decode(picked)?;
        let image = render_byte(table, &rgba, picked.size, job)?;
        drop(rgba);
        let next = Arc::new(GeneratedVariations {
            selected: k,
            ..(**table).clone()
        });
        let out = self.publish_variation(layer, image, next, &picked.tag);
        job.report(1.0, 5)?;
        out
    }

    /// `image` as layer `layer`'s pixels under its Pixels lock, then the tag
    /// and the binding to `table` on the final document. A restore that left
    /// the pixels unchanged keeps them and moves only the tag and selection.
    fn publish_variation(
        &self,
        layer: usize,
        image: image::RgbaImage,
        table: Arc<GeneratedVariations>,
        tag: &str,
    ) -> Result<Option<DrDocument>, String> {
        let raster = LayerRaster::unseeded(image);
        let mut next = self
            .under_locks(layer, EditKind::Pixels, |doc| {
                let mut out = doc.clone();
                out.layers[layer].pixels = raster;
                Some(out)
            })
            .unwrap_or_else(|| self.clone());
        let profile = next.profile.clone();
        let entry = &mut next.layers[layer];
        entry.meta = Some(tag.to_string());
        entry.generated = Some(GeneratedBinding8 {
            table,
            rendered: entry.pixels.clone(),
            rendered_offset: entry.offset,
            profile,
        });
        Ok(Some(next))
    }

    /// The provider's two pictures for a new set of layer `layer`'s
    /// variations: phase 1's input over the stored crop and native size,
    /// the coverage the layer's own mask, the plate with the layer hidden.
    /// The plan and the coverage copy are admitted before the copy is made.
    /// `Err` for an out-of-range index, a layer holding no variations, a bad
    /// buffer, a refused plan and a cancel.
    pub fn gen_fill_more_input(
        &self,
        layer: usize,
        image_out: &mut [u8],
        mask_out: &mut [u8],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<(), String> {
        let binding = self.live_generated(layer)?;
        let table = &binding.table;
        let plane = self.layers[layer]
            .mask
            .as_ref()
            .ok_or_else(|| no_variations(layer))?;
        let n = plane.as_raw().len();
        let plan = self
            .gen_fill_plan(table.native, 0)?
            .saturating_add(2 * n as u64);
        admit(self.gen_fill_retained(), plan, other, limit)?;
        let mut coverage: Vec<u16> = reserved_vec(n)?;
        coverage.extend(plane.as_raw().iter().map(|&v| u16::from(v) * 257));
        let hidden = self
            .with_layer_visible(layer, false)
            .unwrap_or_else(|| self.clone());
        hidden.gen_fill_input_at(
            layer,
            &coverage,
            table.native,
            Some(rect(table.crop)),
            2 * n as u64,
            image_out,
            mask_out,
            limit,
            other,
            control,
        )?;
        Ok(())
    }

    /// `results` appended to layer `layer`'s variations, tagged `tag`, the
    /// first new one selected and rendered. `Err` for an out-of-range index,
    /// a layer holding no variations, a bad result or tag, a lock, a cap, a
    /// refused plan and a cancel.
    #[allow(clippy::too_many_arguments)]
    pub fn gen_fill_add_variations(
        &self,
        layer: usize,
        results: &[GenResult<'_>],
        tag: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<DrDocument>, String> {
        let binding = self.live_generated(layer)?;
        let table = &binding.table;
        check_results(results)?;
        check_tag(tag)?;
        self.refuse_locked(layer)?;
        let (_run, job) = begin(control)?;
        let (w, h) = table.raster;
        let plan = scratch_bytes(w, h, table.native, 4, 1, results.len() as u64);
        admit(self.gen_fill_retained(), plan, other, limit)?;
        job.reserve(plan)?;
        let next = grown(table, encode_results(results, tag, job)?)?;
        self.check_variation_room(next.png_bytes() - table.png_bytes())?;
        let (pixels, size) = results[0];
        let image = render_byte(&next, pixels, size, job)?;
        let out = self.publish_variation(layer, image, Arc::new(next), tag);
        job.report(1.0, 5)?;
        out
    }
}

/// `table` with `added` appended and the first of them selected, under the
/// caps.
fn grown(
    table: &GeneratedVariations,
    added: Vec<crate::gen_variations::Variation>,
) -> Result<GeneratedVariations, String> {
    let first = table.items.len();
    let mut items = (*table.items).clone();
    items.extend(added);
    check_items(&items)?;
    Ok(GeneratedVariations {
        selected: first,
        items: Arc::new(items),
        ..table.clone()
    })
}

impl PrecisionDocument {
    /// Record `layer`'s live binding, else the refusal by name.
    fn live_generated(&self, layer: usize) -> Result<GeneratedBinding16, String> {
        let record = self.layer(layer).ok_or_else(|| out_of_range(layer))?;
        record
            .effective_generated(self.profile(), self.is_cmyk())
            .ok_or_else(|| no_variations(layer))
    }

    /// Every live table's stored picture bytes, over every record.
    pub(crate) fn variation_bytes(&self) -> u64 {
        (0..self.records)
            .filter_map(|i| self.layer(i))
            .filter_map(|l| l.effective_generated(self.profile(), self.is_cmyk()))
            .map(|b| b.table.png_bytes())
            .sum()
    }

    /// The document cap with `added` more stored bytes.
    pub(crate) fn check_variation_room(&self, added: u64) -> Result<(), String> {
        check_document(self.variation_bytes().saturating_add(added))
    }

    /// The 16-bit twin of [`DrDocument::gen_fill_variations`].
    pub fn gen_fill_variations(&self, layer: usize) -> Result<Option<[u32; 8]>, String> {
        let record = self.layer(layer).ok_or_else(|| out_of_range(layer))?;
        Ok(record
            .effective_generated(self.profile(), self.is_cmyk())
            .map(|b| info(&b.table)))
    }

    /// One picture as the record's plane (RGB u16, or ink on CMYK).
    fn variation_plane(
        &self,
        table: &GeneratedVariations,
        pixels: &[u8],
        size: (u32, u32),
        job: JobContext<'_>,
    ) -> Result<GeneratedPlane16, String> {
        Ok(if self.is_cmyk() {
            GeneratedPlane16::Cmyk(self.cmyk_gen_fill_render(table, pixels, size, job)?)
        } else {
            // A Gray document folds the picture to its gray (#547 phase 6b)
            // and a Lab one renders it in Display P3 and returns it to Lab
            // codes (Lab phase 6b-iii, `doc_gen_fill`'s module doc), so the
            // result is the published raster.
            let profile = crate::lab_neural::result_profile(self);
            let rendered = render_rgb16(table, pixels, size, &profile, job)?;
            GeneratedPlane16::Rgb(crate::lab_neural::published(self, rendered, job)?)
        })
    }

    /// The bytes a pixel of the record's plane takes and its CMYK leg's
    /// profile term; on a Gray document the plane's 8 B/px plus the fold's
    /// image (`gray_artwork::fold_bytes`, 8 B/px at 8 and 16 bits), on a Lab
    /// one the plane's 8 B/px plus the Lab image and the way back's tables
    /// (`lab_neural::published_tables`).
    fn variation_terms(&self) -> (u64, u64) {
        match self.cmyk_policy() {
            Some(policy) => (10, 128 * policy.profile_bytes().len() as u64),
            None if self.is_gray() => (16, 0),
            None if self.is_lab() => (16, crate::lab_neural::published_tables(self)),
            None => (8, 0),
        }
    }

    /// The 16-bit twin of [`DrDocument::gen_fill_select_variation`]; a
    /// float or 16-bit CMYK document refuses by name.
    pub fn gen_fill_select_variation(
        &self,
        layer: usize,
        index: u32,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<PrecisionDocument>, String> {
        self.gen_fill_depth()?;
        let binding = self.live_generated(layer)?;
        let table = &binding.table;
        let k = variation_index(table, index)?;
        if k == table.selected {
            return Ok(None);
        }
        self.lock_refusal(layer, EditKind::Pixels)?;
        let (_run, job) = begin(control)?;
        let picked = &table.items[k];
        let (bpp, profile) = self.variation_terms();
        let plan = select_plan(table, picked.size, bpp, 2).saturating_add(profile);
        admit(self.retained_bytes(), plan, other, limit)?;
        job.reserve(plan)?;
        let rgba = decode(picked)?;
        let plane = self.variation_plane(table, &rgba, picked.size, job)?;
        drop(rgba);
        let next = Arc::new(GeneratedVariations {
            selected: k,
            ..(**table).clone()
        });
        let out = self.publish_variation(layer, plane, next, &picked.tag);
        job.report(1.0, 5)?;
        out
    }

    /// `plane` as record `layer`'s content under its Pixels lock, then the
    /// tag and the binding to `table` on the final document.
    fn publish_variation(
        &self,
        layer: usize,
        plane: GeneratedPlane16,
        table: Arc<GeneratedVariations>,
        tag: &str,
    ) -> Result<Option<PrecisionDocument>, String> {
        let next = self
            .under_locks(layer, EditKind::Pixels, |doc| {
                let mut record = doc.layer(layer).ok_or_else(|| out_of_range(layer))?.clone();
                let offset = record.offset();
                record.content = match &plane {
                    GeneratedPlane16::Rgb(image) => Content::Raster {
                        image: image.clone(),
                        offset,
                    },
                    GeneratedPlane16::Cmyk(image) => Content::CmykRaster {
                        image: image.clone(),
                        offset,
                    },
                };
                doc.replacing_layer(layer, record)
            })?
            .unwrap_or_else(|| self.clone());
        let mut record = next
            .layer(layer)
            .ok_or_else(|| out_of_range(layer))?
            .clone();
        let rendered = match &record.content {
            Content::Raster { image, .. } => GeneratedPlane16::Rgb(image.clone()),
            Content::CmykRaster { image, .. } => GeneratedPlane16::Cmyk(image.clone()),
            _ => return Err(no_variations(layer)),
        };
        record.description = Some(Arc::new(tag.to_string()));
        record.generated = Some(GeneratedBinding16 {
            table,
            rendered,
            rendered_offset: record.offset(),
            profile: next.profile().clone(),
        });
        Ok(Some(
            next.replacing_layer(layer, record)?
                .ok_or("generative fill could not bind its variations")?,
        ))
    }

    /// The 16-bit twin of [`DrDocument::gen_fill_more_input`]: the plate has
    /// the record at opacity 0.
    pub fn gen_fill_more_input(
        &self,
        layer: usize,
        image_out: &mut [u8],
        mask_out: &mut [u8],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<(), String> {
        self.gen_fill_depth()?;
        let binding = self.live_generated(layer)?;
        let table = &binding.table;
        let record = self.layer(layer).ok_or_else(|| out_of_range(layer))?;
        let plane = record.mask().ok_or_else(|| no_variations(layer))?.plane();
        let samples = plane.samples();
        let plan = self
            .gen_fill_plan(table.native, 0)?
            .saturating_add(2 * samples.len() as u64);
        admit(self.retained_bytes(), plan, other, limit)?;
        let mut coverage: Vec<u16> = reserved_vec(samples.len())?;
        coverage.extend_from_slice(samples);
        let mut hidden = record.clone();
        hidden.opacity = 0.0;
        let hidden = self.replacing_layer_for_plan(layer, hidden)?;
        hidden.gen_fill_input_at(
            layer,
            &coverage,
            table.native,
            Some(rect(table.crop)),
            2 * samples.len() as u64,
            image_out,
            mask_out,
            limit,
            other,
            control,
        )?;
        Ok(())
    }

    /// The 16-bit twin of [`DrDocument::gen_fill_add_variations`].
    #[allow(clippy::too_many_arguments)]
    pub fn gen_fill_add_variations(
        &self,
        layer: usize,
        results: &[GenResult<'_>],
        tag: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<PrecisionDocument>, String> {
        self.gen_fill_depth()?;
        let binding = self.live_generated(layer)?;
        let table = &binding.table;
        check_results(results)?;
        check_tag(tag)?;
        self.lock_refusal(layer, EditKind::Pixels)?;
        let (_run, job) = begin(control)?;
        let (w, h) = table.raster;
        let (bpp, profile) = self.variation_terms();
        let plan =
            scratch_bytes(w, h, table.native, bpp, 2, results.len() as u64).saturating_add(profile);
        admit(self.retained_bytes(), plan, other, limit)?;
        job.reserve(plan)?;
        let next = grown(table, encode_results(results, tag, job)?)?;
        self.check_variation_room(next.png_bytes() - table.png_bytes())?;
        let (pixels, size) = results[0];
        let plane = self.variation_plane(&next, pixels, size, job)?;
        let out = self.publish_variation(layer, plane, Arc::new(next), tag);
        job.report(1.0, 5)?;
        out
    }
}

/// The byte reader's parsed entries bound to their decoded layers.
pub(crate) fn attach_byte(
    layers: &mut [Layer],
    entries: Vec<(usize, GeneratedVariations)>,
    profile: &Arc<IccProfile>,
) {
    for (index, table) in entries {
        if let Some(layer) = layers.get_mut(index) {
            layer.generated = Some(GeneratedBinding8 {
                table: Arc::new(table),
                rendered: layer.pixels.clone(),
                rendered_offset: layer.offset,
                profile: profile.clone(),
            });
        }
    }
}

/// The typed reader's parsed entries bound to their decoded records
/// (bottom-first record indices, groups after their children).
pub(crate) fn attach_typed(
    layers: &mut [PrecisionDocumentLayer],
    mut entries: Vec<(usize, GeneratedVariations)>,
    profile: &Arc<IccProfile>,
) {
    fn walk(
        layers: &mut [PrecisionDocumentLayer],
        entries: &mut Vec<(usize, GeneratedVariations)>,
        profile: &Arc<IccProfile>,
        index: &mut usize,
    ) {
        for layer in layers {
            if let Content::Group(children) | Content::PassThrough(children) = &mut layer.content {
                walk(
                    Arc::make_mut(children).as_mut_slice(),
                    entries,
                    profile,
                    index,
                );
            }
            if let Some(at) = entries.iter().position(|(i, _)| i == index) {
                let (_, table) = entries.swap_remove(at);
                let rendered = match &layer.content {
                    Content::Raster { image, .. } => Some(GeneratedPlane16::Rgb(image.clone())),
                    Content::CmykRaster { image, .. } => {
                        Some(GeneratedPlane16::Cmyk(image.clone()))
                    }
                    _ => None,
                };
                if let Some(rendered) = rendered {
                    layer.generated = Some(GeneratedBinding16 {
                        table: Arc::new(table),
                        rendered,
                        rendered_offset: layer.offset(),
                        profile: profile.clone(),
                    });
                }
            }
            *index += 1;
        }
    }
    walk(layers, &mut entries, profile, &mut 0);
}

impl DrDocument {
    /// The live entries the `.rz` writer stores (`generated_native`).
    pub(crate) fn generated_entries(&self) -> Vec<crate::generated_native::Entry> {
        self.layers
            .iter()
            .enumerate()
            .filter_map(|(i, l)| Some((i, l.effective_generated(&self.profile)?.table)))
            .collect()
    }
}

impl PrecisionDocument {
    /// The live entries the typed `.rz` writer stores, by record index.
    pub(crate) fn generated_entries(&self) -> Vec<crate::generated_native::Entry> {
        (0..self.records)
            .filter_map(|i| {
                let binding = self
                    .layer(i)?
                    .effective_generated(self.profile(), self.is_cmyk())?;
                Some((i, binding.table))
            })
            .collect()
    }
}
