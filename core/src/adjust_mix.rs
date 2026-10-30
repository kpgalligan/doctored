//! The four DECOMPOSITION and MATRIX adjustments — `black_and_white`,
//! `photo_filter`, `channel_mixer` and `selective_color`. Payload structs,
//! their `parse` and their kernels live here; `adjust` keeps only the schema
//! rows and the dispatch.
//!
//! # The decomposition two of them share
//!
//! Black & White and Selective Color both ask "which hue is this pixel?",
//! and both answer with [`adjust_math::decompose`] — ONE implementation, per
//! `core/CLAUDE.md`: a neutral part plus at most two adjacent hue
//! components, a primary (R/G/B) and a secondary (C/M/Y), on the six-slot
//! wheel `adjust_math::HUE_WHEEL` names.
//!
//! They read it differently, and the difference is the point:
//!
//! * **Black & White multiplies the AMOUNTS.** `gray = neutral +
//!   primary_amount * W[primary] + secondary_amount * W[secondary]`. A grey
//!   pixel has both amounts 0, so `gray = neutral` and no weight can tint a
//!   neutral, whatever the sliders say — the invariant to check first.
//! * **Selective Color needs a MEMBERSHIP**, so it divides the amounts by
//!   the chroma (`max - min`). A pale pink is then fully in Reds exactly as
//!   a saturated red is: `rgb(255, 128, 128)` with Reds magenta +10
//!   relative gives green 115.3, a number the un-normalized amount misses
//!   by six levels.
//!   A pixel with no chroma has no hue and no chromatic membership at all;
//!   the whites / neutrals / blacks ranges are what act on it.
//!
//! Black & White's default mix (reds 40 %, yellows 60 %, greens 40 %,
//! cyans 60 %, blues 20 %, magentas 80 %) is a very different picture from
//! the `grayscale` op's Rec. 709 luma — pure red reads 102 here and 54
//! there.
//!
//! But `grayscale` IS this op's degenerate case, and saying otherwise (an
//! earlier draft of this paragraph did) leads a reader away from the one
//! fact worth knowing. Set the six weights to the LUMA OF EACH WHEEL COLOUR
//! — reds 0.2126, yellows 0.9278, greens 0.7152, cyans 0.7874, blues
//! 0.0722, magentas 0.2848 — and the two agree everywhere. The algebra is
//! exact and short: [`adjust_math::decompose`] partitions the pixel as
//! `neutral*(1,1,1) + primary_amount*P + secondary_amount*S`, Rec. 709 luma
//! is linear with `luma((1,1,1)) = 1`, and each secondary's luma is the sum
//! of its two primaries' — so weighting each wheel slot by the luma of its
//! own colour reproduces luma at every pixel.
//! `adjust_mix_tests::black_and_white_at_luma_weights_is_the_grayscale_op`
//! is that identity as a test.
//!
//! `grayscale` nevertheless stays its own op, for two reasons that have
//! nothing to do with the math: its weights are frozen (it is one of the
//! legacy nine, whose numbers existing oracles pin), and it does its
//! arithmetic in 0..255 rather than 0..1, so on a value falling exactly
//! between two codes it can round one step away from this op.
//!
//! # Photo Filter
//!
//! A density-weighted multiply in XYZ — `x ⊙ (1 - d + d·ratio)`, with
//! `ratio` the filter colour's XYZ over the white
//! ([`PhotoFilter::multiply_xyz`]) — which is what makes a filter
//! SUBTRACTIVE (a warming filter really does remove blue). The basis is the
//! caller's and differs by depth, as Photoshop 26.10.0's does (measured):
//! at 8/16 bits the document profile's D50 PCS
//! (`adjust_photo_filter`), encoded and then — Preserve Luminosity —
//! Photoshop's Luminosity blend against the input on the ENCODED values
//! ([`PhotoFilter::preserve_encoded`]); at 32 bits the profile's native basis
//! with Y held ([`PhotoFilter::filter_native_xyz`], `adjust_float`).
//!
//! # Channel Mixer
//!
//! One 3x4 matrix over the ENCODED values (Photoshop's Channel Mixer is a
//! gamma-space matrix; do not linearize it): `out_i = sum_j M[i][j] * c_j +
//! K[i]`. The stored numbers are FRACTIONS of the dialog's percentages,
//! constant included, and each is TRUNCATED to 1/1024 at parse, as
//! Photoshop stores them (`q(c) = trunc(1024·c)/1024`; the constant is
//! `trunc(1024·K)/1024` at 8/16 bits, so a constant of 0.5 adds 127.5 in
//! 8-bit, and `trunc(512·K)/1024` in a 32-bit float document — measured,
//! take2; the why is on [`ChannelMixer`]). Monochrome is resolved at
//! parse time by copying the gray row into all three outputs, which is
//! precisely what the checkbox means.
//!
//! # Selective Color
//!
//! CMYK ink nudges per hue range, in the naive separation
//! [`adjust_math::rgb_to_cmyk`] defines. The two methods (on 50 % magenta
//! at +10 %, relative gives 55 % and absolute 60 %):
//!
//! ```text
//! total_ink = sum_ranges weight * slider          # weights below
//! relative:  ink' = ink * (1 + total_ink)         # a share of what is there
//! absolute:  ink' = ink + total_ink               # an amount of ink
//! ```
//!
//! The chromatic weights are the membership above; the achromatic three are
//! `whites = clamp((min - 0.5)*2, 0, 1)`, `blacks = clamp((0.5 - max)*2, 0,
//! 1)` and `neutrals = 1 - whites - blacks` (whites and blacks cannot both
//! be non-zero, so neutrals never goes negative). The two families are
//! independent — a pale red pixel is in Reds AND in Neutrals — which is how
//! the dialog behaves.

