//! Hue/Saturation's Colorize and master Lightness, measured in
//! Photoshop 26.10.0 (receipt
//! `doctored-fixtures/photoshop/adjust-parity-535-536/take3`, `judged.json`
//! authoritative; runner `scripts/photoshop-probe/hue_sat_parity.py`, sha256
//! `a696e0fe98b7b9c22d19c181b48f82da7df2c116bb89e42c87c3a702c5586a0f`).
//!
//! The oracles are binary64, written here from that runner's docstring
//! (F-Lm and F-colorize, `C16` at 16 bits and `C32` at 32), never read off
//! the core. `oracles_reproduce_photoshop_take3` pins them against take3's
//! own numbers within the runner's tolerance (16 bits `4·2^-15`, no gain or
//! bands; 32 bits `16·2^-24·max(1, max|x|)`), and the kernel is then
//! checked against them: at 16 bits through `PrecisionAdjustment::apply_at`
//! (today's Colorize IS `C16`), at 32 bits through `apply_linear`.
//!
//! Master Hue alone at 32 bits is pinned by take4
//! (`doctored-fixtures/photoshop/adjust-parity-535-536/take4`, runner
//! `scripts/photoshop-probe/hue_sat_take4.py`, sha256
//! `d44d1d1c819e380a5fdea31cdd690517515c85bb68df09d27627b69f61665db4`): the
//! runner's `s4096t` member, a shift on a 1/24576-turn grid truncated toward
//! zero, rotating the hexcone hue. Its oracle is written from that runner's
//! docstring; `oracles_reproduce_photoshop_take4` pins it against take4's
//! numbers within take3's 32-bit tolerance (`8·2^-24·(1 + K)·max(1, max|x|)`
//! with K = 1 and no bands). Every other setting takes the display-scaled form.

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::icc::IccProfile;
use serde_json::{json, Value};

/// take3's five Colorize settings: (hue, saturation, lightness).
const COLORIZE: [(f64, f64, f64); 5] = [
    (75.0, 0.40, 0.0),
    (310.0, 0.80, -0.65),
    (180.0, 0.15, 0.55),
    (45.0, 1.0, -1.0),
    (260.0, 0.50, 1.0),
];
const LIGHTNESS: [f64; 6] = [0.10, -0.10, 0.33, -0.33, 0.80, -0.80];
/// The runner's EXTRA3 triples (32 bits only).
const EXTRA3: [[f32; 3]; 9] = [
    [1.1, 1.1, 1.1],
    [3.0, 3.0, 3.0],
    [10.0, 10.0, 10.0],
    [2.5, 0.4, 0.1],
    [0.2, 1.3, 0.6],
    [5.0, 4.0, 0.5],
    [-0.1, 0.3, 0.7],
    [0.6, -0.3, 0.1],
    [-0.2, -0.4, 0.9],
];
/// take4's hue set and a few more, including the grid's exact multiples of
/// 15 deg and both ends of the slider.
const HUES: [f64; 10] = [
    7.0, 25.0, -55.0, 70.0, -100.0, -172.0, -20.0, 45.0, 180.0, -180.0,
];
/// The extended values each 32-bit lattice channel adds.
const EXTENDED: [f32; 4] = [1.5, 4.0, 16.0, -0.25];

// ------------------------------------------------------------- oracles --

/// The runner's `from_hexcone`: the triple with that max, min and hue.
fn from_hexcone(big: f64, small: f64, h: f64) -> [f64; 3] {
    let c = big - small;
    let hp = h.rem_euclid(360.0) / 60.0;
    let f = hp - hp.floor();
    let (up, down) = (small + c * f, big - c * f);
    match hp.floor() as u32 % 6 {
        0 => [big, up, small],
        1 => [down, big, small],
        2 => [small, big, up],
        3 => [small, down, big],
        4 => [up, small, big],
        _ => [big, small, down],
    }
}

/// F-Lm on one value; `held` is the 32-bit rule (a value already past the
/// target is unchanged).
fn f_lm(c: f64, a: f64, held: bool) -> f64 {
    if a > 0.0 {
        if held && c > 1.0 {
            c
        } else {
            c + (1.0 - c) * a
        }
    } else if a < 0.0 {
        if held && c < 0.0 {
            c
        } else {
            c * (1.0 + a)
        }
    } else {
        c
    }
}

