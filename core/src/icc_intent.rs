//! Rendering intents, black-point compensation, and the general source →
//! destination [`ColorPipeline`] every Phase 11C conversion and proof runs
//! through. The profile models are `icc`/`icc_transform` (matrix/TRC) and
//! `icc_lut` (A2B/B2A); this module is the ARITHMETIC that joins two of them.
//!
//! # Which table an intent uses, and what happens when it is absent
//!
//! | intent | table | fallback |
//! |---|---|---|
//! | perceptual | `A2B0` / `B2A0` | — (table 0 is the required one) |
//! | relative colorimetric | `A2B1` / `B2A1` | `A2B0` / `B2A0` |
//! | saturation | `A2B2` / `B2A2` | `A2B0` / `B2A0` |
//! | absolute colorimetric | the RELATIVE pair | plus media-white scaling |
//!
//! Falling back to table 0 rather than refusing is what every shipping CMM
//! does and what ICC requires of a reader: `A2B0` is the only table a
//! profile MUST carry, and a profile that ships one table means it for every
//! intent. A matrix/TRC profile has no tables at all: every intent is the
//! matrix path, and absolute is the matrix path plus the scaling.
//!
//! **Absolute colorimetric** is relative colorimetric with the media white
//! put back: the source's PCS values are un-normalized by its own media
//! white and re-normalized by the destination's, componentwise in XYZ
//! (`XYZ' = XYZ · MW_src / MW_dst`). That is the ICC v2/v4 equation, and it
//! is why an absolute proof shows the paper's cast instead of pretending the
//! paper is white.
//!
//! # Black-point compensation
//!
//! ISO 18619's affine map in XYZ, per component:
//!
//! ```text
//! XYZ' = a · XYZ + b
//! a = (D50 − BP_dst) / (D50 − BP_src)
//! b = D50 · (BP_dst − BP_src) / (D50 − BP_src)
//! ```
//!
//! so `BP_src ↦ BP_dst` and `D50 ↦ D50`: the darkest colour the source can
//! show lands on the darkest colour the destination can show, and white does
//! not move. Without it, every tone below the destination's own black
//! collapses onto it — the blocked shadows that make an uncompensated
//! print-space conversion obvious.
//!
//! The black points themselves are `icc_black_point`'s: the SOURCE side
//! follows Little CMS's `cmsDetectBlackPoint` and the
//! DESTINATION side its `cmsDetectDestinationBlackPoint`, the pair
//! lcms's `ComputeConversion` asks of every compensated step.

//! # The fast path is the shipped path
//!
//! When both sides are matrix/TRC, the intent is not absolute and BPC is
//! off, [`ColorPipeline`] holds the EXISTING [`Transform`] and
//! [`PrecisionTransform`] and calls them. That is not an optimization, it is
//! the byte-identity guarantee: today's conversions run today's code, and
//! `conversion_policy_tests` measures it rather than assuming it.
//!
//! `icc_intent` owns `RenderingIntent`, the per-intent table selection and its A2B0/B2A0 fallback,
//! absolute's componentwise media-white scaling, ISO 18619's BPC arithmetic (`bpc_terms`; the black
//! points are `icc_black_point`'s) — and `ColorPipeline`. **`ColorPipeline`'s fast path IS the
//! shipped path**: two matrix/TRC profiles at a non-absolute intent with BPC off hold the existing
//! `Transform`/`PrecisionTransform` and call them, which is what makes "the default conversion is
//! byte-identical" a measured claim (`color_intent_tests`).

use std::sync::Arc;

use crate::icc::IccProfile;
use crate::icc_black_point::{black_point, destination_black_point};
use crate::icc_lut::{self, ProofProfile, D50};
use crate::icc_transform::{MatrixTrc, PrecisionToneEncoder, PrecisionTransform, Transform};

/// A black point this dark is indistinguishable from zero and would make the
/// BPC denominator `D50 − BP_src` differ from `D50` by less than the tables'
/// own resolution; below it, BPC is the identity on that side.
const BLACK_EPSILON: f64 = 1e-9;

