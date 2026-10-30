//! The TYPED twin of `style_adjustment`: a layer style or a
//! Blend If on a 16-bit or 32-bit adjustment layer. Every rule is the byte
//! module's (its module doc, with the take's verdicts): the shape is the
//! mask over the canvas (S-MASK), the adjustment reads the accumulator after
//! the layer's own Below effects (POST) and is applied in place with alpha
//! kept at `k = mask × Fill × Blend If × opacity`, This Layer reads the
//! adjusted colour, and a clip base's members are frozen to the shape and
//! laid over the adjusted pixels as one package.
//!
//! `precision_render::render_styled` hands the styled compositor a
//! `StyledPixels::Opaque` shape over the frame with the layer's prepared
//! mask, and an [`Adjusted16`] — the adjustment and its Shadows/Highlights
//! guide, built from the accumulator before the layer draws anything — which
//! `precision_style_render::composite_direct_into` calls at step 3. The
//! per-pixel body is the MASTER `precision_render::adjust_backdrop_sample16`;
//! no effect, Blend If or clip arithmetic is restated. A knockout or grouped
//! interior's unit pass composites pixels, so it is handed
//! [`Adjusted16::interior_frame`]: the accumulator after the Below group
//! adjusted at `k = 1`, alpha kept, in f32 — the one frame the plan charges
//! (16 B/px and one pass, `PrecisionScene::plan`).

use crate::adjust_precision::PrecisionGuide;
use crate::blend::{composite_source_into, BlendMode};
use crate::blend_float::blend_kind_at;
use crate::job_control::JobContext;
use crate::precision_adjustment::PrecisionAdjustmentState;
use crate::precision_image::reserved_vec;
use crate::precision_render::adjust_backdrop_sample16;
use crate::precision_style_render::{blend_if_weight_at, StyleEnv, StyledLayer, StyledPixels};
use crate::style::BlendIf;
use crate::style_composite::{mix_premultiplied_in, BackdropSnapshot};

/// A styled adjustment's interior as the typed compositor applies it
/// (module doc): its state and its guide over the frame.
pub(crate) struct Adjusted16<'a> {
    pub state: &'a PrecisionAdjustmentState,
    pub guide: Option<&'a PrecisionGuide>,
}

