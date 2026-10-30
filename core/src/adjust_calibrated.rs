//! Versioned document-context Exposure and White Balance evaluation.
//!
//! `params.evaluation = "document_v2"` binds a document's matrix/TRC profile.
//!
//! **Exposure runs per channel in the document's own primaries**, at each
//! channel's pure exponent γ (`MatrixTrc::exposure_domain`, an
//! [`ExposureDomain`]: rule Γ in a profile that passes the precision check —
//! 1 for a linear curve, the exponent of a pure power, 2.2 for sRGB-shaped,
//! sampled and offset curves — and, in one that fails it on a falling branch
//! boundary, the channel's class: ROMM's pure power 1.8, its toe ignored, and
//! ITU-709's offset curve 1.883): `x = v^γ`, then `Exposure::apply_linear`
//! (gain, pedestal, signed `1/gamma` power), then `clamp(u, 0, 1)^(1/γ)`,
//! rounded to the 8-bit grid on the byte path and left continuous for the
//! typed document's own quantization on the 16-bit one. No matrix, no TRC
//! and no encoder, which is why a profile whose curves cannot be encoded
//! into still admits Exposure. That is Photoshop's Exposure at 8 and 16
//! bits, fitted on its receipts and confirmed by fresh takes (#2,
//! `scripts/photoshop-probe/stroke_exposure_578_take.py` take1 and
//! `stroke_exposure_578_take2.py` take2): sRGB 8/16, Display P3 16, ROMM 8/16
//! and ITU-709 8/16 within 1.76 codes (the reference-basis kernel it
//! replaces missed by 16 to 218). On [0, 1] inputs "signed power, final
//! clip", `max(u, 0)` and the clamp after the pedestal are one rule; a clamp
//! after the gain is refuted (16 codes on `expA`). Persisted parameters keep
//! their meaning; only the evaluation changed.
//!
//! **White Balance** decodes the profile into a common linear sRGB reference
//! basis, evaluates its Bradford kernel, and encodes through that SAME
//! document profile (the [`Pipeline`], which only it binds), so a NEW or
//! EDITED White Balance still needs a profile that passes the precision
//! check (`calibrated_refusal`'s `op`). Intermediate reference values are not gamut-clipped: P3
//! colours outside sRGB remain expressible; only the final document encoding
//! clips. It reuses its one illuminant/matrix derivation.
//!
//! Missing evaluation, or `"legacy_srgb"`, keeps the shipped numeric kernel
//! exactly. New evaluation is explicit in native descriptions and Actions.
//! Both destructive and layer paths prepare this same pipeline once and encode
//! its result to the document's 8-bit grid before mask/opacity interpolation.
//! The default operation bypasses the round trip entirely, including inside a
//! float compositor; profile quantization cannot turn an identity into an edit.
//!
//! Scalar targets have no colour profile. Their existing opaque-grayscale
//! interpretation remains legacy and an explicit calibrated request refuses.

use serde_json::{Map, Value};

use crate::adjust::Adjustment;
use crate::adjust_tone::Exposure;
use crate::adjust_white_balance::WhiteBalance;
use crate::doc::{DrDocument, Layer, LayerKind};
use crate::icc::IccProfile;
use crate::icc_transform::{MatrixTrc, PrecisionToneEncoder, ToneEncoder};

pub(crate) const PROFILE_REFUSAL: &str =
    "calibrated Exposure and White Balance require an RGB matrix/TRC document profile";

/// Moved verbatim from Swift `CalibratedAdjustment.profileRefusal`.
pub(crate) const PROFILE_SENTENCE: &str = "Calibrated Exposure and White Balance require an RGB matrix/TRC document profile. Convert or assign a supported profile first.";

/// Why a NEW or EDITED document_v2 `op` ("exposure" | "white_balance"; any
/// other string answers as "white_balance") cannot evaluate in `profile`: no
/// matrix/TRC model → PROFILE_SENTENCE for both; a model failing
/// validate_precision → icc_encode_refusal::own_profile_sentence(profile.name())
/// for White Balance, and for Exposure only when `MatrixTrc::exposure_domain`
/// is `None` (repaired or unusable curves); else None. Depth-independent.
pub(crate) fn calibrated_refusal(profile: &IccProfile, op: &str) -> Option<String> {
    let Some(model) = profile.model() else {
        return Some(PROFILE_SENTENCE.to_owned());
    };
    let admitted = if op == "exposure" {
        model.exposure_domain().is_some()
    } else {
        model.validate_precision().is_ok()
    };
    (!admitted).then(|| crate::icc_encode_refusal::own_profile_sentence(profile.name()))
}

#[derive(Clone)]
pub(crate) enum CalibratedOp {
    Exposure(Exposure),
    WhiteBalance(WhiteBalance),
}