/// The four ICC rendering intents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderingIntent {
    Perceptual,
    RelativeColorimetric,
    Saturation,
    AbsoluteColorimetric,
}

impl RenderingIntent {
    /// The JSON/MCP spelling.
    pub fn name(self) -> &'static str {
        match self {
            RenderingIntent::Perceptual => "perceptual",
            RenderingIntent::RelativeColorimetric => "relative",
            RenderingIntent::Saturation => "saturation",
            RenderingIntent::AbsoluteColorimetric => "absolute",
        }
    }

    /// `None` for a spelling outside the four.
    pub fn parse(text: &str) -> Option<RenderingIntent> {
        match text {
            "perceptual" => Some(RenderingIntent::Perceptual),
            "relative" => Some(RenderingIntent::RelativeColorimetric),
            "saturation" => Some(RenderingIntent::Saturation),
            "absolute" => Some(RenderingIntent::AbsoluteColorimetric),
            _ => None,
        }
    }

    /// The `DR_INTENT_*` discriminant; mapped, never transmuted.
    pub fn from_c(value: i32) -> Option<RenderingIntent> {
        match value {
            0 => Some(RenderingIntent::Perceptual),
            1 => Some(RenderingIntent::RelativeColorimetric),
            2 => Some(RenderingIntent::Saturation),
            3 => Some(RenderingIntent::AbsoluteColorimetric),
            _ => None,
        }
    }

    pub fn to_c(self) -> i32 {
        match self {
            RenderingIntent::Perceptual => 0,
            RenderingIntent::RelativeColorimetric => 1,
            RenderingIntent::Saturation => 2,
            RenderingIntent::AbsoluteColorimetric => 3,
        }
    }

    /// Which of the three table slots this intent asks for first (the module
    /// doc's table). Absolute shares relative's.
    pub fn table(self) -> usize {
        match self {
            RenderingIntent::Perceptual => 0,
            RenderingIntent::RelativeColorimetric | RenderingIntent::AbsoluteColorimetric => 1,
            RenderingIntent::Saturation => 2,
        }
    }

    /// True for the one intent that also scales by the two media whites.
    pub fn is_absolute(self) -> bool {
        self == RenderingIntent::AbsoluteColorimetric
    }
}

impl Default for RenderingIntent {
    /// Relative colorimetric — today's behaviour, and every default in this
    /// phase.
    fn default() -> Self {
        RenderingIntent::RelativeColorimetric
    }
}

// ------------------------------------------------------------------ BPC --

/// The ISO 18619 coefficients for one component pair (module doc).
pub fn bpc_terms(source: [f64; 3], destination: [f64; 3]) -> [(f64, f64); 3] {
    std::array::from_fn(|i| {
        let denominator = D50[i] - source[i];
        if denominator.abs() < BLACK_EPSILON {
            return (1.0, 0.0);
        }
        let a = (D50[i] - destination[i]) / denominator;
        let b = D50[i] * (destination[i] - source[i]) / denominator;
        (a, b)
    })
}

// ---------------------------------------------------------- the pipeline --

/// How one side turns pixels into PCS XYZ, or PCS XYZ back into pixels.
enum Side {
    Matrix(Box<MatrixTrc>),
    Lut(Arc<ProofProfile>),
}

/// The general path: decode with the source, move the XYZ, encode with the
/// destination.
struct General {
    source: Side,
    destination: Side,
    intent: RenderingIntent,
    /// Componentwise `MW_src / MW_dst`, `[1, 1, 1]` for every intent but
    /// absolute.
    white: [f64; 3],
    /// The ISO 18619 `(a, b)` per component, `None` when BPC is off.
    bpc: Option<[(f64, f64); 3]>,
    /// The destination's CONTINUOUS encoder, present only on the matrix
    /// destination. There is deliberately no byte twin: the general path
    /// encodes continuously at BOTH depths, because a `PrecisionToneEncoder`
    /// exists for exactly the destinations that have a matrix model, so a
    /// threshold `ToneEncoder` beside it could never be selected. The
    /// shipped byte thresholds still own every 8-bit matrix->matrix
    /// conversion: those take the fast path and never reach here.
    typed_encoder: Option<PrecisionToneEncoder>,
}