impl Adjusted16<'_> {
    /// The interior onto `acc` (a frame of `dims` whose pixel `(x, y)` is
    /// document pixel `(x + origin.0, y + origin.1)`), the byte
    /// `style_adjustment::Adjusted::interior_into` statement for statement:
    /// each non-transparent pixel adjusted in place at `k = mask × fill ×
    /// Blend If × scale` in `shape.blend`, alpha kept; without `members`
    /// `scale` is `opacity`, with them `scale` is 1, the unit buffer
    /// composites over the adjusted pixel and the result mixes back at
    /// `opacity`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn interior_into(
        &self,
        acc: &mut [[f32; 4]],
        dims: (u32, u32),
        origin: (i32, i32),
        shape: &StyledLayer<'_>,
        blend_if: Option<(&BlendIf, Option<&BackdropSnapshot>)>,
        opacity: f32,
        members: Option<&[[f32; 4]]>,
        env: &StyleEnv<'_>,
        job: JobContext<'_>,
    ) -> Result<(), String> {
        let fill = crate::opacity_byte::composite(shape.style.fill_opacity, env.float);
        let scale = if members.is_some() { 1.0 } else { opacity };
        // The Fill and the opacity in one product (`opacity_byte::joint`).
        let joint = crate::opacity_byte::joint(fill, scale, env.float);
        let kind = env.kind(shape.blend);
        let normal = blend_kind_at(BlendMode::Normal, env.float);
        let width = dims.0 as usize;
        let (gx, gy) = (i64::from(origin.0), i64::from(origin.1));
        for (i, slot) in acc.iter_mut().enumerate() {
            if i % 4096 == 0 {
                job.check()?;
            }
            let (x, y) = ((i % width) as u32, (i / width) as u32);
            let xy = (i64::from(x) + gx, i64::from(y) + gy);
            let coverage = shape.mask.map_or(1.0, |m| m.at(xy.0, xy.1));
            let before = *slot;
            let mut px = before;
            let k0 = coverage * joint;
            if k0 > 0.0 && px[3] != 0.0 {
                let g = self.guide.map_or(0.0, |plane| plane.at(x, y));
                let weight = match blend_if {
                    None => 1.0,
                    Some((b, snapshot)) => {
                        let mut adjusted = px;
                        adjust_backdrop_sample16(
                            &mut adjusted,
                            self.state,
                            g,
                            1.0,
                            normal,
                            env,
                            xy,
                        )?;
                        let under = snapshot
                            .and_then(|s| s.at(i64::from(x), i64::from(y)))
                            .unwrap_or(before);
                        blend_if_weight_at(
                            b,
                            [adjusted[0], adjusted[1], adjusted[2]],
                            [under[0], under[1], under[2]],
                            env.float,
                        )
                    }
                };
                let k = k0 * weight;
                if k > 0.0 {
                    adjust_backdrop_sample16(&mut px, self.state, g, k, kind, env, xy)?;
                }
            }
            if let Some(unit) = members {
                let src = unit[i];
                if src[3] > 0.0 {
                    let mut one = [px];
                    composite_source_into(&mut one, 0, [src[0], src[1], src[2]], src[3], kind, xy);
                    px = one[0];
                }
                if opacity < 1.0 {
                    px = mix_premultiplied_in(before, px, opacity, !env.float);
                }
            }
            *slot = px;
        }
        Ok(())
    }

    /// `acc` (as it stands, after the Below group) adjusted at `k = 1` under
    /// Normal, alpha kept, a transparent pixel left as it is: the pixels a
    /// routed unit pass composites (module doc). Stays in f32, as a group
    /// buffer does.
    pub(crate) fn interior_frame(
        &self,
        acc: &[[f32; 4]],
        dims: (u32, u32),
        origin: (i32, i32),
        env: &StyleEnv<'_>,
        job: JobContext<'_>,
    ) -> Result<Vec<[f32; 4]>, String> {
        let normal = blend_kind_at(BlendMode::Normal, env.float);
        let width = dims.0 as usize;
        let (gx, gy) = (i64::from(origin.0), i64::from(origin.1));
        let mut frame = reserved_vec(acc.len())?;
        for (i, pixel) in acc.iter().enumerate() {
            if i % 4096 == 0 {
                job.check()?;
            }
            let mut out = *pixel;
            if out[3] != 0.0 {
                let (x, y) = ((i % width) as u32, (i / width) as u32);
                let g = self.guide.map_or(0.0, |plane| plane.at(x, y));
                adjust_backdrop_sample16(
                    &mut out,
                    self.state,
                    g,
                    1.0,
                    normal,
                    env,
                    (i64::from(x) + gx, i64::from(y) + gy),
                )?;
            }
            frame.push(out);
        }
        Ok(frame)
    }
}

/// `shape` with its pixels replaced by `frame` (an [`Adjusted16::
/// interior_frame`] of `dims` at `origin`) and no interior hook: what a
/// routed unit pass composites.
pub(crate) fn interior_layer<'a>(
    shape: &StyledLayer<'a>,
    frame: &'a [[f32; 4]],
    dims: (u32, u32),
    origin: (i32, i32),
) -> StyledLayer<'a> {
    StyledLayer {
        pixels: StyledPixels::Buffer {
            pixels: frame,
            width: dims.0,
            height: dims.1,
            origin,
        },
        mask: shape.mask,
        style: shape.style,
        blend_if: shape.blend_if,
        blend: shape.blend,
        opacity: shape.opacity,
        members_want_base: shape.members_want_base,
        adjusted: None,
    }
}
