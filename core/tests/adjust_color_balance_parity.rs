//! Color Balance measured in Photoshop 26.10.0 at 8, 16 and 32 bits
//! (receipt `doctored-fixtures/photoshop/adjust-parity-535-536/take6`,
//! `judged.json` authoritative; runner
//! `scripts/photoshop-probe/color_balance_take6.py`, sha256
//! `f1efa57d1cd31b0fdbaaad527e524e0d2fe3f031f24496334762461d6b5303d4`).
//!
//! The oracle is binary64, written here from that runner's docstring (the
//! form with take6's decided members: tone `sum`, pmid `midtrunc`, bw
//! `levels`, the 32-bit end slopes `stages`), never read off the core.
//! `oracles_reproduce_photoshop_take6` pins it against take6's own numbers
//! within the runner's tolerance (in range `(2 + E)·2^-15·(1 + f')`, beyond
//! `2^-14·(1 + |s|) + 2^-21·|x|·(1 + |s|)`), and take5's set T3 — the one
//! take5 left unmatched — is pinned the same way. The kernel is then checked
//! against the oracle: at 16 bits through `PrecisionAdjustment::apply_at`,
//! at 32 bits through `apply_linear`, and at 8 bits through the C ABI.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::ffi::*;
use doctored_core::ffi_adjust::*;
use doctored_core::icc::IccProfile;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common;
use common::*;

const Q15: f64 = 1.0 / 32768.0;
const EPS: f64 = 1.0 / 16_777_216.0;

/// A setting in Photoshop's whole units: shadows, midtones, highlights as
/// (cyan_red, magenta_green, yellow_blue), and Preserve Luminosity.
#[derive(Clone, Copy, Debug)]
struct Setting {
    s: [i32; 3],
    m: [i32; 3],
    h: [i32; 3],
    preserve: bool,
}

const fn off(s: [i32; 3], m: [i32; 3], h: [i32; 3]) -> Setting {
    Setting {
        s,
        m,
        h,
        preserve: false,
    }
}

const fn on(s: [i32; 3], m: [i32; 3], h: [i32; 3]) -> Setting {
    Setting {
        s,
        m,
        h,
        preserve: true,
    }
}

const Z: [i32; 3] = [0, 0, 0];

/// take6's sets, and take5's T3.
fn set(name: &str) -> Setting {
    match name {
        "U1" => off([33, 17, -45], [-40, 25, 12], [21, -40, -27]),
        "U2" => off([-51, 29, 70], [-35, 60, -90], [-3, -64, 33]),
        "U3" => off([-40, -15, 0], [20, 7, -35], [0, 0, 70]),
        "U4" => off([-99, 87, -3], [55, -71, 100], [-1, 99, -97]),
        "W1" => off([-60, -35, 22], [14, -27, 50], [70, 41, -8]),
        "W2" => off([-88, -5, -100], [-50, 33, 91], [36, 97, 100]),
        "V1" => on(Z, [-43, -65, 52], Z),
        "V2" => on(Z, [5, 37, 38], Z),
        "V3" => on([-25, 40, 11], [56, 75, -78], [30, -15, 52]),
        "V4" => on([100, -75, 32], Z, Z),
        "T3" => off([-73, -20, 45], [0, 64, -18], [91, -35, 15]),
        _ => unreachable!("{name}"),
    }
}

const TAKE6_SETS: [&str; 10] = ["U1", "U2", "U3", "U4", "W1", "W2", "V1", "V2", "V3", "V4"];

// ------------------------------------------------------------- oracle --

/// One channel's curve: black and white points, the r2 gamma and the two
/// stage end slopes.
struct Curve {
    b: f64,
    w: f64,
    gamma: f64,
    hi: f64,
    lo: f64,
}

/// floor(n/d + 1/2), d > 0.
fn rnd_ratio(n: i128, d: i128) -> i128 {
    (2 * n + d).div_euclid(2 * d)
}

