//! KERNEL: THE integer unpremultiply — one premultiplied (associated) colour
//! sample divided by its alpha at the sample's own depth. Every reader of
//! premultiplied integer samples routes through [`straight`]: the host's
//! premultiplied pixel layouts (`host_pixels::HostLayout`), the CMYK TIFF
//! reader's associated inks (`cmyk_tiff_read`), and the flat RGB and gray
//! TIFF opens of an alpha declared ExtraSamples 1 ([`straighten_u16`] and
//! [`straighten_u8`]: the 16-bit typed codec `precision_codec`, the Gray
//! document open `gray_io` and the byte open `dr_image`).
//!
//! The rule: `round(v · max / a)`, a half rounding up, capped at `max`; a
//! sample whose alpha is 0 has no recoverable colour and reads 0. The
//! division is done once, in integers wide enough for any 16-bit sample
//! (`65535 · 65535 + 32767` fits a `u64` with room to spare), so the result
//! is exact rather than a float's nearest neighbour. At full alpha it gives
//! the stored sample back unchanged (`(v · max + max / 2) / max = v`).
//!
//! A TIFF's ExtraSamples 1 means the colour was stored multiplied by alpha
//! (TIFF 6.0, "associated alpha"), and the division happens at the file's
//! own depth before any widening, so an 8-bit file's straight code is what
//! is widened. Recorded takes of Photoshop 26.10.0's flat RGB and Grayscale
//! TIFFs with partial transparency measured its stored colour to match
//! `c · a / max` and not straight or white-matted colour, at 8 and 16 bits
//! (`core/tests/fixtures/psd_photoshop/tiff_associated_alpha/README`).

/// One premultiplied colour sample divided by its alpha (module doc): 0 where
/// `alpha` is 0, else `round(value · maximum / alpha)` capped at `maximum`.
pub(crate) fn straight(value: u32, alpha: u32, maximum: u32) -> u32 {
    if alpha == 0 {
        0
    } else {
        ((u64::from(value) * u64::from(maximum) + u64::from(alpha) / 2) / u64::from(alpha))
            .min(u64::from(maximum)) as u32
    }
}

/// One interleaved 16-bit pixel whose LAST sample is its alpha, made
/// straight in place: every other sample through [`straight`] at 65535. The
/// slice is a decode's own buffer, never a published image.
pub(crate) fn straighten_u16(pixel: &mut [u16]) {
    let Some((alpha, colour)) = pixel.split_last_mut() else {
        return;
    };
    let alpha = u32::from(*alpha);
    for sample in colour {
        // At most 65535: `straight` caps at the maximum it is given.
        *sample = straight(u32::from(*sample), alpha, 65535) as u16;
    }
}

/// [`straighten_u16`] for an 8-bit pixel, at 255.
pub(crate) fn straighten_u8(pixel: &mut [u8]) {
    let Some((alpha, colour)) = pixel.split_last_mut() else {
        return;
    };
    let alpha = u32::from(*alpha);
    for sample in colour {
        *sample = straight(u32::from(*sample), alpha, 255) as u8;
    }
}
