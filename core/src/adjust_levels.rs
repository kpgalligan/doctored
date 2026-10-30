//! `levels`: the payload of a Levels adjustment — a master (composite) stage,
//! an optional stage per colour channel and, on each, an output range — with
//! its `parse`, its kernel and its identity test. `adjust` keeps only the
//! schema row and the dispatch (the `adjust_curves` pattern), and the
//! destructive twin is `dr_image_adjust_op` like every other op.
//!
//! One stage maps a component `v` in [0, 1]:
//!
//! ```text
//!     t   = clamp((v - black) / (white - black), 0, 1) ^ (1 / gamma)
//!     out = output_black + (output_white - output_black) * t
//! ```
//!
//! `0 <= black < white <= 1`, gamma in [0.1, 10] (so gamma > 1 BRIGHTENS),
//! and `output_black` / `output_white` each in [0, 1] in EITHER order — a
//! black above the white inverts. The output line is skipped when it is
//! (0, 1), where it is the identity.
//!
//! **Order: the channel's stage FIRST, then the master's** —
//! `out_c = master(channel_c(v_c))`. That is the order Photoshop applies a
//! `levl` record set in, MEASURED against Photoshop 2025's own renders
//! (`tests/fixtures/psd_photoshop/adjustments_channels/`: within
//! 2.53 codes at 8 bit, the reverse order misses by 43.5), and the order
//! `adjust_curves` already uses for its per-channel tables.
//!
//! **The toe (#2, H-CB).** Photoshop does not run the power law all the
//! way into the shadows: below `x0 = 2·2^(gamma/(p − 1))` (p = 1/gamma) a
//! stage with gamma > 1 follows Color Balance's cubic Hermite, from (0, 0) at
//! slope `2^gamma` to the power at x0 ([`stage_curve`], through
//! `adjust_color_balance::Toe`, the one implementation; no toe at
//! gamma ≤ 1). x0 is at most 1/8 (at gamma = 2). Both free Hermite families
//! of phase 2's offline fit (`scripts/photoshop-probe/
//! levels_toe_578_take.py`, 39 rows) collapsed onto these constants: 0.57 /
//! 0.022 codes at 16 bits where the capped toe it replaces (LcapG,
//! `min(t^p, σ·t)`) gave 8.3 / 0.36, and 5 / 0.39 at 8 bits where LcapG gave
//! 12 / 0.44. One take at eight unused gamma sets confirmed it
//! (`levels_hcb_578_take.py`, `levels-hcb-578/take1`): 0.98 / 0.040 at 16
//! bits (LcapG 11.1 / 2.19), 5 / 0.24 at 8 (LcapG 8 / 0.53).
//!
//! **8 bits round twice (#2 phase 4, R8(h,h,x)).** On a byte document each
//! stage works on CODES ([`LevelsStage::apply8`]): the input position
//! `s = (c − in_lo)·255/(in_hi − in_lo)` is rounded to a code, the curve's
//! value at that code is rounded to a code, and only then is the output line
//! applied; the channel's output goes on to the master unrounded. The member
//! is the best of twelve rounding variants fitted offline on 25 8-bit rows
//! (`scripts/photoshop-probe/eight_bit_578_take.py`): 2 / 0.009 codes where
//! H-CB on its own gave 5 / 0.25, and one take on seven unused sets confirmed
//! it (`eight-bit-578/take1`: 1 / 0.016, H-CB 6 / 0.50). The byte parse is
//! the 8-bit form; `PrecisionAdjustment::parse` clears it
//! ([`Levels::use_precision`]), so 16 bits keep the continuous stage.
//!
//! A 32-bit stage keeps the pure signed power ([`LevelsStage::apply_float`]):
//! on float-adjust-473 take2's T1 the signed power holds within 1.53e-6
//! relative where the toe would apply, and the Hermite misses by 8.28e-3. A
//! CMYK stage (`cmyk_adjust`, not measured) keeps the pure power too.
//!
//! An identity stage is SKIPPED rather than evaluated, so a master-only
//! levels meta — every meta written before the channel stages and output
//! keys existed — computes exactly the arithmetic it always did, bit for
//! bit (`ops_filters::levels_channels`' per-channel statement with the
//! triple repeated).
//!
//! a master stage plus optional `r`/`g`/`b` stages, each with an output range, the channel's stage
//! applied BEFORE the master's (the order A2 measured against Photoshop); identity stages are
//! skipped, so a master-only meta computes exactly what it always did, and `dr_image_adjust_op` is
//! the destructive twin (the legacy single-purpose Levels export is gone;
//! `ops_filters::levels_channels` stays for the Auto commands). `psd_adjustment`'s `levl` codec
//! carries records 0..=3 both ways. Tests: `tests/adjust_levels.rs`, `tests/levels_native.rs`,
//! `tests/psd_levels.rs`, and `tests/psd_photoshop_channels.rs`'s render-parity gate against the
//! committed Photoshop renders (3 codes).