use serde_json::{Map, Value};

use crate::adjust_bw8::{gray8, tint8};
use crate::adjust_math::{cmyk_to_rgb, decompose, luma, rgb_to_cmyk, HUE_WHEEL};
use crate::adjust_parse::{boolean, color, nested, num_in, string_enum};
use crate::blend::{lum, set_lum, set_lum_within};

// ----------------------------------------------------- black and white --

/// The default mix, in [`HUE_WHEEL`] order.
const BW_DEFAULTS: [f64; 6] = [0.4, 0.6, 0.4, 0.6, 0.2, 0.8];

/// The sRGB spelling of the default tint (hue 42 deg, saturation
/// 20 %). A sheet writes the DOCUMENT's own spelling of it; this default
/// only has to be right on an sRGB document (see `adjust`'s colour note).
const BW_TINT: [u8; 3] = [0x99, 0x8a, 0x66];

/// The 8/16-bit Tint's low floor: `1/32768`, one of Photoshop's 15-bit
/// codes, added to every component when ClipColor's LOW branch fired and
/// the grey is above 0. Measured: take6 (`float-adjust-473/take6`,
/// `scripts/photoshop-probe/float_adjust_take6.py`, statement BT16-floor)
/// pinned `e15` exactly on all 126 low-clipped 16-bit patches — the minimum
/// reads code 1, not 0, where the grey is above 0 and code 0 where it is 0.
/// A HIGH-only clip adds nothing: no floor (`hi-e0`) is the best-scoring
/// member there, its residual at most 2 15-bit codes (about 6e-5), shipped
/// under the near-match rule.
const TINT_FLOOR_LOW: f32 = 1.0 / 32768.0;

/// The 32-bit Tint's floor: `1/510` (half an 8-bit code) on every
/// component when either ClipColor branch fired. Measured: take6's
/// BT32-low pinned `clip/e510/num` at 0.003 tolerance units (`e0` misses by
/// 39), as take3 had already read it.
const TINT_FLOOR_FLOAT: f32 = 1.0 / 510.0;

/// The 32-bit Tint's ceiling: the grey is capped at 16 before the tint is
/// set to it, and ClipColor's high branch pulls a component above 16 back
/// toward the grey. Take6's ladder read every grey at or above 16 as (16,
/// 16, 16) + 1/510 under a coloured tint and exactly 16 under a neutral one
/// (L100 reads 16.002, not 100); the cap fits every take6 32-bit sample
/// within 0.003 tolerance units. Take7, the fresh take meant to confirm it,
/// stalled in Photoshop, so the cap is take6's fit, not yet confirmed.
const TINT_CEILING_FLOAT: f32 = 16.0;

