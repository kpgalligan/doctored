//! A CMYK document's LAYER STYLES (#547, the styles batch): the adapter
//! between a styled CMYK record and the row kernel `cmyk_style_row`, and the
//! style's INKS.
//!
//! # Inks
//!
//! Every colour slot of a style may carry its ink (`style`'s module doc, the
//! CMYK INKS paragraph). On a CMYK document the ink is AUTHORITATIVE:
//! [`canonical_hex`] rewrites each inked slot's `#rrggbb` to the ink's sRGB
//! under the document's profile (relative colorimetric with BPC) at the
//! edit, at Assign Profile and at PSD import, so the hex a host shows is
//! the colour the ink prints. A slot WITHOUT an ink (a style authored in
//! hex, an RGB style carried over) converts its hex at composite time
//! ([`effect_inks`]) and at PSD export ([`with_all_inks`]), both through
//! `cmyk_color::RgbToCmyk`, the document's ONE sRGB → ink leg.
//!
//! Photoshop take1 (`scripts/photoshop-probe/cmyk_styles_take.py`,
//! `cmyk-styles-547/take1`) holds an 8-bit document's effect colour as its
//! 8-bit code — 90 % is code 229, the double product `round(p·2.55)` — so on
//! an 8-bit document [`effect_inks`] puts each ink on that grid by the same
//! arithmetic. A 16-bit document keeps the ink as stored.
//!
//! # The styled record
//!
//! [`styled_ink`] builds the kernel's [`StyledInk`] for one record: the SHAPE
//! is `precision_style_render::typed_shape`'s rule on ink —
//! `typed_coverage_byte(a16, mask)` over the record's alpha (a raster's
//! stored code, a group's rendered f32 alpha × 65535) on a plane padded by
//! the style's pad ([`cmyk_shape`], whose master is `typed_shape`); the planes
//! are the SHIPPED u8 renderer's (`style_render::render_effects_with_job`)
//! under the document's global light and canvas; each contribution's colour
//! is resolved to ink once. A styled GROUP is isolated: `cmyk_stack` renders
//! its children into an ink frame first ([`StyledSource::Frame`]), the
//! typed renderer's `StyledPixels::Buffer`. A styled ADJUSTMENT is
//! [`StyledSource::Opaque`] over the stack's read rect under its
//! mask (S-MASK, the typed `StyledPixels::Opaque`), and its tone rides in
//! [`StyledInk`]'s `tone` for the kernel's adjustment interior.
//!
//! [`styled_scratch`] is what a styled record holds beside the stack: the
//! typed renderer's own plan for the planes (`style_scratch`), a group's
//! frame (20 B a pixel) and the kernel's row buffers.

use crate::blend::BlendMode;
use crate::cmyk_color::{ink_to_srgb, ConversionOptions, RgbToCmyk};
use crate::cmyk_composite::{unit, InkPixel};
use crate::cmyk_document::{CmykDepth, CmykPolicy};
use crate::cmyk_image::CmykImage;
use crate::cmyk_style_row::{InkColor, InkTone, StyledInk};
use crate::doc::MAX_PIXELS;
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::pattern_tile::{ink_sampler, InkSampler};
use crate::precision_document::{Content, PrecisionDocumentLayer};
use crate::precision_mask::PreparedMask;
use crate::precision_style_render::{plane_pixels, style_scratch, typed_coverage_byte};
use crate::style::{Effect, EffectKind, GlobalLight, Knockout, LayerStyle};
use crate::style_composite::occluding_fill;
use crate::style_json::q4;
use crate::style_pattern::ink_level_bytes;
use crate::style_pattern_fill::pattern_fill;
use crate::style_render::{
    render_effects_with_job, unit_color, Part, PlaneColor, RenderContext, RenderedEffects, Shape,
};

/// The sRGB of one ink under `policy`, as the slot's byte hex.
pub(crate) fn hex_of(ink: [f32; 4], policy: &CmykPolicy) -> Result<[u8; 3], String> {
    let rgb = ink_to_srgb(
        &policy.profile,
        ink.map(f64::from),
        ConversionOptions::default(),
    )?;
    Ok(rgb.map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8))
}

