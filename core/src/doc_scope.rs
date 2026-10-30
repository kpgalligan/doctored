//! Selection-scoped writeback for destructive filters and adjustments. The
//! caller filters the FULL target storage, so a neighborhood filter can read
//! outside the selection; only this final write is confined by coverage.
//! Automatic tonal analysis likewise reads the whole source, independently
//! of the region that receives its result. No native state is added.
//!
//! A missing selection edits all storage, including off-canvas pixels. A
//! selection is canvas-sized and maps through a layer's offset (also for a
//! group's mask); storage outside the canvas has zero selected coverage.
//! RGBA transitions use encoded-color premultiplied interpolation through
//! `sampling`, the same alpha convention as resize and blur. Scalar targets
//! interpolate their stored byte. Zero coverage and identical results retain
//! every original byte, including hidden RGB beneath zero alpha.
//!
//! raw mask reads preserve translated group storage. RGBA fades use the shared premultiplied
//! sampling math; scalar planes retain byte coverage.

use std::sync::Arc;

use image::{GrayImage, RgbaImage};

use crate::doc::DrDocument;
use crate::doc_lock::EditKind;
use crate::doc_plane::{gray_image, image_plane, write_plane_byte, Plane};
use crate::sampling;

/// The offset mapping `doc_plane::LayerClip` uses, expressed as a coverage
/// lookup because an unselected edit must also visit off-canvas storage.
struct Coverage<'a> {
    selection: Option<&'a [u8]>,
    width: u32,
    height: u32,
    offset: (i32, i32),
}

impl<'a> Coverage<'a> {
    fn new(doc: &DrDocument, selection: Option<&'a [u8]>, offset: (i32, i32)) -> Option<Self> {
        if selection.is_some_and(|s| s.len() != doc.width as usize * doc.height as usize) {
            return None;
        }
        Some(Self {
            selection,
            width: doc.width,
            height: doc.height,
            offset,
        })
    }

    fn at(&self, x: u32, y: u32) -> u8 {
        let Some(selection) = self.selection else {
            return 255;
        };
        let cx = i64::from(x) + i64::from(self.offset.0);
        let cy = i64::from(y) + i64::from(self.offset.1);
        if cx < 0 || cy < 0 || cx >= i64::from(self.width) || cy >= i64::from(self.height) {
            return 0;
        }
        selection[cy as usize * self.width as usize + cx as usize]
    }
}

/// `doc_plane::channel_lerp`'s linear coverage rule for a scalar result that
/// already IS straight gray, rather than a premultiplied painting overlay.
/// Integer arithmetic rounds once and makes coverage 0/255 exact.
pub(crate) fn scalar_result(before: u8, result: u8, coverage: u8) -> u8 {
    let weight = u32::from(coverage);
    ((u32::from(before) * (255 - weight) + u32::from(result) * weight + 127) / 255) as u8
}

fn rgba_result(before: [u8; 4], result: [u8; 4], coverage: u8) -> [u8; 4] {
    if coverage == 0 || before == result {
        return before;
    }
    if coverage == 255 {
        return result;
    }
    let weight = f32::from(coverage) / 255.0;
    let mut acc = [0.0; 4];
    sampling::accumulate(&mut acc, &before, 1.0 - weight);
    sampling::accumulate(&mut acc, &result, weight);
    sampling::straight(acc)
}

impl DrDocument {
    /// Full stored mask for a destructive operation, with no canvas clipping
    /// or group-offset baking (unlike the mask thumbnail reader). `None`
    /// without a mask or when its storage dimensions violate the model.
    pub fn layer_mask_storage_image(&self, idx: usize) -> Option<RgbaImage> {
        let layer = self.layers.get(idx)?;
        let mask = layer.mask.as_deref()?;
        let size = mask.dimensions();
        gray_image(mask.as_raw(), size.0, size.1)
    }

    /// Blends a same-sized full-layer filter result through optional canvas
    /// selection coverage. `None` for a group, invalid dimensions/selection,
    /// an enforced pixel lock, or when no bytes change. Properties, masks,
    /// offsets and metadata are retained; source-description invalidation is
    /// the host's responsibility in the same edit as this writeback.
    pub fn apply_layer_pixels(
        &self,
        idx: usize,
        result: &RgbaImage,
        selection: Option<&[u8]>,
    ) -> Option<Self> {
        self.under_locks(idx, EditKind::Pixels, |doc| {
            let layer = doc.raster_layer(idx)?;
            if result.dimensions() != layer.pixels.dimensions() {
                return None;
            }
            let coverage = Coverage::new(doc, selection, layer.offset)?;
            let mut pixels = layer.pixels.to_contiguous().ok()?;
            let mut changed = false;
            for (x, y, pixel) in pixels.enumerate_pixels_mut() {
                let after = rgba_result(pixel.0, result.get_pixel(x, y).0, coverage.at(x, y));
                changed |= pixel.0 != after;
                pixel.0 = after;
            }
            if !changed {
                return None;
            }
            let mut out = doc.clone();
            out.layers[idx].pixels = out.layers[idx].pixels.rebuilt(pixels);
            Some(out)
        })
    }

