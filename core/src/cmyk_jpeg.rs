//! A CMYK or YCCK JPEG's OWN samples, as ink. `image`'s JPEG decoder
//! turns four-component JPEGs into RGB by a naive multiply before the core
//! sees a sample, so the flat open (`open_profile`) reads such a file again
//! here, through the same `zune-jpeg` decoder `image` itself wraps, asking it
//! for the raw planes instead of RGB.
//!
//! # One decoder configuration
//!
//! Every decoder in this module is built by [`options`], which is exactly
//! what `image`'s `JpegDecoder::new` builds (strict mode off, no width or
//! height cap). The header check ([`cmyk_dimensions`]) and the decode
//! ([`ink_samples`]) therefore admit exactly the same files — with zune's
//! default 16384-pixel cap the header check would call a wide CMYK JPEG
//! "not CMYK" while the decode still read it.
//!
//! # Samples to ink
//!
//! The result is CMYK8 INK, 0 = no ink, 255 = full ink — what a CMYK
//! profile's `A2Bx` takes. A JPEG stores its samples three ways:
//!
//! * CMYK under an Adobe APP14 (transform 0): Photoshop's convention, every
//!   channel stored INVERTED (`255 − ink`);
//! * CMYK with no APP14: stored as ink;
//! * YCCK (APP14 transform 2, the only marker that declares it): the
//!   inverted C, M and Y were encoded as if they were R, G and B, so the
//!   JFIF YCbCr → RGB equations give the C, M and Y INK back directly, and
//!   K is stored inverted (`255 − ink`).
//!
//! Adobe APP14 CMYK is stored inverted, APP14-less CMYK as ink, YCCK's first three planes go
//! through the JFIF equations (master: zune's private `color_convert_ycbcr`) to C/M/Y ink with K
//! inverted; `metadata::Scan::adobe_transform` says whether an APP14 is present.

use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

/// THE zune configuration (module doc): `image` 0.25's `JpegDecoder::new`
/// settings, so this module reads every JPEG `image` reads.
fn options() -> DecoderOptions {
    DecoderOptions::default()
        .set_strict_mode(false)
        .set_max_width(usize::MAX)
        .set_max_height(usize::MAX)
}

/// The decoded header of a four-component CMYK or YCCK JPEG, else `None`.
fn cmyk_header(bytes: &[u8]) -> Option<(JpegDecoder<ZCursor<&[u8]>>, ColorSpace)> {
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), options());
    decoder.decode_headers().ok()?;
    let space = decoder.input_colorspace()?;
    let four = decoder.info().is_some_and(|info| info.components == 4);
    (four && matches!(space, ColorSpace::CMYK | ColorSpace::YCCK)).then_some((decoder, space))
}

/// `(width, height)` when `bytes` is a JPEG whose header declares four
/// components read as CMYK or YCCK — header work only, no pixels decoded.
pub(crate) fn cmyk_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let (decoder, _) = cmyk_header(bytes)?;
    let (w, h) = decoder.dimensions()?;
    Some((u32::try_from(w).ok()?, u32::try_from(h).ok()?))
}

/// Decodes a CMYK or YCCK JPEG to `(width, height, ink)`: `w·h·4` CMYK8 ink
/// samples in STORED orientation (module doc). `adobe` says whether the file
/// carries an Adobe APP14 (`metadata::Scan::adobe_transform`), which is what
/// makes stored CMYK inverted. Any decoder error is an `Err` naming `path`.
pub(crate) fn ink_samples(
    bytes: &[u8],
    path: &str,
    adobe: bool,
) -> Result<(u32, u32, Vec<u8>), String> {
    let fail = |what: String| format!("failed to decode {path}: {what}");
    let (mut decoder, space) = cmyk_header(bytes)
        .ok_or_else(|| fail("the JPEG is not four-component CMYK or YCCK".into()))?;
    // Output = input: zune then copies the four raw planes and converts
    // nothing.
    decoder.set_options(options().jpeg_set_out_colorspace(space));
    let mut samples = decoder.decode().map_err(|e| fail(e.to_string()))?;
    let (w, h) = decoder
        .dimensions()
        .ok_or_else(|| fail("the JPEG header was not read".into()))?;
    let count = w * h;
    if samples.len() < count * 4 {
        return Err(fail("the image data is shorter than its dimensions".into()));
    }
    samples.truncate(count * 4);
    for px in samples.chunks_exact_mut(4) {
        match space {
            ColorSpace::YCCK => {
                let [c, m, y] = ycc_to_rgb(px[0], px[1], px[2]);
                let k = 255 - px[3];
                px.copy_from_slice(&[c, m, y, k]);
            }
            _ if adobe => {
                for s in px.iter_mut() {
                    *s = 255 - *s;
                }
            }
            _ => {}
        }
    }
    let w = u32::try_from(w).map_err(|e| fail(e.to_string()))?;
    let h = u32::try_from(h).map_err(|e| fail(e.to_string()))?;
    Ok((w, h, samples))
}

/// The JFIF YCbCr → RGB equations, rounded and clamped to 0…255. The master
/// is zune-jpeg's private `color_convert_ycbcr`, unreachable from here for a
/// YCCK file's first three planes, so the published equations are written
/// out once more.
fn ycc_to_rgb(y: u8, cb: u8, cr: u8) -> [u8; 3] {
    let (y, cb, cr) = (f64::from(y), f64::from(cb) - 128.0, f64::from(cr) - 128.0);
    let code = |v: f64| v.round().clamp(0.0, 255.0) as u8;
    [
        code(y + 1.402 * cr),
        code(y - 0.344_136 * cb - 0.714_136 * cr),
        code(y + 1.772 * cb),
    ]
}