/// F-colorize: `C16` (`form32` false) or `C32`.
fn colorize(x: [f64; 3], (h, s, a): (f64, f64, f64), form32: bool) -> [f64; 3] {
    let mid = (x[0].max(x[1]).max(x[2]) + x[0].min(x[1]).min(x[2])) / 2.0;
    let y = f_lm(mid, a, form32);
    if form32 {
        let c = s * (1.0 - (2.0 * y - 1.0).abs()) * (1.0 - a.abs());
        return from_hexcone(y + c / 2.0, y - c / 2.0, h);
    }
    let c = (s * (1.0 - (2.0 * y - 1.0).abs())).max(0.0);
    from_hexcone(y + c / 2.0, y - c / 2.0, h).map(|v| v.clamp(0.0, 1.0))
}

/// The runner's `hue_of`: the hexcone hue in degrees, `None` for a grey.
fn hue_of(x: [f64; 3]) -> Option<f64> {
    let big = x[0].max(x[1]).max(x[2]);
    let small = x[0].min(x[1]).min(x[2]);
    let c = big - small;
    if c <= 0.0 {
        return None;
    }
    let h = if big == x[0] {
        ((x[1] - x[2]) / c).rem_euclid(6.0)
    } else if big == x[1] {
        (x[2] - x[0]) / c + 2.0
    } else {
        (x[0] - x[1]) / c + 4.0
    };
    Some((60.0 * h).rem_euclid(360.0))
}

/// take4's `s4096t`: a shift in degrees on the 1/24576-turn grid,
/// truncated toward zero.
fn s4096t(d: f64) -> f64 {
    (d * 24576.0 / 360.0).trunc() * 360.0 / 24576.0
}

/// Master Hue alone at 32 bits: the hexcone rotation by `s4096t`; a grey is
/// unchanged.
fn hue32(x: [f64; 3], d: f64) -> [f64; 3] {
    match hue_of(x) {
        None => x,
        Some(h) => from_hexcone(
            x[0].max(x[1]).max(x[2]),
            x[0].min(x[1]).min(x[2]),
            h + s4096t(d),
        ),
    }
}

/// F-Lm per component at 32 bits.
fn lightness32(x: [f64; 3], a: f64) -> [f64; 3] {
    x.map(|c| f_lm(c, a, true))
}

// -------------------------------------------------------------- kernel --

fn colorize_params((h, s, a): (f64, f64, f64)) -> Value {
    json!({"colorize": true, "colorize_hue": h, "colorize_saturation": s, "colorize_lightness": a})
}

fn prepared(params: &Value) -> PrecisionAdjustment {
    let meta = json!({
        "type": "adjust", "evaluation": "precision_v1", "op": "hue_saturation", "params": params
    })
    .to_string();
    PrecisionAdjustment::parse(&meta, &IccProfile::srgb()).unwrap()
}

fn at16(op: &PrecisionAdjustment, x: [f32; 3]) -> [f32; 3] {
    op.apply_at(x, 0.0, (0, 0))
}

fn at32(op: &PrecisionAdjustment, x: [f32; 3]) -> [f32; 3] {
    op.apply_linear(x, (0, 0)).unwrap()
}

fn wide(v: [f32; 3]) -> [f64; 3] {
    v.map(f64::from)
}

fn t16() -> f64 {
    4.0 * 2f64.powi(-15)
}

fn t32(x: [f32; 3]) -> f64 {
    let big = x.iter().fold(1.0f64, |m, &c| m.max(f64::from(c).abs()));
    16.0 * 2f64.powi(-24) * big
}

/// The largest component error.
fn worst(got: [f64; 3], want: [f64; 3]) -> f64 {
    (0..3).map(|k| (got[k] - want[k]).abs()).fold(0.0, f64::max)
}

// --------------------------------------------------------- take3 rows --

#[derive(Clone, Copy)]
enum Form {
    C16((f64, f64, f64)),
    C32((f64, f64, f64)),
    Lm32(f64),
    Hue32(f64),
}

