//! #1 Lab phase 2c, a Lab Color document's colour adjustments and channel
//! forms (`lab_adjust`, `lab_tone`, `adjust_precision`'s binding). Oracles
//! written here, never the Lab path:
//!
//! - the colour ops on in-gamut samples are within 0.2 L\* and 0.3 a\*/b\*
//!   of the hand CIE formulas (`lab_fixture`) round the same op on a document
//!   of the samples' codes in the op's space (phase 2d): Hue/Saturation,
//!   Color Balance, Photo Filter, Shadows/Highlights and Replace Color on a
//!   16-bit ProPhoto RGB document, their request colours converted by the
//!   hand formulas (Photo Filter's from sRGB, Replace Color's from the Lab
//!   codes its `#LLaabb` names); Color Lookup and Gradient Map on an sRGB
//!   document;
//! - Desaturate (master saturation −1 alone) keeps every L code and sets
//!   a = b = 0x8080 exactly, out of gamut too; a reds-only band leaves an
//!   out-of-gamut blue bit for bit;
//! - Levels and Curves with g and b keep L exactly (a map-form Curves: L
//!   is its kernel's R, as on RGB) and equal the sRGB document of the same
//!   codes per channel; the composite with g acts on L by the composite
//!   alone and on a by g alone;
//! - a Brightness/Contrast saturation scales a\* and b\* about 0 (f64
//!   oracle, within 1 code), L as phase 2b;
//! - Auto Tone and Auto Contrast at clip 0 stretch L to 0…65535 (within 1)
//!   and keep a and b; Equalize's L equals the sRGB `[L, L, L]` document's
//!   channel 0, a and b kept;
//! - each running op as a layer renders equal to its destructive result;
//!   Shadows/Highlights and Replace Color layers refuse; `.rz` and Lab → RGB
//!   keep a Hue/Saturation layer; Lab → RGB carries a channel Levels or
//!   Curves layer (also inside a group) byte-equal, its plan refusing
//!   nothing (phase 2f: channels match by index);
//! - a bound colour or Split preparation retains no more than its
//!   preparation reserve.
//!
//! Bounds declared before measuring.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_insert::dr_precision_doc_add_adjustment_at;
use doctored_core::ffi_lab::{
    dr_precision_doc_convert_lab_to_rgb, dr_precision_doc_convert_lab_to_rgb_plan,
    dr_precision_doc_convert_to_lab,
};
use doctored_core::ffi_precision::{
    dr_precision_doc_descriptor, dr_precision_doc_open, dr_precision_doc_save_native,
};
use doctored_core::ffi_precision_color::{
    dr_precision_doc_add_adjustment, dr_precision_doc_adjustment_plan,
    dr_precision_doc_apply_adjustment, dr_precision_doc_set_adjustment,
};
use doctored_core::ffi_precision_inspect::dr_precision_doc_adjustment_description;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::gray_ops::{edit, free, result, selected, LIMIT};
use crate::lab_fixture::{decode, lab_to_prophoto, lab_to_srgb, prophoto_to_lab, srgb_to_lab};

/// sRGB colours (8-bit codes) and alphas whose L\*a\*b\* spread over the
/// range, a\* and b\* both signs and neutral; no alpha is 0.
const PATCHES: [[u16; 4]; 12] = [
    [200, 40, 90, 255],
    [30, 180, 220, 255],
    [128, 128, 128, 255],
    [250, 250, 10, 200],
    [64, 0, 255, 255],
    [10, 10, 10, 255],
    [245, 245, 245, 255],
    [90, 160, 40, 120],
    [17, 34, 51, 255],
    [240, 128, 200, 255],
    [10, 20, 5, 60],
    [220, 230, 240, 255],
];

const NEUTRAL: u16 = 0x8080;

/// The bounds of the colour ops against the CIE oracle (declared before
/// measuring): L\*, then a\* and b\*.
const BOUND_L: f64 = 0.2;
const BOUND_AB: f64 = 0.3;