impl CalibratedOp {
    pub(crate) fn parse(op: &str, params: &Map<String, Value>) -> Option<Self> {
        match op {
            "exposure" => Exposure::parse(params).map(Self::Exposure),
            "white_balance" => WhiteBalance::parse(params).map(Self::WhiteBalance),
            _ => None,
        }
    }

    fn is_identity(&self) -> bool {
        match self {
            Self::Exposure(op) => op.is_identity(),
            Self::WhiteBalance(op) => op.is_identity(),
        }
    }

    fn linear(&self, rgb: [f64; 3]) -> [f64; 3] {
        match self {
            Self::Exposure(op) => op.apply_linear(rgb),
            Self::WhiteBalance(op) => op.apply_linear(rgb),
        }
    }
}

/// The parsed description is profile-free; only evaluation binds a document.
/// Its prepared variant is transient and never written into metadata. A
/// bound White Balance holds a [`Pipeline`], a bound Exposure an
/// [`ExposureDomain`].
#[derive(Clone)]
pub(crate) struct Calibrated {
    op: CalibratedOp,
    pipeline: Option<Box<Pipeline>>,
    domain: Option<ExposureDomain>,
}

/// White Balance's binding (module doc): the reference basis and the
/// profile's own encoder.
#[derive(Clone)]
struct Pipeline {
    source: MatrixTrc,
    to_reference: [[f64; 3]; 3],
    from_reference: [[f64; 3]; 3],
    encoder: Encoder,
}

/// Exposure's binding (module doc): the per-channel exponents and whether
/// the result lands on the 8-bit grid (the byte path) or stays continuous
/// (the typed one).
#[derive(Clone)]
struct ExposureDomain {
    gammas: [f64; 3],
    byte_grid: bool,
}

impl ExposureDomain {
    /// Exposure per channel at `gammas` (module doc). A non-finite input or
    /// result reads as 0.
    fn apply(&self, op: &Exposure, rgb: [f32; 3]) -> [f32; 3] {
        let finite = |v: f64| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        let linear: [f64; 3] =
            std::array::from_fn(|c| finite(f64::from(rgb[c])).powf(self.gammas[c]));
        let u = op.apply_linear(linear);
        std::array::from_fn(|c| {
            let v = finite(u[c]).powf(1.0 / self.gammas[c]);
            if self.byte_grid {
                ((v * 255.0).round() / 255.0) as f32
            } else {
                v as f32
            }
        })
    }
}

#[derive(Clone)]
// Keep the established fixed byte thresholds inline in the already shared
// Pipeline allocation; boxing them would add an allocation to every legacy op.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Encoder {
    Byte(ToneEncoder),
    Precision(PrecisionToneEncoder),
}
impl Encoder {
    pub(crate) fn rgb(&self, value: [f64; 3]) -> [f32; 3] {
        match self {
            Self::Byte(e) => e.rgb(value),
            Self::Precision(e) => e.rgb(value),
        }
    }
}

impl Calibrated {
    pub(crate) fn parse(op: &str, params: &Map<String, Value>) -> Option<Self> {
        Some(Self {
            op: CalibratedOp::parse(op, params)?,
            pipeline: None,
            domain: None,
        })
    }

    /// Binds the byte (8-bit) evaluation. White Balance keeps the unvalidated
    /// `ToneEncoder` on purpose, and Exposure falls back to rule Γ
    /// (`effective_gammas`) where `exposure_domain` is `None`: the byte render
    /// binds through `in_profile(..).ok()`, so a refusal here would silently
    /// drop an existing layer's effect instead of failing, and a saved
    /// document must keep its look. NEW or EDITED descriptions are refused up
    /// front by `calibrated_refusal`.
    fn prepare(mut self, profile: &IccProfile) -> Result<Self, String> {
        let model = profile.model().ok_or(PROFILE_REFUSAL)?;
        if let CalibratedOp::Exposure(_) = self.op {
            self.domain = Some(ExposureDomain {
                gammas: model
                    .exposure_domain()
                    .unwrap_or_else(|| model.effective_gammas()),
                byte_grid: true,
            });
            return Ok(self);
        }
        let reference = IccProfile::srgb();
        let reference = reference.model().ok_or(PROFILE_REFUSAL)?;
        self.pipeline = Some(Box::new(Pipeline {
            source: model.clone(),
            to_reference: model.combined_with(reference),
            from_reference: reference.combined_with(model),
            encoder: Encoder::Byte(ToneEncoder::new(model)),
        }));
        Ok(self)
    }

