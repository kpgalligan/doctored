//! One-channel encoders for a Grayscale document's kept-gray export (#547
//! phase 1b, `gray_export`): PNG (L8, LA8, L16, LA16), TIFF (one BlackIsZero
//! sample, or two with straight alpha, at 8 or 16 bits; `tiff_encode`) and
//! baseline JPEG (L8). Free functions over interleaved samples; the caller
//! has already reduced the composite to its gray (and alpha) samples.
//!
//! The sidecar is written exactly as `doc_export::encode_bytes` writes an
//! RGB deliverable's: the ICC and EXIF through the `image` encoder's own
//! setters, the JFIF density on a JPEG (`dr_image::clamp_jfif_density`), and
//! the remaining packets (XMP, pHYs, IPTC) spliced by `MetadataInjector`
//! from `EncodeSidecar::png_chunks` / `jpeg_segments`.

use std::io::Cursor;

use image::codecs::jpeg::{JpegEncoder, PixelDensity, PixelDensityUnit};
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};

use crate::dr_image::{clamp_jfif_density, Format};
use crate::metadata_write::{EncodeSidecar, MetadataInjector};

fn err(e: image::ImageError) -> String {
    format!("encoding failed: {e}")
}

fn unsupported(e: image::error::UnsupportedError) -> String {
    format!("encoding failed: {e}")
}

/// The injector `doc_export::encode_bytes` selects for `format`.
pub(crate) fn injector(
    format: Format,
    side: &EncodeSidecar<'_>,
) -> MetadataInjector<Cursor<Vec<u8>>> {
    let writer = Cursor::new(Vec::new());
    let extra = match format {
        Format::Jpeg => side.jpeg_segments(),
        Format::Png => side.png_chunks(),
        _ => Vec::new(),
    };
    match format {
        _ if extra.is_empty() => MetadataInjector::passthrough(writer),
        Format::Jpeg => MetadataInjector::jpeg(writer, extra),
        Format::Png => MetadataInjector::png(writer, extra),
        _ => MetadataInjector::passthrough(writer),
    }
}

/// A PNG of `bytes` (native-endian samples) as `color`.
fn png(
    bytes: &[u8],
    color: ExtendedColorType,
    dims: (u32, u32),
    side: &EncodeSidecar<'_>,
) -> Result<Vec<u8>, String> {
    let mut sink = injector(Format::Png, side);
    let mut enc = PngEncoder::new(&mut sink);
    if let Some(icc) = side.icc {
        enc.set_icc_profile(icc.to_vec()).map_err(unsupported)?;
    }
    if let Some(exif) = &side.exif {
        enc.set_exif_metadata(exif.clone()).map_err(unsupported)?;
    }
    enc.write_image(bytes, dims.0, dims.1, color).map_err(err)?;
    Ok(sink.finish()?.into_inner())
}

/// A TIFF through `write` (one of `tiff_encode`'s gray writers).
fn tiff(
    side: &EncodeSidecar<'_>,
    write: impl FnOnce(&mut MetadataInjector<Cursor<Vec<u8>>>) -> Result<(), String>,
) -> Result<Vec<u8>, String> {
    let mut sink = injector(Format::Tiff, side);
    write(&mut sink)?;
    Ok(sink.finish()?.into_inner())
}

/// `samples`, interleaved 8-bit gray (and straight alpha when `alpha`), as
/// a PNG (L8 / LA8), a TIFF or a JPEG (L8 at `quality`; `alpha` must be
/// false — the caller flattens first). Any other format answers `Err`.
pub(crate) fn encode_gray8(
    samples: &[u8],
    alpha: bool,
    dims: (u32, u32),
    format: Format,
    quality: u8,
    side: &EncodeSidecar<'_>,
) -> Result<Vec<u8>, String> {
    match format {
        Format::Png => {
            let color = if alpha {
                ExtendedColorType::La8
            } else {
                ExtendedColorType::L8
            };
            png(samples, color, dims, side)
        }
        Format::Tiff => tiff(side, |sink| {
            crate::tiff_encode::encode_gray8(samples, alpha, dims, sink, side)
        }),
        Format::Jpeg if !alpha => {
            let mut sink = injector(Format::Jpeg, side);
            let mut enc = JpegEncoder::new_with_quality(&mut sink, quality.clamp(1, 100));
            if let Some(icc) = side.icc {
                enc.set_icc_profile(icc.to_vec()).map_err(unsupported)?;
            }
            if let Some(exif) = &side.exif {
                enc.set_exif_metadata(exif.clone()).map_err(unsupported)?;
            }
            if let Some(dpi) = side.dpi {
                enc.set_pixel_density(PixelDensity {
                    density: (clamp_jfif_density(dpi.x), clamp_jfif_density(dpi.y)),
                    unit: PixelDensityUnit::Inches,
                });
            }
            enc.write_image(samples, dims.0, dims.1, ExtendedColorType::L8)
                .map_err(err)?;
            Ok(sink.finish()?.into_inner())
        }
        Format::Jpeg => Err("a gray JPEG has no alpha; flatten it first".into()),
        _ => Err("a one-channel export is PNG, TIFF or JPEG".into()),
    }
}

/// `samples`, interleaved 16-bit gray (and straight alpha when `alpha`), as
/// a PNG (L16 / LA16) or a TIFF. A JPEG, and any other format, answers
/// `Err`: JPEG holds 8 bits.
pub(crate) fn encode_gray16(
    samples: &[u16],
    alpha: bool,
    dims: (u32, u32),
    format: Format,
    side: &EncodeSidecar<'_>,
) -> Result<Vec<u8>, String> {
    match format {
        Format::Png => {
            let mut bytes = crate::precision_image::reserved_vec(samples.len() * 2)?;
            for value in samples {
                bytes.extend_from_slice(&value.to_ne_bytes());
            }
            let color = if alpha {
                ExtendedColorType::La16
            } else {
                ExtendedColorType::L16
            };
            png(&bytes, color, dims, side)
        }
        Format::Tiff => tiff(side, |sink| {
            crate::tiff_encode::encode_gray16(samples, alpha, dims, sink, side)
        }),
        Format::Jpeg => Err("a gray JPEG is written at 8 bits".into()),
        _ => Err("a one-channel export is PNG, TIFF or JPEG".into()),
    }
}
