//! Premultiplied LINEAR-LIGHT resampling (P7-24 phase 2): the adapters a
//! linear-light 8- or 16-bit document's geometry hands the shared walks
//! (`sampling::resample_into`, `resize_into`, `warp_into`) in place of the
//! encoded `accumulate_unorm` / `straight_unorm` pair.
//!
//! The rule is the projection's: only what enters the accumulator is
//! decoded. Each tap's stored RGB is decoded through the document's transfer
//! ([`LinearCodes::linear`]) and accumulated premultiplied by its coverage
//! through the existing extended adapter (`sampling::accumulate_extended`,
//! the master; `accumulate_extended_f64` in the warp's binary64 walk), so no
//! tap arithmetic is new. The finish unpremultiplies and clamps to [0, 1]
//! through `sampling::straight_extended` with the unit range — a code-domain
//! resample's storage bounds, which a negative lobe's overshoot must not
//! leave — quantizes alpha exactly as `straight_unorm` does (a sample whose
//! alpha rounds to zero is transparent black), and encodes RGB ONCE
//! ([`LinearCodes::codes`]). Alpha is coverage and never passes through a
//! curve.
//!
//! What still copies codes: a `Nearest` walk (`write(di, read(si))`) and
//! every exact form the callers take before a walk (identity, integer
//! translation, quarter turns and flips) — a permutation of stored samples
//! has no domain. Masks, alpha channels and selections are coverage and
//! never come here.
//!
//! The walks that carry straight UNIT samples rather than stored codes (the
//! float of selected pixels, Liquify's render) take the same rule through
//! [`LinearUnit`]: each tap's RGB is decoded before it is premultiplied and
//! accumulated, and the clamped, unpremultiplied result is encoded once,
//! to the value the storage's own single quantize turns into the encoder's
//! code.

use image::imageops::FilterType;
use image::{Rgba, RgbaImage};

use crate::sampling::{
    accumulate_extended, resize_into, straight_extended, ColorRange, Kernel, Unorm,
};

/// The decode and encode a linear-light resample runs through: a document's
/// transfer tables at its storage depth (`ByteTransfer` for `u8`,
/// `TypedTransfer` for `u16`).
pub(crate) trait LinearCodes<T: Unorm> {
    /// Three stored codes as LINEAR components: the table entry exactly.
    fn linear(&self, codes: [T; 3]) -> [f32; 3];
    /// Three linear components in [0, 1] as the stored codes.
    fn codes(&self, linear: [f32; 3]) -> [T; 3];
}

/// [`LinearCodes`] for the walks over straight UNIT samples (module doc): a
/// byte document's `ByteTransfer`, a 16-bit document's `TypedTransfer`.
pub(crate) trait LinearUnit {
    /// Straight encoded RGB in [0, 1] as LINEAR components (the table entry
    /// on a stored code's lattice value).
    fn linear(&self, encoded: [f32; 3]) -> [f32; 3];
    /// Linear RGB in [0, 1] as straight encoded RGB in [0, 1]: the byte
    /// encoder's LATTICE value, or the typed encoder's continuous value that
    /// the store rounds once.
    fn encoded(&self, linear: [f32; 3]) -> [f32; 3];
}

/// The bounds a linear-light finish clamps to: the storage's own [0, 1].
const UNIT: ColorRange = ColorRange {
    min: [0.0; 3],
    max: [1.0; 3],
};

/// One stored pixel as straight linear RGB with unit alpha.
fn linear_sample<T: Unorm>(px: [T; 4], codes: &(impl LinearCodes<T> + ?Sized)) -> [f32; 4] {
    let rgb = codes.linear([px[0], px[1], px[2]]);
    [rgb[0], rgb[1], rgb[2], px[3].raw() / T::MAX]
}

/// Adds one stored pixel, decoded, to a premultiplied linear accumulator
/// (`acc` in unit scale, alpha included).
pub(crate) fn accumulate_linear<T: Unorm>(
    acc: &mut [f32; 4],
    px: [T; 4],
    weight: f32,
    codes: &(impl LinearCodes<T> + ?Sized),
) {
    accumulate_extended(acc, &linear_sample(px, codes), weight);
}

/// [`accumulate_linear`] in the binary64 accumulator `warp_into` hands its
/// adapters (`precision_warp::accumulate_extended_f64`, the master).
pub(crate) fn accumulate_linear_f64<T: Unorm>(
    acc: &mut [f64; 4],
    px: [T; 4],
    weight: f64,
    codes: &(impl LinearCodes<T> + ?Sized),
) {
    crate::precision_warp::accumulate_extended_f64(acc, linear_sample(px, codes), weight);
}

/// The accumulator as stored codes (module doc).
pub(crate) fn straight_linear<T: Unorm>(
    acc: [f32; 4],
    codes: &(impl LinearCodes<T> + ?Sized),
) -> [T; 4] {
    let straight = straight_extended(acc, &UNIT);
    let alpha = T::quantize(straight[3] * T::MAX);
    if alpha == T::default() {
        return [T::default(); 4];
    }
    let rgb = codes.codes([straight[0], straight[1], straight[2]]);
    [rgb[0], rgb[1], rgb[2], alpha]
}

/// [`straight_linear`] from the binary64 warp accumulator.
pub(crate) fn straight_linear_f64<T: Unorm>(
    acc: [f64; 4],
    codes: &(impl LinearCodes<T> + ?Sized),
) -> [T; 4] {
    straight_linear(acc.map(|v| v as f32), codes)
}

/// Image Size of straight RGBA8 under a document's evaluation policy:
/// `None` is `sampling::resize` exactly (an encoded document is untouched);
/// `Some` runs the same separable walk with the linear adapters for every
/// filter that interpolates. Nearest and the identity size still copy bytes
/// (`sampling::resize`), and the opaque `image`-crate shortcut is not taken,
/// since it interpolates codes.
pub(crate) fn resize(
    src: &RgbaImage,
    w: u32,
    h: u32,
    filter: FilterType,
    transfer: Option<&impl LinearCodes<u8>>,
) -> RgbaImage {
    let kernel = match filter {
        FilterType::Nearest => None,
        FilterType::Triangle => Some(Kernel::Bilinear),
        FilterType::CatmullRom => Some(Kernel::Bicubic),
        FilterType::Gaussian => Some(Kernel::Gaussian),
        FilterType::Lanczos3 => Some(Kernel::Lanczos3),
    };
    let (Some(codes), Some(kernel)) = (transfer, kernel) else {
        return crate::sampling::resize(src, w, h, filter);
    };
    if src.dimensions() == (w, h) {
        return crate::sampling::resize(src, w, h, filter);
    }
    let mut out = RgbaImage::new(w, h);
    resize_into(
        src.dimensions(),
        (w, h),
        kernel,
        |i| {
            src.get_pixel(i as u32 % src.width(), i as u32 / src.width())
                .0
        },
        |acc, sample, weight| accumulate_linear(acc, sample, weight, codes),
        |acc| straight_linear(acc, codes),
        |i, sample| out.put_pixel(i as u32 % w, i as u32 / w, Rgba(sample)),
        false,
        crate::job_control::JobContext::NONE,
    )
    .expect("uncancellable resize");
    out
}