/// take3's rows: the form, the input as stored, Photoshop's output. The
/// inputs keep the receipt's printed digits, which name the same f32 values
/// as clippy's shorter spellings would, so they can be grepped in it.
#[allow(clippy::excessive_precision)]
fn take3() -> Vec<(Form, [f32; 3], [f64; 3])> {
    use Form::*;
    vec![
        (
            C16((75.0, 0.40, 0.0)),
            [0.596063172; 3],
            [0.676874952, 0.757656214, 0.434546426],
        ),
        (
            C16((310.0, 0.80, -0.65)),
            [0.937254902; 3],
            [0.590447852, 0.0656137942, 0.502922103],
        ),
        (
            C16((180.0, 0.15, 0.55)),
            [0.0392156863, 0.062745098, 0.062745098],
            [0.508842603, 0.636957351, 0.636957351],
        ),
        (
            C32((75.0, 0.40, 0.0)),
            [10.0, 10.0, 10.0],
            [8.19999886, 6.4000001, 13.6000004],
        ),
        (
            C32((75.0, 0.40, 0.0)),
            [2.5, 0.4, 0.1],
            [1.24000001, 1.17999995, 1.41999996],
        ),
        (
            C32((310.0, 0.80, -0.65)),
            [0.909803927; 3],
            [0.407592237, 0.229270607, 0.37787196],
        ),
        (
            C32((310.0, 0.80, -0.65)),
            [0.6, -0.3, 0.1],
            [0.0672000051, 0.0378000028, 0.0623000078],
        ),
        (
            C32((180.0, 0.15, 0.55)),
            [2.5, 0.4, 0.1],
            [1.32024992, 1.27974987, 1.27974987],
        ),
        (C32((45.0, 1.0, -1.0)), [-0.2, -0.4, 0.9], [0.0, 0.0, 0.0]),
        (C32((260.0, 0.50, 1.0)), [2.5, 0.4, 0.1], [1.29999995; 3]),
        (
            Lm32(-0.80),
            [0.905882359, 0.0, 1.0],
            [0.181176618, 0.0, 0.200000048],
        ),
        (Lm32(-0.80), [3.0, 3.0, 3.0], [0.600000143; 3]),
        (
            Lm32(-0.33),
            [-0.1, 0.3, 0.7],
            [-0.099999994, 0.201000154, 0.469000012],
        ),
        (
            Lm32(0.10),
            [1.0, 0.0, 0.78039217],
            [1.0, 0.100000024, 0.802352428],
        ),
        (
            Lm32(0.10),
            [-0.1, 0.3, 0.7],
            [0.00999999046, 0.369999886, 0.729999959],
        ),
        (Lm32(0.80), [3.0, 3.0, 3.0], [3.0, 3.0, 3.0]),
    ]
}

/// take4's rows for master Hue alone: its worst in-range sample (Hue +7),
/// its worst EXTRA4 sample (Hue +25), another EXTRA4 row from the wide32
/// document, and three more settings.
#[allow(clippy::excessive_precision)]
fn take4() -> Vec<(Form, [f32; 3], [f64; 3])> {
    use Form::*;
    vec![
        (
            Hue32(7.0),
            [0.992156863, 0.00392156886, 0.996078432],
            [0.996078432, 0.00392156839, 0.884457588],
        ),
        (
            Hue32(25.0),
            [-0.150000006, 0.5, 0.800000012],
            [-0.150000036, 0.104320943, 0.800000012],
        ),
        (
            Hue32(25.0),
            [0.300000012, 2.20000005, 0.899999976],
            [0.299999952, 2.20000005, 1.69135797],
        ),
        (
            Hue32(-172.0),
            [0.262745112, 0.615686297, 0.149019614],
            [0.56428194, 0.149019599, 0.615686297],
        ),
        (
            Hue32(-55.0),
            [0.996078432, 0.392156869, 0.00392156886],
            [0.996078432, 0.00392156839, 0.525001764],
        ),
        (
            Hue32(-100.0),
            [-0.150000006, 0.5, 0.800000012],
            [0.18317847, 0.800000012, -0.150000036],
        ),
    ]
}

fn oracle(form: Form, x: [f32; 3]) -> [f64; 3] {
    match form {
        Form::C16(p) => colorize(wide(x), p, false),
        Form::C32(p) => colorize(wide(x), p, true),
        Form::Lm32(a) => lightness32(wide(x), a),
        Form::Hue32(d) => hue32(wide(x), d),
    }
}

