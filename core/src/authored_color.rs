//! The ONE sRGB ↔ document decision for AUTHORED colours: a layer
//! style's colours, a type or shape layer's ink, high-depth `srgb` paint and
//! the colours a PSD `lfx2` / `phfl` record carries are authored in sRGB and
//! convert into the document's space exactly once, where they are drawn.
//!
//! - A document whose profile has a matrix/TRC model converts through the
//!   shipped byte [`Transform`] (`AuthoredColors::Matrix`), byte for byte what
//!   it did before this module existed; sRGB itself is `None` (nothing to do).
//! - A document tagged with an RGB LUT profile converts through
//!   `retained_source_render::lut_pipeline` — relative colorimetric, BPC off,
//!   the one choice every source render makes — but ONLY when that pipeline
//!   builds in BOTH directions (sRGB → profile and profile → sRGB). Photoshop
//!   2025 converts effect, type and shape colours across Convert to Profile
//!   into such a profile and stores them in the document's numbers (the
//!   `scripts/photoshop-probe/lut_colours.py` take, verdict B-APP), so the
//!   authored colour keeps its appearance there as it does on a matrix
//!   profile.
//! - A LUT profile that cannot convert both ways (no B2A table, truncated or
//!   unreadable tables) keeps the authored NUMBERS in both directions, on the
//!   composite, on PSD export and on PSD import alike. Requiring both
//!   directions is what keeps those three in agreement: a decode-only profile
//!   would otherwise convert an imported colour out of the document but never
//!   back in.
//!
//! The host asks the same question through `dr_authored_color`
//! (`ffi_authored_color`), so its brush, fill, gradient and text ink and the
//! core's styles and artwork land on the same numbers.
//!
//! WIDE colours: a layer-style colour slot may hold its colour as
//! LINEAR-LIGHT sRGB-primaries components beyond the byte grid and beyond
//! sRGB's gamut (`style`'s WIDE COLOURS). [`wide_into`] carries one into a
//! document's numbers — on a matrix/TRC document through the same combined
//! matrix ([`LinearInk::undecoded`]) and then the byte encoder (an 8-bit
//! scene) or the high-depth [`PrecisionToneEncoder`] (a 16-bit scene, built
//! on first use) — and [`wide_out`] reads a document's numbers back as one.
//! A LUT profile goes through its pipeline from the clamped sRGB numbers, and
//! sRGB itself through the sRGB curves alone.
//!
//! A Lab document's twin profile converts as a LUT profile does
//! (`AuthoredColors::LabTwin`), and its pattern texels are authored sRGB
//! too: [`AuthoredColors::converts_tiles`] tells the style renderer to turn
//! them into Lab codes (`lab_style::painted_sampler`).

use std::sync::OnceLock;

use crate::adjust_math::{linear_to_srgb, srgb_to_linear};
use crate::icc::IccProfile;
use crate::icc_intent::ColorPipeline;
use crate::icc_transform::{LinearInk, MatrixTrc, PrecisionToneEncoder, ToneEncoder, Transform};
use crate::retained_source_render::lut_pipeline;

/// The largest magnitude a wide colour component may hold (`style_json`
/// clamps into `[-LINEAR_LIMIT, LINEAR_LIMIT]`): four times diffuse white,
/// past every RGB working space's primaries in sRGB's linear components.
pub(crate) const LINEAR_LIMIT: f32 = 4.0;

/// How far, in BYTE codes, a wide colour's sRGB numbers may sit from the byte
/// grid and still be that byte ([`hex_equivalent`]). Photoshop stores an
/// effect colour on a 15-bit scale, so its `RGBC` for a byte lands up to half
/// a 15-bit step (255 / 65536 ≈ 0.0039 codes) off it — `blend_if/source.psd`
/// writes 129 as 129.0019 and 215 as 215.0006 — and a byte's own linear value
/// carried through f32 curves both ways lands within about 3e-5. A deliberate
/// pick between two codes is a tenth of a code or more away.
pub(crate) const HEX_TOLERANCE: f32 = 5e-3;

