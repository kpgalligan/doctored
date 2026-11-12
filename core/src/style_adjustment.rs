//! Layer styles and Blend If on a BYTE adjustment layer.
//!
//! An adjustment layer has no pixels of its own, so it has no shape for an
//! effect to hang off and no colour for This Layer to read; both come from
//! what it does. The rule is the one a take measured in Photoshop
//! (`scripts/photoshop-probe/styles_scope_558_take.py`, its recorded
//! result; S-MASK and POST are the take's winners, fitted from it and not
//! yet confirmed by a second take):
//!
//! - The SHAPE (S-MASK, A1 0 / 0). The effects hang off the layer's MASK over
//!   the canvas, never the backdrop's alpha: an unmasked adjustment's shape
//!   is the whole canvas, so an outside Stroke falls off it and nothing rings
//!   a transparent hole. [`composite_styled_adjustment_into`] hands the
//!   styled compositor a SHAPE layer — opaque over the canvas, or over the
//!   layer's own rect carrying its mask and mask properties when the mask is
//!   enabled (`LayerRaster::filled`, so it costs no pixel plane) — and the
//!   compositor renders the effects, the Blend If snapshot and a clip base's
//!   footprint from it exactly as from a raster's pixels.
//! - The ORDER (POST, A2 0 / 0). The adjustment reads the accumulator AFTER
//!   the layer's own Below group: under its Drop Shadow it adjusts the
//!   shadow. `style_composite::composite_direct_into` calls
//!   [`Adjusted::interior_into`] at step 3, where a raster's pixel pass runs.
//! - The INTERIOR (ALPHA-KEPT, A1's band 0 / 0). The adjustment is applied in
//!   place by the MASTER per-pixel body `doc::adjust_backdrop_sample` at
//!   `k = mask × Fill × Blend If × opacity` with the layer's mode, alpha
//!   kept and a transparent pixel left alone — the unstyled adjustment's
//!   lerp with Fill and the weight folded into `k` (FILL-SCALES, A3), so a
//!   hole stays transparent and a 50 % band stays at 50 %. The effects are
//!   not scaled by Fill.
//! - This Layer (T-ADJUSTED, A4) reads the adjusted colour at `k = 1`,
//!   quantized as the layer's pixels would be (`doc_evaluation::
//!   encode_sample`); Underlying reads the composite beneath, as a raster's.
//! - A clip base (C-SHAPE, A7). The members composite into the unit buffer
//!   frozen to the shape, with no base pixels in it; [`Adjusted::
//!   interior_into`] then lays that buffer over each adjusted pixel in the
//!   layer's mode and mixes the result back at the layer opacity — the
//!   package model, so an opaque member hides the adjustment under it. An
//!   UNSTYLED adjustment base keeps `doc.rs`'s path, where its members
//!   contribute nothing.
//! - A knockout or "Blend Interior Effects as Group" routes step 3 through
//!   `style_knockout`'s unit pass, which composites PIXELS: it is handed
//!   [`Adjusted::interior_layer`], the accumulator after the Below group
//!   adjusted at `k = 1` at its own alpha under the layer's mask. Over an
//!   opaque backdrop that is the interior above; over a partly transparent
//!   one the unit's source-over raises alpha, a case no take measured.
//!
//! The Shadows/Highlights guide is built once, from the accumulator as it
//! stands before the layer draws anything (the backdrop beneath its Below
//! group), so the direct path and every 256-row strip of the package path
//! read one guide.
//!
//! An unstyled adjustment never reaches this module: `doc.rs` routes only an
//! adjustment whose [`Layer::renders_adjustment_style`] is `Some`.

use crate::adjust::Adjustment;
use crate::adjust_tone::GuidePlane;
use crate::blend::{blend_kind, composite_source_into, BlendMode};
use crate::doc::{adjust_backdrop_sample, Layer, LayerKind, LayerRaster};
use crate::style::{BlendIf, LayerStyle};
use crate::style_blend_if::source_weight;
use crate::style_composite::{
    composite_styled_into, mix_premultiplied, BackdropSnapshot, CompositeEnv,
};