/// `style` with every INKED slot's hex the ink's sRGB under `policy` (module
/// doc); ink-less slots untouched.
pub(crate) fn canonical_hex(style: &LayerStyle, policy: &CmykPolicy) -> Result<LayerStyle, String> {
    style.map_inks(&mut |ink| Ok((ink, hex_of(ink, policy)?)))
}

/// Black ink, K 100 % (take1's `default` item FITTED D-K:
/// a new drop shadow, inner shadow, satin or bevel shadow in a CMYK document
/// is K100, not the profile's rich black).
pub(crate) const BLACK_INK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// `style` with the four slots whose default is black — drop shadow, inner
/// shadow, satin and the bevel's shadow side — given [`BLACK_INK`] where
/// they hold sRGB black and no ink: what a CMYK edit stores (module doc,
/// take1's D-K). Every other slot is untouched; a fresh cache.
pub(crate) fn with_black_as_k(style: &LayerStyle) -> LayerStyle {
    let seed = |ink: Option<[f32; 4]>, color: [u8; 3]| match ink {
        None if color == [0, 0, 0] => Some(BLACK_INK),
        other => other,
    };
    let mut out = style.clone();
    out.effects = style
        .effects
        .iter()
        .map(|effect| match effect {
            Effect::DropShadow(e) => Effect::DropShadow(crate::style::DropShadow {
                ink: seed(e.ink, e.color),
                ..e.clone()
            }),
            Effect::InnerShadow(e) => Effect::InnerShadow(crate::style::InnerShadow {
                ink: seed(e.ink, e.color),
                ..e.clone()
            }),
            Effect::Satin(e) => Effect::Satin(crate::style::Satin {
                ink: seed(e.ink, e.color),
                ..e.clone()
            }),
            Effect::BevelEmboss(e) => Effect::BevelEmboss(crate::style::BevelEmboss {
                shadow_ink: seed(e.shadow_ink, e.shadow_color),
                ..e.clone()
            }),
            other => other.clone(),
        })
        .collect();
    // A fresh cache, as every ink helper's result has (`map_colour_slots`).
    out.map_colour_slots(&mut |ink, color| Ok((ink, color)))
        .unwrap_or(out)
}

/// `style` with every ink-less slot given the ink its hex converts to under
/// `policy` (four-decimal quantized, as a parsed ink is) — what a CMYK PSD
/// export writes as `CMYC` (module doc).
pub(crate) fn with_all_inks(style: &LayerStyle, policy: &CmykPolicy) -> Result<LayerStyle, String> {
    let mut leg = RgbToCmyk::new(
        IccProfile::srgb(),
        &policy.profile,
        ConversionOptions::default(),
    )?;
    style.map_colour_slots(&mut |ink, color| match ink {
        Some(ink) => Ok((Some(ink), color)),
        None => {
            let converted = leg.ink(rgb_key(color), unit_color(color).map(f64::from));
            Ok((Some(converted.map(q4)), color))
        }
    })
}

fn rgb_key(color: [u8; 3]) -> u64 {
    u64::from(color[0]) << 16 | u64::from(color[1]) << 8 | u64::from(color[2])
}

/// Puts `ink` on the document's grid (module doc): an 8-bit document's
/// code — Photoshop's own double arithmetic on the four-decimal percent `p`,
/// `round(p·2.55)` half up, so 90 % is 229 and 50 % is 127 (`2.55` is just
/// below its decimal in binary) — a 16-bit one's as stored.
fn on_grid(ink: [f32; 4], depth: CmykDepth) -> [f32; 4] {
    match depth {
        CmykDepth::Eight => ink.map(|v| {
            let percent = (f64::from(v) * 1e6).round() / 1e4;
            ((percent * 2.55 + 0.5).floor().clamp(0.0, 255.0) / 255.0) as f32
        }),
        CmykDepth::Sixteen => ink,
    }
}

