//! Inner glow (Layer Style > Inner Glow): a glow INSIDE the
//! shape, composited above the pixels with its own blend mode (Screen by
//! default), colour and opacity. Where the glow comes from is the `source`:
//! `edge` (the default) hugs the inside of the contour and fades toward the
//! interior; `center` rises from the interior and fades toward the contour.
//!
//! Three regimes, split where Size·(1 − Choke) is 0
//! (`style_matte::law_applies`) and, there, by source and size
//! ([`full_choke_applies`]):
//!
//! - THE LAW (`size · (1 − choke) > 0`), the measured law `L` (#2), for
//!   both sources, through the law's one home
//!   (`style_matte::law_matte_with_job`). `S = max(1, round(size))`, `n =
//!   round(S · choke)` (half up), `w = S − n`, and a width of 1 acts as 2
//!   (`style_matte::law_split`). The band
//!   is `shape.inverted()` grown into the shape by `n` px, measured from the
//!   anti-aliased contour (`style_aa_band`) where the shape has a pixel of
//!   partial coverage, and from the contour binarized at 1/2
//!   (`dilate_plane_with_job`, today's Euclidean band, the same bytes on a
//!   binary shape) where it has none; `B` is two passes
//!   of a `w`-wide box over it (`sampling_box::box_pair`, the separable
//!   triangle `(w − |j|) / w²`), or the band itself at `w = 0`. The Range
//!   `r` (0.01..=1) is `min(1, B / r)` for an edge glow and `1 − min(1, B /
//!   r)` for a centre glow (exactly one minus the edge: the mirrored rule),
//!   rounded once to a byte. The contour then maps that byte with no Range
//!   of its own.
//! - THE CHOKE 100 % MEMBER `M` (`size · (1 − choke) == 0` and size > 0: an
//!   `edge` glow at every such size, a `center` glow below
//!   [`FULL_CHOKE_CENTRE_LIMIT`] = 24; #2's second identify take). The
//!   band is today's, `shape.inverted()` grown by `dilate = size`
//!   (`dilate_plane_with_job`); `e` is 1 on a pixel fully inside it (byte
//!   255: it glows full at every Range) and today's Divide `min(1, b / 2r)`
//!   on every other pixel, unrounded; the edge glow is `e`, the centre glow
//!   `1 − e` (the mirror), rounded once to a byte ([`full_choke_plane`]). The
//!   contour then maps that byte with no Range of its own, as under the law.
//! - TODAY'S KERNEL (`size · (1 − choke) == 0` otherwise: a `center` glow at
//!   Choke 100 % from Size 24, or Size 0), byte for byte as before #2
//!   shipped the law.
//!   `(dilate, sigma) = spread_split(size, choke)`, and sigma is 0 here, so
//!   nothing blurs: an `edge` glow is `shape.inverted()` grown by `dilate`
//!   (exactly the band the shape loses when it is shrunk by `size`, the
//!   closed form the tests pin); a `center` glow is `shape.coverage` eroded
//!   by `dilate` (with `size == 0` the plane IS the shape: the whole interior
//!   glows). The contour maps it with the glow's Range by the outer glow's
//!   Divide rule.
//!
//! Why the split and the law: `scripts/photoshop-probe/inner_glow_578_law_fit.py`
//! (`--judge-split`, its SPLIT JUDGE and RECORDED sections) scores `L'` (this
//! kernel) against Photoshop on the identify take
//! (`~/devel/doctored-fixtures/photoshop/identify-578/take1`) and the A3
//! and style-parity-v1x rows. No existing row rises; the existing rows' mean
//! of row means falls from 21.87 to 1.22 codes (edge) and from 24.30 to
//! 1.29 (centre), and all 76 identify rows the law runs on outside the named
//! bins meet 2 / 0.5 per source (worst 1.32 / 0.10). At Choke 100 % `L` has
//! no blur and only its Range law differs from today, which doubles the
//! band's anti-aliased edge at Range 50 %; three v1x Choke 100 % rows rose
//! under it, so that regime kept today's kernel for the Choke 100 % take.
//!
//! Why `M`: that take (`scripts/photoshop-probe/identify_578_take2.py`,
//! `identify-578/take2`; `inner_glow_578_law_fit.py`'s CHOKE 100
//! DESCRIPTION and `--judge-choke100`) measured that at Choke 100 % a pixel
//! reads `min(1, u / r)` with `u` fixed per pixel: the band's interior is 255
//! at every Range (today's Divide halves it to 127.5 at Range 100 %), and a
//! partial pixel at its inner edge reads about today's Divide (u / b 0.40
//! ... 0.60, so `L`'s `min(1, b / r)` doubles it). For the edge no existing
//! row rises, every row `M` changes is lower (identify take1's Choke 100 %
//! Range 100 % rows: Size 2, 4 and 16 from 127 / 2.0 ... 15.9 codes to 0 /
//! 0), the A3 and style-parity-v1x rows (Range 50 %) are held, and the
//! existing mean of row means falls from 4.04 to 0.87. take2's own rows
//! fall from 17.31 to 0.60 outside the named bins but none meets 2 / 0.5
//! (Photoshop measures the band from the anti-aliased contour, this one
//! from the binarized). The centre was first judged STOP: six identify
//! take1 Size 32 rows rose 128 -> 255 in their max, inside the anomalous
//! 35.9 px band (Photoshop 0, today 127.5, `M` 255).
//!
//! Why the centre's split at Size 24: the controller ruled `M` for the
//! centre only below Size 24, the regime the inner-glow law batch named
//! before scoring (Size 32's 35.9 px band is its anomaly), and the runner's
//! CHOKE 100 CENTRE SPLIT (`--judge-choke100-centre`) judged it before it
//! ran on every style-parity-v1x and identify take1 centre row: SHIP, no row
//! rises, 152 are held (Size 32 keeps today's kernel), all 14 rows `M`
//! changes are lower (Size 2, 4 and 16 at Range 100 % from 127 / 111 ...
//! 126 codes to 0 / 0, the Size 8 disc from 127 / 100.7 to 39.8 / 1.44), and
//! the existing mean of row means falls from 18.49 to 8.67. take2's centre
//! rows outside the named bins fall from 30.47 to 0.60 (none meets 2 / 0.5,
//! the edge's reason). `core/tests/style_inner_glow_choke100_parity.rs` pins
//! take2's rows of both sources and
//! `core/tests/style_inner_glow_identify_parity.rs` the identify residuals.
//!
//! Why the law's band is measured from the anti-aliased contour: the fourth
//! identify take (`identify_578_take4.py`, `identify-578/take4`: a half
//! plane whose edge column has coverage 0.25 / 0.5 / 0.75 and a
//! supersampled disc) shows a partial pixel entering the band at its own
//! coverage, so the band's edge moves with it (Size 12 Choke 50 %, the
//! 127.5 crossing at 7.493 / 7.273 / 7.025 px inward) where the binarized
//! band jumps a whole pixel. `glow_aa_578_law_fit.py`'s INNER JUDGE
//! (`--judge-inner`, its RECORDED) declared that band (`Ia`, no free number)
//! and judged it per regime and source: under the law both sources SHIP by
//! both rules. All 32 held-out take4 rows outside the named bins (the disc
//! at Choke 50 and 80 %) meet 2 / 0.5 per source (mean of row means 0.027
//! edge, 0.026 centre, from 0.26); no existing row rises, the 20 v1x rows it
//! changes per source are all lower (worst 20 / 0.27 codes from 90 / 1.25),
//! and the binary identify, A3 and take2 rows are held.
//! `M` with the same band (`Ma`) did not ship (edge KEEP, centre STOP: 28
//! and 20 take2 disc and `fx` rows rose), so Choke 100 % keeps today's band.
//! `core/tests/style_inner_glow_aa_parity.rs` pins the take4 rows.
//!
//! One Interior contribution (kind InnerGlow, solid colour), multiplied by
//! `shape.coverage`. Nothing at opacity 0, nothing for an `edge` glow of
//! size 0 (a glow with no extent — see [`render`]), and nothing when the
//! plane comes out empty (a fully transparent layer). The inner glow has no
//! light direction, so `ctx` is unused; the pad `LayerStyle::pad()`
//! reserves for it (`style_matte::law_reach` for the law,
//! [`full_choke_reach`] for `M`,
//! `ceil(dilate) + 1` for today's kernel) is exactly what the band and the
//! box pair can reach, so the plane renders as it would on an infinite one.
//!
//! The contour (`style_contour`) maps the plane in place, every
//! pixel, BEFORE the coverage multiply (the Interior rule): through the
//! glow's Range in today's kernel (Linear at Range 50 % skips the pass), with
//! no Range under the law and `M` (Linear skips the pass).
//!
//! The noise (`style_noise`) adds its grain in place
//! at each pixel's layer coordinate, after the contour and before the
//! coverage multiply; amount 0 skips the pass.
//!
//! A Gradient glow (`style_glow_gradient`) ramps its gradient across
//! the plane after the contour and noise and before the coverage multiply:
//! the coverage becomes the stops' opacity wherever the plane is above 0
//! (an Interior effect has no Below cap), and the colour is the gradient
//! read through the per-pixel `t` field. A Color glow runs none of it.

