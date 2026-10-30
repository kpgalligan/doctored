//! The `vibrance` colour adjustment's kernel: its payload struct, `parse` and
//! the two stages. It names no profile: `adjust_vibrance::BoundVibrance`
//! binds the document profile's TRC at 8 and 16 bits, and
//! [`Vibrance::apply`] is the unbound form with the sRGB curve (the 32-bit
//! Scaled master, Develop Filter stage 7, RAW-local Saturation). Hue/Saturation
//! moved to `adjust_hue_saturation` and Color Balance to
//! `adjust_color_balance`, both measured in Photoshop.
//!
//! # The form, measured in Photoshop 26.10.0 (#578)
//!
//! Receipts: `doctored-fixtures/photoshop/adjust-parity-535-536/take3`
//! (VB-sat), `adjust-parity-v1x/take1`-`take4` and `adjust-arith-578/take1`;
//! the rules, the candidate families, the fits and the judged residuals are
//! the docstrings of `scripts/photoshop-probe/adjust_v1x_take.py` (the
//! Saturation stage), `adjust_v1x_round2.py` (round 2's encoded Vibrance
//! stage, which this replaces) and `adjust_arith_578_take.py` (the Vibrance
//! stage below, member V3t-P). Each stage is a pull about one anchor on the
//! document's LINEAR values `c` (the profile's TRC decode),
//!
//! ```text
//! c' = clamp01(A + (c − A)·K)
//! ```
//!
//! * **Vibrance** (v = vibrance/100), first. Two facts of the takes fix its
//!   shape: (a) Photoshop holds the pixel's hexcone hue in LINEAR light, not
//!   in encoded values — given Photoshop's own maximum and minimum, the middle
//!   component follows within 0.05 codes in linear light where encoded values
//!   miss it by up to 27; (b) the light is the DOCUMENT's: one colour moves
//!   differently in an sRGB and a ROMM document (up to 19 codes apart), each a
//!   pull in its own profile's linear light. So one anchor and one gain serve
//!   all three components: with `M`, `m` the linear max and min and `S = (M −
//!   m)/M`, `A = M·(A/M)` and `K`, both read from `vibrance_law`'s tables
//!   over (`S`, `√M`) at the slider (above 0 scaled by a skin term on the
//!   linear hue). Below 0 the gain falls toward `1 + v/4` at full chroma and
//!   much further on pale colours; above 0 the gain is largest on pale, bright
//!   colours and the anchor sits near the maximum. Residual (take1, fresh
//!   colours, the bound being 2 / 0.5): v-only 11 / 0.64 codes at 8 bits and
//!   11.3 / 0.54 at 16, from round 2's 21 / 4.98 and 19.4 / 5.13; combined
//!   with Saturation 4 / 0.44, from 53. It ships as measurably closer, not as
//!   Photoshop's rule: the worst rows are warm colours in the skin window at a
//!   high positive slider (an orange (255, 103, 58) at +90 misses by 11), where
//!   one skin term scales tables fitted mostly off that window.
//! * **Saturation** (s = saturation/100), second: `A = Wromm·L`, the ROMM RGB
//!   (ProPhoto) Y row [`S_ANCHOR`] whatever the document's profile; `K = 1 +
//!   s` at both signs; per-component clamp. take1 8 bits max 1 / mean 0.015,
//!   16 bits 0.18 / 0.011 codes; take3 VB-sat srgb16 0.31, wide16 1.25.
//!
//! A slider at 0 skips its stage, so the defaults return the input exactly.
//! [`Vibrance::vibrance_linear`] and [`Vibrance::saturation_linear`] take the
//! caller's decode; [`Vibrance::vibrance_srgb`] is the Vibrance stage through
//! the sRGB curve (the CMYK and float Sponge) and [`Vibrance::apply`] both
//! stages through it (the unbound form).

use serde_json::{Map, Value};

use crate::adjust_math::{linear_to_srgb, rgb_to_hsl, srgb_to_linear};
use crate::adjust_parse::num_in;
use crate::vibrance_law::gain_and_anchor;

/// The Saturation slider's anchor weights: ROMM RGB's Y row (Photoshop's,
/// fitted on take3 VB-sat with the weights free: (0.287997, 0.712007,
/// -0.000004)), applied to the document's own linear values.
const S_ANCHOR: [f64; 3] = [0.288040, 0.711874, 0.000086];