/// The slot one SOLID contribution reads: `(ink, hex)` of its effect's colour
/// (the bevel's side by `part`) — the `c.instance`-th enabled effect of its
/// kind, so each stacked instance prints its own ink.
fn solid_slot(
    style: &LayerStyle,
    c: &crate::style_render::Contribution,
) -> Option<(Option<[f32; 4]>, [u8; 3])> {
    style
        .enabled_effects()
        .filter(|e| e.kind() == c.kind)
        .nth(usize::from(c.instance))
        .map(|e| match e {
            Effect::DropShadow(e) => (e.ink, e.color),
            Effect::InnerShadow(e) => (e.ink, e.color),
            Effect::OuterGlow(e) => (e.ink, e.color),
            Effect::InnerGlow(e) => (e.ink, e.color),
            Effect::Stroke(e) => (e.ink, e.color),
            Effect::ColorOverlay(e) => (e.ink, e.color),
            Effect::Satin(e) => (e.ink, e.color),
            Effect::BevelEmboss(e) => match c.part {
                Part::Shadow => (e.shadow_ink, e.shadow_color),
                Part::Highlight | Part::Main => (e.highlight_ink, e.highlight_color),
            },
            // A gradient overlay's contribution is a gradient colour and a
            // pattern overlay's a placed tile (`effect_inks` resolves both, and
            // a Gradient or Pattern stroke's alike).
            Effect::GradientOverlay(_) | Effect::PatternOverlay(_) => (None, [0, 0, 0]),
        })
}

/// The document's ONE sRGB → ink leg for a style slot or a tile texel
/// (module doc): `cmyk_color::RgbToCmyk` from sRGB into `policy`'s profile
/// under the default options, then [`on_grid`] at `policy`'s depth. What
/// [`effect_inks`] converts an ink-less slot, a gradient stop and a pattern
/// texel with, and what a CMYK pattern fill layer's texels take
/// (`cmyk_artwork`).
pub(crate) struct TexelInk<'a> {
    leg: RgbToCmyk<'a>,
    depth: CmykDepth,
}

impl<'a> TexelInk<'a> {
    /// `Err` when `policy`'s profile cannot take sRGB (`RgbToCmyk::new`).
    pub(crate) fn new(policy: &'a CmykPolicy) -> Result<Self, String> {
        Ok(Self {
            leg: RgbToCmyk::new(
                IccProfile::srgb(),
                &policy.profile,
                ConversionOptions::default(),
            )?,
            depth: policy.depth,
        })
    }

    /// The ink of one sRGB byte colour, on the document's grid.
    pub(crate) fn ink(&mut self, rgb: [u8; 3]) -> [f32; 4] {
        let ink = self
            .leg
            .ink(rgb_key(rgb), unit_color(rgb).map(f64::from))
            .map(|v| v as f32);
        on_grid(ink, self.depth)
    }
}

