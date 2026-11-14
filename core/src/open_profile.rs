//! What the FLAT open (`DrImage::open_bytes`) does with an embedded colour
//! profile a document cannot store:
//! a Gray-profiled grayscale image and a CMYK-profiled CMYK TIFF or JPEG are
//! CONVERTED from that profile to sRGB as they are read, and every embedded
//! profile the open does not keep is NAMED in one sentence, which the import
//! report carries (`ffi_metadata_report`, key `profile_refusal`).
//!
//! # The decision, in order ([`decide`])
//!
//! 1. No profile over CMYK samples: converted from the WORKING CMYK
//!    profile (`working_cmyk`, set by the host) exactly as an embedded CMYK
//!    profile converts, under [`untagged_cmyk_sentence`] naming it. With none
//!    set, the naive numbers are read as sRGB ([`ProfileAction::Ignore`])
//!    under the sentence saying so.
//! 2. No profile over any other samples, or one [`IccProfile::parse`]
//!    accepts: [`ProfileAction::Keep`] with no sentence. That is the
//!    untouched path — `IccProfile::parse` stays the ONE storable-profile
//!    rule, so every file that opened with a kept or absent profile decodes
//!    exactly as before.
//! 3. Otherwise the bytes are classified by `icc::inspect`:
//!    * not ICC at all — ignored (S6);
//!    * a device link, abstract or named-colour class — ignored (S5);
//!    * an RGB profile — which `IccProfile::parse` refused on SIZE alone, the
//!      16 MiB a document can carry — ignored (S4); converting through a
//!      profile the document then cannot hold is not planned;
//!    * any other data space: read with `ProofProfile::parse`. A Gray device
//!      over gray samples converts (S1); a CMYK device over CMYK samples —
//!      a TIFF's, or a CMYK/YCCK JPEG's raw planes (`cmyk_jpeg`) —
//!      with a relative-colorimetric device-to-PCS table converts (S2);
//!      anything else — a Gray profile on RGB samples, a CMYK one on gray
//!      samples, a Lab profile, a parse error — is ignored (S3).
//!
//! "Ignored" means today's pixels, labelled sRGB — now with a sentence.
//!
//! A PNG with no `iCCP` and no `sRGB` reaches [`decide`] with the profile its
//! bare `gAMA`/`cHRM` describes (`png_declared`, read by
//! `dr_image::embedded_color`) as if an `iCCP` held it; a declaration that
//! profile cannot be built from is ignored under [`declaration_ignored`].
//!
//! # The intent, and why
//!
//! * GRAY runs `gray16_import`'s rule, the typed 16-bit import's master
//!   ([`gray16_import::encoded_gray`]): linearize through the profile's
//!   `kTRC`, re-encode through sRGB's forward curve, round to 8 bits. That is
//!   relative colorimetric between two tone curves, where black-point
//!   compensation has nothing to move, and it keeps the byte and typed
//!   opens of one gray file on one rule.
//! * CMYK is RELATIVE COLORIMETRIC WITH BLACK-POINT COMPENSATION — the only
//!   choice that opens a rich black as black: a press profile's black sits
//!   well above L* = 0, and without BPC every shadow would open as a lifted
//!   grey. The arithmetic is the existing soft-proof chain's, not new pixel
//!   math: `ProofProfile::to_xyz` at the relative table,
//!   `icc_intent::bpc_terms` from the profile's `proof_black_point` — Little
//!   CMS's `cmsDetectBlackPoint` rule — to sRGB's black applied as
//!   `soft_proof` applies it, then `DocumentPcs::from_xyz` into the built-in
//!   sRGB. A profile whose black the rule cannot read (a missing table)
//!   answers lcms's zero black, and BPC onto sRGB's zero black is then the
//!   identity: the conversion runs relative colorimetric uncompensated, under
//!   the same sentence.
//!
//! # One gate for every CMYK source
//!
//! Only 8/16-bit CMYK TIFF — chunky or planar, four to 56 samples a pixel, the
//! first extra sample read as its transparency when it is one (#1 phase 3,
//! `cmyk_tiff_read`) — and a four-component CMYK/YCCK JPEG, inside
//! `MAX_PIXELS` — and, for the TIFF, inside the two buffers `image`'s own
//! decode leaves `tiff` ([`image_tiff_limits`]) — is read here. That gate is ONE function,
//! [`cmyk_gate`], and the open asks it for EVERY CMYK source, whatever
//! [`decide`] answered: a CMYK file it refuses fails the core open,
//! so the host's platform fallback reads it, and the header-only report
//! (`dr_image::embedded_profile_report`, through [`cmyk_convertible`]) gives
//! it no `profile_refusal` — there is no core open to describe. The
//! converted sentence is ONE function, [`converted_sentence`], which the
//! typed 16-bit open's Gray conversion also words its warning through.
//!
//! # What a platform decode says
//!
//! A Gray, CMYK or Lab image the HOST decodes — the ImageIO fallback after
//! the core open fails, or New from Clipboard — is converted to sRGB by
//! ColorSync as it is read. [`platform_conversion_sentence`] words that
//! (`dr_platform_conversion_sentence`, over `dr_image::embedded_icc`): the
//! converted sentence when [`decide`] would itself convert those samples
//! from the EMBEDDED profile, else one plain sentence naming the colour
//! model — a profile this open cannot convert from is not described as the
//! one the platform used, and a Lab image never is. An untagged image gets
//! the plain sentence even with a working CMYK profile set: the platform
//! converted it from its own Generic CMYK, not from the working profile.
//!
//! No profile over CMYK samples → converted from the working CMYK profile (`working_cmyk`)
//! under `untagged_cmyk_sentence`, or read naively as sRGB under its unset sentence when none is
//! set. `IccProfile::parse` accepts → Keep (the untouched path); a Gray profile over gray samples →
//! converted through `gray16_import::encoded_gray`; a CMYK profile over a CMYK TIFF's or CMYK/YCCK
//! JPEG's own samples → converted relative colorimetric WITH BPC through ONE per-pixel
//! converter (`ProofProfile::to_xyz` + `icc_intent::bpc_terms` from `icc_black_point`'s source and
//! destination blacks + `DocumentPcs::from_xyz`, memoized); anything else ignored (numbers read as
//! sRGB). Every non-kept profile gets one sentence, the import report's `profile_refusal`
//! (`dr_image::embedded_profile_report`, header work only). The CMYK gate is ONE function
//! (`cmyk_gate`: the TIFF arm `cmyk_tiff_layout` — 8/16-bit, chunky or planar, 4–56 samples,
//! `MAX_PIXELS`, and the output/strip limits `image_tiff_limits` derives as `image` 0.25 does; the
//! JPEG arm
//! `cmyk_jpeg::cmyk_dimensions` inside `MAX_PIXELS`), asked by the open for EVERY CMYK source
//! whatever the decision and by the report through `cmyk_convertible`, so a CMYK file the
//! open refuses has no `profile_refusal`. A HOST platform decode of a Gray, CMYK or Lab
//! image (the ImageIO fallback, New from Clipboard) is worded by `platform_conversion_sentence`
//! (`PlatformSource`, `dr_platform_conversion_sentence` over
//! `dr_image::embedded_icc`): `converted_sentence` when `decide` would convert those samples from the
//! embedded profile, else one plain sentence naming the model, always for Lab and an untagged image.

