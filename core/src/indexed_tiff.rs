//! Palette TIFFs as Indexed Color documents (#1 Indexed phase 1b): which
//! TIFFs are one ([`palette_tiff`], header work only through the shared
//! `tiff_directory` walk), their decode to indices and a transparent index
//! ([`decode`]), the open's plan ([`open_plan`]) and the export preview's
//! decode ([`preview_rgba8`]). `indexed_io` owns the open itself.
//!
//! # Which files
//!
//! A classic TIFF whose first directory has PhotometricInterpretation 3
//! (RGB Palette) stored as a SHORT; SamplesPerPixel 1 with BitsPerSample
//! 1, 2, 4 or 8, or SamplesPerPixel 2 with [8, 8] and ExtraSamples [2] (an
//! unassociated alpha); PlanarConfiguration 1; SampleFormat absent or all
//! 1; a Compression the `tiff` crate decodes losslessly
//! (`tiff_directory::COMPRESSIONS`); a ColorMap (320) of 3·2^bits SHORTs,
//! entry i `code8` of R[i], G[i] and B[i]; and an ICC (34675, at most
//! 16 MiB) that is an RGB profile (`indexed_document::is_rgb_profile`), or
//! none (sRGB, with the typed open's untagged sentence). Every other file
//! answers `None` and keeps today's open.
//!
//! # The decode
//!
//! The `tiff` crate refuses Photometric 3, so the file is decoded through
//! the shared `Overlay` with Photometric patched to 1 (BlackIsZero): the
//! raw samples are the indices, 1/2/4-bit rows unpacked as a PNG's are
//! (`indexed_io::unpack_rows`). With alpha, every alpha is 0 or 255; no
//! alpha-0 pixel means no transparent index, and otherwise the alpha-0
//! pixels must be exactly the pixels of one index, which becomes the
//! transparent index; anything else refuses.

use crate::dr_image::DrImage;
use crate::icc::IccProfile;
use crate::indexed_palette::{code8, Rgb};
use crate::tiff_directory::{bytes_reader, Directory, Overlay, COMPRESSIONS};

/// The largest embedded profile read (the TIFF Lab and RGB opens' cap).
const MAX_ICC_BYTES: u32 = 16 * 1024 * 1024;

/// A palette TIFF's facts ([`palette_tiff`]) and the overlay's patch.
pub(crate) struct PaletteTiff {
    pub width: u32,
    pub height: u32,
    pub bits: u16,
    pub alpha: bool,
    pub palette: Vec<Rgb>,
    pub icc: Option<Vec<u8>>,
    patches: Vec<(u64, Vec<u8>)>,
}

/// Which palette TIFF the bytes `read` serves are (module doc), header work
/// only; `None` for every other file.
pub(crate) fn palette_tiff(
    read: &mut dyn FnMut(u64, usize) -> Option<Vec<u8>>,
) -> Option<PaletteTiff> {
    let found = Directory::first(read)?;
    let photometric = found.entry(262)?;
    if photometric.kind != 3 || found.scalar(262)? != 3 {
        return None;
    }
    let width = found.scalar(256)?;
    let height = found.scalar(257)?;
    let samples = found.scalar(277).unwrap_or(1);
    if !COMPRESSIONS.contains(&found.scalar(259).unwrap_or(1))
        || found.scalar(284).unwrap_or(1) != 1
    {
        return None;
    }
    let bits: Vec<u16> = match found.shorts(258, 2, read)? {
        Some(values) => values.into_iter().map(|(v, _)| v).collect(),
        None => vec![1; samples as usize],
    };
    let extras: Vec<u16> = found
        .shorts(338, 2, read)?
        .unwrap_or_default()
        .into_iter()
        .map(|(v, _)| v)
        .collect();
    let (depth, alpha) = match (samples, bits.as_slice(), extras.as_slice()) {
        (1, [b], []) if matches!(b, 1 | 2 | 4 | 8) => (*b, false),
        (2, [8, 8], [2]) => (8, true),
        _ => return None,
    };
    if let Some(formats) = found.shorts(339, 2, read)? {
        if formats.iter().any(|(v, _)| *v != 1) {
            return None;
        }
    }
    let entries = 1usize << depth;
    let map: Vec<u16> = found
        .shorts(320, 768, read)??
        .into_iter()
        .map(|(v, _)| v)
        .collect();
    if map.len() != 3 * entries {
        return None;
    }
    let palette = (0..entries)
        .map(|i| {
            [
                code8(map[i]),
                code8(map[entries + i]),
                code8(map[2 * entries + i]),
            ]
        })
        .collect();
    let icc = match found.entry(34675).copied() {
        None => None,
        Some(entry) => {
            if !matches!(entry.kind, 1 | 7) || entry.count <= 4 || entry.count > MAX_ICC_BYTES {
                return None;
            }
            let bytes = read(found.offset_of(&entry), entry.count as usize)?;
            let profile = IccProfile::parse(&bytes)?;
            if !crate::indexed_document::is_rgb_profile(&profile) {
                return None;
            }
            Some(bytes)
        }
    };
    let patches = vec![(photometric.at, found.short_bytes(1))];
    Some(PaletteTiff {
        width,
        height,
        bits: depth,
        alpha,
        palette,
        icc,
        patches,
    })
}

