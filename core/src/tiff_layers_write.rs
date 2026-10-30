//! The layered-TIFF writer (`tiff_layers`' module doc, "What is written"):
//! a flat TIFF plus Photoshop's three private tags (34377 resources, 34675
//! ICC, 37724 the layer block), laid into ONE buffer of the file's exact
//! size. [`write_with`] plans every offset before any byte is written and
//! has its caller append the layer information and the flat strip straight
//! into that buffer (the 32-bit float TIFF, `psd_export_float`, and
//! `tiff_layers_order`'s two re-lays); [`write`] is the big-endian form every
//! 8- and 16-bit writer calls with both already built. Split out of
//! `tiff_layers` (the reader) when the 32-bit form took it past its size.

use crate::metadata::Resolution;
use crate::tiff_layers::{ByteOrder, BLOCK_HEADER, TAG_ICC, TAG_LAYERS, TAG_RESOURCES};
use crate::tiff_spot_samples::SpotCoverage;

/// Spot channels in a file the writer cannot carry them in.
const SPOTS_ONLY: &str =
    "layered TIFF spot channels are written only for RGB or CMYK at 8 or 16 bits";

/// A complete big-endian layered TIFF, [`write_with`] with the layer
/// information and the strip already built. `samples_be` is the flat picture,
/// interleaved, row 0 first, samples big-endian at `depth`: straight RGBA for
/// `color` 3, straight CMYK ink and alpha (five samples, Photometric 5,
/// InkSet 1) for `color` 4, the white-matted gray (one sample, Photometric 1,
/// no ExtraSamples) for `color` 1, the CIELab samples of the composite matted
/// over Lab white (three samples, Photometric 8, no ExtraSamples, a\*/b\*
/// signed; `psd_export_lab::composite_strip_lab`) for `color` 9.
/// `resources` is a PSD image-resource section (tag 34377) and `info` the
/// layer information a classic PSD would carry (the block's body); an empty
/// `icc` writes no tag 34675. `Err` as [`write_with`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn write(
    width: u32,
    height: u32,
    depth: u16,
    color: u16,
    icc: &[u8],
    resolution: Resolution,
    resources: &[u8],
    info: &[u8],
    samples_be: &[u8],
    tail: &[crate::psd_layer_effects::Block],
) -> Result<Vec<u8>, String> {
    write_spots(
        width,
        height,
        depth,
        color,
        icc,
        resolution,
        resources,
        info,
        samples_be,
        tail,
        &[],
    )
}

/// [`write`] with `spots` as the composite's trailing extra samples (#1
/// 1c-2b, the flat TIFF's rule, `tiff_spot`): each pixel's samples are its
/// colour and alpha samples from `samples_be`, then every spot's
/// `SpotCoverage::stored` big-endian at `depth`, each declared ExtraSamples 0
/// after the alpha's 2. `resources` must already hold their resources. No
/// spot writes exactly [`write`]'s bytes. `Err` for spots in a file other
/// than RGB or CMYK at 8 or 16 bits, a spot plane that does not match the
/// canvas, and as [`write_with`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn write_spots(
    width: u32,
    height: u32,
    depth: u16,
    color: u16,
    icc: &[u8],
    resolution: Resolution,
    resources: &[u8],
    info: &[u8],
    samples_be: &[u8],
    tail: &[crate::psd_layer_effects::Block],
    spots: &[SpotCoverage<'_>],
) -> Result<Vec<u8>, String> {
    let file = LayeredTiff {
        width,
        height,
        depth,
        color,
        icc,
        resolution,
        resources,
        tail,
        order: ByteOrder::Big,
        spots: u16::try_from(spots.len())
            .map_err(|_| "a layered TIFF spot plane does not match the canvas".to_string())?,
    };
    let append_info = |out: &mut Vec<u8>| {
        out.extend_from_slice(info);
        Ok(())
    };
    if spots.is_empty() {
        return write_with(
            &file,
            info.len(),
            &append_info,
            Some(samples_be.len()),
            &|out| {
                out.extend_from_slice(samples_be);
                Ok(())
            },
        );
    }
    if !matches!(color, 3 | 4) || !matches!(depth, 8 | 16) {
        return Err(SPOTS_ONLY.into());
    }
    let pixels = width as usize * height as usize;
    let sample = usize::from(depth / 8);
    // The colour samples and the alpha: RGBA 4, CMYK and alpha 5.
    let pixel = (usize::from(color) + 1) * sample;
    if spots.iter().any(|s| s.pixels() != pixels) || samples_be.len() != pixels * pixel {
        return Err("a layered TIFF spot plane does not match the canvas".into());
    }
    let strip = pixels * (pixel + spots.len() * sample);
    write_with(&file, info.len(), &append_info, Some(strip), &|out| {
        for (p, colour) in samples_be.chunks_exact(pixel).enumerate() {
            out.extend_from_slice(colour);
            for spot in spots {
                let v = spot.stored(p, depth);
                if depth == 8 {
                    out.push(v as u8);
                } else {
                    out.extend_from_slice(&v.to_be_bytes());
                }
            }
        }
        Ok(())
    })
}