/// The in-range curve on y: y^(1/gamma), with the cubic Hermite toe below
/// x0 = 2·2^(gamma/(p − 1)) for gamma > 1.
fn g(gamma: f64, y: f64) -> f64 {
    let p = 1.0 / gamma;
    if gamma > 1.0 {
        let x0 = 2.0 * 2f64.powf(gamma / (p - 1.0));
        if y < x0 {
            let (s0, p1, m1) = (2f64.powf(gamma), x0.powf(p), p * x0.powf(p - 1.0));
            let u = y / x0;
            return (u.powi(3) - 2.0 * u * u + u) * x0 * s0
                + (-2.0 * u.powi(3) + 3.0 * u * u) * p1
                + (u.powi(3) - u * u) * x0 * m1;
        }
    }
    y.powf(p)
}

impl Curve {
    fn new(b: i32, w: i32, t: f64) -> Curve {
        let gamma = (100.0 * 2f64.powf(t / 100.0) + 0.5).floor() / 100.0;
        let (bi, wi) = (i128::from(b), i128::from(w));
        // y(k/4096) = (255·k − 4096·b) / (4096·(w − b)).
        let den = 4096 * (wi - bi);
        let (hi, lo) = if gamma == 1.0 {
            // f = clamp(y, 0, 1) as an exact rational over `den`.
            let f = |k: i128| (255 * k - 4096 * bi).clamp(0, den);
            (
                rnd_ratio(32768 * (f(4096) - f(4095)), den) as f64 / 8.0,
                rnd_ratio(32768 * (f(1) - f(0)), den) as f64 / 8.0,
            )
        } else {
            let stage = |k: i128| rnd_ratio(255 * k - 4096 * bi, wi - bi).clamp(0, 4096);
            let level = |l: i128| (32768.0 * g(gamma, l as f64 / 4096.0) + 0.5).floor();
            (
                (level(4096) - level(stage(4095))) / 8.0,
                (level(stage(1)) - level(stage(0))) / 8.0,
            )
        };
        Curve {
            b: f64::from(b),
            w: f64::from(w),
            gamma,
            hi,
            lo,
        }
    }

    /// f(x) for x in [0, 1].
    fn f(&self, x: f64) -> f64 {
        let y = (x - self.b / 255.0) / ((self.w - self.b) / 255.0);
        g(self.gamma, y.clamp(0.0, 1.0))
    }

    /// f(x) for any x at 32 bits.
    fn f32x(&self, x: f64) -> f64 {
        if x > 1.0 {
            1.0 + (x - 1.0) * self.hi
        } else if x < 0.0 {
            x * self.lo
        } else {
            self.f(x)
        }
    }

    /// The runner's tolerance for one component.
    fn tolerance(&self, x: f64) -> f64 {
        if (0.0..=1.0).contains(&x) {
            let e = if self.b > 0.0 || self.w < 255.0 {
                4.0
            } else {
                0.0
            };
            let d = 1.0 / 65536.0;
            let (lo, hi) = ((x - d).max(0.0), (x + d).min(1.0));
            let slope = (self.f(hi) - self.f(lo)).abs() / (hi - lo);
            (2.0 + e) * Q15 * (1.0 + slope)
        } else {
            let s = if x > 1.0 { self.hi } else { self.lo }.abs();
            2.0 * Q15 * (1.0 + s) + 8.0 * EPS * x.abs() * (1.0 + s)
        }
    }
}

fn trunc_half(v: i32) -> f64 {
    (f64::from(v) / 2.0).trunc()
}

/// The three channel curves of a setting (the runner's `channel_params6`
/// with tone `sum`, pmid `midtrunc`).
fn curves(st: Setting) -> [Curve; 3] {
    let Setting { s, m, h, preserve } = st;
    std::array::from_fn(|c| {
        if preserve {
            let top = *s.iter().max().unwrap();
            let bottom = *h.iter().min().unwrap();
            let mid = m.iter().max().unwrap() + m.iter().min().unwrap();
            Curve::new(
                top - s[c],
                255 - (h[c] - bottom),
                f64::from(m[c]) - trunc_half(mid),
            )
        } else {
            Curve::new(
                (-s[c]).max(0),
                255 - h[c].max(0),
                f64::from(m[c]) + trunc_half(s[c] + h[c]),
            )
        }
    })
}

fn params(st: Setting) -> Value {
    let tone = |v: [i32; 3]| {
        json!({
            "cyan_red": f64::from(v[0]) / 100.0,
            "magenta_green": f64::from(v[1]) / 100.0,
            "yellow_blue": f64::from(v[2]) / 100.0,
        })
    };
    json!({
        "shadows": tone(st.s), "midtones": tone(st.m), "highlights": tone(st.h),
        "preserve_luminosity": st.preserve,
    })
}

