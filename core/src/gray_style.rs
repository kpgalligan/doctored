//! Layer effects and Blend If on a Grayscale document (#1 phase 2d,
//! `designs/grayscale-mode.md`): the greying of a style's colours,
//! the effect-mode and Blend If refusals, RGB → Gray's style pass (a
//! recorded comp style's too, phase 6a-ii: [`greyed_json`]) and the open
//! check.
//!
//! # Why the RGB style path, unchanged
//!
//! A Gray document's storage is R = G = B (`gray_document`'s module doc). A
//! style whose every colour is neutral therefore composites through the
//! existing RGB style path (`precision_style_render`) on equal channels and
//! lands on equal channels: no one-channel kernel and no fold of the styled
//! composite. What makes a colour neutral is decided ONCE, when the style is
//! stored ([`greyed`], asked by `LayerStyleEdit::target`), as a Gradient
//! Map's stops are (`gray_adjust`), so the stored style, `get_document`'s
//! `style`, the `.rz` body and the render agree, and every caller of the
//! style edit (MCP, presets, Paste Layer Style, the sheet's preview, Actions)
//! meets the one rule.
//!
//! # The rules
//!
//! * **An authored hex** (an effect's `color`, a bevel side, a gradient
//!   stop) is an sRGB byte. One with r = g = b is already gray and stays;
//!   any other becomes `[v, v, v]` with `Y` sRGB's relative PCS luminance
//!   of it (`cmyk_color::document_xyz` through the built-in sRGB profile,
//!   Mode ▸ Grayscale's rule) and `v = round(255·sRGB(clamp(Y, 0, 1)))`:
//!   the sRGB byte of the same luminance, so the composite's conversion into
//!   the document carries it to the Gray profile's code of `Y`.
//! * **A wide slot** (`linear`) that is not neutral greys to
//!   `[Y, Y, Y]`, `Y` its luminance through sRGB's matrix
//!   (`MatrixTrc::linear_to_pcs`, extended range kept), stored through
//!   `style_json::normalized_wide` — a `[Y, Y, Y]` that IS an sRGB byte
//!   becomes that hex with no `linear` — so the stored style is a fixed
//!   point of its own JSON and re-setting it is the exact no-op.
//! * **A custom pattern tile** (a Pattern Overlay's, a stroke's
//!   `pattern_fill`) holds DOCUMENT values (`pattern_tile`). A texel becomes,
//!   alpha kept, the gray code of its luminance through the profile the
//!   tile's values are in. On a Gray document's edit that profile is the
//!   twin, whose neutral texels are already their gray code, so only a texel
//!   that is not neutral is mapped; RGB → Gray maps EVERY texel of a custom
//!   tile, neutral ones included, because the source profile's neutral `v`
//!   is a different tone than the Gray profile's (sRGB 128 is about Gray
//!   Gamma 2.2's 127, and further off in Dot Gain 20%), as the layer pixels
//!   are re-encoded through `Y`. The mapping: at 8 and 16 bits
//!   `round(GrayPolicy::luminance_code / 257)`, at 32 bits (whose tile
//!   values are linear) `round(255·clamp(GrayPolicy::linear_luminance, 0,
//!   1))`. The presets are neutral already. A bevel's texture is height,
//!   not colour: untouched.
//! * **Effect blend modes** are a layer's: the Grayscale list
//!   (`gray_document::blend_refusal`, same sentence); at 32 bits the float
//!   document's own style refusal runs first (`LayerStyleEdit::target`).
//! * **Blend If** is Gray only: any other
//!   channel in any range as written, an identity ramp included, refuses
//!   by name ([`blend_if_refusal`]).
//!
//! A greyed tile copy (at most 512² texels, 1 MiB) sits inside the style
//! request's parse charge (`ffi_precision_edit::request`); RGB → Gray
//! charges the exact bytes of the tiles it greys, every custom tile
//! ([`tile_bytes`]).

use std::sync::Arc;

use serde_json::Value;

use crate::gray_document::{GrayDepth, GrayPolicy};
use crate::icc::IccProfile;
use crate::precision_document::{Content, PrecisionDocumentLayer};
use crate::style::{BlendIfChannel, Effect, LayerStyle, Pattern, PatternSource, PatternTile};
use crate::style_cache::RenderCache;

