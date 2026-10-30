//! The 16-bit and float regional compositor's STYLED per-sample bodies,
//! the typed twin of `doc_regional_style` (moved out of
//! `precision_regional_tree` in 13A-4b-i, verbatim): one sample of
//! `render_styled` → `composite_styled_into` (`styled_sample16`, the package
//! mixed back by `mix_premultiplied_in`, unclamped at float) and of the typed
//! `composite_direct_into` (`direct_sample16`). Index arithmetic only around
//! `composite_pixels_sample16`, `composite_contribution_sample` (through
//! `doc_regional_style::contributions_sample`), `replace_under_band_sample`
//! (through `doc_regional_style::band_sample`), `typed_coverage_byte`,
//! `composite_source_into` and `PreparedMask::at`; the ROUTED unit pass is
//! `regional_knockout::unit_pass_sample16`. The contract is `regional`'s
//! module doc.

use crate::blend::{composite_source_into, BlendMode};
use crate::clip_unit::{clip_alpha, ClipStep};
use crate::doc::MAX_PIXELS;
use crate::doc_regional_style::{band_sample, contributions_sample};
use crate::lab_blend::fill_in;
use crate::precision_regional_tree::{sample_layers, Node, PreparedStyle, UNSTYLABLE};
use crate::precision_render::{members_want_base_snapshot16, Kind};
use crate::precision_style_render::{
    composite_pixels_sample16, plane_pixels, typed_coverage_byte, PixelPass16, StyleEnv,
    StyledPixels,
};
use crate::regional_knockout::{base_only_backdrop, isolated_env, unit_pass_sample16};
use crate::style_composite::{mix_premultiplied_in, occluding_fill};
use crate::style_effects::EffectKind;
use crate::style_knockout::{backdrop_at, is_grouped_interior};

/// One sample of `render_styled` → `composite_styled_into` for `node` onto
/// `target[ti]`: the package (planes, opacity below 1, not Dissolve) — outside
/// the affected window the sample is untouched, inside it the direct steps run
/// at opacity 1 on a copy that `mix_premultiplied` mixes back, always — or the
/// direct steps at the layer's opacity. `stop` (13A-4b-ii-b-2, `Some(m)` only
/// from a staged ENTER / RESUME to a member cursor) leaves the unit's private
/// buffer after `members[..m]` in `target[ti]`; under the package the copy
/// holds it (the typed package is ONE canvas-sized copy, so there is no
/// window test and no mix: the buffer is the guide's input everywhere).
pub(crate) fn styled_sample16(
    target: &mut [[f32; 4]],
    ti: usize,
    node: &Node<'_>,
    style: &PreparedStyle,
    env: &StyleEnv<'_>,
    xy: (i64, i64),
    stop: Option<usize>,
) -> Result<(), String> {
    let opacity = node.layer.opacity;
    if !style.package {
        return direct_sample16(target, ti, node, style, opacity, env, xy, stop);
    }
    if stop.is_some() {
        let mut copy = [target[ti]];
        direct_sample16(&mut copy, 0, node, style, 1.0, env, xy, stop)?;
        target[ti] = copy[0];
        return Ok(());
    }
    let Some((x0, y0, x1, y1)) = style.window else {
        return Ok(());
    };
    if xy.0 < x0 || xy.1 < y0 || xy.0 >= x1 || xy.1 >= y1 {
        return Ok(()); // outside the affected window: untouched
    }
    let mut copy = [target[ti]];
    direct_sample16(&mut copy, 0, node, style, 1.0, env, xy, None)?;
    target[ti] = mix_premultiplied_in(target[ti], copy[0], opacity, !env.float);
    Ok(())
}