use serde_json::{Map, Value};

use crate::adjust::num;
use crate::adjust_color_balance::Toe;

/// How close (in codes) a value must be to an integer for
/// [`LevelsStage::apply8`] to read it as that code. A byte `v = code/255`
/// in f32 lands within about 2e-5 code of it and a parsed point `k/255`
/// within about 3e-5, so 1e-3 catches every one; a real-valued channel
/// output that close to a code is read as it, a shift of at most 1e-3 code
/// before the stage's own roundings (the runner snaps the same way).
const CODE_SNAP: f64 = 1e-3;

fn snap_code(c: f64) -> f64 {
    let r = c.round();
    if (c - r).abs() <= CODE_SNAP {
        r
    } else {
        c
    }
}

/// The toe of a stage with this gamma (module doc): Color Balance's Hermite,
/// `None` (no toe) at gamma ≤ 1 or NaN.
pub(crate) fn levels_toe(gamma: f32) -> Option<Toe> {
    Toe::new(f64::from(gamma))
}

/// One stage's curve on its input position `t`: `t` clamped to [0, 1], then
/// the toe's Hermite below its x0 (in f64, narrowed to f32) and
/// `t.powf(inv_gamma)` elsewhere; with no toe exactly
/// `t.clamp(0, 1).powf(inv_gamma)`, the curve before the toe. THE one
/// statement of the Levels curve at 8 and 16 bits (the adjustment, the Auto
/// commands' `ops_filters::levels_channels` and the typed `LevelsChannels`).
pub(crate) fn stage_curve(t: f32, inv_gamma: f32, toe: Option<&Toe>) -> f32 {
    let t = t.clamp(0.0, 1.0);
    match toe.and_then(|toe| toe.value(f64::from(t))) {
        Some(v) => v as f32,
        None => t.powf(inv_gamma),
    }
}

/// One parsed stage: the input black point and range, the reciprocal gamma,
/// the output black point and (signed) range, and the toe (`None`: none).
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct LevelsStage {
    black: f32,
    range: f32,
    inv_gamma: f32,
    out_black: f32,
    out_range: f32,
    pub(crate) toe: Option<Toe>,
}

impl LevelsStage {
    const IDENTITY: LevelsStage = LevelsStage {
        black: 0.0,
        range: 1.0,
        inv_gamma: 1.0,
        out_black: 0.0,
        out_range: 1.0,
        toe: None,
    };

    /// The five keys of one stage, each defaulting to the identity. `None`
    /// on a wrong JSON type or any value outside the module doc's ranges.
    pub(crate) fn parse(params: &Map<String, Value>) -> Option<LevelsStage> {
        let black = num(params, "black", 0.0)? as f32;
        let white = num(params, "white", 1.0)? as f32;
        let gamma = num(params, "gamma", 1.0)? as f32;
        let out_black = num(params, "output_black", 0.0)? as f32;
        let out_white = num(params, "output_white", 1.0)? as f32;
        // The input condition is [`LevelsStage::input`]'s, the one
        // `ops_filters::levels_channels` keeps per channel; the output pair
        // only has to lie in [0, 1].
        if !((0.0..=1.0).contains(&out_black) && (0.0..=1.0).contains(&out_white)) {
            return None;
        }
        Some(LevelsStage {
            out_black,
            out_range: out_white - out_black,
            ..LevelsStage::input(black, white, gamma)?
        })
    }

    /// The input half of a stage, its output line the identity: `None`
    /// unless `0 <= black < white <= 1` and gamma is in [0.1, 10] (NaN fails
    /// every comparison).
    pub(crate) fn input(black: f32, white: f32, gamma: f32) -> Option<LevelsStage> {
        if !(black >= 0.0 && black < white && white <= 1.0 && (0.1..=10.0).contains(&gamma)) {
            return None;
        }
        Some(LevelsStage {
            black,
            range: white - black,
            inv_gamma: 1.0 / gamma,
            out_black: 0.0,
            out_range: 1.0,
            toe: levels_toe(gamma),
        })
    }

    pub(crate) fn is_identity(&self) -> bool {
        *self == Self::IDENTITY
    }

    pub(crate) fn apply(&self, v: f32) -> f32 {
        let t = stage_curve(
            (v - self.black) / self.range,
            self.inv_gamma,
            self.toe.as_ref(),
        );
        if self.out_black == 0.0 && self.out_range == 1.0 {
            t
        } else {
            self.out_black + self.out_range * t
        }
    }