/// Each rendered contribution's ink (module doc), `below` then `interior`,
/// parallel to `rendered`'s two lists: a slot's ink, else its hex through the
/// profile ([`TexelInk`]); a gradient's stops alike; a pattern's texels alike
/// (an overlay's or a Pattern stroke's), through `pattern_tile::ink_sampler`
/// (built once per effect) placed where the RGB plane put the tile; on the
/// document's grid.
pub(crate) fn effect_inks(
    style: &LayerStyle,
    rendered: &RenderedEffects,
    policy: &CmykPolicy,
) -> Result<(Vec<InkColor>, Vec<InkColor>), String> {
    let mut texel = TexelInk::new(policy)?;
    let depth = policy.depth;
    let mut ink_of = |ink: Option<[f32; 4]>, color: [u8; 3]| -> [f32; 4] {
        match ink {
            Some(ink) => on_grid(ink, depth),
            None => texel.ink(color),
        }
    };
    let mut built: Vec<((EffectKind, u8), InkSampler)> = Vec::new();
    let mut resolve = |list: &[crate::style_render::Contribution]| -> Vec<InkColor> {
        list.iter()
            .map(|c| match &c.color {
                PlaneColor::Gradient(sampler) => {
                    let stops = sampler
                        .fill
                        .stops
                        .iter()
                        .map(|s| ink_of(s.ink, s.color))
                        .collect();
                    // Dither at half a code of the document's depth
                    // (`style_gradient`).
                    let divisor = match depth {
                        CmykDepth::Eight => 255.0,
                        CmykDepth::Sixteen => 65535.0,
                    };
                    InkColor::Gradient(sampler.clone().with_dither_divisor(divisor), stops)
                }
                PlaneColor::Pattern(placed) => {
                    // One ink level per effect (`(kind, instance)`, as
                    // `solid_slot` finds it), shared by a Center stroke's two
                    // contributions across both lists, so
                    // `ink_level_bytes`' one charge per effect stays exact.
                    let key = (c.kind, c.instance);
                    if let Some((_, sampler)) = built.iter().find(|(k, _)| *k == key) {
                        return InkColor::Pattern(sampler.clone());
                    }
                    let fill = style
                        .enabled_effects()
                        .filter(|e| e.kind() == c.kind)
                        .nth(usize::from(c.instance))
                        .and_then(pattern_fill);
                    match fill {
                        Some(fill) => {
                            let sampler = ink_sampler(
                                &fill.pattern.tile,
                                f64::from(fill.scale),
                                f64::from(fill.angle),
                                placed.origin,
                                &mut |rgb| ink_of(None, rgb),
                            );
                            built.push((key, sampler.clone()));
                            InkColor::Pattern(sampler)
                        }
                        None => InkColor::Solid(ink_of(None, [0, 0, 0])),
                    }
                }
                _ => {
                    let (ink, color) = solid_slot(style, c).unwrap_or((None, [0, 0, 0]));
                    InkColor::Solid(ink_of(ink, color))
                }
            })
            .collect()
    };
    let below = resolve(&rendered.below);
    let interior = resolve(&rendered.interior);
    Ok((below, interior))
}

/// What a styled record draws from (module doc): a CMYK raster's own image
/// at its offset, an isolated group's rendered ink frame at its origin, or
/// an adjustment's opaque shape.
pub(crate) enum StyledSource<'a> {
    Raster {
        image: &'a CmykImage,
        offset: (i32, i32),
    },
    Frame {
        pixels: Vec<InkPixel>,
        origin: (i32, i32),
        dims: (u32, u32),
    },
    /// S-MASK: opaque over the stack's read rect (the canvas for a
    /// projection, a merge's frame for a merge), as the typed master's
    /// `StyledPixels::Opaque`; `sample` answers ([0.0; 4], 1.0, 65535.0)
    /// inside it.
    Opaque {
        origin: (i32, i32),
        dims: (u32, u32),
    },
}

impl StyledSource<'_> {
    pub(crate) fn origin(&self) -> (i32, i32) {
        match self {
            StyledSource::Raster { offset, .. } => *offset,
            StyledSource::Frame { origin, .. } | StyledSource::Opaque { origin, .. } => *origin,
        }
    }

    pub(crate) fn dims(&self) -> (u32, u32) {
        match self {
            StyledSource::Raster { image, .. } => image.dimensions(),
            StyledSource::Frame { dims, .. } | StyledSource::Opaque { dims, .. } => *dims,
        }
    }

    /// `(ink, alpha, alpha16)` at layer pixel `(lx, ly)`: a raster's stored
    /// code (exact), a frame's f32 alpha × 65535, opaque inside an
    /// adjustment's rect.
    fn sample(&self, lx: u32, ly: u32) -> Option<([f32; 4], f32, f32)> {
        match self {
            StyledSource::Raster { image, .. } => image.sample_at(lx, ly).map(|s| {
                (
                    [unit(s[0]), unit(s[1]), unit(s[2]), unit(s[3])],
                    unit(s[4]),
                    f32::from(s[4]),
                )
            }),
            StyledSource::Frame { pixels, dims, .. } => (lx < dims.0 && ly < dims.1).then(|| {
                let p = pixels[ly as usize * dims.0 as usize + lx as usize];
                ([p[0], p[1], p[2], p[3]], p[4], p[4] * 65535.0)
            }),
            StyledSource::Opaque { dims, .. } => {
                (lx < dims.0 && ly < dims.1).then_some(([0.0; 4], 1.0, 65535.0))
            }
        }
    }

    /// The kernel's `PixelAt`: the record's `(ink, alpha, mask coverage)` at
    /// canvas `(x, y)`, `None` outside it.
    pub(crate) fn pixel(
        &self,
        x: i64,
        y: i64,
        mask: Option<&PreparedMask<'_>>,
    ) -> Option<([f32; 4], f32, f32)> {
        let (ox, oy) = self.origin();
        let (lx, ly) = (x - i64::from(ox), y - i64::from(oy));
        if lx < 0 || ly < 0 || lx > i64::from(u32::MAX) || ly > i64::from(u32::MAX) {
            return None;
        }
        let (ink, alpha, _) = self.sample(lx as u32, ly as u32)?;
        Some((ink, alpha, mask.map_or(1.0, |m| m.at(x, y))))
    }
}

