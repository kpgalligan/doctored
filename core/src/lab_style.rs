//! Layer effects and Blend If on a Lab Color document (#547 phases 2e and
//! 2f, `designs/lab-mode.md`): the refusals `LayerStyleEdit::target`,
//! the mode's gate (`lab_document::admit_edit`) and Mode ▸ Lab Color's
//! blocker (`lab_convert::first_lab_blocker`) ask, and the Lab pattern
//! tile ([`painted_sampler`], [`twin_tile_sampler`]).
//!
//! # Why the stored style is unchanged
//!
//! An effect's authored colours are sRGB numbers. On a Lab document they
//! convert at composite through `authored_color::AuthoredColors::LabTwin`
//! on the twin (Mode ▸ Lab Color's rule, the 8-bit `color8` for an
//! authored hex),
//! so the stored style, `get_document`'s `style`, the `.rz` body and the
//! render agree with no conversion when the style is stored, and RGB ↔ Lab
//! carries a style unchanged. The effects composite through the one style
//! path on the L\*a\*b\* codes, as the layers do, and each effect's colour
//! blends by its mode's Lab arm (`lab_blend`), chosen in
//! `style_render::Contribution::ink`.
//!
//! # What still refuses
//!
//! Only **an effect blend mode** `lab_document::blend_refusal` refuses (the
//! effect's own and a Bevel's two sides, `float_document::effect_blend_modes`,
//! disabled effects too: enabling one must not make a document that cannot
//! be stored). Pattern Overlay and Pattern strokes run since phase 2f's
//! second part.
//!
//! # The Lab pattern tile (phase 2f, second part)
//!
//! On a Lab document every pattern tile — a preset's or a custom `rgba` — is
//! AUTHORED sRGB, as an effect's colours are (the 2e rule, and how
//! `cmyk_style::effect_inks` reads a CMYK layer's texels). When the render
//! context says so (`RenderContext::lab_tiles`, from
//! `AuthoredColors::converts_tiles`), [`painted_sampler`] turns each texel
//! into 8-bit Lab codes by Mode ▸ Lab Color's rule (`authored_color::lut_ink`
//! on the twin: relative colorimetric, no BPC; alpha kept) BEFORE the pyramid
//! (`pattern_tile::tile_level_mapped`), so Scale's halvings and Angle's turn
//! box Lab values. The level is the size of RGB's, so
//! `style_pattern::level_bytes` stays exact. The stored tile stays sRGB, so
//! RGB ↔ Lab carries a pattern style byte-equal. A Bevel's texture is height,
//! not colour, and keeps `PatternFill::sampler`. Since phase 6a-i a Pattern
//! fill layer rendered on the twin (`artwork_render`) reads its tile through
//! the same conversion ([`twin_tile_sampler`], the one implementation
//! [`painted_sampler`] calls).
//!
//! # Blend If (phase 2f)
//!
//! A Lab document's Blend If channels are Lightness, a and b (not measured
//! against Photoshop). They are the stored Red, Green and Blue channels
//! ([`BLEND_IF_CHANNELS`]), whose ramps `style_blend_if::source_weight_16`
//! evaluates on the stored codes unchanged: L\*, a\* and b\* on the 0…255
//! slider scale (a\* = 0 at 128). RGB ↔ Lab carries a red, green or blue
//! range by channel index; Gray (and a CMYK plate) refuses by name
//! ([`blend_if_refusal`], [`blend_if_channel_refusal`]).

use std::sync::{Arc, OnceLock};

use crate::icc_intent::ColorPipeline;
use crate::pattern_tile::{tile_level_mapped, PatternSampler};
use crate::style::{BlendIf, BlendIfChannel, LayerStyle};
use crate::style_pattern_fill::PatternFill;
use crate::style_render::RenderContext;

/// The Blend If channels a Lab document has: Lightness, a and b, stored as
/// Red, Green and Blue (module doc).
pub(crate) const BLEND_IF_CHANNELS: &[BlendIfChannel] = &[
    BlendIfChannel::Red,
    BlendIfChannel::Green,
    BlendIfChannel::Blue,
];