fn prepared(params: &Value) -> PrecisionAdjustment {
    let meta = json!({
        "type": "adjust", "evaluation": "precision_v1", "op": "color_balance", "params": params
    })
    .to_string();
    PrecisionAdjustment::parse(&meta, &IccProfile::srgb()).unwrap()
}

// ---------------------------------------------------------- take rows --

/// (set, depth, the input as read back, Photoshop's output). At 16 bits the
/// values are the TIFF codes over 65535; at 32 the stored f32 values.
type Row = (&'static str, u32, [f64; 3], [f64; 3]);

#[allow(clippy::excessive_precision)]
const TAKE6: [Row; 25] = [
    // CB6-tone, the worst `sum` sample at 16 and 32 bits (ramp:11).
    (
        "U4",
        16,
        [
            0.04315251392385748,
            0.04315251392385748,
            0.04315251392385748,
        ],
        [0.0, 0.10165560387579156, 0.0788891432059205],
    ),
    (
        "U4",
        32,
        [
            0.04313725605607033,
            0.04313725605607033,
            0.04313725605607033,
        ],
        [0.0, 0.10162054747343063, 0.07883983105421066],
    ),
    // CB6-tone: mix6:7, ramp:200, fine6:30032.
    (
        "U1",
        16,
        [0.2627603570611124, 0.28628976882581825, 0.40784313725490196],
        [0.25296406500343327, 0.3207751583123522, 0.22452124818799116],
    ),
    (
        "U2",
        32,
        [0.7843137383460999, 0.7843137383460999, 0.7843137383460999],
        [0.6166698932647705, 0.8353056311607361, 0.8717035055160522],
    ),
    (
        "U4",
        16,
        [0.4582589456015869, 0.4582589456015869, 0.4582589456015869],
        [0.12445258258945602, 0.7794766155489433, 0.5692988479438468],
    ),
    // CB6-pmid, the worst `midtrunc` sample at 16 (ramp:30) and 32
    // (fine6:3920), then mix6:11 and ramp:90.
    (
        "V1",
        16,
        [
            0.11764705882352941,
            0.11764705882352941,
            0.11764705882352941,
        ],
        [
            0.062073701075761045,
            0.03909361409933623,
            0.23782711528191042,
        ],
    ),
    (
        "V1",
        32,
        [
            0.05981536582112312,
            0.05981536582112312,
            0.05981536582112312,
        ],
        [
            0.025787923485040665,
            0.014007911086082458,
            0.14645567536354065,
        ],
    ),
    (
        "V1",
        16,
        [0.3607843137254902, 0.4588387884336614, 0.35294117647058826],
        [0.2660563057907988, 0.307164110780499, 0.49710841535057604],
    ),
    (
        "V2",
        32,
        [0.3529411852359772, 0.3529411852359772, 0.3529411852359772],
        [0.3143669664859772, 0.3945976793766022, 0.3978630602359772],
    ),
    // CB6-bw and CB6-form, the worst `levels` / `fitted` sample at 16
    // (fine6:54672) and 32 (ramp:5), then mix6:3, ramp:30, ramp:220.
    (
        "W1",
        16,
        [0.8342259861142901, 0.8342259861142901, 0.8342259861142901],
        [1.0, 0.9919737544823377, 0.8847333485923552],
    ),
    (
        "W2",
        32,
        [
            0.019607843831181526,
            0.019607843831181526,
            0.019607843831181526,
        ],
        [0.0, 0.000507434131577611, 0.0],
    ),
    (
        "W1",
        16,
        [0.7098039215686275, 0.13727016098268102, 0.9490043488212405],
        [
            0.9716182192721446,
            3.0518043793392844e-05,
            0.965270466163119,
        ],
    ),
    (
        "W2",
        32,
        [
            0.11764705926179886,
            0.11764705926179886,
            0.11764705926179886,
        ],
        [0.0, 0.35109397768974304, 0.0],
    ),
    (
        "V3",
        16,
        [0.862729839017319, 0.862729839017319, 0.862729839017319],
        [1.0, 0.9163500419623102, 1.0],
    ),
    // CB6-form: ramp:128 on U3, mix6:20 on V4.
    (
        "U3",
        16,
        [0.5019455252918288, 0.5019455252918288, 0.5019455252918288],
        [0.40921644922560463, 0.4708323796444648, 0.6917982757305257],
    ),
    (
        "V4",
        32,
        [0.6941176652908325, 0.9686274528503418, 0.04313725605607033],
        [0.6941176652908325, 0.8999923467636108, 0.0],
    ),
    // CB6-x32: the worst `stages` sample (V1, extra6:1.2,-0.06,6), then
    // EXTRA6 rows; U3 and V4 carry the two half-way ties on G (b = 15,
    // 2040/240 = 8.5 -> 9/8; b = 175, 2040/80 = 25.5 -> 26/8).
    (
        "V1",
        32,
        [1.2000000476837158, -0.05999999865889549, 6.0],
        [1.25, 0.0, 4.125],
    ),
    ("U3", 32, [3.0, 3.0, 3.0], [3.25, 3.25, 1.0]),
    ("V4", 32, [8.0, 8.0, 8.0], [8.0, 23.75, 10.625]),
    ("U1", 32, [32.0, 32.0, 32.0], [1.0, 28.125, 35.875]),
    ("W2", 32, [-2.0, -2.0, -2.0], [0.0, 0.0, 0.0]),
    ("V1", 32, [-0.5, -0.5, -0.5], [-0.0625, 0.0, -1.375]),
    (
        "U2",
        32,
        [9.0, 0.05000000074505806, 0.8999999761581421],
        [13.0, 0.10856323689222336, 1.0],
    ),
    (
        "W1",
        32,
        [-0.4000000059604645, 1.600000023841858, 0.30000001192092896],
        [0.0, 1.0, 0.44331055879592896],
    ),
    (
        "V3",
        32,
        [1.2000000476837158, -0.05999999865889549, 6.0],
        [1.0, -0.19499999284744263, 1.0],
    ),
];

/// take5's T3, its worst sample under the take6 form at 16 and 32 bits
/// (take5 sample 477: FINE5's 16-bit code 64·93 + 32).
#[allow(clippy::excessive_precision)]
const TAKE5_T3: [Row; 2] = [
    (
        "T3",
        16,
        [
            0.09130998702983138,
            0.09130998702983138,
            0.09130998702983138,
        ],
        [0.0, 0.03314259555962463, 0.11752498664835584],
    ),
    (
        "T3",
        32,
        [
            0.09130998700857162,
            0.09130998700857162,
            0.09130998700857162,
        ],
        [0.0, 0.033145226538181305, 0.11752475798130035],
    ),
];

/// The largest tolerance-unit miss of the oracle over `rows`.
fn worst_units(rows: &[Row]) -> f64 {
    let mut worst = 0.0f64;
    for &(name, depth, x, got) in rows {
        let cs = curves(set(name));
        for c in 0..3 {
            let want = if depth == 32 {
                cs[c].f32x(x[c])
            } else {
                cs[c].f(x[c])
            };
            let units = (got[c] - want).abs() / cs[c].tolerance(x[c]);
            assert!(
                units <= 1.0,
                "{name} @{depth} {x:?} channel {c}: Photoshop {}, oracle {want}, {units} units",
                got[c]
            );
            worst = worst.max(units);
        }
    }
    worst
}

#[test]
fn oracles_reproduce_photoshop_take6() {
    let worst = worst_units(&TAKE6);
    // take6's verdicts put every statement's decided member under 0.74.
    assert!(worst <= 0.75, "worst {worst}");
    // The ties, as slopes.
    assert_eq!(curves(set("U3"))[1].hi, 9.0 / 8.0);
    assert_eq!(curves(set("V4"))[1].hi, 26.0 / 8.0);
}

#[test]
fn oracles_reproduce_photoshop_take5_t3() {
    let worst = worst_units(&TAKE5_T3);
    assert!(worst <= 0.7, "worst {worst}");
}

// -------------------------------------------------------------- kernel --

/// Every take6 set, plus each tone and axis alone at ±1, ±50 and ±100,
/// both ways round Preserve Luminosity.
fn settings() -> Vec<Setting> {
    let mut out: Vec<Setting> = TAKE6_SETS.iter().map(|n| set(n)).collect();
    out.push(set("T3"));
    for tone in 0..3 {
        for axis in 0..3 {
            for amount in [1, -1, 50, -50, 100, -100] {
                for preserve in [false, true] {
                    let mut tones = [Z; 3];
                    tones[tone][axis] = amount;
                    out.push(Setting {
                        s: tones[0],
                        m: tones[1],
                        h: tones[2],
                        preserve,
                    });
                }
            }
        }
    }
    out
}

fn close(got: f32, want: f64) -> bool {
    (f64::from(got) - want).abs() <= 4.0 * EPS * want.abs().max(1.0)
}

#[test]
fn kernel_matches_the_oracle() {
    let ins16: Vec<f32> = (0..=4096).map(|i| i as f32 / 4096.0).collect();
    let ins32: Vec<f32> = (0..=3600)
        .map(|i| -2.0 + 36.0 * i as f32 / 3600.0)
        .collect();
    for st in settings() {
        let op = prepared(&params(st));
        let cs = curves(st);
        for &x in &ins16 {
            let got = op.apply_at([x; 3], 0.0, (0, 0));
            for c in 0..3 {
                let want = cs[c].f(f64::from(x));
                assert!(
                    close(got[c], want),
                    "{st:?} @16 {x} ch {c}: {} vs {want}",
                    got[c]
                );
            }
        }
        for &x in ins32.iter().chain(&ins16) {
            let got = op
                .apply_linear([x; 3], (0, 0))
                .expect("admitted at 32 bits");
            for c in 0..3 {
                let want = cs[c].f32x(f64::from(x));
                assert!(
                    close(got[c], want),
                    "{st:?} @32 {x} ch {c}: {} vs {want}",
                    got[c]
                );
            }
        }
    }
}

/// 8 bits through the C ABI: each byte is round(255·oracle), off by one
/// only where 255·oracle sits within 1e-3 of a half (the f32 kernel's own
/// rounding can land either side there).
#[test]
fn eight_bit_is_the_rounded_curve() {
    let dir = TempDir::new().unwrap();
    let ramp = RgbaImage::from_fn(256, 1, |x, _| {
        let v = x as u8;
        Rgba([v, v, v, 255])
    });
    let img = open_pattern(&dir, "cb-ramp.png", &ramp);
    for st in TAKE6_SETS.iter().map(|n| set(n)).chain([set("T3")]) {
        let c_op = CString::new("color_balance").unwrap();
        let c_params = CString::new(params(st).to_string()).unwrap();
        let mut err: *mut c_char = ptr::null_mut();
        let out = unsafe { dr_image_adjust_op(img, c_op.as_ptr(), c_params.as_ptr(), &mut err) };
        assert!(!out.is_null(), "{st:?}: {}", take_err_string(err));
        let bytes = pixels(out);
        unsafe { dr_image_free(out) };
        let cs = curves(st);
        for (code, px) in bytes.chunks_exact(4).enumerate() {
            for c in 0..3 {
                let v = 255.0 * cs[c].f(code as f64 / 255.0);
                let want = (v + 0.5).floor();
                let got = f64::from(px[c]);
                let near_half = ((v - v.floor()) - 0.5).abs() <= 1e-3;
                assert!(
                    got == want || (near_half && (got - want).abs() <= 1.0),
                    "{st:?} code {code} ch {c}: {got} vs 255·oracle {v}"
                );
            }
        }
    }
    free(img);
}

/// The amounts act in whole Photoshop units, `round(100·amount)` on the
/// JSON number itself, as the PSD `blnc` writer rounds it.
#[test]
fn amounts_act_in_whole_units() {
    let render = |v: f64| {
        let op = prepared(&json!({"midtones": {"cyan_red": v}, "preserve_luminosity": false}));
        let out: Vec<u32> = (0..=255)
            .flat_map(|k| op.apply_at([k as f32 / 255.0; 3], 0.0, (0, 0)))
            .map(f32::to_bits)
            .collect();
        (op.is_identity(), out)
    };
    assert_eq!(render(0.294), render(0.29), "0.294 acts as 0.29");
    assert_eq!(render(0.295), render(0.30), "0.295 acts as 0.30");
    assert_eq!(render(0.005), render(0.01), "0.005 acts as 0.01");
    assert!(!render(0.005).0, "0.005 is one unit, not identity");
    assert!(render(0.004).0, "0.004 rounds to no unit at all");
    assert_ne!(render(0.29), render(0.30));
}