/// A parsed `black_and_white` op. The tint is kept as its colour's NUMBERS
/// over 255: Photoshop sets that colour's luminosity to the computed grey
/// (SetLum with ClipColor), so all three components matter, not only the hue
/// and saturation. The 8-bit form (`eight_bit`, true at parse;
/// derived, never persisted) also keeps the weights in whole percent and the
/// tint's codes, which its integer Tint reads (`adjust_bw8`).
#[derive(Clone)]
pub(crate) struct BlackAndWhite {
    weights: [f32; 6],
    tint: bool,
    tint_color: [f32; 3],
    percents: [i32; 6],
    tint_codes: [u8; 3],
    eight_bit: bool,
}

impl BlackAndWhite {
    /// Parses the `black_and_white` params per the schema row in `adjust`.
    pub(crate) fn parse(params: &Map<String, Value>) -> Option<BlackAndWhite> {
        let mut weights = [0.0f32; 6];
        for ((slot, name), default) in weights.iter_mut().zip(HUE_WHEEL).zip(BW_DEFAULTS) {
            *slot = num_in(params, name, -2.0..=3.0, default)?;
        }
        let tint_color = color(params, "tint_color", BW_TINT)?;
        Some(BlackAndWhite {
            weights,
            tint: boolean(params, "tint", false)?,
            tint_color: tint_color.map(|c| f32::from(c) / 255.0),
            percents: weights.map(|w| (f64::from(w) * 100.0).round() as i32),
            tint_codes: tint_color,
            eight_bit: true,
        })
    }

    /// The 16-bit form: the Tint through SetLum ([`BlackAndWhite::apply`]).
    /// `PrecisionAdjustment::parse` calls it; the byte parse keeps the 8-bit
    /// form.
    pub(crate) fn use_precision(&mut self) {
        self.eight_bit = false;
    }

    /// One straight RGB triple in [0, 1], at 8 and 16 bits: the grey,
    /// clamped to [0, 1], and with Tint on the tint's numbers set to that
    /// luminosity with ClipColor at 1, plus [`TINT_FLOOR_LOW`] when the low
    /// branch fired over a grey above 0. Take6 pinned this form at 16 bits
    /// (`setlum-clip16`, 0.14 tolerance units; the HSL form it replaces
    /// missed by 135). With Tint on, the 8-bit form runs Photoshop's integer
    /// path instead (#2 phase 4, h8a-LE: `adjust_bw8`'s [`tint8`] of the
    /// tint's codes at [`gray8`] of the pixel's codes), which this SetLum form
    /// missed by up to 10 codes.
    pub(crate) fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        if self.tint && self.eight_bit {
            let codes = rgb.map(|v| (v * 255.0).round() as u8);
            return tint8(self.tint_codes, gray8(codes, self.percents))
                .map(|c| f32::from(c) / 255.0);
        }
        let gray = self.gray_unclamped(rgb).clamp(0.0, 1.0);
        if !self.tint {
            return [gray; 3];
        }
        let (c, clipped) = set_lum_within(self.tint_color, gray, 1.0);
        if clipped.low && gray > 0.0 {
            c.map(|v| v + TINT_FLOOR_LOW)
        } else {
            c
        }
    }

    /// One stored linear triple of a 32-bit float document (`adjust_float`):
    /// `max(0, gray)` with no upper clamp; with Tint on the grey is capped at
    /// [`TINT_CEILING_FLOAT`], the tint's numbers set to it with ClipColor's
    /// ceiling at the same 16, plus [`TINT_FLOOR_FLOAT`] when either branch
    /// fired.
    pub(crate) fn apply_float(&self, rgb: [f32; 3]) -> [f32; 3] {
        let gray = self.gray_unclamped(rgb).max(0.0);
        if !self.tint {
            return [gray; 3];
        }
        let (c, clipped) = set_lum_within(
            self.tint_color,
            gray.min(TINT_CEILING_FLOAT),
            TINT_CEILING_FLOAT,
        );
        if clipped.low || clipped.high {
            c.map(|v| v + TINT_FLOOR_FLOAT)
        } else {
            c
        }
    }

    /// The weighted grey [`BlackAndWhite::apply`] clamps, before any clamp:
    /// `neutral + primary_amount * W[primary] + secondary_amount *
    /// W[secondary]` over [`decompose`]. The 32-bit float form
    /// ([`BlackAndWhite::apply_float`]) takes `max(0, gray)` of it with no
    /// upper clamp.
    pub(crate) fn gray_unclamped(&self, rgb: [f32; 3]) -> f32 {
        let parts = decompose(rgb);
        parts.neutral
            + parts.primary_amount * self.weights[parts.primary]
            + parts.secondary_amount * self.weights[parts.secondary]
    }
}