/// A prepared conversion from one profile to another under an intent and a
/// BPC flag. See the module doc for the fast path.
pub struct ColorPipeline {
    fast: Option<(Option<Transform>, PrecisionTransform)>,
    general: Option<General>,
    identity: bool,
    scratch: u64,
}

impl ColorPipeline {
    /// Prepares the conversion. `Err` names what made it impossible — an
    /// unreadable source, a destination that cannot be encoded into, or a
    /// destination that is not RGB.
    pub fn new(
        src: &IccProfile,
        dst: &IccProfile,
        intent: RenderingIntent,
        bpc: bool,
    ) -> Result<Self, String> {
        // The fast path IS the shipped path: same inputs, same code, same
        // bytes. Anything that changes the arithmetic (absolute's white
        // scaling, BPC, a LUT on either side) leaves it.
        if !intent.is_absolute() && !bpc {
            if let (Some(source), Some(destination)) = (src.model(), dst.model()) {
                let byte = Transform::between(source, destination);
                let identity = byte.is_none();
                let typed = PrecisionTransform::new(source, destination)?;
                let scratch = PrecisionTransform::prepare_scratch_bytes(source, destination)
                    + std::mem::size_of::<Transform>() as u64;
                return Ok(ColorPipeline {
                    fast: Some((byte, typed)),
                    general: None,
                    identity,
                    scratch,
                });
            }
        }
        let source = side(src, "source")?;
        let destination = side(dst, "destination")?;
        if let Side::Lut(lut) = &destination {
            if !lut.can_encode() {
                return Err(format!(
                    "the destination profile \"{}\" carries no PCS to device table",
                    lut.name()
                ));
            }
            if lut.device() != icc_lut::DeviceSpace::Rgb {
                return Err(format!(
                    "a {} destination profile cannot hold an RGB document",
                    lut.device().name()
                ));
            }
        }
        if let Side::Lut(lut) = &source {
            if !lut.can_decode() {
                return Err(format!(
                    "the source profile \"{}\" carries no device to PCS table",
                    lut.name()
                ));
            }
        }
        let white = if intent.is_absolute() {
            let source_white = media_white(src);
            let destination_white = media_white(dst);
            std::array::from_fn(|i| {
                if destination_white[i].abs() < BLACK_EPSILON {
                    1.0
                } else {
                    source_white[i] / destination_white[i]
                }
            })
        } else {
            [1.0, 1.0, 1.0]
        };
        let bpc = bpc
            .then(|| {
                let source_black = black_point(src, intent)?;
                let destination_black = destination_black_point(dst, intent)?;
                Some(bpc_terms(source_black, destination_black))
            })
            .flatten();
        let typed_encoder = dst.model().map(PrecisionToneEncoder::new).transpose()?;
        let scratch = side_bytes(&source) + side_bytes(&destination) + 8192;
        Ok(ColorPipeline {
            fast: None,
            general: Some(General {
                source,
                destination,
                intent,
                white,
                bpc,
                typed_encoder,
            }),
            identity: false,
            scratch,
        })
    }

    /// True when running this pipeline would change nothing — the
    /// `MatrixTrc::approx_eq` rule, asked once so a document op can refuse
    /// instead of registering a phantom undo step.
    pub fn is_identity(&self) -> bool {
        self.identity
    }

    /// True when this pipeline runs the SHIPPED transforms (module doc, "The
    /// fast path is the shipped path"): both sides matrix/TRC, not absolute,
    /// no BPC — so its bytes are the default conversion's.
    pub fn is_shipped(&self) -> bool {
        self.fast.is_some()
    }

    /// Conservative bytes the prepared tables hold, for job admission.
    pub fn scratch_bytes(&self) -> u64 {
        self.scratch
    }

