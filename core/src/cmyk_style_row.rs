//! KERNEL: one STYLED CMYK record onto an ink row (#1, the styles batch) —
//! a raster, an isolated group or an adjustment carrying
//! layer effects, a Blend If, knockout or Blend Interior Effects as Group,
//! composited into one row of straight f32 ink by `cmyk_stack` (its adapter
//! is `cmyk_style`).
//!
//! Nothing here is new pixel math. The MASTER COPIES are the RGB styled
//! paths, restated on the row and on ink, statement for statement:
//! `style_composite::composite_direct_into` / `composite_package_into` (the
//! order, the package, the inside band — `replace_under_band_sample`),
//! `style_knockout::composite_interior_unit_into` (D3's unit, D6's grouped
//! interiors) and its typed twin `precision_style_knockout`. The ink
//! substitutions are the CMYK document's decisions: a contribution, the
//! layer's pixels and a finished buffer meet the row through
//! `cmyk_composite::over_mode` (a blend mode on `L = 1 − ink`, Photoshop
//! take1's E-L for every effect), a contribution's effective alpha is
//! `style_composite::contribution_alpha`, the premultiplied mix is
//! `style_composite::mix_premultiplied_planes` on five planes, a gradient's
//! ink is `GradientSampler::color_with` over the stops' INK (G-INK)
//! and a Blend If weight is `style_blend_if::source_weight_ink` (the rule
//! fitted to Photoshop, on the codes of the document's depth: `code_max`).
//!
//! The order inside one record (D3), per row:
//!
//! 1. the Blend If UNDERLYING: the row as it stood before anything of the
//!    record drew (the record's own Below effects are not under it);
//! 2. the Below contributions onto the row, each with its own mode and
//!    `sa = contribution_alpha × opacity`;
//! 3. `B0` under an inside band: the row after the Below group, or under a
//!    knockout the backdrop the record punches to;
//! 4. the INTERIOR UNIT: with no knockout and nothing grouped, the pixels
//!    straight onto the row through the record's mode at `alpha × mask ×
//!    fill × weight × opacity` (a clip base: its pixels and members into a
//!    buffer, each member with the footprint frozen (`clip_unit`), then the buffer through
//!    the mode). Under a knockout or grouped interiors (D3, D6), the unit
//!    `U` (pixels at Normal, members, grouped contributions with the alpha
//!    restored) meets `B0` once: `K = over_mode(B0, U, q, M)`, `q =
//!    fill·min(1, U.a / s)`, `out = mix(row, K, s·opacity)`, `s = alpha ×
//!    mask` — with `B0 = row` this IS step 4's plain statement;
//! 5. the remaining Interior contributions, the inside band REPLACING what
//!    lies under it (`lerp(X, over(B0, band), band/shape)`).
//!
//! At layer opacity < 1 with planes and a mode other than Dissolve the record
//! is the PACKAGE: steps 1–5 at opacity 1 on a copy of the row, mixed back at
//! the opacity over the window the record can touch.
//!
//! A styled ADJUSTMENT ([`StyledInk::tone`] set) has no pixels: its shape is
//! the opaque frame under its mask (S-MASK), and step 4 is its INTERIOR —
//! `precision_style_adjustment::Adjusted16::interior_into` (the master,
//! with `style_adjustment`'s byte twin) statement for statement on ink: each
//! sample with alpha adjusted in place by `cmyk_composite::tone_mix_mode` at
//! `k = mask × fill × weight × scale`, alpha kept, This Layer reading the
//! adjusted ink (POST: after the Below group). With clip members the unit
//! holds the members only, frozen to the footprint, laid over each adjusted
//! sample and mixed back at the opacity. A routed pass (knockout or grouped)
//! composites the row after the Below group adjusted at `k = 1` under Normal,
//! at its own alpha (`Adjusted16::interior_frame`).

