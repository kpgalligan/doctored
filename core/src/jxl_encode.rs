//! Lossless JPEG XL encoding of an 8-bit RGBA image, for the export
//! workspace's `jxl` format. A thin wrapper over `zune-jpegxl`, the crate's
//! only JPEG XL code: nothing here decodes one, which is why a JPEG XL export
//! is previewed by the host (`ExportResult::decode_preview`).
//!
//! The encoder is `zune-jpegxl`'s "simple" one — a fast lossless modular
//! encode, so the file decodes to exactly the samples given. It writes a bare
//! codestream with no container, hence no ICC profile, EXIF or XMP: a bare
//! codestream's colour is sRGB by the format's own default, so the export
//! refuses pixels in any other space rather than write a file that would be
//! read as sRGB (`doc_export`), and `format_carries(Jxl)` is 0.
//!
//! An image whose every alpha is 255 is written as three channels, so an
//! opaque photograph does not carry a constant alpha plane; any other image
//! keeps its alpha as a fourth, straight (unassociated) channel. The encoder
//! itself refuses a side under 2 pixels; this module refuses it first, by
//! name.

use image::RgbaImage;
use zune_jpeg::zune_core::bit_depth::BitDepth;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::EncoderOptions;
use zune_jpegxl::JxlSimpleEncoder;

/// The refusal for an image the encoder cannot hold.
pub(crate) const TOO_SMALL: &str = "a JPEG XL export needs at least 2 pixels on each side";

/// `pixels` as a lossless JPEG XL codestream: RGB when every alpha is 255,
/// else RGBA with straight alpha. `Err` for a side under 2 pixels, or when
/// the encoder itself fails.
pub(crate) fn encode_rgba8(pixels: &RgbaImage) -> Result<Vec<u8>, String> {
    let (width, height) = pixels.dimensions();
    if width < 2 || height < 2 {
        return Err(TOO_SMALL.into());
    }
    let raw = pixels.as_raw();
    let opaque = raw.chunks_exact(4).all(|pixel| pixel[3] == 255);
    let rgb: Vec<u8>;
    let (data, colorspace): (&[u8], ColorSpace) = if opaque {
        rgb = raw
            .chunks_exact(4)
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect();
        (&rgb, ColorSpace::RGB)
    } else {
        (raw, ColorSpace::RGBA)
    };
    let options = EncoderOptions::new(width as usize, height as usize, colorspace, BitDepth::Eight);
    let mut out = Vec::new();
    JxlSimpleEncoder::new(data, options)
        .encode(&mut out)
        .map_err(|e| format!("JPEG XL encoding failed: {}", format!("{e:?}").trim_end()))?;
    Ok(out)
}