/// The palette TIFF the bytes are, when they are one.
pub(crate) fn palette_tiff_of(bytes: &[u8]) -> Option<PaletteTiff> {
    if !(bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*")) {
        return None;
    }
    palette_tiff(&mut bytes_reader(bytes))
}

/// The stored indices, one byte a pixel, and the transparent index (module
/// doc).
pub(crate) fn decode(
    bytes: &[u8],
    tiff: &PaletteTiff,
    path: &str,
) -> Result<(Vec<u8>, Option<u8>), String> {
    let fail = |what: String| format!("failed to decode {path}: {what}");
    let samples = 1 + usize::from(tiff.alpha);
    let decoder = tiff::decoder::Decoder::new(Overlay::new(bytes, &tiff.patches))
        .map_err(|e| fail(e.to_string()))?;
    let mut limits = tiff::decoder::Limits::default();
    limits.decoding_buffer_size =
        ((u64::from(tiff.width) * u64::from(tiff.height) * samples as u64) as usize)
            .max(crate::drdc::MAX_DRDC_BLOB_LEN as usize);
    limits.ifd_value_size = crate::drdc::MAX_DRDC_BLOB_LEN as usize;
    let mut decoder = decoder.with_limits(limits);
    let stored = decoder.dimensions().map_err(|e| fail(e.to_string()))?;
    if stored != (tiff.width, tiff.height) {
        return Err(fail("the directory's dimensions disagree".into()));
    }
    let raw = match decoder.read_image().map_err(|e| fail(e.to_string()))? {
        tiff::decoder::DecodingResult::U8(raw) => raw,
        _ => return Err(fail("the palette TIFF's samples are not 8-bit".into())),
    };
    let (w, h) = (tiff.width as usize, tiff.height as usize);
    if !tiff.alpha {
        let line = (w * usize::from(tiff.bits)).div_ceil(8);
        let indices = crate::indexed_io::unpack_rows(raw, w, h, tiff.bits as u8, line)
            .ok_or_else(|| fail("the sample count differs from the dimensions".into()))?;
        return Ok((indices, None));
    }
    if raw.len() < w * h * 2 {
        return Err(fail("the sample count differs from the dimensions".into()));
    }
    let alpha_refusal = || fail("the palette TIFF's alpha is not one transparent index".into());
    let mut indices = crate::precision_image::reserved_vec(w * h)?;
    // Which indices have a clear pixel and which an opaque one.
    let mut clear = [false; 256];
    let mut opaque = [false; 256];
    for pixel in raw[..w * h * 2].chunks_exact(2) {
        let (index, alpha) = (pixel[0], pixel[1]);
        match alpha {
            0 => clear[usize::from(index)] = true,
            255 => opaque[usize::from(index)] = true,
            _ => return Err(alpha_refusal()),
        }
        indices.push(index);
    }
    drop(raw);
    let mut cleared = (0..256).filter(|&i| clear[i]);
    let transparent = match (cleared.next(), cleared.next()) {
        (None, _) => None,
        (Some(t), None) if !opaque[t] => Some(t as u8),
        _ => return Err(alpha_refusal()),
    };
    Ok((indices, transparent))
}

/// The admitted plan of opening a palette TIFF of these facts from a file
/// of `encoded` bytes: the indexed open's own charge plus the decoded raw
/// samples (pixels × samples).
pub(crate) fn open_plan(tiff: &PaletteTiff, encoded: u64) -> Result<u64, String> {
    let pixels = crate::precision_image::pixel_count(tiff.width, tiff.height)? as u64;
    Ok(
        crate::indexed_io::open_scratch(tiff.width, tiff.height, encoded)?
            + pixels * (1 + u64::from(tiff.alpha)),
    )
}

/// An encoded export's preview when its bytes are a palette TIFF (the
/// generic decoder refuses Photometric 3): the palette colours, alpha 0 at
/// the transparent index. `None` for every other file.
pub(crate) fn preview_rgba8(bytes: &[u8]) -> Option<Result<DrImage, String>> {
    let tiff = palette_tiff_of(bytes)?;
    Some((|| {
        let (indices, transparent) = decode(bytes, &tiff, "the encoded export")?;
        let mut pixels = crate::precision_image::reserved_vec(indices.len() * 4)?;
        for &index in &indices {
            let rgb = tiff
                .palette
                .get(usize::from(index))
                .ok_or("failed to decode the encoded export: an index is outside the palette")?;
            let alpha = if Some(index) == transparent { 0 } else { 255 };
            pixels.extend_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
        }
        let pixels = image::RgbaImage::from_raw(tiff.width, tiff.height, pixels)
            .ok_or("failed to decode the encoded export: the preview's size is invalid")?;
        Ok(DrImage { pixels })
    })())
}