use crate::blend::BlendMode;
use crate::clip_unit::{clip_alpha, ClipStep};
use crate::cmyk_adjust::CmykAdjustment;
use crate::cmyk_composite::{over_mode, tone_mix_mode, InkPixel};
use crate::pattern_tile::InkSampler;
use crate::precision_style_render::typed_coverage_byte;
use crate::style_blend_if::source_weight_ink;
use crate::style_composite::{contribution_alpha, mix_premultiplied_planes};
use crate::style_gradient::GradientSampler;
use crate::style_knockout::is_grouped_interior;
use crate::style_model::{BlendIf, Knockout};
use crate::style_render::{Contribution, RenderedEffects, Shape};

/// Transparent: what a knockout punches to with no backdrop (D4).
const TRANSPARENT: InkPixel = [0.0; 5];

/// One contribution's colour in INK, resolved once per render: a solid ink,
/// a gradient's sampler with one ink per stop, or a pattern's placed ink
/// level (`cmyk_style::effect_inks`).
#[derive(Clone)]
pub(crate) enum InkColor {
    Solid([f32; 4]),
    Gradient(GradientSampler, Vec<[f32; 4]>),
    Pattern(InkSampler),
}

impl InkColor {
    /// The ink at LAYER pixel `(lx, ly)` (`PlaneColor::at`'s rule: a
    /// gradient sampled at the pixel's centre, interpolated over the stops'
    /// ink; a pattern at the pixel's grid point, its alpha already in the
    /// contribution's coverage).
    pub(crate) fn at(&self, lx: i64, ly: i64) -> [f32; 4] {
        match self {
            InkColor::Solid(ink) => *ink,
            InkColor::Gradient(sampler, stops) => {
                let t = sampler.t(lx as f32 + 0.5, ly as f32 + 0.5);
                sampler.color_with(t, |i| stops[i]).0
            }
            InkColor::Pattern(sampler) => {
                let p = sampler.at(lx, ly);
                [p[0], p[1], p[2], p[3]]
            }
        }
    }
}

/// A styled CMYK adjustment's interior: its tone at the document's depth.
pub(crate) struct InkTone {
    pub kernel: CmykAdjustment,
    /// 8 or 16, the depth `CmykAdjustment::apply` reads.
    pub depth_bits: u32,
}

/// One styled record as the row compositor reads it. `below` and
/// `interior` are parallel to `planes`' two lists.
pub(crate) struct StyledInk {
    pub planes: Option<RenderedEffects>,
    pub below: Vec<InkColor>,
    pub interior: Vec<InkColor>,
    /// Fill Opacity, sanitized.
    pub fill: f32,
    /// What a Below stroke band divides by (`contribution_alpha`, #2): the
    /// adapter's `style_composite::occluding_fill`.
    pub occluding_fill: f32,
    /// Layer opacity, sanitized.
    pub opacity: f32,
    pub mode: BlendMode,
    pub blend_if: Option<BlendIf>,
    pub knockout: Knockout,
    /// D6: Blend Interior Effects as Group with a grouped contribution
    /// rendered (`style_knockout::grouped_interior`).
    pub grouped: bool,
    /// The document's code range, 255 at 8 bits and 65535 at 16: the codes a
    /// Blend If ramp reads (`style_blend_if::source_weight_ink`).
    pub code_max: f32,
    /// A styled adjustment's interior (module doc); `None` for a raster or a
    /// group.
    pub tone: Option<InkTone>,
}

/// The layer's own pixel at a canvas position: `(ink, alpha, mask
/// coverage)`, `None` outside the layer.
pub(crate) type PixelAt<'a> = dyn Fn(i64, i64) -> Option<([f32; 4], f32, f32)> + 'a;

/// A styled clip base's members: `(count, render)`, `render(i, row)`
/// composites member `i` onto the unit row (its own knockout environment is
/// the caller's, D4); the kernel clamps the row to the footprint after each.
pub(crate) type MembersRow<'a> = Option<(usize, &'a mut dyn FnMut(usize, &mut [InkPixel]))>;