    /// Writes the Rec. 709 gray of a full-storage result into an existing
    /// layer/group mask. A group mask has canvas dimensions and still maps
    /// through the group's offset. Keeps its enabled flag. `None` on missing
    /// mask, bad dimensions/selection, Lock All, or unchanged bytes.
    pub fn apply_layer_mask(
        &self,
        idx: usize,
        result: &RgbaImage,
        selection: Option<&[u8]>,
    ) -> Option<Self> {
        self.under_locks(idx, EditKind::Mask, |doc| {
            let layer = doc.layers.get(idx)?;
            let before = layer.mask.as_deref()?;
            let size = before.dimensions();
            if result.dimensions() != size {
                return None;
            }
            let coverage = Coverage::new(doc, selection, layer.mask_properties.offset)?;
            let mask = scoped_gray(before, result, &coverage)?;
            let mut out = doc.clone();
            out.layers[idx].mask = Some(Arc::new(mask));
            Some(out)
        })
    }

    /// Writes the result's gray into one RGBA plane at full layer size.
    /// `None` for groups, derived Luma/Mask planes, bad sizes/selection, pixel
    /// locks, or no change. The existing plane-writer rule clears RGB when
    /// selected alpha changes to zero; zero coverage never touches a pixel.
    pub fn apply_layer_plane(
        &self,
        idx: usize,
        plane: Plane,
        result: &RgbaImage,
        selection: Option<&[u8]>,
    ) -> Option<Self> {
        self.under_locks(idx, EditKind::Pixels, |doc| {
            let byte = plane.byte()?;
            let layer = doc.raster_layer(idx)?;
            if result.dimensions() != layer.pixels.dimensions() {
                return None;
            }
            let values = image_plane(result, Plane::Luma)?;
            let coverage = Coverage::new(doc, selection, layer.offset)?;
            let mut pixels = layer.pixels.to_contiguous().ok()?;
            let mut changed = false;
            for ((x, y, pixel), value) in pixels.enumerate_pixels_mut().zip(values) {
                let after = scalar_result(pixel[byte], value, coverage.at(x, y));
                if after != pixel[byte] {
                    changed |= write_plane_byte(&mut pixel.0, byte, after);
                }
            }
            if !changed {
                return None;
            }
            let mut out = doc.clone();
            out.layers[idx].pixels = out.layers[idx].pixels.rebuilt(pixels);
            Some(out)
        })
    }

    /// Writes the result's gray into a saved channel through optional canvas
    /// coverage. `None` for invalid channel/dimensions/selection or unchanged
    /// bytes. Channel identity and overlay properties remain intact.
    pub fn apply_channel(
        &self,
        idx: usize,
        result: &RgbaImage,
        selection: Option<&[u8]>,
    ) -> Option<Self> {
        let before = self.channels.get(idx)?.data.as_ref();
        if before.dimensions() != (self.width, self.height)
            || result.dimensions() != before.dimensions()
        {
            return None;
        }
        let coverage = Coverage::new(self, selection, (0, 0))?;
        let data = scoped_gray(before, result, &coverage)?;
        let mut out = self.clone();
        out.channels[idx].data = Arc::new(data);
        Some(out)
    }
}

/// Shared scalar writeback for layer masks and saved channels. The result
/// reader is `doc_plane::image_plane`, preserving its opaque-gray identity.
fn scoped_gray(
    before: &GrayImage,
    result: &RgbaImage,
    coverage: &Coverage<'_>,
) -> Option<GrayImage> {
    let values = image_plane(result, Plane::Luma)?;
    let mut out = before.clone();
    let mut changed = false;
    for ((x, y, value), result) in out.enumerate_pixels_mut().zip(values) {
        let after = scalar_result(value[0], result, coverage.at(x, y));
        changed |= value[0] != after;
        value[0] = after;
    }
    changed.then_some(out)
}