fn kernel(form: Form, x: [f32; 3]) -> [f64; 3] {
    let got = match form {
        Form::C16(p) => at16(&prepared(&colorize_params(p)), x),
        Form::C32(p) => at32(&prepared(&colorize_params(p)), x),
        Form::Lm32(a) => at32(&prepared(&json!({ "lightness": a })), x),
        Form::Hue32(d) => at32(&prepared(&json!({ "hue": d })), x),
    };
    wide(got)
}

fn tolerance(form: Form, x: [f32; 3]) -> f64 {
    match form {
        Form::C16(_) => t16(),
        Form::C32(_) | Form::Lm32(_) | Form::Hue32(_) => t32(x),
    }
}

#[test]
fn oracles_reproduce_photoshop_take3() {
    for (form, x, photoshop) in take3() {
        let want = oracle(form, x);
        let err = worst(want, photoshop);
        assert!(
            err <= tolerance(form, x),
            "{x:?}: oracle {want:?}, Photoshop {photoshop:?}, off by {err:e}"
        );
    }
}

#[test]
fn kernel_reproduces_photoshop_take3() {
    for (form, x, photoshop) in take3() {
        let got = kernel(form, x);
        let err = worst(got, photoshop);
        assert!(
            err <= tolerance(form, x),
            "{x:?}: kernel {got:?}, Photoshop {photoshop:?}, off by {err:e}"
        );
    }
}

#[test]
fn oracles_reproduce_photoshop_take4() {
    for (form, x, photoshop) in take4() {
        let want = oracle(form, x);
        let err = worst(want, photoshop);
        assert!(
            err <= tolerance(form, x),
            "{x:?}: oracle {want:?}, Photoshop {photoshop:?}, off by {err:e}"
        );
    }
}

#[test]
fn kernel_reproduces_photoshop_take4() {
    for (form, x, photoshop) in take4() {
        let got = kernel(form, x);
        let err = worst(got, photoshop);
        assert!(
            err <= tolerance(form, x),
            "{x:?}: kernel {got:?}, Photoshop {photoshop:?}, off by {err:e}"
        );
    }
}

// ------------------------------------------------------------- lattice --

/// Every 17th 16-bit code, `257·17·j` for j = 0…15, as a channel value.
fn codes() -> Vec<f32> {
    (0..16u32)
        .map(|j| (257 * 17 * j) as f32 / 65535.0)
        .collect()
}

fn triples(channel: &[f32]) -> Vec<[f32; 3]> {
    let mut out = Vec::with_capacity(channel.len().pow(3));
    for &r in channel {
        for &g in channel {
            for &b in channel {
                out.push([r, g, b]);
            }
        }
    }
    out
}

#[test]
fn colorize_at_16_bits_is_c16() {
    let lattice = triples(&codes());
    assert_eq!(lattice.len(), 4096);
    for p in COLORIZE {
        let op = prepared(&colorize_params(p));
        for &x in &lattice {
            let got = wide(at16(&op, x));
            let want = colorize(wide(x), p, false);
            assert!(
                worst(got, want) <= 2e-6,
                "C16 {p:?} on {x:?}: got {got:?}, want {want:?}"
            );
        }
    }
}

#[test]
fn colorize_and_lightness_at_32_bits() {
    let mut channel = codes();
    channel.extend(EXTENDED);
    let mut samples = triples(&channel);
    assert_eq!(samples.len(), 8000);
    samples.extend(EXTRA3);
    let check = |op: &PrecisionAdjustment, what: String, oracle: &dyn Fn([f64; 3]) -> [f64; 3]| {
        for &x in &samples {
            let got = wide(at32(op, x));
            let want = oracle(wide(x));
            assert!(
                (0..3).all(|k| (got[k] - want[k]).abs() <= 2e-6 * want[k].abs().max(1.0)),
                "{what} on {x:?}: got {got:?}, want {want:?}"
            );
        }
    };
    for p in COLORIZE {
        let op = prepared(&colorize_params(p));
        check(&op, format!("C32 {p:?}"), &|x| colorize(x, p, true));
    }
    for a in LIGHTNESS {
        let op = prepared(&json!({ "lightness": a }));
        check(&op, format!("Lm {a}"), &|x| lightness32(x, a));
    }
}

