//! A Lab Color document's file samples both ways (#1): the TIFF 6.0
//! CIELab samples `lab_save` writes (phase 1b), and the file codes a Lab
//! PSD, PSB, layered TIFF or flat Lab TIFF stores, decoded into the
//! document's codes (phase 3a, [`storage_code`]). Free functions over
//! samples; the caller has rendered the composite or framed the planes.
//!
//! The document's codes are the ICC v4 16-bit Lab encoding (`lab_document`):
//! L\* = 100·l/65535, a\* = 255·a/65535 − 128 (b\* the same). TIFF 6.0's
//! CIELab stores L\* unsigned over its full range and a\*/b\* as SIGNED
//! integers:
//!
//! * **8 bits** — each code to its nearest byte first, c = (v + 128) / 257
//!   (`precision_quantize::nearest`'s rule, so an 8-bit document's ×257 grid
//!   returns exactly its own byte); then L = c (0…255 is 0…100), a/b =
//!   c − 128 as an `i8` (a\* = c − 128 exactly), alpha = c.
//! * **16 bits** — L = l unchanged (0…65535 is 0…100); a/b = round(a\*·256)
//!   clamped to −32768…32767 as an `i16` (the 1/256 unit TIFF's 16-bit
//!   CIELab uses; a\* = 127 is 32512), alpha kept.
//!
//! With `alpha` false the alpha sample is dropped (three samples a pixel).
//!
//! # Reading (phase 3a)
//!
//! [`storage_code`] turns one stored sample into the document's code, by
//! the file's [`LabFileEncoding`]. The PSD arms and the CIELab arms were
//! MEASURED against Photoshop 26.10.0 by phase 3a's take
//! (`scripts/photoshop-probe/lab_files_take.py`, `lab-files-547/take1`: the
//! file's sample at each of 104 colour-sampler points against Photoshop's
//! own `color.lab` reading, one meeting candidate per file):
//!
//! * **PSD (PSB, a layered TIFF's layer block)** — 8 bits L\* = 100v/255,
//!   a\* = v − 128; 16 bits L\* = 100v/65535 and a\*/b\* on Photoshop's
//!   15-bit grid ([`psd16_ab_code`]): Photoshop keeps 16-bit Lab as i in
//!   0…32768 and writes v = round(i·65535/32768), so i = round(v·32768/65535)
//!   and a\* = (i − 16384)/128. take1 met (v − 32768)/256 within its 0.05
//!   bound, but the readback in `lab_blend_take.py` measured the grid
//!   EXACTLY, and `lab-files-547/take2` confirmed it through the composite.
//!   The 0…32768 L\* scale once guessed for a 16-bit file is NOT what
//!   Photoshop writes.
//! * **TIFF PhotometricInterpretation 8 (CIELab, TIFF 6.0 §23)** — the
//!   writer's rule above read back: 8 bits L\* = 100v/255, a\* the sample as
//!   an `i8`; 16 bits L\* = 100v/65535, a\* the sample as an `i16` / 256.
//!   Photoshop writes both with SampleFormat absent.
//! * **TIFF PhotometricInterpretation 9 (ICCLab, TIFF Technote 4)** —
//!   DECLARED FROM THE PUBLISHED SPEC, NOT MEASURED AGAINST PHOTOSHOP (the
//!   take saw Photometric 8 only): 8 bits as the PSD (L\* = 100v/255,
//!   a\* = v − 128); 16 bits the ICC v2 legacy encoding, L\* = 100v/65280,
//!   a\* = v/256 − 128.
//!
//! Every 8-bit arm is the code ×257 of the unsigned offset sample (an `i8`
//! a\* first offset by 128): L\* = 100v/255 and a\* = v − 128 are exactly
//! the document's 8-bit grid. A 16-bit value is converted by the exact ratio
//! and rounded to the nearest code (ties up), clamped to 0…65535: a\* = 128
//! (v = 65535 in a PSD, i = 32768) lands on 65535. ICCLab's 16-bit a\* is
//! v·257/256 and CIELab's the `i16`/256; only the PSD arm is on the 15-bit
//! grid.
//!
//! # Writing (phase 3b)
//!
//! The PSD export (`psd_export_lab`) stores each document code as
//! [`psd_sample`] — the reading above inverted, so `psd_import_lab` returns
//! exactly the codes it was given wherever the file can hold them:
//!
//! * **8 bits** — the code to its nearest byte (`precision_quantize::nearest`,
//!   P8), L, a and b alike: an 8-bit document's ×257 grid is written exactly.
//! * **16 bits** — L\* as the code unchanged; a\*/b\* on Photoshop's 15-bit
//!   grid through [`psd16_ab_sample`], the inverse of [`psd16_ab_code`] and
//!   the ONE src encode of that sample: i = round(code·128/257), then v =
//!   round(i·65535/32768), each rounding ties up. A code off the grid snaps
//!   to its nearest i (at most half a 15-bit step, 1 code), and the
//!   document's a\* ceiling (code 65535, a\* = 127) is i = 32640: Photoshop's
//!   i above it (a\* up to 128) reads back as 65535 and is written as 32640.
//!
//! A flat picture (a PSD's composite planes, a layered TIFF's strip) carries
//! no alpha: Photoshop's own Lab files hold three planes. Its colour is
//! matted over Lab white ([`LAB_WHITE`]) by [`matte_white`], linearly in the
//! codes, the inverse of `psd_import_lab::lab_composite`'s unmatte.

/// Lab white in the document's codes: L\* 100, a\* = b\* = 0. The one home
/// of the matte both ways (`psd_import_lab`'s unmatte, [`matte_white`]).
pub(crate) const LAB_WHITE: [u16; 3] = [65535, 32896, 32896];