    /// Binds the typed (16-bit) evaluation: Exposure to `exposure_domain`
    /// (its `None` refuses with the own-profile sentence), White Balance to
    /// the validated `precision_model` and its `PrecisionToneEncoder`.
    pub(crate) fn prepare_precision(mut self, profile: &IccProfile) -> Result<Self, String> {
        if let CalibratedOp::Exposure(_) = self.op {
            let model = profile.model().ok_or(PROFILE_REFUSAL)?;
            let gammas = model
                .exposure_domain()
                .ok_or_else(|| crate::icc_encode_refusal::own_profile_sentence(profile.name()))?;
            self.domain = Some(ExposureDomain {
                gammas,
                byte_grid: false,
            });
            return Ok(self);
        }
        let model = precision_model(profile)
            .map_err(|e| crate::icc_encode_refusal::own_profile_error(profile, e))?;
        let reference = IccProfile::srgb();
        let reference = reference.model().ok_or(PROFILE_REFUSAL)?;
        self.pipeline = Some(Box::new(Pipeline {
            source: model.clone(),
            to_reference: model.combined_with(reference),
            from_reference: reference.combined_with(model),
            encoder: Encoder::Precision(PrecisionToneEncoder::new(model)?),
        }));
        Ok(self)
    }
    pub(crate) fn is_identity(&self) -> bool {
        self.op.is_identity()
    }
    /// The `op` string `CalibratedOp::parse` accepted: "exposure" |
    /// "white_balance".
    pub(crate) fn op_name(&self) -> &'static str {
        match self.op {
            CalibratedOp::Exposure(_) => "exposure",
            CalibratedOp::WhiteBalance(_) => "white_balance",
        }
    }
    /// The Exposure kernel itself when this is an Exposure, for the 32-bit
    /// float form that applies it per stored channel with no reference-basis
    /// matrices (`adjust_float`).
    pub(crate) fn exposure(&self) -> Option<&Exposure> {
        match &self.op {
            CalibratedOp::Exposure(op) => Some(op),
            CalibratedOp::WhiteBalance(_) => None,
        }
    }
    pub(crate) fn record_resources(&self, inventory: &mut crate::resources::Inventory) {
        if let Some(pipeline) = &self.pipeline {
            inventory.object(&**pipeline, "precision_adjustment", false);
            pipeline.source.record_resources(inventory);
            if let Encoder::Precision(encoder) = &pipeline.encoder {
                encoder.record_resources(inventory);
            }
        }
    }

    /// The LINEAR-LIGHT evaluation (Phase 11F): the input is already linear
    /// in the document profile's primaries, so the TRC decode on the way in
    /// and the forward encode on the way out are both skipped and only the
    /// two reference-basis matrices and the tone/Bradford kernel run. Nothing
    /// is clipped: extended-range and negative components pass straight
    /// through the matrices. At float only a White Balance reaches it; float
    /// Exposure runs per stored channel (`adjust_float`), and a bound
    /// Exposure holds no pipeline, so it returns `rgb` here.
    pub(crate) fn apply_linear(&self, rgb: [f32; 3]) -> [f32; 3] {
        if self.op.is_identity() {
            return rgb;
        }
        let Some(pipeline) = &self.pipeline else {
            return rgb;
        };
        let reference = multiply(pipeline.to_reference, rgb.map(f64::from));
        let output = multiply(pipeline.from_reference, self.op.linear(reference));
        output.map(|v| v as f32)
    }

    pub(crate) fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        if self.op.is_identity() {
            return rgb;
        }
        // Evaluation callers bind first; unbound descriptions are only used
        // for classification. Render preflight makes unsupported profiles an
        // explicit error rather than evaluating this defensive branch.
        if let (CalibratedOp::Exposure(op), Some(domain)) = (&self.op, &self.domain) {
            return domain.apply(op, rgb);
        }
        let Some(pipeline) = &self.pipeline else {
            return rgb;
        };
        let reference = multiply(pipeline.to_reference, pipeline.source.decode_rgb(rgb));
        let output = multiply(pipeline.from_reference, self.op.linear(reference));
        pipeline.encoder.rgb(output)
    }
}

/// The profile's RGB matrix/TRC model when it passes the high-depth precision
/// checks: the precondition of typed calibrated Exposure/White Balance and of a
/// high-depth conversion's DESTINATION. A LUT or other unmodelled profile refuses with
/// `PROFILE_REFUSAL`; a model whose tone curves fail `validate_precision` refuses
/// with that check's sentence.
pub(crate) fn precision_model(profile: &IccProfile) -> Result<&MatrixTrc, String> {
    let model = profile.model().ok_or(PROFILE_REFUSAL)?;
    model.validate_precision()?;
    Ok(model)
}

fn multiply(matrix: [[f64; 3]; 3], rgb: [f64; 3]) -> [f64; 3] {
    matrix.map(|row| (0..3).map(|c| row[c] * rgb[c]).sum())
}

