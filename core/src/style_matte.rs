//! The soft matte of a blurred layer effect (#2), in two forms.
//!
//! - THE LAW ([`law_matte_with_job`]): the matte measured in Photoshop for the
//!   drop shadow, the inner shadow, the outer glow and the inner glow
//!   wherever Size·(1 − Spread) > 0 ([`law_applies`]; for the inner effects
//!   the Choke is the spread), and the satin at spread 0 wherever Size > 0
//!   (#2 part 3, `style_fx_satin`). Its ONE home is here.
//! - A [`Matte`]: a Gaussian sized per effect and the map its blurred plane
//!   goes through. The satin's `MATTE` runs only at Size 0 (sigma 0); the outer glow's
//!   [`OUTER_GLOW`] and the shadows' `MATTE` run only where the law does not
//!   (Size·(1 − Spread) == 0, so sigma is 0 and only the dilation and the
//!   outer glow's stretch remain). [`Matte::blur_with_job`] is the ONE f32
//!   blur-and-map every member that is not the plain byte blur runs.
//!
//! # The law
//!
//! `S = max(1, round(size))`, `n = round(S · p)` (half up), `w = S − n`, a
//! width of 1 acting as 2 ([`law_split`]). The band is the coverage `u`
//! grown by `n` px: measured from the anti-aliased contour
//! (`style_aa_band`, [`LawBand::AntiAliased`]) where `u` has a pixel of
//! partial coverage, and from the contour binarized at 1/2
//! (`dilate_plane_with_job`, the same bytes on a binary plane) otherwise or
//! for [`LawBand::Binarized`]. The matte is two passes of a `w`-wide box over
//! it (`sampling_box::box_pair`, the separable triangle `(w − |j|) / w²`), or
//! the band itself at `w = 0`, as unit values; each caller applies its own
//! Range (the glows' `min(1, B / r)`) and rounds once to a byte with
//! [`law_byte`].
//!
//! Why: the inner glow's law (`inner_glow_578_law_fit.py`, shipped first)
//! is the box pair of a band, with no free number, and its sigma `w / √6`
//! sits near the 0.4 the shadows' Gaussian was fitted to. Declared as each
//! kernel's candidate in `scripts/photoshop-probe/effect_kernels_578_take.py`
//! and judged offline on effects-578 take1, soft-effects-578 take1, identify
//! takes 3 and 4 and style-parity-v1x (`--fit`, its RECORDED section), then
//! confirmed on one fresh take of anti-aliased pictures at 8 and 16 bits
//! (`effect-kernels-578/take1`, `--judge`): the anti-aliased band (D1, I1,
//! O1) was confirmed for all three; the binarized band ships for none.
//!
//! Memory: the band (the `aa` band's `12·P` peak, freed before the blur) and
//! the box pair's f64 planes fit inside the `plane_blur::float_plan` every
//! styled scratch plan charges at its largest sigma
//! (`precision_style_render::style_scratch`), so the law needs no plan of
//! its own and its effects report sigma 0.
//!
//! # Why the members differ from `sigma_for_size`
//!
//! `sigma_for_size(size) = size / 2` is the effects' original sizing and
//! stays the one for bevel (except a Smooth bevel, whose
//! measured law is a box pair, `style_bevel_smooth`) and the guides. Photoshop's soft effects are
//! narrower than that, measured on fresh takes and fitted offline against
//! declared families (sigma `size · (1 − spread) · r` with r in {1/2, 1/2.5,
//! 1/3}, a range map and a gamma):
//!
//! - the outer glow, `Mdilate · 1/2.5 · Rscale` (`effects_578_take.py`):
//!   ratio 0.4 and the stretch `min(1, a / 0.5)` ([`OUTER_GLOW`]);
//! - the drop shadow, inner shadow and satin, `G1/2.5 · Rnone · γ 1`
//!   (`soft_effects_578_fit.py`): ratio 0.4 and no map, so their blur is
//!   the byte blur at the narrower sigma.
//!
//! # Numerics
//!
//! - [`Matte::split`] is `spread_split`'s dilation and its sigma times `2 ·
//!   ratio`: at ratio 1/2 ([`TODAY`]) it IS `spread_split`, and `2 · 0.4` is
//!   the outer glow's former `2 / 2.5` in f32 exactly, so the glow renders
//!   byte for byte as it did.
//! - [`Matte::blur_with_job`] at `MatteRange::None` and γ 1 is
//!   `blur_plane_with_job`, byte for byte. Any other member blurs the plane
//!   as unit f32 (`plane_blur::floats`, the byte blur's own schedule without
//!   its byte rounding; its fields live in 0..1), maps each value and rounds
//!   ONCE to a byte, so a stretched ramp keeps every code (stretching the
//!   blurred byte would leave only even codes). With no blur the map runs on
//!   the bytes. The f32 plane lies inside every styled scratch plan, which
//!   charges `plane_blur::float_plan` (the caller's full plane included) at
//!   the largest effect sigma (`precision_style_render::style_scratch`), so a
//!   member needs no plan of its own as long as `blur_sigma` reports its
//!   sigma.
//! - `MatteRange::Scale` is `min(1, a / 0.5)` (the fit's `Rscale`; its
//!   `Rshift` shipped for no effect, so it has no variant); γ is `a^γ` after
//!   the range.