    /// The heap the pipeline actually holds, never [`Self::scratch_bytes`]'
    /// reservation: each `Side::Matrix` box and its sampled curves, each
    /// `Side::Lut` Arc (by address, its tables' decoded size), the general
    /// path's typed encoder and the fast path's precision tables. The byte
    /// `Transform` is inline in the pipeline, its owner's to count. A Lab
    /// document's prepared colour adjustment records its two pipelines here
    /// (`adjust_precision`, #547 phase 2c).
    pub(crate) fn record_resources(&self, inventory: &mut crate::resources::Inventory) {
        if let Some((_, typed)) = &self.fast {
            typed.record_resources(inventory);
        }
        let Some(general) = &self.general else {
            return;
        };
        for side in [&general.source, &general.destination] {
            match side {
                Side::Matrix(model) => {
                    inventory.object(&**model, "profile", false);
                    model.record_resources(inventory);
                }
                Side::Lut(lut) => inventory.record(
                    Arc::as_ptr(lut) as usize,
                    lut.bytes() as usize,
                    "profile",
                    false,
                ),
            }
        }
        if let Some(encoder) = &general.typed_encoder {
            encoder.record_resources(inventory);
        }
    }

    /// One straight RGB byte triple converted. Alpha is the caller's.
    pub fn color8(&self, rgb: [u8; 3]) -> [u8; 3] {
        if let Some((byte, _)) = &self.fast {
            return match byte {
                Some(transform) => transform.color(rgb),
                None => rgb,
            };
        }
        let out = self.color(rgb.map(|v| f32::from(v) / 255.0));
        // The destination's own encoder already rounded to its grid when it
        // is a matrix profile; a LUT destination comes back continuous, so
        // one round is the whole quantization.
        std::array::from_fn(|c| (out[c].clamp(0.0, 1.0) * 255.0).round() as u8)
    }

    /// One straight RGB triple in [0, 1] converted.
    pub fn color(&self, rgb: [f32; 3]) -> [f32; 3] {
        if let Some((_, typed)) = &self.fast {
            return typed.color(rgb);
        }
        let Some(general) = &self.general else {
            return rgb;
        };
        general.color(rgb)
    }

    /// One straight RGB triple in [0, 1] as the DESTINATION's linear light
    /// (its matrix primaries, no transfer curve, no clamp): the general path's
    /// decode, white scaling and BPC, then `MatrixTrc::from_pcs_linear`.
    /// `None` on the fast (matrix/matrix) path or for a LUT destination.
    pub(crate) fn linear(&self, rgb: [f32; 3]) -> Option<[f64; 3]> {
        let general = self.general.as_ref()?;
        let Side::Matrix(model) = &general.destination else {
            return None;
        };
        // `General::color`'s steps up to its encode (the master copy), kept
        // in f64 so the caller rounds once. A source table that cannot
        // answer is black, as it is there.
        let input = rgb.map(|v| f64::from(v).clamp(0.0, 1.0));
        let Some(mut xyz) = decode(&general.source, input, general.intent) else {
            return Some([0.0, 0.0, 0.0]);
        };
        for (slot, scale) in xyz.iter_mut().zip(general.white) {
            *slot *= scale;
        }
        if let Some(terms) = &general.bpc {
            for (slot, (a, b)) in xyz.iter_mut().zip(terms.iter()) {
                *slot = a * *slot + b;
            }
        }
        Some(model.from_pcs_linear(xyz))
    }
}

impl General {
    fn color(&self, rgb: [f32; 3]) -> [f32; 3] {
        let input = rgb.map(|v| f64::from(v).clamp(0.0, 1.0));
        let Some(mut xyz) = decode(&self.source, input, self.intent) else {
            return [0.0, 0.0, 0.0];
        };
        for (slot, scale) in xyz.iter_mut().zip(self.white) {
            *slot *= scale;
        }
        if let Some(terms) = &self.bpc {
            for (slot, (a, b)) in xyz.iter_mut().zip(terms.iter()) {
                *slot = a * *slot + b;
            }
        }
        encode(
            &self.destination,
            self.typed_encoder.as_ref(),
            xyz,
            self.intent,
        )
    }
}

/// One side's device values in [0, 1] as PCS XYZ (D50) — the decode half of
/// [`General::color`] and of [`DocumentPcs`]. `None` when a LUT side has no
/// table for `intent`.
fn decode(side: &Side, input: [f64; 3], intent: RenderingIntent) -> Option<[f64; 3]> {
    match side {
        Side::Matrix(model) => Some(model.to_pcs_xyz64(input)),
        Side::Lut(lut) => lut.to_xyz(&input, intent),
    }
}