// --------------------------------------------------------- photo filter --

/// The sRGB spelling of Warming Filter (85), the default. Same
/// caveat as the Black & White tint: a sheet writes the document's numbers.
const WARMING_85: [u8; 3] = [0xec, 0x8a, 0x00];

/// A parsed `photo_filter` op, its colour already normalized.
#[derive(Clone)]
pub(crate) struct PhotoFilter {
    color: [f32; 3],
    density: f32,
    preserve_luminosity: bool,
}

impl PhotoFilter {
    /// Parses the `photo_filter` params per the schema row in `adjust`.
    pub(crate) fn parse(params: &Map<String, Value>) -> Option<PhotoFilter> {
        Some(PhotoFilter {
            color: color(params, "color", WARMING_85)?.map(|c| f32::from(c) / 255.0),
            density: num_in(params, "density", 0.0..=1.0, 0.25)?,
            preserve_luminosity: boolean(params, "preserve_luminosity", true)?,
        })
    }

    /// The filter colour, each component `code / 255` of the stored hex —
    /// the document's ENCODED numbers (`adjust`'s colour note).
    pub(crate) fn color(&self) -> [f32; 3] {
        self.color
    }

    /// THE density-weighted multiply, `x ⊙ (1 - d + d·ratio)`, on an XYZ
    /// triple with the filter colour's white-normalized XYZ `ratio`. The one
    /// statement of it: the 8/16-bit form (`adjust_photo_filter`, D50 PCS)
    /// and the 32-bit form ([`PhotoFilter::filter_native_xyz`], native
    /// basis) both call it; the caller owns the basis.
    pub(crate) fn multiply_xyz(&self, xyz: [f64; 3], ratio: [f64; 3]) -> [f64; 3] {
        let d = f64::from(self.density);
        std::array::from_fn(|c| xyz[c] * (1.0 - d + d * ratio[c]))
    }

    /// Preserve Luminosity at 8/16 bits: Photoshop's Luminosity blend of the
    /// ENCODED `filtered` result over the encoded `original`, i.e.
    /// `SetLum(filtered, Lum(original))` with ClipColor (`blend::set_lum`);
    /// `filtered` unchanged when the option is off. Why the PDF/W3C Lum
    /// weights (0.3, 0.59, 0.11) and not Rec. 709: they are what Photoshop
    /// 26.10.0's 8/16-bit renders fit (`adjust_photo_filter`'s doc has
    /// the receipts); a Rec. 709 luma rescale is the earlier, unmeasured
    /// reconstruction this replaced.
    pub(crate) fn preserve_encoded(&self, original: [f32; 3], filtered: [f32; 3]) -> [f32; 3] {
        if self.preserve_luminosity {
            set_lum(filtered, lum(original))
        } else {
            filtered
        }
    }

    /// The 32-bit float form: [`PhotoFilter::multiply_xyz`] on XYZ in the
    /// profile's NATIVE (unadapted) basis with the filter's white-normalized
    /// XYZ `ratio`, then — Preserve Luminosity — the result scaled so its Y
    /// is the input's (no clamp; a zero Y is left alone). Photoshop 26.10.0
    /// at 32 bits, measured (take2: within 4.2e-4 relative); the caller
    /// (`adjust_float`) owns the basis. Do not unify it with the 8/16-bit
    /// form: Photoshop itself renders the two depths differently.
    pub(crate) fn filter_native_xyz(&self, xyz: [f64; 3], ratio: [f64; 3]) -> [f64; 3] {
        let mut out = self.multiply_xyz(xyz, ratio);
        if self.preserve_luminosity && out[1] != 0.0 {
            let k = xyz[1] / out[1];
            out = out.map(|v| v * k);
        }
        out
    }
}

// -------------------------------------------------------- channel mixer --

