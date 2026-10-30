//! Outer glow (Layer Style > Outer Glow): a glow around the
//! shape, OUTSIDE only, in two regimes split where Size·(1 − Spread) is 0
//! (`style_matte::law_applies`):
//!
//! - THE LAW (Size·(1 − Spread) > 0, #2): the box-pair law of the
//!   coverage (`style_matte::law_matte_with_job`: the band grown by `n =
//!   round(S · spread)` px from the anti-aliased contour, [`LAW_BAND`], then
//!   two passes of a `w = S − n` box), the Range `r` (0.01..=1) as `min(1,
//!   B / r)`, rounded once to a byte; the contour then maps that byte with
//!   no Range of its own (the inner glow law's precedent).
//! - TODAY'S KERNEL (Spread 100 % or Size 0): the coverage dilated by
//!   `size * spread`, with sigma 0 (nothing blurs), stretched to `min(1, a
//!   / 0.5)` (`style_matte::OUTER_GLOW`), the contour mapping it with the
//!   glow's Range by the Divide rule.
//!
//! Either plane is knocked out by the shape's own coverage
//! (`Shape::inverted`) so it lies only where the layer is transparent, and
//! emitted as ONE Below contribution with the effect's own blend mode
//! (Screen by default) and opacity, which lands at `sa / (1 − a)` under a
//! pixel the layer covers at `a` (`style_composite::contribution_alpha`, the
//! Below stroke band's rule), so after the layer's own pass the visible glow
//! is `g (1 − a)` in both regimes.
//!
//! # Why the law and the composite (#2)
//!
//! The third and fourth identify takes measured the Outer Glow as the inner
//! glow law's outside twin with its band measured from the anti-aliased
//! contour, and a partial pixel composited as `g (1 − a)` with no in-plane
//! knock-out (`glow_aa_578_law_fit.py`'s DESCRIPTION: the edge column reads
//! 255 (1 − c)). Declared as candidate O1 (O2: the binarized band) in
//! `scripts/photoshop-probe/effect_kernels_578_take.py` before any scoring,
//! O1 SHIPs offline by rule (ii) (its RECORDED: no existing row rises, all
//! 36 non-held lower, existing mean of row means 0.896 -> 0.336 codes;
//! style-parity-v1x take2 s12_p80 77 / 0.650 -> 65 / 0.201) where O2 is
//! STOP (four v1x rows rise), and was CONFIRMED by rule (ii) on the fresh
//! take of an anti-aliased rounded rect and a half plane with a 0.4 column
//! at 8 and 16 bits (`effect-kernels-578/take1`): Spread 0 and 40 % within
//! 1.67 / 0.051 codes from up to 25.7 / 1.27, Spread 70 % up to 6.5 / 0.054
//! from 28.0 / 2.60, and the Spread 100 % rows (today's kernel, the band
//! past S a named residual, 41 ... 68 codes) lower in their mean only, from
//! the composite. `core/tests/style_effect_kernels_578_take.rs` pins the
//! take and identify takes 3 and 4.
//!
//! # Why sigma / 2.5 and the stretch (#2, today's kernel)
//!
//! `Mdilate · 1/2.5 · Rscale`, style-parity-v1x's fitted member
//! (`style_v1x_take.py`), shipped on effects-578 take1
//! (`effects_578_take.py`, its RECORDED section): against the original
//! sigma `size / 2` with no stretch it is at or below on every fresh row and
//! strictly lower on the soft glow (A3's default glow 73.4 / 1.36 -> 20.9 /
//! 0.58 codes at 8 bits by the model), and style-parity-v1x take2's
//! aggregate is no worse (142.5 / 5.69 against 160.8 / 27.4 at 8 bits). The
//! stretch is applied to the blur's f32 result before the one rounding to a
//! byte, so the ramp keeps every code (stretching the blurred byte would
//! leave only even codes); `blur_reach` at the narrower sigma still bounds
//! it, because the f32 blur reads and writes exactly as far as the byte
//! one. The inner glow runs no Gaussian: it ships the measured law `L`
//! below Choke 100 % (#2, `style_fx_inner_glow`).
//!
//! # Why the knock-out happens in-plane, and in proportion to the coverage
//!
//! An outer glow is never drawn under the layer's solid pixels: a layer at
//! 0 % fill with an outer glow is a hollow halo, and the dialog has no
//! "layer knocks out" switch for it (that toggle is the drop shadow's
//! opt-out). Multiplying here rather than setting the compositor's
//! `knocked_out_by_shape` flag keeps the cached plane self-contained (no
//! shift, nothing left for composite time). The plane is multiplied by
//! `255 - coverage`, so under a pixel at alpha `a` at most `1 - a` of the
//! glow lands where the layer draws nothing over it (Fill 0 %: the hollow
//! halo `g (1 − a)`); where the layer's pass will cover the pixel, the
//! compositor lands the band at `sa / (1 − a·fill)` (`band_under_layer`,
//! #2), so the layer's `over` leaves exactly `g (1 − a)` of it, the
//! measured composite, where the plane alone under the layer would leave
//! `g (1 − a)²`.
//!
//! # Why size 0 emits nothing
//!
//! At size 0 both primitives are no-ops (spread is a fraction OF size, so
//! the dilation is 0 too) and the plane IS the shape; knocking it out by
//! the shape leaves only `coverage × (255 − coverage)` along anti-aliased
//! edges — a stray ring on soft edges. So a size-0 glow contributes
//! nothing at all, and the projection equals the unstyled layer's byte
//! for byte, which the tests pin.
//!
//! # Extent
//!
//! `LayerStyle::pad()` gives the plane the law's reach
//! (`style_matte::law_reach`: `n + 1 + w − 1`, the band and the box pair)
//! under the law, and `ceil(size * spread) + 1` (the dilation, sigma 0) in
//! today's kernel, plus one px on each side, so the halo is never clipped by
//! the plane's border; the outer edge of the halo sits at `size` px from the
//! shape.
//!
//! # Contour and Range
//!
//! The contour (`style_contour`) maps the plane in place through its
//! capped Below table (`below_lut`, so it fades out with the plane), before
//! the knock-out: with no Range under the law (the plane already holds
//! `min(1, B / r)`; Linear skips the pass), with the glow's Range by the
//! Divide rule in today's kernel (Linear at Range 50 % skips the pass).
//!
//! # Noise
//!
//! The noise (`style_noise`) adds its grain at each
//! pixel's layer coordinate in the same pass as the contour, after it, held
//! to the same Below cap (`below_cap_table` with no Range under the law,
//! since the plane it indexes already holds the Range), before the
//! knock-out; amount 0 skips it.
//!
//! # Gradient and Jitter
//!
//! A Gradient glow (`style_glow_gradient`) ramps its gradient across
//! the plane after the contour and noise pass and before the knock-out:
//! each pixel's place on the gradient comes from its plane value, scattered
//! by Jitter, and its coverage becomes the stops' opacity held to the same
//! Below cap, read on the plane before that pass (kept for the ramp only
//! when the pass changes it; under the law always kept, as the glow at Range
//! 50 %, `min(1, B / 0.5)` rounded once, the plane before the Range map that
//! `GLOW_CAP_BEFORE_CONTOUR` names and today's kernel's stretched matte
//! is); the contribution's colour is the
//! gradient sampled through that per-pixel `t` field. A Color glow runs none
//! of it.