/// The typed `composite_direct_into` at one sample: Blend If reads `under`
/// (`target[ti]` before this layer — the snapshot's value), the Below
/// contributions, the pixel pass — a raster's stored sample inside its rect,
/// a group's children on a transparent sample (the reference renders the
/// whole group buffer first) — onto the target, or for a clip base into a
/// private sample at Normal/1, each member through `sample_layers` with the
/// alpha clamped to the SHAPE (the planes', else the pad-0 `typed_shape`'s
/// `typed_coverage_byte`, 0 outside the layer rect) after it, then
/// `composite_buffer_into`'s statement; the Interior contributions. With
/// `stop` (13A-4b-ii-b-2) the members `[..m]` (or the unit pass's), the
/// private buffer left in `target[ti]`, returning before
/// `composite_source_into` and the Interior contributions.
#[allow(clippy::too_many_arguments)]
fn direct_sample16(
    target: &mut [[f32; 4]],
    ti: usize,
    node: &Node<'_>,
    style: &PreparedStyle,
    opacity: f32,
    env: &StyleEnv<'_>,
    xy: (i64, i64),
    stop: Option<usize>,
) -> Result<(), String> {
    let layer = node.layer;
    let under = target[ti];
    let offset = match &layer.kind {
        Kind::Raster { offset, .. } => *offset,
        _ => (0, 0),
    };
    let split = style.planes.as_ref().map_or(0, |p| p.below.len());
    let (below_inks, interior_inks) = style.inks.split_at(split);
    // A styled adjustment is refused before the tree is built
    // (`regional::STYLED_ADJUSTMENT`).
    let occluding = layer.style.as_deref().map_or(0.0, |s| {
        occluding_fill(
            s,
            layer.blend_if.as_ref(),
            layer.blend_mode,
            env.float,
            false,
        )
    });
    if let Some(planes) = &style.planes {
        contributions_sample(
            target,
            ti,
            &planes.shape,
            &planes.below,
            below_inks,
            offset,
            opacity,
            occluding,
            xy,
        );
    }
    // The layer's pixel and its 16-bit alpha code (`StyledPixels::alpha16_code`,
    // the pad-0 shape's input), `None` outside a raster's rect.
    let pixel = match &layer.kind {
        Kind::Raster { image, offset } => {
            let (iw, ih) = image.dimensions();
            let sx = xy.0 - i64::from(offset.0);
            let sy = xy.1 - i64::from(offset.1);
            if sx < 0 || sy < 0 || sx >= i64::from(iw) || sy >= i64::from(ih) {
                None
            } else {
                let pixels = StyledPixels::Image {
                    image,
                    offset: *offset,
                };
                pixels
                    .sample_at(sx as u32, sy as u32)
                    .map(|p| (p, pixels.alpha16_code(p)))
            }
        }
        // A materialized group reads its buffer (`StyledPixels::Buffer`):
        // the regional tree renders the canvas, so it is canvas-sized at
        // origin (0, 0).
        Kind::Group(_) if style.buffer.is_some() => {
            let buffer = style.buffer.as_deref().unwrap_or_default();
            let pixels = StyledPixels::Buffer {
                pixels: buffer,
                width: env.canvas.0,
                height: env.canvas.1,
                origin: (0, 0),
            };
            pixels
                .sample_at(xy.0 as u32, xy.1 as u32)
                .map(|p| (p, pixels.alpha16_code(p)))
        }
        Kind::Group(_) => {
            // A styled group is isolated: D4's hard stop; a staged level's
            // held plane (13A-4b-i) in place of its children.
            let mut g = [[0.0f32; 4]];
            match &node.staged {
                Some(staged) => g[0] = staged.at(xy),
                None => sample_layers(&node.children, &mut g, 0, &isolated_env(env), xy)?,
            }
            let pixels = StyledPixels::Buffer {
                pixels: &g,
                width: 1,
                height: 1,
                origin: (0, 0),
            };
            Some((g[0], pixels.alpha16_code(g[0])))
        }
        // `render_styled` refuses any other kind, and the scene builder never
        // hands it one (`with_style`).
        _ => return Err(UNSTYLABLE.into()),
    };
    let coverage = node.mask.as_ref().map_or(1.0, |m| m.at(xy.0, xy.1));
    let pass = |mode: BlendMode, opacity: f32| PixelPass16 {
        joint: crate::opacity_byte::joint(style.fill, opacity, env.float),
        blend_if: layer.blend_if.as_ref(),
        kind: env.kind(mode),
        direct: mode == BlendMode::Normal,
        opacity,
        float: env.float,
        fill_blend: fill_in(mode, style.fill, env.float, env.lab()),
    };
    // `composite_pixels_into`'s early return, then its sample.
    let pixel_pass = |target: &mut [[f32; 4]], ti: usize, mode: BlendMode, opacity: f32| match pixel
    {
        Some((px, _)) if style.fill > 0.0 && opacity > 0.0 => {
            composite_pixels_sample16(target, ti, px, coverage, under, &pass(mode, opacity), xy)
        }
        _ => Ok(()),
    };
    // The footprint a clip base clamps its members to: the planes' shape,
    // else the pad-0 `typed_shape` (which exists unless the plane exceeds
    // `MAX_PIXELS`, where the reference forces the pass's own alpha back).
    let limit = if node.members.is_empty() {
        None
    } else {
        match &style.planes {
            Some(planes) => {
                let shape = &planes.shape;
                Some(
                    f32::from(shape.at(
                        xy.0 - i64::from(offset.0) - shape.origin.0,
                        xy.1 - i64::from(offset.1) - shape.origin.1,
                    )) / 255.0,
                )
            }
            None => {
                let dims = match &layer.kind {
                    Kind::Raster { image, .. } => image.dimensions(),
                    _ => env.canvas,
                };
                plane_pixels(dims, 0)
                    .is_ok_and(|p| p <= MAX_PIXELS)
                    .then(|| {
                        f32::from(pixel.map_or(0, |(_, a16)| typed_coverage_byte(a16, coverage)))
                            / 255.0
                    })
            }
        }
    };
    // The inside band's B0 (`composite_direct_into`, step 4): this sample
    // after the Below contributions, before the pixel pass.
    let b0 = backdrop_at(env.knockout, style.knockout, target[ti], 0);
    if style.routed {
        // ROUTED (13A-3c): the knockout / grouped-interior unit pass.
        unit_pass_sample16(
            target, ti, node, style, opacity, pixel, coverage, under, limit, env, xy, stop,
        )?;
    } else if node.members.is_empty() {
        pixel_pass(target, ti, layer.blend_mode, opacity)?;
    } else {
        let mut buf = [[0.0f32; 4]];
        pixel_pass(&mut buf, 0, BlendMode::Normal, 1.0)?;
        let a = buf[0][3];
        // `clip_unit`'s Open against the footprint (#2), then D4's
        // clipped-member rule, the STYLED base's loop: the base-only sample
        // (opened) as BOTH backdrops when a member asks for it.
        let footprint = limit.unwrap_or(a);
        buf[0][3] = clip_alpha(buf[0][3], footprint, ClipStep::Open);
        let base_only = [buf[0]];
        let member_env = StyleEnv {
            knockout: base_only_backdrop(
                members_want_base_snapshot16(&layer.clipped_members),
                &base_only,
            ),
            ..*env
        };
        let end = stop.map_or(node.members.len(), |m| m.min(node.members.len()));
        for member in &node.members[..end] {
            sample_layers(std::slice::from_ref(member), &mut buf, 0, &member_env, xy)?;
            buf[0][3] = clip_alpha(buf[0][3], footprint, ClipStep::Settle);
        }
        // A stop is a member cursor: the unit still open.
        if stop.is_some() {
            target[ti] = buf[0];
            return Ok(());
        }
        buf[0][3] = clip_alpha(buf[0][3], footprint, ClipStep::Close);
        let src = buf[0];
        let sa = src[3] * opacity;
        if sa > 0.0 {
            composite_source_into(
                target,
                ti,
                [src[0], src[1], src[2]],
                sa,
                env.kind(layer.blend_mode),
                xy,
            );
        }
    }
    if stop.is_some() {
        return Ok(());
    }
    if let Some(planes) = &style.planes {
        // Under D6 the five GROUPED contributions were composited inside the
        // unit; only the rest remain out here.
        // A band is an interior Stroke (`inside_bands`' rule), tested per
        // contribution so the per-sample loop allocates nothing.
        for (c, ink) in planes
            .interior
            .iter()
            .zip(interior_inks)
            .filter(|(c, _)| !(style.grouped && is_grouped_interior(c.kind)))
        {
            if c.kind == EffectKind::Stroke {
                band_sample(target, ti, b0, &planes.shape, c, ink, offset, opacity, xy);
                continue;
            }
            contributions_sample(
                target,
                ti,
                &planes.shape,
                std::slice::from_ref(c),
                std::slice::from_ref(ink),
                offset,
                opacity,
                occluding,
                xy,
            );
        }
    }
    Ok(())
}