/// A stored Blend If channel's name on a Lab document: Red, Green and Blue
/// are Lightness, a and b ([`BLEND_IF_CHANNELS`]); any other keeps its own
/// (Lab → CMYK's blocker, `lab_cmyk`).
pub(crate) fn channel_name(channel: BlendIfChannel) -> &'static str {
    match channel {
        BlendIfChannel::Red => "Lightness",
        BlendIfChannel::Green => "a",
        BlendIfChannel::Blue => "b",
        other => other.display_name(),
    }
}

fn channel_sentence(channel: BlendIfChannel) -> String {
    format!(
        "Blend If on the {} channel is unavailable in a Lab Color document: its Blend If channels are Lightness, a and b.",
        channel.display_name()
    )
}

/// The refusal of a `blend_if` request value naming a channel outside
/// [`BLEND_IF_CHANNELS`] in any range as written, an identity one included
/// (`style_json::requested_blend_if_channels`); `None` for null or Lab
/// channels only.
pub(crate) fn blend_if_refusal(blend_if: &serde_json::Value) -> Option<String> {
    crate::style_json::requested_blend_if_channels(blend_if)
        .into_iter()
        .find(|channel| !BLEND_IF_CHANNELS.contains(channel))
        .map(channel_sentence)
}

/// [`blend_if_refusal`] over a stored record's ranges: what RGB/Gray → Lab
/// asks of a layer's or a style's Blend If.
pub(crate) fn blend_if_channel_refusal(blend_if: &BlendIf) -> Option<String> {
    blend_if
        .ranges()
        .map(|range| range.channel)
        .find(|channel| !BLEND_IF_CHANNELS.contains(channel))
        .map(channel_sentence)
}

/// The first effect blend mode `lab_document::blend_refusal` refuses
/// (disabled effects too, via `float_document::effect_blend_modes`); None
/// otherwise.
pub(crate) fn style_refusal(style: &LayerStyle) -> Option<String> {
    style.effects.iter().find_map(|effect| {
        crate::float_document::effect_blend_modes(effect)
            .into_iter()
            .flatten()
            .find_map(crate::lab_document::blend_refusal)
    })
}

/// sRGB → the Lab twin by Mode ▸ Lab Color's rule (module doc), built once;
/// `None` when the pipeline cannot build.
fn twin_pipeline() -> Option<&'static ColorPipeline> {
    static PIPELINE: OnceLock<Option<ColorPipeline>> = OnceLock::new();
    PIPELINE
        .get_or_init(|| crate::authored_color::lut_ink(&crate::lab_document::lab_twin(), true))
        .as_ref()
}

/// The placed level a Pattern Overlay's or a Pattern stroke's colour and
/// coverage read (module doc's Lab pattern tile): on a Lab layer
/// (`ctx.lab_tiles`) each sRGB texel becomes 8-bit Lab codes before the
/// pyramid; otherwise, or when the twin pipeline cannot build, it is
/// `fill.sampler` unchanged.
pub(crate) fn painted_sampler(fill: &PatternFill, ctx: &RenderContext) -> PatternSampler {
    ctx.lab_tiles
        .then(|| twin_tile_sampler(fill, ctx.layer_offset))
        .flatten()
        .unwrap_or_else(|| fill.sampler(ctx.layer_offset))
}

/// `fill`'s placed level with each sRGB texel as 8-bit Lab codes by Mode ▸
/// Lab Color's rule before the pyramid (module doc's Lab pattern tile), at
/// `layer_offset`; `None` when the twin pipeline cannot build. The one
/// conversion [`painted_sampler`] and a Pattern fill layer on the twin
/// (`artwork_render`, phase 6a-i) share.
pub(crate) fn twin_tile_sampler(
    fill: &PatternFill,
    layer_offset: (i32, i32),
) -> Option<PatternSampler> {
    let pipeline = twin_pipeline()?;
    Some(PatternSampler {
        level: Arc::new(tile_level_mapped(
            &fill.pattern.tile,
            f64::from(fill.scale),
            f64::from(fill.angle),
            &mut |rgb| pipeline.color8(rgb),
        )),
        origin: fill.origin(layer_offset),
    })
}