use crate::job_control::JobContext;
use crate::style::{EffectKind, GlowFill, GlowSource, InnerGlow};
use crate::style_contour::{effect_lut, map_plane_with_job};
use crate::style_glow_gradient::{ramp_color, ramp_field_with_job, GLOW_RAMP_SITE_AFTER_CONTOUR};
use crate::style_matte::{law_applies, law_byte, law_matte_with_job, LawBand};
use crate::style_noise::{apply_noise_with_job, NOISE_AFTER_CONTOUR};
use crate::style_render::{
    blur_plane_with_job, dilate_plane_with_job, erode_plane_with_job, multiply_planes, slot_color,
    spread_split, Contribution, Placement, RenderContext, Shape,
};

/// Renders `fx` over `shape`: one Interior contribution whose coverage is
/// bounded by `shape.coverage` (module doc), or none when there is nothing
/// to draw. An `edge` glow of size 0 is nothing rather than
/// `inverted × coverage`: that product is 0 on every hard pixel and only
/// non-zero along anti-aliased edges (`(255 - c) * c / 255`, at most 64),
/// which would draw a faint one-pixel halo where a 0 px glow should draw
/// nothing. A `center` glow of size 0 is the whole shape. Polls `job`
/// before every primitive and inside the dilate/erode and the blur
/// (13A-5b-i).
pub(crate) fn render(
    fx: &InnerGlow,
    shape: &Shape,
    ctx: &RenderContext,
    job: JobContext<'_>,
) -> Result<Vec<Contribution>, String> {
    let _ = ctx;
    if fx.skipped_by_settings() {
        return Ok(Vec::new());
    }
    if law_applies(fx.size, fx.choke) {
        let plane = law_plane(fx, shape, job)?;
        return finish(fx, shape, plane, None, job);
    }
    if full_choke_applies(fx.size, fx.choke, fx.source) {
        let plane = full_choke_plane(fx, shape, job)?;
        return finish(fx, shape, plane, None, job);
    }
    let (dilate, sigma) = spread_split(fx.size, fx.choke);
    let mut plane = match fx.source {
        GlowSource::Edge => {
            job.check()?;
            let mut plane = shape.inverted();
            job.check()?;
            dilate_plane_with_job(&mut plane, shape.w, shape.h, dilate, job)?;
            plane
        }
        GlowSource::Center => {
            job.check()?;
            let mut plane = shape.coverage.clone();
            job.check()?;
            erode_plane_with_job(&mut plane, shape.w, shape.h, dilate, job)?;
            plane
        }
    };
    job.check()?;
    blur_plane_with_job(&mut plane, shape.w, shape.h, sigma, job)?;
    finish(fx, shape, plane, Some(fx.range), job)
}

