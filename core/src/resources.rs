//! Retained allocation accounting for a host-supplied set of live immutable
//! documents and image handles. Undo snapshots, worker inputs and projections
//! belong in the SAME call: summing reports double-counts shared `Arc` buffers.
//!
//! Numbers measure heap payload capacity, not allocator size classes or process
//! RSS. Pointer identity is used only while the supplied handles and cache plane
//! snapshots remain alive, and is never exposed in JSON. Empty allocations are
//! ignored. Shared pixels/masks/channels/profile/metadata and rendered planes
//! contribute once to unique bytes, however many snapshots reference them.
//!
//! Immutable handles are Send + Sync. Style/LUT/content-box caches already use
//! mutexes; the compositor's `Cell` budget is local to one render, never shared.
//! Cache eviction may overlap a render: readers own `Arc` planes and remain
//! valid, so bytes removed from a cache are not a promise of immediate RSS drop.
//! A concurrent renderer can populate it again. No document state is changed.
//!
//! Shared snapshots do not multiply measured retained bytes; serialized plane counts still govern
//! native capacity. Immutable document/image handles are Send + Sync, but freeing a handle while
//! another caller uses it remains invalid.

use std::collections::{BTreeMap, HashSet};
use std::mem::size_of;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::doc::DrDocument;
use crate::style::{Effect, LayerStyle};
use crate::DrImage;

#[derive(Default)]
struct Allocation {
    bytes: u64,
    references: u64,
    category: &'static str,
    cache: bool,
}

/// Collected heap payloads. The allocation address is internal to one report.
#[derive(Default)]
pub(crate) struct Inventory {
    allocations: BTreeMap<usize, Allocation>,
    pins: Vec<Arc<dyn std::any::Any + Send + Sync>>,
}

impl Inventory {
    /// Cache snapshots may be evicted while another owner is being walked.
    /// Retain their allocation owners until the complete union is discarded.
    pub(crate) fn pin<T: std::any::Any + Send + Sync>(&mut self, value: Arc<T>) {
        self.pins.push(value);
    }
    /// Unique retained payload capacity across the still-live supplied handles.
    pub(crate) fn unique_bytes(&self) -> u64 {
        self.allocations.values().fold(0_u64, |total, allocation| {
            total.saturating_add(allocation.bytes)
        })
    }

    /// Whether an allocation at `ptr` has been recorded.
    pub(crate) fn has(&self, ptr: usize) -> bool {
        self.allocations.contains_key(&ptr)
    }

    pub(crate) fn record(&mut self, ptr: usize, bytes: usize, category: &'static str, cache: bool) {
        if bytes == 0 {
            return;
        }
        let allocation = self.allocations.entry(ptr).or_insert(Allocation {
            bytes: bytes as u64,
            references: 0,
            category,
            cache,
        });
        // A native fallback may share its pixels with a disposable source
        // cache. Any authoritative owner makes that allocation retained,
        // regardless of the order in which the live wrappers are walked.
        if allocation.cache && !cache {
            allocation.cache = false;
            allocation.category = category;
        }
        allocation.references += 1;
    }

    pub(crate) fn vec<T>(&mut self, value: &Vec<T>, category: &'static str, cache: bool) {
        self.record(
            value.as_ptr() as usize,
            value.capacity() * size_of::<T>(),
            category,
            cache,
        );
    }

    pub(crate) fn string(&mut self, value: &String, category: &'static str, cache: bool) {
        self.record(value.as_ptr() as usize, value.capacity(), category, cache);
    }

    pub(crate) fn object<T>(&mut self, value: &T, category: &'static str, cache: bool) {
        self.record(value as *const T as usize, size_of::<T>(), category, cache);
    }