/// The four keys of one mixer row, and the identity/gray defaults.
pub(crate) const MIXER_KEYS: [&str; 4] = ["r", "g", "b", "constant"];
pub(crate) const MIXER_ROWS: [(&str, [f64; 4]); 3] = [
    ("red", [1.0, 0.0, 0.0, 0.0]),
    ("green", [0.0, 1.0, 0.0, 0.0]),
    ("blue", [0.0, 0.0, 1.0, 0.0]),
];
pub(crate) const MIXER_GRAY: [f64; 4] = [0.4, 0.4, 0.2, 0.0];

/// A parsed `channel_mixer` op: the 3x3 matrix in Photoshop's 1/1024 steps,
/// with Monochrome already resolved into it, and the constant column at its
/// two depths.
///
/// Why the steps: Photoshop stores the mixer as integer 1/1024 steps and
/// its 32-bit constant spans half the 16-bit one — take2, where +10 %
/// adds 51/1024 at 32 bits and 102/1024 at 16, and mono 40/40/20 gives
/// 409/409/204. Truncated coefficients with `trunc(512·K)/1024` fit the
/// 32-bit renders within 4.8e-8 (exact fractions miss by 1.95e-3), and with
/// `trunc(1024·K)/1024` the 16-bit twins within 3.05e-5, two codes (exact
/// fractions miss by 1.95e-3). The byte path runs this same kernel, so the
/// truncation holds at every depth; it moves an in-range pixel by at most
/// `Σ|v|/1024`, under one 8-bit code.
#[derive(Clone)]
pub(crate) struct ChannelMixer {
    rows: [[f32; 3]; 3],
    constant16: [f32; 3],
    constant32: [f32; 3],
}

/// `trunc(1024·value)/1024`, rounding toward zero: one mixer coefficient as
/// Photoshop stores it (see [`ChannelMixer`]).
pub(crate) fn truncate_1024(value: f64) -> f32 {
    ((1024.0 * value).trunc() / 1024.0) as f32
}

/// One mixer row's four numbers as the JSON f64s, before any f32 narrowing,
/// so a coefficient truncates from the number the description holds.
/// `nested` validates the row first (types, ranges, the f32 narrowing) with
/// the schema's own rules; this reads the same keys again only to keep f64.
fn mixer_row(params: &Map<String, Value>, name: &str, defaults: [f64; 4]) -> Option<[f64; 4]> {
    nested(params, name, MIXER_KEYS, -2.0..=2.0, defaults)?;
    let Some(sub) = params.get(name).and_then(Value::as_object) else {
        return Some(defaults);
    };
    Some(std::array::from_fn(|k| {
        sub.get(MIXER_KEYS[k])
            .and_then(Value::as_f64)
            .unwrap_or(defaults[k])
    }))
}

impl ChannelMixer {
    /// Parses the `channel_mixer` params per the schema row in `adjust`.
    /// The `gray` row is read whether or not Monochrome is on, so a host can
    /// keep both settings in the layer and toggle the checkbox without
    /// losing either — exactly what the dialog does. Every coefficient is
    /// truncated to 1/1024 and the constant `K` kept as `trunc(1024·K)/1024`
    /// (8/16 bits) and `trunc(512·K)/1024` (32 bits), as Photoshop keeps
    /// them (see [`ChannelMixer`]).
    pub(crate) fn parse(params: &Map<String, Value>) -> Option<ChannelMixer> {
        let mut rows = [[0.0f64; 4]; 3];
        for (row, (name, defaults)) in rows.iter_mut().zip(MIXER_ROWS) {
            *row = mixer_row(params, name, defaults)?;
        }
        let gray = mixer_row(params, "gray", MIXER_GRAY)?;
        if boolean(params, "monochrome", false)? {
            rows = [gray; 3];
        }
        Some(ChannelMixer {
            rows: rows.map(|row| [row[0], row[1], row[2]].map(truncate_1024)),
            constant16: rows.map(|row| truncate_1024(row[3])),
            constant32: rows.map(|row| truncate_1024(row[3] / 2.0)),
        })
    }