use std::collections::HashMap;
use std::ffi::c_int;
use std::io::Cursor;
use std::sync::Arc;

use image::{DynamicImage, ImageBuffer, ImageDecoder, Pixel, RgbaImage};
use tiff::decoder::Decoder;
use tiff::tags::Tag;
use tiff::ColorType;

use crate::cmyk_color::{CmykToRgb, ConversionOptions};
use crate::cmyk_jpeg;
use crate::cmyk_tiff_read::{AlphaKind, CmykTiffLayout};
use crate::doc::MAX_PIXELS;
use crate::gray16_import::{self, encoded_gray};
use crate::icc::{self, IccBytes, IccProfile, Inspect};
use crate::icc_intent::RenderingIntent;
use crate::icc_lut::{DeviceSpace, ProofProfile};
use crate::icc_transform::Curve;
use crate::soft_proof::MEMO_CAPACITY;
use crate::working_cmyk::working_cmyk_profile;

/// Which container a CMYK source's own samples are read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CmykContainer {
    Tiff,
    Jpeg,
}

/// What the file's samples are, before any conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceSamples {
    Rgb,
    Gray,
    Cmyk(CmykContainer),
}

impl SourceSamples {
    /// Cmyk when [`cmyk_container`] finds CMYK samples in `bytes` (asked
    /// FIRST: `image`'s JPEG decoder has already turned CMYK into RGB, and
    /// a CMYK+alpha TIFF never reaches it); Gray when the decoder's colour
    /// type is L8, La8, L16 or La16; else Rgb.
    pub(crate) fn of(decoder: &impl ImageDecoder, bytes: &[u8]) -> SourceSamples {
        use image::ColorType::{La16, La8, L16, L8};
        if let Some(container) = cmyk_container(bytes) {
            SourceSamples::Cmyk(container)
        } else if matches!(decoder.color_type(), L8 | La8 | L16 | La16) {
            SourceSamples::Gray
        } else {
            SourceSamples::Rgb
        }
    }
}