/// The open check's refusal of a style colour that is not gray.
const NOT_GRAY: &str = "a Grayscale document's layer style colours must be gray";

/// The sRGB value (0…1, unrounded) of the luminance of `rgb`, sRGB 0…1:
/// `Y` through the built-in sRGB profile (`cmyk_color::document_xyz`, Mode
/// ▸ Grayscale's rule), clamped to 0…1 and sRGB-encoded. [`grey_srgb`]
/// rounds it to a byte; the artwork colours (`gray_artwork`) keep it.
pub(crate) fn srgb_gray(rgb: [f64; 3]) -> f64 {
    let y = crate::cmyk_color::document_xyz(&IccProfile::srgb(), rgb)[1];
    f64::from(crate::adjust_math::linear_to_srgb(y.clamp(0.0, 1.0) as f32))
}

/// The luminance `Y` of a linear sRGB triple through sRGB's matrix
/// (`MatrixTrc::linear_to_pcs`, extended range kept); `None` when the
/// built-in profile has no model. [`grey_wide`]'s and `gray_artwork`'s.
pub(crate) fn linear_gray(linear: [f64; 3]) -> Option<f64> {
    let srgb = IccProfile::srgb();
    let model = srgb.model()?;
    Some(model.linear_to_pcs(linear)[1])
}

/// `rgb`, an sRGB byte, as the sRGB byte of its luminance (module doc);
/// r = g = b is returned as it is.
pub(crate) fn grey_srgb(rgb: [u8; 3]) -> [u8; 3] {
    if rgb[0] == rgb[1] && rgb[1] == rgb[2] {
        return rgb;
    }
    let v = srgb_gray(rgb.map(|c| f64::from(c) / 255.0));
    let v = (v * 255.0).round().clamp(0.0, 255.0) as u8;
    [v; 3]
}

/// One wide slot greyed (module doc): a neutral `linear` stays with its
/// slot's `color`; any other becomes `normalized_wide([Y, Y, Y])`.
fn grey_wide(linear: [f32; 3], color: [u8; 3]) -> ([u8; 3], Option<[f32; 3]>) {
    if linear[0] == linear[1] && linear[1] == linear[2] {
        return (color, Some(linear));
    }
    let Some(y) = linear_gray(linear.map(f64::from)) else {
        return (color, Some(linear));
    };
    let y = y as f32;
    crate::style_json::normalized_wide([y, y, y])
}

/// A gray PSD's `Grsc` effect colour (#1 phase 3a): `Gry ` is Photoshop's
/// K percent, so the code is `c = 1 − K/100` in the file's Gray profile; its
/// luminance `Y` through the twin (`cmyk_color::document_xyz`) is stored as a
/// style slot is, `style_json::normalized_wide([Y, Y, Y])` — the sRGB byte
/// of that luminance, with a wide `linear` only off the byte grid.
pub(crate) fn grsc_colour(policy: &GrayPolicy, k_percent: f64) -> ([u8; 3], Option<[f32; 3]>) {
    let c = (1.0 - k_percent / 100.0).clamp(0.0, 1.0);
    let y = crate::cmyk_color::document_xyz(policy.twin(), [c; 3])[1] as f32;
    crate::style_json::normalized_wide([y; 3])
}

/// The inverse of [`grsc_colour`] for a gray PSD's export (#1 phase 3b): a
/// slot's luminance `Y` (its `linear` when it has one, else the sRGB decode
/// of its byte — a stored slot is gray, so channel 0 is its luminance) as the
/// Gray profile's K percent, `100·(1 − encode(Y))`, to four decimals.
pub(crate) fn grsc_k_percent(policy: &GrayPolicy, rgb: [u8; 3], linear: Option<[f32; 3]>) -> f64 {
    let y = match linear {
        Some(linear) => f64::from(linear[0]),
        None => f64::from(crate::adjust_math::srgb_to_linear(
            f32::from(rgb[0]) / 255.0,
        )),
    };
    let k = 100.0 * (1.0 - policy.encode(y.clamp(0.0, 1.0)));
    (k * 1e4).round() / 1e4
}

/// Whether some texel of `tile` is not neutral.
fn coloured(tile: &PatternTile) -> bool {
    tile.rgba
        .chunks_exact(4)
        .any(|t| t[0] != t[1] || t[1] != t[2])
}

