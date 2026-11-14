//! The paint blend: a layer blend mode, or one of the two PAINT-ONLY modes,
//! Behind and Clear, which act on a layer's alpha and so have no meaning as a
//! layer's own blend setting.
//!
//! The rule (Doctored's own; `designs/phase-11f-hdr-float-status.md` holds
//! it too). With the layer pixel straight `(Cb, ab)` and the paint `(Cs, sa)`,
//! `sa = ink alpha × opacity × coverage` (coverage = the dab or the band
//! times the selection):
//!
//! - **Behind** paints under what the layer already holds:
//!   `ao = ab + sa(1 − ab)`, `Co = (Cb·ab + Cs·sa(1 − ab)) / ao`. A fully
//!   opaque pixel is not written (bit for bit, at float too); a fully
//!   transparent one takes `(Cs, sa)`. No blend function and no dither take
//!   part. It is the W3C compositing step with `B(Cb, Cs) = Cb`
//!   (`blend::BEHIND`), which is exactly destination-over, so every paint
//!   walk runs it through the step it already has.
//! - **Clear** removes what the layer holds: `ao = ab(1 − sa)`; colour is
//!   neither read nor written, so the ink's colour is inert and only its
//!   alpha counts. It is the Eraser's rule (`blend::erase_alpha`) at the
//!   Brush's, Fill's or Stroke's coverage, and it never grows a layer.
//!
//! Depth: the same formulas on the stored straight samples — 8-bit and 16-bit
//! codes rounded once at storage; 32-bit float in linear light with colour
//! unclamped (values above white and below zero pass through) and alpha in
//! 0…1. Both need an alpha that is free to change: under Preserve
//! Transparency or a transparency lock every pixel stays as it is (Behind's
//! frozen form is `lerp(Cb, Cb, sa)`), so each walk answers its no-change
//! `None` and the host refuses by name before the call. A mask, a colour
//! plane, an alpha channel and the Quick Mask have no transparency of their
//! own and refuse both.
//!
//! Where they apply: the Brush (and the Pencil, which strokes as it), Edit ▸
//! Fill… (colour and Pattern contents) and Edit ▸ Stroke… (a selection's band
//! and the layer edge), in RGB, Grayscale, Duotone and Lab at every depth those
//! have — the formulas are alpha arithmetic on straight components, so they
//! are the same in every colour model. CMYK, Indexed and Bitmap refuse both by
//! name ([`mode_phrase`]), and so does every other tool that takes a mode (the
//! Paint Bucket, the Gradient, the Clone Stamp, a layer's own mode): the codes
//! 28 and 29 are not [`BlendMode`]s, so every `BlendMode::from_c` site refuses
//! them as an unknown mode.

use serde_json::Value;

use crate::blend::BlendMode;

/// `DR_BLEND_BEHIND`.
pub const BEHIND_CODE: i32 = 28;
/// `DR_BLEND_CLEAR`.
pub const CLEAR_CODE: i32 = 29;

/// How a Brush stroke, a Fill or a Stroke meets the layer's pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintBlend {
    /// A layer blend mode, through the W3C step (Normal is source-over).
    Mode(BlendMode),
    /// Paint under the layer's pixels (module doc).
    Behind,
    /// Take the paint's alpha off the layer's (module doc).
    Clear,
}

impl From<BlendMode> for PaintBlend {
    fn from(mode: BlendMode) -> Self {
        PaintBlend::Mode(mode)
    }
}

impl PaintBlend {
    /// A raw `DrBlendMode` value from a paint, Fill or Stroke entry point: 28
    /// is Behind, 29 Clear, anything else [`BlendMode::from_c`]'s answer.
    pub fn from_c(code: i32) -> Option<Self> {
        match code {
            BEHIND_CODE => Some(PaintBlend::Behind),
            CLEAR_CODE => Some(PaintBlend::Clear),
            _ => BlendMode::from_c(code).map(PaintBlend::Mode),
        }
    }

    /// [`Self::from_c`] of a request's unsigned `blend_mode`.
    pub(crate) fn from_u32(code: u32) -> Option<Self> {
        i32::try_from(code).ok().and_then(Self::from_c)
    }

    /// True for Behind and Clear.
    pub fn is_paint_only(self) -> bool {
        matches!(self, PaintBlend::Behind | PaintBlend::Clear)
    }

    /// True for a group's Pass Through, which no paint takes.
    pub fn is_group_only(self) -> bool {
        matches!(self, PaintBlend::Mode(mode) if mode.is_group_only())
    }

    /// The mode's display name for Behind and Clear; `None` for a layer mode
    /// (each colour mode names those its own way).
    pub fn paint_only_name(self) -> Option<&'static str> {
        match self {
            PaintBlend::Behind => Some("Behind"),
            PaintBlend::Clear => Some("Clear"),
            PaintBlend::Mode(_) => None,
        }
    }
}

/// The paint-only mode a request's `blend_mode` names, if it names one — what
/// a colour mode that does not offer them asks before its own parser.
pub(crate) fn requested(value: &Value) -> Option<PaintBlend> {
    value["blend_mode"]
        .as_i64()
        .and_then(|code| i32::try_from(code).ok())
        .and_then(PaintBlend::from_c)
        .filter(|mode| mode.is_paint_only())
}

/// `"The Behind blend mode"` / `"The Clear blend mode"`: the subject each
/// colour mode's refusal sentence takes.
pub(crate) fn mode_phrase(mode: PaintBlend) -> String {
    format!(
        "The {} blend mode",
        mode.paint_only_name().unwrap_or("Normal")
    )
}

/// The sentence a native paint request that is not a Brush stroke, a Fill or
/// a Stroke (the Paint Bucket, the Gradient) refuses Behind or Clear with.
pub(crate) fn tool_refusal(mode: PaintBlend) -> String {
    format!(
        "{} applies to the Brush, Fill and Stroke only",
        mode_phrase(mode)
    )
}

/// The sentence a colour-plane target refuses Behind or Clear with: one
/// channel has no transparency of its own.
pub(crate) fn plane_refusal(mode: PaintBlend) -> String {
    format!(
        "{} paints a layer's transparency; a colour plane has none of its own",
        mode_phrase(mode)
    )
}