    pub(crate) fn effects(&mut self, effects: &[Effect], cache: bool) {
        for effect in effects {
            // Every glow holds a stops list too (`style_glow_gradient`):
            // a Color glow's default two stops are a heap allocation all the same.
            let gradient = match effect {
                Effect::Stroke(value) => Some(&value.gradient),
                Effect::GradientOverlay(value) => Some(&value.gradient),
                Effect::OuterGlow(value) => Some(&value.gradient),
                Effect::InnerGlow(value) => Some(&value.gradient),
                _ => None,
            };
            if let Some(gradient) = gradient {
                self.vec(&gradient.stops, "style_parameters", cache);
            }
            // A pattern overlay's tile, every stroke's `pattern_fill` tile and
            // a bevel's texture tile when the bevel carries one (a default
            // texture is not part of what the style stores),
            // shared by every style version that keeps it (one allocation,
            // counted once by address).
            let fill = match effect {
                Effect::PatternOverlay(value) => Some(&value.fill),
                Effect::Stroke(value) => Some(&value.pattern_fill),
                Effect::BevelEmboss(value)
                    if crate::style_pattern_fill::bevel_carries_texture(value) =>
                {
                    Some(&value.texture.fill)
                }
                _ => None,
            };
            if let Some(fill) = fill {
                let rgba = &fill.pattern.tile.rgba;
                self.record(
                    rgba.as_ptr() as usize,
                    rgba.len(),
                    "style_parameters",
                    cache,
                );
            }
        }
    }

    pub(crate) fn document(&mut self, document: &DrDocument) {
        self.object(document, "document_structure", false);
        self.vec(&document.layers, "document_structure", false);
        self.vec(&document.channels, "document_structure", false);
        self.vec(&document.guides, "document_structure", false);
        for layer in &document.layers {
            if let Some(warp) = &layer.warp {
                warp.record_resources(self);
            }
            if let Some(generated) = &layer.generated {
                generated.record_resources(self);
            }
            if let Some(source) = &layer.retained_source {
                source.record_resources(self);
            }
            layer.pixels.record_resources(self, "layer_pixels", false);
            if let Some(mask) = &layer.mask {
                self.object(&**mask, "shared_buffer_structure", false);
                self.vec(mask.as_raw(), "layer_masks", false);
            }
            self.string(&layer.name, "document_strings", false);
            if let Some(meta) = &layer.meta {
                self.string(meta, "layer_metadata", false);
            }
            if let Some(style) = &layer.style {
                self.object(&**style, "style_parameters", false);
                self.vec(&style.effects, "style_parameters", false);
                self.effects(&style.effects, false);
            }
        }
        // A live vector mask's raster part is one more mask plane of its
        // layer (`doc_vector_mask`). One that IS the slot's plane was
        // recorded above and is not a second plane in the file either.
        for (idx, record) in document.live_vector_masks() {
            let Some(raster) = &record.raster else {
                continue;
            };
            let slot = document.layers[idx].mask.as_ref();
            if slot.is_some_and(|slot| Arc::ptr_eq(slot, &raster.plane)) {
                continue;
            }
            self.object(&*raster.plane, "shared_buffer_structure", false);
            self.vec(raster.plane.as_raw(), "layer_masks", false);
        }
        for channel in &document.channels {
            self.object(&*channel.data, "shared_buffer_structure", false);
            self.vec(channel.data.as_raw(), "saved_channels", false);
            self.string(&channel.name, "document_strings", false);
        }
        self.object(&*document.profile, "profile", false);
        document.profile.record_resources(self);
        for packet in [
            &document.metadata.exif,
            &document.metadata.xmp,
            &document.metadata.iptc,
        ]
        .into_iter()
        .flatten()
        {
            self.record(
                packet.as_ptr() as usize,
                packet.len(),
                "capture_metadata",
                false,
            );
        }
    }

    /// Σ bytes × references over the authoritative (non-cache) allocations:
    /// each owner's reference counted at full size, caches excluded
    /// (`drdc_reopen_plan`'s N).
    pub(crate) fn retained_referenced_bytes(&self) -> u64 {
        self.allocations
            .values()
            .filter(|allocation| !allocation.cache)
            .fold(0_u64, |total, allocation| {
                total.saturating_add(allocation.bytes.saturating_mul(allocation.references))
            })
    }

    pub(crate) fn cache_bytes(&self) -> u64 {
        self.allocations
            .values()
            .filter(|allocation| allocation.cache)
            .map(|allocation| allocation.bytes)
            .sum()
    }

