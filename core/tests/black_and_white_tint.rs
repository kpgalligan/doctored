//! Black & White with Tint at 16 and 32 bits, as Photoshop
//! 26.10.0 renders it — the tint colour's NUMBERS (over 255) set to the
//! computed grey's luminosity with ClipColor, measured by the float_adjust
//! take6 (`scripts/photoshop-probe/float_adjust_take6.py`, receipt
//! `doctored-fixtures/photoshop/float-adjust-473/take6`, `judged.json`).
//!
//! The oracle is written here in binary64 from the runner's docstring
//! (THE PIPELINE), never read off the implementation:
//!
//! * the grey `g` is the six-weight decomposition; `T = numbers / 255`;
//!   `c = T + (G − lum T)` with lum's .3/.59/.11; `l = G`, `n = min c` and
//!   `x = max c` taken once; Low (`n < 0`): `c_k = l + (c_k − l)·l/(l − n)`;
//!   High at `H` (`x > H` and `x > l`): `c_k = l + (c_k − l)·(H − l)/(x − l)`.
//! * 8 and 16 bits (`setlum-clip16`, pinned at 0.14 tolerance units): `G =
//!   clamp(g, 0, 1)`, `H = 1`, plus `1/32768` on every component when Low
//!   fired over `G > 0` (`e15`, pinned exactly), no floor for a High-only
//!   clip (`hi-e0`, shipped under the near-match rule: Photoshop's minimum
//!   lands up to 2 of its 15-bit codes away), the result clamped to [0, 1].
//!   At 8 bits this form misses Photoshop's own integer path by up to 3
//!   codes (take6 BT8), so a byte document runs that path instead (#578
//!   phase 4, h8a-LE, oracle `common/bw_tint8_reference.rs`, fitted and
//!   confirmed by `scripts/photoshop-probe/eight_bit_578_take.py`):
//!   `eight_bit_tint_is_the_integer_path` holds it exactly.
//! * 32 bits (`clip/e510/num`, pinned at 0.003 units): `G = min(max(0, g),
//!   16)`, `H = 16`, plus `1/510` on every component when either branch
//!   fired. The cap at 16 is take6's fit (its ladder reads every grey at or
//!   above 16 as 16 + 1/510 under a coloured tint, 16 under a neutral one);
//!   take7, the fresh take meant to confirm it, stalled in Photoshop.
//!
//! `the_oracle_reproduces_take6` pins the oracle to Photoshop's own numbers
//! within the take's tolerances; the render tests then hold the 16-bit
//! document within one code and the 32-bit document within
//! `1e-5·max(1, |want|)` of the oracle.

