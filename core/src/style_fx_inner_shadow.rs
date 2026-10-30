//! Inner shadow (Layer Style > Inner Shadow): the shadow a raised
//! edge casts INTO the shape — the layer read as a hole cut in the surface
//! and lit from `angle`, so the rim nearest the light throws a shadow onto
//! the pixels along that edge.
//!
//! # Algorithm
//!
//! The plane starts as the INVERTED shape (`255 - coverage`: opaque outside,
//! clear inside), because the shadow is what the outside casts inward:
//!
//! 1. Where Size·(1 − Choke) > 0, the box-pair law on the inverted plane
//!    (`style_matte::law_matte_with_job`, the drop shadow's with the choke
//!    as the spread): the OUTSIDE grown into the shape by `n = round(S ·
//!    choke)` px from the anti-aliased contour ([`LAW_BAND`]) — "choke" is
//!    spread's mirror, the hard part of the shadow pulled inward rather
//!    than pushed outward — then two passes of a `w = S − n` box, rounded
//!    once to a byte. Elsewhere `(dilate, sigma) = MATTE.split(size,
//!    choke)` has sigma 0, so `dilate_plane_with_job` grows the outside by
//!    `size * choke` and nothing blurs. Every primitive is the shared one
//!    (`style_matte`, `style_render`), never a second blur or EDT.
//! 2. The plane is shifted by `light_offset(resolved angle, distance)`,
//!    exactly the drop shadow's offset for the same light: at angle 0 the
//!    light comes from the right, the offset is `(-distance, 0)`, the
//!    outside slides LEFT into the shape and the shadow is the band inside
//!    the RIGHT edge; at 90° (light from the top) it sits under the top
//!    edge; at the default 120° it hugs the top-left edges. The
//!    shift fills with 255 — everything beyond the plane is "outside" too,
//!    so a region shifted in from the border must stay shadow rather than
//!    open a clear seam. `LayerStyle::pad` adds `ceil(distance)` to this
//!    effect's reach for precisely this step, so the fill is never actually
//!    read inside the layer rect and the plane renders as it would on an
//!    infinite one.
//! 3. The plane is multiplied by the shape coverage. Every Interior
//!    contribution must be bounded by the shape (`style_composite` draws it
//!    straight onto the accumulator on that promise), and for an inner
//!    shadow the clip IS the effect: only what falls on the pixels shows.
//!
//! Why the shift happens inside the plane rather than at composite time as
//! the drop shadow's does: the drop shadow is clipped by nothing (or knocked
//! out at the landing position), but an inner shadow is clipped to the
//! shape AFTER the shift, so the shift has to precede the multiply.
//!
//! With `size == 0 && choke == 0` both primitives are no-ops and the plane
//! is exactly the inverted shape translated: for a hard rect the shadow is
//! `rect \ (rect + offset)` at full coverage — the closed form the tests
//! pin. With `choke == 1` the plane is the grown inverted shape, so the
//! band follows the grow-rect closed form of `doc_select::grow_mask`.
//!
//! One Interior contribution (kind `InnerShadow`, solid colour, the
//! effect's own blend mode and opacity); none at opacity 0 or when the
//! clipped plane is empty (a hard shape at distance 0 with no size casts
//! nothing, and an all-transparent shape has nothing to cast onto).
//!
//! The contour (`style_contour`) maps the plane before the coverage
//! multiply (the Interior rule); Linear skips it. Where the law runs it
//! reads the law's unrounded matte, each value through `read_lut` (the
//! table read piecewise linearly at `255 m`) and rounded once by
//! `law_byte`, before the shift, whose fill is then `C(255)`
//! ([`CONTOUR_UNROUNDED`]; the map is per pixel, so mapping before the
//! shift equals mapping after it); elsewhere it maps the shifted byte plane
//! in place between steps 2 and 3.
//!
//! The noise (`style_noise`) adds its grain in place
//! at each pixel's layer coordinate, after the contour and before the
//! coverage multiply; amount 0 skips the pass.
//!
//! # Why the law (#2)
//!
//! The drop shadow's reason (`style_fx_drop_shadow`): the inner glow's law
//! on `1 − coverage`, declared as candidates I1 (the anti-aliased band) and
//! I2 (the binarized) in `scripts/photoshop-probe/effect_kernels_578_take.py`
//! before any scoring, SHIPs offline by both rules (its RECORDED:
//! soft-effects-578's held-out rows 0.143 -> 0.014 codes mean of row means,
//! is_s9_ch30_a30 5.68 / 0.287 -> 0.71 / 0.029 by the model; effects-578's
//! existing rows 0.036 -> 0.008, the hard row held) and I1 was CONFIRMED by
//! both rules on the fresh take (`effect-kernels-578/take1`: all 16 rows
//! within 1.05 / 0.028 codes, from up to 5.65 / 0.070). The shifted plane is
//! the same as before the law: only the matte changed.
//! `core/tests/style_effect_kernels_578_take.rs` pins the take.
//!
//! # Why the contour reads the unrounded matte (#2 part 4)
//!
//! The Satin's measured site (#2 part 3, `style_fx_satin`: the contour
//! on each shifted copy of the matte) declared for the inner shadow as
//! candidate IC1 in `scripts/photoshop-probe/effect_kernels_578_p4_take.py`
//! before any take (IC2, the complement table `255 − C(255 − x)`, was the
//! alternative): the contour on the matte before the composite, read
//! unrounded. It SHIPs offline by rule (ii) (its RECORDED: the identify
//! take's 64 contoured rows 0.352 -> 0.308 codes mean of row means,
//! contour-557's is_cone 2.0 / 0.241 -> 1.0 / 0.134, none rising; IC2
//! KEEP, 255-code misses) and was CONFIRMED by rule (ii) on a fresh take of
//! an anti-aliased ellipse and rounded rect at 8 and 16 bits
//! (`effect-kernels-578-p4/take1`: Cone, Half Round and Ring, 0.121 ->
//! 0.070, none rising). Ring still misses by up to 7 codes (Photoshop's
//! residual, recorded there). A Linear contour, and an inner shadow outside
//! the law, render byte for byte as before.
//! `core/tests/style_effect_kernels_578_p4_law.rs` holds the oracle and
//! `style_effect_kernels_578_p4_take.rs` the take.
//!
//! # The Gaussian it replaced
//!
//! `G1/2.5 · Rnone · γ 1` (sigma `size · (1 − choke) / 2.5`, no map, the
//! offset's half still rounded away from zero), fitted offline by
//! `soft_effects_578_fit.py` on effects-578 take1 with the drop shadow's
//! families and ONE offset rule for both. By the model, A3's default inner
//! shadow (size 5, distance 5, 120°) went from 7.36 / 0.247 to 2.81 /
//! 0.072 codes at 8 bits (6.89 / 0.243 to 2.74 / 0.067 at 16), the hard row
//! unchanged; soft-effects-578 take1's held-out rows confirm it (size 3 at
//! 60° 8.12 / 0.188 -> 2.94 / 0.039; size 9 choke 30 % at 30° 7.69 / 0.406
//! -> 5.68 / 0.287; size 8 at 90° 6.95 / 0.357 -> 3.45 / 0.115). The
//! module's SHIP rows are recorded in the runner's RECORDED section.

