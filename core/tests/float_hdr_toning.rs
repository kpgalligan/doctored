//! HDR Toning through the FFI (`dr_precision_doc_hdr_toning`,
//! the two plans): the three global methods, the flatten, the depth rules,
//! the refusals, the cancel and the NULL sweep.
//!
//! # The oracles
//!
//! Written here from `hdr_toning.rs`'s module doc, sharing no line with the
//! kernel: Exposure and Gamma as `clamp(sign(u)·|u|^(1/γ), 0, 1)` of
//! `u = v · 2^e`, the extended Reinhard curve with `W` the largest RGB
//! sample, and Equalize Histogram's table built again in f64 over 65536
//! levels of `log2(max(v, 2⁻²⁰))` across the image's own log range. The
//! linear results meet the sRGB profile's own forward curve ([`encoded`]:
//! Convert Precision's `dr_precision_doc_float_to_u16` under the clip map),
//! which is not under test; Equalize Histogram's table is the encoded value
//! itself. Bound: 1 code of 65535 per channel (f64 here, f32 in the core).
//! Exposure and Gamma at (0, 1) must equal `dr_precision_doc_float_to_u16`
//! BIT FOR BIT.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_float::dr_precision_doc_float_to_u16;
use doctored_core::ffi_hdr_toning::{
    dr_doc_hdr_toning_plan, dr_precision_doc_hdr_toning, dr_precision_doc_hdr_toning_plan,
};
use doctored_core::ffi_precision::{dr_precision_doc_free, dr_precision_doc_open};
use doctored_core::float_document::ToneMap;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::cmyk_tiff_builder::cmyk_tiff;
use crate::common::{cpath, take_err_string};
use crate::float_fixture::float_doc;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 16;
const H: u32 = 4;

// ----------------------------------------------------------------- oracles --

/// Exposure and Gamma in linear light, clipped: `u = v · 2^e`, the signed
/// reciprocal power.
fn eg_oracle(v: f64, exposure: f64, gamma: f64) -> f64 {
    let u = v * exposure.exp2();
    (u.signum() * u.abs().powf(1.0 / gamma)).clamp(0.0, 1.0)
}

/// The extended Reinhard curve at white `white`.
fn hc_oracle(v: f64, white: f64) -> f64 {
    let u = v.max(0.0);
    (u * (1.0 + u / (white * white)) / (1.0 + u)).min(1.0)
}

/// Linear triples in [0, 1] through the sRGB profile's own forward curve:
/// a float document of them converted by `dr_precision_doc_float_to_u16`
/// under the clip map. The encode is not what is under test (the identity
/// test pins it), so the oracle's tone math meets the core's curve.
fn encoded(values: &[[f64; 3]]) -> Vec<[i64; 3]> {
    let pixels: Vec<[f32; 4]> = values
        .iter()
        .map(|v| [v[0] as f32, v[1] as f32, v[2] as f32, 1.0])
        .collect();
    let doc = float_scene(&[pixels]);
    let mut error = ptr::null_mut();
    let out = unsafe { dr_precision_doc_float_to_u16(&doc, LIMIT, 0, ptr::null(), &mut error) };
    assert!(!out.is_null(), "{}", take_err_string(error));
    let out = *unsafe { Box::from_raw(out) };
    let codes = out.layer(0).unwrap().source().unwrap().to_rgba16().unwrap();
    codes
        .iter()
        .map(|p| [0, 1, 2].map(|c| i64::from(p[c])))
        .collect()
}