use doctored_core::ffi_adjust::dr_image_adjust_op;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::bw_tint8_reference::h8a_le;
use crate::common::{free, open_pattern, pixels, take_err_string};
use crate::float_adjustments::{add, float_row, render};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// Take6's sets: (weights in wheel order, the tint's numbers).
pub(crate) type Set = ([f64; 6], [u8; 3]);
/// A take6 pin: the stored triple, its set, Photoshop's reading, a label.
type Pin<X, Y> = (X, Set, Y, &'static str);

const DEFAULT: [f64; 6] = [0.4, 0.6, 0.4, 0.6, 0.2, 0.8];
const B3_WEIGHTS: [f64; 6] = [1.2, 0.6, 0.4, 0.6, -0.5, 0.8];
const B1: Set = (DEFAULT, [153, 138, 102]);
const B2: Set = (DEFAULT, [40, 120, 220]);
const B3: Set = (B3_WEIGHTS, [200, 60, 90]);
const B5: Set = (DEFAULT, [128, 128, 128]);

const FLOOR_16: f64 = 1.0 / 32768.0;
const FLOOR_32: f64 = 1.0 / 510.0;
const CAP_32: f64 = 16.0;

// --------------------------------------------------------------- oracle --

/// The six-weight grey, before any clamp: the minimum, plus `(max −
/// mid)·W[primary]` and `(mid − min)·W[pair]` on the wheel (reds, yellows,
/// greens, cyans, blues, magentas).
fn gray(weights: [f64; 6], rgb: [f64; 3]) -> f64 {
    let mut order = [0usize, 1, 2];
    order.sort_by(|&a, &b| rgb[b].total_cmp(&rgb[a]).then(a.cmp(&b)));
    let [hi, mid, lo] = order;
    let pair = match (hi.min(mid), hi.max(mid)) {
        (0, 1) => 1,
        (1, 2) => 3,
        _ => 5,
    };
    rgb[lo] + (rgb[hi] - rgb[mid]) * weights[2 * hi] + (rgb[mid] - rgb[lo]) * weights[pair]
}

fn lum(c: [f64; 3]) -> f64 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

/// The pipeline's SetLum and ClipColor: (c, low fired, high fired).
fn set_lum_clip(numbers: [u8; 3], big: f64, ceiling: f64) -> ([f64; 3], bool, bool) {
    let t = numbers.map(|n| f64::from(n) / 255.0);
    let d = big - lum(t);
    let mut c = t.map(|k| k + d);
    let (l, n, x) = (big, c[0].min(c[1]).min(c[2]), c[0].max(c[1]).max(c[2]));
    let low = n < 0.0;
    let high = x > ceiling && x > l;
    if low {
        c = c.map(|k| l + (k - l) * l / (l - n));
    }
    if high {
        c = c.map(|k| l + (k - l) * (ceiling - l) / (x - l));
    }
    (c, low, high)
}

/// The 8/16-bit form on a stored triple in [0, 1].
pub(crate) fn tint16(set: Set, rgb: [f64; 3]) -> [f64; 3] {
    let big = gray(set.0, rgb).clamp(0.0, 1.0);
    let (c, low, _) = set_lum_clip(set.1, big, 1.0);
    let floor = if low && big > 0.0 { FLOOR_16 } else { 0.0 };
    c.map(|k| (k + floor).clamp(0.0, 1.0))
}

/// The 32-bit form on a stored linear triple.
pub(crate) fn tint32(set: Set, rgb: [f64; 3]) -> [f64; 3] {
    let big = gray(set.0, rgb).clamp(0.0, CAP_32);
    let (c, low, high) = set_lum_clip(set.1, big, CAP_32);
    if low || high {
        c.map(|k| k + FLOOR_32)
    } else {
        c
    }
}

/// The `black_and_white` params of a set.
pub(crate) fn params(set: Set) -> Value {
    let [r, g, b] = set.1;
    let w = set.0;
    json!({"reds": w[0], "yellows": w[1], "greens": w[2], "cyans": w[3], "blues": w[4],
        "magentas": w[5], "tint": true, "tint_color": format!("#{r:02x}{g:02x}{b:02x}")})
}

fn code16(v: f64) -> i64 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as i64
}

/// Photoshop's 15-bit code of a 16-bit sample, as the take reads it.
fn code15(u16: i64) -> i64 {
    (u16 as f64 * 32768.0 / 65535.0).round() as i64
}

// ---------------------------------------------------------- take6's pins --

/// 32-bit pins (`judged.json` readings of srgb32 / ladder32, sRGB): the
/// stored triple, its set, Photoshop's read-back.
fn pins32() -> Vec<Pin<[f32; 3], [f64; 3]>> {
    vec![
        (
            [100.0; 3],
            B1,
            [16.00196075439453; 3],
            "B1 L100: capped, high",
        ),
        ([20.0; 3], B5, [16.0; 3], "B5 L20: capped, neutral tint"),
        (
            [15.9; 3],
            B2,
            [15.842668533325195, 15.91346549987793, 16.00196075439453],
            "B2 L15.9: high",
        ),
        (
            [12.0, 20.0, 30.0],
            B3,
            [12.171372413635254, 11.622352600097656, 11.739999771118164],
            "B3 Lc1: unfired above 4.5",
        ),
        (
            [0.02, 0.01, 0.3],
            B2,
            [
                0.001960782101377845,
                0.09031900018453598,
                0.20076677203178406,
            ],
            "B2 c5: low",
        ),
        (
            [0.05, 0.2, 0.6],
            B3,
            [0.0019607844296842813; 3],
            "B3 c1: grey below 0",
        ),
        (
            [1.0; 3],
            B1,
            [1.0567059516906738, 0.9978823661804199, 0.856705904006958],
            "B1 n1.0: unfired above 1",
        ),
    ]
}

