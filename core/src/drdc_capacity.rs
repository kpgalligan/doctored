//! Shared native-format capacity checks for saving, record preflight and
//! document edits. Counts measure serialized planes, even when their pixel
//! storage is Arc-shared: reopening allocates each record independently.
//!
//! Capacity counts serialized planes even when editing shares their allocations; all records are
//! preflighted before any PNG pixels or raw masks are allocated.

use super::*;

pub(super) fn canvas_size(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err("invalid canvas size".to_string());
    }
    Ok(())
}

pub(super) fn layer_count(count: usize) -> Result<u32, String> {
    if count == 0 || count > MAX_DRDC_LAYERS as usize {
        return Err(format!(
            "invalid layer count {count} (max {MAX_DRDC_LAYERS})"
        ));
    }
    Ok(count as u32)
}

pub(super) fn layer_pixels(pixels: u64) -> Result<(), String> {
    if pixels > MAX_DRDC_TOTAL_LAYER_PIXELS {
        return Err(format!(
            "total layer pixels exceed {MAX_DRDC_TOTAL_LAYER_PIXELS}; reduce image size or remove layers, or export a flattened copy"
        ));
    }
    Ok(())
}

pub(super) fn mask_dimensions((w, h): (u32, u32)) -> Result<(), String> {
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err("invalid independent mask dimensions".to_string());
    }
    Ok(())
}

pub(super) fn mask_pixels(pixels: u64) -> Result<(), String> {
    if pixels > MAX_DRDC_TOTAL_MASK_PIXELS {
        return Err(format!("total mask pixels exceed {MAX_DRDC_TOTAL_MASK_PIXELS}; remove masks or reduce their dimensions"));
    }
    Ok(())
}

pub(super) fn channel_count(count: usize, width: u32, height: u32) -> Result<u32, String> {
    if count > MAX_CHANNELS {
        return Err(format!(
            "invalid channel count {count}: too many channels (max {MAX_CHANNELS})"
        ));
    }
    if count as u64 * u64::from(width) * u64::from(height) > MAX_DRDC_TOTAL_CHANNEL_PIXELS {
        return Err(format!("total channel pixels exceed {MAX_DRDC_TOTAL_CHANNEL_PIXELS}; remove saved channels or reduce image size"));
    }
    Ok(count as u32)
}

/// Keep the writer consistent with the image decoder's existing allocation
/// limit. Older files can contain other PNG color types, so the reader checks
/// the decoder's actual output bytes rather than assuming RGBA8 there.
pub(super) fn decoded_bytes(bytes: u64) -> Result<(), String> {
    if image::Limits::default()
        .max_alloc
        .is_some_and(|cap| bytes > cap)
    {
        return Err(
            "layer PNG exceeds the decoder's memory limit; reduce its dimensions".to_string(),
        );
    }
    Ok(())
}

/// WARP retains original planes in addition to every independently decoded
/// serialized fallback plane. The byte writer and reader both use this
/// admission, against the byte format's `MAX_DRDC_DECODED_BYTES`.
pub fn warp_decoded_capacity(
    originals: u64,
    fallback_rgba: u64,
    fallback_masks: u64,
    channels: u64,
    retained_sources: u64,
) -> Result<(), String> {
    let total = [
        originals,
        fallback_rgba,
        fallback_masks,
        channels,
        retained_sources,
    ]
    .into_iter()
    .try_fold(0u64, |sum, bytes| sum.checked_add(bytes))
    .ok_or("WARP decoded capacity overflow")?;
    if total > MAX_DRDC_DECODED_BYTES {
        return Err("WARP originals and fallback exceed native capacity".into());
    }
    Ok(())
}

pub(super) fn slot_length(len: usize, cap: u32, what: &str) -> Result<u32, String> {
    u32::try_from(len)
        .ok()
        .filter(|&len| len <= cap)
        .ok_or_else(|| format!("{what} too large: length {len} out of range (max {cap})"))
}

fn total_pixels(dims: impl IntoIterator<Item = (u32, u32)>) -> u64 {
    dims.into_iter().fold(0u64, |sum, (w, h)| {
        sum.saturating_add(u64::from(w) * u64::from(h))
    })
}