/// Equalize Histogram's per-sample codes, built again from the rule.
fn eh_oracle(pixels: &[[f32; 4]]) -> Vec<[i64; 3]> {
    let log = |v: f32| f64::from(v).max(2f64.powi(-20)).log2();
    let opaque = pixels.iter().filter(|p| p[3] > 0.0);
    let logs: Vec<f64> = opaque
        .flat_map(|p| p[..3].iter().map(|v| log(*v)))
        .collect();
    let low = logs.iter().copied().fold(f64::INFINITY, f64::min);
    let high = logs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let level =
        |v: f32| (((log(v) - low) / (high - low)).clamp(0.0, 1.0) * 65535.0).round() as usize;
    let mut bins = vec![0.0f64; 65536];
    for p in pixels.iter().filter(|p| p[3] > 0.0) {
        for v in &p[..3] {
            bins[level(*v)] += f64::from(p[3]);
        }
    }
    let first = bins.iter().position(|w| *w > 0.0).unwrap();
    let total: f64 = bins.iter().sum();
    let floor = bins[first];
    let mut cdf = vec![0.0f64; 65536];
    let mut running = 0.0;
    for (i, w) in bins.iter().enumerate() {
        running += w;
        cdf[i] = running;
    }
    pixels
        .iter()
        .map(|p| {
            [0, 1, 2].map(|c| {
                let t = ((cdf[level(p[c])] - floor) / (total - floor)).clamp(0.0, 1.0);
                (65535.0 * t).round() as i64
            })
        })
        .collect()
}

// ---------------------------------------------------------------- fixtures --

/// An opaque record whose channels run over about 22 stops, −0.25 to ~300,
/// each channel on its own phase, with one negative sample.
fn scene() -> Vec<[f32; 4]> {
    (0..W * H)
        .map(|i| {
            let t = i as f32 / (W * H - 1) as f32;
            let stop = |shift: f32| (-12.0 + 20.0 * ((t + shift) % 1.0)).exp2();
            let red = if i == 5 { -0.25 } else { stop(0.0) };
            [red, stop(0.31), stop(0.67) * 1.5, 1.0]
        })
        .collect()
}

fn float_scene(records: &[Vec<[f32; 4]>]) -> PrecisionDocument {
    let doc = float_doc(W, H, &IccProfile::srgb(), records);
    match doc.with_tone_map(ToneMap::Clip).unwrap() {
        Some(clip) => clip,
        None => doc,
    }
}

fn u16_document(codes: Vec<[u16; 4]>) -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let image = PrecisionImage::from_rgba16(W, H, codes, profile.clone(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("Layer 1", image, (0, 0), 1.0, true).unwrap();
    PrecisionDocument::from_parts(
        W,
        H,
        profile,
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap()
}

fn cmyk_document(dir: &TempDir) -> *mut PrecisionDocument {
    let data: Vec<u8> = (0..W * H * 4)
        .flat_map(|i| ((i * 997) as u16).to_le_bytes())
        .collect();
    let path = dir.path().join("cmyk.tif");
    let icc = press_profile("Press Select");
    std::fs::write(&path, cmyk_tiff(W, H, 16, 4, 0, &data, Some(&icc))).unwrap();
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!doc.is_null(), "open: {}", take_err_string(err));
    doc
}

// --------------------------------------------------------------------- ffi --

fn taken(pointer: *mut c_char) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    Some(text)
}

fn toned_with(
    doc: &PrecisionDocument,
    request: &Value,
    control: Option<&DrJobControl>,
) -> Result<PrecisionDocument, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let control = control.map_or(ptr::null(), |c| c as *const DrJobControl);
    let out =
        unsafe { dr_precision_doc_hdr_toning(doc, text.as_ptr(), LIMIT, 0, control, &mut error) };
    if out.is_null() {
        return Err(taken(error).expect("a NULL result names its reason"));
    }
    assert!(error.is_null(), "a result comes with no error");
    Ok(*unsafe { Box::from_raw(out) })
}

fn toned(doc: &PrecisionDocument, request: &Value) -> PrecisionDocument {
    toned_with(doc, request, None).unwrap_or_else(|e| panic!("{request}: {e}"))
}

fn plan(doc: &PrecisionDocument, request: &Value, bits: u32) -> Result<Value, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe { dr_precision_doc_hdr_toning_plan(doc, text.as_ptr(), bits, &mut error) };
    match taken(out) {
        Some(reply) => Ok(serde_json::from_str(&reply).unwrap()),
        None => Err(taken(error).expect("a NULL plan names its reason")),
    }
}

