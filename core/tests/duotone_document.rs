//! #1 Duotone phase 1a (`duotone_ink`, `duotone_document`, through
//! `ffi_duotone`): the options' schema and normalized form, Image ▸ Mode ▸
//! Duotone…'s plan, set and replace (every sample shared; NULL options
//! refuse), the refusals, the Gray profile edit's Duotone arm and the view.
//!
//! The view is checked against an oracle written here, not the crate's: the
//! CIE Lab → XYZ formula (D50), the ICC sRGB D50 colorants, the IEC
//! 61966-2-1 sRGB curve both ways (decode for each ink's coverage through
//! sGray's tone curve, encode for the output) and the Neugebauer sum the
//! status doc declares. The curve is checked against a three-point natural
//! cubic spline worked by hand. Every document is an sGray PNG written here
//! (`common/gray_fixture.rs`) and opened as Grayscale, so its codes are the
//! file's own. Tolerance, fixed before measuring: 32/65535 a channel.

use std::ffi::{c_char, CStr, CString};
use std::path::PathBuf;
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_duotone::*;
use doctored_core::ffi_gray::{
    dr_precision_doc_gray_profile_edit, dr_precision_doc_gray_profile_plan,
    dr_precision_doc_open_gray,
};
use doctored_core::ffi_precision::{dr_precision_doc_free, dr_precision_image_free};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::gray_fixture::{png, srgb_chunk};

pub const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

/// The tolerance, a u16 code a channel.
const TOLERANCE: i32 = 32;

pub fn free(doc: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(doc) };
}

/// A `codes.len()` × 1 sGray document of `depth` (8: `codes` are 8-bit)
/// with `alpha` (same length, file codes) or opaque.
pub fn gray_doc(codes: &[u16], depth: u8, alpha: Option<&[u16]>) -> *mut PrecisionDocument {
    let dir = TempDir::new().unwrap();
    let (colour, channels, samples) = match alpha {
        Some(alpha) => (
            4,
            2,
            codes
                .iter()
                .zip(alpha)
                .flat_map(|(&c, &a)| [c, a])
                .collect::<Vec<_>>(),
        ),
        None => (0, 1, codes.to_vec()),
    };
    let bytes = png(
        codes.len() as u32,
        1,
        depth,
        colour,
        channels,
        &samples,
        &[srgb_chunk()],
    );
    let path: PathBuf = dir.path().join("gray.png");
    std::fs::write(&path, bytes).unwrap();
    let c = cpath(&path);
    let working = CString::new("sgray").unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_gray(
            c.as_ptr(),
            working.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

fn text(value: &Value) -> CString {
    CString::new(value.to_string()).unwrap()
}

fn owned_json(out: *mut c_char, err: *mut c_char) -> Result<Value, String> {
    if out.is_null() {
        return Err(take_err_string(err));
    }
    let value = serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(out) };
    Ok(value)
}

/// `dr_precision_doc_set_duotone`: `Ok(None)` is NULL with no error.
pub fn set(
    doc: *const PrecisionDocument,
    options: &Value,
) -> Result<Option<*mut PrecisionDocument>, String> {
    let options = text(options);
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_set_duotone(doc, options.as_ptr(), &mut err) };
    if !out.is_null() {
        Ok(Some(out))
    } else if err.is_null() {
        Ok(None)
    } else {
        Err(take_err_string(err))
    }
}

/// A Duotone document of `doc` under `options` (must change it).
pub fn duotone(doc: *const PrecisionDocument, options: &Value) -> *mut PrecisionDocument {
    set(doc, options).unwrap().expect("a change")
}

fn plan(doc: *const PrecisionDocument, options: &Value) -> Result<Value, String> {
    let options = text(options);
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_set_duotone_plan(doc, options.as_ptr(), &mut err) };
    owned_json(out, err)
}