/// The record's shape on a plane padded by `pad` — `typed_shape`
/// (`precision_style_render`, the master) on ink: `typed_coverage_byte(a16,
/// mask)`, bounds the box of `a16 > 0` without the mask; `None` past
/// `MAX_PIXELS`.
pub(crate) fn cmyk_shape(
    source: &StyledSource<'_>,
    mask: Option<&PreparedMask<'_>>,
    pad: u32,
) -> Option<Shape> {
    let (lw, lh) = source.dims();
    let (ox, oy) = source.origin();
    if plane_pixels((lw, lh), pad).ok()? > MAX_PIXELS {
        return None;
    }
    let w = lw.checked_add(pad.checked_mul(2)?)?;
    let h = lh.checked_add(pad.checked_mul(2)?)?;
    let mut coverage = vec![0u8; w as usize * h as usize];
    let (mut bx0, mut by0, mut bx1, mut by1) = (i64::from(lw), i64::from(lh), 0i64, 0i64);
    for y in 0..lh {
        let row = (y + pad) as usize * w as usize + pad as usize;
        let mut row_min = lw;
        let mut row_max = 0;
        for x in 0..lw {
            let Some((_, _, a16)) = source.sample(x, y) else {
                continue;
            };
            let cov = mask.map_or(1.0, |m| {
                m.at(i64::from(ox) + i64::from(x), i64::from(oy) + i64::from(y))
            });
            coverage[row + x as usize] = typed_coverage_byte(a16, cov);
            if a16 > 0.0 {
                row_min = row_min.min(x);
                row_max = x;
            }
        }
        if row_min < lw {
            bx0 = bx0.min(i64::from(row_min));
            bx1 = bx1.max(i64::from(row_max) + 1);
            by0 = by0.min(i64::from(y));
            by1 = i64::from(y) + 1;
        }
    }
    let bounds = (bx1 > bx0).then_some((bx0, by0, bx1, by1));
    Some(Shape {
        lw,
        lh,
        w,
        h,
        origin: (-i64::from(pad), -i64::from(pad)),
        coverage,
        bounds,
    })
}

/// Whether a CMYK record composites through the styled kernel: a Blend If,
/// an enabled effect or a knockout (a Fill-only style keeps the plain path),
/// or an adjustment with any style or Blend If (its Fill rides in the
/// kernel's `k`).
pub(crate) fn needs_styled(layer: &PrecisionDocumentLayer) -> bool {
    if matches!(layer.content, Content::CmykAdjustment(_)) {
        return layer.blend_if().is_some() || layer.style().is_some();
    }
    layer.blend_if().is_some()
        || layer
            .style()
            .is_some_and(|s| s.has_enabled_effects() || s.knockout != Knockout::None)
}