/// The one raster's RGBA16 codes, after asserting the result is flattened.
fn codes(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    assert_eq!(doc.layer_count(), 1, "the result is one raster");
    let layer = doc.layer(0).unwrap();
    assert_eq!(layer.name(), "Background");
    assert!(!doc.is_float(), "the result is 16-bit");
    layer.source().unwrap().to_rgba16().unwrap()
}

fn assert_within_one(got: &[[u16; 4]], want: impl Fn(usize, usize) -> i64, label: &str) {
    for (i, pixel) in got.iter().enumerate() {
        for (c, value) in pixel[..3].iter().enumerate() {
            let expected = want(i, c);
            assert!(
                (i64::from(*value) - expected).abs() <= 1,
                "{label}: sample {i} channel {c}: {value} against {expected}"
            );
        }
        assert_eq!(pixel[3], 65535, "{label}: alpha {i} is kept");
    }
}

// ------------------------------------------------------------------- tests --

#[test]
fn exposure_gamma_at_identity_is_convert_precision_bit_for_bit() {
    let doc = float_scene(&[scene()]);
    let ours = codes(&toned(&doc, &json!({"method": "exposure_gamma"})));
    let mut error = ptr::null_mut();
    let theirs = unsafe { dr_precision_doc_float_to_u16(&doc, LIMIT, 0, ptr::null(), &mut error) };
    assert!(!theirs.is_null(), "{}", take_err_string(error));
    let theirs = *unsafe { Box::from_raw(theirs) };
    let theirs = theirs
        .layer(0)
        .unwrap()
        .source()
        .unwrap()
        .to_rgba16()
        .unwrap();
    assert_eq!(ours, theirs);
    let explicit = codes(&toned(
        &doc,
        &json!({"method": "exposure_gamma", "exposure": 0, "gamma": 1}),
    ));
    assert_eq!(explicit, theirs);
}

#[test]
fn exposure_gamma_follows_the_formula() {
    let pixels = scene();
    let doc = float_scene(std::slice::from_ref(&pixels));
    for (exposure, gamma) in [(1.5, 1.8), (-2.0, 0.6), (3.0, 1.0), (0.0, 2.2)] {
        let got = codes(&toned(
            &doc,
            &json!({"method": "exposure_gamma", "exposure": exposure, "gamma": gamma}),
        ));
        let linear: Vec<[f64; 3]> = pixels
            .iter()
            .map(|p| [0, 1, 2].map(|c| eg_oracle(f64::from(p[c]), exposure, gamma)))
            .collect();
        let want = encoded(&linear);
        assert_within_one(
            &got,
            |i, c| want[i][c],
            &format!("EG ({exposure}, {gamma})"),
        );
    }
}

#[test]
fn highlight_compression_is_reinhard_at_the_brightest_sample() {
    let pixels = scene();
    let white = pixels
        .iter()
        .flat_map(|p| p[..3].iter().copied())
        .fold(0.0f32, f32::max);
    assert!(white > 1.0);
    let doc = float_scene(std::slice::from_ref(&pixels));
    let got = codes(&toned(&doc, &json!({"method": "highlight_compression"})));
    let linear: Vec<[f64; 3]> = pixels
        .iter()
        .map(|p| [0, 1, 2].map(|c| hc_oracle(f64::from(p[c]), f64::from(white))))
        .collect();
    let want = encoded(&linear);
    assert_within_one(&got, |i, c| want[i][c], "HC");
    // The brightest sample lands at white.
    assert!(got.iter().flat_map(|p| p[..3].iter()).any(|v| *v == 65535));
    // An image at or below 1 compresses against white 1.
    let dim: Vec<[f32; 4]> = pixels
        .iter()
        .map(|p| [p[0].min(0.8), 0.5, 0.25, 1.0])
        .collect();
    let got = codes(&toned(
        &float_scene(std::slice::from_ref(&dim)),
        &json!({"method": "highlight_compression"}),
    ));
    let linear: Vec<[f64; 3]> = dim
        .iter()
        .map(|p| [0, 1, 2].map(|c| hc_oracle(f64::from(p[c]), 1.0)))
        .collect();
    let want = encoded(&linear);
    assert_within_one(&got, |i, c| want[i][c], "HC dim");
}