/// The admitted master sliders over the 32-bit lattice and EXTRA3, within
/// `2e-6·(1 + K)·max(1, |want|)` (K = 1: no saturation is admitted), |want|
/// the triple's largest magnitude: the f32 rotation rebuilds every component
/// from L and C, so its rounding scales with the triple, not the component.
#[test]
fn master_sliders_at_32_bits() {
    let mut channel = codes();
    channel.extend(EXTENDED);
    let mut samples = triples(&channel);
    assert_eq!(samples.len(), 8000);
    samples.extend(EXTRA3);
    for d in HUES {
        let op = prepared(&json!({ "hue": d }));
        for &x in &samples {
            let got = wide(at32(&op, x));
            let want = hue32(wide(x), d);
            let scale = want.iter().fold(1.0f64, |m, &c| m.max(c.abs()));
            assert!(
                worst(got, want) <= 4e-6 * scale,
                "Hue {d} on {x:?}: got {got:?}, want {want:?}"
            );
        }
    }
}

#[test]
fn every_other_setting_takes_the_display_form_at_32_bits() {
    let x = [0.5, -0.25, 2.0];
    for params in [
        json!({"saturation": 0.3}),
        json!({"hue": 20, "saturation": 0.3}),
        json!({"hue": 20, "lightness": 0.3}),
        json!({"saturation": 0.3, "lightness": 0.3}),
        json!({"hue": 20, "bands": {"greens": {"hue": 10}}}),
        json!({"bands": {"reds": {"hue": 20}}}),
        json!({"bands": {"reds": {"saturation": 0.3}}}),
        json!({"bands": {"reds": {"lightness": 0.3}}}),
    ] {
        // Display-scaled (`float_display_adjustments.rs` holds the
        // oracle); here only that it is admitted and finite.
        let got = prepared(&params).apply_linear(x, (0, 0)).expect("admitted");
        assert!(got.iter().all(|v| v.is_finite()), "{params}");
    }
    for params in [json!({"bands": {"reds": {"center": 10}}}), json!({})] {
        let got = prepared(&params).apply_linear(x, (0, 0)).expect("admitted");
        assert_eq!(got.map(f32::to_bits), x.map(f32::to_bits), "{params}");
    }
}

// ------------------------------------------------- R0 at 8 and 16 bits --
//
// take4's reference form R0 (the runner's docstring: FORM4 stages 1-6 with
// gain `tid`, hue `s4096t`, domain16 `float`, weights `h359`, combine
// `mgrid`, order `before`, net `netted`), written here in binary64 from
// that docstring. take4 recorded R0's 8-bit maxima as 2 codes on its
// lightness, gain and hue sets and 3 on its master set; the rows below are
// take4's worst srgb8 / srgb16 samples of those sets, and the oracle is held
// to exactly those bounds (16 bits: 2 8-bit codes, `2·257` 16-bit codes).

/// Photoshop's hue wheel units a turn.
const WHEEL: f64 = 1536.0;

/// One R0 band in Photoshop units: handles in degrees, hue, saturation,
/// lightness.
#[derive(Clone, Copy)]
struct R0Band {
    handles: [f64; 4],
    h: f64,
    s: f64,
    l: f64,
}

/// One R0 setting in Photoshop units.
#[derive(Clone)]
pub(crate) struct R0 {
    h: f64,
    s: f64,
    l: f64,
    bands: Vec<R0Band>,
}

fn rha(v: f64) -> f64 {
    if v >= 0.0 {
        (v + 0.5).floor()
    } else {
        -((-v + 0.5).floor())
    }
}

/// take3's `kslider`: 255/(255 - floor(254 s/100)) above 0, 1 + s/100 at
/// or below.
fn kslider(s: f64) -> f64 {
    if s <= 0.0 {
        1.0 + s / 100.0
    } else {
        255.0 / (255.0 - (254.0 * s / 100.0).floor())
    }
}

/// `tid`: G = 256 + trunc(256 Km - 256).
fn tid(s: f64) -> f64 {
    256.0 + (256.0 * kslider(s) - 256.0).trunc()
}

