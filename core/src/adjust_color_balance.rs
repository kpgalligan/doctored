//! `color_balance`, measured in Photoshop 26.10.0 at 8, 16 and 32 bits.
//! A kernel module: it names no `Adjustment`, `PrecisionDocument` or
//! `IccProfile`; `adjust` parses into it and `adjust_float` adapts it.
//!
//! Receipt: `doctored-fixtures/photoshop/adjust-parity-535-536/take6`
//! (`judged.json` authoritative; the form was fitted from take2 and take5),
//! runner `scripts/photoshop-probe/color_balance_take6.py`, sha256
//! `f1efa57d1cd31b0fdbaaad527e524e0d2fe3f031f24496334762461d6b5303d4`.
//! take6 pinned every statement at 16 and 32 bits within the runner's
//! tolerance: tone `sum` 0.64 / 0.61 units, pmid `midtrunc` 0.31 / 0.21,
//! bw `levels` 0.68 / 0.74, the form 0.68 / 0.74, and the 32-bit end
//! slopes `stages` 4.3e-4.
//!
//! # The form
//!
//! The nine amounts act in whole Photoshop units, `round(100·amount)` (the
//! PSD `blnc` writer's `hundredths`), S, M, H per tone and c = R, G, B (the
//! `cyan_red`, `magenta_green`, `yellow_blue` axes). Each channel is a
//! Levels: a black point b, a white point w and a gamma.
//!
//! * Preserve Luminosity off: `b = max(0, −S_c)`, `w = 255 − max(0, H_c)`,
//!   `t = M_c + trunc((S_c + H_c)/2)`.
//! * Preserve Luminosity on: each tone's vector is renormalised rather than
//!   any luminance being held: `b = max(S) − S_c`,
//!   `w = 255 − (H_c − min(H))`, `t = M_c − trunc((max(M) + min(M))/2)`.
//!
//! `γ = r2(2^(t/100))` with `r2(v) = floor(100·v + ½)/100`, `p = 1/γ`,
//! `y = (x − b/255)·255/(w − b)`, and the curve is `y^p` on `clamp(y, 0, 1)`
//! — with a toe for γ > 1, below.
//!
//! **Why r2.** Photoshop carries the gamma as a two-decimal number, the
//! value its Levels dialog would show: take2 missed the unrounded gamma by
//! 12 tolerance units and r2 by 0.38.
//!
//! **Why the toe, and why x0.** `y^p` with p < 1 has an infinite slope at
//! 0, which no table-driven curve can have. Photoshop replaces it below
//! `x0 = 2·2^(γ/(p − 1))` with the cubic Hermite from (0, 0) at slope
//! `S0 = 2^γ` to `(x0, x0^p)` at the power's own slope `p·x0^(p−1)`, so the
//! curve and its slope are continuous at x0. Both constants are the ones
//! fitted from take2 and pinned by take5 (the Hermite 0.33 units, a linear
//! cap `min(S0·y, y^p)` 136, no toe 224); x0 shrinks as γ grows (1/8 at
//! γ = 2), so the toe only ever touches the deepest shadows. Levels shares
//! this toe ([`Toe`], `adjust_levels::stage_curve`): #2 fitted Photoshop's
//! Levels toe back onto these constants and one take confirmed it.
//!
//! # 32 bits
//!
//! In range the same curve. Outside [0, 1] Photoshop continues each end
//! along a slope read off its 4096-entry stage tables (`stages`): with
//! `L = clamp(rnd(4096·y(4095/4096)), 0, 4096)` the high slope is
//! `(rnd(32768·g(1)) − rnd(32768·g(L/4096)))/8`, and the low slope the same
//! between `L1` and `L0` at x = 1/4096 and x = 0 (g the in-range curve on
//! y). At γ = 1 the slope is instead `rnd(32768·(f(1) − f(4095/4096)))/8`
//! of the composed curve, `rnd(2040/(w − b))/8` on a sloped end and 0 on a
//! flat one, computed here in integers because 2040/240 and 2040/80 are
//! exact halves. **Why a LUT slope:** the measured end slopes are whole
//! eighths (9/8, 10/8, 23/8 …), which neither the analytic derivative nor
//! the 32768-step difference of the final curve predicts on a black or
//! white point (take5: 4.4e4 and 4.6e4 units; `stages` 5.5e-4). The output
//! is `1 + (x − 1)·hi` above 1 and `x·lo` below 0.
//!
//! A channel whose black point reaches its white point (`w ≤ b`, possible
//! only with Preserve Luminosity on and opposed shadows and highlights) is
//! unmeasured; its span is taken as 1 so the curve stays a finite step.

