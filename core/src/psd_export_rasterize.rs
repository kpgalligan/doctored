//! Rasterizing one styled layer for the 8-bit PSD export. Since 10C slice 3
//! this is the FALLBACK, not the rule: a style is written as Photoshop's own `lfx2`,
//! `iOpa` and blending ranges (`psd_layer_effects`), and only what
//! Photoshop's dialog or a PSD mask cannot hold — `bake_reason`, then
//! `mask_bake_reason` — lands here as pixels instead of refusing the export.
//!
//! Nothing here is new pixel math. The bake runs the SHIPPED compositor
//! (`style_composite::merge_extent` for the grown rectangle,
//! `doc::composite_layer_into` for the package, `doc_evaluation::
//! encode_projection` for the samples — `doc::quantize_with_job` on an encoded
//! document) over a transparent accumulator, where every blend reduces to
//! source-over — so the record's own blend key and opacity byte still apply
//! the whole package in Photoshop, which is the package model
//! `style_composite` documents. The layer's own opacity, blend mode and Blend
//! If are therefore stripped from the baked copy; its ENABLED mask stays
//! inside the bake, because `style_render::Shape::of_layer` shapes the effects
//! with `Layer::active_mask` and a separately written mask would apply twice.
//!
//! The preflight's pixel-free half (`style_extent`, `export_style`) lives here
//! too, so the rectangle the report forecasts and the rectangle the writer
//! produces come from one place.
//!
//! A layer style is WRITTEN, not baked: `ExportStyle.bake` is
//! `psd_layer_effects::bake_reason(style).or_else(mask_bake_reason)`, and `None` keeps the layer's
//! own planes, offset and `-2` mask and adds the `lfx2`/`iOpa` blocks and the 40-byte blending
//! ranges (image resources 1037/1049 are written iff at least one `lfx2` is — the document's global
//! light at both depths). `psd_export_rasterize.rs` is the FALLBACK rule: a style Photoshop's own
//! dialog or a PSD mask cannot hold (a sub-1 px stroke, a satin distance outside 1–250 px, an
//! enabled mask with density or feather) is baked into pixels through the SHIPPED compositor
//! (`merge_extent`, `composite_layer_into`, `encode_projection`) over a transparent accumulator, with
//! opacity/blend/Blend If left to the record's own bytes and an ENABLED mask inside the bake; an
//! adjustment layer whose op has no Photoshop record, a bake past 100 MP and a GROUP whose style
//! would need a bake still refuse.

use std::sync::Arc;

use image::RgbaImage;

use crate::authored_color::AuthoredColors;
use crate::blend::BlendMode;
use crate::doc::{composite_layer_into, DrDocument, Layer, MAX_PIXELS};
use crate::psd_export_preflight::{mask_bake_reason, ExportStyle};
use crate::psd_layer_effects::{
    bake_reason, effect_block_bytes, encode_fill, encode_ranges, knockout_blocks,
};
use crate::style::{GlobalLight, LayerStyle};
use crate::style_composite::merge_extent;

/// One rasterized style: the grown rectangle's origin and its straight RGBA8
/// samples, which is exactly what one PSD layer record holds.
pub(crate) struct Bake {
    pub origin: (i32, i32),
    pub pixels: RgbaImage,
}

/// The refusal a styled layer whose grown rectangle is past the editing limit
/// carries, in the preflight and in the bake alike.
pub(crate) fn oversize_refusal(name: &str, width: u64, height: u64) -> String {
    format!(
        "Layer “{name}” with its rasterized style is {width} x {height} pixels, past the 100 MP layer limit; reduce the effect sizes or export the flattened composite."
    )
}

impl DrDocument {
    /// The canvas rect a styled raster's bake will occupy, without reading a
    /// single pixel: the extent the compositor itself would use. `None` when
    /// the layer renders no style.
    pub(crate) fn style_extent(&self, index: usize) -> Option<(i64, i64, i64, i64)> {
        let layer = self.layers.get(index)?;
        layer.renders_any_style()?;
        // The extent is a projection question, so it asks the policy's
        // transfer as every projection does (`doc_evaluation`).
        Some(merge_extent(
            layer,
            self.composite_env(None, self.transfer().ok()?),
        ))
    }