/// The container a CMYK file's samples live in, header work only: a TIFF
/// whose colour type is CMYK or CMYK+alpha at any depth, or a JPEG
/// `cmyk_jpeg` reads as four-component CMYK/YCCK. `None` for everything
/// else.
pub(crate) fn cmyk_container(bytes: &[u8]) -> Option<CmykContainer> {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        return cmyk_jpeg::cmyk_dimensions(bytes).map(|_| CmykContainer::Jpeg);
    }
    let mut decoder = Decoder::new(Cursor::new(bytes)).ok()?;
    matches!(
        decoder.colortype().ok()?,
        ColorType::CMYK(_) | ColorType::CMYKA(_)
    )
    .then_some(CmykContainer::Tiff)
}

/// What the open does with the pixels.
pub(crate) enum ProfileAction {
    /// Today's path; the profile (if any) becomes the document's.
    Keep,
    /// Convert gray samples through this `kTRC` to sRGB.
    ConvertGray(Curve),
    /// Convert CMYK samples, read again from this container, through this
    /// profile — the embedded one, or the working CMYK profile for an
    /// untagged file — to sRGB.
    ConvertCmyk(Arc<ProofProfile>, CmykContainer),
    /// Today's pixels, labelled sRGB.
    Ignore,
}

/// The decision and, for every action but [`ProfileAction::Keep`], the
/// sentence the import report carries.
pub(crate) struct OpenProfile {
    pub(crate) action: ProfileAction,
    pub(crate) sentence: Option<String>,
}

const READ_AS_SRGB: &str = "so it was ignored and the image's colour numbers were read as sRGB.";

/// The module doc's decision, from the embedded bytes and the decoder's
/// sample kind. Header work only — nothing here touches pixels.
pub(crate) fn decide(icc: Option<&[u8]>, source: SourceSamples) -> OpenProfile {
    let Some(icc) = icc else {
        return match source {
            SourceSamples::Cmyk(container) => untagged_cmyk(container),
            SourceSamples::Rgb | SourceSamples::Gray => keep(),
        };
    };
    if IccProfile::parse(icc).is_some() {
        return keep();
    }
    // The view exists exactly when `inspect` does not answer NotIcc (both
    // are `header_end`); the sentences read the view.
    let kind = icc::inspect(icc, &mut String::new());
    let Some(view) = IccBytes::new(icc) else {
        return not_icc();
    };
    let name = view.name();
    match kind {
        Inspect::NotIcc => not_icc(),
        Inspect::NotImageProfile => ignore(format!(
            "The embedded colour profile “{name}” is a device link, an abstract profile or a \
             named-colour list, not a colour space, {READ_AS_SRGB}"
        )),
        Inspect::RgbMatrix | Inspect::RgbLut | Inspect::RgbUnconvertible => ignore(format!(
            "The embedded colour profile “{name}” is {} bytes, over the 16 MiB a Doctored \
             document can carry, {READ_AS_SRGB}",
            view.declared_len()
        )),
        Inspect::NotRgb => {
            let converted = |kind: &str| Some(converted_sentence(&name, kind));
            match ProofProfile::parse(icc) {
                Ok(profile)
                    if profile.device() == DeviceSpace::Gray && source == SourceSamples::Gray =>
                {
                    match profile.gray_curve() {
                        Some(curve) => OpenProfile {
                            action: ProfileAction::ConvertGray(curve.clone()),
                            sentence: converted("Gray"),
                        },
                        None => cannot_convert(&view, &name),
                    }
                }
                Ok(profile)
                    if profile.device() == DeviceSpace::Cmyk
                        && profile
                            .to_pcs(RenderingIntent::RelativeColorimetric)
                            .is_some() =>
                {
                    match source {
                        SourceSamples::Cmyk(container) => OpenProfile {
                            action: ProfileAction::ConvertCmyk(Arc::new(profile), container),
                            sentence: converted("CMYK"),
                        },
                        _ => cannot_convert(&view, &name),
                    }
                }
                _ => cannot_convert(&view, &name),
            }
        }
    }
}