    /// Disposable contiguous layer views (`layer_raster`'s view cache) still
    /// counted as cache: a view an authoritative owner also holds is retained.
    fn contiguous_view_bytes(&self) -> u64 {
        self.allocations
            .values()
            .filter(|allocation| allocation.cache && allocation.category == "contiguous_views")
            .map(|allocation| allocation.bytes)
            .sum()
    }

    pub(crate) fn report(&self, transient: u64, limit: u64) -> Value {
        let mut categories = BTreeMap::<&str, Value>::new();
        let mut unique = 0u64;
        let mut referenced = 0u64;
        for allocation in self.allocations.values() {
            unique = unique.saturating_add(allocation.bytes);
            referenced =
                referenced.saturating_add(allocation.bytes.saturating_mul(allocation.references));
            let item = categories.entry(allocation.category).or_insert_with(
                || json!({"unique_bytes":0, "referenced_bytes":0, "allocation_count":0}),
            );
            for (key, added) in [
                ("unique_bytes", allocation.bytes),
                (
                    "referenced_bytes",
                    allocation.bytes.saturating_mul(allocation.references),
                ),
                ("allocation_count", 1),
            ] {
                item[key] = json!(item[key].as_u64().unwrap_or(0).saturating_add(added));
            }
        }
        let estimated_working = unique.saturating_add(transient);
        json!({
            "schema_version":1,
            "measurement":"retained_heap_payload_capacity",
            "unique_retained_bytes":unique,
            "referenced_bytes":referenced,
            "shared_bytes_saved":referenced.saturating_sub(unique),
            "allocation_count":self.allocations.len(),
            "cache_bytes":self.cache_bytes(),
            "contiguous_view_bytes":self.contiguous_view_bytes(),
            "categories":categories,
            "transient_estimate_bytes":transient,
            "estimated_working_bytes":estimated_working,
            "working_limit_bytes":limit,
            "within_working_limit":limit == 0 || estimated_working <= limit,
            "cache_snapshot_consistency":"each cache synchronized; concurrent readers may retain evicted planes and renderers may refill caches",
            "known_unaccounted":[
                "allocator rounding, Arc control blocks, thread stacks and temporary report storage",
                "small cache entry bookkeeping and weak Arc shells",
                "worker scratch not included in the supplied transient estimate",
                "Swift objects, selections and undo closures outside the supplied handles",
                "CoreGraphics/AppKit/ImageIO/RAW/CoreImage/IOSurface/GPU allocations and encoded I/O buffers",
                "in-flight renderer-local buffers and evicted planes still borrowed by a renderer"
            ]
        })
    }
}

fn unique_styles<'a>(documents: &[&'a DrDocument]) -> Vec<&'a LayerStyle> {
    let mut seen = HashSet::new();
    documents
        .iter()
        .flat_map(|document| &document.layers)
        .filter_map(|layer| layer.style.as_ref())
        .filter(|style| seen.insert(Arc::as_ptr(style) as usize))
        .map(Arc::as_ref)
        .collect()
}

pub(crate) fn cache_inventory(documents: &[&DrDocument], inventory: &mut Inventory) {
    // Keep every plane pinned until all caches have been walked. Without these
    // Arcs an eviction could free a plane and reuse its address during a report.
    let snapshots: Vec<_> = unique_styles(documents)
        .into_iter()
        .map(|style| style.cache.resource_snapshot())
        .collect();
    for snapshot in &snapshots {
        snapshot.record_resources(inventory);
    }
    let masks: Vec<_> = documents
        .iter()
        .flat_map(|d| &d.layers)
        .flat_map(|l| l.mask_properties.cached_planes())
        .collect();
    for mask in &masks {
        inventory.object(&**mask, "mask_cache_structure", true);
        inventory.vec(mask.as_raw(), "mask_cache_planes", true);
    }
    crate::adjust_lut::record_cached_resources(inventory);
}

