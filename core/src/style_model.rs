//! The layer-style MODEL's shared value types: the document's global
//! light, Blend If, the gradient fill, the small enums every effect pane
//! offers, and the parser's strictness switch. The nine effect structs live
//! in `style_effects`, the `LayerStyle` itself (with the identity rule, the
//! padding arithmetic and Scale Effects) in `style`, which re-exports
//! everything here so `style::GlobalLight` stays the public path. The JSON
//! schema table is `style`'s module doc.

use crate::style_json::{canonical_angle, RANGES};

/// Direction of the document's shared light source, in degrees: `angle`
/// (0 = from the right, 90 = from the top) and `altitude` above the canvas
/// plane (90 = straight down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlobalLight {
    pub angle: f32,
    pub altitude: f32,
}

impl Default for GlobalLight {
    fn default() -> Self {
        GlobalLight {
            angle: 120.0,
            altitude: 30.0,
        }
    }
}

impl GlobalLight {
    /// The ONE sanitizer, used by the setter, the DRDC reader and the FFI: a
    /// non-finite component takes its default, altitude clamps to [0, 90],
    /// angle normalizes to [-180, 180), and both are quantized to four
    /// decimals through the style parser's own canonicalizers
    /// (`style_json::canonical_angle`, the `RANGES` clamp table) — so the
    /// stored light IS its canonical form exactly like every style number,
    /// and a host that echoes the reported four-decimal value back through
    /// the setter is refused as unchanged instead of registering a phantom
    /// edit. Sanitized values compare with plain `==` (the cache key relies
    /// on it).
    pub fn sane(self) -> Self {
        let default = GlobalLight::default();
        let angle = if self.angle.is_finite() {
            canonical_angle(f64::from(self.angle))
        } else {
            default.angle
        };
        let altitude = if self.altitude.is_finite() {
            RANGES.altitude.apply(f64::from(self.altitude))
        } else {
            default.altitude
        };
        GlobalLight { angle, altitude }
    }
}

// ------------------------------------------------------------- blend if --

/// Which channel Blend If evaluates its ramps on. Gray serves every mode;
/// Red, Green and Blue an RGB document's colour, and Cyan, Magenta, Yellow
/// and Black a CMYK document's plates (#1, `style_blend_if::source_weight_ink`).
/// The declaration order is the canonical order of a multichannel Blend If's
/// ranges ([`BlendIf::from_ranges`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum BlendIfChannel {
    #[default]
    Gray,
    Red,
    Green,
    Blue,
    Cyan,
    Magenta,
    Yellow,
    Black,
}

impl BlendIfChannel {
    /// One of the four CMYK plates.
    pub fn is_cmyk(self) -> bool {
        matches!(
            self,
            BlendIfChannel::Cyan
                | BlendIfChannel::Magenta
                | BlendIfChannel::Yellow
                | BlendIfChannel::Black
        )
    }

    /// One of the three RGB colour channels.
    pub fn is_rgb(self) -> bool {
        matches!(
            self,
            BlendIfChannel::Red | BlendIfChannel::Green | BlendIfChannel::Blue
        )
    }

    /// The channel's name as the Blending Options pop-up spells it, for a
    /// refusal sentence.
    pub fn display_name(self) -> &'static str {
        match self {
            BlendIfChannel::Gray => "Gray",
            BlendIfChannel::Red => "Red",
            BlendIfChannel::Green => "Green",
            BlendIfChannel::Blue => "Blue",
            BlendIfChannel::Cyan => "Cyan",
            BlendIfChannel::Magenta => "Magenta",
            BlendIfChannel::Yellow => "Yellow",
            BlendIfChannel::Black => "Black",
        }
    }
}

/// One channel's Blend If ranges: two split-slider pairs `[lo0, lo1, hi0,
/// hi1]` over 0..=255 on `channel` — one evaluated on this layer's pixels,
/// one on the composite beneath.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlendIfRange {
    pub channel: BlendIfChannel,
    pub this_layer: [u8; 4],
    pub underlying: [u8; 4],
}

impl BlendIfRange {
    /// True when both ramps weight every value 1 (the channel is then moot).
    pub fn is_identity(&self) -> bool {
        self.this_layer == FULL_RAMP && self.underlying == FULL_RAMP
    }
}

/// How many ranges a Blend If holds beside its primary one: every channel
/// once (eight channels, the primary being one of them), so a canonical
/// value always fits.
pub const MAX_EXTRA_RANGES: usize = 7;