use crate::job_control::JobContext;
use crate::sampling_box::box_pair;
use crate::style_aa_band::{aa_band_with_job, has_partial};
use crate::style_plane_ops::{blur_plane_with_job, dilate_plane_with_job, spread_split};

/// Whether the law runs (module doc): Size·(1 − Choke) > 0, read
/// as `spread_split`'s sigma, so a non-finite or negative size or choke is
/// sanitized exactly as today's kernel sanitizes it.
pub(crate) fn law_applies(size: f32, choke: f32) -> bool {
    spread_split(size, choke).1 > 0.0
}

/// The law's `(n, w)` (module doc; the runner's `law_split`): `S = max(1,
/// floor(size + 0.5))`, `n = min(S, floor(S·p + 0.5))` with `p` the choke as
/// `spread_split` clamps it, `w = S − n`, and `w = 2` where `S − n = 1` (a
/// one-pixel box pair would be no blur at all; the take shows a width of 1
/// acting as 2). `S·p` is snapped to a millionth before the rounding (the
/// `light_offset` precedent): an f32 choke such as 0.7 is 0.699999988, and
/// without the snap `10 × 0.7` would round to 6 where the runner's decimal
/// 0.7 rounds to 7.
pub(crate) fn law_split(size: f32, choke: f32) -> (u32, u32) {
    let size = if size.is_finite() { size.max(0.0) } else { 0.0 };
    let p = if choke.is_finite() {
        choke.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let s = (f64::from(size) + 0.5).floor().max(1.0);
    let snap = |v: f64| (v * 1e6).round() / 1e6;
    let n = (snap(s * f64::from(p)) + 0.5).floor().min(s);
    let w = s - n;
    let w = if w == 1.0 { 2.0 } else { w };
    (n as u32, w as u32)
}

/// How far the law reads or writes from any pixel (`style_effects::reach`):
/// the band's `n` px plus one for its anti-aliased edge, plus the box pair's
/// half-width `w − 1`.
pub(crate) fn law_reach(size: f32, choke: f32) -> u32 {
    let (n, w) = law_split(size, choke);
    n.saturating_add(1).saturating_add(w.saturating_sub(1))
}

/// Which contour the law's band is measured from (module doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LawBand {
    /// The anti-aliased contour where the coverage has a partial pixel.
    AntiAliased,
    /// The contour binarized at 1/2, always.
    Binarized,
}

/// The law's matte (module doc): the band of `n` px grown into `u` (a `w ×
/// h` byte coverage), then `sampling_box::box_pair` at width `w` (the band
/// itself at `w == 0`), unit values. `AntiAliased`: `aa_band_with_job(..)
/// .joined()` when `has_partial(u)`, else `dilate_plane_with_job`;
/// `Binarized`: always the dilation. Polls `job` before each primitive and
/// inside them; the box pair reports its rows on `job`'s range at stage 6,
/// as the Gaussian it replaced did.
pub(crate) fn law_matte_with_job(
    u: &[u8],
    w: u32,
    h: u32,
    size: f32,
    spread: f32,
    band: LawBand,
    job: JobContext<'_>,
) -> Result<Vec<f64>, String> {
    let (n, width) = law_split(size, spread);
    job.check()?;
    let grown = if band == LawBand::AntiAliased && has_partial(u) {
        aa_band_with_job(u, w, h, n as f32, job)?.joined()
    } else {
        let mut grown = u.to_vec();
        dilate_plane_with_job(&mut grown, w, h, n as f32, job)?;
        grown
    };
    job.check()?;
    if width > 0 {
        box_pair(
            |i| f64::from(grown[i]) / 255.0,
            w as usize,
            h as usize,
            width,
            job,
        )
    } else {
        Ok(grown.iter().map(|&v| f64::from(v) / 255.0).collect())
    }
}