/// The row cursor every pass shares.
struct RowAt {
    y: i64,
    x0: i64,
    /// Canvas columns between buffer indices (1 but for a reduced-resolution
    /// region render, `cmyk_stack::CmykStack::row_strided`).
    step: i64,
    origin: (i32, i32),
}

impl RowAt {
    /// Plane pixel of canvas column `x` for a contribution shifted by
    /// `shift`, `None` outside the plane.
    fn plane_xy(&self, shape: &Shape, x: i64, shift: (i32, i32)) -> Option<(i64, i64)> {
        let px = x - i64::from(self.origin.0) - shape.origin.0 - i64::from(shift.0);
        let py = self.y - i64::from(self.origin.1) - shape.origin.1 - i64::from(shift.1);
        (px >= 0 && py >= 0 && px < i64::from(shape.w) && py < i64::from(shape.h))
            .then_some((px, py))
    }
}

/// Composites `styled` onto `acc` (canvas row `y`, columns `x0 + k·step`), the
/// record's pixels read through `pixel`; `b0` is the knockout backdrop row
/// (`None` = transparent) the record punches to under `Knockout::Shallow` or
/// `Deep`; `origin` is the canvas pixel of the record's layer pixel (0, 0).
/// `scratch` holds the row buffers (module doc).
#[allow(clippy::too_many_arguments)]
pub(crate) fn composite_styled_row(
    acc: &mut [InkPixel],
    b0: Option<&[InkPixel]>,
    y: i64,
    x0: i64,
    step: i64,
    origin: (i32, i32),
    styled: &StyledInk,
    pixel: &PixelAt<'_>,
    members: MembersRow<'_>,
    scratch: &mut Vec<InkPixel>,
) {
    let n = acc.len();
    if n == 0 {
        return;
    }
    let step = step.max(1);
    let at = RowAt {
        y,
        x0,
        step,
        origin,
    };
    scratch.clear();
    // A styled adjustment's routed pass holds one more row (module doc).
    let rows = if styled.tone.is_some() { 6 } else { 5 };
    scratch.resize(n * rows, TRANSPARENT);
    let (package, rest) = scratch.split_at_mut(n);
    match styled.planes.as_ref() {
        Some(planes) if styled.opacity < 1.0 && styled.mode != BlendMode::Dissolve => {
            package.copy_from_slice(acc);
            direct(package, b0, &at, styled, 1.0, pixel, members, rest);
            // The window the record can touch (`style_composite::affected_window`).
            let shape = &planes.shape;
            let (mut lo_x, mut hi_x, mut lo_y, mut hi_y) = (0i64, 0i64, 0i64, 0i64);
            for c in &planes.below {
                lo_x = lo_x.min(i64::from(c.shift.0));
                hi_x = hi_x.max(i64::from(c.shift.0));
                lo_y = lo_y.min(i64::from(c.shift.1));
                hi_y = hi_y.max(i64::from(c.shift.1));
            }
            let base_x = i64::from(origin.0) + shape.origin.0;
            let base_y = i64::from(origin.1) + shape.origin.1;
            if y < base_y + lo_y || y >= base_y + i64::from(shape.h) + hi_y {
                return;
            }
            // Canvas columns to buffer indices, rounding up at both ends.
            let index = |x: i64| ((x - x0 + step - 1).div_euclid(step)).clamp(0, n as i64) as usize;
            let from = index(base_x + lo_x);
            let to = index(base_x + i64::from(shape.w) + hi_x);
            for k in from..to {
                acc[k] = mix_premultiplied_planes(acc[k], package[k], styled.opacity, true);
            }
        }
        _ => direct(acc, b0, &at, styled, styled.opacity, pixel, members, rest),
    }
}