/// The document's normalized options; `None` for a document that is not
/// Duotone (NULL with no error).
pub fn options(doc: *const PrecisionDocument) -> Option<Value> {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_duotone_options(doc, &mut err) };
    assert!(err.is_null(), "{}", take_err_string(err));
    (!out.is_null()).then(|| owned_json(out, err).unwrap())
}

fn normalize(options: &Value) -> Result<Value, String> {
    let c = text(options);
    let mut err = ptr::null_mut();
    let out = unsafe { dr_duotone_options_normalize(c.as_ptr(), &mut err) };
    owned_json(out, err)
}

pub fn render16(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

/// The Duotone view of the document's render.
pub fn view16(doc: *const PrecisionDocument) -> Result<Vec<[u16; 4]>, String> {
    let image = Box::into_raw(Box::new(unsafe { &*doc }.render(LIMIT, 0, None).unwrap()));
    let mut err = ptr::null_mut();
    let view =
        unsafe { dr_precision_doc_duotone_view(doc, image, LIMIT, 0, ptr::null(), &mut err) };
    unsafe { dr_precision_image_free(image) };
    if view.is_null() {
        return Err(take_err_string(err));
    }
    let pixels = unsafe { &*view }.to_rgba16().unwrap();
    unsafe { dr_precision_image_free(view) };
    Ok(pixels)
}

pub fn descriptor(doc: *const PrecisionDocument) -> Value {
    unsafe { &*doc }.descriptor().unwrap()
}

/// A straight curve from `low` % at 0 to `high` % at 100, the eleven slots
/// between blank.
pub fn line(low: f64, high: f64) -> Value {
    let mut curve = vec![Value::Null; 13];
    curve[0] = json!(low);
    curve[12] = json!(high);
    Value::Array(curve)
}

/// The Monotone Black options: one ink, Lab (0, 0, 0), curve 0 → 0, 100 → 100.
pub fn monotone_black() -> Value {
    json!({"inks": [{"name": "Black", "lab": [0, 0, 0], "curve": line(0.0, 100.0)}]})
}

/// The test's four inks, in order.
pub fn ink(i: usize) -> Value {
    [
        json!({"name": "Warm Black", "lab": [20, 2, 4], "curve": line(0.0, 100.0)}),
        json!({"name": "Rust", "lab": [55, 45, 55], "curve": line(10.0, 70.0)}),
        json!({"name": "Lime", "lab": [80, -25, 70], "curve": line(0.0, 50.0)}),
        json!({"name": "Teal", "lab": [50, -35, -20], "curve": line(5.0, 90.0)}),
    ][i]
        .clone()
}

// ---- the oracle ----------------------------------------------------------

const D50: [f64; 3] = [0.9642, 1.0, 0.8249];

/// CIE Lab (D50) → XYZ, from the CIE formula.
fn lab_xyz(lab: [f64; 3]) -> [f64; 3] {
    let fy = (lab[0] + 16.0) / 116.0;
    let f = [fy + lab[1] / 500.0, fy, fy - lab[2] / 200.0];
    let epsilon = 216.0 / 24389.0;
    let kappa = 24389.0 / 27.0;
    std::array::from_fn(|i| {
        let t = f[i];
        let v = if t * t * t > epsilon {
            t * t * t
        } else {
            (116.0 * t - 16.0) / kappa
        };
        v * D50[i]
    })
}

/// The ICC sRGB profile's D50 colorants, columns r, g, b.
const SRGB: [[f64; 3]; 3] = [
    [0.436035, 0.385117, 0.143036],
    [0.222485, 0.716888, 0.060608],
    [0.013916, 0.097061, 0.713928],
];

/// XYZ → linear sRGB: the colorant matrix inverted by cofactors.
fn xyz_linear(xyz: [f64; 3]) -> [f64; 3] {
    let m = SRGB;
    let cof = |r: usize, c: usize| {
        let (r1, r2) = ((r + 1) % 3, (r + 2) % 3);
        let (c1, c2) = ((c + 1) % 3, (c + 2) % 3);
        m[r1][c1] * m[r2][c2] - m[r1][c2] * m[r2][c1]
    };
    let det: f64 = (0..3).map(|c| m[0][c] * cof(0, c)).sum();
    // inverse[i][j] = cof(j, i) / det.
    std::array::from_fn(|i| (0..3).map(|j| cof(j, i) / det * xyz[j]).sum())
}

fn srgb_decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn srgb_encode(y: f64) -> f64 {
    if y <= 0.0031308 {
        12.92 * y
    } else {
        1.055 * y.powf(1.0 / 2.4) - 0.055
    }
}

/// One ink of the oracle: its Lab and its straight curve's ends, in percent.
struct OracleInk {
    lab: [f64; 3],
    low: f64,
    high: f64,
}

fn oracle_ink(value: &Value) -> OracleInk {
    let lab = value["lab"].as_array().unwrap();
    let curve = value["curve"].as_array().unwrap();
    OracleInk {
        lab: [0, 1, 2].map(|i| lab[i].as_f64().unwrap()),
        low: curve[0].as_f64().unwrap(),
        high: curve[12].as_f64().unwrap(),
    }
}

/// The declared view of gray code `g` (u16) under `inks`, with `authored`
/// overprints by ink mask: RGB u16 codes.
fn oracle(g: u16, inks: &[OracleInk], authored: &[(usize, [f64; 3])]) -> [u16; 3] {
    let k = 1.0 - f64::from(g) / 65535.0;
    // A two-point natural spline is the straight line between its points.
    let coverage: Vec<f64> = inks
        .iter()
        .map(|ink| {
            let c = (ink.low + (ink.high - ink.low) * k) / 100.0;
            1.0 - srgb_decode(1.0 - c)
        })
        .collect();
    let paper = lab_xyz([100.0, 0.0, 0.0]);
    let colours: Vec<[f64; 3]> = inks.iter().map(|ink| lab_xyz(ink.lab)).collect();
    let mut xyz = [0.0; 3];
    for mask in 0..1usize << inks.len() {
        let primary = match authored.iter().find(|(m, _)| *m == mask) {
            Some((_, lab)) => lab_xyz(*lab),
            None => std::array::from_fn(|c| {
                let mut p = paper[c];
                for (i, colour) in colours.iter().enumerate() {
                    if mask & (1 << i) != 0 {
                        p *= colour[c] / paper[c];
                    }
                }
                p
            }),
        };
        let weight: f64 = coverage
            .iter()
            .enumerate()
            .map(|(i, &c)| if mask & (1 << i) != 0 { c } else { 1.0 - c })
            .product();
        for c in 0..3 {
            xyz[c] += weight * primary[c];
        }
    }
    xyz_linear(xyz).map(|v| (srgb_encode(v.clamp(0.0, 1.0)) * 65535.0).round() as u16)
}

fn assert_near(actual: [u16; 3], expected: [u16; 3], what: &str) {
    for c in 0..3 {
        let d = (i32::from(actual[c]) - i32::from(expected[c])).abs();
        assert!(
            d <= TOLERANCE,
            "{what}: channel {c} is {} against {} (|Δ| {d})",
            actual[c],
            expected[c]
        );
    }
}

// ---- the tests -----------------------------------------------------------

/// Test gray codes (u16 file codes of a 16-bit document).
const CODES: [u16; 6] = [0, 9000, 21000, 33000, 50000, 65535];

#[test]
fn the_view_matches_the_neugebauer_oracle_for_one_to_four_inks() {
    let gray = gray_doc(&CODES, 16, None);
    for n in 1..=4 {
        let inks: Vec<Value> = (0..n).map(ink).collect();
        // The tritone authors its 1+3 overprint; the others take the default.
        let (options, authored) = if n == 3 {
            (
                json!({"inks": inks, "overprints": {"1+3": [30, 10, 20]}}),
                vec![(0b101usize, [30.0, 10.0, 20.0])],
            )
        } else {
            (json!({ "inks": inks }), Vec::new())
        };
        let doc = duotone(gray, &options);
        let oracle_inks: Vec<OracleInk> = inks.iter().map(oracle_ink).collect();
        let view = view16(doc).unwrap();
        let render = render16(gray);
        for (pixel, source) in view.iter().zip(&render) {
            let expected = oracle(source[0], &oracle_inks, &authored);
            assert_near(
                [pixel[0], pixel[1], pixel[2]],
                expected,
                &format!("{n} inks, gray {}", source[0]),
            );
            assert_eq!(pixel[3], source[3], "alpha kept");
        }
        free(doc);
    }
    free(gray);
}

#[test]
fn the_view_keeps_alpha() {
    let gray = gray_doc(&[0, 128, 255], 8, Some(&[255, 100, 0]));
    let doc = duotone(gray, &json!({"inks": [ink(0), ink(1)]}));
    let alphas: Vec<u16> = view16(doc).unwrap().iter().map(|p| p[3]).collect();
    assert_eq!(alphas, vec![65535, 100 * 257, 0]);
    free(doc);
    free(gray);
}

/// Monotone Black reproduces the Grayscale display: every 8-bit code, and
/// at 16 bits every 257th code (off the 8-bit grid) plus both ends.
#[test]
fn monotone_black_is_the_grayscale_display() {
    let eight: Vec<u16> = (0..=255).collect();
    let mut sixteen: Vec<u16> = (0..255).map(|i| 128 + i * 257).collect();
    sixteen.extend([0, 65535]);
    for (codes, depth) in [(eight, 8), (sixteen, 16)] {
        let gray = gray_doc(&codes, depth, None);
        let doc = duotone(gray, &monotone_black());
        let render = render16(gray);
        for (pixel, source) in view16(doc).unwrap().iter().zip(&render) {
            assert_near(
                [pixel[0], pixel[1], pixel[2]],
                [source[0], source[1], source[2]],
                &format!("{depth}-bit Monotone Black at {}", source[0]),
            );
        }
        free(doc);
        free(gray);
    }
}

/// The curve (0 → 0, 50 → 70, 100 → 100): a natural cubic spline through
/// three points, worked by hand. With h = 50 and the secant slopes 1.4 and
/// 0.6, the one interior second derivative solves 2(h + h)·M = 6(0.6 −
/// 1.4), M = −0.024; on each half S = a·y₀ + b·y₁ + (a³ − a)·M₀·h²/6 +
/// (b³ − b)·M₁·h²/6. So S(25) = 35 + 3.75 = 38.75, S(75) = 85 + 3.75 =
/// 88.75 and S(10) = 14 + 1.92 = 15.92.
fn hand_spline(x: f64) -> f64 {
    const M: f64 = -0.024;
    let h2 = 50.0 * 50.0 / 6.0;
    if x <= 50.0 {
        let b = x / 50.0;
        b * 70.0 + (b * b * b - b) * M * h2
    } else {
        let a = (100.0 - x) / 50.0;
        let b = (x - 50.0) / 50.0;
        a * 70.0 + b * 100.0 + (a * a * a - a) * M * h2
    }
}

#[test]
fn the_curve_is_curves_natural_spline_through_the_set_slots() {
    assert!((hand_spline(25.0) - 38.75).abs() < 1e-9);
    assert!((hand_spline(75.0) - 88.75).abs() < 1e-9);
    assert!((hand_spline(10.0) - 15.92).abs() < 1e-9);
    assert!((hand_spline(50.0) - 70.0).abs() < 1e-9);
    let mut curve = vec![Value::Null; 13];
    curve[0] = json!(0);
    curve[6] = json!(70);
    curve[12] = json!(100);
    // Monotone Black: the view's code is 1 − S(x)/100 (the tone curve's
    // decode and encode cancel), x = 100·(1 − g/65535): the slot inputs 0,
    // 10, 50, 90 and 100 % and the between points 25 and 75 %.
    let codes: Vec<u16> = [0.0, 10.0, 25.0, 50.0, 75.0, 90.0, 100.0]
        .map(|x: f64| (65535.0 * (1.0 - x / 100.0)).round() as u16)
        .to_vec();
    let gray = gray_doc(&codes, 16, None);
    let doc = duotone(
        gray,
        &json!({"inks": [{"name": "Black", "lab": [0, 0, 0], "curve": curve}]}),
    );
    for (pixel, source) in view16(doc).unwrap().iter().zip(render16(gray)) {
        let x = 100.0 * (1.0 - f64::from(source[0]) / 65535.0);
        let expected = (65535.0 * (1.0 - hand_spline(x) / 100.0)).round() as u16;
        assert_near(
            [pixel[0], pixel[1], pixel[2]],
            [expected; 3],
            &format!("curve at {x:.2} %"),
        );
    }
    free(doc);
    free(gray);
}

#[test]
fn set_and_replace_share_every_sample_and_null_options_refuse() {
    let gray = gray_doc(&CODES, 16, None);
    let before = render16(gray);
    let d = descriptor(gray);
    assert!(d.get("duotone").is_none());
    assert_eq!(options(gray), None);
    let two = duotone(gray, &json!({"inks": [ink(0), ink(1)]}));
    let d = descriptor(two);
    assert_eq!(d["color_mode"], "gray");
    assert_eq!(d["duotone"], json!({"type": "duotone", "inks": 2}));
    assert_eq!(render16(two), before, "the storage is the Gray document's");
    // The same description changes nothing; another replaces it.
    assert_eq!(set(two, &json!({"inks": [ink(0), ink(1)]})), Ok(None));
    let three = duotone(two, &json!({"inks": [ink(0), ink(1), ink(2)]}));
    assert_eq!(descriptor(three)["duotone"]["type"], "tritone");
    assert_eq!(render16(three), before);
    // NULL options refuse on a live document: Duotone → Grayscale is
    // `dr_precision_doc_convert_duotone_to_gray` (`duotone_close.rs`).
    for doc in [gray, three] {
        let mut err = ptr::null_mut();
        assert!(unsafe { dr_precision_doc_set_duotone(doc, ptr::null(), &mut err) }.is_null());
        assert!(!err.is_null(), "NULL options answer an error");
        unsafe { dr_string_free(err) };
        let mut err = ptr::null_mut();
        assert!(unsafe { dr_precision_doc_set_duotone_plan(doc, ptr::null(), &mut err) }.is_null());
        assert!(!err.is_null(), "NULL options answer an error");
        unsafe { dr_string_free(err) };
    }
    for doc in [two, three, gray] {
        free(doc);
    }
}

#[test]
fn the_plan_answers_bytes_refusal_and_changes() {
    let gray = gray_doc(&CODES, 16, None);
    let options = json!({"inks": [ink(0)]});
    let p = plan(gray, &options).unwrap();
    assert_eq!(
        p["working_memory_plan_bytes"],
        json!(CODES.len() as u64 * 31 + 1024 * 1024)
    );
    assert_eq!(
        (p["refusal"].clone(), p["changes"].clone()),
        (Value::Null, json!(true))
    );
    let doc = duotone(gray, &options);
    assert_eq!(plan(doc, &options).unwrap()["changes"], json!(false));
    assert_eq!(
        plan(doc, &json!({"inks": [ink(1)]})).unwrap()["changes"],
        json!(true)
    );
    assert!(plan(doc, &json!({"inks": []})).is_err());
    free(doc);
    free(gray);
}

#[test]
fn the_options_normalize_with_every_overprint_key() {
    let options = json!({"inks": [ink(0), ink(1), ink(2)], "overprints": {"2+3": [40, 5, 30]}});
    let n = normalize(&options).unwrap();
    assert_eq!(n["type"], "tritone");
    assert_eq!(n["inks"][1]["name"], "Rust");
    assert_eq!(n["inks"][1]["curve"][1], Value::Null);
    let keys: Vec<&String> = n["overprints"].as_object().unwrap().keys().collect();
    let mut expected = vec!["1+2", "1+3", "2+3", "1+2+3"];
    expected.sort();
    assert_eq!(keys, expected);
    assert_eq!(n["overprints"]["1+2"], Value::Null);
    assert_eq!(n["overprints"]["2+3"], json!([40.0, 5.0, 30.0]));
    // The effective colour of an authored overprint is its own Lab; a
    // default one is the inks' product over paper.
    let shown = n["overprint_display"]["2+3"].as_array().unwrap();
    for (v, want) in shown.iter().zip([40.0, 5.0, 30.0]) {
        assert!((v.as_f64().unwrap() - want).abs() < 1e-6);
    }
    let default = n["overprint_display"]["1+2"].as_array().unwrap();
    let product: [f64; 3] = {
        let (a, b, w) = (
            lab_xyz([20.0, 2.0, 4.0]),
            lab_xyz([55.0, 45.0, 55.0]),
            lab_xyz([100.0, 0.0, 0.0]),
        );
        std::array::from_fn(|c| a[c] * b[c] / w[c])
    };
    // CIE L* of the product's Y (its linear branch below ε).
    let y = product[1] / D50[1];
    let l = if y > 216.0 / 24389.0 {
        116.0 * y.cbrt() - 16.0
    } else {
        24389.0 / 27.0 * y
    };
    assert!((default[0].as_f64().unwrap() - l).abs() < 1e-6);
    let four = normalize(&json!({"inks": [ink(0), ink(1), ink(2), ink(3)]})).unwrap();
    assert_eq!(four["overprints"].as_object().unwrap().len(), 11);
    assert_eq!(four["type"], "quadtone");
}

#[test]
fn malformed_options_refuse_by_name() {
    let cases = [
        (
            json!({"inks": [ink(0), ink(1), ink(2), ink(3), ink(0)]}),
            "A Duotone holds 1 to 4 inks (Monotone, Duotone, Tritone or Quadtone), not 5.",
        ),
        (
            json!({"inks": [ink(0)], "type": "monotone"}),
            "duotone options have an unknown key \"type\"",
        ),
        (
            json!({"inks": [{"name": "", "lab": [0, 0, 0], "curve": line(0.0, 100.0)}]}),
            "ink 1's name must be 1 to 64 characters",
        ),
        (
            json!({"inks": [{"name": "A", "lab": [101, 0, 0], "curve": line(0.0, 100.0)}]}),
            "ink 1's lab must be [L, a, b] with L from 0 to 100 and a and b from -128 to 127",
        ),
        (
            json!({"inks": [ink(0), {"name": "B", "lab": [0, 0, 0], "curve": [0, null, null, null, null, null, null, null, null, null, null, null, null]}]}),
            "ink 2's curve must set its 0 % and 100 % slots",
        ),
        (
            json!({"inks": [ink(0), ink(1)], "overprints": {"1+3": [0, 0, 0]}}),
            "overprints key \"1+3\" is not a combination of two or more of the 2 inks (\"1+2\", …)",
        ),
    ];
    for (options, sentence) in cases {
        assert_eq!(normalize(&options), Err(sentence.to_string()));
    }
}

#[test]
fn only_an_8_or_16_bit_grayscale_document_converts() {
    let pixels = vec![[1000u16, 20000, 40000, 65535]; 4];
    let image = PrecisionImage::from_rgba16(2, 2, pixels, IccProfile::srgb(), None).unwrap();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ));
    let options = json!({"inks": [ink(0)]});
    let source = "Duotone converts an 8- or 16-bit Grayscale document; convert to Grayscale first.";
    assert_eq!(set(rgb, &options), Err(source.to_string()));
    assert_eq!(plan(rgb, &options).unwrap()["refusal"], json!(source));
    let float = crate::gray_ops::gray_doc(2, 2, 32);
    assert_eq!(
        set(float, &options),
        Err("Duotone converts an 8- or 16-bit Grayscale document; convert to 8 or 16 Bits/Channel first.".to_string())
    );
    let gray = gray_doc(&CODES, 16, None);
    assert_eq!(
        view16(gray),
        Err("the Duotone view takes a Duotone document".to_string())
    );
    free(rgb);
    free(float);
    free(gray);
}

