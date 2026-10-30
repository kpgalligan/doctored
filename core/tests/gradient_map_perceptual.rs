//! The Gradient Map's `method: "perceptual"` at 8, 16 and 32
//! bits through the FFI, against an oracle written here in f64 from Björn
//! Ottosson's published Oklab matrices and `gradient_map_678_take.py`'s
//! PERC-OK, never read off the core: each stop's sRGB colour is decoded
//! (IEC 61966-2.1) and taken to Oklab; between stops `i` and `i + 1` at the
//! segment fraction `f`, remapped through stop `i`'s midpoint, the Oklab
//! value is `(1 − k)·L + k·C` with `L` the linear interpolation and `C` the
//! uniform Catmull-Rom Hermite clamped per channel to the two bracketing
//! stops; it goes back to linear sRGB and is encoded. The opacity is linear
//! through stop `i`'s opacity midpoint and blends the original with the
//! mapped colour. A CMYK document refuses the method by name.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba};
use doctored_core::ffi_adjust::dr_image_adjust_op;
use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_color::{
    dr_precision_doc_add_adjustment, dr_precision_doc_apply_adjustment,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};

use crate::cmyk_fixture::press_profile;
use crate::common::take_err_string;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// One oracle stop: position, sRGB colour in 0..1, opacity, colour midpoint,
/// opacity midpoint.
type Stop = (f64, [f64; 3], f64, f64, f64);

// Ottosson (2020), "A perceptual color space for image processing": M1, M2
// and his published inverses.
const M1: [[f64; 3]; 3] = [
    [0.412_221_470_8, 0.536_332_536_3, 0.051_445_992_9],
    [0.211_903_498_2, 0.680_699_545_1, 0.107_396_956_6],
    [0.088_302_461_9, 0.281_718_837_6, 0.629_978_700_5],
];
const M2: [[f64; 3]; 3] = [
    [0.210_454_255_3, 0.793_617_785_0, -0.004_072_046_8],
    [1.977_998_495_1, -2.428_592_205_0, 0.450_593_709_9],
    [0.025_904_037_1, 0.782_771_766_2, -0.808_675_766_0],
];
const M2_INV: [[f64; 3]; 3] = [
    [1.0, 0.396_337_777_4, 0.215_803_757_3],
    [1.0, -0.105_561_345_8, -0.063_854_172_8],
    [1.0, -0.089_484_177_5, -1.291_485_548_0],
];
const M1_INV: [[f64; 3]; 3] = [
    [4.076_741_662_1, -3.307_711_591_3, 0.230_969_929_2],
    [-1.268_438_004_6, 2.609_757_401_1, -0.341_319_396_5],
    [-0.004_196_086_3, -0.703_418_614_7, 1.707_614_701_0],
];

fn mul(m: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|r| m[r][0] * v[0] + m[r][1] * v[1] + m[r][2] * v[2])
}