/// take3's F-wheel H(d).
fn wheel_handle(d: f64) -> f64 {
    (d * WHEEL / 359.0 + 0.5).floor().rem_euclid(WHEEL)
}

/// take3's band weight at wheel position `t` (arcs mod 1536).
fn band_weight(t: f64, quad: [f64; 4]) -> f64 {
    let [a, b, c, d] = quad;
    let t = (t - a).rem_euclid(WHEEL);
    let (tb, tc, td) = (
        (b - a).rem_euclid(WHEEL),
        (c - a).rem_euclid(WHEEL),
        (d - a).rem_euclid(WHEEL),
    );
    if t <= tb {
        if tb > 0.0 {
            t / tb
        } else {
            1.0
        }
    } else if t <= tc {
        1.0
    } else if t <= td && td > tc {
        (td - t) / (td - tc)
    } else {
        0.0
    }
}

/// take3's `sat_stage`, bounded: about (M + m)/2, K limited to the box.
fn sat_stage(v: [f64; 3], k: f64) -> [f64; 3] {
    let big = v[0].max(v[1]).max(v[2]);
    let small = v[0].min(v[1]).min(v[2]);
    if big == small {
        return v;
    }
    let mid = (big + small) / 2.0;
    let mut k = k;
    for c in v {
        if c > mid {
            k = k.min((1.0 - mid) / (c - mid));
        } else if c < mid {
            k = k.min((0.0 - mid) / (c - mid));
        }
    }
    v.map(|c| mid + (c - mid) * k)
}

/// R0 on one triple in [0, 1] (8/16 bits).
pub(crate) fn r0(x: [f64; 3], p: &R0) -> [f64; 3] {
    let a = p.l / 100.0;
    let mut v = x.map(|c| f_lm(c, a, false));
    let quads: Vec<[f64; 4]> = p
        .bands
        .iter()
        .map(|b| b.handles.map(wheel_handle))
        .collect();
    let mut dh = if p.h != 0.0 { s4096t(p.h) } else { 0.0 };
    let h = hue_of(v);
    let mut ws = None;
    if let Some(h) = h {
        if !p.bands.is_empty() {
            let pos = h * WHEEL / 360.0;
            let w: Vec<f64> = quads.iter().map(|&q| band_weight(pos, q)).collect();
            for (wb, b) in w.iter().zip(&p.bands) {
                if b.h != 0.0 {
                    dh += wb * s4096t(b.h);
                }
            }
            ws = Some((pos, w));
        }
        if dh != 0.0 {
            v = from_hexcone(v[0].max(v[1]).max(v[2]), v[0].min(v[1]).min(v[2]), h + dh);
        }
    }
    let g0 = tid(p.s);
    let mut k = g0.min(32767.0) / 256.0;
    if let Some((pos, w)) = ws {
        let big = v[0].max(v[1]).max(v[2]);
        let small = v[0].min(v[1]).min(v[2]);
        let lb: f64 = w.iter().zip(&p.bands).map(|(wb, b)| wb * b.l).sum::<f64>() / 100.0;
        if lb > 0.0 {
            v = v.map(|c| c + (big - c) * lb);
        } else if lb < 0.0 {
            v = v.map(|c| c + (c - small) * lb);
        }
        let sat: Vec<([f64; 4], f64)> = quads
            .iter()
            .zip(&p.bands)
            .filter(|(_, b)| b.s != 0.0)
            .map(|(&q, b)| (q, rha(g0 * (tid(b.s) - 256.0) / 256.0)))
            .collect();
        if !sat.is_empty() {
            let entry = |k: f64| {
                let sum: f64 = sat.iter().map(|&(q, d)| rha(band_weight(k, q) * d)).sum();
                (g0 + sum).clamp(0.0, 32767.0)
            };
            let k0 = pos.floor();
            let f = pos - k0;
            let (t0, t1) = (
                entry(k0.rem_euclid(WHEEL)),
                entry((k0 + 1.0).rem_euclid(WHEEL)),
            );
            k = (t0 + (t1 - t0) * f) / 256.0;
        }
    }
    if k != 1.0 {
        v = sat_stage(v, k);
    }
    v.map(|c| c.clamp(0.0, 1.0))
}

pub(crate) fn master(h: f64, s: f64, l: f64) -> R0 {
    R0 {
        h,
        s,
        l,
        bands: Vec::new(),
    }
}