/// Every regime's tail on the glow's byte plane: the noise and the contour
/// (with `range`, today's Divide rule, or none under the law and `M`), the
/// gradient ramp, the coverage multiply and the contribution. Today's kernel
/// runs it exactly as its body did before the law shipped.
fn finish(
    fx: &InnerGlow,
    shape: &Shape,
    mut plane: Vec<u8>,
    range: Option<f32>,
    job: JobContext<'_>,
) -> Result<Vec<Contribution>, String> {
    let gradient = fx.fill_type == GlowFill::Gradient;
    let source = (gradient && !GLOW_RAMP_SITE_AFTER_CONTOUR).then(|| plane.clone());
    let stream = EffectKind::InnerGlow as u64;
    let noise = |plane: &mut Vec<u8>| {
        apply_noise_with_job(plane, shape.w, shape.origin, fx.noise, stream, job)
    };
    if !NOISE_AFTER_CONTOUR {
        noise(&mut plane)?;
    }
    if let Some(lut) = effect_lut(&fx.contour, fx.anti_aliased, range) {
        job.check()?;
        map_plane_with_job(&mut plane, shape.w, &lut, None, job)?;
    }
    if NOISE_AFTER_CONTOUR {
        noise(&mut plane)?;
    }
    let ramp = if gradient {
        job.check()?;
        Some(ramp_field_with_job(
            &mut plane,
            source.as_deref(),
            shape.w,
            shape.origin,
            &fx.gradient,
            fx.jitter,
            stream,
            None,
            job,
        )?)
    } else {
        None
    };
    job.check()?;
    multiply_planes(&mut plane, &shape.coverage);
    job.check()?;
    if plane.iter().all(|&v| v == 0) {
        return Ok(Vec::new());
    }
    let mut out = Contribution::solid(
        EffectKind::InnerGlow,
        plane,
        slot_color(fx.color, fx.linear),
        fx.blend,
        fx.opacity,
        Placement::Interior,
    );
    if let Some(t) = ramp {
        out.color = ramp_color(&fx.gradient, shape.origin, shape.w, shape.h, t);
    }
    Ok(vec![out])
}

