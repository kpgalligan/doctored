//! A byte Convert or Assign into an RGB profile with NO
//! matrix/TRC model (a LUT-only profile) while the document holds something
//! only a matrix/TRC model can evaluate — a `document_v2` Exposure/White
//! Balance layer, or the linear-light evaluation policy — refuses up front,
//! naming the layer or the policy, instead of landing a document that cannot
//! render. Preserve-appearance bakes such a layer instead.
//!
//! The LUT destination is built here byte by byte (`common/icc_lut_builders`):
//! an RGB `mntr` profile with an XYZ PCS, a D50 `wtpt` and grid-2 identity
//! `mft2` `A2B0`/`B2A0` tables. The bake's oracle is independent of the
//! conversion pipeline under test: the source document's own flattened pixel,
//! decoded by the sRGB MODEL (`to_pcs_xyz64`), encoded by the parsed LUT
//! profile (`ProofProfile::from_xyz`, relative), rounded once — within one
//! code.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::*;
use doctored_core::ffi_color::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_evaluation::*;
use doctored_core::ffi_group::*;
use doctored_core::ffi_output_color::*;
use doctored_core::icc::IccProfile;
use doctored_core::icc_intent::RenderingIntent;
use doctored_core::icc_lut::{ProofProfile, D50};
use image::{Rgba, RgbaImage};
use tempfile::TempDir;

use crate::common;
use common::*;

use crate::icc_lut_builders as builders;
use builders::{build, tag_desc, tag_mft2, tag_wtpt};

const PROFILE_DISPLAY_P3: std::ffi::c_int = 1;
const EVALUATION_LINEAR_LIGHT: std::ffi::c_int = 1;
const BASE: [u8; 4] = [100, 120, 140, 255];
const W: u32 = 8;
const H: u32 = 6;

/// The LUT-only destination (module doc).
fn lut_profile() -> Vec<u8> {
    build(
        [0x02, 0x20, 0, 0],
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[
            (b"desc", tag_desc("Synthetic LUT RGB")),
            (b"wtpt", tag_wtpt(D50)),
            (b"A2B0", tag_mft2(3, 3, 2, &|c| c.to_vec())),
            (b"B2A0", tag_mft2(3, 3, 2, &|c| c.to_vec())),
        ],
    )
}

fn builtin_bytes(which: std::ffi::c_int) -> Vec<u8> {
    let len = dr_builtin_profile_len(which);
    let mut out = vec![0u8; len];
    assert!(unsafe { dr_builtin_profile(which, out.as_mut_ptr(), len) });
    out
}

fn solid(color: [u8; 4]) -> RgbaImage {
    RgbaImage::from_pixel(W, H, Rgba(color))
}

fn meta(op: &str, params: &str) -> CString {
    CString::new(format!(
        "{{\"type\":\"adjust\",\"op\":\"{op}\",\"params\":{params}}}"
    ))
    .expect("static json")
}

const EXPOSURE_V2: (&str, &str) = (
    "exposure",
    "{\"exposure\":0.5,\"evaluation\":\"document_v2\"}",
);

/// A document over a solid `base` with one adjustment layer "Adjust" (index
/// 1) above it.
fn with_adjustment(dir: &TempDir, base: [u8; 4], op: &str, params: &str) -> *mut DrDocument {
    let doc = doc_from(dir, "base.png", &solid(base));
    let doc = add_layer(dir, "adj.png", doc, 0, &solid([0, 0, 0, 255]), "Adjust");
    let description = meta(op, params);
    apply(doc, |d| unsafe {
        dr_doc_with_layer_meta(d, 1, description.as_ptr())
    })
}

fn owned(raw: *mut c_char) -> Option<String> {
    if raw.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(raw) };
    Some(text)
}

fn options(policy: &str) -> CString {
    CString::new(format!("{{\"adjustments\":\"{policy}\"}}")).unwrap()
}

