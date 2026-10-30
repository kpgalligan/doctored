//! The ten layer-style effects (one struct each with the schema defaults of
//! `style`'s module doc; Pattern Overlay's lives in `style_pattern`), the
//! [`EffectKind`] enumeration whose declaration order is the canonical JSON
//! order and the render order, and the [`Effect`] sum type with each
//! effect's plane REACH — the input to `LayerStyle::pad`. `style`
//! re-exports everything here.
//!
//! the effect structs with `EffectKind`/`Effect` and each effect's plane reach.

use crate::blend::BlendMode;
use crate::style_contour::{Contour, DEFAULT_RANGE};
use crate::style_fx_bevel_emboss::reach as bevel_reach;
use crate::style_model::{
    BevelDirection, BevelStyle, BevelTechnique, GlobalLight, GlowFill, GlowSource, GradientFill,
    StrokeFill, StrokePosition,
};
use crate::style_pattern::PatternOverlay;
use crate::style_pattern_fill::{BevelTexture, PatternFill};
use crate::style_render::{blur_reach, spread_split};

#[derive(Clone, Debug, PartialEq)]
pub struct DropShadow {
    pub enabled: bool,
    pub blend: BlendMode,
    pub color: [u8; 3],
    /// A CMYK document's colour as straight ink `[c, m, y, k]` in `0..=1`,
    /// four-decimal quantized; `None` = none (#547, `cmyk_style`).
    pub ink: Option<[f32; 4]>,
    /// The colour as LINEAR-LIGHT sRGB-primaries components, extended range
    /// (negative outside sRGB's gamut), when it is not an sRGB byte
    /// (`authored_color::wide_into`); `color` is then its nearest sRGB byte.
    pub linear: Option<[f32; 3]>,
    pub opacity: f32,
    pub angle: f32,
    pub use_global_light: bool,
    pub distance: f32,
    pub spread: f32,
    pub size: f32,
    pub layer_knocks_out: bool,
    /// The contour on the shadow's plane (`style_contour`; Linear).
    pub contour: Contour,
    /// The contour's Anti-aliased (false).
    pub anti_aliased: bool,
    /// Noise, `0..=1` (0..100 %; 0): the grain `style_noise`
    /// adds to the plane.
    pub noise: f32,
}

impl Default for DropShadow {
    fn default() -> Self {
        DropShadow {
            enabled: true,
            blend: BlendMode::Multiply,
            color: [0, 0, 0],
            ink: None,
            linear: None,
            opacity: 0.75,
            angle: 120.0,
            use_global_light: true,
            distance: 5.0,
            spread: 0.0,
            size: 5.0,
            layer_knocks_out: true,
            contour: Contour::default(),
            anti_aliased: false,
            noise: 0.0,
        }
    }
}