/// Blending Options > Blend If: the PRIMARY range (`channel`, `this_layer`,
/// `underlying`, the single-channel model every surface shipped with) plus
/// up to [`MAX_EXTRA_RANGES`] more on other channels, whose
/// per-range weights MULTIPLY (`style_blend_if`). Canonical form
/// ([`BlendIf::from_ranges`]): no channel twice, no identity range beside
/// another, the ranges in [`BlendIfChannel`] order, `extra` filled from
/// index 0 — so a single-channel value has every `extra` slot `None` and
/// is the old struct exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlendIf {
    pub channel: BlendIfChannel,
    pub this_layer: [u8; 4],
    pub underlying: [u8; 4],
    pub extra: [Option<BlendIfRange>; MAX_EXTRA_RANGES],
}

/// The ramp that weights every value 1.
pub const FULL_RAMP: [u8; 4] = [0, 0, 255, 255];

impl Default for BlendIf {
    fn default() -> Self {
        BlendIf::single(BlendIfChannel::Gray, FULL_RAMP, FULL_RAMP)
    }
}

impl BlendIf {
    /// A single-channel Blend If (no extra range).
    pub fn single(channel: BlendIfChannel, this_layer: [u8; 4], underlying: [u8; 4]) -> BlendIf {
        BlendIf {
            channel,
            this_layer,
            underlying,
            extra: [None; MAX_EXTRA_RANGES],
        }
    }

    /// The canonical value of `first` then `rest` as written: a channel
    /// named twice is an error (`blend_if names the <Display> channel
    /// twice`); identity ranges are dropped; when none remains the result
    /// is `first` alone (the identity, carrying its channel as today);
    /// otherwise the remaining ranges in [`BlendIfChannel`] order, the
    /// first of them the primary.
    pub fn from_ranges(first: BlendIfRange, rest: &[BlendIfRange]) -> Result<BlendIf, String> {
        let all: Vec<BlendIfRange> = std::iter::once(first).chain(rest.iter().copied()).collect();
        for (i, range) in all.iter().enumerate() {
            if all[..i].iter().any(|r| r.channel == range.channel) {
                return Err(format!(
                    "blend_if names the {} channel twice",
                    range.channel.display_name()
                ));
            }
        }
        let mut kept: Vec<BlendIfRange> = all.into_iter().filter(|r| !r.is_identity()).collect();
        kept.sort_by_key(|r| r.channel);
        let Some((primary, others)) = kept.split_first() else {
            return Ok(BlendIf::single(
                first.channel,
                first.this_layer,
                first.underlying,
            ));
        };
        // Distinct channels number at most eight, so `others` fits.
        let mut out = BlendIf::single(primary.channel, primary.this_layer, primary.underlying);
        for (slot, range) in out.extra.iter_mut().zip(others) {
            *slot = Some(*range);
        }
        Ok(out)
    }

    /// The primary range.
    pub fn primary(&self) -> BlendIfRange {
        BlendIfRange {
            channel: self.channel,
            this_layer: self.this_layer,
            underlying: self.underlying,
        }
    }

    /// Every range: the primary, then the extras in order.
    pub fn ranges(&self) -> impl Iterator<Item = BlendIfRange> + '_ {
        std::iter::once(self.primary()).chain(self.extra.iter().flatten().copied())
    }

    /// True when the value holds more than its primary range.
    pub fn is_multichannel(&self) -> bool {
        self.extra.iter().any(Option::is_some)
    }

    /// True when every range weights every value 1.
    pub fn is_identity(&self) -> bool {
        self.ranges().all(|r| r.is_identity())
    }

    /// This value without its CMYK-plate ranges, canonical; `None` when no
    /// range remains (what a byte document's reader keeps of a Blend If a
    /// newer file carries, #1).
    pub fn without_cmyk(self) -> Option<BlendIf> {
        let kept: Vec<BlendIfRange> = self.ranges().filter(|r| !r.channel.is_cmyk()).collect();
        let (first, rest) = kept.split_first()?;
        BlendIf::from_ranges(*first, rest).ok()
    }
}

