//! KERNEL: THE integer unpremultiply — one premultiplied (associated) colour
//! sample divided by its alpha at the sample's own depth. Every reader of
//! premultiplied integer samples routes through [`straight`]: the host's
//! premultiplied pixel layouts (`host_pixels::HostLayout`) and the CMYK TIFF
//! reader's associated inks (`cmyk_tiff_read`).
//!
//! The rule: `round(v · max / a)`, a half rounding up, capped at `max`; a
//! sample whose alpha is 0 has no recoverable colour and reads 0. The
//! division is done once, in integers wide enough for any 16-bit sample
//! (`65535 · 65535 + 32767` fits a `u64` with room to spare), so the result
//! is exact rather than a float's nearest neighbour. At full alpha it gives
//! the stored sample back unchanged (`(v · max + max / 2) / max = v`).

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