/// `tile`'s texels greyed (module doc), alpha kept; `tiles_in` is the
/// profile its values are in, and `every` maps neutral texels too (RGB →
/// Gray) instead of keeping them.
fn grey_tile(
    tile: &PatternTile,
    tiles_in: &IccProfile,
    policy: &GrayPolicy,
    every: bool,
) -> Result<PatternTile, String> {
    let float = policy.depth() == GrayDepth::ThirtyTwo;
    let mut rgba = Vec::with_capacity(tile.rgba.len());
    for texel in tile.rgba.chunks_exact(4) {
        let v = if !every && texel[0] == texel[1] && texel[1] == texel[2] {
            texel[0]
        } else {
            let unit = [texel[0], texel[1], texel[2]].map(|c| f64::from(c) / 255.0);
            let v = if float {
                255.0 * GrayPolicy::linear_luminance(tiles_in, unit)?.clamp(0.0, 1.0)
            } else {
                f64::from(policy.luminance_code(tiles_in, unit)) / 257.0
            };
            v.round().clamp(0.0, 255.0) as u8
        };
        rgba.extend_from_slice(&[v, v, v, texel[3]]);
    }
    Ok(PatternTile {
        width: tile.width,
        height: tile.height,
        rgba: Arc::from(rgba),
    })
}

/// `pattern` with its tile greyed when it is a custom tile with a colour
/// texel (any custom tile when `every`); else the same pattern. Also a
/// pattern fill layer's tile (`gray_artwork`).
pub(crate) fn grey_pattern(
    pattern: &Pattern,
    tiles_in: &IccProfile,
    policy: &GrayPolicy,
    every: bool,
) -> Result<Pattern, String> {
    if !matches!(pattern.source, PatternSource::Custom { .. })
        || (!every && !coloured(&pattern.tile))
    {
        return Ok(pattern.clone());
    }
    Ok(Pattern {
        source: pattern.source.clone(),
        tile: Arc::new(grey_tile(&pattern.tile, tiles_in, policy, every)?),
    })
}

/// The custom tiles the greying reaches: a Pattern Overlay's and every
/// stroke's `pattern_fill` (the bevel's texture is height, module doc).
fn custom_tiles(style: &LayerStyle) -> impl Iterator<Item = &Pattern> {
    style
        .effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::PatternOverlay(p) => Some(&p.fill.pattern),
            Effect::Stroke(s) => Some(&s.pattern_fill.pattern),
            _ => None,
        })
        .filter(|p| matches!(p.source, PatternSource::Custom { .. }))
}

/// `style` with every colour greyed (module doc): wide slots, hexes and
/// gradient stops, custom tiles; a fresh cache. An already-gray style
/// comes back equal. The edit path's rule: a neutral tile texel stays.
pub(crate) fn greyed(
    style: &LayerStyle,
    tiles_in: &IccProfile,
    policy: &GrayPolicy,
) -> Result<LayerStyle, String> {
    greyed_with(style, tiles_in, policy, false)
}

/// [`greyed`], with `every` mapping every texel of a custom tile (RGB →
/// Gray, module doc).
fn greyed_with(
    style: &LayerStyle,
    tiles_in: &IccProfile,
    policy: &GrayPolicy,
    every: bool,
) -> Result<LayerStyle, String> {
    let wide = style.map_wide_slots(&mut grey_wide);
    let mut out = wide.map_colour_slots(&mut |ink, color| Ok((ink, grey_srgb(color))))?;
    let mut effects = Vec::with_capacity(out.effects.len());
    for effect in &out.effects {
        effects.push(match effect {
            Effect::PatternOverlay(p) => {
                let mut p = p.clone();
                p.fill.pattern = grey_pattern(&p.fill.pattern, tiles_in, policy, every)?;
                Effect::PatternOverlay(p)
            }
            Effect::Stroke(s) => {
                let mut s = s.clone();
                s.pattern_fill.pattern =
                    grey_pattern(&s.pattern_fill.pattern, tiles_in, policy, every)?;
                Effect::Stroke(s)
            }
            other => other.clone(),
        });
    }
    out.effects = effects;
    out.cache = RenderCache::default();
    Ok(out)
}