/// Steps 1–5 of the module doc straight onto `t` at `opacity`.
#[allow(clippy::too_many_arguments)]
fn direct(
    t: &mut [InkPixel],
    b0: Option<&[InkPixel]>,
    at: &RowAt,
    styled: &StyledInk,
    opacity: f32,
    pixel: &PixelAt<'_>,
    members: MembersRow<'_>,
    scratch: &mut [InkPixel],
) {
    let n = t.len();
    let (under, rest) = scratch.split_at_mut(n);
    let (band_b0, rest) = rest.split_at_mut(n);
    let (unit, rest) = rest.split_at_mut(n);
    let (alphas, adjusted) = rest.split_at_mut(n);
    let backdrop = |row: &[InkPixel], k: usize| match styled.knockout {
        Knockout::None => row[k],
        _ => b0.and_then(|b| b.get(k).copied()).unwrap_or(TRANSPARENT),
    };
    // 1. The Blend If underlying.
    if styled.blend_if.is_some() {
        under.copy_from_slice(t);
    }
    let blend_if = styled
        .blend_if
        .as_ref()
        .map(|b| (b, &*under, styled.code_max));
    // 2. Below.
    if let Some(planes) = styled.planes.as_ref() {
        for (c, ink) in planes.below.iter().zip(&styled.below) {
            contribution_row(
                t,
                at,
                &planes.shape,
                c,
                ink,
                c.opacity * opacity,
                styled.occluding_fill,
            );
        }
    }
    // 3. B0 under the inside bands (one row, shared by every stacked band).
    let bands = styled
        .planes
        .as_ref()
        .map(|p| p.inside_bands())
        .unwrap_or_default();
    if !bands.is_empty() {
        for (k, b0) in band_b0.iter_mut().enumerate().take(n) {
            *b0 = backdrop(t, k);
        }
    }
    // 4. The interior unit.
    let routed = styled.knockout != Knockout::None || styled.grouped;
    if let Some(tone) = styled.tone.as_ref() {
        if routed {
            // The row after the Below group adjusted at `k = 1` under Normal,
            // at its own alpha (`Adjusted16::interior_frame`).
            for (dst, src) in adjusted.iter_mut().zip(t.iter()) {
                *dst = if src[4] > 0.0 {
                    let a = tone
                        .kernel
                        .apply([src[0], src[1], src[2], src[3]], tone.depth_bits);
                    [a[0], a[1], a[2], a[3], src[4]]
                } else {
                    TRANSPARENT
                };
            }
            let row: &[InkPixel] = adjusted;
            let routed_pixel = |x: i64, y: i64| -> Option<([f32; 4], f32, f32)> {
                let (_, _, coverage) = pixel(x, y)?;
                let p = row[row_index(at, x, y, n)?];
                Some(([p[0], p[1], p[2], p[3]], p[4], coverage))
            };
            unit_pass(
                t,
                at,
                styled,
                opacity,
                &routed_pixel,
                members,
                blend_if,
                unit,
                alphas,
                &backdrop,
            );
        } else {
            tone_interior(t, at, styled, tone, opacity, pixel, members, blend_if, unit);
        }
    } else if routed {
        unit_pass(
            t, at, styled, opacity, pixel, members, blend_if, unit, alphas, &backdrop,
        );
    } else if let Some((count, render)) = members {
        unit.fill(TRANSPARENT);
        pixels_row(
            unit,
            at,
            pixel,
            styled.fill,
            blend_if,
            BlendMode::Normal,
            1.0,
        );
        // `clip_unit`'s Open / Settle / Close (#2).
        clip_footprint(unit, at, styled, pixel, ClipStep::Open);
        for i in 0..count {
            render(i, unit);
            clip_footprint(unit, at, styled, pixel, ClipStep::Settle);
        }
        clip_footprint(unit, at, styled, pixel, ClipStep::Close);
        for (k, (dst, src)) in t.iter_mut().zip(unit.iter()).enumerate() {
            let sa = src[4] * opacity;
            if sa > 0.0 {
                let xy = (at.x0 + k as i64 * at.step, at.y);
                over_mode(dst, [src[0], src[1], src[2], src[3]], sa, styled.mode, xy);
            }
        }
    } else {
        pixels_row(t, at, pixel, styled.fill, blend_if, styled.mode, opacity);
    }
    // 5. The remaining interior contributions.
    if let Some(planes) = styled.planes.as_ref() {
        for (j, (c, ink)) in planes.interior.iter().zip(&styled.interior).enumerate() {
            if styled.grouped && is_grouped_interior(c.kind) {
                continue;
            }
            let scale = c.opacity * opacity;
            if bands.contains(&j) {
                replace_under_band_row(t, at, &planes.shape, c, ink, scale, band_b0);
            } else {
                contribution_row(t, at, &planes.shape, c, ink, scale, styled.occluding_fill);
            }
        }
    }
}