use serde_json::{Map, Value};

use crate::adjust_parse::{boolean, nested_f64};

/// One stage table's resolution and the 15-bit scale its entries round to.
const STAGES: i64 = 4096;
const SCALE: f64 = 32768.0;

/// A parsed `color_balance` op: its whole units `[S, M, H][R, G, B]` and
/// the three channel curves derived from them and Preserve Luminosity.
#[derive(Clone)]
pub(crate) struct ColorBalance {
    units: [[i32; 3]; 3],
    curves: [ChannelCurve; 3],
}

/// One channel's Levels curve (module doc).
#[derive(Clone, Copy)]
struct ChannelCurve {
    black: f64,
    white: f64,
    gamma: f64,
    toe: Option<Toe>,
    slope_hi: f64,
    slope_lo: f64,
}

/// The cubic Hermite below x0 (module doc "Why the toe"): its end, start
/// slope, end value and end slope; also Levels' toe (#2).
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Toe {
    x0: f64,
    s0: f64,
    p1: f64,
    m1: f64,
}

impl Toe {
    /// The toe of a stage with this gamma: `None` unless `gamma > 1` (a NaN
    /// gamma gives `None`).
    pub(crate) fn new(gamma: f64) -> Option<Toe> {
        let p = 1.0 / gamma;
        (gamma > 1.0).then(|| {
            let x0 = 2.0 * 2f64.powf(gamma / (p - 1.0));
            Toe {
                x0,
                s0: 2f64.powf(gamma),
                p1: x0.powf(p),
                m1: p * x0.powf(p - 1.0),
            }
        })
    }

    /// The Hermite at `y` when `y < x0`; `None` at `y >= x0`, where
    /// `y^(1/gamma)` applies.
    pub(crate) fn value(&self, y: f64) -> Option<f64> {
        (y < self.x0).then(|| {
            let u = y / self.x0;
            (u * u * u - 2.0 * u * u + u) * self.x0 * self.s0
                + (-2.0 * u * u * u + 3.0 * u * u) * self.p1
                + (u * u * u - u * u) * self.x0 * self.m1
        })
    }
}

/// Whole Photoshop units of one amount: `psd_adjustment::hundredths`' own
/// rounding (the master; half away from zero), on the same unnarrowed f64.
fn units(amount: f64) -> i32 {
    (amount * 100.0).round() as i32
}

/// `floor(n/d + ½)` for d > 0, exactly.
pub(crate) fn round_ratio(n: i64, d: i64) -> i64 {
    (2 * n + d).div_euclid(2 * d)
}

/// `(b, w, t)` per channel under the decided members (module doc).
fn channel_params(units: &[[i32; 3]; 3], preserve: bool) -> [(i32, i32, f64); 3] {
    let [s, m, h] = *units;
    if !preserve {
        return std::array::from_fn(|c| {
            let t = m[c] + (s[c] + h[c]) / 2;
            ((-s[c]).max(0), 255 - h[c].max(0), f64::from(t))
        });
    }
    let top = s.iter().copied().max().unwrap_or(0);
    let bottom = h.iter().copied().min().unwrap_or(0);
    let mid = m.iter().copied().max().unwrap_or(0) + m.iter().copied().min().unwrap_or(0);
    std::array::from_fn(|c| (top - s[c], 255 - (h[c] - bottom), f64::from(m[c] - mid / 2)))
}

