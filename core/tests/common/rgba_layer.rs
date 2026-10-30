//! The test-side oracle that replaced the byte RGBA layer-pixels export:
//! layer `idx`'s pixels replaced by a straight RGBA8 buffer through
//! `DrDocument::with_layer_pixels`. A suite root mounts it once and its files
//! `use crate::rgba_layer;`; a standalone binary mounts it with
//! `#[path = "common/rgba_layer.rs"]`.
#![allow(dead_code)]

use doctored_core::doc::DrDocument;
use image::RgbaImage;

/// `doc` with layer `idx`'s pixels replaced by `rgba` (`w * h * 4` bytes,
/// row 0 top): a new owned handle (free with `dr_doc_free`), or NULL where
/// `with_layer_pixels` answers `None`. `doc` must be live and non-NULL.
pub fn with_layer_rgba(
    doc: *const DrDocument,
    idx: usize,
    rgba: &[u8],
    w: u32,
    h: u32,
) -> *mut DrDocument {
    let pixels = RgbaImage::from_raw(w, h, rgba.to_vec()).expect("w*h*4 bytes");
    match unsafe { &*doc }.with_layer_pixels(idx, pixels) {
        Some(out) => Box::into_raw(Box::new(out)),
        None => std::ptr::null_mut(),
    }
}