/// S1/S2: the sentence for a profile the image was converted FROM. The typed
/// 16-bit open's Gray conversion (`precision_codec::gray_interpretation`)
/// words its warning through this too, so both opens of one file say the same
/// thing and the Assign Profile sheet can show it verbatim.
pub(crate) fn converted_sentence(name: &str, kind: &str) -> String {
    format!(
        "The embedded colour profile “{name}” is a {kind} profile, so the image was converted \
         from it to sRGB as it was read."
    )
}

/// The sentence for an untagged CMYK image — converted from the
/// working CMYK profile named `working`, or, with none set, read without
/// colour management.
pub(crate) fn untagged_cmyk_sentence(working: Option<&str>) -> String {
    match working {
        Some(name) => format!(
            "The image is CMYK with no embedded colour profile, so it was converted to sRGB from \
             the working CMYK profile “{name}” as it was read."
        ),
        None => "The image is CMYK with no embedded colour profile and no working CMYK profile \
                 is set, so its colours were converted to sRGB without colour management as it \
                 was read."
            .to_string(),
    }
}

/// [`decide`] for CMYK samples with no embedded profile: through the
/// working CMYK profile when one is set, else today's naive numbers.
fn untagged_cmyk(container: CmykContainer) -> OpenProfile {
    match working_cmyk_profile() {
        Some(working) => OpenProfile {
            sentence: Some(untagged_cmyk_sentence(Some(working.name()))),
            action: ProfileAction::ConvertCmyk(working, container),
        },
        None => ignore(untagged_cmyk_sentence(None)),
    }
}

/// A platform-decoded image's colour model (module doc), `DR_PLATFORM_SOURCE_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlatformSource {
    Gray,
    Cmyk,
    Lab,
}

impl PlatformSource {
    /// The `DR_PLATFORM_SOURCE_*` discriminant; mapped, never transmuted.
    pub(crate) fn from_c(value: c_int) -> Option<Self> {
        match value {
            0 => Some(PlatformSource::Gray),
            1 => Some(PlatformSource::Cmyk),
            2 => Some(PlatformSource::Lab),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            PlatformSource::Gray => "Gray",
            PlatformSource::Cmyk => "CMYK",
            PlatformSource::Lab => "Lab",
        }
    }
}

/// The sentence for a platform decode of `source` samples carrying `icc`:
/// [`decide`]'s converted sentence when it would convert such samples from
/// that EMBEDDED profile itself (S1, S2), else the plain one naming the
/// model — for no profile (even with a working CMYK profile set: the
/// platform did not convert from it), any other profile, and every Lab
/// image.
pub(crate) fn platform_conversion_sentence(icc: Option<&[u8]>, source: PlatformSource) -> String {
    if icc.is_none() {
        return plain_platform_sentence(source);
    }
    let decision = match source {
        PlatformSource::Gray => Some(decide(icc, SourceSamples::Gray)),
        // The container only rides into `ConvertCmyk`'s action, which is
        // never run here: the platform has already decoded the samples.
        PlatformSource::Cmyk => Some(decide(icc, SourceSamples::Cmyk(CmykContainer::Tiff))),
        PlatformSource::Lab => None,
    };
    match decision {
        Some(OpenProfile {
            action: ProfileAction::ConvertGray(_) | ProfileAction::ConvertCmyk(..),
            sentence: Some(sentence),
        }) => sentence,
        _ => plain_platform_sentence(source),
    }
}

fn plain_platform_sentence(source: PlatformSource) -> String {
    format!(
        "The image is {}, with no embedded colour profile Doctored can convert from, so it was \
         converted to sRGB as it was read.",
        source.name()
    )
}

/// A PNG whose bare `gAMA`/`cHRM` `png_declared` cannot use, `clause` its
/// reason ("the PNG's gAMA chunk is 0, …"): today's numbers, read as sRGB,
/// under a sentence that says so.
pub(crate) fn declaration_ignored(clause: &str) -> OpenProfile {
    let mut letters = clause.chars();
    let opening: String = letters
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    ignore(format!("{opening}{}, {READ_AS_SRGB}", letters.as_str()))
}