impl Adjustment {
    pub(crate) fn requires_profile(&self) -> bool {
        matches!(self, Self::Calibrated(_))
    }

    /// Binds the byte (8-bit) evaluation to `profile`. A Photo Filter binds
    /// its D50 PCS basis and a Vibrance the profile's TRC, and neither fails
    /// (`BoundPhotoFilter::in_profile`, `BoundVibrance::in_profile`): the
    /// layer render binds through `in_profile(..).ok()`, so an `Err` would
    /// silently drop the layer.
    pub(crate) fn in_profile(self, profile: &IccProfile) -> Result<Self, String> {
        match self {
            Self::Calibrated(op) => op.prepare(profile).map(Self::Calibrated),
            Self::PhotoFilter(pf) => Ok(Self::PhotoFilter(pf.in_profile(profile))),
            Self::Vibrance(v) => Ok(Self::Vibrance(v.in_profile(profile))),
            legacy => Ok(legacy),
        }
    }
}

/// True when `profile` can evaluate calibrated (`document_v2`) Exposure and
/// White Balance: it has an RGB matrix/TRC model. The ONE statement of that
/// gate — the render preflight below, the scratch forecast and the Convert /
/// Assign destination refusals (`doc_color_destination`) all ask it.
pub(crate) fn evaluates_in(profile: &IccProfile) -> bool {
    profile.model().is_some()
}

/// Conservative render preflight, shared by full projections, group materialization
/// and merge-down. A hidden calibrated description still needs a valid context:
/// toggling visibility must not change whether its stored meaning is supported.
pub(crate) fn validate_context(layers: &[Layer], profile: &IccProfile) -> Result<(), String> {
    validate_context_with_job(layers, profile, crate::job_control::JobContext::NONE)
}

/// Conservative peak for one metadata parse, never the sum of all records.
/// JSON Value is 32 bytes on the target; short-number arrays can double
/// Vec capacity, and tiny nested objects allocate mostly-empty BTree nodes.
/// 256 bytes per input byte covers those container/node and String costs
/// plus the Adjustment parser's second-phase temporaries. This is a working
/// estimate, not an allocator/RSS ceiling. Matrix/TRC profiles skip parsing.
///
/// `Some(0)` for a matrix/TRC profile (`validate_context` parses nothing);
/// else [`parse_scratch`].
pub(crate) fn context_scratch(layers: &[Layer], profile: &IccProfile) -> Option<u64> {
    if evaluates_in(profile) {
        Some(0)
    } else {
        parse_scratch(layers)
    }
}

/// The description parse's peak under EVERY profile: the largest non-group
/// `meta` length × 256 (`context_scratch`'s figure without its profile
/// gate); `None` past `MAX_DRDC_META_LEN`. The byte regional admission
/// reserves it because its walk parses `Adjustment::from_meta` per
/// adjustment node whatever the profile (13A-5c-3).
pub(crate) fn parse_scratch(layers: &[Layer]) -> Option<u64> {
    let largest = layers
        .iter()
        .filter(|l| l.kind != LayerKind::Group)
        .filter_map(|l| l.meta.as_ref().map(String::len))
        .max()
        .unwrap_or(0);
    if largest > crate::drdc::MAX_DRDC_META_LEN as usize {
        return None;
    }
    Some(largest as u64 * 256)
}

/// Same classifier/refusal as every byte projection. Individual serde parses
/// remain bounded platform-independent calls; controlled callers admit their
/// forecast first and check cancellation on both sides of each layer parse.
pub(crate) fn validate_context_with_job(
    layers: &[Layer],
    profile: &IccProfile,
    job: crate::job_control::JobContext<'_>,
) -> Result<(), String> {
    job.check()?;
    if !evaluates_in(profile) {
        for layer in layers {
            job.check()?;
            let requires_profile = layer.kind != LayerKind::Group
                && layer
                    .meta
                    .as_deref()
                    .and_then(Adjustment::from_meta)
                    .is_some_and(|op| op.requires_profile());
            job.check()?;
            if requires_profile {
                return Err(PROFILE_REFUSAL.into());
            }
        }
    }
    Ok(())
}

impl DrDocument {
    /// Apply a complete adjustment description to an image in this document's
    /// colour context. Selection/target writeback remains in `doc_scope`.
    pub(crate) fn adjust_image(
        &self,
        image: &crate::DrImage,
        adjustment: Adjustment,
    ) -> Result<crate::DrImage, String> {
        if adjustment.requires_profile() {
            if let Some(refusal) = calibrated_refusal(&self.profile, adjustment.op_name()) {
                return Err(refusal);
            }
        }
        let bound = adjustment.in_profile(&self.profile)?;
        Ok(crate::DrImage {
            pixels: crate::adjust_math::apply_to_image(&bound, &image.pixels),
        })
    }
}