/// A matrix/TRC conversion's wide half (module doc): the combined matrix,
/// the destination's byte encoder and, built on first use, its high-depth
/// encoder.
pub(crate) struct WideInk {
    ink: LinearInk,
    bytes: ToneEncoder,
    destination: MatrixTrc,
    encoder: OnceLock<Option<PrecisionToneEncoder>>,
}

impl WideInk {
    fn new(source: &MatrixTrc, destination: &MatrixTrc) -> WideInk {
        WideInk {
            ink: LinearInk::new(source, destination),
            bytes: ToneEncoder::new(destination),
            destination: destination.clone(),
            encoder: OnceLock::new(),
        }
    }
}

/// One authored-colour conversion between sRGB and a document's space,
/// prepared once and applied per colour. Both variants are tables of about
/// 12 KB held once per projection or export, so neither is boxed.
#[allow(clippy::large_enum_variant)]
pub(crate) enum AuthoredColors {
    /// Both sides have a matrix/TRC model: the shipped byte transform, and
    /// its wide half (module doc).
    Matrix(Transform, WideInk),
    /// The document's profile is an RGB LUT profile that converts both ways.
    Lut(ColorPipeline),
    /// The document's profile is the Lab twin (`lab_document::is_lab_twin`):
    /// converts as [`AuthoredColors::Lut`] does, and its pattern tiles are
    /// authored sRGB too ([`AuthoredColors::converts_tiles`]).
    LabTwin(ColorPipeline),
}

impl AuthoredColors {
    /// sRGB → `profile`; `None` when `profile` is sRGB (nothing to do) or a
    /// profile with no model that cannot convert both ways (the numbers stay).
    pub(crate) fn into_document(profile: &IccProfile) -> Option<AuthoredColors> {
        let srgb = IccProfile::srgb();
        match (srgb.model(), profile.model()) {
            (Some(from), Some(to)) => Transform::between(from, to)
                .map(|transform| AuthoredColors::Matrix(transform, WideInk::new(from, to))),
            _ if crate::lab_document::is_lab_twin(profile) => {
                lut_ink(profile, true).map(AuthoredColors::LabTwin)
            }
            _ => lut_ink(profile, true).map(AuthoredColors::Lut),
        }
    }

    /// True only for [`AuthoredColors::LabTwin`]: a Lab document converts
    /// its pattern texels from sRGB (module doc); every other document keeps
    /// a tile's values as they are.
    pub(crate) fn converts_tiles(&self) -> bool {
        matches!(self, AuthoredColors::LabTwin(_))
    }

    /// `profile` → sRGB, on the same terms as [`AuthoredColors::into_document`].
    pub(crate) fn out_of_document(profile: &IccProfile) -> Option<AuthoredColors> {
        let srgb = IccProfile::srgb();
        match (profile.model(), srgb.model()) {
            (Some(from), Some(to)) => Transform::between(from, to)
                .map(|transform| AuthoredColors::Matrix(transform, WideInk::new(from, to))),
            _ => lut_ink(profile, false).map(AuthoredColors::Lut),
        }
    }

    /// One straight RGB byte triple converted.
    pub(crate) fn color(&self, rgb: [u8; 3]) -> [u8; 3] {
        match self {
            AuthoredColors::Matrix(transform, _) => transform.color(rgb),
            AuthoredColors::Lut(pipeline) | AuthoredColors::LabTwin(pipeline) => {
                pipeline.color8(rgb)
            }
        }
    }

    /// A wide colour's linear sRGB components (an `into_document`
    /// conversion) as the document's numbers in 0…1: on the byte grid unless
    /// `deep`, where the high-depth encoder answers (the byte one when that
    /// cannot build for this profile). A LUT profile converts the clamped
    /// sRGB numbers through its pipeline.
    pub(crate) fn wide(&self, linear: [f32; 3], deep: bool) -> [f32; 3] {
        match self {
            AuthoredColors::Matrix(_, wide) => {
                let document = wide.ink.undecoded(linear);
                if deep {
                    let encoder = wide
                        .encoder
                        .get_or_init(|| PrecisionToneEncoder::new(&wide.destination).ok());
                    if let Some(encoder) = encoder {
                        return encoder.rgb(document);
                    }
                }
                wide.bytes.rgb(document)
            }
            AuthoredColors::Lut(pipeline) | AuthoredColors::LabTwin(pipeline) => pipeline
                .color(srgb_numbers(linear))
                .map(|v| if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }),
        }
    }

    /// A document's numbers in 0…1 (an `out_of_document` conversion) as
    /// linear sRGB components, extended range.
    pub(crate) fn linear_of(&self, numbers: [f32; 3]) -> [f32; 3] {
        match self {
            AuthoredColors::Matrix(_, wide) => wide.ink.linear(numbers).map(|v| v as f32),
            AuthoredColors::Lut(pipeline) | AuthoredColors::LabTwin(pipeline) => {
                srgb_linear(pipeline.color(numbers))
            }
        }
    }
}