    /// One straight RGB triple in [0, 1], on the ENCODED values, with the
    /// 8/16-bit constant.
    pub(crate) fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        self.mix(rgb, self.constant16)
    }

    /// The 32-bit float form (`adjust_float`): [`ChannelMixer::apply`]'s
    /// matrix on the stored linear values with the 32-bit constant and no
    /// clamp.
    pub(crate) fn apply_float(&self, rgb: [f32; 3]) -> [f32; 3] {
        self.mix(rgb, self.constant32)
    }

    fn mix(&self, rgb: [f32; 3], constant: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|i| {
            let row = self.rows[i];
            row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2] + constant[i]
        })
    }
}

// ------------------------------------------------------ selective color --

/// The three achromatic ranges. They are read AFTER [`HUE_WHEEL`]'s six, so
/// a chromatic range's index IS the wheel slot [`decompose`] reports — an
/// ordering the parse loop chains rather than restates, since a second copy
/// of those six names could drift out of step with the decomposition.
const ACHROMATIC_NAMES: [&str; 3] = ["whites", "neutrals", "blacks"];
const WHITES: usize = 6;
const NEUTRALS: usize = 7;
const BLACKS: usize = 8;
const INK_KEYS: [&str; 4] = ["c", "m", "y", "k"];

/// A parsed `selective_color` op: nine `{c, m, y, k}` nudges and the method.
#[derive(Clone)]
pub(crate) struct SelectiveColor {
    ranges: [[f32; 4]; 9],
    absolute: bool,
}

impl SelectiveColor {
    /// Parses the `selective_color` params per the schema row in `adjust`.
    pub(crate) fn parse(params: &Map<String, Value>) -> Option<SelectiveColor> {
        let mut ranges = [[0.0f32; 4]; 9];
        for (slot, name) in ranges
            .iter_mut()
            .zip(HUE_WHEEL.iter().chain(&ACHROMATIC_NAMES))
        {
            *slot = nested(params, name, INK_KEYS, -1.0..=1.0, [0.0; 4])?;
        }
        let method = string_enum(params, "method", &["relative", "absolute"], Some(0))?;
        Some(SelectiveColor {
            ranges,
            absolute: method == 1,
        })
    }

    /// One straight RGB triple in [0, 1]. All-zero sliders are an exact
    /// identity: every ink delta is 0 and the CMYK round trip is exact.
    pub(crate) fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        cmyk_to_rgb(self.nudged(rgb_to_cmyk(rgb), Self::weights(rgb)))
    }

    /// The nine range memberships of the straight RGB triple `rgb` (the six
    /// hue ranges by [`decompose`], then whites, neutrals and blacks).
    /// A CMYK document reads them off its `L = 1 − ink` triple
    /// (`cmyk_adjust_ink`).
    pub(crate) fn weights(rgb: [f32; 3]) -> [f32; 9] {
        let mut weights = [0.0f32; 9];
        let parts = decompose(rgb);
        let mx = rgb[0].max(rgb[1]).max(rgb[2]);
        let mn = rgb[0].min(rgb[1]).min(rgb[2]);
        let chroma = mx - mn;
        if chroma > 0.0 {
            weights[parts.primary] = parts.primary_amount / chroma;
            weights[parts.secondary] = parts.secondary_amount / chroma;
        }
        weights[WHITES] = ((mn - 0.5) * 2.0).clamp(0.0, 1.0);
        weights[BLACKS] = ((0.5 - mx) * 2.0).clamp(0.0, 1.0);
        weights[NEUTRALS] = (1.0 - weights[WHITES] - weights[BLACKS]).clamp(0.0, 1.0);
        weights
    }

    /// `ink` (C, M, Y, K in [0, 1]) moved by every range's nudge at its
    /// membership `weights`: relative scales each ink by `1 + Σ w·nudge`,
    /// absolute adds it, then clamps. RGB runs it on the naive separation
    /// ([`Self::apply`]); a CMYK document on its stored ink.
    pub(crate) fn nudged(&self, ink: [f32; 4], weights: [f32; 9]) -> [f32; 4] {
        let mut delta = [0.0f32; 4];
        for (range, weight) in self.ranges.iter().zip(weights) {
            if weight == 0.0 {
                continue;
            }
            for (slot, nudge) in delta.iter_mut().zip(range) {
                *slot += weight * nudge;
            }
        }
        let mut ink = ink;
        for (value, d) in ink.iter_mut().zip(delta) {
            *value = if self.absolute {
                *value + d
            } else {
                *value * (1.0 + d)
            }
            .clamp(0.0, 1.0);
        }
        ink
    }
}