fn layer_dimensions_fit(
    before: &DrDocument,
    dims: &[(u32, u32)],
    before_source_pixels: u64,
    after_source_pixels: u64,
) -> bool {
    let before_pixels = total_pixels(before.layers.iter().map(|l| l.pixels.dimensions()))
        .saturating_add(before_source_pixels);
    let largest_before = before
        .layers
        .iter()
        .map(|l| l.pixels.byte_len())
        .max()
        .unwrap_or(0);
    let decode_cap = image::Limits::default().max_alloc.unwrap_or(u64::MAX);
    dims.len() <= before.layers.len().max(MAX_DRDC_LAYERS as usize)
        && dims.iter().all(|&(w, h)| {
            w > 0 && h > 0 && u64::from(w) * u64::from(h) <= largest_before.max(decode_cap) / 4
        })
        && total_pixels(dims.iter().copied()).saturating_add(after_source_pixels)
            <= before_pixels.max(MAX_DRDC_TOTAL_LAYER_PIXELS)
}

impl DrDocument {
    /// The whole in-memory native contract, checked before PNG encoding or
    /// destination creation. Malformed masks are errors, never silently omitted.
    pub(crate) fn native_save_preflight(&self) -> Result<(), String> {
        canvas_size(self.width, self.height)?;
        layer_count(self.layers.len())?;
        layer_pixels(total_pixels(
            self.layers.iter().map(|l| l.pixels.dimensions()),
        ))?;
        let (warp_encoded, warp_originals) = crate::warp_native::byte_capacity(self)?;
        if warp_encoded != 0 {
            let source_pixels = crate::native_sources::byte_sources(self)?.pixel_count();
            warp_decoded_capacity(
                warp_originals,
                total_pixels(self.layers.iter().map(|l| l.pixels.dimensions())).saturating_mul(4),
                self.layers
                    .iter()
                    .filter_map(|l| l.mask.as_ref())
                    .map(|m| m.as_raw().len() as u64)
                    .sum(),
                u64::from(self.width) * u64::from(self.height) * self.channels.len() as u64,
                source_pixels.saturating_mul(4),
            )?;
        }
        crate::native_sources::byte_sources(self)?.validate_fallbacks(|index| {
            self.layers
                .get(index)
                .map(|layer| (layer.pixels.dimensions(), layer.offset))
        })?;
        crate::native_sources::byte_sources(self)?.preflight(total_pixels(
            self.layers.iter().map(|l| l.pixels.dimensions()),
        ))?;
        if !validate_structure(&self.layers) {
            return Err("malformed layer group structure".to_string());
        }
        if isolated_group_pixels(&self.layers, self.width, self.height)
            > MAX_DRDC_GROUP_CANVAS_PIXELS
        {
            return Err("layer groups would need more memory than this build allows".to_string());
        }
        // A vector mask's raster part is one more mask plane of its layer
        // (`doc_vector_mask`), written in the PATH table.
        let raster_parts = self.vector_mask_raster_dimensions();
        mask_pixels(
            self.layers
                .iter()
                .filter_map(|l| l.mask.as_ref())
                .map(|m| u64::from(m.width()) * u64::from(m.height()))
                .sum::<u64>()
                .saturating_add(total_pixels(raster_parts.iter().copied())),
        )?;
        for dims in raster_parts {
            mask_dimensions(dims)?;
        }
        for layer in &self.layers {
            let (w, h) = layer.pixels.dimensions();
            if w == 0 || h == 0 {
                return Err("invalid layer pixel size".to_string());
            }
            decoded_bytes(u64::from(w) * u64::from(h) * 4)?;
            if let Some(mask) = &layer.mask {
                mask_dimensions(mask.dimensions())?;
                if !layer.mask_properties.valid(mask.dimensions()) {
                    return Err("invalid mask properties or feather extent".to_string());
                }
            }
            if let Some(meta) = &layer.meta {
                slot_length(meta.len(), MAX_DRDC_META_LEN, "layer meta")?;
            }
            if let Some(style) = &layer.style {
                slot_length(style.to_json().len(), MAX_DRDC_META_LEN, "layer style")?;
            }
        }
        channel_count(self.channels.len(), self.width, self.height)?;
        for channel in &self.channels {
            if channel.data.dimensions() != (self.width, self.height) {
                return Err("channel plane dimensions do not match the canvas".to_string());
            }
        }
        for (what, bytes) in [
            ("ICC profile", Some(self.profile.bytes().as_ref())),
            ("EXIF packet", self.metadata.exif.as_deref()),
            ("XMP packet", self.metadata.xmp.as_deref()),
            ("IPTC packet", self.metadata.iptc.as_deref()),
        ] {
            if let Some(bytes) = bytes {
                slot_length(bytes.len(), MAX_DRDC_BLOB_LEN, what)?;
            }
        }
        slot_length(
            self.guides
                .iter()
                .filter(|g| g.position.is_finite())
                .count(),
            MAX_GUIDES as u32,
            "guide count",
        )?;
        Ok(())
    }