/// 16-bit pins (srgb16, codes): the stored codes, the set, Photoshop's codes.
fn pins16() -> Vec<Pin<[u16; 3], [u16; 3]>> {
    vec![
        ([32768; 3], B1, [36483, 32628, 23376], "B1 n0.5: unfired"),
        ([1311, 655, 19661], B1, [6768, 4780, 2], "B1 c5: low"),
        ([3277, 13107, 39321], B2, [2, 17214, 38731], "B2 c1: low"),
        (
            [32768, 13107, 6554],
            B2,
            [1130, 21690, 47389],
            "B2 c0: unfired",
        ),
        (
            [62565, 22884, 52280],
            B2,
            [41599, 52237, 65535],
            "B2 c2:trc: high",
        ),
        ([66; 3], B1, [92, 66, 2], "B1 n0.001: low, neutral"),
        ([6554; 3], B2, [2, 7826, 17608], "B2 n0.1: low, neutral"),
        (
            [62470; 3],
            B2,
            [60653, 62821, 65535],
            "B2 n0.9:gamma22: high, neutral",
        ),
        (
            [45874; 3],
            B2,
            [34219, 48135, 65535],
            "B2 n0.7: high, neutral",
        ),
        (
            [58981; 3],
            B3,
            [65535, 55847, 57925],
            "B3 n0.9: high, neutral",
        ),
    ]
}

/// 8-bit pins (srgb8, codes): the stored codes, the set, Photoshop's codes.
fn pins8() -> Vec<Pin<[u8; 3], [u8; 3]>> {
    vec![
        ([13, 51, 153], B1, [71, 56, 20], "B1 c1"),
        ([243, 89, 203], B1, [211, 196, 160], "B1 c2:trc"),
        ([128; 3], B3, [223, 83, 113], "B3 n0.5"),
        ([5, 3, 77], B3, [0, 0, 0], "B3 c5: grey below 0"),
        ([63; 3], B2, [1, 76, 172], "B2 n0.05:trc: low, 3 codes"),
        ([149; 3], B2, [89, 162, 255], "B2 n0.3:trc: high, 3 codes"),
        ([128; 3], B1, [143, 128, 92], "B1 n0.5"),
    ]
}

/// The near-match residuals of the 8/16-bit SetLum form (take6): 3 codes
/// at 8 bits (BT8 `setlum-clip8`; a byte document now runs h8a-LE
/// instead), 2 of Photoshop's 15-bit codes on a
/// High-only clip's minimum at 16 bits (BT16-floor-hi `hi-e0`).
const RESIDUAL_8: i64 = 3;
const RESIDUAL_15: i64 = 2;

/// The 16-bit pin, on the clamped prediction: take6 judged `setlum-clip16`
/// at 0.14 units of take3's `T_DOM` (2e-3), so the oracle holds Photoshop's
/// codes within 0.15 of a unit, not the whole unit the take allowed.
const PIN_16: f64 = 0.15 * 2e-3;

// ---------------------------------------------------------------- tests --

#[test]
fn the_oracle_reproduces_take6() {
    for (x, set, want, what) in pins32() {
        let got = tint32(set, x.map(f64::from));
        for c in 0..3 {
            assert!(
                (got[c] - want[c]).abs() <= 5e-5 * want[c].abs().max(1.0),
                "{what}: oracle {got:?}, Photoshop {want:?}"
            );
        }
    }
    for (x, set, want, what) in pins16() {
        let got = tint16(set, x.map(|v| f64::from(v) / 65535.0));
        for c in 0..3 {
            let ps = f64::from(want[c]) / 65535.0;
            assert!(
                (got[c] - ps).abs() <= PIN_16,
                "{what}: oracle {got:?}, Photoshop {want:?}"
            );
        }
    }
    for (x, set, want, what) in pins8() {
        let got = tint16(set, x.map(|v| f64::from(v) / 255.0));
        for c in 0..3 {
            let ours = (got[c] * 255.0).round() as i64;
            assert!(
                (ours - i64::from(want[c])).abs() <= RESIDUAL_8,
                "{what}: oracle {got:?}, Photoshop {want:?}"
            );
        }
    }
}