#[test]
fn equalize_histogram_is_the_log_histogram_table() {
    let pixels = scene();
    let doc = float_scene(std::slice::from_ref(&pixels));
    let got = codes(&toned(&doc, &json!({"method": "equalize_histogram"})));
    let want = eh_oracle(&pixels);
    assert_within_one(&got, |i, c| want[i][c], "EH");
    // The darkest level maps to 0 and the brightest to full scale.
    let flat: Vec<u16> = got.iter().flat_map(|p| p[..3].to_vec()).collect();
    assert_eq!(flat.iter().min(), Some(&0));
    assert_eq!(flat.iter().max(), Some(&65535));
    // One level only: the clip and the curve.
    let flat_scene = vec![[0.5f32, 0.5, 0.5, 1.0]; (W * H) as usize];
    let got = codes(&toned(
        &float_scene(&[flat_scene]),
        &json!({"method": "equalize_histogram"}),
    ));
    let want = encoded(&vec![[0.5; 3]; (W * H) as usize]);
    assert_within_one(&got, |_, c| want[0][c], "EH one level");
}

#[test]
fn a_16_bit_source_keeps_its_codes_at_identity() {
    let source: Vec<[u16; 4]> = (0..W * H)
        .map(|i| {
            let v = (i * 1031 % 65536) as u16;
            [v, 65535 - v, v / 3, 65535]
        })
        .collect();
    let doc = u16_document(source.clone());
    let got = codes(&toned(&doc, &json!({"method": "exposure_gamma"})));
    assert_within_one(&got, |i, c| i64::from(source[i][c]), "16-bit EG (0, 1)");
    // The other methods run on a 16-bit source too.
    for method in ["highlight_compression", "equalize_histogram"] {
        codes(&toned(&doc, &json!({ "method": method })));
    }
    let planned = plan(&doc, &json!({"method": "exposure_gamma"}), 16).unwrap();
    assert_eq!(planned["output_bytes"], json!(u64::from(W * H) * 8));
    assert!(planned["working_memory_plan_bytes"].as_u64().unwrap() > 0);
}

#[test]
fn a_layered_source_flattens_to_one_background() {
    let half: Vec<[f32; 4]> = (0..W * H)
        .map(|i| {
            if i % W < W / 2 {
                [4.0, 0.2, 0.1, 1.0]
            } else {
                [0.0; 4]
            }
        })
        .collect();
    let doc = float_scene(&[scene(), half]);
    assert_eq!(doc.layer_count(), 2);
    let out = toned(&doc, &json!({"method": "highlight_compression"}));
    codes(&out);
    assert_eq!(out.dimensions(), (W, H));
    assert_eq!(doc.layer_count(), 2, "the source is unchanged");
}

#[test]
fn the_plans_state_both_depths() {
    let doc = float_scene(&[scene()]);
    let request = json!({"method": "equalize_histogram"});
    let n = u64::from(W * H);
    let sixteen = plan(&doc, &request, 16).unwrap();
    assert_eq!(sixteen["output_bytes"], json!(n * 8));
    let eight = plan(&doc, &request, 8).unwrap();
    assert_eq!(eight["output_bytes"], json!(n * 4));
    assert!(
        eight["working_memory_plan_bytes"].as_u64()
            >= sixteen["working_memory_plan_bytes"].as_u64()
    );
    assert!(plan(&doc, &request, 12)
        .unwrap_err()
        .contains("8 or 16 bits"));
    let u16_doc = u16_document(vec![[1000, 2000, 3000, 65535]; n as usize]);
    assert_eq!(
        plan(&u16_doc, &request, 8).unwrap_err(),
        "a 16-bit document keeps its depth through hdr toning"
    );
    // An 8-bit document's plan: promote, tone, narrow.
    let byte = DrDocument::from_pixels(RgbaImage::from_pixel(W, H, Rgba([10, 20, 30, 255])));
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let reply = taken(unsafe { dr_doc_hdr_toning_plan(&byte, text.as_ptr(), &mut error) })
        .unwrap_or_else(|| panic!("{}", take_err_string(error)));
    let reply: Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(reply["output_bytes"], json!(n * 4));
    assert!(reply["working_memory_plan_bytes"].as_u64().unwrap() > n * 24);
}

