//! #547 Lab phase 2d, the colour adjustments' ProPhoto RGB route
//! (`lab_adjust::color_space`, `kernel_params`, `kernel_rgb`): a Lab colour
//! well outside sRGB — S = L\* 60, a\* −100, b\* 60 — survives a +5° hue shift
//! and a Color Balance nudge with its chroma within 1 % of the hand oracle
//! (`lab_fixture`'s ProPhoto formulas round the kernel's own rule) and well
//! above the chroma sRGB would clip it to; identity settings render or leave
//! every code exactly (8 and 16 bits); Photo Filter's sRGB colour is
//! converted (the in-gamut sample lands on 2c's sRGB oracle); the value-only
//! `dr_lab_kernel_rgb` reads what the kernel reads; and Replace Color's
//! `#LLaabb` colours are the document's own codes. Bounds declared before
//! measuring.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_lab::{
    dr_lab_kernel_rgb, dr_precision_doc_convert_to_lab, dr_precision_doc_new_lab,
};
use doctored_core::ffi_precision::dr_precision_doc_descriptor;
use doctored_core::ffi_precision_color::{
    dr_precision_doc_add_adjustment, dr_precision_doc_apply_adjustment,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::gray_ops::{free, result, LIMIT};
use crate::lab_fixture::{
    decode, lab_code_hex, lab_to_prophoto, lab_to_srgb, prophoto_to_lab, srgb_to_lab,
};

/// The wide-gamut sample: a high-chroma green outside sRGB.
const S: [f64; 3] = [60.0, -100.0, 60.0];

/// An in-sRGB sample.
const IN: [f64; 3] = [50.0, 20.0, -30.0];

/// Bounds (declared before measuring): chroma within 1 % of the oracle's,
/// L\* within 0.5, and the result's chroma at least 1.15× the sRGB clip's.
const CHROMA_RATIO: f64 = 0.01;
const BOUND_L: f64 = 0.5;
const OVER_CLIP: f64 = 1.15;

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
pub(crate) fn rgb_doc(pixels: Vec<[u16; 4]>, profile: Arc<IccProfile>) -> *mut PrecisionDocument {
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

/// The Lab twin, read off a new Lab document.
pub(crate) fn twin() -> Arc<IccProfile> {
    let request = CString::new(r#"{"depth":16,"fill_srgb":[1,1,1,1]}"#).unwrap();
    let mut err = ptr::null_mut();
    let lab = unsafe { dr_precision_doc_new_lab(1, 1, request.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!lab.is_null(), "{}", take_err_string(err));
    let profile = unsafe { &*lab }.profile().clone();
    free(lab);
    profile
}

/// The ICC v4 16-bit Lab codes of an L\*a\*b\* colour.
pub(crate) fn encode(lab: [f64; 3]) -> [u16; 4] {
    let code = |v: f64| (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
    [
        code(lab[0] / 100.0),
        code((lab[1] + 128.0) / 255.0),
        code((lab[2] + 128.0) / 255.0),
        65535,
    ]
}

/// A Lab document of `depth` holding `colours`: their codes in the twin,
/// through Mode ▸ Lab Color (the twin into itself).
pub(crate) fn lab_doc(colours: &[[f64; 3]], depth: u32) -> *mut PrecisionDocument {
    let rgb = rgb_doc(colours.iter().map(|c| encode(*c)).collect(), twin());
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

fn description(op: &str, params: Value) -> String {
    json!({"type":"adjust","evaluation":"precision_v1","op":op,"params":params}).to_string()
}

/// `meta` applied destructively to layer 0; `Ok(None)` is the core's
/// no-change answer.
fn apply(
    doc: *const PrecisionDocument,
    meta: &str,
) -> Result<Option<*mut PrecisionDocument>, String> {
    let meta = CString::new(meta).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_apply_adjustment(doc, 0, meta.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    result(out, err)
}

fn add(doc: *const PrecisionDocument, meta: &str) -> *mut PrecisionDocument {
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
    result(out, err).unwrap().unwrap()
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

fn chroma(lab: [f64; 3]) -> f64 {
    lab[1].hypot(lab[2])
}

fn unit16(v: f64) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

/// Hue/Saturation's master hue step: a hexcone rotation by q(d) =
/// trunc(d·24576/360)·360/24576° that keeps the pixel's max and min
/// (`adjust_hue_saturation`'s step 3), written here from the hexcone.
fn rotate(rgb: [f64; 3], degrees: f64) -> [f64; 3] {
    let q = (degrees * 24576.0 / 360.0).trunc() * 360.0 / 24576.0;
    let max = rgb.iter().cloned().fold(f64::MIN, f64::max);
    let min = rgb.iter().cloned().fold(f64::MAX, f64::min);
    let c = max - min;
    if c <= 0.0 {
        return rgb;
    }
    let [r, g, b] = rgb;
    let h = if max == r {
        60.0 * ((g - b) / c).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / c + 2.0)
    } else {
        60.0 * ((r - g) / c + 4.0)
    };
    let turned = (h + q).rem_euclid(360.0);
    let sector = turned / 60.0;
    let x = c * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
    let (r1, g1, b1) = match sector as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r1 + min, g1 + min, b1 + min]
}

/// The chroma sRGB would clip `lab` to: the hand sRGB, clamped, back to Lab.
fn srgb_clip_chroma(lab: [f64; 3]) -> f64 {
    chroma(srgb_to_lab(lab_to_srgb(lab).map(|v| v.clamp(0.0, 1.0))))
}

fn assert_survives(got: [f64; 3], want: [f64; 3], source: [f64; 3], what: &str) {
    let (c, c_want) = (chroma(got), chroma(want));
    assert!(
        (c - c_want).abs() <= CHROMA_RATIO * c_want,
        "{what}: chroma {c} vs {c_want} (got {got:?}, want {want:?})"
    );
    assert!(
        (got[0] - want[0]).abs() <= BOUND_L,
        "{what}: L {} vs {}",
        got[0],
        want[0]
    );
    let clip = srgb_clip_chroma(source);
    assert!(
        c >= OVER_CLIP * clip,
        "{what}: chroma {c} is not past {OVER_CLIP} × the sRGB clip's {clip}"
    );
}

#[test]
fn the_wide_green_is_outside_srgb_and_inside_prophoto() {
    let srgb = lab_to_srgb(S);
    assert!(srgb.iter().any(|v| !(0.0..=1.0).contains(v)), "{srgb:?}");
    let prophoto = lab_to_prophoto(S);
    assert!(
        prophoto.iter().all(|v| (0.0..=1.0).contains(v)),
        "{prophoto:?}"
    );
    let lab = lab_doc(&[S], 16);
    let source = decode(codes(lab, 0)[0]);
    for c in 0..3 {
        assert!((source[c] - S[c]).abs() <= 0.01, "{source:?}");
    }
    free(lab);
}

#[test]
fn a_five_degree_hue_shift_keeps_the_wide_green() {
    let lab = lab_doc(&[S], 16);
    let source = decode(codes(lab, 0)[0]);
    let out = apply(lab, &description("hue_saturation", json!({"hue":5})))
        .unwrap()
        .expect("the hue shift changed nothing");
    let got = decode(codes(out, 0)[0]);
    let want = prophoto_to_lab(rotate(lab_to_prophoto(source), 5.0));
    assert_survives(got, want, source, "hue 5");
    free(out);
    free(lab);
}

#[test]
fn a_color_balance_nudge_keeps_the_wide_green() {
    let lab = lab_doc(&[S], 16);
    let source = decode(codes(lab, 0)[0]);
    let meta = description(
        "color_balance",
        json!({"midtones":{"cyan_red":0.1,"magenta_green":0.05}}),
    );
    let out = apply(lab, &meta)
        .unwrap()
        .expect("the nudge changed nothing");
    let got = decode(codes(out, 0)[0]);
    // The same meta on a 16-bit ProPhoto RGB document of the hand ProPhoto.
    let p = lab_to_prophoto(source).map(unit16);
    let prophoto = rgb_doc(vec![[p[0], p[1], p[2], 65535]], IccProfile::prophoto_rgb());
    let oracle = apply(prophoto, &meta).unwrap().unwrap();
    let w = codes(oracle, 0)[0];
    let want = prophoto_to_lab([w[0], w[1], w[2]].map(|c| f64::from(c) / 65535.0));
    assert_survives(got, want, source, "color balance");
    for doc in [oracle, prophoto, out, lab] {
        free(doc);
    }
}

#[test]
fn identity_settings_keep_every_code() {
    for depth in [8, 16] {
        let lab = lab_doc(&[S, IN, [90.0, -5.0, 70.0], [20.0, 60.0, -90.0]], depth);
        let source = codes(lab, 0);
        let before = render(lab);
        let layered = [
            ("hue_saturation", json!({})),
            ("color_balance", json!({})),
            ("photo_filter", json!({"density":0})),
        ];
        for (op, params) in &layered {
            let layered = add(lab, &description(op, params.clone()));
            assert_eq!(render(layered), before, "depth {depth} {op} layer");
            free(layered);
        }
        let destructive = layered.iter().cloned().chain([
            (
                "shadows_highlights",
                json!({"shadows":{"amount":0},"highlights":{"amount":0},"color":0}),
            ),
            (
                "replace_color",
                json!({"colors":[lab_code_hex(S)],"hue":0,"saturation":0,"lightness":0}),
            ),
        ]);
        for (op, params) in destructive {
            match apply(lab, &description(op, params)).unwrap() {
                None => println!("depth {depth} {op}: no-change answer"),
                Some(out) => {
                    println!("depth {depth} {op}: every code equal");
                    assert_eq!(codes(out, 0), source, "depth {depth} {op}");
                    free(out);
                }
            }
        }
        free(lab);
    }
}

#[test]
fn photo_filters_srgb_colour_is_converted() {
    let lab = lab_doc(&[IN], 16);
    let source = codes(lab, 0)[0];
    let meta = description(
        "photo_filter",
        json!({"color":"#ec8a00","density":0.5,"preserve_luminosity":false}),
    );
    let out = apply(lab, &meta).unwrap().unwrap();
    let got = decode(codes(out, 0)[0]);
    // 2c's sRGB oracle: the op on an sRGB document of the hand sRGB.
    let rgb = lab_to_srgb(decode(source)).map(unit16);
    let srgb = rgb_doc(vec![[rgb[0], rgb[1], rgb[2], 65535]], IccProfile::srgb());
    let oracle = apply(srgb, &meta).unwrap().unwrap();
    let w = codes(oracle, 0)[0];
    let want = srgb_to_lab([w[0], w[1], w[2]].map(|c| f64::from(c) / 65535.0));
    assert!((got[0] - want[0]).abs() <= 0.3, "L {got:?} vs {want:?}");
    for c in 1..3 {
        assert!((got[c] - want[c]).abs() <= 0.5, "{c}: {got:?} vs {want:?}");
    }
    for doc in [oracle, srgb, out, lab] {
        free(doc);
    }
}

fn kernel_rgb(op: &str, lab: [f64; 3]) -> Option<[f64; 3]> {
    let op = CString::new(op).unwrap();
    let lab = lab.map(|v| v as f32);
    let mut rgb = [0.0f32; 3];
    unsafe { dr_lab_kernel_rgb(op.as_ptr(), lab.as_ptr(), rgb.as_mut_ptr()) }
        .then(|| rgb.map(f64::from))
}

#[test]
fn the_kernel_sample_is_what_the_kernel_reads() {
    let prophoto = kernel_rgb("hue_saturation", S).unwrap();
    let want = lab_to_prophoto(S);
    for c in 0..3 {
        assert!(
            (prophoto[c] - want[c]).abs() <= 1e-3,
            "{prophoto:?} vs {want:?}"
        );
    }
    let srgb = kernel_rgb("color_lookup", IN).unwrap();
    let want = lab_to_srgb(IN);
    for c in 0..3 {
        assert!((srgb[c] - want[c]).abs() <= 1e-3, "{srgb:?} vs {want:?}");
    }
    assert!(kernel_rgb("levels", IN).is_none());
    assert!(kernel_rgb("hue_saturation", [f64::NAN, 0.0, 0.0]).is_none());
}

#[test]
fn replace_colors_hex_is_the_documents_own_codes() {
    let lab = lab_doc(&[S, IN], 16);
    let source = codes(lab, 0);
    let meta = description(
        "replace_color",
        json!({"colors":[lab_code_hex(S)],"fuzziness":8,"hue":30}),
    );
    let out = apply(lab, &meta).unwrap().unwrap();
    let got = codes(out, 0);
    assert_ne!(got[0], source[0], "S is the sampled colour");
    assert_eq!(got[1], source[1], "the other sample is far from it");
    free(out);
    free(lab);
}