impl PhotoFilter {
    pub(crate) fn is_identity(&self) -> bool {
        self.density == 0.0
    }
}
impl ChannelMixer {
    pub(crate) fn is_identity(&self) -> bool {
        self.rows == [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
            && self.constant16 == [0.0; 3]
            && self.constant32 == [0.0; 3]
    }
}
impl SelectiveColor {
    pub(crate) fn is_identity(&self) -> bool {
        self.ranges == [[0.0; 4]; 9]
    }
}

// ------------------------------------------- brightness/contrast/saturation --

/// The `bcs` op's statements (`adjust::Adjustment::apply_at`'s `Bcs` arm,
/// mirroring `ops::adjust`): brightness, then contrast about 0.5, then
/// luma-anchored saturation. No clamp of its own — `apply_at` clamps the
/// input and the output around it, and the 32-bit float form
/// (`adjust_float`) calls it bare.
pub(crate) fn brightness_contrast_saturation(
    rgb: [f32; 3],
    brightness: f32,
    contrast_slope: f32,
    saturation_scale: f32,
) -> [f32; 3] {
    let mut ch = rgb;
    for v in &mut ch {
        *v += brightness;
        *v = (*v - 0.5) * contrast_slope + 0.5;
    }
    saturation_about_luma(ch, saturation_scale)
}

/// The modern (non-legacy) `bcs` statements: a brightness POWER, then
/// a contrast S-curve about 0.5, then the same luma-anchored saturation scale
/// the legacy form ends with ([`saturation_about_luma`]).
///
/// Why this shape: legacy brightness is an offset, so it clips a highlight
/// (or a shadow) as soon as it moves, and legacy contrast is a straight line
/// about 0.5 that clips both ends. The modern form keeps 0 and 1 FIXED at
/// every setting, so neither slider clips anything by itself, and stays
/// monotonic, so it never inverts a tone:
///
/// * brightness `b` is `x^gamma` with `gamma = 2^(-1.5·b)` — `b = 1` lifts
///   the midtone 0.5 to 0.5^(1/2.83) ≈ 0.78, `b = -1` drops it to ≈ 0.14;
/// * contrast `c` is `x < 0.5 ? 0.5·(2x)^k : 1 − 0.5·(2(1−x))^k` with
///   `k = contrast_power = 2^(1.5·c)` — symmetric about (0.5, 0.5) with slope
///   `k` there, so `c = 1` steepens the midtones 2.83× and `c = -1` flattens
///   them to 0.35× without either end leaving its endpoint.
///
/// Each stage is SKIPPED when its slider is 0 (`gamma`, `k` or the
/// saturation scale exactly 1), so the identity is bit-exact rather than a
/// pow or `a + (v − a)` round trip. A channel value
/// outside [0, 1] — only a 32-bit float document hands one in; `apply_at`
/// clamps first — passes both power stages UNCHANGED: 0 and 1 are fixed
/// points of each stage, so the map stays continuous and monotonic across
/// them, and extended range survives the edit. The saturation stage is
/// unclamped, as legacy's is.
pub(crate) fn brightness_contrast_modern(
    rgb: [f32; 3],
    gamma: f32,
    contrast_power: f32,
    saturation_scale: f32,
) -> [f32; 3] {
    let mut ch = rgb;
    for v in &mut ch {
        if !(0.0..=1.0).contains(v) {
            continue;
        }
        if gamma != 1.0 {
            *v = v.powf(gamma);
        }
        if contrast_power != 1.0 {
            *v = if *v < 0.5 {
                0.5 * (2.0 * *v).powf(contrast_power)
            } else {
                1.0 - 0.5 * (2.0 * (1.0 - *v)).powf(contrast_power)
            };
        }
    }
    if saturation_scale == 1.0 {
        return ch;
    }
    saturation_about_luma(ch, saturation_scale)
}

/// The luma-anchored saturation scale both `bcs` forms end with: each
/// channel's distance from the Rec. 709 luma times `scale`, unclamped.
fn saturation_about_luma(rgb: [f32; 3], scale: f32) -> [f32; 3] {
    let anchor = luma(rgb);
    rgb.map(|v| anchor + (v - anchor) * scale)
}