/// A parsed `vibrance` op: the chroma-weighted gain and the flat
/// saturation slider that rides with it in the same dialog, as fractions
/// (Photoshop's units / 100).
#[derive(Clone)]
pub(crate) struct Vibrance {
    vibrance: f32,
    saturation: f32,
}

impl Vibrance {
    /// Parses the `vibrance` params per the schema row in `adjust`.
    pub(crate) fn parse(params: &Map<String, Value>) -> Option<Vibrance> {
        Some(Vibrance {
            vibrance: num_in(params, "vibrance", -1.0..=1.0, 0.0)?,
            saturation: num_in(params, "saturation", -1.0..=1.0, 0.0)?,
        })
    }

    /// The Vibrance slider alone (`vibrance` a fraction in −1…1, saturation
    /// 0): the Sponge's Vibrance target (`retouch_brush`).
    pub(crate) fn slider(vibrance: f32) -> Vibrance {
        Vibrance {
            vibrance,
            saturation: 0.0,
        }
    }

    pub(crate) fn is_identity(&self) -> bool {
        self.vibrance == 0.0 && self.saturation == 0.0
    }

    /// Whether the Saturation stage moves anything (a slider at 0 skips its
    /// stage, so a caller skips the decode and encode around it too).
    pub(crate) fn moves_saturation(&self) -> bool {
        self.saturation != 0.0
    }

    /// The unbound form: one ENCODED triple in [0, 1] decoded through the sRGB
    /// curve, both stages, encoded back. Identity returns `rgb` bit for bit.
    pub(crate) fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        if self.is_identity() {
            return rgb;
        }
        let linear = self.vibrance_linear(rgb.map(srgb_to_linear));
        self.saturation_linear(linear).map(linear_to_srgb)
    }

    /// The Vibrance stage alone through the sRGB curve, on one ENCODED triple:
    /// the Sponge's target on a CMYK record's `L` pass and through a float
    /// document's display encoding. At 0 it returns `encoded` bit for bit.
    pub(crate) fn vibrance_srgb(&self, encoded: [f32; 3]) -> [f32; 3] {
        if self.vibrance == 0.0 {
            return encoded;
        }
        self.vibrance_linear(encoded.map(srgb_to_linear))
            .map(linear_to_srgb)
    }

    /// The Vibrance stage (module doc) on the caller's LINEAR decode of a
    /// pixel; returns linear light. At 0 it returns `linear` bit for bit.
    pub(crate) fn vibrance_linear(&self, linear: [f32; 3]) -> [f32; 3] {
        if self.vibrance == 0.0 {
            return linear;
        }
        let l = linear.map(|c| f64::from(c).clamp(0.0, 1.0));
        let hi = l[0].max(l[1]).max(l[2]);
        let lo = l[0].min(l[1]).min(l[2]);
        let chroma = if hi > 0.0 {
            ((hi - lo) / hi).clamp(0.0, 1.0)
        } else {
            0.0
        };
        // Only the skin term of a positive slider reads the hue.
        let hue =
            (self.vibrance > 0.0 && hi > lo).then(|| f64::from(rgb_to_hsl(l.map(|c| c as f32))[0]));
        let (gain, share) = gain_and_anchor(f64::from(self.vibrance), chroma, hi, hue);
        pull(l, share * hi, gain).map(|c| c as f32)
    }

    /// The Saturation stage (module doc) on the caller's LINEAR decode of a
    /// pixel; returns linear light. At 0 it returns `linear` bit for bit.
    pub(crate) fn saturation_linear(&self, linear: [f32; 3]) -> [f32; 3] {
        if !self.moves_saturation() {
            return linear;
        }
        let l = linear.map(f64::from);
        let anchor = (0..3).map(|c| S_ANCHOR[c] * l[c]).sum();
        pull(l, anchor, (1.0 + f64::from(self.saturation)).max(0.0)).map(|c| c as f32)
    }
}

/// One stage's pull: `clamp01(anchor + (c − anchor)·k)` per component.
fn pull(rgb: [f64; 3], anchor: f64, k: f64) -> [f64; 3] {
    rgb.map(|c| (anchor + (c - anchor) * k).clamp(0.0, 1.0))
}