/// The preflight's answer: `Ok(report JSON)` or `Err(sentence)`.
fn preflight(
    doc: *const DrDocument,
    icc: &[u8],
    policy: &str,
) -> Result<serde_json::Value, String> {
    let json = options(policy);
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe {
        dr_doc_conversion_preflight(
            doc,
            icc.as_ptr(),
            icc.len(),
            json.as_ptr(),
            std::ptr::null(),
            &mut err,
        )
    };
    match owned(raw) {
        Some(text) => {
            assert!(err.is_null());
            Ok(serde_json::from_str(&text).expect("preflight JSON"))
        }
        None => Err(owned(err).expect("a preflight refusal carries a sentence")),
    }
}

/// The conversion's answer: `Ok(document)` or `Err(sentence)`.
fn convert(doc: *const DrDocument, icc: &[u8], policy: &str) -> Result<*mut DrDocument, String> {
    let json = options(policy);
    let mut report: *mut c_char = ptr::null_mut();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_convert_to_profile_with(
            doc,
            icc.as_ptr(),
            icc.len(),
            json.as_ptr(),
            ptr::null(),
            &mut report,
            &mut err,
        )
    };
    owned(report);
    if out.is_null() {
        Err(owned(err).unwrap_or_else(|| "<no sentence>".into()))
    } else {
        assert!(err.is_null());
        Ok(out)
    }
}

fn flattened(doc: *const DrDocument) -> Vec<u8> {
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe { dr_doc_flattened(doc, 0, std::ptr::null(), &mut err) };
    assert!(!image.is_null(), "renders: {}", take_err_string(err));
    let pixels = img_pixels(image);
    unsafe { dr_image_free(image) };
    pixels
}

fn assign_refusal(doc: *const DrDocument, icc: &[u8]) -> Option<String> {
    owned(unsafe { dr_doc_assign_profile_refusal(doc, icc.as_ptr(), icc.len()) })
}

fn linear(doc: *mut DrDocument) -> *mut DrDocument {
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_set_evaluation(doc, EVALUATION_LINEAR_LIGHT, &mut err) };
    assert!(!out.is_null(), "linear-light: {}", take_err_string(err));
    unsafe { dr_doc_free(doc) };
    out
}