/// S6.
fn not_icc() -> OpenProfile {
    ignore(format!(
        "The embedded colour profile is not an ICC profile Doctored can read, {READ_AS_SRGB}"
    ))
}

fn keep() -> OpenProfile {
    OpenProfile {
        action: ProfileAction::Keep,
        sentence: None,
    }
}

fn ignore(sentence: String) -> OpenProfile {
    OpenProfile {
        action: ProfileAction::Ignore,
        sentence: Some(sentence),
    }
}

/// S3: a profile of a space this open cannot convert THIS image from.
fn cannot_convert(view: &IccBytes, name: &str) -> OpenProfile {
    let space = match view.data_space() {
        b"GRAY" => "Gray".to_string(),
        _ => view.space_name(),
    };
    ignore(format!(
        "The embedded colour profile “{name}” is a {space} profile that Doctored cannot convert \
         this image from, {READ_AS_SRGB}"
    ))
}

/// Converts a decoded (and already oriented) grayscale image through a Gray
/// profile's `curve` to sRGB RGBA8. Alpha is `to_rgba8`'s, exactly today's;
/// R = G = B is overwritten from the ORIGINAL luma sample — 8-bit through a
/// 256-entry table, 16-bit through a 65536-entry one, so a 16-bit file opened
/// in 8 bits converts from its 16-bit samples — each entry
/// `round(255 · clamp(encoded_gray(x)))`, the typed import's rule.
pub(crate) fn gray_to_srgb8(
    image: &DynamicImage,
    curve: &Curve,
    path: &str,
) -> Result<RgbaImage, String> {
    let encoder =
        gray16_import::srgb_encoder().map_err(|e| format!("failed to decode {path}: {e}"))?;
    let table = |levels: usize| -> Vec<u8> {
        let top = (levels - 1) as f64;
        (0..levels)
            .map(|g| {
                let c = encoded_gray(curve, &encoder, g as f64 / top);
                (255.0 * c.clamp(0.0, 1.0)).round() as u8
            })
            .collect()
    };
    let mut out = image.to_rgba8();
    match image {
        DynamicImage::ImageLuma8(g) => overwrite_gray(&mut out, g, &table(256)),
        DynamicImage::ImageLumaA8(g) => overwrite_gray(&mut out, g, &table(256)),
        DynamicImage::ImageLuma16(g) => overwrite_gray(&mut out, g, &table(65536)),
        DynamicImage::ImageLumaA16(g) => overwrite_gray(&mut out, g, &table(65536)),
        _ => {
            return Err(format!(
                "failed to decode {path}: grayscale samples changed representation"
            ))
        }
    }
    Ok(out)
}

fn overwrite_gray<P>(out: &mut RgbaImage, source: &ImageBuffer<P, Vec<P::Subpixel>>, table: &[u8])
where
    P: Pixel,
    P::Subpixel: Into<usize>,
{
    for (dst, src) in out.pixels_mut().zip(source.pixels()) {
        let code = table[src.channels()[0].into()];
        dst.0[..3].fill(code);
    }
}

/// The `tiff` crate's limits exactly as `image` 0.25 derives them for its own
/// TIFF decode (`TiffDecoder::set_limits` under `image::Limits::default()`,
/// which `ImageReader::into_decoder` applies): the output buffer is what it
/// is, and the intermediate buffer — which bounds one strip or tile — is the
/// allowance left beside it. Copying the precision codec's limits instead
/// would leave the intermediate at `tiff`'s 128 MiB default and refuse a
/// large single-strip CMYK TIFF that `image` itself admits.
pub(crate) fn image_tiff_limits(output_bytes: u64) -> tiff::decoder::Limits {
    let max_alloc = image::Limits::default().max_alloc.unwrap_or(u64::MAX);
    let intermediate = max_alloc.saturating_sub(output_bytes);
    let mut limits = tiff::decoder::Limits::default();
    limits.decoding_buffer_size = usize::try_from(max_alloc - intermediate).unwrap_or(usize::MAX);
    limits.intermediate_buffer_size = usize::try_from(intermediate).unwrap_or(usize::MAX);
    limits.ifd_value_size = limits.intermediate_buffer_size;
    limits
}