/// take4's worst rows per family: (setting, depth, input codes, Photoshop's
/// codes), from `measured.json` through the runner's own loader.
fn take4_r0_rows() -> Vec<(R0, u32, [u32; 3], [u32; 3])> {
    vec![
        (master(0.0, 0.0, 17.0), 8, [254, 93, 1], [254, 119, 44]),
        (master(0.0, 0.0, 17.0), 8, [254, 110, 1], [254, 133, 44]),
        (master(0.0, 0.0, 61.0), 8, [1, 242, 254], [155, 250, 255]),
        (
            master(0.0, 0.0, -22.0),
            16,
            [65277, 33409, 258],
            [50917, 26068, 202],
        ),
        (
            master(0.0, 0.0, -22.0),
            16,
            [44203, 65277, 258],
            [34487, 50917, 202],
        ),
        (master(0.0, 7.0, 0.0), 8, [230, 206, 25], [237, 210, 18]),
        (master(0.0, 16.0, 0.0), 8, [190, 27, 220], [200, 9, 237]),
        (master(0.0, 7.0, 0.0), 8, [1, 112, 254], [0, 112, 255]),
        (
            master(0.0, -13.0, 0.0),
            16,
            [65277, 64507, 258],
            [61085, 60407, 4448],
        ),
        (
            master(0.0, -62.0, 0.0),
            16,
            [18762, 5654, 62193],
            [28122, 23102, 44745],
        ),
        (master(7.0, 0.0, 0.0), 8, [254, 225, 1], [252, 254, 1]),
        (master(7.0, 0.0, 0.0), 8, [254, 228, 1], [249, 254, 1]),
        (master(7.0, 0.0, 0.0), 8, [1, 254, 114], [1, 254, 143]),
        (
            master(70.0, 0.0, 0.0),
            16,
            [65277, 22616, 258],
            [32102, 65277, 258],
        ),
        (
            master(-55.0, 0.0, 0.0),
            16,
            [4884, 47801, 7710],
            [41389, 47801, 4884],
        ),
        (master(25.0, 29.0, -22.0), 8, [218, 62, 75], [194, 78, 23]),
        (master(70.0, 44.0, 61.0), 8, [73, 14, 187], [254, 134, 191]),
        (
            master(70.0, 44.0, 61.0),
            8,
            [218, 190, 187],
            [237, 244, 223],
        ),
        (master(-172.0, -62.0, -55.0), 8, [254, 240, 1], [38, 35, 79]),
        (
            master(25.0, 29.0, -22.0),
            16,
            [59367, 48573, 27242],
            [48487, 51301, 16258],
        ),
        (
            master(-55.0, -21.0, 17.0),
            16,
            [258, 52941, 65277],
            [16940, 59733, 28632],
        ),
    ]
}

/// A row's bound in its own codes: 2 8-bit codes (3 on a master set, as
/// take4 recorded) and the same 8-bit-equivalent bound at 16 bits.
fn r0_bound(p: &R0, depth: u32) -> f64 {
    let master_set = [p.h, p.s, p.l].iter().filter(|v| **v != 0.0).count() > 1;
    let codes = if master_set { 3.0 } else { 2.0 };
    if depth == 8 {
        codes
    } else {
        codes * 257.0
    }
}

fn top(depth: u32) -> f64 {
    if depth == 8 {
        255.0
    } else {
        65535.0
    }
}

fn r0_params(p: &R0) -> Value {
    let names = ["reds", "yellows", "greens", "cyans", "blues", "magentas"];
    let mut bands = serde_json::Map::new();
    for (i, b) in p.bands.iter().enumerate() {
        // Symmetric handles only: centre, inner half-width, falloff.
        let [a, bb, c, _] = b.handles;
        let inner = (c - bb).rem_euclid(360.0) / 2.0;
        let falloff = (bb - a).rem_euclid(360.0);
        let center = (bb + inner).rem_euclid(360.0);
        bands.insert(
            names[i].into(),
            json!({"hue": b.h, "saturation": b.s / 100.0, "lightness": b.l / 100.0,
                   "center": center, "inner": inner, "falloff": falloff}),
        );
    }
    json!({"hue": p.h, "saturation": p.s / 100.0, "lightness": p.l / 100.0, "bands": bands})
}