impl Layer {
    /// The style the compositor must render for this ADJUSTMENT layer:
    /// `Some` only when a style is present, the entry is not a group (whose
    /// meta the core never interprets) and the meta parses as an
    /// [`Adjustment`]. The twin of `renders_style`, which answers `None` for
    /// every adjustment layer.
    pub(crate) fn renders_adjustment_style(&self) -> Option<&LayerStyle> {
        let style = self.style.as_deref()?;
        if self.kind == LayerKind::Group {
            return None;
        }
        self.meta.as_deref().and_then(Adjustment::from_meta)?;
        Some(style)
    }

    /// The style this layer renders, whichever kind it is:
    /// `renders_style()` or [`Self::renders_adjustment_style`]. What a reader
    /// of "the style this entry shows" (an export, a scratch plan) asks.
    pub(crate) fn renders_any_style(&self) -> Option<&LayerStyle> {
        self.renders_style()
            .or_else(|| self.renders_adjustment_style())
    }
}

/// A styled adjustment's interior as the styled compositor applies it
/// (module doc): the adjustment and its Shadows/Highlights guide, built over
/// the accumulator window at canvas `guide_origin` of `guide_size`.
pub(crate) struct Adjusted<'a> {
    adjustment: &'a Adjustment,
    guide: Option<GuidePlane>,
    guide_origin: (i32, i32),
    guide_size: (u32, u32),
}