/// The TIFF arm of [`cmyk_gate`], header only: the [`CmykTiffLayout`] when
/// the TIFF is 8/16-bit CMYK (or CMYK with transparency) with 4 to 56
/// SamplesPerPixel, chunky or planar (a planar file one strip or tile set per
/// sample), inside `MAX_PIXELS`, and its decode fits the two buffers
/// [`image_tiff_limits`] gives `tiff` — the output (`decoding_buffer_size`,
/// the whole image's kept samples, reachable at 16 bits past 67,108,864
/// pixels) and every strip or tile (`intermediate_buffer_size`) — else the
/// reason this open does not convert it.
pub(crate) fn cmyk_tiff_layout<R: std::io::Read + std::io::Seek>(
    decoder: &mut Decoder<R>,
    path: &str,
) -> Result<CmykTiffLayout, String> {
    let fail = |what: String| format!("failed to decode {path}: {what}");
    let (w, h) = decoder.dimensions().map_err(|e| fail(e.to_string()))?;
    let pixels = u64::from(w) * u64::from(h);
    if w == 0 || h == 0 || pixels > MAX_PIXELS {
        return Err(fail(format!(
            "a {w} x {h} image is outside the {MAX_PIXELS}-pixel limit"
        )));
    }
    // `tiff`'s own colour type: CMYKA when the first extra sample is the
    // transparency (`cmyk_tiff_read`).
    let (depth, transparent): (u64, bool) =
        match decoder.colortype().map_err(|e| fail(e.to_string()))? {
            ColorType::CMYK(8) => (1, false),
            ColorType::CMYK(16) => (2, false),
            ColorType::CMYKA(8) => (1, true),
            ColorType::CMYKA(16) => (2, true),
            other => {
                return Err(fail(format!(
                    "{other:?} is not 8- or 16-bit CMYK this open converts"
                )))
            }
        };
    let samples: u16 = decoder
        .find_tag_unsigned(Tag::SamplesPerPixel)
        .map_err(|e| fail(e.to_string()))?
        .unwrap_or(1);
    if !(4..=56).contains(&samples) || (transparent && samples < 5) {
        return Err(fail(format!(
            "a CMYK image with {samples} samples per pixel is not read by this open"
        )));
    }
    let planar: u16 = decoder
        .find_tag_unsigned(Tag::PlanarConfiguration)
        .map_err(|e| fail(e.to_string()))?
        .unwrap_or(1);
    if !matches!(planar, 1 | 2) {
        return Err(fail(format!(
            "PlanarConfiguration {planar} is not a CMYK layout this open reads"
        )));
    }
    let first_extra: Option<u16> = decoder
        .find_tag_unsigned_vec::<u16>(Tag::ExtraSamples)
        .map_err(|e| fail(e.to_string()))?
        .and_then(|extras| extras.first().copied());
    let alpha = transparent.then(|| {
        if first_extra == Some(1) {
            AlphaKind::Associated
        } else {
            AlphaKind::Unassociated
        }
    });
    let layout = CmykTiffLayout {
        width: w,
        height: h,
        bytes: depth,
        samples,
        planar: planar == 2,
        alpha,
        extras: samples - 4 - u16::from(transparent),
    };
    // One strip or tile set a plane: `tiff` indexes a planar file's chunks
    // by plane, so a short offset list must refuse here, not in the decode.
    let rows = decoder
        .find_tag_unsigned::<u32>(Tag::RowsPerStrip)
        .map_err(|e| fail(e.to_string()))?;
    let tile = (
        decoder
            .find_tag_unsigned::<u32>(Tag::TileWidth)
            .map_err(|e| fail(e.to_string()))?,
        decoder
            .find_tag_unsigned::<u32>(Tag::TileLength)
            .map_err(|e| fail(e.to_string()))?,
    );
    let (chunks, offsets) = match tile {
        (Some(tw), Some(th)) if tw > 0 && th > 0 => (
            u64::from(w.div_ceil(tw)) * u64::from(h.div_ceil(th)),
            decoder
                .find_tag_unsigned_vec::<u64>(Tag::TileOffsets)
                .map_err(|e| fail(e.to_string()))?
                .map_or(0, |v| v.len() as u64),
        ),
        _ => (
            u64::from(h.div_ceil(rows.unwrap_or(h).clamp(1, h))),
            decoder
                .find_tag_unsigned_vec::<u64>(Tag::StripOffsets)
                .map_err(|e| fail(e.to_string()))?
                .map_or(0, |v| v.len() as u64),
        ),
    };
    let planes = if layout.planar {
        u64::from(layout.samples)
    } else {
        1
    };
    if offsets != chunks * planes {
        return Err(fail(if layout.planar {
            crate::cmyk_tiff_read::PLANAR_STRIPS.into()
        } else {
            format!("a CMYK TIFF with {offsets} strips or tiles needs {chunks}")
        }));
    }
    // The two limits `tiff` enforces under these exact limits: the output
    // buffer (decoder/mod.rs, `DecodingResult::new`) and each compressed
    // strip or tile (decoder/image.rs, `expand_chunk`).
    let bytes = layout.decoded_bytes();
    let limits = image_tiff_limits(bytes);
    let max = limits.decoding_buffer_size as u64;
    if bytes > max {
        return Err(fail(format!(
            "a {w} x {h} {}-bit CMYK image is {bytes} bytes, over the {max}-byte decode limit",
            8 * depth
        )));
    }
    let beside = limits.intermediate_buffer_size as u64;
    for tag in [Tag::StripByteCounts, Tag::TileByteCounts] {
        let counts: Vec<u64> = decoder
            .find_tag_unsigned_vec(tag)
            .map_err(|e| fail(e.to_string()))?
            .unwrap_or_default();
        if let Some(n) = counts.into_iter().find(|&n| n > beside) {
            return Err(fail(format!(
                "a {n}-byte strip or tile is over the {beside} bytes the decode leaves beside \
                 the image"
            )));
        }
    }
    Ok(layout)
}