#[test]
fn r0_oracle_reproduces_photoshop_take4() {
    for (p, depth, x, photoshop) in take4_r0_rows() {
        let t = top(depth);
        let want = r0(x.map(|c| f64::from(c) / t), &p);
        let err = (0..3)
            .map(|k| ((want[k] * t).round() - f64::from(photoshop[k])).abs())
            .fold(0.0, f64::max);
        assert!(
            err <= r0_bound(&p, depth),
            "{x:?} at {depth}: R0 {want:?}, Photoshop {photoshop:?}, off by {err} codes"
        );
    }
}

#[test]
fn kernel_reproduces_photoshop_take4_r0_rows() {
    for (p, depth, x, photoshop) in take4_r0_rows() {
        let t = top(depth);
        let got = at16(
            &prepared(&r0_params(&p)),
            x.map(|c| (f64::from(c) / t) as f32),
        );
        let err = (0..3)
            .map(|k| ((f64::from(got[k]) * t).round() - f64::from(photoshop[k])).abs())
            .fold(0.0, f64::max);
        assert!(
            err <= r0_bound(&p, depth),
            "{x:?} at {depth}: kernel {got:?}, Photoshop {photoshop:?}, off by {err} codes"
        );
    }
}

fn default_band(index: usize) -> [f64; 4] {
    let c = 60.0 * index as f64;
    [c - 45.0, c - 15.0, c + 15.0, c + 45.0].map(|d: f64| d.rem_euclid(360.0))
}

fn r0_band(index: usize, h: f64, s: f64, l: f64) -> R0Band {
    R0Band {
        handles: default_band(index),
        h,
        s,
        l,
    }
}

/// The kernel against the R0 oracle over a 16-bit lattice, for master
/// settings and every band stage (hue, netted lightness, the `mgrid`
/// saturation table, its combination with a master gain), within
/// `4e-6·(1 + K)`: the kernel runs in f32, and the saturation stage
/// amplifies its rounding by the gain.
#[test]
fn kernel_is_r0_at_16_bits() {
    let lattice = triples(&codes());
    let mut settings = vec![
        master(20.0, 25.0, -10.0),
        master(-100.0, 0.0, 0.0),
        master(0.0, 100.0, 0.0),
        master(0.0, -100.0, 0.0),
        master(0.0, 44.0, 61.0),
        master(0.0, 0.0, -55.0),
    ];
    let mut banded = |bands: Vec<R0Band>, h: f64, s: f64| {
        settings.push(R0 {
            h,
            s,
            l: 0.0,
            bands,
        })
    };
    banded(vec![r0_band(0, 35.0, 0.0, 0.0)], 0.0, 0.0);
    banded(
        vec![r0_band(0, 0.0, 0.0, -40.0), r0_band(1, 0.0, 0.0, 50.0)],
        0.0,
        0.0,
    );
    banded(vec![r0_band(2, 0.0, 44.0, 0.0)], -40.0, 0.0);
    banded(vec![r0_band(3, 0.0, -37.0, 0.0)], 0.0, 44.0);
    banded(
        (0..6).map(|i| r0_band(i, 0.0, 22.0, 0.0)).collect(),
        0.0,
        0.0,
    );
    banded(vec![r0_band(4, -25.0, 0.0, 35.0)], 0.0, -21.0);
    for p in &settings {
        let op = prepared(&r0_params(p));
        let gain = tid(p.s).min(32767.0) / 256.0 + 1.0;
        for &x in &lattice {
            let got = wide(at16(&op, x));
            let want = r0(wide(x), p);
            let slack = if p.bands.iter().any(|b| b.s != 0.0) {
                128.0
            } else {
                gain
            };
            assert!(
                worst(got, want) <= 4e-6 * (1.0 + slack),
                "R0 on {x:?}: got {got:?}, want {want:?}"
            );
        }
    }
}

#[test]
fn an_identity_setting_returns_its_input_exactly() {
    let op = prepared(&json!({"bands": {"reds": {"center": 10}}}));
    for x in triples(&codes()) {
        assert_eq!(at16(&op, x).map(f32::to_bits), x.map(f32::to_bits));
    }
}
