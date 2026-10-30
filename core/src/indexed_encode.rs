//! The encoders of an Indexed Color document's kept-Indexed export (#547
//! Indexed phase 1b, `indexed_export`): the palette, the transparent index
//! and the index plane written as they are, with no render and no
//! quantization. Free functions over one byte a pixel.
//!
//! * **PNG** — colour type 3 at the least bit depth of 1, 2, 4 or 8 that
//!   holds the palette, rows packed most significant bits first. `PLTE` is
//!   exactly the palette; with a transparent index t, `tRNS` holds t + 1
//!   alphas, 255 but 0 at t. The ICC (`iCCP`) and EXIF (`eXIf`) go through
//!   the `png` encoder's own chunks; XMP, `pHYs` and IPTC are spliced by the
//!   same injector a Gray PNG uses (`gray_encode::injector`).
//! * **GIF** — GIF89a with the palette as the global colour table, padded
//!   with black to a power of two of at least 2 entries, and one frame of the
//!   whole plane whose Graphic Control Extension carries t. A GIF carries no
//!   profile and no metadata.
//! * **TIFF** — 8-bit RGB Palette (Photometric 3) with a ColorMap of 3 × 256
//!   SHORTs, entry c as c·257, padded with black to 256 entries
//!   (`tiff_encode::encode_palette8`). With t, a second sample tagged
//!   straight alpha is 0 at t and 255 elsewhere: TIFF records transparency
//!   per pixel, so a t no pixel uses is not recorded.

use std::borrow::Cow;
use std::io::Write;

use crate::dr_image::Format;
use crate::indexed_palette::Rgb;
use crate::metadata_write::EncodeSidecar;

/// The least PNG bit depth whose indices reach every one of `entries`.
fn png_depth(entries: usize) -> u8 {
    match entries {
        0..=2 => 1,
        3..=4 => 2,
        5..=16 => 4,
        _ => 8,
    }
}

/// The plane's rows packed at `depth` bits, most significant bits first.
fn packed_rows(indices: &[u8], (w, h): (u32, u32), depth: u8) -> Result<Vec<u8>, String> {
    let (w, h, depth) = (w as usize, h as usize, usize::from(depth));
    if depth == 8 {
        return Ok(indices.to_vec());
    }
    let line = (w * depth).div_ceil(8);
    let mut out = crate::precision_image::reserved_vec(line * h)?;
    out.resize(line * h, 0u8);
    for (y, row) in indices.chunks_exact(w).enumerate().take(h) {
        for (x, &index) in row.iter().enumerate() {
            let bit = x * depth;
            out[y * line + bit / 8] |= index << (8 - depth - bit % 8);
        }
    }
    Ok(out)
}

fn check_plane(palette: &[Rgb], indices: &[u8], (w, h): (u32, u32)) -> Result<(), String> {
    if indices.len() as u64 != u64::from(w) * u64::from(h) {
        return Err("encoding failed: the index plane does not match the dimensions".into());
    }
    if !(2..=256).contains(&palette.len()) {
        return Err("encoding failed: an Indexed Color palette holds 2 to 256 colours".into());
    }
    Ok(())
}

/// A palette PNG (module doc).
pub(crate) fn encode_png(
    palette: &[Rgb],
    transparent: Option<u8>,
    indices: &[u8],
    dims: (u32, u32),
    side: &EncodeSidecar<'_>,
) -> Result<Vec<u8>, String> {
    check_plane(palette, indices, dims)?;
    let depth = png_depth(palette.len());
    let mut info = png::Info::with_size(dims.0, dims.1);
    info.color_type = png::ColorType::Indexed;
    info.bit_depth = match depth {
        1 => png::BitDepth::One,
        2 => png::BitDepth::Two,
        4 => png::BitDepth::Four,
        _ => png::BitDepth::Eight,
    };
    info.palette = Some(Cow::Owned(palette.concat()));
    info.trns = transparent.map(|t| {
        let mut alphas = vec![255u8; usize::from(t) + 1];
        alphas[usize::from(t)] = 0;
        Cow::Owned(alphas)
    });
    info.icc_profile = side.icc.map(|icc| Cow::Owned(icc.to_vec()));
    info.exif_metadata = side.exif.clone().map(Cow::Owned);
    let data = packed_rows(indices, dims, depth)?;
    let mut sink = crate::gray_encode::injector(Format::Png, side);
    let fail = |e: png::EncodingError| format!("encoding failed: {e}");
    {
        let encoder = png::Encoder::with_info(&mut sink, info).map_err(fail)?;
        let mut writer = encoder.write_header().map_err(fail)?;
        writer.write_image_data(&data).map_err(fail)?;
        writer.finish().map_err(fail)?;
    }
    Ok(sink.finish()?.into_inner())
}

/// A palette GIF (module doc).
pub(crate) fn encode_gif(
    palette: &[Rgb],
    transparent: Option<u8>,
    indices: &[u8],
    dims: (u32, u32),
) -> Result<Vec<u8>, String> {
    check_plane(palette, indices, dims)?;
    let (Ok(w), Ok(h)) = (u16::try_from(dims.0), u16::try_from(dims.1)) else {
        return Err("encoding failed: a GIF is at most 65535 pixels a side".into());
    };
    let mut table = palette.concat();
    table.resize(palette.len().next_power_of_two().max(2) * 3, 0);
    let fail = |e: gif::EncodingError| format!("encoding failed: {e}");
    let mut encoder = gif::Encoder::new(Vec::new(), w, h, &table).map_err(fail)?;
    let frame = gif::Frame {
        width: w,
        height: h,
        transparent,
        buffer: Cow::Borrowed(indices),
        ..Default::default()
    };
    encoder.write_frame(&frame).map_err(fail)?;
    let mut out = encoder.into_inner().map_err(fail)?;
    out.flush().map_err(|e| format!("encoding failed: {e}"))?;
    Ok(out)
}

/// A palette TIFF (module doc).
pub(crate) fn encode_tiff(
    palette: &[Rgb],
    transparent: Option<u8>,
    indices: &[u8],
    dims: (u32, u32),
    side: &EncodeSidecar<'_>,
) -> Result<Vec<u8>, String> {
    check_plane(palette, indices, dims)?;
    let mut map = vec![0u16; 768];
    for (i, rgb) in palette.iter().enumerate() {
        for (c, &v) in rgb.iter().enumerate() {
            map[c * 256 + i] = u16::from(v) * 257;
        }
    }
    let samples: Cow<'_, [u8]> = match transparent {
        None => Cow::Borrowed(indices),
        Some(t) => {
            let mut both = crate::precision_image::reserved_vec(indices.len() * 2)?;
            for &index in indices {
                both.extend_from_slice(&[index, if index == t { 0 } else { 255 }]);
            }
            Cow::Owned(both)
        }
    };
    let mut sink = crate::gray_encode::injector(Format::Tiff, side);
    crate::tiff_encode::encode_palette8(
        &samples,
        transparent.is_some(),
        &map,
        dims,
        &mut sink,
        side,
    )?;
    Ok(sink.finish()?.into_inner())
}