/// The oracle: one sRGB pixel through the sRGB model to PCS XYZ, then the
/// LUT profile's own PCS→device table, rounded once.
fn oracle(lut: &ProofProfile, rgb: [u8; 3]) -> [u8; 3] {
    let srgb = IccProfile::srgb();
    let model = srgb.model().expect("sRGB is matrix/TRC");
    let xyz = model.to_pcs_xyz64(rgb.map(|v| f64::from(v) / 255.0));
    let (device, n) = lut
        .from_xyz(xyz, RenderingIntent::RelativeColorimetric)
        .expect("the LUT encodes");
    assert_eq!(n, 3);
    std::array::from_fn(|c| (device[c].clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn assert_calibrated(sentence: &str, verb: &str, offer_bake: bool) {
    assert!(
        sentence.contains("\"Adjust\""),
        "names the layer: {sentence}"
    );
    assert!(sentence.contains("matrix/TRC"), "{sentence}");
    assert!(
        sentence.contains(&format!("{verb} a matrix/TRC profile")),
        "{sentence}"
    );
    assert!(!sentence.contains("rasterize"), "{sentence}");
    assert_eq!(
        sentence.contains("preserve appearance"),
        offer_bake,
        "bake offer {offer_bake}: {sentence}"
    );
}

// ------------------------------------------------------------ keep (2a) --

#[test]
fn keep_refuses_a_document_v2_layer_into_a_lut_profile_naming_it() {
    let dir = TempDir::new().unwrap();
    let lut = lut_profile();
    for (op, params) in [
        EXPOSURE_V2,
        (
            "exposure",
            "{\"exposure\":0,\"evaluation\":\"document_v2\"}",
        ),
        (
            "white_balance",
            "{\"temperature\":5000,\"tint\":10,\"evaluation\":\"document_v2\"}",
        ),
    ] {
        let doc = with_adjustment(&dir, BASE, op, params);
        let refusal = preflight(doc, &lut, "keep").expect_err("keep refuses");
        assert_calibrated(&refusal, "convert to", true);
        assert!(refusal.contains("Synthetic LUT RGB"), "{refusal}");
        let converted = convert(doc, &lut, "keep").expect_err("the conversion refuses too");
        assert_eq!(converted, refusal, "{op}: one sentence");
        unsafe { dr_doc_free(doc) };
    }
}

#[test]
fn keep_offers_no_bake_that_would_itself_refuse() {
    let dir = TempDir::new().unwrap();
    let lut = lut_profile();

    // Hidden: nothing to preserve, so nothing to offer.
    let doc = with_adjustment(&dir, BASE, EXPOSURE_V2.0, EXPOSURE_V2.1);
    let hidden = apply(doc, |d| unsafe { dr_doc_with_layer_visible(d, 1, false) });
    let refusal = preflight(hidden, &lut, "keep").expect_err("a hidden layer still refuses");
    assert_calibrated(&refusal, "convert to", false);
    unsafe { dr_doc_free(hidden) };

    // Over transparent pixels: the bake would refuse, so it is not offered.
    let doc = with_adjustment(&dir, [0, 0, 0, 0], EXPOSURE_V2.0, EXPOSURE_V2.1);
    let refusal = preflight(doc, &lut, "keep").expect_err("keep refuses");
    assert_calibrated(&refusal, "convert to", false);
    let baked = preflight(doc, &lut, "preserve_appearance").expect_err("and the bake refuses");
    assert!(baked.contains("transparent"), "{baked}");
    unsafe { dr_doc_free(doc) };
}

// ------------------------------------------------ preserve appearance (2a) --

#[test]
fn preserve_appearance_refuses_what_it_cannot_bake() {
    let dir = TempDir::new().unwrap();
    let lut = lut_profile();

    let doc = with_adjustment(&dir, BASE, EXPOSURE_V2.0, EXPOSURE_V2.1);
    let hidden = apply(doc, |d| unsafe { dr_doc_with_layer_visible(d, 1, false) });
    let refusal = preflight(hidden, &lut, "preserve_appearance").expect_err("hidden refuses");
    assert_calibrated(&refusal, "convert to", false);
    unsafe { dr_doc_free(hidden) };

    let doc = with_adjustment(&dir, BASE, EXPOSURE_V2.0, EXPOSURE_V2.1);
    let name = CString::new("G").unwrap();
    let mut group = 0usize;
    let mut cleared = [0usize; 4];
    let mut cleared_len = 0usize;
    let mut reordered = [0usize; 4];
    let mut reordered_len = 0usize;
    let grouped = apply(doc, |d| unsafe {
        dr_doc_group_layers(
            d,
            [1usize].as_ptr(),
            1,
            name.as_ptr(),
            &mut group,
            cleared.as_mut_ptr(),
            &mut cleared_len,
            reordered.as_mut_ptr(),
            &mut reordered_len,
            4,
        )
    });
    let refusal = preflight(grouped, &lut, "preserve_appearance").expect_err("grouped refuses");
    assert!(refusal.contains("inside a group"), "{refusal}");
    unsafe { dr_doc_free(grouped) };

    let doc = with_adjustment(&dir, BASE, EXPOSURE_V2.0, EXPOSURE_V2.1);
    let clipped = apply(doc, |d| unsafe { dr_doc_with_layer_clipped(d, 1, true) });
    let refusal = preflight(clipped, &lut, "preserve_appearance").expect_err("clipped refuses");
    assert!(refusal.contains("is clipped"), "{refusal}");
    unsafe { dr_doc_free(clipped) };
}

#[test]
fn preserve_appearance_bakes_a_visible_document_v2_layer_into_a_lut_profile() {
    let dir = TempDir::new().unwrap();
    let lut = lut_profile();
    let model = ProofProfile::parse(&lut).expect("the LUT fixture parses");
    let doc = with_adjustment(&dir, BASE, EXPOSURE_V2.0, EXPOSURE_V2.1);

    let report = preflight(doc, &lut, "preserve_appearance").expect("preserve bakes");
    let rows = report["adjustments"].as_array().expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["action"], "baked");
    let reason = rows[0]["reason"].as_str().unwrap();
    assert!(
        reason.contains("cannot be evaluated in \"Synthetic LUT RGB\""),
        "{reason}"
    );
    assert!(reason.contains("\"Adjust (baked)\""), "{reason}");

    let source = flattened(doc);
    let converted = convert(doc, &lut, "preserve_appearance").expect("the bake converts");
    // It renders: nothing left in it needs a matrix/TRC model.
    flattened(converted);
    let baked = layer_pixels(converted, 1);
    let exposed = [source[0], source[1], source[2]];
    let want = oracle(&model, exposed);
    let unexposed = oracle(&model, [BASE[0], BASE[1], BASE[2]]);
    for (i, px) in baked.chunks_exact(4).enumerate() {
        for c in 0..3 {
            let got = i32::from(px[c]);
            assert!(
                (got - i32::from(want[c])).abs() <= 1,
                "pixel {i} channel {c}: {got} vs oracle {}",
                want[c]
            );
        }
        assert_eq!(px[3], 255);
    }
    // The exposure is IN the bake: dropping it would land on the base's own
    // conversion, which differs by more than a code.
    assert!(
        (0..3).any(|c| (i32::from(want[c]) - i32::from(unexposed[c])).abs() > 1),
        "the fixture distinguishes an exposed bake: {want:?} vs {unexposed:?}"
    );
    assert!(
        (0..3).any(|c| (i32::from(baked[c]) - i32::from(unexposed[c])).abs() > 1),
        "the bake carries the exposure: {:?} vs {unexposed:?}",
        &baked[..3]
    );
    unsafe {
        dr_doc_free(converted);
        dr_doc_free(doc);
    }
}

#[test]
fn legacy_evaluation_and_matrix_destinations_are_unchanged() {
    let dir = TempDir::new().unwrap();
    let lut = lut_profile();

    // legacy_srgb exposure: kept, converts and renders.
    let doc = with_adjustment(
        &dir,
        BASE,
        "exposure",
        "{\"exposure\":0.5,\"evaluation\":\"legacy_srgb\"}",
    );
    let report = preflight(doc, &lut, "keep").expect("legacy keeps");
    assert_eq!(report["adjustments"][0]["action"], "kept");
    let converted = convert(doc, &lut, "keep").expect("legacy converts");
    flattened(converted);
    unsafe {
        dr_doc_free(converted);
        dr_doc_free(doc);
    }

    // A matrix/TRC destination keeps a document_v2 layer, as it always did.
    let p3 = builtin_bytes(PROFILE_DISPLAY_P3);
    let doc = with_adjustment(&dir, BASE, EXPOSURE_V2.0, EXPOSURE_V2.1);
    for policy in ["keep", "preserve_appearance"] {
        let report = preflight(doc, &p3, policy).expect("P3 keeps");
        assert_eq!(report["adjustments"][0]["action"], "kept", "{policy}");
    }
    let converted = convert(doc, &p3, "keep").expect("P3 converts");
    flattened(converted);
    unsafe {
        dr_doc_free(converted);
        dr_doc_free(doc);
    }
}

#[test]
fn a_linear_light_document_refuses_a_lut_destination() {
    let dir = TempDir::new().unwrap();
    let lut = lut_profile();
    let doc = linear(doc_from(&dir, "base.png", &solid(BASE)));
    let refusal = preflight(doc, &lut, "keep").expect_err("linear light refuses");
    assert!(refusal.contains("linear-light"), "{refusal}");
    let converted = convert(doc, &lut, "keep").expect_err("the conversion refuses too");
    assert_eq!(converted, refusal);
    unsafe { dr_doc_free(doc) };
}

// ----------------------------------------------------------- assign (2b) --

#[test]
fn assign_refuses_a_lut_profile_under_a_document_v2_layer() {
    let dir = TempDir::new().unwrap();
    let lut = lut_profile();
    let doc = with_adjustment(&dir, BASE, EXPOSURE_V2.0, EXPOSURE_V2.1);
    let refusal = assign_refusal(doc, &lut).expect("a refusal");
    assert_calibrated(&refusal, "assign", false);
    unsafe {
        let mut err = ptr::null_mut();
        assert!(dr_doc_assign_profile(doc, lut.as_ptr(), lut.len(), &mut err).is_null());
        assert_eq!(
            take_err_string(err),
            refusal,
            "assign refuses with the same sentence"
        );
        let mut err = ptr::null_mut();
        assert!(
            dr_doc_assign_profile_rasterizing(doc, lut.as_ptr(), lut.len(), &mut err).is_null()
        );
        assert_eq!(
            take_err_string(err),
            refusal,
            "and so does the rasterizing assign"
        );
    }

    // A hidden layer still refuses: the render preflight refuses it too.
    let hidden = apply(doc, |d| unsafe { dr_doc_with_layer_visible(d, 1, false) });
    assert!(assign_refusal(hidden, &lut).is_some());

    // The document's own profile bytes: no refusal (it is the no-op).
    let len = unsafe { dr_doc_icc_profile_len(hidden) };
    let mut own = vec![0u8; len];
    assert!(unsafe { dr_doc_icc_profile(hidden, own.as_mut_ptr(), len) });
    assert_eq!(assign_refusal(hidden, &own), None);
    unsafe { dr_doc_free(hidden) };
}

#[test]
fn assign_still_accepts_what_can_render() {
    let dir = TempDir::new().unwrap();
    let lut = lut_profile();
    let p3 = builtin_bytes(PROFILE_DISPLAY_P3);

    // A legacy_srgb layer evaluates on the stored numbers.
    let legacy = with_adjustment(
        &dir,
        BASE,
        "exposure",
        "{\"exposure\":0.5,\"evaluation\":\"legacy_srgb\"}",
    );
    // A plain encoded document: the open-time tagging case.
    let plain = doc_from(&dir, "plain.png", &solid(BASE));
    // A document_v2 layer into a matrix/TRC profile.
    let v2 = with_adjustment(&dir, BASE, EXPOSURE_V2.0, EXPOSURE_V2.1);
    for (what, doc, icc) in [
        ("legacy", legacy, &lut),
        ("plain", plain, &lut),
        ("P3", v2, &p3),
    ] {
        assert_eq!(assign_refusal(doc, icc), None, "{what}");
        let assigned =
            unsafe { dr_doc_assign_profile(doc, icc.as_ptr(), icc.len(), std::ptr::null_mut()) };
        assert!(!assigned.is_null(), "{what} assigns");
        flattened(assigned);
        unsafe {
            dr_doc_free(assigned);
            dr_doc_free(doc);
        }
    }
}

#[test]
fn assign_refuses_a_lut_profile_on_a_linear_light_document() {
    let dir = TempDir::new().unwrap();
    let lut = lut_profile();
    let doc = linear(doc_from(&dir, "base.png", &solid(BASE)));
    let refusal = assign_refusal(doc, &lut).expect("a refusal");
    assert!(refusal.contains("linear-light"), "{refusal}");
    unsafe {
        let mut err = ptr::null_mut();
        assert!(dr_doc_assign_profile(doc, lut.as_ptr(), lut.len(), &mut err).is_null());
        assert_eq!(
            take_err_string(err),
            refusal,
            "assign refuses with the same sentence"
        );
        dr_doc_free(doc);
    }
}

#[test]
fn the_assign_refusal_tolerates_null_and_non_profiles() {
    let lut = lut_profile();
    let dir = TempDir::new().unwrap();
    let doc = with_adjustment(&dir, BASE, EXPOSURE_V2.0, EXPOSURE_V2.1);
    unsafe {
        assert!(dr_doc_assign_profile_refusal(ptr::null(), lut.as_ptr(), lut.len()).is_null());
        assert!(dr_doc_assign_profile_refusal(doc, ptr::null(), 0).is_null());
        let junk = b"not a profile";
        assert!(dr_doc_assign_profile_refusal(doc, junk.as_ptr(), junk.len()).is_null());
        dr_doc_free(doc);
    }
}

// ------------------------------------------------------ the missing cases --

/// `doc` with layer 1 moved into a new group "G".
fn grouped(doc: *mut DrDocument) -> *mut DrDocument {
    let name = CString::new("G").unwrap();
    let mut group = 0usize;
    let mut cleared = [0usize; 4];
    let mut cleared_len = 0usize;
    let mut reordered = [0usize; 4];
    let mut reordered_len = 0usize;
    apply(doc, |d| unsafe {
        dr_doc_group_layers(
            d,
            [1usize].as_ptr(),
            1,
            name.as_ptr(),
            &mut group,
            cleared.as_mut_ptr(),
            &mut cleared_len,
            reordered.as_mut_ptr(),
            &mut reordered_len,
            4,
        )
    })
}

#[test]
fn keep_refuses_a_grouped_or_clipped_document_v2_layer_without_a_bake_offer() {
    let dir = TempDir::new().unwrap();
    let lut = lut_profile();
    let group = grouped(with_adjustment(&dir, BASE, EXPOSURE_V2.0, EXPOSURE_V2.1));
    let doc = with_adjustment(&dir, BASE, EXPOSURE_V2.0, EXPOSURE_V2.1);
    let clipped = apply(doc, |d| unsafe { dr_doc_with_layer_clipped(d, 1, true) });
    for (what, doc) in [("grouped", group), ("clipped", clipped)] {
        let refusal = preflight(doc, &lut, "keep").expect_err("keep refuses");
        assert_calibrated(&refusal, "convert to", false);
        let converted = convert(doc, &lut, "keep").expect_err("the conversion refuses too");
        assert_eq!(converted, refusal, "{what}: one sentence");
        unsafe { dr_doc_free(doc) };
    }
}

#[test]
fn assign_refuses_a_lut_profile_under_a_grouped_document_v2_layer() {
    let dir = TempDir::new().unwrap();
    let lut = lut_profile();
    let doc = grouped(with_adjustment(&dir, BASE, EXPOSURE_V2.0, EXPOSURE_V2.1));
    let refusal = assign_refusal(doc, &lut).expect("a refusal");
    assert_calibrated(&refusal, "assign", false);
    unsafe {
        let mut err = ptr::null_mut();
        assert!(dr_doc_assign_profile(doc, lut.as_ptr(), lut.len(), &mut err).is_null());
        assert_eq!(
            take_err_string(err),
            refusal,
            "assign refuses with the same sentence"
        );
        dr_doc_free(doc);
    }
}

#[test]
fn a_lut_tagged_document_converting_to_its_own_profile_is_no_change() {
    let dir = TempDir::new().unwrap();
    let lut = lut_profile();
    let plain = doc_from(&dir, "base.png", &solid(BASE));
    let tagged = apply(plain, |d| unsafe {
        dr_doc_assign_profile(d, lut.as_ptr(), lut.len(), ptr::null_mut())
    });
    let doc = add_layer(&dir, "adj.png", tagged, 0, &solid([0, 0, 0, 255]), "Adjust");
    let description = meta(EXPOSURE_V2.0, EXPOSURE_V2.1);
    let doc = apply(doc, |d| unsafe {
        dr_doc_with_layer_meta(d, 1, description.as_ptr())
    });
    for policy in ["keep", "preserve_appearance"] {
        let report = preflight(doc, &lut, policy).expect("the own profile is no refusal");
        assert_eq!(report["adjustments"], serde_json::json!([]), "{policy}");
        let json = options(policy);
        let mut report: *mut c_char = ptr::null_mut();
        let mut err: *mut c_char = ptr::null_mut();
        let out = unsafe {
            dr_doc_convert_to_profile_with(
                doc,
                lut.as_ptr(),
                lut.len(),
                json.as_ptr(),
                ptr::null(),
                &mut report,
                &mut err,
            )
        };
        owned(report);
        assert!(out.is_null(), "{policy}: nothing changes");
        assert_eq!(owned(err), None, "{policy}: and nothing is refused");
    }
    unsafe { dr_doc_free(doc) };
}