/// PCS XYZ (D50) as one side's device values — the encode half of
/// [`General::color`] and of [`DocumentPcs`]. `encoder` is the matrix side's
/// continuous encoder; black when a side cannot encode.
fn encode(
    side: &Side,
    encoder: Option<&PrecisionToneEncoder>,
    xyz: [f64; 3],
    intent: RenderingIntent,
) -> [f32; 3] {
    match side {
        Side::Matrix(model) => {
            let linear = model.from_pcs_linear(xyz);
            match encoder {
                Some(encoder) => encoder.rgb(linear),
                None => [0.0, 0.0, 0.0],
            }
        }
        Side::Lut(lut) => match lut.from_xyz(xyz, intent) {
            Some((device, _)) => std::array::from_fn(|c| device[c] as f32),
            None => [0.0, 0.0, 0.0],
        },
    }
}

/// A DOCUMENT profile as a two-way bridge to PCS XYZ, relative colorimetric:
/// the same decode and encode [`General::color`] runs, with no second
/// profile on the other side. The Color Lookup profile reader
/// (`icc_lookup`) runs an ABSTRACT profile's Lab→Lab table between the two
/// halves — RGB → PCS → abstract → PCS → RGB.
pub(crate) struct DocumentPcs {
    side: Side,
    encoder: Option<PrecisionToneEncoder>,
}

impl DocumentPcs {
    /// `Err` names why the profile cannot go both ways: neither matrix/TRC
    /// nor a LUT profile this build reads, or a LUT without one direction.
    pub(crate) fn new(profile: &IccProfile) -> Result<Self, String> {
        let side = side(profile, "document")?;
        if let Side::Lut(lut) = &side {
            if !lut.can_decode() || !lut.can_encode() {
                return Err(format!(
                    "the document profile \"{}\" cannot be converted both to and from PCS",
                    lut.name()
                ));
            }
        }
        let encoder = profile.model().map(PrecisionToneEncoder::new).transpose()?;
        Ok(DocumentPcs { side, encoder })
    }

    /// Document RGB in [0, 1] as PCS XYZ (D50); black when the profile's
    /// table cannot answer.
    pub(crate) fn to_xyz(&self, rgb: [f64; 3]) -> [f64; 3] {
        let input = rgb.map(|v| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                0.0
            }
        });
        decode(&self.side, input, RenderingIntent::RelativeColorimetric).unwrap_or([0.0; 3])
    }

    /// PCS XYZ (D50) as document RGB in [0, 1]. Named as the twin of
    /// `ProofProfile::from_xyz`, a conversion OUT of this profile's space.
    #[allow(clippy::wrong_self_convention)]
    pub(crate) fn from_xyz(&self, xyz: [f64; 3]) -> [f32; 3] {
        encode(
            &self.side,
            self.encoder.as_ref(),
            xyz,
            RenderingIntent::RelativeColorimetric,
        )
    }
}

fn side(profile: &IccProfile, role: &str) -> Result<Side, String> {
    if let Some(model) = profile.model() {
        return Ok(Side::Matrix(Box::new(model.clone())));
    }
    match profile.lut() {
        Some(lut) => Ok(Side::Lut(lut.clone())),
        None => Err(format!(
            "the {role} profile \"{}\" is neither a matrix/TRC nor a LUT profile this build can read",
            profile.name()
        )),
    }
}

fn side_bytes(side: &Side) -> u64 {
    match side {
        Side::Matrix(_) => PrecisionToneEncoder::scratch_bytes(),
        Side::Lut(lut) => lut.bytes(),
    }
}

/// A profile's media white: the LUT model's `wtpt`, or D50 for a matrix/TRC
/// profile, whose PCS white is D50 by definition (`icc`'s PCS white rule).
pub fn media_white(profile: &IccProfile) -> [f64; 3] {
    match profile.lut() {
        Some(lut) => lut.media_white(),
        None => profile.model().map(MatrixTrc::media_white).unwrap_or(D50),
    }
}