/// The sRGB encode and decode, written out from IEC 61966-2.1.
fn encode(v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn decode(e: f64) -> f64 {
    if e <= 0.04045 {
        e / 12.92
    } else {
        ((e + 0.055) / 1.055).powf(2.4)
    }
}

fn to_oklab(srgb: [f64; 3]) -> [f64; 3] {
    mul(&M2, mul(&M1, srgb.map(decode)).map(f64::cbrt))
}

fn from_oklab(lab: [f64; 3]) -> [f64; 3] {
    mul(&M1_INV, mul(&M2_INV, lab).map(|v| v * v * v)).map(encode)
}

/// `pwl.pre`.
fn remap(f: f64, m: f64) -> f64 {
    if m == 0.5 {
        f
    } else if f <= m {
        f / (2.0 * m)
    } else {
        0.5 + (f - m) / (2.0 * (1.0 - m))
    }
}

/// The declared law (module doc) over `values` (one per stop, in the law's
/// space): (value, opacity) at `t`.
fn law(stops: &[Stop], values: &[[f64; 3]], t: f64, k: f64) -> ([f64; 3], f64) {
    let n = stops.len();
    if t <= stops[0].0 {
        return (values[0], stops[0].2);
    }
    if t >= stops[n - 1].0 {
        return (values[n - 1], stops[n - 1].2);
    }
    let i = (0..n - 1)
        .find(|&i| t >= stops[i].0 && t < stops[i + 1].0)
        .unwrap();
    let f0 = (t - stops[i].0) / (stops[i + 1].0 - stops[i].0);
    let f = remap(f0, stops[i].3);
    let fo = remap(f0, stops[i].4);
    let (a, b) = (values[i], values[i + 1]);
    let before = if i > 0 { values[i - 1] } else { a };
    let after = if i + 2 < n { values[i + 2] } else { b };
    let (f2, f3) = (f * f, f * f * f);
    let value = std::array::from_fn(|c| {
        let cr = (2.0 * f3 - 3.0 * f2 + 1.0) * a[c]
            + (f3 - 2.0 * f2 + f) * (b[c] - before[c]) / 2.0
            + (-2.0 * f3 + 3.0 * f2) * b[c]
            + (f3 - f2) * (after[c] - a[c]) / 2.0;
        let cr = cr.clamp(a[c].min(b[c]), a[c].max(b[c]));
        (1.0 - k) * (a[c] + (b[c] - a[c]) * f) + k * cr
    });
    (value, stops[i].2 + (stops[i + 1].2 - stops[i].2) * fo)
}

/// The Perceptual oracle: (sRGB colour, opacity) at `t`.
fn perceptual(stops: &[Stop], t: f64, k: f64) -> ([f64; 3], f64) {
    let lab: Vec<[f64; 3]> = stops.iter().map(|s| to_oklab(s.1)).collect();
    let (value, opacity) = law(stops, &lab, t, k);
    (from_oklab(value), opacity)
}

/// The Classic oracle (the same law over the sRGB numbers), only to show the
/// scene separates the two methods.
fn classic(stops: &[Stop], t: f64, k: f64) -> [f64; 3] {
    let srgb: Vec<[f64; 3]> = stops.iter().map(|s| s.1).collect();
    law(stops, &srgb, t, k).0
}

/// Four stops with midpoints off 50 % and a half-opaque third stop with an
/// opacity midpoint.
fn n4(opaque: bool) -> Vec<Stop> {
    let c = |r: u8, g: u8, b: u8| [r, g, b].map(|v| f64::from(v) / 255.0);
    vec![
        (0.0, c(40, 200, 220), 1.0, 0.7, 0.5),
        (0.3, c(120, 80, 30), 1.0, 0.2, 0.3),
        (
            0.6,
            c(60, 150, 90),
            if opaque { 1.0 } else { 0.5 },
            0.6,
            0.5,
        ),
        (1.0, c(250, 20, 60), 1.0, 0.5, 0.5),
    ]
}

fn hex(c: [f64; 3]) -> String {
    let v = c.map(|x| (x * 255.0).round() as u8);
    format!("#{:02x}{:02x}{:02x}", v[0], v[1], v[2])
}

/// The Perceptual `gradient_map` params of `stops`.
fn params(stops: &[Stop], k: f64, reverse: bool) -> Value {
    let list: Vec<Value> = stops
        .iter()
        .map(|s| {
            json!({"position": s.0, "color": hex(s.1), "opacity": s.2, "midpoint": s.3,
                   "opacity_midpoint": s.4})
        })
        .collect();
    json!({"method": "perceptual", "dither": false,
           "gradient": {"stops": list, "smoothness": k, "reverse": reverse}})
}

/// What the map does to the gray `g` (unit): `t` is `g`, or `1 − g` reversed.
fn mapped_gray(stops: &[Stop], g: f64, k: f64, reverse: bool) -> [f64; 3] {
    let t = if reverse { 1.0 - g } else { g };
    let (color, opacity) = perceptual(stops, t, k);
    color.map(|c| g + (c - g) * opacity.clamp(0.0, 1.0))
}

/// `params` over the 8-bit ramp (gray x at column x) by `dr_image_adjust_op`.
fn map8(params: &Value) -> Result<Vec<u8>, String> {
    let rgba: Vec<u8> = (0..=255u8).flat_map(|g| [g, g, g, 255]).collect();
    let op = CString::new("gradient_map").unwrap();
    let text = CString::new(params.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    unsafe {
        let img = dr_image_from_rgba(rgba.as_ptr(), 256, 1);
        let out = dr_image_adjust_op(img, op.as_ptr(), text.as_ptr(), &mut err);
        dr_image_free(img);
        if out.is_null() {
            return Err(take_err_string(err));
        }
        let px = std::slice::from_raw_parts(dr_image_pixels_rgba(out), 256 * 4).to_vec();
        dr_image_free(out);
        Ok(px)
    }
}

fn description(params: &Value) -> String {
    json!({"type": "adjust", "evaluation": "precision_v1", "op": "gradient_map",
           "params": params})
    .to_string()
}

/// The 16-bit ramp, 1024 grays round(65535 x / 1023), mapped as an
/// adjustment layer by `dr_precision_doc_add_adjustment`.
fn map16(params: &Value) -> Vec<[u16; 4]> {
    let pixels: Vec<[u16; 4]> = (0..1024u32)
        .map(|x| {
            let g = (65535.0 * f64::from(x) / 1023.0 + 0.5).floor() as u16;
            [g, g, g, 65535]
        })
        .collect();
    let image = PrecisionImage::from_rgba16(1024, 1, pixels, IccProfile::srgb(), None).unwrap();
    let doc = PrecisionDocument::from_parts(
        1024,
        1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![PrecisionDocumentLayer::raster("L", image, (0, 0), 1.0, true).unwrap()],
    )
    .unwrap();
    let text = CString::new(description(params)).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment(
            &doc,
            c"Gradient Map".as_ptr(),
            text.as_ptr(),
            1.0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    let layered = *unsafe { Box::from_raw(out) };
    layered.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
}

#[test]
fn the_oracle_carries_ottossons_numbers_and_separates_the_methods() {
    // Ottosson's published Oklab of sRGB red, and white at (1, 0, 0).
    let red = to_oklab([1.0, 0.0, 0.0]);
    for (got, want) in red.iter().zip([0.627_955, 0.224_863, 0.125_846]) {
        assert!((got - want).abs() <= 1e-4, "red {red:?}");
    }
    let white = to_oklab([1.0, 1.0, 1.0]);
    assert!((white[0] - 1.0).abs() <= 1e-4 && white[1].abs() <= 1e-4 && white[2].abs() <= 1e-4);
    // The scene: Perceptual and Classic part by at least 3 codes somewhere.
    let stops = n4(true);
    let apart = (0..=255)
        .map(|x| {
            let t = f64::from(x) / 255.0;
            let (p, c) = (perceptual(&stops, t, 1.0).0, classic(&stops, t, 1.0));
            (0..3)
                .map(|i| (p[i] - c[i]).abs() * 255.0)
                .fold(0.0, f64::max)
        })
        .fold(0.0, f64::max);
    assert!(apart >= 3.0, "Perceptual and Classic part by {apart} codes");
}

#[test]
fn perceptual_matches_the_oracle_at_8_bits() {
    for opaque in [true, false] {
        let stops = n4(opaque);
        for k in [0.0, 0.5, 1.0] {
            let out = map8(&params(&stops, k, false)).unwrap();
            for (x, px) in out.chunks_exact(4).enumerate() {
                let want = mapped_gray(&stops, x as f64 / 255.0, k, false);
                for c in 0..3 {
                    let w = (want[c] * 255.0).round();
                    assert!(
                        (f64::from(px[c]) - w).abs() <= 1.0,
                        "opaque {opaque} k {k} x {x} c {c}: {} vs {w}",
                        px[c]
                    );
                }
                assert_eq!(px[3], 255);
            }
        }
    }
}

#[test]
fn perceptual_matches_the_oracle_at_16_bits() {
    let stops = n4(false);
    for k in [0.0, 0.5, 1.0] {
        let out = map16(&params(&stops, k, false));
        for (x, px) in out.iter().enumerate() {
            let g = (65535.0 * x as f64 / 1023.0 + 0.5).floor() / 65535.0;
            let want = mapped_gray(&stops, g, k, false);
            for c in 0..3 {
                let w = want[c] * 65535.0;
                assert!(
                    (f64::from(px[c]) - w).abs() <= 3.0,
                    "k {k} x {x} c {c}: {} vs {w}",
                    px[c]
                );
            }
        }
    }
}

#[test]
fn perceptual_matches_the_oracle_at_32_bits_through_the_display_form() {
    // A 32-bit map is display-mapped (`adjust_float`): the map runs on the
    // sRGB-encoded clamped value and its result is decoded.
    let stops = n4(true);
    let row: Vec<[f32; 4]> = (0..64)
        .map(|i| {
            let v = decode(f64::from(i) / 63.0) as f32;
            [v, v, v, 1.0]
        })
        .collect();
    let profile = IccProfile::srgb();
    let doc = crate::float_fixture::float_doc(64, 1, &profile, std::slice::from_ref(&row));
    for k in [0.0, 0.5, 1.0] {
        let layered = doc
            .add_adjustment(
                "Gradient Map",
                &description(&params(&stops, k, false)),
                1.0,
                LIMIT,
                0,
                None,
            )
            .unwrap()
            .expect("the stack changed");
        let out = layered.render(LIMIT, 0, None).unwrap().to_rgba32().unwrap();
        for (px, input) in out.iter().zip(&row) {
            let e = encode(f64::from(input[0]));
            let want = mapped_gray(&stops, e, k, false).map(decode);
            for c in 0..3 {
                assert!(
                    (f64::from(px[c]) - want[c]).abs() <= 2e-3,
                    "k {k} e {e} c {c}: {} vs {}",
                    px[c],
                    want[c]
                );
            }
        }
    }
}

#[test]
fn a_reversed_perceptual_map_reads_one_minus_t() {
    let stops = n4(false);
    let out = map8(&params(&stops, 1.0, true)).unwrap();
    for (x, px) in out.chunks_exact(4).enumerate() {
        let want = mapped_gray(&stops, x as f64 / 255.0, 1.0, true);
        for c in 0..3 {
            let w = (want[c] * 255.0).round();
            assert!(
                (f64::from(px[c]) - w).abs() <= 1.0,
                "x {x} c {c}: {} vs {w}",
                px[c]
            );
        }
    }
    assert_ne!(out, map8(&params(&stops, 1.0, false)).unwrap());
}

#[test]
fn perceptual_refuses_out_of_range_values_as_classic_does() {
    let mut smooth = params(&n4(true), 1.0, false);
    smooth["gradient"]["smoothness"] = json!(1.5);
    assert!(map8(&smooth).is_err(), "smoothness 1.5");
    let mut midpoint = params(&n4(true), 1.0, false);
    midpoint["gradient"]["stops"][0]["midpoint"] = json!(0.01);
    assert!(map8(&midpoint).is_err(), "midpoint 0.01");
}

#[test]
fn a_cmyk_document_refuses_perceptual_by_name() {
    let pixels: Vec<[u16; 4]> = (0..9u32)
        .map(|i| [(i * 65535 / 8) as u16, 30000, 20000, 65535])
        .collect();
    let image = PrecisionImage::from_rgba16(9, 1, pixels, IccProfile::srgb(), None).unwrap();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ));
    let profile = press_profile("Press Test");
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_convert_to_cmyk(
            rgb,
            profile.as_ptr(),
            profile.len(),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!doc.is_null(), "to CMYK: {}", take_err_string(err));
    unsafe { dr_precision_doc_free(rgb) };
    let p = json!({"method": "perceptual", "dither": false, "gradient": {"stops": [
        {"position": 0.0, "ink": [0.0, 0.0, 0.0, 1.0]},
        {"position": 1.0, "ink": [0.0, 0.0, 0.0, 0.0]},
    ]}});
    let text = CString::new(description(&p)).unwrap();
    let sentence = "The gradient map's Perceptual method is unavailable in a CMYK document";
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_apply_adjustment(doc, 0, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(out.is_null(), "applied");
    assert_eq!(take_err_string(err), sentence, "destructive");
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment(
            doc,
            c"Gradient Map".as_ptr(),
            text.as_ptr(),
            1.0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null(), "added");
    assert_eq!(take_err_string(err), sentence, "as a layer");
    unsafe { dr_precision_doc_free(doc) };
}
