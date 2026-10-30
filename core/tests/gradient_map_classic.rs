//! The Gradient Map's `method: "classic"` at 8, 16 and 32 bits and on
//! CMYK ink, through the FFI, against an oracle written here from
//! `gradient_map_678_take.py`'s declared law, never read off the core:
//! between stops `i` and `i + 1` at the segment fraction `f`, `f` is
//! remapped through stop `i`'s midpoint `m` (`f / 2m` up to `m`, then
//! `1/2 + (f − m) / (2 (1 − m))`), the colour is `(1 − k)·L + k·C` with
//! `L` the linear interpolation and `C` the uniform Catmull-Rom Hermite
//! (tangents `(v[i+1] − v[i−1]) / 2`, end stops repeated) clamped per
//! channel to the two bracketing stops, `k` the Smoothness; the opacity is
//! linear through stop `i`'s opacity midpoint and blends the original with
//! the mapped colour. A legacy map (no `method`) is byte-identical to
//! Classic at Smoothness 0 with every midpoint 0.5, and the refusals
//! (another method, a Smoothness or a midpoint out of range) name nothing
//! partial.

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

/// One oracle stop: position, colour (N components in 0..1), opacity, colour
/// midpoint, opacity midpoint.
type Stop<const N: usize> = (f64, [f64; N], f64, f64, f64);

/// `pwl.pre` (module doc).
fn remap(f: f64, m: f64) -> f64 {
    if m == 0.5 {
        f
    } else if f <= m {
        f / (2.0 * m)
    } else {
        0.5 + (f - m) / (2.0 * (1.0 - m))
    }
}

/// The declared Classic law (module doc): (colour, opacity) at `t`.
fn oracle<const N: usize>(stops: &[Stop<N>], t: f64, k: f64) -> ([f64; N], f64) {
    let n = stops.len();
    if t <= stops[0].0 {
        return (stops[0].1, stops[0].2);
    }
    if t >= stops[n - 1].0 {
        return (stops[n - 1].1, stops[n - 1].2);
    }
    let i = (0..n - 1)
        .find(|&i| t >= stops[i].0 && t < stops[i + 1].0)
        .unwrap();
    let (a, b) = (stops[i], stops[i + 1]);
    let f0 = (t - a.0) / (b.0 - a.0);
    let f = remap(f0, a.3);
    let fo = remap(f0, a.4);
    let before = if i > 0 { stops[i - 1].1 } else { a.1 };
    let after = if i + 2 < n { stops[i + 2].1 } else { b.1 };
    let (f2, f3) = (f * f, f * f * f);
    let color = std::array::from_fn(|c| {
        let cr = (2.0 * f3 - 3.0 * f2 + 1.0) * a.1[c]
            + (f3 - 2.0 * f2 + f) * (b.1[c] - before[c]) / 2.0
            + (-2.0 * f3 + 3.0 * f2) * b.1[c]
            + (f3 - f2) * (after[c] - a.1[c]) / 2.0;
        let cr = cr.clamp(a.1[c].min(b.1[c]), a.1[c].max(b.1[c]));
        let lin = a.1[c] + (b.1[c] - a.1[c]) * f;
        (1.0 - k) * lin + k * cr
    });
    (color, a.2 + (b.2 - a.2) * fo)
}

