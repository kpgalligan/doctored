//! KERNEL: the Inner Shadow composited INSIDE the layer (#2's IK1) — the one
//! home of the rule every compositor path calls: the byte and typed direct
//! paths (`style_composite::composite_direct_into`,
//! `precision_style_render`), both regional samplers
//! (`doc_regional_style::inside_sample`) and the CMYK styled row
//! (`cmyk_style_row`, on five planes).
//!
//! # The rule
//!
//! An Inner Shadow is cast onto the layer's own surface, so on a pixel the
//! layer covers only partly it shades the layer there, never the stack the
//! layer lets through. Per pixel, with `a` the shape / 255 (the BYTE shape,
//! the plane's own domain), `cov` the shadow plane's byte (already `m · a`),
//! `T0` the stack after the layer's Below group and before its interior unit
//! — never a knockout's backdrop: the unit pass is `mix(acc, K, s)`, affine
//! in the shape from the accumulator — and `acc` the pixel as it stands, all
//! sums in premultiplied space:
//!
//! - `X = T0 + (acc − T0) / a`, the layer at full coverage ([`open_inside`]),
//!   used only as the shadow's backdrop;
//! - `X' = over(X, ink, min(1, cov / shape) · scale)` with the shadow's own
//!   blend;
//! - `acc = acc + a · (X' − X)` ([`land_inside`]).
//!
//! At `shape == 255` it is the over-the-composite statement, so the rule
//! runs only on the ring (`0 < shape < 255`, `cov > 0`) and every other pixel
//! keeps `style_composite::composite_contribution_sample` byte for byte. A
//! Dissolve layer and a styled adjustment keep over-the-composite
//! ([`inside_layer_applies`]): their pixel pass is not affine in the shape.
//!
//! Part 4's confirm take (`effect-kernels-578-p4/take1`, the runner
//! `scripts/photoshop-probe/effect_kernels_578_p4_take.py`) measured a match
//! on the anti-aliased ring: its eight Fill 100 % rows from 26 ... 28 codes
//! to at most 2 at 8 bits and 0.84 at 16, and effect-kernels-578's sixteen
//! Inner Shadow rows from 19 ... 35 to at most 1;
//! `core/tests/style_effect_kernels_578_p4_take.rs` and
//! `style_effect_kernels_578_take.rs` hold the takes and
//! `core/tests/style_inner_shadow_inside.rs` the oracle.

use crate::blend::{composite_source_into, BlendMode};
use crate::job_control::JobContext;
use crate::style_composite::{composite_contribution_sample, BandBackdrop};
use crate::style_effects::EffectKind;
use crate::style_render::{Contribution, Ink, InkDecode, Placement, Shape, StyleColors};

/// Whether an Inner Shadow composites inside the layer (module doc): IK1,
/// confirmed on part 4's take (`effect-kernels-578-p4/take1`, the runner
/// `scripts/photoshop-probe/effect_kernels_578_p4_take.py`). Off, every path
/// composites the shadow over the layer's composite as before.
const SHADOW_INSIDE_LAYER: bool = true;

/// Whether a layer in `mode` (a styled adjustment when `adjusted`) takes the
/// rule: never in Dissolve, whose coin toss lands a pixel at 0 or full, nor
/// for a styled adjustment, whose interior is not a pass over its shape
/// (module doc).
pub(crate) fn inside_layer_applies(mode: BlendMode, adjusted: bool) -> bool {
    SHADOW_INSIDE_LAYER && mode != BlendMode::Dissolve && !adjusted
}

/// `Some((a, sa))` for an Interior Inner Shadow at plane pixel `plane_xy`
/// where `0 < shape < 255` and `cov > 0`: `a = shape / 255` and the shadow's
/// alpha over the open layer, `sa = min(1, cov / shape) · scale`. `None`
/// everywhere else, where the contribution keeps
/// `composite_contribution_sample`. The caller guarantees what that body
/// does (`plane_xy` inside the plane, the coverage the shape's size).
pub(crate) fn inside_layer_weights(
    shape: &Shape,
    c: &Contribution,
    plane_xy: (i64, i64),
    scale: f32,
) -> Option<(f32, f32)> {
    if c.kind != EffectKind::InnerShadow || c.placement != Placement::Interior {
        return None;
    }
    let i = shape.idx(plane_xy.0 as u32, plane_xy.1 as u32);
    let (s, cov) = (shape.coverage[i], c.coverage[i]);
    if s == 0 || s == 255 || cov == 0 {
        return None;
    }
    let s = f32::from(s);
    Some((s / 255.0, (f32::from(cov) / s).min(1.0) * scale))
}

/// `X` of the rule (module doc): `t0 + (now − t0) / a` in premultiplied space
/// over `N` planes, the last one alpha, returned straight. Colour is divided
/// by the UNCLAMPED alpha, then alpha is clamped to [0, 1] and colour too
/// when `clamp` (every scene but a 32-bit float one, which keeps
/// extended-range colour); transparent at alpha <= 0. The clamp matters only
/// where `now` landed above the byte shape (a 16-bit or float alpha off the
/// byte grid, by up to half a byte code; an alpha × mask product): there `X`
/// overshoots, and the clamped `X` only weights the shadow ([`land_inside`]).
pub(crate) fn open_inside<const N: usize>(
    t0: [f32; N],
    now: [f32; N],
    a: f32,
    clamp: bool,
) -> [f32; N] {
    let k = N - 1;
    let alpha = t0[k] + (now[k] - t0[k]) / a;
    if alpha <= 0.0 {
        return [0.0; N];
    }
    let mut out = [0.0f32; N];
    for c in 0..k {
        let b = t0[c] * t0[k];
        let v = (b + (now[c] * now[k] - b) / a) / alpha;
        out[c] = if clamp { v.clamp(0.0, 1.0) } else { v };
    }
    out[k] = alpha.min(1.0);
    out
}