    /// Layer `index` with its style rendered into pixels. Pure: the document is
    /// untouched, and the result is a standalone rectangle the record walk
    /// writes as the layer's own planes.
    pub(crate) fn rasterized_style(
        &self,
        index: usize,
        colors: Option<&AuthoredColors>,
    ) -> Result<Bake, String> {
        let layer = self
            .layers
            .get(index)
            .ok_or("no such layer to rasterize its style")?;
        let style = layer
            .renders_any_style()
            .ok_or("this layer renders no layer style")?;
        // The bake is the layer as the canvas shows it: under linear light it
        // composites in linear light and is encoded back below
        // (`doc_evaluation::encode_projection`, which on an encoded document
        // is `quantize` statement for statement).
        let env = self.composite_env(colors, self.transfer()?);
        let (x0, y0, x1, y1) = merge_extent(layer, env);
        let (uw, uh) = ((x1 - x0).max(0) as u64, (y1 - y0).max(0) as u64);
        if uw == 0 || uh == 0 {
            return Err(format!(
                "Layer “{}” with its rasterized style is empty; clear the style or export the flattened composite.",
                layer.name
            ));
        }
        if uw.saturating_mul(uh) > MAX_PIXELS {
            return Err(oversize_refusal(&layer.name, uw, uh));
        }
        if x0 < i64::from(i32::MIN) || y0 < i64::from(i32::MIN) {
            return Err(format!(
                "Layer “{}” with its rasterized style starts past the coordinate limit; move it onto the canvas or export the flattened composite.",
                layer.name
            ));
        }
        let origin = (x0 as i32, y0 as i32);
        let mut acc = vec![[0.0f32; 4]; (uw * uh) as usize];
        // Opacity, blend mode and Blend If belong to the PACKAGE, which the
        // record's own bytes carry: over a transparent accumulator a Blend If
        // ramp against alpha 0 would zero the layer outright.
        let baked = Layer {
            opacity: 1.0,
            blend: BlendMode::Normal,
            style: Some(Arc::new(LayerStyle {
                blend_if: None,
                // The bake runs over a TRANSPARENT accumulator, where a
                // knockout has nothing to punch through and a grouped
                // interior would meet transparency rather than the picture.
                // Both keys are cleared so the baked pixels are the plain
                // package, no `knko`/`infx` block is written beside them
                // (12E-3c, E2 — writing one would punch a second time), and
                // the export reports the narrowed per-layer loss instead.
                knockout: crate::style::Knockout::None,
                blend_interior_as_group: false,
                ..style.clone()
            })),
            ..layer.clone()
        };
        composite_layer_into(&mut acc, uw as u32, uh as u32, origin, &baked, env)?;
        Ok(Bake {
            origin,
            pixels: crate::doc_evaluation::encode_projection(
                &acc,
                uw as u32,
                uh as u32,
                env.transfer,
            ),
        })
    }
}

/// What one entry's style costs the export, or `None` when the layer renders
/// no style at all. `clip_base` is the caller's structural answer: a visible
/// entry directly above this one, at the same depth, is clipped to it.
pub(crate) fn export_style(layer: &Layer, clip_base: bool) -> Option<ExportStyle> {
    // An adjustment layer's style too (`style_adjustment`).
    let style = layer.renders_any_style()?;
    // A DISABLED mask shapes nothing (`Layer::active_mask`), so it stays
    // outside the bake and is written as an ordinary disabled PSD mask.
    let masked = layer.mask.is_some() && layer.mask_enabled;
    Some(ExportStyle {
        effects: style
            .enabled_effects()
            .map(|effect| effect.kind().type_name().to_string())
            .collect(),
        fill_opacity: style.fill_opacity,
        blend_if: style
            .blend_if
            .filter(|ranges| !ranges.is_identity())
            .map_or(Vec::new(), |ranges| {
                ranges.ranges().map(|r| r.channel).collect()
            }),
        knockout: style.knockout,
        blend_interior: style.blend_interior_as_group,
        masked,
        clip_base,
        bake: bake_reason(style).or_else(|| {
            mask_bake_reason(
                masked,
                layer.mask_properties.density,
                layer.mask_properties.feather,
            )
        }),
        writes_effects: style.has_enabled_effects(),
        block_bytes: style_block_bytes(style),
    })
}

/// What a preserved style's additional layer blocks and blending ranges cost
/// one record: the `lfx2` block, the `iOpa` block, the `infx`/`knko` blocks
/// (16 bytes each — signature, key, length and a 4-byte body) and the 40-byte
/// ranges section, each only when it is written.
pub(crate) fn style_block_bytes(style: &LayerStyle) -> u64 {
    let effects = if !style.has_enabled_effects() {
        0
    } else {
        effect_block_bytes(
            style,
            GlobalLight::default(),
            crate::psd_layer_effects::FxColours::Rgb(None),
        )
    };
    let fill = encode_fill(style.fill_opacity).map_or(0, |_| 16);
    let ranges = style
        .blend_if
        .filter(|ranges| !ranges.is_identity())
        .map_or(0, |ranges| encode_ranges(&ranges).len() as u64);
    let keys = knockout_blocks(style).len() as u64 * 16;
    // A record's `fxrp` is in the preflight's per-record allowance.
    effects + fill + ranges + keys
}