/// Whether a regime and source take the band measured from the
/// anti-aliased contour (`style_aa_band`; module doc): exactly the pairs
/// the runner's INNER JUDGE shipped, the law (`law` true) for both sources.
/// `M` (`law` false) keeps today's binarized band at Choke 100 % (its edge
/// pair was KEEP, its centre pair STOP).
fn aa_applies(law: bool, source: GlowSource) -> bool {
    law && matches!(source, GlowSource::Edge | GlowSource::Center)
}

/// The law's byte plane before the contour (module doc): the band (from the
/// anti-aliased contour where the shape has a partial pixel), the box pair,
/// the Range, one rounding.
fn law_plane(fx: &InnerGlow, shape: &Shape, job: JobContext<'_>) -> Result<Vec<u8>, String> {
    job.check()?;
    let inverted = shape.inverted();
    let band = if aa_applies(true, fx.source) {
        LawBand::AntiAliased
    } else {
        LawBand::Binarized
    };
    let b = law_matte_with_job(&inverted, shape.w, shape.h, fx.size, fx.choke, band, job)?;
    drop(inverted);
    let r = if fx.range.is_finite() {
        f64::from(fx.range).clamp(0.01, 1.0)
    } else {
        1.0
    };
    let center = fx.source == GlowSource::Center;
    job.check()?;
    Ok(b.iter()
        .map(|&v| {
            let e = (v / r).min(1.0);
            law_byte(if center { 1.0 - e } else { e })
        })
        .collect())
}

/// The size from which a `center` glow at Choke 100 % keeps today's kernel
/// (module doc): the regime the inner-glow law batch named before scoring
/// ("Choke 100 at Size 1, 8 or 32, or Size >= 24"), the runner's
/// `CENTRE_LIMIT`.
pub(crate) const FULL_CHOKE_CENTRE_LIMIT: f32 = 24.0;

/// Whether the Choke 100 % member `M` runs (module doc): the law does not
/// ([`law_applies`] false, so Size·(1 − Choke) is 0), the size sanitized as
/// `spread_split` sanitizes it is above 0, and, for a `center` glow, below
/// [`FULL_CHOKE_CENTRE_LIMIT`] (the controller's split of the centre STOP).
pub(crate) fn full_choke_applies(size: f32, choke: f32, source: GlowSource) -> bool {
    let size = if size.is_finite() { size.max(0.0) } else { 0.0 };
    !law_applies(size, choke)
        && size > 0.0
        && (source == GlowSource::Edge || size < FULL_CHOKE_CENTRE_LIMIT)
}

/// How far `M` reads or writes from any pixel (`style_effects::reach`):
/// today's band of `size` px plus one for its anti-aliased edge, today's
/// `ceil(dilate) + 1` at Choke 100 %.
pub(crate) fn full_choke_reach(size: f32) -> u32 {
    let size = if size.is_finite() { size.max(0.0) } else { 0.0 };
    (size.ceil() as u32).saturating_add(1)
}

/// `M`'s byte plane before the contour (module doc): today's band grown by
/// `size` px, then `e` = 1 where the band is full and today's Divide `min(1,
/// b / 2r)` elsewhere, unrounded; `g = e` for an `edge` glow and `1 − e` for
/// a `center` glow (the mirror), rounded once. A non-finite Range reads as
/// none, today's identity, which the Divide gives at 1/2. Both are
/// duplicates applied per pixel here rather than through a LUT: the master
/// copies are `style_contour`'s `range_input` (`RangeRule::Divide`) and
/// `sanitized_range`.
fn full_choke_plane(fx: &InnerGlow, shape: &Shape, job: JobContext<'_>) -> Result<Vec<u8>, String> {
    let (dilate, _) = spread_split(fx.size, fx.choke);
    job.check()?;
    let mut band = shape.inverted();
    job.check()?;
    dilate_plane_with_job(&mut band, shape.w, shape.h, dilate, job)?;
    let r = if fx.range.is_finite() {
        f64::from(fx.range).clamp(0.01, 1.0)
    } else {
        0.5
    };
    let center = fx.source == GlowSource::Center;
    job.check()?;
    Ok(band
        .iter()
        .map(|&v| {
            let e = if v == 255 {
                1.0
            } else {
                (f64::from(v) / 255.0 / (2.0 * r)).min(1.0)
            };
            let g = if center { 1.0 - e } else { e };
            (255.0 * g + 0.5).floor().clamp(0.0, 255.0) as u8
        })
        .collect())
}