fn text_of(text: *mut c_char) -> String {
    let out = unsafe { CStr::from_ptr(text) }
        .to_str()
        .unwrap()
        .to_string();
    unsafe { dr_string_free(text) };
    out
}

fn descriptor(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_descriptor(doc, &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&text_of(text)).unwrap()
}

/// A document of one layer holding exactly `pixels` (one row) in `profile`.
fn rgb_doc(pixels: Vec<[u16; 4]>, profile: Arc<IccProfile>) -> *mut PrecisionDocument {
    let w = pixels.len() as u32;
    let image = PrecisionImage::from_rgba16(w, 1, pixels, profile.clone(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("Layer", image, (0, 0), 1.0, true).unwrap();
    let doc = PrecisionDocument::from_parts(
        w,
        1,
        profile,
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap();
    Box::into_raw(Box::new(doc))
}

fn srgb_doc(pixels: Vec<[u16; 4]>) -> *mut PrecisionDocument {
    rgb_doc(pixels, IccProfile::srgb())
}

/// `pixels` in `profile` through Mode ▸ Lab Color at `depth`.
fn lab_of(pixels: Vec<[u16; 4]>, profile: Arc<IccProfile>, depth: u32) -> *mut PrecisionDocument {
    let rgb = rgb_doc(pixels, profile);
    let options = CString::new(format!(r#"{{"depth":{depth}}}"#)).unwrap();
    let mut err = ptr::null_mut();
    let lab = unsafe {
        dr_precision_doc_convert_to_lab(rgb, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!lab.is_null(), "to Lab: {}", take_err_string(err));
    free(rgb);
    assert_eq!(descriptor(lab)["color_mode"], "lab");
    lab
}

/// The patches as a Lab document of `depth`.
fn lab_doc(depth: u32) -> *mut PrecisionDocument {
    lab_of(
        PATCHES.iter().map(|p| p.map(|c| c * 257)).collect(),
        IccProfile::srgb(),
        depth,
    )
}

/// The patches plus Display P3's blue, which is out of sRGB.
fn wide_lab(depth: u32) -> *mut PrecisionDocument {
    let mut pixels: Vec<[u16; 4]> = PATCHES.iter().map(|p| p.map(|c| c * 257)).collect();
    pixels.push([0, 0, 65535, 65535]);
    lab_of(pixels, IccProfile::display_p3(), depth)
}

fn description(op: &str, params: Value) -> String {
    json!({"type":"adjust","evaluation":"precision_v1","op":op,"params":params}).to_string()
}

fn apply(doc: *const PrecisionDocument, meta: &str) -> Result<*mut PrecisionDocument, String> {
    let meta = CString::new(meta).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_apply_adjustment(doc, 0, meta.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    result(out, err).map(|d| d.expect("the adjustment changed nothing"))
}

fn add(doc: *const PrecisionDocument, meta: &str) -> Result<*mut PrecisionDocument, String> {
    let name = CString::new("Adjustment").unwrap();
    let meta = CString::new(meta).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment(
            doc,
            name.as_ptr(),
            meta.as_ptr(),
            1.0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err).map(Option::unwrap)
}

fn add_at(doc: *const PrecisionDocument, meta: &str) -> Result<*mut PrecisionDocument, String> {
    let name = CString::new("Adjustment").unwrap();
    let meta = CString::new(meta).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment_at(
            doc,
            name.as_ptr(),
            meta.as_ptr(),
            1.0,
            0,
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err).map(Option::unwrap)
}

fn set(doc: *const PrecisionDocument, index: usize, meta: &str) -> Result<(), String> {
    let meta = CString::new(meta).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_set_adjustment(doc, index, meta.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    result(out, err).map(|d| {
        if let Some(d) = d {
            free(d);
        }
    })
}

fn plan(doc: *const PrecisionDocument, meta: &str, destructive: bool) -> Result<(), String> {
    let meta = CString::new(meta).unwrap();
    let mut err = ptr::null_mut();
    let index = if destructive { 0 } else { -1 };
    let out = unsafe {
        dr_precision_doc_adjustment_plan(doc, index, meta.as_ptr(), destructive, &mut err)
    };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        unsafe { dr_string_free(out) };
        Ok(())
    }
}

fn stored(doc: *const PrecisionDocument, index: usize) -> Option<String> {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_adjustment_description(doc, index, &mut err) };
    if out.is_null() {
        None
    } else {
        Some(text_of(out))
    }
}

fn codes(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(index)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn render(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

/// `meta` applied to an sRGB document holding exactly `pixels`.
fn oracle(pixels: Vec<[u16; 4]>, meta: &str) -> Vec<[u16; 4]> {
    oracle_in(pixels, meta, IccProfile::srgb())
}

/// `meta` applied to a `profile` document holding exactly `pixels`.
fn oracle_in(pixels: Vec<[u16; 4]>, meta: &str, profile: Arc<IccProfile>) -> Vec<[u16; 4]> {
    let rgb = rgb_doc(pixels, profile);
    let out = apply(rgb, meta).unwrap_or_else(|e| panic!("oracle {meta}: {e}"));
    let result = codes(out, 0);
    free(out);
    free(rgb);
    result
}

fn gray_of(pixels: &[[u16; 4]]) -> Vec<[u16; 4]> {
    pixels.iter().map(|p| [p[0], p[0], p[0], p[3]]).collect()
}

fn unit16(v: f64) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

/// A size-2 Color Lookup table that is not the identity: (r, g, b) → (g, b, r).
fn rotate_lookup() -> Value {
    let mut bytes = Vec::new();
    for b in 0..2 {
        for g in 0..2 {
            for r in 0..2 {
                for v in [g, b, r] {
                    bytes.extend((v as f32).to_le_bytes());
                }
            }
        }
    }
    json!({"kind":"3d","size":2,"table":base64(&bytes)})
}

/// Standard base64 (RFC 4648, padded), written here so the test does not
/// borrow the core's encoder.
fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The colour ops with parameters that move the patches.
fn color_ops() -> Vec<(&'static str, Value)> {
    vec![
        ("hue_saturation", json!({"hue":40,"saturation":0.3})),
        (
            "color_balance",
            json!({"shadows":{"cyan_red":-0.4,"magenta_green":0.3},
                "midtones":{"cyan_red":0.2,"magenta_green":-0.5,"yellow_blue":0.8},
                "highlights":{"cyan_red":0.6,"yellow_blue":-0.7},"preserve_luminosity":false}),
        ),
        (
            "photo_filter",
            json!({"color":"#ec8a00","density":0.5,"preserve_luminosity":true}),
        ),
        ("color_lookup", rotate_lookup()),
        (
            "gradient_map",
            json!({"gradient":{"stops":[{"position":0.0,"color":"#ff0000"},
                {"position":0.6,"color":"#20c040"},{"position":1.0,"color":"#00ffff"}]}}),
        ),
        (
            "shadows_highlights",
            json!({"shadows":{"amount":0.5,"tone":0.5},"highlights":{"amount":0.3,"tone":0.5},
                "radius":2,"color":0.4}),
        ),
        (
            "replace_color",
            json!({"colors":["#c82a5a"],"fuzziness":120,"hue":40,"saturation":0.5,
                "lightness":0.3}),
        ),
    ]
}

/// The colour ops that run in ProPhoto RGB (`lab_adjust::color_space`).
const PROPHOTO_OPS: [&str; 5] = [
    "hue_saturation",
    "color_balance",
    "photo_filter",
    "shadows_highlights",
    "replace_color",
];

/// A unit RGB colour as the 8-bit hex of its codes, clamped.
fn hex8(rgb: [f64; 3]) -> String {
    let c = rgb.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// `#rrggbb` as unit components.
fn unit_hex(hex: &str) -> [f64; 3] {
    let byte = |i: usize| f64::from(u8::from_str_radix(&hex[i..i + 2], 16).unwrap());
    [byte(1), byte(3), byte(5)]
}

/// `params` with each request colour in ProPhoto RGB numbers by the hand
/// formulas: Photo Filter's sRGB `color`, Replace Color's `colors` (the Lab
/// document's 8-bit `#LLaabb` codes).
fn prophoto_params(op: &str, mut params: Value) -> Value {
    match op {
        "photo_filter" => {
            let srgb = unit_hex(params["color"].as_str().unwrap()).map(|c| c / 255.0);
            params["color"] = hex8(lab_to_prophoto(srgb_to_lab(srgb))).into();
        }
        "replace_color" => {
            for color in params["colors"].as_array_mut().unwrap() {
                let [l, a, b] = unit_hex(color.as_str().unwrap());
                let lab = [100.0 * l / 255.0, a - 128.0, b - 128.0];
                *color = hex8(lab_to_prophoto(lab)).into();
            }
        }
        _ => {}
    }
    params
}

/// Each op of `ops` on the patches against its oracle: `to_space` and
/// `from_space` the hand formulas of the oracle document's space (Lab →
/// unit codes, unit codes → Lab), `profile` the oracle document's and
/// `request` the oracle's params.
fn check_colour_ops(
    ops: Vec<(&'static str, Value)>,
    profile: Arc<IccProfile>,
    to_space: &dyn Fn([f64; 3]) -> [f64; 3],
    from_space: &dyn Fn([f64; 3]) -> [f64; 3],
    request: &dyn Fn(&str, Value) -> Value,
) {
    let lab = lab_doc(16);
    let source = codes(lab, 0);
    // The patches' codes in the space by the hand formulas (in gamut by
    // construction).
    let pixels: Vec<[u16; 4]> = source
        .iter()
        .map(|p| {
            let rgb = to_space(decode(*p));
            [unit16(rgb[0]), unit16(rgb[1]), unit16(rgb[2]), p[3]]
        })
        .collect();
    for (op, params) in ops {
        let meta = description(op, params.clone());
        let out = apply(lab, &meta).unwrap_or_else(|e| panic!("{op}: {e}"));
        assert_eq!(descriptor(out)["color_mode"], "lab", "{op}");
        let got = codes(out, 0);
        let want = oracle_in(
            pixels.clone(),
            &description(op, request(op, params)),
            profile.clone(),
        );
        let mut moved = false;
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            let lab = decode(*g);
            // The Lab encoding holds L* 0…100 and a*, b* −128…127 (a
            // saturated ProPhoto result can pass 127, which clips).
            let unclamped = from_space([w[0], w[1], w[2]].map(|c| f64::from(c) / 65535.0));
            let expected = [
                unclamped[0].clamp(0.0, 100.0),
                unclamped[1].clamp(-128.0, 127.0),
                unclamped[2].clamp(-128.0, 127.0),
            ];
            assert!(
                (lab[0] - expected[0]).abs() <= BOUND_L,
                "{op} pixel {i} L {} vs {}",
                lab[0],
                expected[0]
            );
            for c in 1..3 {
                assert!(
                    (lab[c] - expected[c]).abs() <= BOUND_AB,
                    "{op} pixel {i} channel {c} {} vs {}",
                    lab[c],
                    expected[c]
                );
            }
            assert_eq!(g[3], source[i][3], "{op} pixel {i} alpha");
            moved |= g != &source[i];
        }
        assert!(moved, "{op} changed nothing");
        free(out);
    }
    free(lab);
}

#[test]
fn colour_ops_match_the_cie_oracle_round_the_prophoto_op() {
    let ops = color_ops()
        .into_iter()
        .filter(|(op, _)| PROPHOTO_OPS.contains(op))
        .collect();
    check_colour_ops(
        ops,
        IccProfile::prophoto_rgb(),
        &lab_to_prophoto,
        &prophoto_to_lab,
        &prophoto_params,
    );
}

#[test]
fn lookup_ops_match_the_cie_oracle_round_the_srgb_op() {
    let ops = color_ops()
        .into_iter()
        .filter(|(op, _)| !PROPHOTO_OPS.contains(op))
        .collect();
    check_colour_ops(
        ops,
        IccProfile::srgb(),
        &lab_to_srgb,
        &srgb_to_lab,
        &|_, params| params,
    );
}

#[test]
fn desaturate_keeps_lightness_and_neutralizes_a_and_b() {
    for depth in [8, 16] {
        let lab = wide_lab(depth);
        let source = codes(lab, 0);
        let blue = *source.last().unwrap();
        let rgb = lab_to_srgb(decode(blue));
        assert!(
            rgb.iter().any(|c| !(0.0..=1.0).contains(c)),
            "P3 blue is out of sRGB: {rgb:?}"
        );
        let meta = description("hue_saturation", json!({"saturation":-1}));
        let out = apply(lab, &meta).unwrap();
        for (i, (p, s)) in codes(out, 0).iter().zip(&source).enumerate() {
            assert_eq!(p[0], s[0], "depth {depth} pixel {i} L");
            assert_eq!(p[1], NEUTRAL, "depth {depth} pixel {i} a");
            assert_eq!(p[2], NEUTRAL, "depth {depth} pixel {i} b");
            assert_eq!(p[3], s[3], "depth {depth} pixel {i} alpha");
        }
        // The selected edit (the host's Image ▸ Adjustments ▸ Desaturate).
        let request = json!({"layer":0,"op":"adjustment","description":meta});
        let picked = selected(lab, &request, &[]).unwrap().unwrap();
        assert_eq!(codes(picked, 0), codes(out, 0), "depth {depth} selected");
        // With a hue it is the Color form, not Desaturate.
        let hued = apply(
            lab,
            &description("hue_saturation", json!({"saturation":-1,"hue":10})),
        )
        .unwrap();
        assert_ne!(codes(hued, 0), codes(out, 0), "depth {depth} hue 10");
        for doc in [hued, picked, out, lab] {
            free(doc);
        }
    }
}

#[test]
fn a_reds_band_leaves_an_out_of_gamut_blue_bit_for_bit() {
    let lab = wide_lab(16);
    let source = codes(lab, 0);
    let meta = description(
        "hue_saturation",
        json!({"bands":{"reds":{"hue":30,"saturation":0.2}}}),
    );
    let out = apply(lab, &meta).unwrap();
    let got = codes(out, 0);
    assert_eq!(got.last(), source.last(), "the P3 blue");
    assert_ne!(got[0], source[0], "the red patch moves");
    free(out);
    free(lab);
}

#[test]
fn levels_and_curves_channels_act_on_lightness_a_and_b() {
    let lab = lab_doc(16);
    let source = codes(lab, 0);
    for (op, params) in [
        (
            "levels",
            json!({"g":{"gamma":1.4},"b":{"black":0.1,"white":0.9}}),
        ),
        (
            "curves",
            json!({"g":[[0,20],[128,150],[255,240]],"b":[[0,0],[100,60],[255,255]]}),
        ),
        (
            "curves",
            json!({"g_map":(0..256).map(|v| (v * 3 / 4) as u32).collect::<Vec<u32>>(),
                "b_map":(0..256).map(|v| 255 - v as u32).collect::<Vec<u32>>()}),
        ),
    ] {
        let what = format!("{op} {params}");
        let meta = description(op, params);
        let out = apply(lab, &meta).unwrap();
        let got = codes(out, 0);
        free(out);
        let want = oracle(source.clone(), &meta);
        // A map-form Curves resamples its absent identity maps (the RGB
        // kernel's own R on RGB too), so L is the kernel's R; the point
        // forms keep L bit for bit.
        let maps = meta.contains("_map");
        for (i, p) in got.iter().enumerate() {
            assert_eq!(p[0], want[i][0], "{what} pixel {i} L by the R stage");
            if !maps {
                assert_eq!(p[0], source[i][0], "{what} pixel {i} L kept");
            }
            assert_eq!(p[1..], want[i][1..], "{what} pixel {i} a, b, alpha");
        }
    }
    // The composite with g: L by the composite alone, a by g alone.
    let composite = json!({"gamma":1.3,"black":0.05});
    let g = json!({"gamma":0.7});
    let meta = description("levels", json!({"gamma":1.3,"black":0.05,"g":g}));
    let out = apply(lab, &meta).unwrap();
    let got = codes(out, 0);
    free(out);
    let on_l = oracle(gray_of(&source), &description("levels", composite));
    let on_a = oracle(source.clone(), &description("levels", json!({ "g": g })));
    for (i, p) in got.iter().enumerate() {
        assert_eq!(p[0], on_l[i][0], "pixel {i} L by the composite");
        assert_eq!(p[1], on_a[i][1], "pixel {i} a by g");
        assert_eq!(p[2], source[i][2], "pixel {i} b kept");
    }
    free(lab);
}

#[test]
fn a_bcs_saturation_scales_a_and_b_about_zero() {
    let lab = lab_doc(16);
    let source = codes(lab, 0);
    // saturation −0.5 is a scale of 0.5 (`1 + saturation`).
    let meta = description(
        "bcs",
        json!({"brightness":0.1,"contrast":0.2,"saturation":-0.5}),
    );
    let out = apply(lab, &meta).unwrap();
    let got = codes(out, 0);
    free(out);
    let on_l = oracle(gray_of(&source), &meta);
    let n = f64::from(NEUTRAL) / 65535.0;
    for (i, p) in got.iter().enumerate() {
        assert_eq!(p[0], on_l[i][0], "pixel {i} L");
        for c in 1..3 {
            let v = f64::from(source[i][c]) / 65535.0;
            let want = (n + 0.5 * (v - n)).clamp(0.0, 1.0) * 65535.0;
            assert!(
                (f64::from(p[c]) - want).abs() <= 1.0,
                "pixel {i} channel {c}: {} vs {want}",
                p[c]
            );
        }
        assert_eq!(p[3], source[i][3]);
    }
    free(lab);
}

#[test]
fn auto_tone_contrast_and_equalize_act_on_lightness() {
    for op in ["auto_tone", "auto_contrast"] {
        let lab = lab_doc(16);
        let source = codes(lab, 0);
        let request = json!({"layer":0,"op":op,"params":{"clip":0}});
        let out = selected(lab, &request, &[]).unwrap().unwrap();
        let got = codes(out, 0);
        let lo = got.iter().map(|p| p[0]).min().unwrap();
        let hi = got.iter().map(|p| p[0]).max().unwrap();
        assert!(lo <= 1, "{op} L min {lo}");
        assert!(hi >= 65534, "{op} L max {hi}");
        for (i, p) in got.iter().enumerate() {
            assert_eq!(p[1..], source[i][1..], "{op} pixel {i} a, b, alpha");
        }
        free(out);
        free(lab);
    }
    let lab = lab_doc(16);
    let source = codes(lab, 0);
    let request = json!({"layer":0,"op":"equalize","params":{"scope":"entire"}});
    let out = selected(lab, &request, &[]).unwrap().unwrap();
    let rgb = srgb_doc(gray_of(&source));
    let equalized = selected(rgb, &request, &[]).unwrap().unwrap();
    let want = codes(equalized, 0);
    free(equalized);
    for (i, p) in codes(out, 0).iter().enumerate() {
        assert_eq!(p[0], want[i][0], "equalize pixel {i} L");
        assert_eq!(p[1..], source[i][1..], "equalize pixel {i} a, b, alpha");
    }
    for doc in [out, rgb, lab] {
        free(doc);
    }
}

/// The ops that run as a layer, with parameters that move the patches.
fn layer_ops() -> Vec<(&'static str, Value)> {
    let mut ops: Vec<_> = color_ops()
        .into_iter()
        .filter(|(op, _)| !matches!(*op, "shadows_highlights" | "replace_color"))
        .collect();
    ops.push(("hue_saturation", json!({"saturation":-1})));
    ops.push((
        "levels",
        json!({"gamma":1.2,"g":{"gamma":1.4},"b":{"gamma":0.8}}),
    ));
    ops.push(("curves", json!({"b":[[0,20],[255,235]]})));
    ops.push(("bcs", json!({"brightness":0.1,"saturation":-0.4})));
    ops
}

#[test]
fn every_running_op_as_a_layer_renders_equal_to_the_destructive_result() {
    let lab = lab_doc(16);
    for (op, params) in layer_ops() {
        let what = format!("{op} {params} layer");
        let meta = description(op, params);
        let destructive = apply(lab, &meta).unwrap();
        for layered in [add(lab, &meta), add_at(lab, &meta)] {
            let layered = layered.unwrap_or_else(|e| panic!("{what}: {e}"));
            assert_eq!(render(layered), render(destructive), "{what}");
            assert_eq!(stored(layered, 1).as_deref(), Some(meta.as_str()), "{what}");
            free(layered);
        }
        free(destructive);
    }
    free(lab);
}

#[test]
fn shadows_highlights_and_replace_color_refuse_as_layers() {
    let lab = lab_doc(16);
    let levels = add(lab, &description("levels", json!({"gamma":1.2}))).unwrap();
    for (op, params) in color_ops() {
        let (name, layer) = match op {
            "shadows_highlights" => ("Shadows/Highlights", true),
            "replace_color" => ("Replace Color", true),
            _ => ("", false),
        };
        if !layer {
            continue;
        }
        let sentence = format!(
            "A {name} adjustment layer is unavailable in a Lab Color document: {name} applies to pixels only."
        );
        let meta = description(op, params);
        assert_eq!(add(lab, &meta).unwrap_err(), sentence, "{op} add");
        assert_eq!(add_at(lab, &meta).unwrap_err(), sentence, "{op} add_at");
        assert_eq!(set(levels, 1, &meta).unwrap_err(), sentence, "{op} set");
        assert_eq!(plan(lab, &meta, false).unwrap_err(), sentence, "{op} plan");
        // Destructively they run.
        plan(lab, &meta, true).unwrap();
        free(apply(lab, &meta).unwrap());
    }
    free(levels);
    free(lab);
}

fn to_rgb_plan(doc: *const PrecisionDocument) -> Value {
    let srgb = IccProfile::srgb();
    let options = CString::new("{}").unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_lab_to_rgb_plan(
            doc,
            srgb.bytes().as_ptr(),
            srgb.bytes().len(),
            options.as_ptr(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&text_of(out)).unwrap()
}

fn to_rgb(doc: *const PrecisionDocument) -> Result<*mut PrecisionDocument, String> {
    let srgb = IccProfile::srgb();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_lab_to_rgb(
            doc,
            srgb.bytes().as_ptr(),
            srgb.bytes().len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err).map(|d| d.expect("the conversion changed nothing"))
}

#[test]
fn a_hue_saturation_layer_survives_rz_and_lab_to_rgb() {
    let lab = lab_doc(16);
    let meta = description("hue_saturation", json!({"hue":40,"saturation":0.3}));
    let layered = add(lab, &meta).unwrap();
    let before = render(layered);
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("hue.rz");
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_native(layered, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
        },
        "{}",
        take_err_string(err)
    );
    let reopened = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!reopened.is_null(), "{}", take_err_string(err));
    assert_eq!(descriptor(reopened)["color_mode"], "lab");
    assert_eq!(stored(reopened, 1).as_deref(), Some(meta.as_str()));
    assert_eq!(render(reopened), before);
    // Lab → RGB keeps the Hue/Saturation layer and a composite-only Levels.
    let levels = description("levels", json!({"gamma":1.4,"r":{"gamma":1.0}}));
    let composite = description("levels", json!({"gamma":1.4}));
    let both = add(layered, &composite).unwrap();
    assert!(to_rgb_plan(both)["refusal"].is_null());
    let rgb = to_rgb(both).unwrap();
    let d = descriptor(rgb);
    assert_eq!(d["color_mode"], "rgb");
    assert_eq!(d["layer_count"], 3);
    assert_eq!(stored(rgb, 1).as_deref(), Some(meta.as_str()));
    assert_eq!(stored(rgb, 2).as_deref(), Some(composite.as_str()));
    // A Levels layer with an r setting is a Lightness stage: since phase 2f
    // it carries byte-equal, as a Red one.
    let channel = add(lab, &levels).unwrap();
    assert!(to_rgb_plan(channel)["refusal"].is_null());
    let carried = to_rgb(channel).unwrap();
    assert_eq!(stored(carried, 1).as_deref(), Some(levels.as_str()));
    for doc in [carried, channel, rgb, both, reopened, layered, lab] {
        free(doc);
    }
}

#[test]
fn lab_to_rgb_carries_a_channel_levels_or_curves_layer() {
    let lab = lab_doc(16);
    let before = codes(lab, 0);
    let g_levels = description("levels", json!({"g":{"gamma":1.3}}));
    let levels = add(lab, &g_levels).unwrap();
    assert!(to_rgb_plan(levels)["refusal"].is_null());
    let rgb_levels = to_rgb(levels).unwrap();
    assert_eq!(descriptor(rgb_levels)["color_mode"], "rgb");
    assert_eq!(stored(rgb_levels, 1).as_deref(), Some(g_levels.as_str()));
    // A Curves r_map layer inside a group.
    let r_map: Vec<u32> = (0..256).map(|v| 255 - v).collect();
    let r_curves = description("curves", json!({ "r_map": r_map }));
    let curves = add(lab, &r_curves).unwrap();
    let grouped = edit(
        curves,
        &json!({"op":"group_layers","layers":[1],"name":"Group"}),
        &[],
    )
    .unwrap()
    .unwrap();
    assert_eq!(descriptor(grouped)["color_mode"], "lab");
    assert!(to_rgb_plan(grouped)["refusal"].is_null());
    let rgb_grouped = to_rgb(grouped).unwrap();
    assert_eq!(stored(rgb_grouped, 1).as_deref(), Some(r_curves.as_str()));
    // Every source document is unchanged.
    for doc in [levels, grouped] {
        assert_eq!(descriptor(doc)["color_mode"], "lab");
        assert_eq!(codes(doc, 0), before);
    }
    for doc in [rgb_grouped, grouped, curves, rgb_levels, levels, lab] {
        free(doc);
    }
}

#[test]
fn a_bound_preparation_retains_no_more_than_its_reserve() {
    let lab = lab_doc(16);
    let twin = unsafe { &*lab }.profile().clone();
    let identity: Vec<u32> = (0..256).collect();
    for meta in [
        description(
            "curves",
            json!({"g_map":identity.iter().map(|v| v * 3 / 4).collect::<Vec<u32>>(),
                "b_map":identity}),
        ),
        description("hue_saturation", json!({"hue":40,"saturation":0.3})),
        description(
            "gradient_map",
            json!({"gradient":{"stops":[{"position":0.0,"color":"#ff0000"},
                {"position":1.0,"color":"#00ffff"}]}}),
        ),
        description(
            "replace_color",
            json!({"colors":["#c82a5a"],"fuzziness":120,"hue":40}),
        ),
        description(
            "photo_filter",
            json!({"color":"#ec8a00","density":0.5,"preserve_luminosity":true}),
        ),
        description(
            "shadows_highlights",
            json!({"shadows":{"amount":0.5,"tone":0.5},"highlights":{"amount":0.3,"tone":0.5},
                "radius":2,"color":0.4}),
        ),
    ] {
        let prepared = PrecisionAdjustment::parse(&meta, &twin).unwrap();
        let reserve = PrecisionAdjustment::prepare_scratch_bytes(&meta, &twin).unwrap();
        assert!(
            prepared.retained_bytes() <= reserve,
            "{meta}: {} > {reserve}",
            prepared.retained_bytes()
        );
    }
    free(lab);
}