/// The law's one rounding of a unit value to a byte, `floor(255·v + 1/2)`.
pub(crate) fn law_byte(v: f64) -> u8 {
    (255.0 * v + 0.5).floor().clamp(0.0, 255.0) as u8
}

/// The map a member applies to its blurred plane (module doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MatteRange {
    None,
    Scale,
}

/// One effect's soft-matte member (module doc).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Matte {
    /// The Gaussian's sigma as a fraction of `size · (1 − spread)`.
    pub(crate) ratio: f32,
    pub(crate) range: MatteRange,
    pub(crate) gamma: f32,
}

/// `sigma_for_size`'s member: sigma `size · (1 − spread) / 2`, no map.
pub(crate) const TODAY: Matte = Matte {
    ratio: 0.5,
    range: MatteRange::None,
    gamma: 1.0,
};

/// The outer glow's member, `Mdilate · 1/2.5 · Rscale` (module doc).
pub(crate) const OUTER_GLOW: Matte = Matte {
    ratio: 0.4,
    range: MatteRange::Scale,
    gamma: 1.0,
};

impl Matte {
    /// `(dilate_px, sigma)`: `spread_split`'s dilation and its sigma times
    /// `ratio / TODAY.ratio`, i.e. `2 · ratio` exactly (module doc). The ONE
    /// place an effect's blur is sized: its render, `Effect::reach` and the
    /// typed scratch plan's `blur_sigma` read it.
    pub(crate) fn split(&self, size: f32, spread: f32) -> (f32, f32) {
        let (dilate, sigma) = spread_split(size, spread);
        (dilate, sigma * (self.ratio / TODAY.ratio))
    }

    /// The blur at `sigma` and this member's map (module doc): the byte
    /// blur when there is no map, else the f32 blur, the map and one
    /// rounding. Polls `job` inside the blur and per 4096 pixels of each
    /// pass.
    pub(crate) fn blur_with_job(
        &self,
        plane: &mut [u8],
        w: u32,
        h: u32,
        sigma: f32,
        job: JobContext<'_>,
    ) -> Result<(), String> {
        if self.range == MatteRange::None && self.gamma == 1.0 {
            return blur_plane_with_job(plane, w, h, sigma, job);
        }
        if sigma.is_nan() || sigma <= 0.0 {
            for chunk in plane.chunks_mut(4096) {
                job.check()?;
                for c in chunk {
                    *c = self.code(f32::from(*c) / 255.0);
                }
            }
            return Ok(());
        }
        // One exact, fallible allocation: the plan charges one f32 plane.
        let mut field: Vec<f32> = Vec::new();
        field
            .try_reserve_exact(plane.len())
            .map_err(|_| "could not allocate effect blur field".to_string())?;
        for chunk in plane.chunks(4096) {
            job.check()?;
            field.extend(chunk.iter().map(|&c| f32::from(c) / 255.0));
        }
        crate::plane_blur::floats(&mut field, w, h, sigma, job)?;
        for (out, values) in plane.chunks_mut(4096).zip(field.chunks(4096)) {
            job.check()?;
            for (c, &v) in out.iter_mut().zip(values) {
                *c = self.code(v);
            }
        }
        Ok(())
    }

    /// The range, then γ, on one unit value.
    fn map(&self, v: f32) -> f32 {
        let v = match self.range {
            MatteRange::None => v,
            MatteRange::Scale => (v / 0.5).min(1.0),
        };
        if self.gamma == 1.0 {
            v
        } else {
            v.max(0.0).powf(self.gamma)
        }
    }

    /// `round(255 · map(v))` as a byte: the one rounding.
    fn code(&self, v: f32) -> u8 {
        (255.0 * self.map(v)).round().clamp(0.0, 255.0) as u8
    }
}