/// Everything about a layered TIFF but its layer information and its strip,
/// which [`write_with`] has its caller append straight into the file.
pub(crate) struct LayeredTiff<'a> {
    pub width: u32,
    pub height: u32,
    /// 8, 16 or 32; 32 is `color` 3 only, IEEE float (SampleFormat 3 on every
    /// sample) and the block key `Lr32`.
    pub depth: u16,
    pub color: u16,
    /// Empty writes no tag 34675.
    pub icc: &'a [u8],
    pub resolution: Resolution,
    /// A PSD image-resource section (tag 34377), always big-endian.
    pub resources: &'a [u8],
    /// The document-level blocks after the layers (the `Patt` table),
    /// big-endian: must be empty when `order` is little.
    pub tail: &'a [crate::psd_layer_effects::Block],
    /// [`ByteOrder::Little`] at depth 32 only.
    pub order: ByteOrder,
    /// The spot channels' trailing extra samples after the alpha, ExtraSamples
    /// 0 each ([`write_spots`]); 0 for every file without spot channels.
    pub spots: u16,
}

/// Where `bytes` go: zeros up to `at` (the plan's word alignment), then the
/// bytes. `Err` when the buffer has already passed `at`, which would be a
/// plan this writer did not follow.
fn put_at(out: &mut Vec<u8>, at: u64, bytes: &[u8]) -> Result<(), String> {
    let at = usize::try_from(at).map_err(|_| "layered TIFF offset overflow")?;
    if out.len() > at {
        return Err("internal error: the layered TIFF layout overran its plan".into());
    }
    out.resize(at, 0);
    out.extend_from_slice(bytes);
    Ok(())
}

/// The next word-aligned offset from `*len` for a value of `size` bytes, and
/// `*len` moved past it: every out-of-line value starts on a word boundary,
/// as TIFF requires.
fn place(len: &mut u64, size: usize) -> u64 {
    *len += *len & 1;
    let at = *len;
    *len += size as u64;
    at
}

