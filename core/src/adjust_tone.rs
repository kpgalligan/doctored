//! The two TONE adjustments: `exposure` (a light-linear gain, pedestal and
//! gamma) and `shadows_highlights` (the one SPATIAL op, whose lift is
//! weighted by a large-radius alpha-weighted blur of the luma). Payload
//! structs, their `parse`, their kernels, and [`tone_plane`] — the guide
//! plane — live here; `adjust` keeps only the schema rows and the dispatch.
//!
//! # Exposure
//!
//! Exposure performs its calculations in a linear colour space (gamma 1.0)
//! rather than the image's current colour space. So:
//!
//! ```text
//! u   = srgb_to_linear(v)
//! u   = u * 2^exposure          # a pure linear gain, in stops
//! u   = u + offset              # a linear pedestal
//! u   = max(u, 0) ^ (1/gamma)   # still in linear light
//! out = linear_to_srgb(u)
//! ```
//!
//! That listing is the legacy (`legacy_srgb`) kernel, [`Exposure::apply`].
//! A `document_v2` description evaluates [`Exposure::apply_linear`] per
//! channel in the document's own primaries instead, each channel linearized
//! at the profile's effective pure gamma (2.2 for sRGB-shaped and sampled
//! curves, a pure power's own exponent) and re-encoded by its inverse — no
//! TRC, no matrix (`adjust_calibrated`, #578, fitted to Photoshop at 8 and 16
//! bits) — and a 32-bit float document applies it per stored linear channel
//! (`adjust_float`).
//!
//! Order matters and is not commutative: gain, THEN offset, THEN gamma —
//! offset after gain is what makes Offset "darken the shadows and midtones
//! with minimal effect on the highlights", and the reverse order would scale
//! the pedestal. The `max(u, 0)` is because a negative offset can push a
//! dark pixel below zero, and a negative base under a fractional exponent is
//! a NaN.
//!
//! **The gamma is applied as the reciprocal power, so `gamma > 1`
//! brightens** — the same direction as the `levels` op's midtone gamma
//! (`adjust_levels` applies `t^(1/gamma)` too). That is Photoshop's Gamma
//! Correction, measured at 32 and 16 bits (the take2 receipt:
//! `1/gamma` fits within 9.5e-8 at 32 bits where the plain power misses by
//! 3.07). The stored `gamma` is Photoshop's parameter verbatim, so a saved
//! description with gamma ≠ 1 renders as Photoshop renders it.
//!
//! # Shadows / Highlights
//!
//! The weight comes from a large-radius blur of the LUMINANCE, not from the
//! pixel's own value — that is the whole mechanism, and what preserves local
//! contrast: a dark pixel inside a bright region is not lifted, because its
//! neighbourhood says "highlight".
//!
//! ```text
//! L   = luma(rgb);  B = the guide (the blurred luma at this pixel)
//! w_s = clamp(1 - B/t_s, 0, 1)^2 ;  w_h = clamp(1 - (1 - B)/t_h, 0, 1)^2
//! e   = (1 + a_h*w_h) / (1 + a_s*w_s)
//! L'  = L^e
//! L'  = L' + m*(3L'^2 - 2L'^3 - L')        # midtone_contrast
//! k   = L > 0 ? L'/L : 1
//! rgb'= rgb * k                            # NOT clamped yet
//! w   = clamp(a_s*w_s + a_h*w_h, 0, 1)     # "how much this pixel moved"
//! rgb'= L' + (rgb' - L') * (1 + color*w)
//! rgb'= clamp(rgb', 0, 1)                  # ONE clamp, at the end
//! ```
//!
//! The SQUARED weight is what gives the smooth shoulder; a linear ramp
//! visibly bands at the tone boundary.
//!
//! Two details are load-bearing and easy to "tidy" into bugs:
//!
//! * **The single trailing clamp.** Clamping `rgb * k` before the Color step
//!   would anchor the saturation to a grey that is not the pixel's own luma —
//!   precisely where the op works hardest (a deep shadow lifted several
//!   stops), so a channel that overshot would come back desaturated toward
//!   the wrong centre.
//! * **The `w` gate on Color.** `color` defaults to 0.2, so without a gate
//!   an all-zero-amount Shadows/Highlights would still resaturate the whole
//!   image. `w` is zero exactly where the tone did not move, which makes
//!   `amount 0` an exact identity.
//!
//! The midtone-contrast term is monotone for every `m` in [-1, 1]: its
//! derivative is `(1 - m) + 6m*x*(1 - x)`, whose minimum over x in [0, 1] is
//! `1 - m` for positive m and `1 + m/2` for negative m — both non-negative
//! on that interval, so the op can never invert an ordering.

use serde_json::{Map, Value};

pub(crate) use crate::adjust_guide::{tone_plane, tone_plane_with_job, GuidePlane};
use crate::adjust_math::{linear_to_srgb, luma, srgb_to_linear};
use crate::adjust_parse::{num_in, obj};