use crate::job_control::JobContext;
use crate::style::{EffectKind, InnerShadow};
use crate::style_contour::{effect_lut, map_plane_with_job, read_lut};
use crate::style_matte::{law_applies, law_byte, law_matte_with_job, LawBand, Matte, MatteRange};
use crate::style_noise::{apply_noise_with_job, NOISE_AFTER_CONTOUR};
use crate::style_render::{
    dilate_plane_with_job, light_offset, multiply_planes, shift_plane, slot_color, Contribution,
    Placement, RenderContext, Shape,
};

/// The band the inner shadow's law measures (module doc): the anti-aliased
/// contour, the confirmed candidate I1.
pub(crate) const LAW_BAND: LawBand = LawBand::AntiAliased;

/// Whether the contour reads the law's UNROUNDED matte (module doc, "Why
/// the contour reads the unrounded matte"): the confirmed candidate IC1.
/// Outside the law, and for a Linear contour, nothing changes.
const CONTOUR_UNROUNDED: bool = true;

/// The inner shadow's matte where the law does not run (Size·(1 − Choke)
/// == 0, so its sigma is 0 and only the dilation remains): `G1/2.5 · Rnone
/// · γ 1` (module doc), read by the render, `Effect::reach` and the typed
/// scratch plan's `blur_sigma`.
pub(crate) const MATTE: Matte = Matte {
    ratio: 0.4,
    range: MatteRange::None,
    gamma: 1.0,
};