/// The buffer index of canvas `(x, y)` on `at`'s row of `n`, `None` off it.
fn row_index(at: &RowAt, x: i64, y: i64, n: usize) -> Option<usize> {
    let d = x - at.x0;
    if y != at.y || d < 0 || d % at.step != 0 {
        return None;
    }
    usize::try_from(d / at.step).ok().filter(|&k| k < n)
}

/// Step 4 of a styled ADJUSTMENT (module doc):
/// `precision_style_adjustment::Adjusted16::interior_into`, the master, on
/// ink — each sample with alpha adjusted in place by `tone_mix_mode` at `k =
/// mask × fill × weight × scale`, alpha kept, the weight reading the adjusted
/// ink against the underlying row. Without `members` `scale` is `opacity`;
/// with them it is 1, the unit (the members only, frozen to the footprint)
/// is laid over each adjusted sample through the mode and the result mixes
/// back toward the sample before the step at `opacity`.
#[allow(clippy::too_many_arguments)]
fn tone_interior(
    t: &mut [InkPixel],
    at: &RowAt,
    styled: &StyledInk,
    tone: &InkTone,
    opacity: f32,
    pixel: &PixelAt<'_>,
    members: MembersRow<'_>,
    blend_if: Option<(&BlendIf, &[InkPixel], f32)>,
    unit: &mut [InkPixel],
) {
    let has_members = members.is_some();
    if let Some((count, render)) = members {
        unit.fill(TRANSPARENT);
        // `clip_unit`'s Open / Settle / Close (#2): no base pixels in it.
        clip_footprint(unit, at, styled, pixel, ClipStep::Open);
        for i in 0..count {
            render(i, unit);
            clip_footprint(unit, at, styled, pixel, ClipStep::Settle);
        }
        clip_footprint(unit, at, styled, pixel, ClipStep::Close);
    }
    let scale = if has_members { 1.0 } else { opacity };
    let joint = crate::opacity_byte::joint(styled.fill, scale, false);
    for (k, slot) in t.iter_mut().enumerate() {
        let x = at.x0 + k as i64 * at.step;
        let Some((_, _, coverage)) = pixel(x, at.y) else {
            continue;
        };
        let xy = (x, at.y);
        let before = *slot;
        let mut px = before;
        let k0 = coverage * joint;
        if k0 > 0.0 && px[4] > 0.0 {
            let adjusted = tone
                .kernel
                .apply([px[0], px[1], px[2], px[3]], tone.depth_bits);
            let weight = blend_if.map_or(1.0, |(b, under, code_max)| {
                let u = under[k];
                source_weight_ink(b, adjusted, [u[0], u[1], u[2], u[3]], code_max)
            });
            let amount = k0 * weight;
            if amount > 0.0 {
                tone_mix_mode(&mut px, adjusted, amount, styled.mode, xy);
            }
        }
        if has_members {
            let src = unit[k];
            if src[4] > 0.0 {
                over_mode(
                    &mut px,
                    [src[0], src[1], src[2], src[3]],
                    src[4],
                    styled.mode,
                    xy,
                );
            }
            if opacity < 1.0 {
                px = mix_premultiplied_planes(before, px, opacity, true);
            }
        }
        *slot = px;
    }
}