/// A parsed `exposure` op: the gain (already `2^stops`), the linear
/// pedestal, and the linear-light exponent `1/gamma` (module doc).
#[derive(Clone)]
pub(crate) struct Exposure {
    gain: f32,
    offset: f32,
    inv_gamma: f32,
}

impl Exposure {
    /// The same light-linear gain used by spatially varying calibrated exposure.
    pub(crate) fn gain_only(stops: f32) -> Self {
        Self {
            gain: stops.exp2(),
            offset: 0.0,
            inv_gamma: 1.0,
        }
    }
    /// Parses the `exposure` params per the schema row in `adjust`.
    pub(crate) fn parse(params: &Map<String, Value>) -> Option<Exposure> {
        let stops = num_in(params, "exposure", -20.0..=20.0, 0.0)?;
        let offset = num_in(params, "offset", -0.5..=0.5, 0.0)?;
        let gamma = num_in(params, "gamma", 0.01..=9.99, 1.0)?;
        Some(Exposure {
            offset,
            inv_gamma: 1.0 / gamma,
            ..Self::gain_only(stops)
        })
    }

    /// The version-2 kernel in its fixed reference linear RGB basis. The
    /// legacy encoded kernel below stays untouched for persisted descriptions.
    pub(crate) fn apply_linear(&self, rgb: [f64; 3]) -> [f64; 3] {
        rgb.map(|v| {
            let u = v * f64::from(self.gain) + f64::from(self.offset);
            // The reference basis can contain negative components for real
            // wide-gamut colours. Signed power extends gamma continuously;
            // clipping happens only after returning to the document gamut.
            u.signum() * u.abs().powf(f64::from(self.inv_gamma))
        })
    }

    pub(crate) fn is_identity(&self) -> bool {
        self.gain == 1.0 && self.offset == 0.0 && self.inv_gamma == 1.0
    }

    /// One straight RGB triple in [0, 1]. The defaults (0 stops, 0 offset,
    /// gamma 1) are a byte-for-byte no-op: the gain is exactly 1, the
    /// pedestal exactly 0, `powf(1.0)` is the identity, and the sRGB
    /// transfer pair round-trips every 8-bit code exactly.
    pub(crate) fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        rgb.map(|v| {
            let u = (srgb_to_linear(v) * self.gain + self.offset).max(0.0);
            linear_to_srgb(u.powf(self.inv_gamma))
        })
    }
}

/// A parsed `shadows_highlights` op. ONE radius serves both bands (two
/// radii would mean two blurred guide planes per composite for the same
/// picture), and the amounts/tones are stored as fractions.
#[derive(Clone)]
pub(crate) struct ShadowsHighlights {
    shadow_amount: f32,
    shadow_tone: f32,
    highlight_amount: f32,
    highlight_tone: f32,
    radius: f32,
    color: f32,
    midtone_contrast: f32,
}

/// Reads one `{amount, tone}` band object, which may be absent entirely.
fn band(
    params: &Map<String, Value>,
    key: &str,
    amount_default: f64,
    tone_default: f64,
) -> Option<(f32, f32)> {
    match obj(params, key)? {
        None => Some((amount_default as f32, tone_default as f32)),
        Some(sub) => Some((
            num_in(sub, "amount", 0.0..=1.0, amount_default)?,
            num_in(sub, "tone", 0.01..=1.0, tone_default)?,
        )),
    }
}

impl ShadowsHighlights {
    /// Parses the `shadows_highlights` params per the schema row in
    /// `adjust`. Defaults: shadows 35 % at tone 50 %, highlights off,
    /// radius 30 px, Color +20 %.
    pub(crate) fn parse(params: &Map<String, Value>) -> Option<ShadowsHighlights> {
        let (shadow_amount, shadow_tone) = band(params, "shadows", 0.35, 0.5)?;
        let (highlight_amount, highlight_tone) = band(params, "highlights", 0.0, 0.5)?;
        Some(ShadowsHighlights {
            shadow_amount,
            shadow_tone,
            highlight_amount,
            highlight_tone,
            radius: num_in(params, "radius", 0.0..=1000.0, 30.0)?,
            color: num_in(params, "color", -1.0..=1.0, 0.2)?,
            midtone_contrast: num_in(params, "midtone_contrast", -1.0..=1.0, 0.0)?,
        })
    }

    /// Develop Filter's Highlights and Shadows (`sampling_develop_filter`):
    /// both bands at tone 0.5, no Color and no midtone contrast, and amounts
    /// that may be NEGATIVE, which `parse` never admits. An amount in [-0.5,
    /// 1] keeps `1 + a·w` at or above 0.5 for every weight in [0, 1], so the
    /// exponent stays in [0.5, 2] and never divides by zero; with Color 0 the
    /// `moved` weight (clamped to 0 for a negative amount) is never read.
    pub(crate) fn signed(
        shadow_amount: f32,
        highlight_amount: f32,
        radius: f32,
    ) -> ShadowsHighlights {
        ShadowsHighlights {
            shadow_amount,
            shadow_tone: 0.5,
            highlight_amount,
            highlight_tone: 0.5,
            radius,
            color: 0.0,
            midtone_contrast: 0.0,
        }
    }