impl Adjusted<'_> {
    /// The guide at canvas pixel `canvas_xy`, 0.0 for a guide-less op. A
    /// package strip lies inside the window the guide was built over, so the
    /// clamp only keeps the lookup defined.
    fn guide_at(&self, canvas_xy: (i64, i64)) -> f32 {
        self.guide.as_ref().map_or(0.0, |plane| {
            let x = canvas_xy.0 - i64::from(self.guide_origin.0);
            let y = canvas_xy.1 - i64::from(self.guide_origin.1);
            let (w, h) = self.guide_size;
            plane.at(
                x.clamp(0, i64::from(w.max(1)) - 1) as u32,
                y.clamp(0, i64::from(h.max(1)) - 1) as u32,
            )
        })
    }

    /// The interior onto `acc` (module doc): inside `shape`'s frame, each
    /// non-transparent pixel adjusted in place at `k = mask × fill × Blend
    /// If × scale` in `shape.blend`, alpha kept. Without `members`, `scale`
    /// is `opacity` (the unstyled lerp). With `members` — a clip base's unit
    /// buffer, the members frozen to the shape and no base pixels — `scale`
    /// is 1, the buffer composites over the adjusted pixel in the layer's
    /// mode and the result mixes back at `opacity` (the package). The Blend
    /// If weight reads `blend_if`'s snapshot where there is one, else the
    /// pixel before it is written (`composite_pixels_into`'s rule).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn interior_into(
        &self,
        acc: &mut [[f32; 4]],
        acc_w: u32,
        acc_h: u32,
        origin: (i32, i32),
        shape: &Layer,
        fill_opacity: f32,
        blend_if: Option<(&BlendIf, Option<&BackdropSnapshot>)>,
        opacity: f32,
        members: Option<&[[f32; 4]]>,
        env: CompositeEnv<'_>,
    ) -> Result<(), String> {
        let fill = crate::opacity_byte::quantized(fill_opacity);
        let scale = if members.is_some() { 1.0 } else { opacity };
        // The Fill and the opacity in one product (`opacity_byte::joint`).
        let joint = crate::opacity_byte::joint(fill, scale, false);
        let kind = blend_kind(shape.blend);
        let normal = blend_kind(BlendMode::Normal);
        let active_mask = shape.active_mask();
        let mask = active_mask.as_deref().map(|m| m.as_raw().as_slice());
        let (lw, lh) = shape.pixels.dimensions();
        let rel_x = i64::from(shape.offset.0) - i64::from(origin.0);
        let rel_y = i64::from(shape.offset.1) - i64::from(origin.1);
        let x0 = rel_x.max(0);
        let y0 = rel_y.max(0);
        let x1 = (rel_x + i64::from(lw)).min(i64::from(acc_w));
        let y1 = (rel_y + i64::from(lh)).min(i64::from(acc_h));
        for ay in y0..y1 {
            env.job.check()?;
            let ly = (ay - rel_y) as u64;
            for ax in x0..x1 {
                let lx = (ax - rel_x) as u64;
                let coverage = mask.map_or(1.0, |m| {
                    f32::from(m[(ly * u64::from(lw) + lx) as usize]) / 255.0
                });
                let ai = (ay as u64 * u64::from(acc_w) + ax as u64) as usize;
                let canvas_xy = (ax + i64::from(origin.0), ay + i64::from(origin.1));
                let before = acc[ai];
                let mut px = before;
                let k0 = coverage * joint;
                if k0 > 0.0 && px[3] > 0.0 {
                    let g = self.guide_at(canvas_xy);
                    let weight = match blend_if {
                        None => 1.0,
                        Some((b, snapshot)) => {
                            let mut adjusted = px;
                            adjust_backdrop_sample(
                                &mut adjusted,
                                self.adjustment,
                                1.0,
                                g,
                                normal,
                                env.transfer,
                                canvas_xy,
                            );
                            let code = crate::doc_evaluation::encode_sample(adjusted, env.transfer);
                            let cs = [code[0], code[1], code[2]].map(|v| f32::from(v) / 255.0);
                            let under = snapshot.and_then(|s| s.at(ax, ay)).unwrap_or(before);
                            source_weight(b, cs, under)
                        }
                    };
                    let k = k0 * weight;
                    if k > 0.0 {
                        adjust_backdrop_sample(
                            &mut px,
                            self.adjustment,
                            k,
                            g,
                            kind,
                            env.transfer,
                            canvas_xy,
                        );
                    }
                }
                if let Some(unit) = members {
                    let src = unit[ai];
                    if src[3] > 0.0 {
                        let mut one = [px];
                        composite_source_into(
                            &mut one,
                            0,
                            [src[0], src[1], src[2]],
                            src[3],
                            kind,
                            canvas_xy,
                        );
                        px = one[0];
                    }
                    if opacity < 1.0 {
                        px = mix_premultiplied(before, px, opacity);
                    }
                }
                acc[ai] = px;
            }
        }
        Ok(())
    }

    /// The PIXELS a routed unit pass composites (module doc): `shape` with
    /// its pixels replaced by `acc` (as it stands, after the Below group)
    /// adjusted at `k = 1` under Normal and quantized, at the accumulator's
    /// own alpha, transparent outside the window. Only the window's tiles
    /// are allocated (`LayerRaster::with_rect_with_job`).
    pub(crate) fn interior_layer(
        &self,
        acc: &[[f32; 4]],
        acc_w: u32,
        acc_h: u32,
        origin: (i32, i32),
        shape: &Layer,
        env: CompositeEnv<'_>,
    ) -> Result<Layer, String> {
        let normal = blend_kind(BlendMode::Normal);
        let (lw, lh) = shape.pixels.dimensions();
        let rel_x = i64::from(shape.offset.0) - i64::from(origin.0);
        let rel_y = i64::from(shape.offset.1) - i64::from(origin.1);
        let x0 = rel_x.max(0);
        let y0 = rel_y.max(0);
        let x1 = (rel_x + i64::from(lw)).min(i64::from(acc_w));
        let y1 = (rel_y + i64::from(lh)).min(i64::from(acc_h));
        let empty = LayerRaster::filled(lw, lh, [0; 4]);
        let pixels = if x1 > x0 && y1 > y0 {
            let mut samples = Vec::with_capacity(((x1 - x0) * (y1 - y0)) as usize);
            for ay in y0..y1 {
                env.job.check()?;
                for ax in x0..x1 {
                    let mut px = acc[(ay as u64 * u64::from(acc_w) + ax as u64) as usize];
                    if px[3] <= 0.0 {
                        samples.push([0; 4]);
                        continue;
                    }
                    let canvas_xy = (ax + i64::from(origin.0), ay + i64::from(origin.1));
                    adjust_backdrop_sample(
                        &mut px,
                        self.adjustment,
                        1.0,
                        self.guide_at(canvas_xy),
                        normal,
                        env.transfer,
                        canvas_xy,
                    );
                    samples.push(crate::doc_evaluation::encode_sample(px, env.transfer));
                }
            }
            empty
                .with_rect_with_job(
                    (x0 - rel_x) as u32,
                    (y0 - rel_y) as u32,
                    (x1 - x0) as u32,
                    (y1 - y0) as u32,
                    &samples,
                    env.job,
                )?
                .unwrap_or(empty)
        } else {
            empty
        };
        Ok(Layer {
            pixels,
            ..shape.clone()
        })
    }
}

