//! Drop shadow (Layer Style > Drop Shadow): the layer's shape
//! through the box-pair law where Size·(1 − Spread) > 0
//! (`style_matte::law_matte_with_job`, the band grown by `n = round(S ·
//! spread)` px from the anti-aliased contour, [`LAW_BAND`], then two passes
//! of a `w = S − n` box, no Range, rounded once to a byte), and elsewhere
//! dilated by `size * spread` with no blur ([`MATTE`] at sigma 0); offset by
//! (`distance`, `angle`) and colourized, composited
//! BELOW the pixels with the effect's own blend mode and opacity. With
//! "Layer Knocks Out Drop Shadow" (`layer_knocks_out`, the default) the
//! shadow is not drawn under the layer's own shape, so a layer at 0 % fill
//! shows no shadow through itself.
//!
//! The contour (`style_contour`) maps the blurred plane in place
//! through its capped Below table (`below_lut`, so it fades out with the
//! plane), before the offset; Linear skips the pass.
//!
//! The noise (`style_noise`) adds its grain at each
//! pixel's layer coordinate in the same pass as the contour, after it, held
//! to the same Below cap, before the offset; amount 0 skips it.
//!
//! # Why the law (#2)
//!
//! The Gaussian below left 6 / 0.15 codes on A3's default shadow and 11 /
//! 0.79 on size 9 spread 30 %; its fitted ratio 0.4 sits near the box
//! pair's `1/√6`. The inner glow's law, declared as candidate D1 (the
//! anti-aliased band) and D2 (the binarized) in
//! `scripts/photoshop-probe/effect_kernels_578_take.py` before any scoring,
//! SHIPs offline by both rules (its RECORDED: soft-effects-578's held-out
//! rows 0.432 -> 0.043 codes mean of row means, ds_s9_sp30_a30 10.70 /
//! 0.819 -> 1.61 / 0.082 by the model; effects-578's existing rows 0.096 ->
//! 0.014, the hard row held) and was CONFIRMED on a fresh take of an
//! anti-aliased rounded rect and a half plane with a 0.4 column at 8 and 16
//! bits (`effect-kernels-578/take1`: Size 7 Spread 50 % 37.4 / 0.355 -> 2.9
//! / 0.012, every other row within 1.03 / 0.044). D2 was confirmed too but
//! reads the partial edge worse (18.7 / 0.059), so D1 is built.
//! `core/tests/style_effect_kernels_578_take.rs` pins the take.
//!
//! # The Gaussian it replaced
//!
//! `G1/2.5 · Rnone · γ 1` (sigma `size · (1 − spread) / 2.5`, no map, the
//! offset's half still rounded away from zero), fitted offline by
//! `soft_effects_578_fit.py` on effects-578 take1 against declared
//! families (Gaussian ratios 1/2, 1/2.5, 1/3, two box blurs, three range
//! maps, five gammas, four tie rules for the offset). By the model, A3's
//! default shadow (size 5, distance 5, 120°) went from 14.31 / 0.584 to
//! 5.60 / 0.168 codes at 8 bits (13.92 / 0.570 to 5.89 / 0.158 at 16), the
//! hard row unchanged; soft-effects-578 take1's held-out rows confirm it
//! (size 3 at 60° 16.70 / 0.395 -> 6.04 / 0.092; size 9 spread 30 % at 30°
//! 23.47 / 1.302 -> 10.70 / 0.819; size 12 at 90° 11.79 / 1.113 -> 6.18 /
//! 0.400), and every tie row prefers today's away-from-zero half. The
//! module's SHIP rows are recorded in the runner's RECORDED section.

use crate::job_control::JobContext;
use crate::style::{DropShadow, EffectKind};
use crate::style_contour::{below_cap_table, below_lut, map_plane_with_job, BELOW_CAP_DROP_SHADOW};
use crate::style_matte::{law_applies, law_byte, law_matte_with_job, LawBand, Matte, MatteRange};
use crate::style_noise::apply_noise_below_with_job;
use crate::style_render::{
    dilate_plane_with_job, light_offset, slot_color, Contribution, Part, Placement, RenderContext,
    Shape,
};

/// The band the drop shadow's law measures (module doc): the anti-aliased
/// contour, the confirmed candidate D1.
pub(crate) const LAW_BAND: LawBand = LawBand::AntiAliased;

/// The drop shadow's matte where the law does not run (Size·(1 − Spread)
/// == 0, so its sigma is 0 and only the dilation remains): `G1/2.5 · Rnone
/// · γ 1` (module doc), read by the render, `Effect::reach` and the typed
/// scratch plan's `blur_sigma`.
pub(crate) const MATTE: Matte = Matte {
    ratio: 0.4,
    range: MatteRange::None,
    gamma: 1.0,
};

/// Renders `fx` over `shape`: one Below contribution, shifted at composite
/// time by [`light_offset`] of the resolved angle and `distance`. Nothing
/// at opacity 0. With `size == 0 && spread == 0` the plane IS the shape
/// (both primitives are no-ops at 0), so a hard shadow is the shape
/// translated — the oracle the tests pin. Polls `job` before every
/// primitive and inside the dilate and the blur (13A-5b-i).
pub(crate) fn render(
    fx: &DropShadow,
    shape: &Shape,
    ctx: &RenderContext,
    job: JobContext<'_>,
) -> Result<Vec<Contribution>, String> {
    if fx.opacity.is_nan() || fx.opacity <= 0.0 {
        return Ok(Vec::new());
    }
    job.check()?;
    let mut plane = if law_applies(fx.size, fx.spread) {
        let m = law_matte_with_job(
            &shape.coverage,
            shape.w,
            shape.h,
            fx.size,
            fx.spread,
            LAW_BAND,
            job,
        )?;
        job.check()?;
        m.into_iter().map(law_byte).collect()
    } else {
        let mut plane = shape.coverage.clone();
        let (dilate, sigma) = MATTE.split(fx.size, fx.spread);
        job.check()?;
        dilate_plane_with_job(&mut plane, shape.w, shape.h, dilate, job)?;
        job.check()?;
        MATTE.blur_with_job(&mut plane, shape.w, shape.h, sigma, job)?;
        plane
    };
    let lut = below_lut(&fx.contour, fx.anti_aliased, None, BELOW_CAP_DROP_SHADOW);
    if fx.noise > 0.0 {
        job.check()?;
        let cap = below_cap_table(None, BELOW_CAP_DROP_SHADOW);
        apply_noise_below_with_job(
            &mut plane,
            shape.w,
            shape.origin,
            fx.noise,
            EffectKind::DropShadow as u64,
            lut.as_ref(),
            &cap,
            job,
        )?;
    } else if let Some(lut) = lut {
        job.check()?;
        map_plane_with_job(&mut plane, shape.w, &lut, None, job)?;
    }
    Ok(vec![Contribution {
        kind: EffectKind::DropShadow,
        part: Part::Main,
        coverage: plane,
        color: slot_color(fx.color, fx.linear),
        blend: fx.blend,
        opacity: fx.opacity,
        placement: Placement::Below,
        shift: light_offset(fx.resolved_angle(ctx.light), fx.distance),
        knocked_out_by_shape: fx.layer_knocks_out,
        instance: 0,
    }])
}