    /// Develop Filter's Shadows and Highlights SLIDERS (−100…100 each) through
    /// [`Self::signed`]: Shadows +100 lifts the shadows by amount 1, Highlights
    /// +100 brightens the highlights (a NEGATIVE highlight amount, so the
    /// exponent falls below 1), and each negative side stops at half, which
    /// keeps every amount in `signed`'s [-0.5, 1]. The one copy of the mapping:
    /// `sampling_develop_filter` and HDR Toning's Local Adaptation
    /// (`hdr_local_adaptation`) both read it.
    pub(crate) fn sliders(shadows: f64, highlights: f64, radius: f32) -> ShadowsHighlights {
        let s = shadows / 100.0;
        let h = -highlights / 100.0;
        let shadow = if s < 0.0 { s / 2.0 } else { s };
        let highlight = if h < 0.0 { h / 2.0 } else { h };
        Self::signed(shadow as f32, highlight as f32, radius)
    }

    /// Whether this op's output depends on the guide at all.
    ///
    /// `b` reaches [`ShadowsHighlights::apply`] only through `w_s` and
    /// `w_h`, and both are multiplied by their band's amount everywhere they
    /// are used — in the exponent and in `moved`. With both amounts zero the
    /// guide is therefore UNREAD: the exponent is 1, `moved` is 0, and the
    /// op is either the identity or the midtone-contrast curve alone. So the
    /// plane is not built, which matters because building it is the most
    /// expensive thing any adjustment does and the compositor rebuilds it on
    /// every render, export, thumbnail and live stroke tick.
    pub(crate) fn reads_guide(&self) -> bool {
        self.shadow_amount != 0.0 || self.highlight_amount != 0.0
    }

    pub(crate) fn radius(&self) -> f32 {
        self.radius
    }
    pub(crate) fn is_identity(&self) -> bool {
        !self.reads_guide() && self.midtone_contrast == 0.0
    }
    /// This op's guide plane at its own radius — [`tone_plane`], which is
    /// the only reason `Adjustment::guide` exists. `None` when the guide
    /// cannot change a single output pixel ([`Self::reads_guide`]).
    pub(crate) fn guide_plane(
        &self,
        w: u32,
        h: u32,
        rgba_at: &dyn Fn(usize) -> [f32; 4],
    ) -> Option<GuidePlane> {
        if !self.reads_guide() {
            return None;
        }
        Some(tone_plane(w, h, self.radius, rgba_at))
    }

    /// One straight RGB triple with `guide` = the blurred luma of its
    /// neighbourhood (the module doc's `B`). See the module doc for why the
    /// clamp is at the end and why `w` gates the Color step.
    pub(crate) fn apply(&self, rgb: [f32; 3], guide: f32) -> [f32; 3] {
        self.unclamped(rgb, guide).map(|c| c.clamp(0.0, 1.0))
    }

    /// **H, Highlights and Shadows at 32 bits**, the Develop
    /// Filter's stage 4 on a float layer's display-encoded copy: the guide is
    /// read as at 16 bits (clamped to 0…1); with `l = luma(rgb)`, a pixel with
    /// `l ≥ 1` is returned unchanged (`L^e` fixes white, so above white is
    /// held there), and any other takes [`Self::apply`]'s expression without
    /// its final clamp.
    pub(crate) fn apply_float(&self, rgb: [f32; 3], guide: f32) -> [f32; 3] {
        if luma(rgb) >= 1.0 {
            return rgb;
        }
        self.unclamped(rgb, guide)
    }

    /// [`Self::apply`] before its one trailing clamp.
    fn unclamped(&self, rgb: [f32; 3], guide: f32) -> [f32; 3] {
        let l = luma(rgb);
        let b = guide.clamp(0.0, 1.0);
        let w_s = (1.0 - b / self.shadow_tone).clamp(0.0, 1.0);
        let w_s = w_s * w_s;
        let w_h = (1.0 - (1.0 - b) / self.highlight_tone).clamp(0.0, 1.0);
        let w_h = w_h * w_h;
        let exponent = (1.0 + self.highlight_amount * w_h) / (1.0 + self.shadow_amount * w_s);
        // `powf(1.0)` is exactly `l`, so the shortcut is bit-identical — and
        // it is taken for every pixel of an amount-free pass and for every
        // midtone pixel of a normal one, where `powf` is otherwise the most
        // expensive instruction in the whole adjustment.
        let mut lifted = if l <= 0.0 {
            0.0
        } else if exponent == 1.0 {
            l
        } else {
            l.powf(exponent)
        };
        let m = self.midtone_contrast;
        lifted += m * (3.0 * lifted * lifted - 2.0 * lifted * lifted * lifted - lifted);
        let k = if l > 0.0 { lifted / l } else { 1.0 };
        let moved = (self.shadow_amount * w_s + self.highlight_amount * w_h).clamp(0.0, 1.0);
        let boost = 1.0 + self.color * moved;
        rgb.map(|c| lifted + (c * k - lifted) * boost)
    }
}