/// A style's canonical JSON with every colour greyed as RGB → Gray greys a
/// layer's style (every custom tile texel mapped, `tiles_in` the source
/// profile): a recorded comp style's carry (`gray_comps`).
pub(crate) fn greyed_json(
    json: &str,
    tiles_in: &IccProfile,
    policy: &GrayPolicy,
) -> Result<String, String> {
    let style = LayerStyle::from_json(json)?;
    Ok(greyed_with(&style, tiles_in, policy, true)?.to_json())
}

/// The first effect blend mode a Gray document refuses (`float` for a
/// 32-bit one), as a layer mode's sentence (`gray_document::blend_refusal`);
/// disabled effects included, as the float document's own rule reads them.
pub(crate) fn style_refusal(style: &LayerStyle, float: bool) -> Option<String> {
    style.effects.iter().find_map(|effect| {
        crate::float_document::effect_blend_modes(effect)
            .into_iter()
            .flatten()
            .find_map(|mode| crate::gray_document::blend_refusal(mode, float))
    })
}

pub(crate) fn channel_sentence(channel: BlendIfChannel) -> String {
    format!(
        "Blend If on the {} channel is unavailable in a Grayscale document.",
        channel.display_name()
    )
}

/// The refusal of a `blend_if` request value naming a channel other than
/// Gray in any range as written, an identity one included
/// (`style_json::requested_blend_if_channels`); `None` for null or Gray only.
pub(crate) fn blend_if_refusal(blend_if: &Value) -> Option<String> {
    crate::style_json::requested_blend_if_channels(blend_if)
        .into_iter()
        .find(|channel| *channel != BlendIfChannel::Gray)
        .map(channel_sentence)
}

/// Every record's style (groups recursed) greyed in place: RGB → Gray's
/// style pass, `tiles_in` the source profile, every custom tile texel
/// mapped (module doc).
pub(crate) fn grey_records(
    layers: &mut [PrecisionDocumentLayer],
    tiles_in: &IccProfile,
    policy: &GrayPolicy,
) -> Result<(), String> {
    for layer in layers.iter_mut() {
        if let Some(style) = layer.style.as_deref() {
            let grey = greyed_with(style, tiles_in, policy, true)?;
            if grey != *style {
                layer.style = Some(Arc::new(grey));
            }
        }
        if let Content::Group(children) | Content::PassThrough(children) = &mut layer.content {
            grey_records(Arc::make_mut(children).as_mut_slice(), tiles_in, policy)?;
        }
    }
    Ok(())
}

/// The bytes RGB → Gray's style pass allocates: `w·h·4` for every custom
/// tile (each is mapped, neutral or not), groups recursed.
pub(crate) fn tile_bytes(layers: &[PrecisionDocumentLayer]) -> u64 {
    layers
        .iter()
        .map(|layer| {
            let own: u64 = layer.style.as_deref().map_or(0, |style| {
                custom_tiles(style)
                    .map(|p| 4 * u64::from(p.tile.width) * u64::from(p.tile.height))
                    .sum()
            });
            own + layer.children().map_or(0, tile_bytes)
        })
        .sum()
}

/// The open check (`PrecisionDocument::with_gray`): every record (groups
/// recursed) holds an effect mode the Grayscale list offers
/// ([`style_refusal`], `float` at depth 32), a Blend If on Gray only, and a
/// style already gray (its greying leaves its JSON unchanged).
pub(crate) fn check_records(
    layers: &[PrecisionDocumentLayer],
    tiles_in: &IccProfile,
    policy: &GrayPolicy,
) -> Result<(), String> {
    let float = policy.depth() == GrayDepth::ThirtyTwo;
    for layer in layers {
        if let Some(style) = layer.style.as_deref() {
            if let Some(reason) = style_refusal(style, float) {
                return Err(reason);
            }
            if greyed(style, tiles_in, policy)?.to_json() != style.to_json() {
                return Err(NOT_GRAY.into());
            }
        }
        if let Some(range) = layer
            .blend_if
            .and_then(|b| b.ranges().find(|r| r.channel != BlendIfChannel::Gray))
        {
            return Err(channel_sentence(range.channel));
        }
        if let Some(children) = layer.children() {
            check_records(children, tiles_in, policy)?;
        }
    }
    Ok(())
}