/// How a Lab file stores its samples (module doc's reading).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LabFileEncoding {
    /// A PSD, a PSB, or a layered TIFF's layer block (measured).
    Psd,
    /// A TIFF's PhotometricInterpretation 8 (measured).
    CieLab,
    /// A TIFF's PhotometricInterpretation 9 (declared from the published
    /// spec, not measured against Photoshop).
    IccLab,
}

/// `numerator / denominator` rounded to the nearest code, ties up, clamped
/// to the u16 range.
fn ratio(numerator: u64, denominator: u64) -> u16 {
    ((numerator * 2 + denominator) / (denominator * 2)).min(65535) as u16
}

/// A 16-bit PSD, PSB or layer-block a\*/b\* sample as the document's code
/// (module doc): i = round(v·32768/65535) on Photoshop's 15-bit grid, a\* =
/// (i − 16384)/128, whose ICC v4 code (a\* + 128)·257 is i·257/128, rounded
/// (ties up) and clamped to 65535. The ONE src decode of that sample.
pub(crate) fn psd16_ab_code(sample: u16) -> u16 {
    // v·32768/65535 is never a tie (65535 is odd and v·65536 even).
    let i = ratio(u64::from(sample) * 32768, 65535);
    ratio(u64::from(i) * 257, 128)
}

/// The inverse of [`psd16_ab_code`], the ONE src encode of a 16-bit PSD
/// a\*/b\* sample (module doc's writing): i = round(code·128/257), at most
/// 32768, then v = round(i·65535/32768), both ties up (i = 16384, a\* = 0,
/// is 32767.5 → 32768).
pub(crate) fn psd16_ab_sample(code: u16) -> u16 {
    let i = ratio(u64::from(code) * 128, 257).min(32768);
    ratio(u64::from(i) * 65535, 32768)
}

/// One document code of `channel` (0 L\*, 1 a\*, 2 b\*) as the sample a
/// `bits`-deep Lab PSD stores (module doc's writing): at 8 bits its nearest
/// byte (P8), at 16 the L\* code unchanged and a\*/b\* through
/// [`psd16_ab_sample`].
pub(crate) fn psd_sample(bits: u16, channel: usize, code: u16) -> u16 {
    if bits == 8 {
        return u16::from(crate::precision_quantize::nearest(code));
    }
    if channel == 0 {
        code
    } else {
        psd16_ab_sample(code)
    }
}

/// `code` of `channel` over [`LAB_WHITE`] at `alpha`, linearly in the codes
/// and rounded to the nearest code: (code·α + white·(65535 − α)) / 65535,
/// the flat picture's matte (module doc), which `psd_import_lab`'s unmatte
/// inverts.
pub(crate) fn matte_white(channel: usize, code: u16, alpha: u16) -> u16 {
    let a = u64::from(alpha);
    let white = u64::from(LAB_WHITE[channel.min(2)]);
    ratio(u64::from(code) * a + white * (65535 - a), 65535)
}

/// One stored sample of `channel` (0 L\*, 1 a\*, 2 b\*) as the document's
/// ICC v4 u16 code (module doc). `bits` is 8 or 16; an 8-bit `sample` is
/// its byte (0…255) and a signed sample arrives as its unsigned bits.
pub(crate) fn storage_code(
    encoding: LabFileEncoding,
    bits: u16,
    channel: usize,
    sample: u16,
) -> u16 {
    let lightness = channel == 0;
    if bits == 8 {
        let byte = sample & 0xFF;
        // CIELab's a*/b* byte is an i8: its offset form is the byte XOR 0x80.
        let offset = if encoding == LabFileEncoding::CieLab && !lightness {
            byte ^ 0x80
        } else {
            byte
        };
        return offset * 257;
    }
    let v = u64::from(sample);
    match (encoding, lightness) {
        // L* = 100v/65535: the document's own scale.
        (LabFileEncoding::Psd | LabFileEncoding::CieLab, true) => sample,
        // L* = 100v/65280 → code = v · 65535 / 65280.
        (LabFileEncoding::IccLab, true) => ratio(v * 65535, 65280),
        // The PSD's a* on the 15-bit grid (module doc).
        (LabFileEncoding::Psd, false) => psd16_ab_code(sample),
        // ICCLab: a* = v/256 − 128 → code = (a* + 128) · 257 = v · 257/256.
        (LabFileEncoding::IccLab, false) => ratio(v * 257, 256),
        // a* = i16/256: its offset form is the sample XOR 0x8000, then as ICCLab.
        (LabFileEncoding::CieLab, false) => ratio(u64::from(sample ^ 0x8000) * 257, 256),
    }
}

/// `pixels`' 8-bit CIELab samples, appended to `out` (module doc).
pub(crate) fn tiff_samples8(pixels: &[[u16; 4]], alpha: bool, out: &mut Vec<u8>) {
    let byte = crate::precision_quantize::nearest;
    for pixel in pixels {
        out.push(byte(pixel[0]));
        for c in [pixel[1], pixel[2]] {
            out.push((i16::from(byte(c)) - 128) as i8 as u8);
        }
        if alpha {
            out.push(byte(pixel[3]));
        }
    }
}

/// `pixels`' 16-bit CIELab samples, appended to `out` (module doc).
pub(crate) fn tiff_samples16(pixels: &[[u16; 4]], alpha: bool, out: &mut Vec<u16>) {
    let signed = |code: u16| {
        let star = 255.0 * f64::from(code) / 65535.0 - 128.0;
        (star * 256.0).round().clamp(-32768.0, 32767.0) as i16 as u16
    };
    for pixel in pixels {
        out.push(pixel[0]);
        out.push(signed(pixel[1]));
        out.push(signed(pixel[2]));
        if alpha {
            out.push(pixel[3]);
        }
    }
}