impl ChannelCurve {
    fn new(black: i32, white: i32, t: f64) -> Self {
        let gamma = (100.0 * 2f64.powf(t / 100.0) + 0.5).floor() / 100.0;
        let toe = Toe::new(gamma);
        let mut curve = ChannelCurve {
            black: f64::from(black),
            white: f64::from(white),
            gamma,
            toe,
            slope_hi: 0.0,
            slope_lo: 0.0,
        };
        (curve.slope_hi, curve.slope_lo) = curve.stage_slopes();
        curve
    }

    /// The end slopes `(hi, lo)` of the stage tables (module doc).
    fn stage_slopes(&self) -> (f64, f64) {
        let black = self.black as i64;
        let span = (self.white as i64 - black).max(1);
        if self.gamma == 1.0 {
            // f(1) = 1 and f(0) = 0 always; an end is sloped exactly when
            // its point is at the rail (w = 255 above, b = 0 below).
            let end = |sloped: bool| {
                if sloped {
                    round_ratio(2040, span) as f64 / 8.0
                } else {
                    0.0
                }
            };
            return (end(self.white == 255.0), end(black == 0));
        }
        // rnd(4096·y(k/4096)) clamped, y = (255·k/4096 − b)/(w − b).
        let stage = |k: i64| round_ratio(255 * k - STAGES * black, span).clamp(0, STAGES);
        let level = |l: i64| (SCALE * self.g(l as f64 / STAGES as f64) + 0.5).floor();
        (
            (level(STAGES) - level(stage(STAGES - 1))) / 8.0,
            (level(stage(1)) - level(stage(0))) / 8.0,
        )
    }

    /// The in-range curve on y in [0, 1].
    fn g(&self, y: f64) -> f64 {
        self.toe
            .and_then(|toe| toe.value(y))
            .unwrap_or_else(|| y.powf(1.0 / self.gamma))
    }

    /// f(x) for x in [0, 1].
    fn value(&self, x: f64) -> f64 {
        let span = (self.white - self.black).max(1.0);
        self.g(((x - self.black / 255.0) * 255.0 / span).clamp(0.0, 1.0))
    }

    /// f(x) for any x: the curve in range, the stage slopes outside.
    fn value_float(&self, x: f64) -> f64 {
        if x > 1.0 {
            1.0 + (x - 1.0) * self.slope_hi
        } else if x < 0.0 {
            // A flat end stores +0, never `x·0 = −0`.
            let v = x * self.slope_lo;
            if v == 0.0 {
                0.0
            } else {
                v
            }
        } else {
            self.value(x)
        }
    }
}

impl ColorBalance {
    /// Parses the `color_balance` params per the schema row in `adjust`.
    pub(crate) fn parse(params: &Map<String, Value>) -> Option<ColorBalance> {
        let keys = ["cyan_red", "magenta_green", "yellow_blue"];
        let tone = |key| nested_f64(params, key, keys, -1.0..=1.0, [0.0; 3]);
        let units = [tone("shadows")?, tone("midtones")?, tone("highlights")?]
            .map(|amounts| amounts.map(units));
        let preserve = boolean(params, "preserve_luminosity", true)?;
        let curves = channel_params(&units, preserve)
            .map(|(black, white, t)| ChannelCurve::new(black, white, t));
        Some(ColorBalance { units, curves })
    }

    /// One straight RGB triple at 8/16 bits: each channel clamped to [0, 1]
    /// and through its curve.
    pub(crate) fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|c| self.curves[c].value(f64::from(rgb[c]).clamp(0.0, 1.0)) as f32)
    }

    /// One stored triple at 32 bits, any sign or magnitude.
    pub(crate) fn apply_float(&self, rgb: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|c| self.curves[c].value_float(f64::from(rgb[c])) as f32)
    }

    /// All nine whole units are 0 (Preserve Luminosity alone changes
    /// nothing then).
    pub(crate) fn is_identity(&self) -> bool {
        self.units == [[0; 3]; 3]
    }
}