/// A CMYK TIFF the gate admits but `image`'s own decoder does not read — a
/// planar one, or one with a transparency or extra samples (#1 phase 3):
/// the flat open reads it through `cmyk_tiff_read` alone, and only when it
/// converts from a CMYK profile; otherwise the platform reads it, as before.
pub(crate) fn cmyk_tiff_beyond_image(bytes: &[u8]) -> bool {
    if cmyk_container(bytes) != Some(CmykContainer::Tiff) {
        return false;
    }
    let Ok(mut decoder) = Decoder::new(Cursor::new(bytes)) else {
        return false;
    };
    cmyk_tiff_layout(&mut decoder, "the file")
        .is_ok_and(|layout| layout.planar || layout.samples != 4)
}

/// THE CMYK gate (module doc), header only: `Ok` exactly when this open
/// reads the file's own CMYK samples — a TIFF through [`cmyk_tiff_layout`],
/// a JPEG whose CMYK/YCCK header (`cmyk_jpeg::cmyk_dimensions`) is inside
/// `MAX_PIXELS` — else the reason, naming `path`. The open asks it for every
/// CMYK source, so a file it refuses is left to the platform.
pub(crate) fn cmyk_gate(bytes: &[u8], path: &str, container: CmykContainer) -> Result<(), String> {
    let fail = |what: String| format!("failed to decode {path}: {what}");
    match container {
        CmykContainer::Tiff => {
            let mut decoder = Decoder::new(Cursor::new(bytes)).map_err(|e| fail(e.to_string()))?;
            cmyk_tiff_layout(&mut decoder, path).map(|_| ())
        }
        CmykContainer::Jpeg => {
            let (w, h) = cmyk_jpeg::cmyk_dimensions(bytes)
                .ok_or_else(|| fail("the JPEG is not four-component CMYK or YCCK".into()))?;
            let pixels = u64::from(w) * u64::from(h);
            if pixels == 0 || pixels > MAX_PIXELS {
                return Err(fail(format!(
                    "a {w} x {h} image is outside the {MAX_PIXELS}-pixel limit"
                )));
            }
            Ok(())
        }
    }
}

/// Whether [`cmyk_gate`] admits these bytes — header work only, no pixels
/// decoded. The import report's question (`dr_image::embedded_profile_report`).
pub(crate) fn cmyk_convertible(bytes: &[u8], container: CmykContainer) -> bool {
    cmyk_gate(bytes, "the file", container).is_ok()
}