/// D3's unit pass (`style_knockout::composite_interior_unit_into`, the
/// master): the pixels at Normal into `unit` (fill 1 when grouped, D6),
/// the clip members onto it with its alpha frozen relative to the footprint
/// (`clip_unit`, #2), the grouped contributions with
/// the unit's alpha restored after each, then per pixel `K =
/// over_mode(B0, U, q, M)` mixed into `t` by the shape × opacity.
#[allow(clippy::too_many_arguments)]
fn unit_pass(
    t: &mut [InkPixel],
    at: &RowAt,
    styled: &StyledInk,
    opacity: f32,
    pixel: &PixelAt<'_>,
    members: MembersRow<'_>,
    blend_if: Option<(&BlendIf, &[InkPixel], f32)>,
    unit: &mut [InkPixel],
    alphas: &mut [InkPixel],
    backdrop: &dyn Fn(&[InkPixel], usize) -> InkPixel,
) {
    let (fill_in, fill_out) = if styled.grouped {
        (1.0, styled.fill)
    } else {
        (styled.fill, 1.0)
    };
    unit.fill(TRANSPARENT);
    pixels_row(unit, at, pixel, fill_in, blend_if, BlendMode::Normal, 1.0);
    if let Some((count, render)) = members {
        clip_footprint(unit, at, styled, pixel, ClipStep::Open);
        for i in 0..count {
            render(i, unit);
            clip_footprint(unit, at, styled, pixel, ClipStep::Settle);
        }
        clip_footprint(unit, at, styled, pixel, ClipStep::Close);
    }
    if styled.grouped {
        alphas.copy_from_slice(unit);
        if let Some(planes) = styled.planes.as_ref() {
            for (c, ink) in planes.interior.iter().zip(&styled.interior) {
                if !is_grouped_interior(c.kind) {
                    continue;
                }
                contribution_row(unit, at, &planes.shape, c, ink, c.opacity, 0.0);
                for (px, kept) in unit.iter_mut().zip(alphas.iter()) {
                    px[4] = kept[4];
                }
            }
        }
    }
    for k in 0..t.len() {
        let x = at.x0 + k as i64 * at.step;
        let Some((_, alpha, coverage)) = pixel(x, at.y) else {
            continue;
        };
        let s = alpha * coverage;
        if s <= 0.0 {
            continue;
        }
        let u = unit[k];
        let q = (fill_out * (u[4] / s).min(1.0)).clamp(0.0, 1.0);
        // A `q` of 0 is the HOLE: `K` stays `B0`.
        let mut kp = backdrop(t, k);
        if q > 0.0 {
            over_mode(&mut kp, [u[0], u[1], u[2], u[3]], q, styled.mode, (x, at.y));
        }
        t[k] = mix_premultiplied_planes(t[k], kp, s * opacity, true);
    }
}

/// The record's pixels onto `t` through `mode` at `alpha × mask × fill ×
/// weight × opacity` (`precision_style_render::composite_pixels_sample16`,
/// the master, on ink): a transparent target under Normal takes the pixel
/// directly. The weight reads the underlying row.
fn pixels_row(
    t: &mut [InkPixel],
    at: &RowAt,
    pixel: &PixelAt<'_>,
    fill: f32,
    blend_if: Option<(&BlendIf, &[InkPixel], f32)>,
    mode: BlendMode,
    opacity: f32,
) {
    if fill <= 0.0 || opacity <= 0.0 {
        return;
    }
    // The Fill and the opacity in one product (`opacity_byte::joint`).
    let joint = crate::opacity_byte::joint(fill, opacity, false);
    for (k, dst) in t.iter_mut().enumerate() {
        let x = at.x0 + k as i64 * at.step;
        let Some((ink, alpha, coverage)) = pixel(x, at.y) else {
            continue;
        };
        let weight = blend_if.map_or(1.0, |(b, under, code_max)| {
            let u = under[k];
            source_weight_ink(b, ink, [u[0], u[1], u[2], u[3]], code_max)
        });
        let sa = alpha * coverage * weight * joint;
        if sa <= 0.0 {
            continue;
        }
        if mode == BlendMode::Normal && dst[4] <= 0.0 {
            *dst = [ink[0], ink[1], ink[2], ink[3], sa];
            continue;
        }
        over_mode(dst, ink, sa, mode, (x, at.y));
    }
}