/// The four-stop scene of the confirm take (N4) with the take's SEG
/// midpoints moved onto our stop `i` (stop `i` governs segment `i`), and a
/// half-opaque third stop with an opacity midpoint.
fn n4(opaque: bool) -> Vec<Stop<3>> {
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

/// The `gradient_map` params of `stops`: Classic unless `legacy`.
fn params(stops: &[Stop<3>], k: f64, legacy: bool) -> Value {
    let list: Vec<Value> = stops
        .iter()
        .map(|s| {
            json!({"position": s.0, "color": hex(s.1), "opacity": s.2, "midpoint": s.3,
                   "opacity_midpoint": s.4})
        })
        .collect();
    let mut p = json!({"dither": false, "gradient": {"stops": list, "smoothness": k}});
    if !legacy {
        p["method"] = json!("classic");
    }
    p
}

/// The 8-bit ramp: gray x at column x.
fn ramp8() -> Vec<u8> {
    (0..=255u8).flat_map(|g| [g, g, g, 255]).collect()
}

/// `params` over the 8-bit ramp by `dr_image_adjust_op`: `Err` its refusal.
fn map8(params: &Value) -> Result<Vec<u8>, String> {
    let rgba = ramp8();
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

fn layered16(doc: &PrecisionDocument, params: &Value) -> PrecisionDocument {
    let text = CString::new(description(params)).unwrap();
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
    assert!(!out.is_null(), "{}", take_err_string(err));
    *unsafe { Box::from_raw(out) }
}

/// The 16-bit ramp: 1024 grays round(65535 x / 1023), mapped as a layer.
fn map16(params: &Value) -> Vec<[u16; 4]> {
    let gray: Vec<u16> = (0..1024u32)
        .map(|x| (65535.0 * f64::from(x) / 1023.0 + 0.5).floor() as u16)
        .collect();
    let pixels: Vec<[u16; 4]> = gray.iter().map(|&g| [g, g, g, 65535]).collect();
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
    layered16(&doc, params)
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

/// What the map does to the gray `g` (unit) under the oracle.
fn mapped_gray(stops: &[Stop<3>], g: f64, k: f64) -> [f64; 3] {
    let (color, opacity) = oracle(stops, g, k);
    color.map(|c| g + (c - g) * opacity.clamp(0.0, 1.0))
}

#[test]
fn classic_matches_the_oracle_at_8_bits() {
    for opaque in [true, false] {
        let stops = n4(opaque);
        for k in [0.0, 0.5, 1.0] {
            let out = map8(&params(&stops, k, false)).unwrap();
            for (x, px) in out.chunks_exact(4).enumerate() {
                let want = mapped_gray(&stops, x as f64 / 255.0, k);
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
fn classic_matches_the_oracle_at_16_bits() {
    let stops = n4(false);
    for k in [0.0, 0.5, 1.0] {
        let out = map16(&params(&stops, k, false));
        for (x, px) in out.iter().enumerate() {
            let g = (65535.0 * x as f64 / 1023.0 + 0.5).floor() / 65535.0;
            let want = mapped_gray(&stops, g, k);
            for c in 0..3 {
                let w = want[c] * 65535.0;
                assert!(
                    (f64::from(px[c]) - w).abs() <= 1.0,
                    "k {k} x {x} c {c}: {} vs {w}",
                    px[c]
                );
            }
        }
    }
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

#[test]
fn classic_matches_the_oracle_at_32_bits_through_the_display_form() {
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
            let want = mapped_gray(&stops, e, k).map(decode);
            for c in 0..3 {
                assert!(
                    (f64::from(px[c]) - want[c]).abs() <= 1e-3,
                    "k {k} e {e} c {c}: {} vs {}",
                    px[c],
                    want[c]
                );
            }
        }
    }
}

/// A CMYK document converted from u16 sRGB `pixels` into the press profile.
fn ink_doc(pixels: Vec<[u16; 4]>) -> *mut PrecisionDocument {
    let w = pixels.len() as u32;
    let image = PrecisionImage::from_rgba16(w, 1, pixels, IccProfile::srgb(), None).unwrap();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ));
    let profile = press_profile("Press Test");
    let mut err = ptr::null_mut();
    let out = unsafe {
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
    assert!(!out.is_null(), "to CMYK: {}", take_err_string(err));
    unsafe { dr_precision_doc_free(rgb) };
    out
}

#[test]
fn classic_matches_the_oracle_on_cmyk_ink() {
    let pixels: Vec<[u16; 4]> = (0..97u32)
        .map(|i| {
            let v = (i * 65535 / 96) as u16;
            [
                v,
                (v / 3).wrapping_add(i as u16 * 200),
                65535 - v / 2,
                65535,
            ]
        })
        .collect();
    let doc = ink_doc(pixels);
    let source = unsafe { &*doc }.cmyk_layer_samples(0).unwrap();
    let ink_stops: Vec<Stop<4>> = vec![
        (0.0, [0.0, 0.0, 0.0, 1.0], 1.0, 0.7, 0.5),
        (0.3, [0.8, 0.1, 0.0, 0.2], 0.6, 0.2, 0.3),
        (0.6, [0.0, 0.9, 0.7, 0.0], 1.0, 0.6, 0.5),
        (1.0, [0.0, 0.0, 0.0, 0.0], 1.0, 0.5, 0.5),
    ];
    for k in [0.0, 0.5, 1.0] {
        let list: Vec<Value> = ink_stops
            .iter()
            .map(|s| {
                json!({"position": s.0, "ink": s.1, "opacity": s.2, "midpoint": s.3,
                       "opacity_midpoint": s.4})
            })
            .collect();
        let p = json!({"method": "classic", "dither": false,
                       "gradient": {"stops": list, "smoothness": k}});
        let text = CString::new(description(&p)).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_apply_adjustment(
                doc,
                0,
                text.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!out.is_null(), "{}", take_err_string(err));
        let got = unsafe { &*out }.cmyk_layer_samples(0).unwrap();
        for (i, (g, s)) in got.iter().zip(&source).enumerate() {
            let ink = [0, 1, 2, 3].map(|c| f64::from(s[c]) / 65535.0);
            let luma = 0.2126 * (1.0 - ink[0]) + 0.7152 * (1.0 - ink[1]) + 0.0722 * (1.0 - ink[2]);
            let t = (luma * (1.0 - ink[3])).clamp(0.0, 1.0);
            let (mapped, opacity) = oracle(&ink_stops, t, k);
            let opacity = opacity.clamp(0.0, 1.0);
            for c in 0..4 {
                let want =
                    ((ink[c] + (mapped[c] - ink[c]) * opacity).clamp(0.0, 1.0) * 65535.0).round();
                assert!(
                    (f64::from(g[c]) - want).abs() <= 2.0,
                    "k {k} pixel {i} plate {c}: {} vs {want}",
                    g[c]
                );
            }
        }
        unsafe { dr_precision_doc_free(out) };
    }
    unsafe { dr_precision_doc_free(doc) };
}

/// A CMYK stop's midpoints round to 0.01 as the RGB map's do, so a map
/// renders the same before and after a `grdm` round trip (which writes
/// round(m × 100)): 0.333 renders exactly as 0.33.
#[test]
fn cmyk_stop_midpoints_round_to_whole_percent() {
    let pixels: Vec<[u16; 4]> = (0..97u32)
        .map(|i| [(i * 65535 / 96) as u16, 30000, 20000, 65535])
        .collect();
    let doc = ink_doc(pixels);
    let render = |m: f64| {
        let p = json!({"method": "classic", "dither": false, "gradient": {"stops": [
            {"position": 0.0, "ink": [0.0, 0.0, 0.0, 1.0], "midpoint": m, "opacity_midpoint": m},
            {"position": 1.0, "ink": [0.0, 0.0, 0.0, 0.0]},
        ]}});
        let text = CString::new(description(&p)).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_apply_adjustment(
                doc,
                0,
                text.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!out.is_null(), "{}", take_err_string(err));
        let got = unsafe { &*out }.cmyk_layer_samples(0).unwrap();
        unsafe { dr_precision_doc_free(out) };
        got
    };
    assert_eq!(render(0.333), render(0.33));
    assert_ne!(render(0.33), render(0.5));
    unsafe { dr_precision_doc_free(doc) };
}

#[test]
fn a_legacy_map_is_classic_at_smoothness_0_with_midpoints_at_half() {
    let stops: Vec<Stop<3>> = n4(false)
        .into_iter()
        .map(|s| (s.0, s.1, s.2, 0.5, 0.5))
        .collect();
    let legacy = params(&stops, 1.0, true);
    let classic = params(&stops, 0.0, false);
    assert_eq!(map8(&legacy).unwrap(), map8(&classic).unwrap(), "8 bits");
    assert_eq!(map16(&legacy), map16(&classic), "16 bits");
    // A legacy map ignores its midpoints and Smoothness, as before.
    let moved = params(&n4(false), 0.25, true);
    assert_eq!(
        map8(&moved).unwrap(),
        map8(&legacy).unwrap(),
        "legacy ignores them"
    );
}

#[test]
fn another_method_and_out_of_range_values_refuse() {
    for method in ["linear", "smooth"] {
        let mut other = params(&n4(true), 1.0, false);
        other["method"] = json!(method);
        assert!(map8(&other).is_err(), "{method} is refused for good");
    }
    let mut smooth = params(&n4(true), 1.0, false);
    smooth["gradient"]["smoothness"] = json!(1.5);
    assert!(map8(&smooth).is_err(), "smoothness 1.5");
    let mut midpoint = params(&n4(true), 1.0, false);
    midpoint["gradient"]["stops"][0]["midpoint"] = json!(0.01);
    assert!(map8(&midpoint).is_err(), "midpoint 0.01");
    let mut edge = params(&n4(true), 1.0, false);
    edge["gradient"]["stops"][0]["midpoint"] = json!(0.05);
    edge["gradient"]["stops"][1]["midpoint"] = json!(0.95);
    assert!(map8(&edge).is_ok(), "5 % and 95 % are admitted");
}