impl DropShadow {
    /// The angle this effect is lit from: the document's when
    /// `use_global_light`, else its own.
    pub fn resolved_angle(&self, light: GlobalLight) -> f32 {
        if self.use_global_light {
            light.angle
        } else {
            self.angle
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct InnerShadow {
    pub enabled: bool,
    pub blend: BlendMode,
    pub color: [u8; 3],
    /// A CMYK document's colour as straight ink `[c, m, y, k]` in `0..=1`,
    /// four-decimal quantized; `None` = none (#547, `cmyk_style`).
    pub ink: Option<[f32; 4]>,
    /// The colour as LINEAR-LIGHT sRGB-primaries components, extended range
    /// (negative outside sRGB's gamut), when it is not an sRGB byte
    /// (`authored_color::wide_into`); `color` is then its nearest sRGB byte.
    pub linear: Option<[f32; 3]>,
    pub opacity: f32,
    pub angle: f32,
    pub use_global_light: bool,
    pub distance: f32,
    pub choke: f32,
    pub size: f32,
    /// See [`DropShadow::contour`].
    pub contour: Contour,
    /// See [`DropShadow::anti_aliased`].
    pub anti_aliased: bool,
    /// See [`DropShadow::noise`].
    pub noise: f32,
}

impl Default for InnerShadow {
    fn default() -> Self {
        InnerShadow {
            enabled: true,
            blend: BlendMode::Multiply,
            color: [0, 0, 0],
            ink: None,
            linear: None,
            opacity: 0.75,
            angle: 120.0,
            use_global_light: true,
            distance: 5.0,
            choke: 0.0,
            size: 5.0,
            contour: Contour::default(),
            anti_aliased: false,
            noise: 0.0,
        }
    }
}

impl InnerShadow {
    /// See [`DropShadow::resolved_angle`].
    pub fn resolved_angle(&self, light: GlobalLight) -> f32 {
        if self.use_global_light {
            light.angle
        } else {
            self.angle
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OuterGlow {
    pub enabled: bool,
    pub blend: BlendMode,
    pub color: [u8; 3],
    /// A CMYK document's colour as straight ink `[c, m, y, k]` in `0..=1`,
    /// four-decimal quantized; `None` = none (#547, `cmyk_style`).
    pub ink: Option<[f32; 4]>,
    /// The colour as LINEAR-LIGHT sRGB-primaries components, extended range
    /// (negative outside sRGB's gamut), when it is not an sRGB byte
    /// (`authored_color::wide_into`); `color` is then its nearest sRGB byte.
    pub linear: Option<[f32; 3]>,
    pub opacity: f32,
    pub spread: f32,
    pub size: f32,
    /// See [`DropShadow::contour`].
    pub contour: Contour,
    /// See [`DropShadow::anti_aliased`].
    pub anti_aliased: bool,
    /// Range, 0.01..=1 (1..100 %; 0.5): which part of the glow
    /// the contour spans (`style_contour`).
    pub range: f32,
    /// See [`DropShadow::noise`].
    pub noise: f32,
    /// Color or Gradient: what the glow paints with
    /// (`style_glow_gradient`).
    pub fill_type: GlowFill,
    /// The glow's gradient: only `stops` is kept (the parser resets every
    /// other key to its default); read only when `fill_type` is Gradient.
    pub gradient: GradientFill,
    /// Jitter, 0..=1 (0-100 %): how far each pixel's place on
    /// the gradient is scattered (`style_glow_gradient`).
    pub jitter: f32,
}

impl Default for OuterGlow {
    fn default() -> Self {
        OuterGlow {
            enabled: true,
            blend: BlendMode::Screen,
            color: [0xff, 0xff, 0xbe],
            ink: None,
            linear: None,
            opacity: 0.75,
            spread: 0.0,
            size: 5.0,
            contour: Contour::default(),
            anti_aliased: false,
            range: DEFAULT_RANGE,
            noise: 0.0,
            fill_type: GlowFill::Color,
            gradient: GradientFill::default(),
            jitter: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct InnerGlow {
    pub enabled: bool,
    pub blend: BlendMode,
    pub color: [u8; 3],
    /// A CMYK document's colour as straight ink `[c, m, y, k]` in `0..=1`,
    /// four-decimal quantized; `None` = none (#547, `cmyk_style`).
    pub ink: Option<[f32; 4]>,
    /// The colour as LINEAR-LIGHT sRGB-primaries components, extended range
    /// (negative outside sRGB's gamut), when it is not an sRGB byte
    /// (`authored_color::wide_into`); `color` is then its nearest sRGB byte.
    pub linear: Option<[f32; 3]>,
    pub opacity: f32,
    pub choke: f32,
    pub size: f32,
    pub source: GlowSource,
    /// See [`DropShadow::contour`].
    pub contour: Contour,
    /// See [`DropShadow::anti_aliased`].
    pub anti_aliased: bool,
    /// See [`OuterGlow::range`].
    pub range: f32,
    /// See [`DropShadow::noise`].
    pub noise: f32,
    /// See [`OuterGlow::fill_type`].
    pub fill_type: GlowFill,
    /// See [`OuterGlow::gradient`].
    pub gradient: GradientFill,
    /// See [`OuterGlow::jitter`].
    pub jitter: f32,
}

impl Default for InnerGlow {
    fn default() -> Self {
        InnerGlow {
            enabled: true,
            blend: BlendMode::Screen,
            color: [0xff, 0xff, 0xbe],
            ink: None,
            linear: None,
            opacity: 0.75,
            choke: 0.0,
            size: 5.0,
            source: GlowSource::Edge,
            contour: Contour::default(),
            anti_aliased: false,
            range: DEFAULT_RANGE,
            noise: 0.0,
            fill_type: GlowFill::Color,
            gradient: GradientFill::default(),
            jitter: 0.0,
        }
    }
}

impl InnerGlow {
    /// True iff the renderer emits no contribution for ANY shape
    /// (shape-dependent no-ops, such as all-zero coverage, are not this);
    /// `style_knockout::has_grouped_interior_effect` relies on it mirroring the
    /// renderer. An `edge` glow of size 0 draws nothing; a `center` glow of size 0
    /// is the whole shape. The NaN clauses cannot be reached through JSON or
    /// PSD (the parsers clamp) and are kept only so the predicate matches the
    /// renderer.
    pub(crate) fn skipped_by_settings(&self) -> bool {
        self.opacity.is_nan()
            || self.opacity <= 0.0
            || (self.source == GlowSource::Edge && (self.size.is_nan() || self.size <= 0.0))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub enabled: bool,
    pub blend: BlendMode,
    pub opacity: f32,
    pub size: f32,
    pub position: StrokePosition,
    pub fill_type: StrokeFill,
    pub color: [u8; 3],
    /// A CMYK document's colour as straight ink `[c, m, y, k]` in `0..=1`,
    /// four-decimal quantized; `None` = none (#547, `cmyk_style`).
    pub ink: Option<[f32; 4]>,
    /// The colour as LINEAR-LIGHT sRGB-primaries components, extended range
    /// (negative outside sRGB's gamut), when it is not an sRGB byte
    /// (`authored_color::wide_into`); `color` is then its nearest sRGB byte.
    pub linear: Option<[f32; 3]>,
    pub gradient: GradientFill,
    /// The Fill Type Pattern's fill: the Pattern Overlay's model, read
    /// only when `fill_type` is Pattern (`style_pattern_fill`).
    pub pattern_fill: PatternFill,
}

impl Default for Stroke {
    fn default() -> Self {
        Stroke {
            enabled: true,
            blend: BlendMode::Normal,
            opacity: 1.0,
            size: 3.0,
            position: StrokePosition::Outside,
            fill_type: StrokeFill::Color,
            color: [0, 0, 0],
            ink: None,
            linear: None,
            gradient: GradientFill::default(),
            pattern_fill: PatternFill::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColorOverlay {
    pub enabled: bool,
    pub blend: BlendMode,
    pub color: [u8; 3],
    /// A CMYK document's colour as straight ink `[c, m, y, k]` in `0..=1`,
    /// four-decimal quantized; `None` = none (#547, `cmyk_style`).
    pub ink: Option<[f32; 4]>,
    /// The colour as LINEAR-LIGHT sRGB-primaries components, extended range
    /// (negative outside sRGB's gamut), when it is not an sRGB byte
    /// (`authored_color::wide_into`); `color` is then its nearest sRGB byte.
    pub linear: Option<[f32; 3]>,
    pub opacity: f32,
}

impl Default for ColorOverlay {
    fn default() -> Self {
        ColorOverlay {
            enabled: true,
            blend: BlendMode::Normal,
            color: [0xff, 0, 0],
            ink: None,
            linear: None,
            opacity: 1.0,
        }
    }
}

impl ColorOverlay {
    /// True iff the renderer emits no contribution for ANY shape
    /// (shape-dependent no-ops, such as all-zero coverage, are not this);
    /// `style_knockout::has_grouped_interior_effect` relies on it mirroring the
    /// renderer. The NaN clause cannot be reached through JSON or PSD and is kept
    /// only so the predicate matches the renderer.
    pub(crate) fn skipped_by_settings(&self) -> bool {
        self.opacity.is_nan() || self.opacity <= 0.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GradientOverlay {
    pub enabled: bool,
    pub blend: BlendMode,
    pub opacity: f32,
    pub gradient: GradientFill,
}

impl Default for GradientOverlay {
    fn default() -> Self {
        GradientOverlay {
            enabled: true,
            blend: BlendMode::Normal,
            opacity: 1.0,
            gradient: GradientFill::default(),
        }
    }
}

impl GradientOverlay {
    /// True iff the renderer emits no contribution for ANY shape
    /// (shape-dependent no-ops, such as all-zero coverage, are not this);
    /// `style_knockout::has_grouped_interior_effect` relies on it mirroring the
    /// renderer. The NaN and empty-stops clauses cannot be reached through JSON or
    /// PSD and are kept only so the predicate matches the renderer.
    pub(crate) fn skipped_by_settings(&self) -> bool {
        self.opacity.is_nan() || self.opacity <= 0.0 || self.gradient.stops.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BevelEmboss {
    pub enabled: bool,
    pub style: BevelStyle,
    /// The Technique (Smooth; #578).
    pub technique: BevelTechnique,
    pub depth: f32,
    pub direction: BevelDirection,
    pub size: f32,
    pub soften: f32,
    pub angle: f32,
    pub use_global_light: bool,
    pub altitude: f32,
    pub highlight_blend: BlendMode,
    pub highlight_color: [u8; 3],
    /// The highlight's CMYK ink (see [`DropShadow::ink`]).
    pub highlight_ink: Option<[f32; 4]>,
    /// The highlight's wide colour (see [`DropShadow::linear`]).
    pub highlight_linear: Option<[f32; 3]>,
    pub highlight_opacity: f32,
    pub shadow_blend: BlendMode,
    pub shadow_color: [u8; 3],
    /// The shadow's CMYK ink (see [`DropShadow::ink`]).
    pub shadow_ink: Option<[f32; 4]>,
    /// The shadow's wide colour (see [`DropShadow::linear`]).
    pub shadow_linear: Option<[f32; 3]>,
    pub shadow_opacity: f32,
    /// The Gloss Contour on the shading (`style_contour`; Linear).
    pub gloss_contour: Contour,
    /// The Gloss Contour's Anti-aliased (false).
    pub gloss_anti_aliased: bool,
    /// The Contour sub-panel's checkbox (false): whether `contour` reshapes
    /// the height.
    pub contour_enabled: bool,
    /// The Contour sub-panel's curve (Linear).
    pub contour: Contour,
    /// The Contour sub-panel's Anti-aliased (false).
    pub contour_anti_aliased: bool,
    /// The Contour sub-panel's Range, 0.01..=1 (0.5).
    pub contour_range: f32,
    /// The Texture sub-panel's checkbox (false; `style_texture`).
    pub texture_enabled: bool,
    /// The Texture sub-panel (`style_pattern_fill::BevelTexture`), kept
    /// while switched off.
    pub texture: BevelTexture,
}

impl Default for BevelEmboss {
    fn default() -> Self {
        BevelEmboss {
            enabled: true,
            style: BevelStyle::InnerBevel,
            technique: BevelTechnique::Smooth,
            depth: 1.0,
            direction: BevelDirection::Up,
            size: 5.0,
            soften: 0.0,
            angle: 120.0,
            use_global_light: true,
            altitude: 30.0,
            highlight_blend: BlendMode::Screen,
            highlight_color: [0xff, 0xff, 0xff],
            highlight_ink: None,
            highlight_linear: None,
            highlight_opacity: 0.75,
            shadow_blend: BlendMode::Multiply,
            shadow_color: [0, 0, 0],
            shadow_ink: None,
            shadow_linear: None,
            shadow_opacity: 0.75,
            gloss_contour: Contour::default(),
            gloss_anti_aliased: false,
            contour_enabled: false,
            contour: Contour::default(),
            contour_anti_aliased: false,
            contour_range: DEFAULT_RANGE,
            texture_enabled: false,
            texture: BevelTexture::default(),
        }
    }
}

impl BevelEmboss {
    /// See [`DropShadow::resolved_angle`].
    pub fn resolved_angle(&self, light: GlobalLight) -> f32 {
        if self.use_global_light {
            light.angle
        } else {
            self.angle
        }
    }

    /// The light's altitude: the document's when `use_global_light`, else
    /// the effect's own.
    pub fn resolved_altitude(&self, light: GlobalLight) -> f32 {
        if self.use_global_light {
            light.altitude
        } else {
            self.altitude
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Satin {
    pub enabled: bool,
    pub blend: BlendMode,
    pub color: [u8; 3],
    /// A CMYK document's colour as straight ink `[c, m, y, k]` in `0..=1`,
    /// four-decimal quantized; `None` = none (#547, `cmyk_style`).
    pub ink: Option<[f32; 4]>,
    /// The colour as LINEAR-LIGHT sRGB-primaries components, extended range
    /// (negative outside sRGB's gamut), when it is not an sRGB byte
    /// (`authored_color::wide_into`); `color` is then its nearest sRGB byte.
    pub linear: Option<[f32; 3]>,
    pub opacity: f32,
    pub angle: f32,
    pub distance: f32,
    pub size: f32,
    pub invert: bool,
    /// See [`DropShadow::contour`].
    pub contour: Contour,
    /// See [`DropShadow::anti_aliased`].
    pub anti_aliased: bool,
}

impl Default for Satin {
    fn default() -> Self {
        Satin {
            enabled: true,
            blend: BlendMode::Multiply,
            color: [0, 0, 0],
            ink: None,
            linear: None,
            opacity: 0.5,
            angle: 19.0,
            distance: 11.0,
            size: 14.0,
            invert: true,
            contour: Contour::default(),
            anti_aliased: false,
        }
    }
}

impl Satin {
    /// True iff the renderer emits no contribution for ANY shape
    /// (shape-dependent no-ops, such as all-zero coverage, are not this);
    /// `style_knockout::has_grouped_interior_effect` relies on it mirroring the
    /// renderer. The NaN clause cannot be reached through JSON or PSD and is kept
    /// only so the predicate matches the renderer.
    pub(crate) fn skipped_by_settings(&self) -> bool {
        self.opacity.is_nan() || self.opacity <= 0.0
    }
}

/// The ten effect kinds. Declaration order == canonical JSON order ==
/// render order (Stroke and BevelEmboss decide Below/Interior per
/// contribution; see `style_render`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EffectKind {
    DropShadow,
    OuterGlow,
    Stroke,
    PatternOverlay,
    GradientOverlay,
    ColorOverlay,
    Satin,
    InnerGlow,
    InnerShadow,
    BevelEmboss,
}

impl EffectKind {
    /// Every kind, in render order.
    pub const ALL: [EffectKind; 10] = [
        EffectKind::DropShadow,
        EffectKind::OuterGlow,
        EffectKind::Stroke,
        EffectKind::PatternOverlay,
        EffectKind::GradientOverlay,
        EffectKind::ColorOverlay,
        EffectKind::Satin,
        EffectKind::InnerGlow,
        EffectKind::InnerShadow,
        EffectKind::BevelEmboss,
    ];

    /// The JSON `type` string.
    pub fn type_name(self) -> &'static str {
        match self {
            EffectKind::DropShadow => "drop_shadow",
            EffectKind::OuterGlow => "outer_glow",
            EffectKind::Stroke => "stroke",
            EffectKind::PatternOverlay => "pattern_overlay",
            EffectKind::GradientOverlay => "gradient_overlay",
            EffectKind::ColorOverlay => "color_overlay",
            EffectKind::Satin => "satin",
            EffectKind::InnerGlow => "inner_glow",
            EffectKind::InnerShadow => "inner_shadow",
            EffectKind::BevelEmboss => "bevel_emboss",
        }
    }

    /// The kind for a JSON `type` string; `None` for anything unknown.
    pub fn from_type_name(name: &str) -> Option<Self> {
        EffectKind::ALL.into_iter().find(|k| k.type_name() == name)
    }

    /// Whether a style may hold several instances of this kind
    /// (`style_instances`): the five kinds the Layer Style dialog gives a
    /// "+" button. Every other kind holds at most one.
    pub fn repeats(self) -> bool {
        matches!(
            self,
            EffectKind::DropShadow
                | EffectKind::InnerShadow
                | EffectKind::Stroke
                | EffectKind::ColorOverlay
                | EffectKind::GradientOverlay
        )
    }
}

/// One effect of the stack.
#[derive(Clone, Debug, PartialEq)]
// The bevel's two contours are held inline (`style_contour::ContourCurve`,
// no heap, so a style's resource accounting is unchanged); effects live in
// the style's `Vec`, so the arms' size difference is never copied around.
#[allow(clippy::large_enum_variant)]
pub enum Effect {
    DropShadow(DropShadow),
    InnerShadow(InnerShadow),
    OuterGlow(OuterGlow),
    InnerGlow(InnerGlow),
    Stroke(Stroke),
    ColorOverlay(ColorOverlay),
    GradientOverlay(GradientOverlay),
    PatternOverlay(PatternOverlay),
    BevelEmboss(BevelEmboss),
    Satin(Satin),
}

impl Effect {
    pub fn kind(&self) -> EffectKind {
        match self {
            Effect::DropShadow(_) => EffectKind::DropShadow,
            Effect::InnerShadow(_) => EffectKind::InnerShadow,
            Effect::OuterGlow(_) => EffectKind::OuterGlow,
            Effect::InnerGlow(_) => EffectKind::InnerGlow,
            Effect::Stroke(_) => EffectKind::Stroke,
            Effect::ColorOverlay(_) => EffectKind::ColorOverlay,
            Effect::GradientOverlay(_) => EffectKind::GradientOverlay,
            Effect::PatternOverlay(_) => EffectKind::PatternOverlay,
            Effect::BevelEmboss(_) => EffectKind::BevelEmboss,
            Effect::Satin(_) => EffectKind::Satin,
        }
    }

    pub fn enabled(&self) -> bool {
        match self {
            Effect::DropShadow(e) => e.enabled,
            Effect::InnerShadow(e) => e.enabled,
            Effect::OuterGlow(e) => e.enabled,
            Effect::InnerGlow(e) => e.enabled,
            Effect::Stroke(e) => e.enabled,
            Effect::ColorOverlay(e) => e.enabled,
            Effect::GradientOverlay(e) => e.enabled,
            Effect::PatternOverlay(e) => e.enabled,
            Effect::BevelEmboss(e) => e.enabled,
            Effect::Satin(e) => e.enabled,
        }
    }

    /// The effect of `kind` with every key at its default.
    pub fn default_for(kind: EffectKind) -> Effect {
        match kind {
            EffectKind::DropShadow => Effect::DropShadow(DropShadow::default()),
            EffectKind::InnerShadow => Effect::InnerShadow(InnerShadow::default()),
            EffectKind::OuterGlow => Effect::OuterGlow(OuterGlow::default()),
            EffectKind::InnerGlow => Effect::InnerGlow(InnerGlow::default()),
            EffectKind::Stroke => Effect::Stroke(Stroke::default()),
            EffectKind::ColorOverlay => Effect::ColorOverlay(ColorOverlay::default()),
            EffectKind::GradientOverlay => Effect::GradientOverlay(GradientOverlay::default()),
            EffectKind::PatternOverlay => Effect::PatternOverlay(PatternOverlay::default()),
            EffectKind::BevelEmboss => Effect::BevelEmboss(BevelEmboss::default()),
            EffectKind::Satin => Effect::Satin(Satin::default()),
        }
    }

    /// This effect's reach in px: the farthest its plane lands beyond the
    /// shape, or the farthest an input change moves one of its outputs —
    /// whichever is larger. It is the input to [`LayerStyle::pad`], which
    /// sizes both the shape plane's padding and the cache's incremental
    /// re-render windows (`style_cache`), so an interior effect that reads
    /// inward still reports how far it reads. Distance enters only where
    /// the shift happens INSIDE the plane (inner shadow, satin); the drop
    /// shadow's shift is applied at composite time and needs no padding.
    pub(crate) fn reach(&self) -> u32 {
        let ceil = |v: f32| v.max(0.0).ceil() as u32;
        use crate::style_matte::{law_applies, law_reach};
        match self {
            // The law's band and box pair where Size·(1 − Spread) > 0
            // (`style_matte`); today's dilation (sigma 0, no blur) elsewhere.
            Effect::DropShadow(e) => {
                if law_applies(e.size, e.spread) {
                    law_reach(e.size, e.spread)
                } else {
                    let (dilate, sigma) =
                        crate::style_fx_drop_shadow::MATTE.split(e.size, e.spread);
                    ceil(dilate) + 1 + blur_reach(sigma)
                }
            }
            Effect::OuterGlow(e) => {
                if law_applies(e.size, e.spread) {
                    law_reach(e.size, e.spread)
                } else {
                    let (dilate, sigma) = crate::style_matte::OUTER_GLOW.split(e.size, e.spread);
                    ceil(dilate) + 1 + blur_reach(sigma)
                }
            }
            Effect::InnerShadow(e) => {
                let matte = if law_applies(e.size, e.choke) {
                    law_reach(e.size, e.choke)
                } else {
                    let (dilate, sigma) =
                        crate::style_fx_inner_shadow::MATTE.split(e.size, e.choke);
                    ceil(dilate) + 1 + blur_reach(sigma)
                };
                matte + ceil(e.distance)
            }
            // The measured law's band and box pair where Size·(1 − Choke)
            // > 0; the Choke 100 % member's band (today's, no blur) for an
            // edge glow and a centre glow below Size 24; today's band
            // (sigma 0, no blur) elsewhere (`style_fx_inner_glow`).
            Effect::InnerGlow(e) => {
                if law_applies(e.size, e.choke) {
                    law_reach(e.size, e.choke)
                } else if crate::style_fx_inner_glow::full_choke_applies(e.size, e.choke, e.source)
                {
                    crate::style_fx_inner_glow::full_choke_reach(e.size)
                } else {
                    let (dilate, sigma) = spread_split(e.size, e.choke);
                    ceil(dilate) + 1 + blur_reach(sigma)
                }
            }
            // An inside band is `coverage − erode(coverage, size)`: nothing
            // lands beyond the shape, but the erosion reads `size` px
            // inward, so an input change (an erased hole) moves outputs
            // that far — the same radius as the outside band's dilation.
            Effect::Stroke(e) => match e.position {
                StrokePosition::Outside | StrokePosition::Inside => ceil(e.size) + 1,
                StrokePosition::Center => ceil(e.size / 2.0) + 1,
            },
            // The measured law's box pairs and central difference for a
            // Smooth bevel; the Chisel law's facet, blur and box pair for
            // its shipped arms (`style_bevel_chisel`); today's height reach,
            // soften blur and stencil for every other Chisel
            // (`style_fx_bevel_emboss::reach`).
            Effect::BevelEmboss(e) => bevel_reach(e),
            // The law's box pair (size > 0) or today's blur at size 0, plus
            // the in-plane shift (`style_fx_satin`).
            Effect::Satin(e) => crate::style_fx_satin::satin_reach(e.size, e.distance),
            Effect::ColorOverlay(_) | Effect::GradientOverlay(_) | Effect::PatternOverlay(_) => 0,
        }
    }
}