use crate::job_control::JobContext;
use crate::style::{EffectKind, GlowFill, OuterGlow};
use crate::style_contour::{below_cap_table, below_lut, map_plane_with_job, BELOW_CAP_OUTER_GLOW};
use crate::style_glow_gradient::{
    ramp_color, ramp_field_with_job, GlowCap, GLOW_CAP_BEFORE_CONTOUR, GLOW_RAMP_SITE_AFTER_CONTOUR,
};
use crate::style_matte::{law_applies, law_byte, law_matte_with_job, LawBand, OUTER_GLOW};
use crate::style_noise::apply_noise_below_with_job;
use crate::style_render::{
    dilate_plane_with_job, multiply_planes, slot_color, Contribution, Placement, RenderContext,
    Shape,
};

/// The band the outer glow's law measures (module doc): the anti-aliased
/// contour, the confirmed candidate O1.
pub(crate) const LAW_BAND: LawBand = LawBand::AntiAliased;

/// Renders `fx` over `shape`: one Below contribution (kind OuterGlow, solid
/// colour or, for a Gradient glow, its ramp; the effect's blend and
/// opacity, no shift, not knocked out at composite time — the plane is
/// already knocked out by the shape). Nothing at opacity 0 or size 0
/// (module doc). The context goes unread: a glow has no light direction
/// and no canvas alignment. Polls `job` before every primitive and inside
/// the dilate and the blur (13A-5b-i).
pub(crate) fn render(
    fx: &OuterGlow,
    shape: &Shape,
    _ctx: &RenderContext,
    job: JobContext<'_>,
) -> Result<Vec<Contribution>, String> {
    if fx.opacity.is_nan() || fx.opacity <= 0.0 || fx.size.is_nan() || fx.size <= 0.0 {
        return Ok(Vec::new());
    }
    let gradient = fx.fill_type == GlowFill::Gradient;
    let law = law_applies(fx.size, fx.spread);
    // The Range: in the law's plane (`min(1, B / r)`), or the contour's
    // Divide in today's kernel (module doc, Contour and Range).
    let range = (!law).then_some(fx.range);
    let lut = below_lut(&fx.contour, fx.anti_aliased, range, BELOW_CAP_OUTER_GLOW);
    job.check()?;
    let (mut plane, unmapped) = if law {
        law_plane(fx, shape, gradient, job)?
    } else {
        let mut plane = shape.coverage.clone();
        let (dilate, sigma) = OUTER_GLOW.split(fx.size, fx.spread);
        job.check()?;
        dilate_plane_with_job(&mut plane, shape.w, shape.h, dilate, job)?;
        job.check()?;
        OUTER_GLOW.blur_with_job(&mut plane, shape.w, shape.h, sigma, job)?;
        // A Gradient glow keeps the unmapped plane when the pass below
        // changes it and the ramp reads it (its site or its cap;
        // `style_glow_gradient`). Dropped before the knock-out's inverted
        // plane is made, so it adds nothing to the effect's peak.
        let unmapped = (gradient
            && (!GLOW_RAMP_SITE_AFTER_CONTOUR || GLOW_CAP_BEFORE_CONTOUR)
            && (fx.noise > 0.0 || lut.is_some()))
        .then(|| plane.clone());
        (plane, unmapped)
    };
    if fx.noise > 0.0 {
        job.check()?;
        let cap = below_cap_table(range, BELOW_CAP_OUTER_GLOW);
        apply_noise_below_with_job(
            &mut plane,
            shape.w,
            shape.origin,
            fx.noise,
            EffectKind::OuterGlow as u64,
            lut.as_ref(),
            &cap,
            job,
        )?;
    } else if let Some(lut) = lut {
        job.check()?;
        map_plane_with_job(&mut plane, shape.w, &lut, None, job)?;
    }
    let ramp = if gradient {
        job.check()?;
        let unmapped = unmapped.as_deref();
        Some(ramp_field_with_job(
            &mut plane,
            unmapped.filter(|_| !GLOW_RAMP_SITE_AFTER_CONTOUR),
            shape.w,
            shape.origin,
            &fx.gradient,
            fx.jitter,
            EffectKind::OuterGlow as u64,
            Some(GlowCap {
                k: BELOW_CAP_OUTER_GLOW,
                plane: unmapped.filter(|_| GLOW_CAP_BEFORE_CONTOUR),
            }),
            job,
        )?)
    } else {
        None
    };
    drop(unmapped);
    job.check()?;
    let inverted = shape.inverted();
    job.check()?;
    multiply_planes(&mut plane, &inverted);
    let mut out = Contribution::solid(
        EffectKind::OuterGlow,
        plane,
        slot_color(fx.color, fx.linear),
        fx.blend,
        fx.opacity,
        Placement::Below,
    );
    if let Some(t) = ramp {
        out.color = ramp_color(&fx.gradient, shape.origin, shape.w, shape.h, t);
    }
    Ok(vec![out])
}

