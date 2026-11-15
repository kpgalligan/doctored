//! The 8-bit flat PNG (13C-5): a flat save's or export's RGBA8 PNG written
//! row by row by the native writer's encoder (`png_rows8`), so it holds rows
//! beside the flattened pixels rather than `png`'s whole zlib stream, and
//! Cancel is polled while it compresses. The file is byte for byte the one
//! `image`'s `PngEncoder` wrote behind `metadata_write::MetadataInjector`
//! (`tests/flat_png_rows.rs`).
//!
//! **The chunks around the image data.** `PngEncoder` writes the profile
//! (`iCCP`, its zlib stream compressed by `png`) and the EXIF packet
//! (`eXIf`) between IHDR and IDAT, and the injector puts the sidecar's own
//! chunks (`EncodeSidecar::png_chunks`: `pHYs`, the XMP `iTXt`) straight
//! after IHDR. Those chunks do not depend on the image's size, so they are
//! taken from `PngEncoder`'s encode of one pixel with the same profile and
//! packet, and none of `png`'s chunk rules is restated here.

use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, RgbaImage};

use crate::job_control::JobContext;
use crate::metadata_write::EncodeSidecar;
use crate::native_stream::NativeSink;
use crate::png_rows8::{put_png8, Png8};

/// Writes `pixels` as the PNG `side` describes to `out`.
pub(crate) fn encode_png8(
    pixels: &RgbaImage,
    out: &mut dyn NativeSink,
    side: &EncodeSidecar<'_>,
    job: JobContext<'_>,
) -> Result<(), String> {
    let (after_ihdr, before_iend) = chunks(side)?;
    let (width, height) = pixels.dimensions();
    let png = Png8 {
        width,
        height,
        channels: 4,
        after_ihdr: &after_ihdr,
        before_iend: &before_iend,
    };
    let stride = 4 * width as usize;
    let raw = pixels.as_raw();
    let mut rows = |y: u32, row: &mut [u8]| {
        let start = y as usize * stride;
        let line = raw
            .get(start..start + stride)
            .ok_or("encoding failed: the image is shorter than its size")?;
        row.copy_from_slice(line);
        Ok(())
    };
    put_png8(out, &png, &mut rows, job).map(|_| ())
}

/// The bytes written after IHDR and before IEND: the sidecar's chunks, then
/// what `PngEncoder` writes between IHDR and its first IDAT; and what it
/// writes between its last IDAT and IEND (module doc).
fn chunks(side: &EncodeSidecar<'_>) -> Result<(Vec<u8>, Vec<u8>), String> {
    let unsupported = |e: image::error::UnsupportedError| format!("encoding failed: {e}");
    let mut probe = Vec::new();
    let mut encoder = PngEncoder::new(&mut probe);
    if let Some(icc) = side.icc {
        encoder.set_icc_profile(icc.to_vec()).map_err(unsupported)?;
    }
    if let Some(exif) = &side.exif {
        encoder
            .set_exif_metadata(exif.clone())
            .map_err(unsupported)?;
    }
    encoder
        .write_image(&[0; 4], 1, 1, ExtendedColorType::Rgba8)
        .map_err(|e| format!("encoding failed: {e}"))?;
    // Walk the chunks: (start, end, type) after the 8-byte signature.
    let mut spans = Vec::new();
    let mut at = 8;
    while at + 8 <= probe.len() {
        let len = u32::from_be_bytes([probe[at], probe[at + 1], probe[at + 2], probe[at + 3]]);
        let end = at + 12 + len as usize;
        if end > probe.len() {
            break;
        }
        let kind = [probe[at + 4], probe[at + 5], probe[at + 6], probe[at + 7]];
        spans.push((at, end, kind));
        at = end;
    }
    let ihdr_end = spans
        .iter()
        .find(|(_, _, kind)| kind == b"IHDR")
        .map(|&(_, end, _)| end);
    let first_idat = spans
        .iter()
        .find(|(_, _, kind)| kind == b"IDAT")
        .map(|&(start, _, _)| start);
    let last_idat_end = spans
        .iter()
        .rev()
        .find(|(_, _, kind)| kind == b"IDAT")
        .map(|&(_, end, _)| end);
    let iend = spans
        .iter()
        .find(|(_, _, kind)| kind == b"IEND")
        .map(|&(start, _, _)| start);
    let (Some(ihdr_end), Some(first_idat), Some(last_idat_end), Some(iend)) =
        (ihdr_end, first_idat, last_idat_end, iend)
    else {
        return Err("encoding failed: the PNG encoder wrote no IHDR, IDAT or IEND".into());
    };
    let mut after = side.png_chunks().concat();
    after.extend_from_slice(&probe[ihdr_end..first_idat]);
    Ok((after, probe[last_idat_end..iend].to_vec()))
}