/// [`AuthoredColors::wide`] under an optional conversion: `None` (sRGB, or a
/// profile that keeps the authored numbers) is [`srgb_numbers`].
pub(crate) fn wide_into(colors: Option<&AuthoredColors>, linear: [f32; 3], deep: bool) -> [f32; 3] {
    colors.map_or_else(|| srgb_numbers(linear), |t| t.wide(linear, deep))
}

/// [`AuthoredColors::linear_of`] under an optional conversion: `None` is
/// [`srgb_linear`].
pub(crate) fn wide_out(colors: Option<&AuthoredColors>, numbers: [f32; 3]) -> [f32; 3] {
    colors.map_or_else(|| srgb_linear(numbers), |t| t.linear_of(numbers))
}

/// Linear sRGB components as sRGB numbers in 0…1 (the sRGB OETF, each
/// component clamped into 0…1 first).
pub(crate) fn srgb_numbers(linear: [f32; 3]) -> [f32; 3] {
    linear.map(|v| if v.is_nan() { 0.0 } else { linear_to_srgb(v) })
}

/// sRGB numbers as linear sRGB components (the sRGB EOTF).
pub(crate) fn srgb_linear(numbers: [f32; 3]) -> [f32; 3] {
    numbers.map(srgb_to_linear)
}

/// The sRGB byte a wide colour IS, when it is one: every component in 0…1
/// and its sRGB numbers each within [`HEX_TOLERANCE`] of a byte code. `None`
/// for a colour off the grid or outside sRGB's gamut.
pub(crate) fn hex_equivalent(linear: [f32; 3]) -> Option<[u8; 3]> {
    if !linear.iter().all(|v| (0.0..=1.0).contains(v)) {
        return None;
    }
    let mut out = [0u8; 3];
    for (slot, n) in out.iter_mut().zip(srgb_numbers(linear)) {
        let code = n * 255.0;
        let byte = code.round();
        if (code - byte).abs() > HEX_TOLERANCE {
            return None;
        }
        *slot = byte as u8;
    }
    Some(out)
}

/// The scratch a 16-bit scene's wide colour may build: one high-depth
/// encoder ([`AuthoredColors::wide`]).
pub(crate) fn wide_scratch_bytes() -> u64 {
    PrecisionToneEncoder::scratch_bytes()
}

/// The LUT pipeline an authored colour takes on a document tagged with
/// `profile`: `to_document` true is sRGB → profile, false profile → sRGB.
/// `None` when `profile` has a matrix/TRC model (the [`Transform`] path owns
/// it) or when `lut_pipeline` does not build in BOTH directions (module doc).
pub(crate) fn lut_ink(profile: &IccProfile, to_document: bool) -> Option<ColorPipeline> {
    if profile.model().is_some() {
        return None;
    }
    let srgb = IccProfile::srgb();
    let into = lut_pipeline(&srgb, profile).ok()??;
    let out = lut_pipeline(profile, &srgb).ok()??;
    Some(if to_document { into } else { out })
}

/// One authored straight RGB triple in 0…1 (clamped) converted between sRGB
/// and the LUT profile `profile` ([`lut_ink`]), clamped to 0…1 the way
/// `ColorPipeline::color8` clamps before it rounds; `None` exactly when
/// [`lut_ink`] is.
pub fn authored_color(profile: &IccProfile, to_document: bool, rgb: [f32; 3]) -> Option<[f32; 3]> {
    let pipeline = lut_ink(profile, to_document)?;
    let clamped = rgb.map(|v| if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) });
    Some(pipeline.color(clamped).map(|v| v.clamp(0.0, 1.0)))
}