/// `clip_unit`'s `step` against the footprint, the SHAPE
/// (`style_composite::clip_pass_shape`, the master) — the planes' when
/// rendered, else the record's own `alpha × mask` on the byte grid
/// (`typed_coverage_byte`, the pad-0 shape).
fn clip_footprint(
    buf: &mut [InkPixel],
    at: &RowAt,
    styled: &StyledInk,
    pixel: &PixelAt<'_>,
    step: ClipStep,
) {
    for (k, px) in buf.iter_mut().enumerate() {
        let x = at.x0 + k as i64 * at.step;
        let limit = match styled.planes.as_ref() {
            Some(planes) => at
                .plane_xy(&planes.shape, x, (0, 0))
                .map_or(0.0, |(px, py)| f32::from(planes.shape.at(px, py)) / 255.0),
            None => pixel(x, at.y).map_or(0.0, |(_, alpha, coverage)| {
                f32::from(typed_coverage_byte((alpha * 65535.0).round(), coverage)) / 255.0
            }),
        };
        px[4] = clip_alpha(px[4], limit, step);
    }
}

/// One contribution onto `t` (`style_composite::composite_contribution`,
/// the master): `sa = contribution_alpha(..., scale, occluding_fill)`, the ink at the plane
/// pixel's layer coordinate, `over_mode` with the contribution's own mode.
fn contribution_row(
    t: &mut [InkPixel],
    at: &RowAt,
    shape: &Shape,
    c: &Contribution,
    ink: &InkColor,
    scale: f32,
    occluding_fill: f32,
) {
    if scale.is_nan() || scale <= 0.0 || c.coverage.len() != shape.coverage.len() {
        return;
    }
    for (k, dst) in t.iter_mut().enumerate() {
        let x = at.x0 + k as i64 * at.step;
        let Some((px, py)) = at.plane_xy(shape, x, c.shift) else {
            continue;
        };
        let sa = contribution_alpha(shape, c, (px, py), scale, occluding_fill);
        if sa <= 0.0 {
            continue;
        }
        let cs = ink.at(px + shape.origin.0, py + shape.origin.1);
        over_mode(dst, cs, sa, c.blend, (x, at.y));
    }
}

/// The inside band's REPLACEMENT on the row
/// (`style_composite::replace_under_band_sample`, the master): with `s =
/// band / shape`, `t = lerp(t, over(B0, band at shape·scale), s)`.
fn replace_under_band_row(
    t: &mut [InkPixel],
    at: &RowAt,
    shape: &Shape,
    c: &Contribution,
    ink: &InkColor,
    scale: f32,
    band_b0: &[InkPixel],
) {
    // A band at opacity 0 still replaces (`Ocut`).
    if scale.is_nan() || scale < 0.0 || c.coverage.len() != shape.coverage.len() {
        return;
    }
    for (k, dst) in t.iter_mut().enumerate() {
        let x = at.x0 + k as i64 * at.step;
        let Some((px, py)) = at.plane_xy(shape, x, c.shift) else {
            continue;
        };
        let i = shape.idx(px as u32, py as u32);
        let band = c.coverage[i];
        let cov = shape.coverage[i];
        if band == 0 || cov == 0 {
            continue;
        }
        let s = (f32::from(band) / f32::from(cov)).min(1.0);
        let sa = f32::from(cov) / 255.0 * scale;
        let mut replaced = band_b0[k];
        if sa > 0.0 {
            let cs = ink.at(px + shape.origin.0, py + shape.origin.1);
            over_mode(&mut replaced, cs, sa, c.blend, (x, at.y));
        }
        *dst = mix_premultiplied_planes(*dst, replaced, s, true);
    }
}