/// One 16-bit sRGB document holding `codes` as a row, under `set`'s tinted
/// Black & White layer, rendered to 16-bit codes.
fn render16(set: Set, codes: &[[u16; 3]]) -> Vec<[u16; 4]> {
    let srgb = IccProfile::srgb();
    let pixels: Vec<[u16; 4]> = codes.iter().map(|c| [c[0], c[1], c[2], 65535]).collect();
    let image =
        PrecisionImage::from_rgba16(codes.len() as u32, 1, pixels, srgb.clone(), None).unwrap();
    let photo = Layer::raster("Photo", image, (0, 0), 1.0, true).unwrap();
    let doc = PrecisionDocument::from_parts(
        codes.len() as u32,
        1,
        srgb,
        Metadata::default(),
        Resolution::default(),
        vec![photo],
    )
    .unwrap();
    let description = json!({"type": "adjust", "evaluation": "precision_v1",
        "op": "black_and_white", "params": params(set)})
    .to_string();
    doc.add_adjustment("Tint", &description, 1.0, LIMIT, 0, None)
        .unwrap()
        .unwrap()
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

/// At 16 bits the render is the oracle within one code on every take6 pin,
/// a Low clip's minimum is take6's exact code (the `1/32768` floor) and a
/// High-only clip's minimum on a NEUTRAL input (whose grey is the stored
/// value, so no grey error enters) is within the shipped 2 15-bit codes.
#[test]
fn sixteen_bits_render_the_oracle_and_take6s_floors() {
    for (x, set, want, what) in pins16() {
        let out = render16(set, &[x]);
        let oracle = tint16(set, x.map(|v| f64::from(v) / 65535.0));
        for c in 0..3 {
            assert!(
                (i64::from(out[0][c]) - code16(oracle[c])).abs() <= 1,
                "{what}: render {:?}, oracle {oracle:?}",
                out[0]
            );
        }
        let min = |p: [u16; 3]| i64::from(p[0].min(p[1]).min(p[2]));
        let ours = min([out[0][0], out[0][1], out[0][2]]);
        if what.contains("low") {
            assert!(
                (ours - min(want)).abs() <= 1,
                "{what}: min {ours} vs {want:?}"
            );
        }
        if what.contains("high, neutral") {
            assert!(
                (code15(ours) - code15(min(want))).abs() <= RESIDUAL_15,
                "{what}: min {ours} vs {want:?}"
            );
        }
    }
}

/// At 32 bits the render is the oracle within `1e-5·max(1, |want|)` over
/// 0, negatives, greys at and above 16 and both ClipColor branches, and
/// Photoshop's own take6 numbers within the take's `5e-5`.
#[test]
fn thirty_two_bits_render_the_oracle_at_every_value() {
    let srgb = IccProfile::srgb();
    let patches: Vec<[f32; 3]> = vec![
        [0.0; 3],
        [-0.25, 0.5, 1.0],
        [-0.05; 3],
        [0.02, 0.01, 0.3],
        [0.05, 0.2, 0.6],
        [0.5; 3],
        [1.0; 3],
        [4.0, 0.3, 2.0],
        [12.0, 20.0, 30.0],
        [15.9; 3],
        [16.0; 3],
        [20.0; 3],
        [100.0; 3],
        [8.0, 6.0, 4.0],
    ];
    for set in [B1, B2, B3, B5] {
        let out = render(&add(
            &float_row(srgb.bytes(), &patches),
            "black_and_white",
            &params(set),
            1.0,
        ));
        for (x, got) in patches.iter().zip(&out) {
            let want = tint32(set, x.map(f64::from));
            for c in 0..3 {
                assert!(
                    (f64::from(got[c]) - want[c]).abs() <= 1e-5 * want[c].abs().max(1.0),
                    "{:?} on {x:?}: render {got:?}, oracle {want:?}",
                    set.1
                );
            }
            assert_eq!(got[3], 1.0);
        }
    }
    for (x, set, want, what) in pins32() {
        let got = render(&add(
            &float_row(srgb.bytes(), &[x]),
            "black_and_white",
            &params(set),
            1.0,
        ))[0];
        for c in 0..3 {
            assert!(
                (f64::from(got[c]) - want[c]).abs() <= 5e-5 * want[c].abs().max(1.0),
                "{what}: render {got:?}, Photoshop {want:?}"
            );
        }
    }
}

/// Whole percents of a set's weights, as the 8-bit Tint reads them.
fn percents(weights: [f64; 6]) -> [i32; 6] {
    weights.map(|w| (w * 100.0).round() as i32)
}

/// #578 phase 4: a byte document's Tint is the integer path (h8a-LE,
/// measured against Photoshop), exactly. The byte op on float_adjust_take7's 8-bit stimulus
/// (its `N8` greys and `C8` colours, restated) under its sets C1-C5 equals
/// the oracle on every code, and so do take6's own 8-bit Photoshop pins.
#[test]
fn eight_bit_tint_is_the_integer_path() {
    let mut greys: Vec<u8> = (0..16).collect();
    greys.extend((17..252).step_by(6));
    greys.extend([253, 254, 255]);
    let mut codes: Vec<[u8; 3]> = greys.iter().map(|&v| [v; 3]).collect();
    codes.extend([
        [200, 40, 10],
        [30, 160, 220],
        [90, 90, 30],
        [250, 245, 5],
        [5, 5, 250],
        [128, 0, 255],
        [60, 120, 61],
        [12, 3, 7],
        [240, 250, 235],
        [180, 60, 140],
        [77, 33, 201],
        [255, 128, 0],
        [0, 255, 128],
        [99, 101, 100],
        [2, 9, 4],
        [222, 199, 248],
        [16, 48, 32],
        [140, 141, 255],
        [255, 0, 0],
        [0, 0, 255],
    ]);
    let sets: [Set; 5] = [
        (DEFAULT, [230, 121, 40]),
        ([0.7, 0.2, 0.9, 0.3, 0.5, 0.1], [70, 200, 151]),
        (DEFAULT, [90, 41, 250]),
        ([-0.2, 1.1, 0.6, 1.5, 1.0, 0.4], [255, 255, 0]),
        (DEFAULT, [64, 64, 64]),
    ];
    let dir = tempfile::tempdir().unwrap();
    let src = RgbaImage::from_fn(codes.len() as u32, 1, |x, _| {
        let c = codes[x as usize];
        Rgba([c[0], c[1], c[2], 255])
    });
    let img = open_pattern(&dir, "tint8.png", &src);
    let op = std::ffi::CString::new("black_and_white").unwrap();
    let run = |set: Set| -> Vec<u8> {
        let text = std::ffi::CString::new(params(set).to_string()).unwrap();
        let mut err = std::ptr::null_mut();
        let out = unsafe { dr_image_adjust_op(img, op.as_ptr(), text.as_ptr(), &mut err) };
        assert!(!out.is_null(), "black_and_white: {}", take_err_string(err));
        let bytes = pixels(out);
        free(out);
        bytes
    };
    for (n, set) in sets.into_iter().enumerate() {
        let got = run(set);
        for (i, c) in codes.iter().enumerate() {
            let want = h8a_le(*c, percents(set.0), set.1);
            assert_eq!(got[i * 4..i * 4 + 3], want, "C{} {c:?}", n + 1);
            assert_eq!(got[i * 4 + 3], 255, "alpha");
        }
    }
    free(img);
    for (x, set, photoshop, what) in pins8() {
        assert_eq!(h8a_le(x, percents(set.0), set.1), photoshop, "{what}");
    }
}