/// Decodes a CMYK TIFF's own samples through THE reader
/// (`cmyk_tiff_read`) and converts them through `profile` to sRGB RGBA8, its
/// transparency (when it has one) as the RGBA8 alpha `round(a / 257)`, in
/// STORED orientation — the caller orients. Only what [`cmyk_tiff_layout`]
/// admits is converted; anything else is an `Err`, so the host's platform
/// fallback keeps reading it.
pub(crate) fn cmyk_tiff_to_srgb8(
    bytes: &[u8],
    path: &str,
    profile: &ProofProfile,
) -> Result<RgbaImage, String> {
    let fail = |what: String| format!("failed to decode {path}: {what}");
    let mut decoder = Decoder::new(Cursor::new(bytes)).map_err(|e| fail(e.to_string()))?;
    let layout = cmyk_tiff_layout(&mut decoder, path)?;
    let samples = crate::cmyk_tiff_read::read_samples(bytes, path, &layout)?;
    let mut convert = srgb8_converter(profile, path)?;
    let mut out = RgbaImage::new(layout.width, layout.height);
    for (dst, s) in out.pixels_mut().zip(&samples) {
        let alpha = ((u32::from(s[4]) + 128) / 257) as u8;
        // Keys and device values from the file's own codes, exactly as
        // before the reader was shared: 8-bit codes for an 8-bit file.
        let rgb = if layout.bytes == 1 {
            let codes: [u8; 4] = std::array::from_fn(|i| (s[i] / 257) as u8);
            convert(
                ink8_key(&codes),
                std::array::from_fn(|i| f64::from(codes[i]) / 255.0),
            )
        } else {
            let key = s[..4]
                .iter()
                .fold(0u64, |acc, &sample| (acc << 16) | u64::from(sample));
            convert(key, std::array::from_fn(|i| f64::from(s[i]) / 65535.0))
        };
        dst.0 = [rgb[0], rgb[1], rgb[2], alpha];
    }
    Ok(out)
}

/// Decodes a CMYK or YCCK JPEG's own planes as ink (`cmyk_jpeg::ink_samples`;
/// `adobe` = the file carries an Adobe APP14) and converts them through
/// `profile` exactly as [`cmyk_tiff_to_srgb8`] converts 8-bit samples, in
/// STORED orientation — the caller orients.
pub(crate) fn cmyk_jpeg_to_srgb8(
    bytes: &[u8],
    path: &str,
    profile: &ProofProfile,
    adobe: bool,
) -> Result<RgbaImage, String> {
    let (w, h, ink) = cmyk_jpeg::ink_samples(bytes, path, adobe)?;
    let mut convert = srgb8_converter(profile, path)?;
    let mut out = RgbaImage::new(w, h);
    for (dst, s) in out.pixels_mut().zip(ink.chunks_exact(4)) {
        let rgb = convert(
            ink8_key(s),
            std::array::from_fn(|i| f64::from(s[i]) / 255.0),
        );
        dst.0 = [rgb[0], rgb[1], rgb[2], 255];
    }
    Ok(out)
}

/// The memo key of four 8-bit samples.
fn ink8_key(s: &[u8]) -> u64 {
    u64::from(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// THE per-pixel CMYK → sRGB8 conversion both containers run: normalized
/// samples through `ProofProfile::to_xyz` at relative colorimetric, the BPC
/// affine (module doc), `DocumentPcs::from_xyz` into the built-in sRGB,
/// `round(255 · clamp)`. Memoized per distinct sample tuple (the caller's
/// `key`), and the memo stops GROWING at `soft_proof`'s `MEMO_CAPACITY`, the
/// `Proofer::pixel` rule — a scan repeats colours heavily, a synthetic
/// gradient must not become a huge table.
fn srgb8_converter<'a>(
    profile: &'a ProofProfile,
    path: &str,
) -> Result<impl FnMut(u64, [f64; 4]) -> [u8; 3] + 'a, String> {
    let srgb = IccProfile::srgb();
    // The CMYK → RGB leg Image ▸ Mode ▸ RGB Color runs too
    // (`cmyk_color::CmykToRgb`), at relative colorimetric with BPC into the
    // built-in sRGB. A profile without a relative table cannot reach here
    // (`decide` asks for one), so a refusal is the PCS bridge's.
    let leg = CmykToRgb::new(profile, &srgb, ConversionOptions::default())
        .map_err(|e| format!("failed to decode {path}: {e}"))?;
    let mut memo: HashMap<u64, [u8; 3]> = HashMap::new();
    Ok(move |key: u64, device: [f64; 4]| -> [u8; 3] {
        if let Some(hit) = memo.get(&key) {
            return *hit;
        }
        let value = leg
            .evaluate(device)
            .map(|c| (255.0 * f64::from(c).clamp(0.0, 1.0)).round() as u8);
        if memo.len() < MEMO_CAPACITY {
            memo.insert(key, value);
        }
        value
    })
}