/// The SHAPE layer a styled adjustment composites as (module doc): opaque
/// over the canvas, or over the layer's own rect carrying its mask and mask
/// properties when the mask is enabled, so `Layer::active_mask` aligns it
/// exactly as the unstyled path does. It keeps the adjustment layer's
/// identity, mode, opacity, style, visibility and clip flag, and carries no
/// meta.
fn shape_layer(layer: &Layer, env: CompositeEnv<'_>) -> Layer {
    let masked = layer.mask_enabled && layer.mask.is_some();
    let (offset, (w, h)) = if masked {
        (layer.offset, layer.pixels.dimensions())
    } else {
        ((0, 0), env.canvas)
    };
    Layer {
        // The SAME entry, rendered (the `doc_group::rendered_group` rule).
        id: layer.id,
        retained_source: None,
        warp: None,
        generated: None,
        pixels: LayerRaster::filled(w, h, [0, 0, 0, 255]),
        offset,
        name: layer.name.clone(),
        opacity: layer.opacity,
        blend: layer.blend,
        visible: layer.visible,
        mask: if masked { layer.mask.clone() } else { None },
        mask_enabled: masked,
        mask_properties: if masked {
            layer.mask_properties.clone()
        } else {
            crate::doc_mask::MaskProperties::at(offset)
        },
        meta: None,
        clipped: layer.clipped,
        style: layer.style.clone(),
        kind: LayerKind::Raster,
        depth: layer.depth,
        locks: 0,
        link: 0,
        open: true,
        label: layer.label,
        reference: layer.reference,
    }
}

/// A styled adjustment layer (with `group`, its clipped members, when it is a
/// clip base) onto the accumulator (module doc): its guide built from `acc`
/// as it stands, then `composite_styled_into` with its shape layer and the
/// interior hook.
#[allow(clippy::too_many_arguments)]
pub(crate) fn composite_styled_adjustment_into(
    acc: &mut [[f32; 4]],
    acc_w: u32,
    acc_h: u32,
    origin: (i32, i32),
    layer: &Layer,
    group: &[Layer],
    adjustment: &Adjustment,
    style: &LayerStyle,
    opacity: f32,
    env: CompositeEnv<'_>,
) -> Result<(), String> {
    let shape = shape_layer(layer, env);
    let adjusted = Adjusted {
        adjustment,
        guide: adjustment.guide_with_job(
            acc_w,
            acc_h,
            &|i| crate::doc_evaluation::guide_view(acc[i], env.transfer),
            env.job.checkpoints_only(),
        )?,
        guide_origin: origin,
        guide_size: (acc_w, acc_h),
    };
    composite_styled_into(
        acc,
        acc_w,
        acc_h,
        origin,
        &shape,
        group,
        style,
        opacity,
        env,
        Some(&adjusted),
    )
}