/// A complete layered TIFF (`tiff_layers`' module doc) laid into ONE buffer
/// of its exact size. `append_info` appends the layer information (exactly
/// `info_len` bytes, in `file.order`) and `append_strip` the flat picture
/// (exactly `strip_len` bytes, interleaved, row 0 first, samples in
/// `file.order`: straight RGBA for `color` 3 — IEEE f32 at depth 32 —,
/// straight CMYK ink and alpha for 4, the white-matted gray for 1, the CIELab
/// composite over Lab white for 9). `strip_len` `None` writes no
/// StripOffsets, RowsPerStrip or StripByteCounts: the layer-only form
/// `tiff_layers_order::big_endian_form` hands the layered reader, never a
/// file on disk. `Err` for a depth other than 8, 16 or 32, a colour other
/// than 1, 3, 4 or 9 (3 only at 32), a little-endian file at another depth or
/// with a `tail`, a strip that does not match the canvas, an append of the
/// wrong length, or a file past the classic format's 4 GiB.
pub(crate) fn write_with(
    file: &LayeredTiff<'_>,
    info_len: usize,
    append_info: &dyn Fn(&mut Vec<u8>) -> Result<(), String>,
    strip_len: Option<usize>,
    append_strip: &dyn Fn(&mut Vec<u8>) -> Result<(), String>,
) -> Result<Vec<u8>, String> {
    let LayeredTiff {
        width,
        height,
        depth,
        color,
        icc,
        resolution,
        resources,
        tail,
        order,
        spots,
    } = *file;
    if !matches!(depth, 8 | 16 | 32) {
        return Err("layered TIFF export writes only 8-, 16- or 32-bit channels".into());
    }
    if depth == 32 && color != 3 {
        return Err("a 32-bit layered TIFF is written only for RGB".into());
    }
    if order == ByteOrder::Little && (depth != 32 || !tail.is_empty()) {
        return Err(
            "a little-endian layered TIFF is written only at 32 bits, with no document-level blocks"
                .into(),
        );
    }
    let (samples, photometric): (u16, u16) = match color {
        3 => (4, 2),
        4 => (5, 5),
        // #1 phase 3b, M6: Photoshop's own gray TIFF, one sample.
        1 => (1, 1),
        // #1 Lab phase 3b, L4: Photoshop's own Lab TIFF, three CIELab
        // samples (TIFF 6.0 §23), no alpha.
        9 => (3, 8),
        _ => return Err("layered TIFF export writes only RGB, CMYK, gray or Lab".into()),
    };
    if spots != 0 && (!matches!(color, 3 | 4) || !matches!(depth, 8 | 16)) {
        return Err(SPOTS_ONLY.into());
    }
    // The spot channels trail the alpha (`write_spots`).
    let samples = samples + spots;
    let strip = u64::from(width) * u64::from(height) * u64::from(samples) * u64::from(depth / 8);
    if width == 0 || height == 0 || strip_len.is_some_and(|len| len as u64 != strip) {
        return Err("layered TIFF flat image does not match the canvas".into());
    }
    let too_large = || "layered TIFF export exceeds the TIFF format's 4 GiB limit".to_string();
    let len32 = |len: u64| u32::try_from(len).map_err(|_| too_large());

    let key = order.code(match depth {
        8 => *b"Layr",
        16 => *b"Lr16",
        _ => *b"Lr32",
    });
    let tail = crate::psd_patterns::encode_tail(tail);
    let info_pad = info_len.wrapping_neg() & 3;
    let block_len = BLOCK_HEADER.len() + 12 + info_len + info_pad + tail.len();
    let array = |value: u16| -> Vec<u8> { (0..samples).flat_map(|_| order.put16(value)).collect() };
    let bits = array(depth);
    // IEEE float on every sample at 32 bits; 8 and 16 bits write none.
    let formats = if depth == 32 { array(3) } else { Vec::new() };
    // The alpha's unassociated 2, then 0 (unspecified) for each spot channel:
    // with none, the one inline SHORT every RGB and CMYK file writes.
    let extras: Vec<u8> = std::iter::once(2)
        .chain(std::iter::repeat_n(0, usize::from(spots)))
        .flat_map(|value: u16| order.put16(value))
        .collect();
    // A TIFF RATIONAL for a pixels-per-inch value, in ten-thousandths: exact
    // for every density the app's own fields can express.
    let rational = |value: f32| -> [u8; 8] {
        let numerator = (f64::from(value) * 10_000.0)
            .round()
            .clamp(0.0, 4_294_967_295.0) as u32;
        let mut out = [0u8; 8];
        out[..4].copy_from_slice(&order.put32(numerator));
        out[4..].copy_from_slice(&order.put32(10_000));
        out
    };
    let (x, y) = (rational(resolution.x), rational(resolution.y));

    // The plan: every offset before any byte is written.
    let mut len = 8u64;
    let strip_offset = len;
    len += strip_len.unwrap_or(0) as u64;
    // TIFF stores a value of four bytes or fewer in the entry itself, so a
    // gray file's one SHORT is inline, left-justified; more are placed.
    let bits_at = (bits.len() > 4).then(|| place(&mut len, bits.len()));
    let formats_at = (formats.len() > 4).then(|| place(&mut len, formats.len()));
    let extras_at = (extras.len() > 4).then(|| place(&mut len, extras.len()));
    let x_at = place(&mut len, x.len());
    let y_at = place(&mut len, y.len());
    let resources_at = place(&mut len, resources.len());
    let icc_at = place(&mut len, icc.len());
    let block_at = place(&mut len, block_len);
    let directory = place(&mut len, 0);
    let inline = |bytes: &[u8], at: Option<u64>| -> Result<[u8; 4], String> {
        Ok(match at {
            Some(at) => order.put32(len32(at)?),
            None => {
                let mut value = [0u8; 4];
                value[..bytes.len()].copy_from_slice(bytes);
                value
            }
        })
    };
    let short = |value: u16| {
        let b = order.put16(value);
        [b[0], b[1], 0, 0]
    };
    let long = |value: u64| len32(value).map(|v| order.put32(v));

    // Entries in ascending tag order, as TIFF requires.
    let mut entries: Vec<(u16, u16, u32, [u8; 4])> = vec![
        (256, 4, 1, order.put32(width)),
        (257, 4, 1, order.put32(height)),
        (258, 3, u32::from(samples), inline(&bits, bits_at)?),
        (259, 3, 1, short(1)),           // no compression
        (262, 3, 1, short(photometric)), // RGB, separated (CMYK), BlackIsZero or CIELab
    ];
    if strip_len.is_some() {
        entries.push((273, 4, 1, long(strip_offset)?));
    }
    entries.push((274, 3, 1, short(1))); // top-left
    entries.push((277, 3, 1, short(samples)));
    if let Some(strip) = strip_len {
        entries.push((278, 4, 1, order.put32(height)));
        entries.push((279, 4, 1, long(strip as u64)?));
    }
    entries.push((282, 5, 1, long(x_at)?));
    entries.push((283, 5, 1, long(y_at)?));
    entries.push((284, 3, 1, short(1))); // chunky
    entries.push((296, 3, 1, short(2))); // inches
    if color == 4 {
        entries.push((332, 3, 1, short(1))); // InkSet: CMYK
    }
    // RGB and CMYK carry an alpha sample; gray and Lab, as Photoshop
    // writes them, none.
    if matches!(color, 3 | 4) {
        // Unassociated alpha, then the spot channels' samples.
        let count = 1 + u32::from(spots);
        entries.push((338, 3, count, inline(&extras, extras_at)?));
    }
    if !formats.is_empty() {
        entries.push((339, 3, u32::from(samples), inline(&formats, formats_at)?));
    }
    if !resources.is_empty() {
        let count = len32(resources.len() as u64)?;
        entries.push((TAG_RESOURCES, 1, count, long(resources_at)?));
    }
    if !icc.is_empty() {
        entries.push((TAG_ICC, 7, len32(icc.len() as u64)?, long(icc_at)?));
    }
    entries.push((TAG_LAYERS, 7, len32(block_len as u64)?, long(block_at)?));
    let total = directory + 2 + 12 * entries.len() as u64 + 4;
    len32(total)?;
    let total = usize::try_from(total).map_err(|_| too_large())?;

    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(match order {
        ByteOrder::Big => b"MM\0*",
        ByteOrder::Little => b"II*\0",
    });
    out.extend_from_slice(&order.put32(len32(directory)?));
    if let Some(strip) = strip_len {
        append_strip(&mut out)?;
        if out.len() as u64 != strip_offset + strip as u64 {
            return Err("internal error: the layered TIFF strip does not match the canvas".into());
        }
    }
    if let Some(at) = bits_at {
        put_at(&mut out, at, &bits)?;
    }
    if let Some(at) = formats_at {
        put_at(&mut out, at, &formats)?;
    }
    if let Some(at) = extras_at {
        put_at(&mut out, at, &extras)?;
    }
    put_at(&mut out, x_at, &x)?;
    put_at(&mut out, y_at, &y)?;
    put_at(&mut out, resources_at, resources)?;
    put_at(&mut out, icc_at, icc)?;
    put_at(&mut out, block_at, BLOCK_HEADER)?;
    out.extend_from_slice(&order.code(*b"8BIM"));
    out.extend_from_slice(&key);
    out.extend_from_slice(&order.put32(len32(info_len as u64)?));
    let info_start = out.len();
    append_info(&mut out)?;
    if out.len() - info_start != info_len {
        return Err("internal error: the layered TIFF layer information changed length".into());
    }
    out.resize(out.len() + info_pad, 0);
    out.extend_from_slice(&tail);
    put_at(&mut out, directory, &order.put16(entries.len() as u16))?;
    for (tag, kind, count, value) in entries {
        out.extend_from_slice(&order.put16(tag));
        out.extend_from_slice(&order.put16(kind));
        out.extend_from_slice(&order.put32(count));
        out.extend_from_slice(&value);
    }
    out.extend_from_slice(&[0; 4]); // no next directory
    if out.len() != total {
        return Err("internal error: the layered TIFF does not match its plan".into());
    }
    Ok(out)
}
