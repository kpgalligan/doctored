//! Bounded persistent encoded-RGB layer blending. This is metadata over the
//! existing f32 W3C compositor, not paint blending or a new pixel kernel.
//! Admission is by TARGET (`BlendTarget`): the 26 non-Normal color modes are
//! open to raster layers, adjustment layers and isolated/pass-through groups;
//! Pass Through stays group-only. An adjustment layer's mode pushes its
//! adjusted result through the mode as the SOURCE against the backdrop, the
//! byte `doc.rs::composite_adjustment_into` rule, and persists as the
//! conditional native v37 / LBLD schema7 word specified in
//! `precision_blend_native.rs`. Knockout is not a blend mode and is not part
//! of this admission set; it is a layer-style blending option implemented in
//! `style_knockout.rs` at both depths (Phase 12E-3/12E-3b, carryover P7-37).
//!
//! the authoritative native Normal/Multiply/Screen plus twenty-four additional qualified raster and
//! isolated-group mode set uses the existing `blend.rs` kernel (the four nonseparable HSL modes
//! through its unchanged f32 NonSeparable path; Dissolve through its canvas-absolute hash).
//! Preserve the mode through ancestor reconstruction. No new blend math or intermediate group
//! quantization. Adjustment-layer blending is closed
//! (`designs/phase-10d-adjustment-blending-status.md`); knockout is implemented at BOTH depths
//! (`style_knockout.rs` at 8 bits, `precision_style_knockout.rs` at 16, Phase 12E-3 / 12E-3b) and
//! its PSD `knko`/`infx` records cross both ways at both depths since 12E-3c; P7-37 is VERIFIED
//! since 12E-3d, whose Photoshop block measured the 16-bit re-measurement (ten MATCH under T3), the
//! `deep_over_ordinary_bottom` divergence (DIVERGENCE (disclosed) at both depths), the per-record
//! export verification (eight MATCH plus PX-PAD) and both contract observations.
use crate::blend::BlendMode;
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, BOOKKEEPING_BYTES};
use serde::Deserialize;

/// Authoritative native raster/isolated-group semantic set. Persistence and
/// target admission consume this same set; Pass Through is separately group-only.
pub(crate) const COLOR_MODES: [BlendMode; 27] = [
    BlendMode::Normal,
    BlendMode::Multiply,
    BlendMode::Screen,
    BlendMode::Overlay,
    BlendMode::SoftLight,
    BlendMode::HardLight,
    BlendMode::Darken,
    BlendMode::Lighten,
    BlendMode::Difference,
    BlendMode::Exclusion,
    BlendMode::ColorDodge,
    BlendMode::ColorBurn,
    BlendMode::Addition,
    BlendMode::LinearBurn,
    BlendMode::Subtract,
    BlendMode::VividLight,
    BlendMode::LinearLight,
    BlendMode::PinLight,
    BlendMode::Divide,
    BlendMode::Hue,
    BlendMode::Saturation,
    BlendMode::Color,
    BlendMode::Luminosity,
    BlendMode::Dissolve,
    BlendMode::DarkerColor,
    BlendMode::LighterColor,
    BlendMode::HardMix,
];
pub(crate) fn is_nondefault(mode: BlendMode) -> bool {
    mode != BlendMode::Normal && COLOR_MODES.contains(&mode)
}

/// What a record is, for blend-mode admission. Adjustment layers blend their
/// adjusted result as the source; only Pass Through stays group-only.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BlendTarget {
    Raster,
    IsolatedGroup,
    PassThroughGroup,
    Adjustment,
}

pub(crate) fn validate(mode: BlendMode, target: BlendTarget) -> Result<(), String> {
    match mode {
        BlendMode::Normal => Ok(()),
        mode if COLOR_MODES.contains(&mode) => Ok(()),
        BlendMode::PassThrough
            if matches!(
                target,
                BlendTarget::IsolatedGroup | BlendTarget::PassThroughGroup
            ) =>
        {
            Ok(())
        }
        _ => Err("native layer blending supports Normal, Multiply, Screen, Overlay, Soft Light, Hard Light, Darken, Lighten, Difference, Exclusion, Color Dodge, Color Burn, Linear Dodge (Add), Linear Burn, Subtract, Vivid Light, Linear Light, Pin Light, Divide, Hue, Saturation, Color, Luminosity, Dissolve, Darker Color, Lighter Color and Hard Mix on raster layers, adjustment layers or isolated groups; Pass Through requires a group".into()),
    }
}
impl PrecisionDocumentLayer {
    pub(crate) fn blend_target(&self) -> BlendTarget {
        match &self.content {
            Content::CmykRaster { .. } => BlendTarget::Raster,
            Content::Raster { .. } => BlendTarget::Raster,
            Content::Adjustment(_) | Content::CmykAdjustment(_) => BlendTarget::Adjustment,
            Content::Group(_) => BlendTarget::IsolatedGroup,
            Content::PassThrough(_) => BlendTarget::PassThroughGroup,
        }
    }
    pub fn blend_mode(&self) -> BlendMode {
        if self.is_pass_through() {
            BlendMode::PassThrough
        } else {
            self.blend_mode
        }
    }
    /// Consumes a newly constructed layer; incompatible modes refuse without
    /// rewriting pixels, masks, source descriptions or independent placement.
    pub fn with_blend_mode(mut self, mode: BlendMode) -> Result<Self, String> {
        validate(mode, self.blend_target())?;
        if let Content::Group(children) | Content::PassThrough(children) = &self.content {
            self.content = if mode == BlendMode::PassThrough {
                Content::PassThrough(children.clone())
            } else {
                Content::Group(children.clone())
            };
        }
        self.blend_mode = if mode == BlendMode::PassThrough {
            BlendMode::Normal
        } else {
            mode
        };
        Ok(self)
    }
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum LayerBlendEdit {
    LayerBlend {
        version: u32,
        layer: usize,
        blend_mode: i32,
    },
}
impl LayerBlendEdit {
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| format!("invalid native layer blend request: {e}"))
    }
    fn target<'a>(
        &self,
        document: &'a PrecisionDocument,
    ) -> Result<(usize, &'a PrecisionDocumentLayer, BlendMode), String> {
        let Self::LayerBlend {
            version,
            layer,
            blend_mode,
        } = *self;
        if version != 1 {
            return Err("unsupported native layer blend request version".into());
        }
        let old = document
            .layer(layer)
            .ok_or("native blend layer index is out of range")?;
        let mode = BlendMode::from_c(blend_mode).ok_or("unknown native layer blend mode")?;
        validate(mode, old.blend_target())?;
        Ok((layer, old, mode))
    }
    pub(crate) fn scratch_bytes(&self, document: &PrecisionDocument) -> Result<u64, String> {
        self.target(document)?;
        let names: u64 = (0..document.layer_count())
            .filter_map(|i| document.layer(i))
            .map(|l| l.name().len() as u64)
            .sum();
        Ok(4 * BOOKKEEPING_BYTES
            + names * 2
            + document.layer_count() as u64 * 4096
            + document.channels().len() as u64 * 1024)
    }
    pub(crate) fn apply(
        &self,
        document: &PrecisionDocument,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        let scratch = self.scratch_bytes(document)?;
        admit(document.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        job.check()?;
        let (index, old, mode) = self.target(document)?;
        if old.blend_mode() == mode {
            return Ok(None);
        }
        let replacement = old.clone().with_blend_mode(mode)?;
        let result = document.replacing_layer(index, replacement)?;
        job.check()?;
        Ok(result)
    }
}