#[test]
fn refusals_name_their_reason() {
    let doc = float_scene(&[scene()]);
    let refused = |request: Value| toned_with(&doc, &request, None).err().expect("a refusal");
    assert!(refused(json!({"method": "exposure_gamma", "radius": 3})).contains("\"radius\""));
    assert!(refused(json!({"method": "highlight_compression", "gamma": 1})).contains("\"gamma\""));
    assert!(refused(json!({"method": "exposure_gamma", "gamma": 0})).contains("gamma"));
    assert!(refused(json!({"method": "exposure_gamma", "exposure": 21})).contains("exposure"));
    assert!(refused(json!({"method": "posterize"})).contains("posterize"));
    assert!(refused(json!({})).contains("method"));
    let dir = TempDir::new().unwrap();
    let cmyk = cmyk_document(&dir);
    let request = json!({"method": "exposure_gamma"});
    let result = toned_with(unsafe { &*cmyk }, &request, None);
    assert_eq!(
        result.err().expect("a refusal"),
        "hdr toning is unavailable in a CMYK document"
    );
    assert_eq!(
        plan(unsafe { &*cmyk }, &request, 16).unwrap_err(),
        "hdr toning is unavailable in a CMYK document"
    );
    unsafe { dr_precision_doc_free(cmyk) };
}

#[test]
fn a_cancelled_control_answers_operation_cancelled() {
    let doc = float_scene(&[scene()]);
    let control = DrJobControl::default();
    control.cancel();
    let refused = toned_with(
        &doc,
        &json!({"method": "equalize_histogram"}),
        Some(&control),
    );
    assert_eq!(refused.err().expect("a refusal"), "operation cancelled");
    let control = DrJobControl::default();
    toned_with(
        &doc,
        &json!({"method": "equalize_histogram"}),
        Some(&control),
    )
    .unwrap();
}

#[test]
fn every_hdr_toning_export_tolerates_null() {
    let doc = float_scene(&[scene()]);
    let byte = DrDocument::from_pixels(RgbaImage::new(2, 2));
    let request = CString::new(json!({"method": "exposure_gamma"}).to_string()).unwrap();
    unsafe {
        for (typed, bytes, text) in [
            (ptr::null(), ptr::null(), request.as_ptr()),
            (
                &doc as *const PrecisionDocument,
                &byte as *const DrDocument,
                ptr::null(),
            ),
        ] {
            let mut error = ptr::null_mut();
            assert!(dr_precision_doc_hdr_toning_plan(typed, text, 16, &mut error).is_null());
            assert!(taken(error).is_some());
            assert!(dr_precision_doc_hdr_toning_plan(typed, text, 16, ptr::null_mut()).is_null());
            let mut error = ptr::null_mut();
            assert!(dr_doc_hdr_toning_plan(bytes, text, &mut error).is_null());
            assert!(taken(error).is_some());
            assert!(dr_doc_hdr_toning_plan(bytes, text, ptr::null_mut()).is_null());
            let mut error = ptr::null_mut();
            let out = dr_precision_doc_hdr_toning(typed, text, LIMIT, 0, ptr::null(), &mut error);
            assert!(out.is_null());
            assert!(taken(error).is_some());
            let out =
                dr_precision_doc_hdr_toning(typed, text, LIMIT, 0, ptr::null(), ptr::null_mut());
            assert!(out.is_null());
        }
        // NULL control and err_out on calls that succeed.
        let text = request.as_ptr();
        assert!(taken(dr_precision_doc_hdr_toning_plan(
            &doc,
            text,
            16,
            ptr::null_mut()
        ))
        .is_some());
        assert!(taken(dr_doc_hdr_toning_plan(&byte, text, ptr::null_mut())).is_some());
        let out = dr_precision_doc_hdr_toning(&doc, text, LIMIT, 0, ptr::null(), ptr::null_mut());
        assert!(!out.is_null());
        dr_precision_doc_free(out);
    }
}
