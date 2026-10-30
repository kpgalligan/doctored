//! Generative Fill's VARIATIONS (M3b phase 2): the pictures a
//! Generative Layer keeps, so the user can switch to another one later. The
//! model, the caps both `.rz` sides enforce and the
//! PNG legs of one picture; no document type (the bindings and the ops are
//! `doc_gen_variations`, the table `generated_native`).
//!
//! A [`GeneratedVariations`] table holds the provider's pictures as PNG
//! bytes, each at its own size and with the generated tag it was asked with,
//! plus the frame they were read through: `rect` the covered rect, `crop` the
//! crop sent, `native` the native size, `raster` the layer's raster size (the
//! canvas at apply). `ink` says the pictures belong to a CMYK document, where
//! they are sRGB; on RGB they are in the document's encoding (phase 1's rule).
//!
//! The caps are sized from what a provider answers: a 4096 × 4096 RGBA8
//! picture of pure noise is about 64 MiB as PNG, so [`MAX_VARIATION_PNG_BYTES`]
//! admits every picture the apply accepts (`gen_fill::RESULT_SIDES`); a
//! provider's real 1–2 MP answer is 1.5–4 MB, so [`MAX_LAYER_VARIATION_BYTES`]
//! keeps dozens of them on one layer, and [`MAX_VARIATIONS`] stops at eight
//! generated sets of three. [`MAX_DOCUMENT_VARIATION_BYTES`] bounds what one
//! file can make a reader copy, beside the 1.6 GB of pixels it already admits.

use std::sync::Arc;

use image::codecs::png::{PngDecoder, PngEncoder};
use image::{ExtendedColorType, ImageDecoder, ImageEncoder};

/// The most variations one layer keeps: eight generated sets of three.
pub const MAX_VARIATIONS: usize = 24;
/// The largest one stored picture (module doc).
pub const MAX_VARIATION_PNG_BYTES: u64 = 80 << 20;
/// The most stored picture bytes one layer keeps (module doc).
pub const MAX_LAYER_VARIATION_BYTES: u64 = 256 << 20;
/// The most stored picture bytes one document keeps (module doc).
pub const MAX_DOCUMENT_VARIATION_BYTES: u64 = 1 << 30;

/// One stored picture: its PNG, its pixel size and the generated tag it was
/// asked with (the layer's tag while it is the selected one).
#[derive(Clone, Debug, PartialEq)]
pub struct Variation {
    pub png: Arc<[u8]>,
    pub size: (u32, u32),
    pub tag: Arc<str>,
}

/// A Generative Layer's pictures and the frame they read through (module
/// doc). `selected < items.len()`, and `items` is never empty.
#[derive(Clone, Debug, PartialEq)]
pub struct GeneratedVariations {
    pub raster: (u32, u32),
    pub rect: [u32; 4],
    pub crop: [u32; 4],
    pub native: (u32, u32),
    pub ink: bool,
    pub selected: usize,
    pub items: Arc<Vec<Variation>>,
}

impl GeneratedVariations {
    /// The stored PNG bytes of every picture.
    pub fn png_bytes(&self) -> u64 {
        self.items.iter().map(|v| v.png.len() as u64).sum()
    }
}

/// The caps one layer's table must meet, from its pictures' PNG lengths:
/// at most [`MAX_VARIATIONS`], none over [`MAX_VARIATION_PNG_BYTES`], all
/// together at most [`MAX_LAYER_VARIATION_BYTES`]. A test seam: the writer,
/// the reader and every op that grows a table call it.
#[doc(hidden)]
pub fn check_caps(png_lengths: &[u64]) -> Result<(), String> {
    if png_lengths.len() > MAX_VARIATIONS {
        return Err(format!(
            "a generative layer keeps at most {MAX_VARIATIONS} variations"
        ));
    }
    if png_lengths.iter().any(|&n| n > MAX_VARIATION_PNG_BYTES) {
        return Err("a generative variation is over 80 MiB".into());
    }
    let total = png_lengths
        .iter()
        .fold(0u64, |sum, &n| sum.saturating_add(n));
    if total > MAX_LAYER_VARIATION_BYTES {
        return Err("a generative layer keeps at most 256 MiB of variations".into());
    }
    Ok(())
}

/// The document cap over every live table's PNG bytes.
pub(crate) fn check_document(total: u64) -> Result<(), String> {
    if total > MAX_DOCUMENT_VARIATION_BYTES {
        return Err("a document keeps at most 1 GiB of generative variations".into());
    }
    Ok(())
}

/// `rgba` (straight RGBA8 of `size`) as a stored picture tagged `tag`.
pub(crate) fn encode(rgba: &[u8], size: (u32, u32), tag: &str) -> Result<Variation, String> {
    if rgba.len() as u64 != 4 * u64::from(size.0) * u64::from(size.1) {
        return Err("invalid generative variation buffer".into());
    }
    let mut png = crate::drdc_save_plan::reserved(crate::drdc_save_plan::byte_png_bound(size, 4)?)?;
    PngEncoder::new(&mut png)
        .write_image(rgba, size.0, size.1, ExtendedColorType::Rgba8)
        .map_err(|e| format!("generative variation PNG encoding failed: {e}"))?;
    if png.len() as u64 > MAX_VARIATION_PNG_BYTES {
        return Err("a generative variation is over 80 MiB".into());
    }
    Ok(Variation {
        png: Arc::from(png),
        size,
        tag: Arc::from(tag),
    })
}

/// A stored picture's pixels, straight RGBA8 of `v.size`. `Err` for a PNG
/// that does not decode to exactly that size.
pub(crate) fn decode(v: &Variation) -> Result<Vec<u8>, String> {
    let decoder = PngDecoder::new(std::io::Cursor::new(&*v.png))
        .map_err(|e| format!("invalid generative variation PNG: {e}"))?;
    if decoder.dimensions() != v.size {
        return Err("a generative variation's PNG does not match its size".into());
    }
    let image = image::DynamicImage::from_decoder(decoder)
        .map_err(|e| format!("invalid generative variation PNG: {e}"))?;
    Ok(image.into_rgba8().into_raw())
}

/// A PNG's IHDR size, read from its first 24 bytes without inflating
/// anything; `None` for bytes that do not start as a PNG.
pub(crate) fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    if png.len() < 24 || png[..8] != crate::metadata::PNG_SIGNATURE || &png[12..16] != b"IHDR" {
        return None;
    }
    let word = |at: usize| u32::from_be_bytes([png[at], png[at + 1], png[at + 2], png[at + 3]]);
    (word(8) == 13).then(|| (word(16), word(20)))
}
