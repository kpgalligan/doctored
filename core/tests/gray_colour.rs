//! #1 phase 5a, colour on a Grayscale document: Edit ▸ Assign Profile and
//! Convert to Profile between Gray profiles, Image ▸ Mode ▸ 8/16/32
//! Bits/Channel, and HDR Toning of a 32-bit Gray document
//! (`dr_precision_doc_gray_profile_edit`, `_plan`).
//!
//! The oracles are independent of the Gray arms: the RGB TWIN (the Gray
//! document converted to RGB through its own profile bytes, which keeps every
//! sample) through the RGB exports (`dr_precision_doc_convert_to_profile_with`
//! into the destination's twin, `dr_precision_doc_promote_to_float`,
//! `dr_precision_doc_hdr_toning`), each profile's own `kTRC` read here from
//! its bytes by the ICC `curveType` rule (a gamma, or a table interpolated
//! linearly and inverted by bisection), and `round(v / 257) · 257` for the
//! 8-bit grid.
//!
//! Bounds, declared before measuring: Assign keeps every sample bit for bit;
//! Convert's R and alpha equal the twin's conversion exactly (`eight` of it
//! at 8 bits), its 16-bit R within 1 code of 65535 of the curves, its 32-bit
//! samples unchanged; the depth op exact; HDR Toning exact against the
//! twin's.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_gray::{
    dr_precision_doc_gray_profile, dr_precision_doc_gray_profile_edit,
    dr_precision_doc_gray_profile_plan, dr_precision_doc_new_gray,
};
use doctored_core::ffi_hdr_toning::{
    dr_precision_doc_hdr_toning, dr_precision_doc_hdr_toning_histogram,
    dr_precision_doc_hdr_toning_plan,
};
use doctored_core::ffi_output_color::{
    dr_precision_doc_assign_profile_rasterizing, dr_precision_doc_convert_to_profile_with,
};
use doctored_core::ffi_precision::dr_precision_doc_descriptor;
use doctored_core::ffi_precision_color::{
    dr_precision_doc_assign_profile, dr_precision_doc_convert_to_profile,
};
use doctored_core::ffi_precision_conversion::dr_precision_doc_promote_to_float;
use doctored_core::ffi_precision_inspect::dr_precision_doc_profile;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::gray_fixture::gray_icc;
use crate::gray_ops::{
    assert_neutral, edit, free, gray_doc, layer, plan_json, result, twin, LIMIT,
};

const W: u32 = 24;
const H: u32 = 16;
const GENERIC_CMYK: &str = "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc";
const FLOAT_DEPTH_REFUSAL: &str = "A 32-bit Grayscale document converts to 8 or 16 Bits/Channel through HDR Toning (hdr_toning with bit_depth 8 or 16).";
const RGB_REFUSAL: &str = "a Grayscale document's Assign Profile, Convert to Profile and Bits/Channel are its own Gray profile edit (dr_precision_doc_gray_profile_edit)";
const TONING: [&str; 2] = [
    r#"{"method":"exposure_gamma","exposure":0.5,"gamma":1.2}"#,
    r#"{"method":"local_adaptation"}"#,
];

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

fn icc_ptr(icc: Option<&[u8]>) -> (*const u8, usize) {
    icc.map_or((ptr::null(), 0), |b| (b.as_ptr(), b.len()))
}

fn profile_edit(
    doc: *const PrecisionDocument,
    icc: Option<&[u8]>,
    request: &Value,
) -> Result<Option<*mut PrecisionDocument>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let (bytes, len) = icc_ptr(icc);
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_gray_profile_edit(
            doc,
            bytes,
            len,
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err)
}

fn changed(
    doc: *const PrecisionDocument,
    icc: Option<&[u8]>,
    request: &Value,
) -> *mut PrecisionDocument {
    profile_edit(doc, icc, request)
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request} changed nothing"))
}

