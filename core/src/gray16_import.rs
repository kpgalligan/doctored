//! The 16-bit GRAYSCALE interpretation (Phase 11D): how a Gray16 PNG or TIFF
//! becomes a correctly profiled RGBA16 document.
//!
//! # The question this module answers
//!
//! A gray sample is a number with no colour attached. Turning it into an RGB
//! document is therefore an INTERPRETATION, and the only honest ones are the
//! two written here:
//!
//! * [`GrayInterpretation::SrgbCodes`] — the file says nothing about its
//!   transfer function (no ICC, or an `sRGB` chunk that declares exactly
//!   this), so the samples are read as sRGB-ENCODED gray and copied into
//!   R = G = B unchanged. No pixel conversion happens at all, which is what
//!   makes it a defensible default: the codes are preserved exactly, and a
//!   viewer that assumed sRGB sees what it saw before.
//! * [`GrayInterpretation::GrayProfile`] — the file carries a Gray ICC
//!   profile, so its `kTRC` says what the codes MEAN. The sample is
//!   linearized through that curve and re-encoded through the DOCUMENT's
//!   profile (always sRGB here, so the document's numbers are sRGB's
//!   numbers), which is a real conversion and changes the codes.
//!
//! Both leave the picture neutral: R = G = B in every output pixel, because
//! a gray file carries no chromaticity and inventing one would be a colour
//! cast the user never authored.
//!
//! # Why the destination is always sRGB
//!
//! A Gray profile has a tone curve and a white point and NO primaries, so
//! there is no "the file's own RGB space" to build. sRGB is the space every
//! other undeclared input in this build lands in, it round-trips a neutral
//! exactly (the three curves are one curve), and it is a space this crate can
//! both decode and encode. Adopting a working space instead is a document
//! policy question (`doc_color::adopt_working_space`) and happens afterwards,
//! on the RGB document this produces.
//!
//! # What is NOT here
//!
//! Gray as an export or convert DESTINATION and a grayscale document mode.
//! The BYTE path's Gray handling is `open_profile`: the flat open
//! converts a Gray-profiled file — including a 16-bit one opened with the
//! `.rgba8` choice, from its 16-bit samples — through [`encoded_gray`], the
//! same per-code rule, rounded to 8 bits. This module is the TYPED import.
//!
//! `GrayInterpretation` is either `SrgbCodes` (no ICC, or an `sRGB` chunk — the codes are copied
//! into R = G = B unchanged, so nothing is converted) or `GrayProfile(curve)` (a Gray ICC, or the
//! Gray profile a PNG's bare `gAMA` describes (`png_declared`) — `y = curve(g/65535)`, re-encoded
//! through the DOCUMENT profile's forward curve, `round(65535·c)` in all three channels).
//! `precision_codec::header` takes the gray branch BEFORE the shared `IccProfile::parse` step,
//! which gray never reaches — a Gray ICC is read with `icc_lut::ProofProfile` and must present
//! `DeviceSpace::Gray` with a `kTRC`, so an RGB ICC on a gray image refuses by sentence — and
//! `png_profile_contract(bytes, has_icc, gray)` tells it that a gray image reads `gAMA` alone. The
//! RGB path's bytes and sentences are untouched.

use crate::icc::IccProfile;
use crate::icc_transform::{Curve, PrecisionToneEncoder};
use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;

/// What the container said its gray samples mean.
#[derive(Clone, Debug)]
pub enum GrayInterpretation {
    /// sRGB-encoded gray: the codes are copied into R = G = B unchanged.
    SrgbCodes,
    /// A Gray ICC profile's tone curve: linearize through it, re-encode
    /// through the document's (sRGB) curve.
    GrayProfile(Curve),
}

/// Turns `channels`-per-pixel gray samples into straight RGBA16 pixels under
/// `interpretation`. `channels` is 1 (Gray) or 2 (Gray + unassociated alpha);
/// `encoder` is the DOCUMENT profile's forward encoder, which is always
/// sRGB's here and is passed in rather than built so a caller that imports
/// many planes builds its table once.
///
/// The arithmetic, stated so a test can reproduce it without reading code:
///
/// * `GrayProfile(curve)`: `y = curve.eval(g / 65535)` is the LINEAR
///   luminance the file's own curve says that code carries; `c` is that
///   value encoded by sRGB's forward curve (`encoder.rgb([y, y, y])[0]`);
///   the stored code is `round(65535 · c)`, written into R, G and B alike.
/// * `SrgbCodes`: R = G = B = `g`, exactly.
/// * Alpha is the second sample when `channels == 2`, else 65535.
pub(crate) fn gray_to_rgba16(
    samples: &[u16],
    channels: u8,
    interpretation: &GrayInterpretation,
    encoder: &PrecisionToneEncoder,
    job: JobContext<'_>,
) -> Result<Vec<[u16; 4]>, String> {
    if channels != 1 && channels != 2 {
        return Err("grayscale import: gray input has one or two samples per pixel".into());
    }
    let stride = usize::from(channels);
    if !samples.len().is_multiple_of(stride) {
        return Err("grayscale import: sample count differs from the declared channels".into());
    }
    let count = samples.len() / stride;
    let mut out = reserved_vec(count)?;
    // The curve is a pure function of ONE u16 code, so it is tabulated once
    // rather than evaluated per pixel: 65536 entries is 128 KiB and turns a
    // 100 MP import from 100 million curve evaluations into 65536.
    let table = match interpretation {
        GrayInterpretation::SrgbCodes => None,
        GrayInterpretation::GrayProfile(curve) => {
            let mut table: Vec<u16> = reserved_vec(65536)?;
            for code in 0..=u16::MAX {
                let encoded = encoded_gray(curve, encoder, f64::from(code) / 65535.0);
                table.push((encoded * 65535.0).round().clamp(0.0, 65535.0) as u16);
            }
            Some(table)
        }
    };
    for i in 0..count {
        job.every(i)?;
        let gray = samples[i * stride];
        let code = match &table {
            None => gray,
            Some(table) => table[usize::from(gray)],
        };
        let alpha = if channels == 2 {
            samples[i * stride + 1]
        } else {
            65535
        };
        out.push([code, code, code, alpha]);
    }
    job.check()?;
    Ok(out)
}

/// THE per-code Gray rule, shared by [`gray_to_rgba16`] and the flat open
/// (`open_profile::gray_to_srgb8`): `x` in [0, 1] is a gray code, `y =
/// curve(x)` the linear luminance the file's own curve says it carries, and
/// the answer is that luminance re-encoded by `encoder` (sRGB's forward
/// curve) as a CONTINUOUS code in [0, 1]. Each caller rounds it to its own
/// depth.
pub(crate) fn encoded_gray(curve: &Curve, encoder: &PrecisionToneEncoder, x: f64) -> f64 {
    let y = curve.eval(x);
    f64::from(encoder.rgb([y, y, y])[0])
}

/// The document profile every gray import lands in, and the encoder the
/// `GrayProfile` rule re-encodes through. One call so the two can never be a
/// different space.
pub(crate) fn srgb_encoder() -> Result<PrecisionToneEncoder, String> {
    let srgb = IccProfile::srgb();
    let model = srgb
        .model()
        .ok_or("grayscale import: the built-in sRGB profile has no matrix/TRC model")?;
    PrecisionToneEncoder::new(model)
}