    /// A growth check, not a blanket refusal of an imported/legacy oversized
    /// document: same-sized edits and capacity-reducing edits remain available
    /// so users can remove layers, downsize, undo, or export their work.
    pub(crate) fn native_layers_fit_dimensions(
        &self,
        dims: impl IntoIterator<Item = (u32, u32)>,
    ) -> bool {
        let source_pixels = match crate::native_sources::byte_sources(self) {
            Ok(sources) => sources.pixel_count(),
            Err(_) => return false,
        };
        let dims: Vec<_> = dims.into_iter().collect();
        layer_dimensions_fit(self, &dims, source_pixels, source_pixels)
    }

    /// Source edits can change both asset storage and fallback dimensions.
    /// The future view may contain placeholder fallbacks, so use the planned
    /// dimensions while retaining the original document's growth allowance.
    pub(crate) fn native_source_layers_fit_dimensions(
        &self,
        future_view: &Self,
        dims: impl IntoIterator<Item = (u32, u32)>,
    ) -> bool {
        let dims: Vec<_> = dims.into_iter().collect();
        if dims.len() != future_view.layers.len() {
            return false;
        }
        match (
            crate::native_sources::byte_sources(self),
            crate::native_sources::byte_sources(future_view),
        ) {
            (Ok(before), Ok(after)) => {
                after.preflight(0).is_ok()
                    && layer_dimensions_fit(self, &dims, before.pixel_count(), after.pixel_count())
            }
            _ => false,
        }
    }

    pub(crate) fn native_masks_fit_dimensions(
        &self,
        dims: impl IntoIterator<Item = (u32, u32)>,
    ) -> bool {
        // The raster parts of live vector masks (`doc_vector_mask`) are mask
        // planes no caller lists: they count on both sides.
        let parts = total_pixels(self.vector_mask_raster_dimensions());
        let before = total_pixels(
            self.layers
                .iter()
                .filter_map(|l| l.mask.as_ref().map(|m| m.dimensions())),
        )
        .saturating_add(parts);
        let dims: Vec<_> = dims.into_iter().collect();
        dims.iter().all(|d| mask_dimensions(*d).is_ok())
            && total_pixels(dims).saturating_add(parts) <= before.max(MAX_DRDC_TOTAL_MASK_PIXELS)
    }

    /// Final safety net for the shared document-edit FFI path. Expensive
    /// growth paths also call the dimensions check before allocating pixels.
    pub(crate) fn native_edit_fits(&self, next: &Self) -> bool {
        let mask_total = |d: &DrDocument| {
            d.layers
                .iter()
                .filter_map(|l| l.mask.as_ref())
                .fold(0u64, |n, m| {
                    n.saturating_add(u64::from(m.width()) * u64::from(m.height()))
                })
                .saturating_add(total_pixels(d.vector_mask_raster_dimensions()))
        };
        self.native_source_layers_fit_dimensions(
            next,
            next.layers.iter().map(|l| l.pixels.dimensions()),
        ) && mask_total(next) <= mask_total(self).max(MAX_DRDC_TOTAL_MASK_PIXELS)
            && next.layers.iter().all(|l| {
                l.mask
                    .as_ref()
                    .is_none_or(|m| l.mask_properties.valid(m.dimensions()))
            })
            && isolated_group_pixels(&next.layers, next.width, next.height)
                <= isolated_group_pixels(&self.layers, self.width, self.height)
                    .max(MAX_DRDC_GROUP_CANVAS_PIXELS)
            && (next.channels.len() as u64 * u64::from(next.width) * u64::from(next.height))
                <= (self.channels.len() as u64 * u64::from(self.width) * u64::from(self.height))
                    .max(MAX_DRDC_TOTAL_CHANNEL_PIXELS)
    }
}