fn profile_plan(
    doc: *const PrecisionDocument,
    icc: Option<&[u8]>,
    request: &Value,
) -> Result<Value, String> {
    let text = CString::new(request.to_string()).unwrap();
    let (bytes, len) = icc_ptr(icc);
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_gray_profile_plan(doc, bytes, len, text.as_ptr(), &mut err) };
    plan_json(out, err)
}

/// A 1 × 1 Gray document of `depth` in `icc` or `space`.
fn blank(icc: Option<&[u8]>, space: &str, depth: u32) -> *mut PrecisionDocument {
    let request = if icc.is_some() {
        json!({"depth":depth,"fill_srgb":[0.5,0.5,0.5,1]})
    } else {
        json!({"depth":depth,"space":space,"fill_srgb":[0.5,0.5,0.5,1]})
    };
    let text = CString::new(request.to_string()).unwrap();
    let (bytes, len) = icc_ptr(icc);
    let mut err = ptr::null_mut();
    let doc =
        unsafe { dr_precision_doc_new_gray(1, 1, bytes, len, text.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

/// The RGB twin's bytes of a Gray document in `icc` or `space`.
fn twin_bytes(icc: Option<&[u8]>, space: &str) -> Vec<u8> {
    let doc = blank(icc, space, 16);
    let mut length = 0;
    let bytes = unsafe { dr_precision_doc_profile(doc, &mut length) };
    let out = unsafe { std::slice::from_raw_parts(bytes, length) }.to_vec();
    free(doc);
    out
}

/// `dr_precision_doc_convert_to_profile_with` of `doc` into `profile` at
/// the default options; `doc` itself when nothing changes.
fn rgb_convert(doc: *const PrecisionDocument, profile: &[u8]) -> Vec<[u32; 4]> {
    let mut err = ptr::null_mut();
    let mut report = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_profile_with(
            doc,
            profile.as_ptr(),
            profile.len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut report,
            &mut err,
        )
    };
    if !report.is_null() {
        unsafe { dr_string_free(report) };
    }
    match result(out, err).unwrap() {
        Some(converted) => {
            let samples = layer(converted, 0, 16);
            free(converted);
            samples
        }
        None => layer(doc, 0, 16),
    }
}

fn eight(v: u32) -> u32 {
    (v + 128) / 257 * 257
}

/// A Gray profile's `kTRC` as the ICC `curv` type defines it: one entry is
/// a u8Fixed8 gamma, more are a table over [0, 1] interpolated linearly,
/// each entry `/ 65535` (ICC.1, `curveType`). Parsed here from the bytes.
enum Curve {
    Gamma(f64),
    Table(Vec<f64>),
}

fn be32(b: &[u8], at: usize) -> usize {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as usize
}

fn ktrc(icc: &[u8]) -> Curve {
    let count = be32(icc, 128);
    let entry = (0..count)
        .map(|i| 132 + 12 * i)
        .find(|e| &icc[*e..*e + 4] == b"kTRC")
        .expect("a kTRC tag");
    let data = &icc[be32(icc, entry + 4)..];
    assert_eq!(&data[..4], b"curv");
    let n = be32(data, 8);
    let entry = |i: usize| f64::from(u16::from_be_bytes([data[12 + 2 * i], data[13 + 2 * i]]));
    match n {
        1 => Curve::Gamma(entry(0) / 256.0),
        _ => Curve::Table((0..n).map(|i| entry(i) / 65535.0).collect()),
    }
}

/// Relative luminance of the unit code `v` through `curve`.
fn decode(curve: &Curve, v: f64) -> f64 {
    match curve {
        Curve::Gamma(g) => v.powf(*g),
        Curve::Table(t) => {
            let x = v.clamp(0.0, 1.0) * (t.len() - 1) as f64;
            let i = (x.floor() as usize).min(t.len() - 2);
            t[i] + (t[i + 1] - t[i]) * (x - i as f64)
        }
    }
}

/// The unit code whose luminance through `curve` is `y` (bisection on the
/// monotonic curve).
fn encode(curve: &Curve, y: f64) -> f64 {
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if decode(curve, mid) < y {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// The Gray ICC bytes of `doc`.
fn gray_bytes(doc: *const PrecisionDocument) -> Vec<u8> {
    let mut length = 0;
    let bytes = unsafe { dr_precision_doc_gray_profile(doc, &mut length) };
    assert!(!bytes.is_null());
    unsafe { std::slice::from_raw_parts(bytes, length) }.to_vec()
}

/// The Convert request into `space` (`"icc"`: the fixture ICC) and its ICC.
fn target(space: &str, icc: &[u8]) -> (Value, Option<Vec<u8>>) {
    if space == "icc" {
        (json!({"op":"convert"}), Some(icc.to_vec()))
    } else {
        (json!({"op":"convert","space":space}), None)
    }
}

#[test]
fn assign_keeps_every_sample_and_names_the_new_profile() {
    let named = blank(None, "dot_gain_20", 16);
    let dot_gain = descriptor(named)["gray"]["profile"].clone();
    free(named);
    for depth in [8, 16, 32] {
        let gray = gray_doc(W, H, depth);
        let request = json!({"op":"assign","space":"dot_gain_20"});
        let plan = profile_plan(gray, None, &request).unwrap();
        assert_eq!(plan["refusal"], Value::Null, "{depth}");
        assert_eq!(plan["changes"], true, "{depth}");
        let assigned = changed(gray, None, &request);
        assert_eq!(layer(assigned, 0, depth), layer(gray, 0, depth), "{depth}");
        let d = descriptor(assigned);
        assert_eq!(d["color_mode"], "gray");
        assert_eq!(d["gray"]["depth"], depth);
        assert_eq!(d["gray"]["profile"], dot_gain, "{depth}");
        let same = json!({"op":"assign","space":"gray_gamma_22"});
        assert_eq!(profile_plan(gray, None, &same).unwrap()["changes"], false);
        assert!(
            profile_edit(gray, None, &same).unwrap().is_none(),
            "{depth}"
        );
        free(assigned);
        free(gray);
    }
}

#[test]
fn an_rgb_or_cmyk_profile_refuses_by_name() {
    let gray = gray_doc(W, H, 16);
    let srgb = IccProfile::srgb();
    let sentence = |name: &str, kind: &str| {
        format!(
            "“{name}” is {kind} profile: a Grayscale document is assigned and converted to Gray profiles only."
        )
    };
    let rgb_sentence = sentence(srgb.name(), "an RGB");
    for op in ["assign", "convert"] {
        let request = json!({ "op": op });
        assert_eq!(
            profile_edit(gray, Some(srgb.bytes()), &request).unwrap_err(),
            rgb_sentence
        );
        assert_eq!(
            profile_plan(gray, Some(srgb.bytes()), &request).unwrap()["refusal"],
            rgb_sentence.as_str()
        );
    }
    match std::fs::read(GENERIC_CMYK) {
        Ok(cmyk) => {
            let name = doctored_core::icc_lut::ProofProfile::parse(&cmyk)
                .unwrap()
                .name()
                .to_string();
            assert_eq!(
                profile_edit(gray, Some(&cmyk), &json!({"op":"convert"})).unwrap_err(),
                sentence(&name, "a CMYK")
            );
        }
        Err(_) => eprintln!("skipped the CMYK case: {GENERIC_CMYK} is absent"),
    }
    free(gray);
}

#[test]
fn convert_equals_the_twins_conversion_and_the_curves() {
    let icc = gray_icc("Gamma 1.8", 1.8);
    // (source space, destination space); "icc" is the fixture ICC.
    let cases = [
        ("gray_gamma_22", "dot_gain_20"),
        ("gray_gamma_22", "sgray"),
        ("gray_gamma_22", "icc"),
        ("icc", "gray_gamma_22"),
    ];
    for depth in [8, 16] {
        for (from, to) in cases {
            let what = format!("{from} -> {to} at {depth}");
            let base = gray_doc(W, H, depth);
            let source = if from == "icc" {
                changed(base, Some(&icc), &json!({"op":"assign"}))
            } else {
                gray_doc(W, H, depth)
            };
            free(base);
            let (request, bytes) = target(to, &icc);
            let destination = twin_bytes(bytes.as_deref(), to);
            let converted = changed(source, bytes.as_deref(), &request);
            let curve_a = ktrc(&gray_bytes(source));
            let curve_b = ktrc(&gray_bytes(converted));
            let d = descriptor(converted);
            assert_eq!(d["color_mode"], "gray", "{what}");
            assert_eq!(d["gray"]["depth"], depth, "{what}");
            let out = layer(converted, 0, depth);
            assert_neutral(&out, &what);
            let rgb = twin(source);
            let oracle = rgb_convert(rgb, &destination);
            let before = layer(source, 0, depth);
            for (i, ((p, q), s)) in out.iter().zip(&oracle).zip(&before).enumerate() {
                let r = if depth == 8 { eight(q[0]) } else { q[0] };
                assert_eq!((p[0], p[3]), (r, q[3]), "{what} pixel {i}");
                if depth == 16 {
                    let want =
                        encode(&curve_b, decode(&curve_a, f64::from(s[0]) / 65535.0)) * 65535.0;
                    assert!(
                        (f64::from(p[0]) - want).abs() <= 1.0,
                        "{what} pixel {i}: {} against {want}",
                        p[0]
                    );
                }
            }
            free(rgb);
            free(converted);
            free(source);
        }
    }
    // At 32 bits linear Y is the same under every Gray profile.
    let float = gray_doc(W, H, 32);
    let converted = changed(float, None, &json!({"op":"convert","space":"dot_gain_20"}));
    assert_eq!(layer(converted, 0, 32), layer(float, 0, 32));
    assert_eq!(descriptor(converted)["gray"]["depth"], 32);
    free(converted);
    free(float);
}

/// `doc` with a layer mask on layer 0 and one alpha channel, both of
/// off-grid samples.
fn with_planes(doc: *mut PrecisionDocument) -> *mut PrecisionDocument {
    let mask: Vec<u16> = (0..W * H)
        .map(|i| ((i * 977 + 13) % 65536) as u16)
        .collect();
    let masked = edit(
        doc,
        &json!({"op":"set_mask","layer":0,"width":W,"height":H,"x":0,"y":0,"linked":true}),
        &mask,
    )
    .unwrap()
    .unwrap();
    let channel: Vec<u16> = (0..W * H)
        .map(|i| ((i * 4099 + 7) % 65536) as u16)
        .collect();
    let out = edit(
        masked,
        &json!({"op":"set_channel","index":null,"name":"Alpha 1"}),
        &channel,
    )
    .unwrap()
    .unwrap();
    free(masked);
    free(doc);
    out
}

fn mask_samples(doc: *const PrecisionDocument) -> Vec<u16> {
    let layer = unsafe { &*doc }.layer(0).unwrap();
    layer.mask().unwrap().plane().samples().to_vec()
}

fn channel_samples(doc: *const PrecisionDocument) -> Vec<u16> {
    unsafe { &*doc }.channels()[0].plane().samples().to_vec()
}

fn depth(bits: u32) -> Value {
    json!({"op":"depth","depth":bits})
}

#[test]
fn bits_per_channel_rounds_relabels_and_promotes() {
    let sixteen = with_planes(gray_doc(W, H, 16));
    let eight_bit = changed(sixteen, None, &depth(8));
    assert_eq!(descriptor(eight_bit)["gray"]["depth"], 8);
    let rounded: Vec<[u32; 4]> = layer(sixteen, 0, 16).iter().map(|p| p.map(eight)).collect();
    assert_eq!(layer(eight_bit, 0, 16), rounded, "raster");
    let grid =
        |v: &Vec<u16>| -> Vec<u16> { v.iter().map(|s| eight(u32::from(*s)) as u16).collect() };
    assert_eq!(
        mask_samples(eight_bit),
        grid(&mask_samples(sixteen)),
        "mask"
    );
    assert_eq!(
        channel_samples(eight_bit),
        grid(&channel_samples(sixteen)),
        "channel"
    );
    // 8 → 16 relabels; 8 → 16 → 8 is the identity.
    let back = changed(eight_bit, None, &depth(16));
    assert_eq!(descriptor(back)["gray"]["depth"], 16);
    assert_eq!(layer(back, 0, 16), layer(eight_bit, 0, 16));
    assert_eq!(mask_samples(back), mask_samples(eight_bit));
    let again = changed(back, None, &depth(8));
    assert_eq!(layer(again, 0, 16), layer(eight_bit, 0, 16));
    assert_eq!(channel_samples(again), channel_samples(eight_bit));
    // The same depth changes nothing.
    assert_eq!(
        profile_plan(sixteen, None, &depth(16)).unwrap()["changes"],
        false
    );
    assert!(profile_edit(sixteen, None, &depth(16)).unwrap().is_none());
    for d in [again, back, eight_bit, sixteen] {
        free(d);
    }
    // 16 → 32 is the twin's promotion.
    let gray = gray_doc(W, H, 16);
    let float = changed(gray, None, &depth(32));
    let d = descriptor(float);
    assert_eq!(d["color_mode"], "gray");
    assert_eq!(d["gray"]["depth"], 32);
    let rgb = twin(gray);
    let mut err = ptr::null_mut();
    let promoted =
        unsafe { dr_precision_doc_promote_to_float(rgb, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!promoted.is_null(), "{}", take_err_string(err));
    let out = layer(float, 0, 32);
    assert_neutral(&out, "16 -> 32");
    for (i, (p, q)) in out.iter().zip(&layer(promoted, 0, 32)).enumerate() {
        assert_eq!((p[0], p[3]), (q[0], q[3]), "16 -> 32 pixel {i}");
    }
    // 32 → 16 is HDR Toning's.
    assert_eq!(
        profile_edit(float, None, &depth(16)).unwrap_err(),
        FLOAT_DEPTH_REFUSAL
    );
    assert_eq!(
        profile_plan(float, None, &depth(8)).unwrap()["refusal"],
        FLOAT_DEPTH_REFUSAL
    );
    for d in [promoted, rgb, float, gray] {
        free(d);
    }
}

fn tone(doc: *const PrecisionDocument, request: &str) -> *mut PrecisionDocument {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_hdr_toning(doc, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    result(out, err)
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .expect("toning answers a document")
}

fn tone_plan(doc: *const PrecisionDocument, request: &str, bits: u32) -> Result<Value, String> {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_hdr_toning_plan(doc, text.as_ptr(), bits, &mut err) };
    plan_json(out, err)
}

fn working(plan: &Value) -> u64 {
    plan["working_memory_plan_bytes"].as_u64().unwrap()
}

#[test]
fn hdr_toning_a_32_bit_gray_document_equals_the_twins() {
    let float = gray_doc(W, H, 32);
    let rgb = twin(float);
    for request in TONING {
        let toned = tone(float, request);
        let d = descriptor(toned);
        assert_eq!(d["color_mode"], "gray", "{request}");
        assert_eq!(d["gray"]["depth"], 16, "{request}");
        let out = layer(toned, 0, 16);
        assert_neutral(&out, request);
        let oracle = tone(rgb, request);
        for (i, (p, q)) in out.iter().zip(&layer(oracle, 0, 16)).enumerate() {
            assert_eq!((p[0], p[3]), (q[0], q[3]), "{request} pixel {i}");
        }
        let sixteen = tone_plan(float, request, 16).unwrap();
        let eight_bit = tone_plan(float, request, 8).unwrap();
        assert_eq!(sixteen["output_bytes"], u64::from(W * H) * 8);
        assert_eq!(eight_bit["output_bytes"], u64::from(W * H) * 8);
        let narrow = profile_plan(toned, None, &depth(8)).unwrap();
        assert!(working(&eight_bit) >= working(&sixteen), "{request}");
        assert!(working(&eight_bit) >= working(&narrow), "{request}");
        assert_eq!(
            tone_plan(float, request, 32).unwrap_err(),
            "hdr toning converts a 32-bit Grayscale document to 8 or 16 bits per channel; 32 was given"
        );
        free(oracle);
        free(toned);
    }
    let mut err = ptr::null_mut();
    let histogram =
        unsafe { dr_precision_doc_hdr_toning_histogram(float, LIMIT, 0, ptr::null(), &mut err) };
    assert_eq!(
        plan_json(histogram, err).unwrap()["bins"]
            .as_array()
            .unwrap()
            .len(),
        256
    );
    free(rgb);
    free(float);
}

#[test]
fn sixteen_to_thirty_two_then_hdr_toning_is_the_twins_chain() {
    let gray = gray_doc(W, H, 16);
    let float = changed(gray, None, &depth(32));
    let toned = tone(float, TONING[0]);
    let rgb = twin(gray);
    let mut err = ptr::null_mut();
    let promoted =
        unsafe { dr_precision_doc_promote_to_float(rgb, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!promoted.is_null(), "{}", take_err_string(err));
    let oracle = tone(promoted, TONING[0]);
    for (i, (p, q)) in layer(toned, 0, 16)
        .iter()
        .zip(&layer(oracle, 0, 16))
        .enumerate()
    {
        assert_eq!(p[0], q[0], "pixel {i}");
    }
    for d in [oracle, promoted, rgb, toned, float, gray] {
        free(d);
    }
}

#[test]
fn the_rgb_profile_exports_refuse_a_gray_document() {
    let gray = gray_doc(W, H, 16);
    let srgb = IccProfile::srgb();
    let (bytes, len) = (srgb.bytes().as_ptr(), srgb.bytes().len());
    let refusal = |out: *mut PrecisionDocument, err: *mut c_char| {
        assert!(out.is_null());
        assert_eq!(take_err_string(err), RGB_REFUSAL);
    };
    unsafe {
        let mut err = ptr::null_mut();
        let out =
            dr_precision_doc_assign_profile(gray, bytes, len, LIMIT, 0, ptr::null(), &mut err);
        refusal(out, err);
        let mut err = ptr::null_mut();
        let out =
            dr_precision_doc_convert_to_profile(gray, bytes, len, LIMIT, 0, ptr::null(), &mut err);
        refusal(out, err);
        let mut err = ptr::null_mut();
        let mut report = ptr::null_mut();
        let out = dr_precision_doc_convert_to_profile_with(
            gray,
            bytes,
            len,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut report,
            &mut err,
        );
        assert!(report.is_null());
        refusal(out, err);
        let mut err = ptr::null_mut();
        let out = dr_precision_doc_assign_profile_rasterizing(
            gray,
            bytes,
            len,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        );
        refusal(out, err);
    }
    free(gray);
}

#[test]
fn the_gray_profile_exports_tolerate_null() {
    unsafe {
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_gray_profile_plan(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        dr_string_free(err);
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_gray_profile_edit(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        dr_string_free(err);
        assert!(dr_precision_doc_gray_profile_plan(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_doc_gray_profile_edit(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
    }
    // A request on an RGB document refuses.
    let gray = gray_doc(W, H, 16);
    let rgb = twin(gray);
    assert_eq!(
        profile_edit(rgb, None, &depth(8)).unwrap_err(),
        "8 Bits/Channel requires a Grayscale document"
    );
    assert_eq!(
        profile_edit(gray, None, &json!({"op":"depth"})).unwrap_err(),
        "a depth change needs \"depth\": 8, 16 or 32"
    );
    free(rgb);
    free(gray);
}