/// Renders `fx` over `shape`: one Interior contribution, or none when the
/// effect is invisible (opacity 0, or a clipped plane that is all zero).
/// Polls `job` before every primitive and inside the dilate and the blur
/// (13A-5b-i).
pub(crate) fn render(
    fx: &InnerShadow,
    shape: &Shape,
    ctx: &RenderContext,
    job: JobContext<'_>,
) -> Result<Vec<Contribution>, String> {
    if fx.opacity.is_nan() || fx.opacity <= 0.0 {
        return Ok(Vec::new());
    }
    job.check()?;
    let lut = effect_lut(&fx.contour, fx.anti_aliased, None);
    let mut plane = shape.inverted();
    // The shift's fill and whether the contour still has to run after the
    // shift: under the law with a contour (`CONTOUR_UNROUNDED`) the matte is
    // mapped here, so beyond the plane is `C(255)` and no pass follows. This
    // runs the contour before the noise, which assumes `NOISE_AFTER_CONTOUR`.
    let mut fill = 255u8;
    let mut mapped = false;
    if law_applies(fx.size, fx.choke) {
        let m = law_matte_with_job(&plane, shape.w, shape.h, fx.size, fx.choke, LAW_BAND, job)?;
        job.check()?;
        plane = match lut.as_ref().filter(|_| CONTOUR_UNROUNDED) {
            Some(lut) => {
                fill = lut[255];
                mapped = true;
                m.into_iter()
                    .map(|v| law_byte(read_lut(lut, 255.0 * v) / 255.0))
                    .collect()
            }
            None => m.into_iter().map(law_byte).collect(),
        };
    } else {
        let (dilate, sigma) = MATTE.split(fx.size, fx.choke);
        job.check()?;
        dilate_plane_with_job(&mut plane, shape.w, shape.h, dilate, job)?;
        job.check()?;
        MATTE.blur_with_job(&mut plane, shape.w, shape.h, sigma, job)?;
    }
    let (dx, dy) = light_offset(fx.resolved_angle(ctx.light), fx.distance);
    // `shift_plane` allocates a second plane; at distance 0 (or an angle
    // whose rounded offset is zero) the shifted plane would be a copy.
    if (dx, dy) != (0, 0) {
        job.check()?;
        // Fill 255 (or `C(255)` once mapped): beyond the plane is outside
        // the shape, hence shadow (module doc, step 2).
        plane = shift_plane(&plane, shape.w, shape.h, dx, dy, fill);
    }
    let stream = EffectKind::InnerShadow as u64;
    let noise = |plane: &mut Vec<u8>| {
        apply_noise_with_job(plane, shape.w, shape.origin, fx.noise, stream, job)
    };
    if !NOISE_AFTER_CONTOUR {
        noise(&mut plane)?;
    }
    if let Some(lut) = lut.filter(|_| !mapped) {
        job.check()?;
        map_plane_with_job(&mut plane, shape.w, &lut, None, job)?;
    }
    if NOISE_AFTER_CONTOUR {
        noise(&mut plane)?;
    }
    job.check()?;
    multiply_planes(&mut plane, &shape.coverage);
    job.check()?;
    // An empty plane would cost the compositor a full pass over the shape
    // for nothing; the scan is one byte per pixel against a blur that was
    // dozens of taps per pixel.
    if plane.iter().all(|&v| v == 0) {
        return Ok(Vec::new());
    }
    Ok(vec![Contribution::solid(
        EffectKind::InnerShadow,
        plane,
        slot_color(fx.color, fx.linear),
        fx.blend,
        fx.opacity,
        Placement::Interior,
    )])
}