// -------------------------------------------------------------- gradients --

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
    pub position: f32,
    pub color: [u8; 3],
    pub opacity: f32,
    /// A CMYK document's stop colour as straight ink `[c, m, y, k]` in
    /// `0..=1`, four-decimal quantized; `None` = none, so the stop's `color`
    /// converts through the profile at composite time (#1, `cmyk_style`).
    pub ink: Option<[f32; 4]>,
    /// The colour midpoint toward the NEXT stop as a fraction
    /// in `gradient_field::MIDPOINT_MIN..=MIDPOINT_MAX`, 0.01 steps: where
    /// the colour reaches half way (`gradient_field::midpoint_fraction`).
    /// 0.5, the default, is written nowhere; the last stop's is always 0.5.
    pub midpoint: f32,
    /// The opacity midpoint toward the next stop, as `midpoint`.
    pub opacity_midpoint: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GradientStyle {
    #[default]
    Linear,
    Radial,
    Angle,
    Reflected,
    Diamond,
    /// The Stroke's own style: `t` follows the band's distance across
    /// its width (`style_stroke_fill`), never the box. The parser admits it
    /// on a stroke only (`style_json::effect_gradient`).
    ShapeBurst,
}

/// A gradient fill shared by Stroke (gradient fill type) and Gradient
/// Overlay; see the module doc for the key table.
#[derive(Clone, Debug, PartialEq)]
pub struct GradientFill {
    pub stops: Vec<GradientStop>,
    pub style: GradientStyle,
    pub angle: f32,
    pub scale: f32,
    pub reverse: bool,
    pub align_with_layer: bool,
    /// Dither: `t` jittered by the ordered threshold at
    /// half a code of the document's depth (`style_gradient`). Written only
    /// when true.
    pub dither: bool,
    /// Smoothness as a fraction: 1 (100 %, the default) is
    /// the Classic ramp, 0 the linear one, between them the two blended by
    /// it (`gradient_field::smooth_color`). Written only when not 1.
    pub smoothness: f32,
}

impl Default for GradientFill {
    fn default() -> Self {
        GradientFill {
            stops: vec![
                GradientStop {
                    position: 0.0,
                    color: [0, 0, 0],
                    opacity: 1.0,
                    ink: None,
                    midpoint: 0.5,
                    opacity_midpoint: 0.5,
                },
                GradientStop {
                    position: 1.0,
                    color: [255, 255, 255],
                    opacity: 1.0,
                    ink: None,
                    midpoint: 0.5,
                    opacity_midpoint: 0.5,
                },
            ],
            style: GradientStyle::Linear,
            angle: 90.0,
            scale: 1.0,
            reverse: false,
            align_with_layer: true,
            dither: false,
            smoothness: 1.0,
        }
    }
}

// ---------------------------------------------------------- effect enums --

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GlowSource {
    #[default]
    Edge,
    Center,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrokePosition {
    #[default]
    Outside,
    Inside,
    Center,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StrokeFill {
    #[default]
    Color,
    Gradient,
    Pattern,
}

/// What an Outer or Inner Glow paints with: its `color`, or its
/// `gradient` ramped across the glow (`style_glow_gradient`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GlowFill {
    #[default]
    Color,
    Gradient,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BevelStyle {
    OuterBevel,
    #[default]
    InnerBevel,
    Emboss,
    PillowEmboss,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BevelDirection {
    #[default]
    Up,
    Down,
}

/// Bevel & Emboss > Technique (#2): Smooth's height is a blur of the
/// coverage, the two Chisels' a clamped distance ramp
/// (`style_fx_bevel_emboss`'s module doc, `# Technique`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BevelTechnique {
    #[default]
    Smooth,
    ChiselHard,
    ChiselSoft,
}

// -------------------------------------------------------------- knockout --

/// Blending Options > Knockout: whether the layer's own shape punches a hole
/// through a BACKDROP instead of compositing onto it, and how far down that
/// backdrop is taken (`style_knockout`, which owns the whole rule).
///
/// `None` is today's compositing exactly. `Shallow` stops at the enclosing
/// ISOLATED group's buffer, or at the accumulator a PASS-THROUGH group's
/// children began with; `Deep` goes to the Background layer (`style_knockout`'s
/// D5 rule), stopping at an isolated group's buffer all the same. At the ROOT
/// the two are the same thing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Knockout {
    #[default]
    None,
    Shallow,
    Deep,
}

/// Strict = the FFI setter (an unknown `type`/enum value is malformed);
/// Lenient = the DRDC reader (unknown types skipped, unknown enum values
/// defaulted — a file from a newer build keeps the effects this build knows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strictness {
    Strict,
    Lenient,
}