/// The law's byte plane before the contour (module doc): the coverage's
/// band and box pair `B` (`style_matte::law_matte_with_job`), the Range as
/// `min(1, B / r)`, one rounding; and, for a Gradient glow, the plane its
/// ramp's cap reads (module doc, Gradient and Jitter): the glow before its
/// Range map, which the take measured as the glow at Range 50 % (`v₀`,
/// `style_glow_gradient`; today's kernel keeps its stretched matte `min(1,
/// 2a)` for it), so `min(1, B / 0.5)` rounded once. A non-finite Range reads
/// as 1, as the inner glow's law reads it (`style_fx_inner_glow::law_plane`).
fn law_plane(
    fx: &OuterGlow,
    shape: &Shape,
    gradient: bool,
    job: JobContext<'_>,
) -> Result<(Vec<u8>, Option<Vec<u8>>), String> {
    let b = law_matte_with_job(
        &shape.coverage,
        shape.w,
        shape.h,
        fx.size,
        fx.spread,
        LAW_BAND,
        job,
    )?;
    let r = if fx.range.is_finite() {
        f64::from(fx.range).clamp(0.01, 1.0)
    } else {
        1.0
    };
    job.check()?;
    let unmapped = gradient.then(|| b.iter().map(|&v| law_byte((v / 0.5).min(1.0))).collect());
    job.check()?;
    let plane = b.iter().map(|&v| law_byte((v / r).min(1.0))).collect();
    Ok((plane, unmapped))
}