    /// The 8-bit form of [`LevelsStage::apply`] (module doc, R8(h,h,x)), in
    /// f64 on codes: `c = 255·v`, `in_lo = 255·black` and `in_hi =
    /// 255·(black + range)`, each snapped to its nearest integer only within
    /// [`CODE_SNAP`] (a byte input or a code-valued point lands there; a
    /// real-valued channel output stays real); `s = clamp((c − in_lo)·255 /
    /// (in_hi − in_lo), 0, 255)` rounded half up to a code; the curve
    /// ([`stage_curve`]) at that code, rounded half up to a code; then the
    /// output line. Two points closer than [`CODE_SNAP`] that snap to one
    /// code keep their unsnapped values, so the span is never zero.
    pub(crate) fn apply8(&self, v: f32) -> f32 {
        let c = snap_code(255.0 * f64::from(v));
        let lo = 255.0 * f64::from(self.black);
        let hi = 255.0 * (f64::from(self.black) + f64::from(self.range));
        let (in_lo, in_hi) = match (snap_code(lo), snap_code(hi)) {
            (l, h) if h > l => (l, h),
            _ => (lo, hi),
        };
        let s = ((c - in_lo) * 255.0 / (in_hi - in_lo)).clamp(0.0, 255.0);
        let code = (s + 0.5).floor();
        let f = f64::from(stage_curve(
            (code / 255.0) as f32,
            self.inv_gamma,
            self.toe.as_ref(),
        ));
        let f = (255.0 * f + 0.5).floor() / 255.0;
        (f64::from(self.out_black) + f64::from(self.out_range) * f) as f32
    }

    /// The 32-bit float form of [`LevelsStage::apply`], its master: the same
    /// stage with every clamp removed and the power taken signed,
    /// `t = (v - black) / (white - black)`, `sgn(t)·|t|^(1/gamma)`, then the
    /// output line. Photoshop 26.10.0's 32-bit Levels, measured (take2:
    /// within 1.5e-6 relative over every patch, profile and set).
    fn apply_float(&self, v: f32) -> f32 {
        let t = (v - self.black) / self.range;
        let p = t.signum() * t.abs().powf(self.inv_gamma);
        if self.out_black == 0.0 && self.out_range == 1.0 {
            p
        } else {
            self.out_black + self.out_range * p
        }
    }
}

/// A parsed `levels` op: the master stage and the red, green and blue
/// stages (identity when absent), and whether it runs the 8-bit form
/// (`eight_bit`, true at parse; derived, never persisted).
#[derive(Clone)]
pub(crate) struct Levels {
    master: LevelsStage,
    channels: [LevelsStage; 3],
    eight_bit: bool,
}

impl Levels {
    /// Parses the `levels` params per the schema row in `adjust`: the five
    /// master keys at the top level, and `r` / `g` / `b` each an OPTIONAL
    /// object of the same five keys. Unknown keys are ignored; a present
    /// `r` / `g` / `b` that is not an object, or any out-of-range value,
    /// makes the meta malformed (`None`).
    pub(crate) fn parse(params: &Map<String, Value>) -> Option<Levels> {
        let master = LevelsStage::parse(params)?;
        let mut channels = [LevelsStage::IDENTITY; 3];
        for (key, stage) in ["r", "g", "b"].into_iter().zip(&mut channels) {
            if let Some(value) = params.get(key) {
                *stage = LevelsStage::parse(value.as_object()?)?;
            }
        }
        Some(Levels {
            master,
            channels,
            eight_bit: true,
        })
    }

    /// The 16-bit form: every stage continuous ([`LevelsStage::apply`]).
    /// `PrecisionAdjustment::parse` calls it; the byte parse keeps the 8-bit
    /// form.
    pub(crate) fn use_precision(&mut self) {
        self.eight_bit = false;
    }

    /// One straight RGB triple in [0, 1]: each channel's stage, then the
    /// master's (the module doc's measured order), through
    /// [`LevelsStage::apply8`] in the 8-bit form, the channel's output passed
    /// on unrounded. Identity stages are skipped.
    pub(crate) fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let master = (!self.master.is_identity()).then_some(&self.master);
        let run = |stage: &LevelsStage, v: f32| {
            if self.eight_bit {
                stage.apply8(v)
            } else {
                stage.apply(v)
            }
        };
        std::array::from_fn(|c| {
            let channel = &self.channels[c];
            let mut v = rgb[c];
            if !channel.is_identity() {
                v = run(channel, v);
            }
            match master {
                Some(stage) => run(stage, v),
                None => v,
            }
        })
    }

    /// The 32-bit float form of [`Levels::apply`], its master: the same
    /// order and the same skipped identity stages, each stage through
    /// [`LevelsStage::apply_float`], on stored linear values with no clamp.
    pub(crate) fn apply_float(&self, rgb: [f32; 3]) -> [f32; 3] {
        let master = (!self.master.is_identity()).then_some(&self.master);
        std::array::from_fn(|c| {
            let channel = &self.channels[c];
            let mut v = rgb[c];
            if !channel.is_identity() {
                v = channel.apply_float(v);
            }
            match master {
                Some(stage) => stage.apply_float(v),
                None => v,
            }
        })
    }

    /// Every stage is the identity.
    pub(crate) fn is_identity(&self) -> bool {
        self.master.is_identity() && self.channels.iter().all(LevelsStage::is_identity)
    }
}