/// The rule's last line (module doc): `now + a · (shadowed − open)` in
/// premultiplied space over `N` planes, the last one alpha, returned
/// straight; alpha clamped to [0, 1], transparent at alpha <= 0, colour
/// clamped when `clamp`. The ONE landing of the rule.
///
/// It is a DELTA on the pixel as it stands, never a re-mix `mix(T0, X', a)`:
/// the two are the same sum, but the delta never rewrites the layer's own
/// alpha and colour from the byte shape, so a landed alpha that is not
/// `a` (a 16-bit or float alpha that is not a `× 257` code, an alpha × mask
/// product) is kept exactly, where a re-mix moves it by up to half a byte
/// code (100 codes of 65535 at alpha 32996). What the clamped `open` costs
/// is only shadow: at most `q · a · |ΔX|`, `q` the shadow's alpha.
pub(crate) fn land_inside<const N: usize>(
    now: [f32; N],
    open: [f32; N],
    shadowed: [f32; N],
    a: f32,
    clamp: bool,
) -> [f32; N] {
    let k = N - 1;
    let alpha = now[k] + a * (shadowed[k] - open[k]);
    if alpha <= 0.0 {
        return [0.0; N];
    }
    let mut out = [0.0f32; N];
    for c in 0..k {
        let v = (now[c] * now[k] + a * (shadowed[c] * shadowed[k] - open[c] * open[k])) / alpha;
        out[c] = if clamp { v.clamp(0.0, 1.0) } else { v };
    }
    out[k] = alpha.min(1.0);
    out
}

/// One plane pixel of an Interior contribution onto `target[ti]`: the rule
/// (module doc) where `t0` is `Some` and [`inside_layer_weights`] holds, else
/// `composite_contribution_sample`, unchanged. The caller guarantees what
/// that body does; `clamp` is false only on a 32-bit float scene.
#[allow(clippy::too_many_arguments)]
#[inline]
pub(crate) fn composite_inside_layer_sample(
    target: &mut [[f32; 4]],
    ti: usize,
    t0: Option<[f32; 4]>,
    shape: &Shape,
    c: &Contribution,
    ink: &Ink,
    decode: InkDecode<'_>,
    plane_xy: (i64, i64),
    scale: f32,
    occluding_fill: f32,
    clamp: bool,
    canvas_xy: (i64, i64),
) {
    let ring = t0.and_then(|t0| inside_layer_weights(shape, c, plane_xy, scale).map(|w| (t0, w)));
    let Some((t0, (a, sa))) = ring else {
        composite_contribution_sample(
            target,
            ti,
            shape,
            c,
            ink,
            decode,
            plane_xy,
            scale,
            occluding_fill,
            canvas_xy,
        );
        return;
    };
    let now = target[ti];
    let open = open_inside(t0, now, a, clamp);
    let (px, py) = plane_xy;
    let cs = decode.apply(ink.color.at(px + shape.origin.0, py + shape.origin.1));
    let mut shadowed = [open];
    composite_source_into(&mut shadowed, 0, cs, sa, ink.kind, canvas_xy);
    target[ti] = land_inside(now, open, shadowed[0], a, clamp);
}

/// `style_composite::composite_contribution`'s loop and early returns with
/// [`composite_inside_layer_sample`] as its body; `t0` is the ring's
/// `BandBackdrop::of_ring` over this same target, read only where the
/// weights hold.
#[allow(clippy::too_many_arguments)]
pub(crate) fn composite_inside_layer(
    target: &mut [[f32; 4]],
    acc_w: u32,
    acc_h: u32,
    origin: (i32, i32),
    rel: (i32, i32),
    shape: &Shape,
    c: &Contribution,
    t0: &BandBackdrop,
    opacity_scale: f32,
    occluding_fill: f32,
    clamp: bool,
    colors: StyleColors<'_>,
    job: JobContext<'_>,
) -> Result<(), String> {
    let scale = c.opacity * opacity_scale;
    if scale.is_nan() || scale <= 0.0 || c.coverage.len() != shape.coverage.len() {
        return Ok(());
    }
    let ink = c.ink(colors);
    let decode = colors.decode();
    let (sx, sy) = (i64::from(c.shift.0), i64::from(c.shift.1));
    let base_x = i64::from(rel.0) + shape.origin.0 + sx;
    let base_y = i64::from(rel.1) + shape.origin.1 + sy;
    let px0 = (-base_x).max(0);
    let py0 = (-base_y).max(0);
    let px1 = (i64::from(acc_w) - base_x).min(i64::from(shape.w));
    let py1 = (i64::from(acc_h) - base_y).min(i64::from(shape.h));
    if px0 >= px1 || py0 >= py1 {
        return Ok(());
    }
    for py in py0..py1 {
        job.check()?;
        let ay = base_y + py;
        for px in px0..px1 {
            let ax = base_x + px;
            let ai = (ay as u64 * u64::from(acc_w) + ax as u64) as usize;
            let canvas_xy = (ax + i64::from(origin.0), ay + i64::from(origin.1));
            // The store's binary search only on the ring.
            let under = inside_layer_weights(shape, c, (px, py), scale).and_then(|_| t0.at(ai));
            composite_inside_layer_sample(
                target,
                ai,
                under,
                shape,
                c,
                &ink,
                decode,
                (px, py),
                scale,
                occluding_fill,
                clamp,
                canvas_xy,
            );
        }
    }
    Ok(())
}