/// The kernel's [`StyledInk`] for `layer` over `source` (module doc).
pub(crate) fn styled_ink(
    layer: &PrecisionDocumentLayer,
    source: &StyledSource<'_>,
    mask: Option<&PreparedMask<'_>>,
    policy: &CmykPolicy,
    light: GlobalLight,
    canvas: (u32, u32),
    job: JobContext<'_>,
) -> Result<StyledInk, String> {
    let style = layer.style().map(|s| &**s);
    let planes = match style.filter(|s| s.has_enabled_effects()) {
        Some(style) => {
            let shape = cmyk_shape(source, mask, style.pad())
                .ok_or("typed layer style plane exceeds the pixel limit")?;
            let ctx = RenderContext {
                light,
                canvas,
                layer_offset: source.origin(),
                lab_tiles: false,
            };
            Some(render_effects_with_job(style, shape, &ctx, job)?)
        }
        None => None,
    };
    let (below, interior) = match (style, planes.as_ref()) {
        (Some(style), Some(rendered)) => effect_inks(style, rendered, policy)?,
        _ => (Vec::new(), Vec::new()),
    };
    let grouped =
        style.is_some_and(|s| crate::style_knockout::grouped_interior(s, planes.as_ref()));
    let mode = if layer.blend_mode == BlendMode::PassThrough {
        BlendMode::Normal
    } else {
        layer.blend_mode
    };
    let adjusted = matches!(layer.content, Content::CmykAdjustment(_));
    Ok(StyledInk {
        planes,
        below,
        interior,
        fill: style.map_or(1.0, |s| crate::opacity_byte::quantized(s.fill_opacity)),
        // The ink pass has no special Fill (`blend_fill` is RGB only), which
        // is `fill_blend`'s answer with `float` set.
        occluding_fill: style.map_or(0.0, |s| {
            crate::opacity_byte::quantized(occluding_fill(
                s,
                layer.blend_if().as_ref(),
                mode,
                true,
                adjusted,
            ))
        }),
        opacity: crate::opacity_byte::quantized(layer.opacity()),
        mode,
        blend_if: layer.blend_if(),
        knockout: style.map_or(Knockout::None, |s| s.knockout),
        grouped,
        code_max: match policy.depth {
            CmykDepth::Eight => 255.0,
            CmykDepth::Sixteen => 65535.0,
        },
        tone: match &layer.content {
            Content::CmykAdjustment(state) => Some(InkTone {
                kernel: state.kernel().clone(),
                depth_bits: policy.depth.bits(),
            }),
            _ => None,
        },
    })
}

/// Rows of kernel scratch a styled record holds (the package copy, the Blend
/// If underlying, `B0`, the unit and its alpha, a styled adjustment's
/// adjusted row on the routed path) plus a clip base's base-only copy.
const STYLED_ROWS: u64 = 7;

/// What a styled `layer` holds beside the stack (module doc): the typed
/// renderer's plan for its planes over `layer.style_plane(canvas)`, each
/// enabled pattern overlay's ink tile and pyramid level
/// (`style_pattern::ink_level_bytes`), a group's frame over the canvas
/// (`canvas` is the larger of the canvas and a merge's frame), and
/// [`STYLED_ROWS`] rows of `row` pixels at 20 B.
pub(crate) fn styled_scratch(
    layer: &PrecisionDocumentLayer,
    canvas: (u32, u32),
    row: u64,
) -> Result<u64, String> {
    let overflow = || "CMYK layer style calculation overflow".to_string();
    let mut total = row.checked_mul(20 * STYLED_ROWS).ok_or_else(overflow)?;
    if let Some(style) = layer.style().filter(|s| s.has_enabled_effects()) {
        let (planes, _) = style_scratch(style, layer.style_plane(canvas), false, None)?;
        total = total.checked_add(planes).ok_or_else(overflow)?;
        total = total
            .checked_add(ink_level_bytes(style))
            .ok_or_else(overflow)?;
    }
    if layer.children().is_some() {
        let frame = u64::from(canvas.0)
            .checked_mul(u64::from(canvas.1))
            .and_then(|p| p.checked_mul(20))
            .ok_or_else(overflow)?;
        total = total.checked_add(frame).ok_or_else(overflow)?;
    }
    Ok(total)
}