/// Cross-depth retained originals can be shared by byte and typed documents.
/// Inventory them together; adding independent family totals loses that alias.
pub(crate) fn mixed_report_with_masks(
    documents: &[&DrDocument],
    images: &[&DrImage],
    typed_documents: &[&crate::precision_document::PrecisionDocument],
    typed_images: &[&crate::precision_image::PrecisionImage],
    masks: &[&crate::raw_local_mask::RawLocalMask],
    transient: u64,
    limit: u64,
) -> Value {
    let mut inventory = Inventory::default();
    let mut seen_documents = HashSet::new();
    let mut seen_images = HashSet::new();
    for document in documents {
        if seen_documents.insert(*document as *const DrDocument as usize) {
            inventory.document(document);
        }
    }
    for image in images {
        if seen_images.insert(*image as *const DrImage as usize) {
            inventory.object(*image, "image_structure", false);
            inventory.vec(image.pixels.as_raw(), "image_pixels", false);
        }
    }
    let mut seen_typed_documents = HashSet::new();
    let mut seen_typed_images = HashSet::new();
    for document in typed_documents {
        if seen_typed_documents
            .insert(*document as *const crate::precision_document::PrecisionDocument as usize)
        {
            document.record_resources(&mut inventory);
        }
    }
    for image in typed_images {
        if seen_typed_images
            .insert(*image as *const crate::precision_image::PrecisionImage as usize)
        {
            image.record_resources(&mut inventory);
        }
    }
    let mut seen_masks = HashSet::new();
    for mask in masks {
        if seen_masks.insert(*mask as *const crate::raw_local_mask::RawLocalMask as usize) {
            mask.record_resources(&mut inventory);
        }
    }
    cache_inventory(documents, &mut inventory);
    // Views are reported and evicted here only; capacity plans and the
    // eviction inventory go through `cache_inventory` and never count them.
    let views = crate::contiguous_views::contiguous_views_of(&inventory);
    crate::contiguous_views::record_contiguous_views(views, &mut inventory);
    let mut result = inventory.report(transient, limit);
    result["document_handle_count"] = json!(seen_documents.len());
    result["image_handle_count"] = json!(seen_images.len());
    result["precision_document_handle_count"] = json!(seen_typed_documents.len());
    result["precision_image_handle_count"] = json!(seen_typed_images.len());
    result["raw_local_mask_handle_count"] = json!(seen_masks.len());
    result["projection_scratch_estimate_bytes"] = json!(documents
        .iter()
        .map(|document| crate::render_scratch::projection_scratch_bytes(document))
        .max()
        .unwrap_or(0));
    result["projection_scratch_scope"] = json!(
        "maximum single-document full projection estimate among supplied handles; not added to working total unless caller reserves it"
    );
    result
}

/// Removes only disposable caches, never pixels, document state or history.
pub(crate) fn evict_caches(documents: &[&DrDocument]) -> Value {
    // One walk of the supplied documents scopes the views both before and
    // after the eviction.
    let mut walked = Inventory::default();
    for document in documents {
        walked.document(document);
    }
    let mut before = Inventory::default();
    cache_inventory(documents, &mut before);
    let views = crate::contiguous_views::contiguous_views_of(&walked);
    crate::contiguous_views::record_contiguous_views(views, &mut before);
    for style in unique_styles(documents) {
        style.cache.clear();
    }
    for layer in documents.iter().flat_map(|d| &d.layers) {
        layer.mask_properties.clear_cache();
    }
    crate::adjust_lut::evict_cached_resources();
    crate::doc_group_query::evict_content_box_cache();
    crate::precision_content_box::evict_precision_content_box_cache();
    crate::contiguous_views::evict_contiguous_views();
    let mut after = Inventory::default();
    cache_inventory(documents, &mut after);
    let views = crate::contiguous_views::contiguous_views_of(&walked);
    crate::contiguous_views::record_contiguous_views(views, &mut after);
    json!({
        "cache_bytes_before":before.cache_bytes(),
        "cache_bytes_after":after.cache_bytes(),
        "removed_cache_bytes":before.cache_bytes().saturating_sub(after.cache_bytes()),
        "scope":"supplied document mask/style caches, process-wide LUT/content-box memos and every contiguous layer view (removed_cache_bytes counts only the supplied documents' views)",
        "release":"evicted planes remain alive until concurrent readers finish; cache refill is permitted"
    })
}