/// `dr_precision_doc_gray_profile_plan` / `_edit` on a Duotone document.
pub fn profile_edit(
    doc: *const PrecisionDocument,
    request: &Value,
) -> (Value, Result<Option<*mut PrecisionDocument>, String>) {
    let request = text(request);
    let mut err = ptr::null_mut();
    let p = owned_json(
        unsafe {
            dr_precision_doc_gray_profile_plan(doc, ptr::null(), 0, request.as_ptr(), &mut err)
        },
        err,
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_gray_profile_edit(
            doc,
            ptr::null(),
            0,
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    let edit = if !out.is_null() {
        Ok(Some(out))
    } else if err.is_null() {
        Ok(None)
    } else {
        Err(take_err_string(err))
    };
    (p, edit)
}

#[test]
fn the_gray_profile_edit_refuses_32_bits_and_keeps_inks_through_depth() {
    let gray = gray_doc(&CODES, 16, None);
    let doc = duotone(gray, &json!({"inks": [ink(0), ink(1)]}));
    let sentence = "32 Bits/Channel is unavailable in a Duotone document: the 32-bit modes are RGB and Grayscale.";
    let (p, edit) = profile_edit(doc, &json!({"op": "depth", "depth": 32}));
    assert_eq!(p["refusal"], json!(sentence));
    assert_eq!(edit, Err(sentence.to_string()));
    let (p, edit) = profile_edit(doc, &json!({"op": "depth", "depth": 8}));
    assert_eq!(p["refusal"], Value::Null);
    let eight = edit.unwrap().expect("16 → 8 changes the depth");
    assert_eq!(descriptor(eight)["gray"]["depth"], 8);
    assert_eq!(options(eight), options(doc));
    free(eight);
    free(doc);
    free(gray);
}

#[test]
fn every_duotone_export_tolerates_null() {
    unsafe {
        let mut err = ptr::null_mut();
        assert!(dr_duotone_options_normalize(ptr::null(), &mut err).is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(dr_duotone_options_normalize(ptr::null(), ptr::null_mut()).is_null());
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_duotone_options(ptr::null(), &mut err).is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(dr_precision_doc_duotone_options(ptr::null(), ptr::null_mut()).is_null());
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_set_duotone_plan(ptr::null(), ptr::null(), &mut err).is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(
            dr_precision_doc_set_duotone_plan(ptr::null(), ptr::null(), ptr::null_mut()).is_null()
        );
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_set_duotone(ptr::null(), ptr::null(), &mut err).is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(dr_precision_doc_set_duotone(ptr::null(), ptr::null(), ptr::null_mut()).is_null());
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_duotone_view(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(dr_precision_doc_duotone_view(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        // A live document with a NULL image refuses too.
        let gray = gray_doc(&CODES, 16, None);
        let doc = duotone(gray, &monotone_black());
        let mut err = ptr::null_mut();
        assert!(
            dr_precision_doc_duotone_view(doc, ptr::null(), LIMIT, 0, ptr::null(), &mut err)
                .is_null()
        );
        assert!(!err.is_null());
        dr_string_free(err);
        free(doc);
        free(gray);
    }
}
