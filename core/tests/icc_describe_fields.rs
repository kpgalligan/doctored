//! `dr_icc_describe`'s two narrower answers, through the FFI:
//!
//! - `proof_refusal`: why a profile cannot be proofed through — null
//!   exactly when `proofable`, naming the missing A2B/B2A direction and its
//!   floating-point D2Bx/B2Dx twin, or the parser's own sentence;
//! - `high_depth_destination`: whether a 16/32-bit Convert accepts the
//!   profile as a destination. Its oracle is the typed conversion preflight
//!   itself, so the field cannot drift from the rule it reports;
//! - the export destination rule: an export into a profile file
//!   converts exactly when `model` is not `matrix` or `high_depth_destination`
//!   holds, at 16 bits, at 8 bits and from a byte document alike;
//! - a failing matrix profile as a SOURCE: a document tagged with it
//!   converts, adopts and exports OUT of it at both depths, while
//!   `export_refusal` / `convert_refusal` carry the host's capitalised
//!   sentences for an encode INTO it (null for every other profile).
//!
//! Profiles are built byte by byte (`common/icc_lut_builders`), with the
//! matrix/TRC tags written here.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use crate::common;

use doctored_core::doc_export::ExportResult;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_string_free};
use doctored_core::ffi_color::*;
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_from_image};
use doctored_core::ffi_export::{
    dr_doc_export_encode, dr_export_result_free, dr_export_result_info,
    dr_precision_doc_export_encode,
};
use doctored_core::ffi_output_color::*;
use doctored_core::ffi_precision_color::{
    dr_precision_doc_assign_profile, dr_precision_doc_convert_to_profile,
};
use doctored_core::icc::IccProfile;
use doctored_core::icc_lut::D50;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::Value;

use crate::icc_lut_builders as builders;
use builders::{build, tag_desc, tag_mft2, tag_wtpt};

const PROFILE_SRGB: std::ffi::c_int = 0;
const PROFILE_DISPLAY_P3: std::ffi::c_int = 1;

fn builtin_bytes(which: std::ffi::c_int) -> Vec<u8> {
    let len = dr_builtin_profile_len(which);
    let mut out = vec![0u8; len];
    assert!(unsafe { dr_builtin_profile(which, out.as_mut_ptr(), len) });
    out
}

unsafe fn text(pointer: *mut c_char) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    let string = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    Some(string)
}

fn describe(bytes: &[u8]) -> Value {
    let mut err = ptr::null_mut();
    let raw = unsafe { dr_icc_describe(bytes.as_ptr(), bytes.len(), &mut err) };
    let json = unsafe { text(raw) }.expect("describes");
    assert_eq!(unsafe { text(err) }, None);
    serde_json::from_str(&json).expect("describe JSON")
}

/// The invariant every described profile holds.
fn assert_invariant(d: &Value) {
    assert_eq!(
        d["proofable"].as_bool().expect("proofable"),
        d["proof_refusal"].is_null(),
        "{d}"
    );
}

/// 32 zero bytes: a floating-point tag's presence is all the parser reads.
fn float_tag() -> Vec<u8> {
    vec![0u8; 32]
}

fn identity3() -> Vec<u8> {
    tag_mft2(3, 3, 2, &|c| c.to_vec())
}

fn rgb_lut(name: &str, tags: Vec<(&[u8; 4], Vec<u8>)>) -> Vec<u8> {
    let mut all: Vec<(&[u8; 4], Vec<u8>)> =
        vec![(b"desc", tag_desc(name)), (b"wtpt", tag_wtpt(D50))];
    all.extend(tags);
    build([0x02, 0x20, 0, 0], b"mntr", b"RGB ", b"XYZ ", &all)
}

/// An RGB matrix/TRC profile whose three TRCs are a count-1 `curv` of the
/// given u8Fixed8 gamma (0 is the precision-incompatible curve).
fn matrix_profile(gamma: u16) -> Vec<u8> {
    let mut curve = b"curv\0\0\0\0".to_vec();
    curve.extend_from_slice(&1u32.to_be_bytes());
    curve.extend_from_slice(&gamma.to_be_bytes());
    let xyz = |v: [f64; 3]| tag_wtpt(v);
    build(
        [0x02, 0x10, 0, 0],
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[
            (b"desc", tag_desc("Synthetic Bad Gamma")),
            (b"rXYZ", xyz([0.4361, 0.2225, 0.0139])),
            (b"gXYZ", xyz([0.3851, 0.7169, 0.0971])),
            (b"bXYZ", xyz([0.1431, 0.0606, 0.7141])),
            (b"rTRC", curve.clone()),
            (b"gTRC", curve.clone()),
            (b"bTRC", curve),
        ],
    )
}

fn cmyk(tags: Vec<(&[u8; 4], Vec<u8>)>) -> Vec<u8> {
    let mut all: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"desc", tag_desc("Synthetic CMYK")),
        (b"wtpt", tag_wtpt(D50)),
    ];
    all.extend(tags);
    build([0x02, 0x20, 0, 0], b"prtr", b"CMYK", b"Lab ", &all)
}

// ------------------------------------------------------ proof_refusal --

#[test]
fn proof_refusal_names_the_missing_direction_and_its_float_twin() {
    // A2B0 + B2D0: decodes, cannot encode; the B2D0 is the float twin.
    let d = describe(&rgb_lut(
        "A2B B2D",
        vec![(b"A2B0", identity3()), (b"B2D0", float_tag())],
    ));
    assert_invariant(&d);
    assert!(d["refusal"].is_null(), "{d}");
    assert_eq!(d["proofable"], false);
    assert_eq!(
        d["proof_refusal"],
        "it has no B2A (PCS-to-device) table; its B2D0 is floating-point, which this build does not read"
    );

    // B2A0 + D2B0: encodes, cannot decode; the D2B0 is the float twin.
    let d = describe(&rgb_lut(
        "B2A D2B",
        vec![(b"B2A0", identity3()), (b"D2B0", float_tag())],
    ));
    assert_invariant(&d);
    let reason = d["proof_refusal"].as_str().expect("a refusal");
    assert!(reason.contains("A2B"), "{reason}");
    assert!(reason.contains("D2B0"), "{reason}");
    assert_eq!(
        reason,
        "it has no A2B (device-to-PCS) table; its D2B0 is floating-point, which this build does not read"
    );

    // A2B0 only: no twin, so the bare sentence.
    let d = describe(&rgb_lut("A2B", vec![(b"A2B0", identity3())]));
    assert_invariant(&d);
    assert_eq!(d["proof_refusal"], "it has no B2A (PCS-to-device) table");

    // Both directions: proofable.
    let d = describe(&rgb_lut(
        "A2B B2A",
        vec![(b"A2B0", identity3()), (b"B2A0", identity3())],
    ));
    assert_invariant(&d);
    assert_eq!(d["proofable"], true);
    assert!(d["proof_refusal"].is_null(), "{d}");
}

#[test]
fn proof_refusal_is_the_parsers_sentence_when_the_profile_does_not_parse() {
    // The built-in sRGB is matrix/TRC with no tables.
    let d = describe(&builtin_bytes(PROFILE_SRGB));
    assert_invariant(&d);
    assert!(d["proof_refusal"].is_string(), "{d}");
    assert_eq!(d["proof_refusal"], d["refusal"]);

    // A CMYK profile whose only table is the floating-point B2D2.
    let d = describe(&cmyk(vec![(b"B2D2", float_tag())]));
    assert_invariant(&d);
    assert_eq!(d["proof_refusal"], d["refusal"]);
    let reason = d["proof_refusal"].as_str().expect("a refusal");
    assert!(reason.contains("B2D2"), "{reason}");
}

// --------------------------------------------- high_depth_destination --

fn srgb_u16_document() -> *mut PrecisionDocument {
    let image = PrecisionImage::from_rgba16(
        2,
        1,
        vec![[32123, 12345, 56789, 65535], [1000, 60000, 30000, 65535]],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let doc =
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap();
    Box::into_raw(Box::new(doc))
}

/// Whether the typed conversion preflight of an sRGB u16 document accepts
/// `profile` as its destination.
fn typed_preflight_accepts(doc: *const PrecisionDocument, profile: &[u8]) -> bool {
    let mut err = ptr::null_mut();
    let raw = unsafe {
        dr_precision_doc_conversion_preflight(
            doc,
            profile.as_ptr(),
            profile.len(),
            ptr::null(),
            REFERENCE_WORKING_BYTES,
            0,
            std::ptr::null(),
            &mut err,
        )
    };
    let accepted = unsafe { text(raw) }.is_some();
    unsafe { text(err) };
    accepted
}

#[test]
fn high_depth_destination_is_the_typed_preflights_answer() {
    let srgb = builtin_bytes(PROFILE_SRGB);
    let p3 = builtin_bytes(PROFILE_DISPLAY_P3);
    let bad_gamma = matrix_profile(0);
    let lut = rgb_lut(
        "Synthetic LUT RGB",
        vec![(b"A2B0", identity3()), (b"B2A0", identity3())],
    );
    let press = cmyk(vec![
        (b"A2B0", tag_mft2(4, 3, 2, &|c| vec![c[0], c[1], c[2]])),
        (b"B2A0", tag_mft2(3, 4, 2, &|c| vec![c[0], c[1], c[2], 0.0])),
    ]);
    let float_cmyk = cmyk(vec![(b"B2D2", float_tag())]);

    for (what, bytes, expected) in [
        ("sRGB", &srgb, true),
        ("Display P3", &p3, true),
        ("gamma-0 matrix", &bad_gamma, false),
        ("LUT RGB", &lut, false),
        ("CMYK", &press, false),
        ("float CMYK", &float_cmyk, false),
    ] {
        let d = describe(bytes);
        assert_invariant(&d);
        assert_eq!(
            d["high_depth_destination"].as_bool(),
            Some(expected),
            "{what}: {d}"
        );
    }
    assert_eq!(describe(&bad_gamma)["model"], "matrix");

    let doc = srgb_u16_document();
    for (what, bytes) in [
        ("Display P3", &p3),
        ("gamma-0 matrix", &bad_gamma),
        ("LUT RGB", &lut),
    ] {
        assert_eq!(
            describe(bytes)["high_depth_destination"].as_bool(),
            Some(typed_preflight_accepts(doc, bytes)),
            "{what}"
        );
    }
    unsafe { doctored_core::ffi_precision::dr_precision_doc_free(doc) };
}

// ------------------------------------------ export destinations --

/// Encode one export request, answering the result's info or the refusal.
fn export_outcome(
    encode: impl FnOnce(*const c_char, *mut *mut c_char) -> *mut ExportResult,
    request: &str,
) -> Result<Value, String> {
    let json = CString::new(request).expect("request");
    let mut err = ptr::null_mut();
    let result = encode(json.as_ptr(), &mut err);
    if result.is_null() {
        return Err(unsafe { text(err) }.unwrap_or_default());
    }
    let mut info_err = ptr::null_mut();
    let info = unsafe { text(dr_export_result_info(result, &mut info_err)) }.expect("info");
    assert_eq!(unsafe { text(info_err) }, None);
    unsafe { dr_export_result_free(result) };
    Ok(serde_json::from_str(&info).expect("info JSON"))
}

#[test]
fn export_converts_into_exactly_the_destinations_high_depth_destination_and_model_allow() {
    let bad_gamma = matrix_profile(0);
    let lut = rgb_lut(
        "Synthetic LUT RGB",
        vec![(b"A2B0", identity3()), (b"B2A0", identity3())],
    );
    let dir = tempfile::TempDir::new().expect("tempdir");
    let profiles = [
        ("sRGB", builtin_bytes(PROFILE_SRGB)),
        ("Display P3", builtin_bytes(PROFILE_DISPLAY_P3)),
        ("gamma-0 matrix", bad_gamma),
        ("LUT RGB", lut),
    ];

    let typed = srgb_u16_document();
    let pixels = [200u8, 40, 90, 255, 10, 220, 130, 255];
    let image = unsafe { dr_image_from_rgba(pixels.as_ptr(), 2, 1) };
    assert!(!image.is_null());
    let byte = unsafe { dr_doc_from_image(image) };
    assert!(!byte.is_null());
    unsafe { dr_image_free(image) };

    let typed_encode = |request: &str| {
        export_outcome(
            |json, err| unsafe {
                dr_precision_doc_export_encode(
                    typed,
                    ptr::null(),
                    json,
                    REFERENCE_WORKING_BYTES,
                    0,
                    ptr::null(),
                    err,
                )
            },
            request,
        )
    };
    let byte_encode = |request: &str| {
        export_outcome(
            |json, err| unsafe { dr_doc_export_encode(byte, ptr::null(), json, ptr::null(), err) },
            request,
        )
    };

    for (index, (what, bytes)) in profiles.iter().enumerate() {
        let path = dir.path().join(format!("{index}.icc"));
        std::fs::write(&path, bytes).expect("write profile");
        let d = describe(bytes);
        let allowed = d["model"] != "matrix" || d["high_depth_destination"] == true;
        let path_json = Value::from(path.to_string_lossy().as_ref());
        let request = |extra: &str| {
            format!(r#"{{"format":"png","profile":"file","profile_path":{path_json}{extra}}}"#)
        };

        for (arm, outcome) in [
            ("typed 16", typed_encode(&request(r#","bit_depth":16"#))),
            ("typed 8", typed_encode(&request(r#","bit_depth":8"#))),
            ("byte", byte_encode(&request(""))),
        ] {
            assert_eq!(outcome.is_ok(), allowed, "{what} {arm}: {outcome:?} {d}");
            if let Err(reason) = &outcome {
                assert_eq!(
                    Some(reason.as_str()),
                    d["export_refusal"].as_str(),
                    "{what} {arm}"
                );
            }
        }

        if *what == "gamma-0 matrix" {
            for extra in [
                r#","intent":"absolute""#,
                r#","black_point_compensation":true"#,
            ] {
                for (arm, outcome) in [
                    (
                        "typed 16",
                        typed_encode(&request(&format!(r#","bit_depth":16{extra}"#))),
                    ),
                    ("typed 8", typed_encode(&request(extra))),
                    ("byte", byte_encode(&request(extra))),
                ] {
                    let reason = outcome.expect_err(&format!("{what} {arm} {extra}"));
                    assert_eq!(
                        Some(reason.as_str()),
                        d["export_refusal"].as_str(),
                        "{arm} {extra}"
                    );
                }
            }
        }
        if *what == "LUT RGB" {
            let info = typed_encode(&request(r#","bit_depth":16"#)).expect("LUT at 16 bits");
            assert_eq!(info["bit_depth"], 16, "{info}");
        }
    }

    unsafe { dr_doc_free(byte) };
    unsafe { doctored_core::ffi_precision::dr_precision_doc_free(typed) };
}

// ------------------------------------------ a failing matrix source --

/// The sRGB transfer function, written independently of the core.
fn srgb_oetf(linear: f64) -> f64 {
    if linear <= 0.0031308 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

#[test]
fn describe_carries_the_destination_refusals_for_a_failing_matrix_profile_only() {
    let d = describe(&matrix_profile(0));
    for (key, verb) in [("export_refusal", "export"), ("convert_refusal", "convert")] {
        assert_eq!(
            d[key].as_str(),
            Some(
                format!(
                    "Doctored cannot {verb} into \u{201c}Synthetic Bad Gamma\u{201d} \u{2014} \
                     its matrix/TRC tone curves fail the precision check every conversion \
                     into it runs (they must be finite and monotone). Choose another profile."
                )
                .as_str()
            ),
            "{d}"
        );
    }
    let lut = rgb_lut(
        "Synthetic LUT RGB",
        vec![(b"A2B0", identity3()), (b"B2A0", identity3())],
    );
    for (what, bytes) in [
        ("sRGB", builtin_bytes(PROFILE_SRGB)),
        ("Display P3", builtin_bytes(PROFILE_DISPLAY_P3)),
        ("gamma 2.2 matrix", matrix_profile(563)),
        ("LUT RGB", lut),
    ] {
        let d = describe(&bytes);
        assert!(d["export_refusal"].is_null(), "{what}: {d}");
        assert!(d["convert_refusal"].is_null(), "{what}: {d}");
    }
}

#[test]
fn a_document_tagged_with_a_failing_matrix_profile_exports_converts_and_adopts_out_of_it() {
    let bad = matrix_profile(0);
    let srgb = builtin_bytes(PROFILE_SRGB);
    let dir = tempfile::TempDir::new().expect("tempdir");
    let own_path = dir.path().join("bad.icc");
    std::fs::write(&own_path, &bad).expect("write profile");
    let own = Value::from(own_path.to_string_lossy().as_ref());

    // Byte document: a gray ramp reinterpreted in `bad`, which decodes linear.
    let ramp = [0u8, 1, 10, 50, 100, 128, 200, 254, 255];
    let pixels: Vec<u8> = ramp.iter().flat_map(|&v| [v, v, v, 255]).collect();
    let image = unsafe { dr_image_from_rgba(pixels.as_ptr(), ramp.len() as u32, 1) };
    assert!(!image.is_null());
    let plain = unsafe { dr_doc_from_image(image) };
    unsafe { dr_image_free(image) };
    let mut err = ptr::null_mut();
    let byte = unsafe { dr_doc_assign_profile(plain, bad.as_ptr(), bad.len(), &mut err) };
    assert!(!byte.is_null(), "{:?}", unsafe { text(err) });
    unsafe { dr_doc_free(plain) };

    let mut err = ptr::null_mut();
    let converted = unsafe { dr_doc_convert_to_profile(byte, srgb.as_ptr(), srgb.len(), &mut err) };
    assert!(!converted.is_null(), "{:?}", unsafe { text(err) });
    let out = common::flat_pixels(converted);
    for (i, &v) in ramp.iter().enumerate() {
        let oracle = (255.0 * srgb_oetf(f64::from(v) / 255.0)).round();
        for c in 0..3 {
            let actual = f64::from(out[i * 4 + c]);
            assert!((actual - oracle).abs() <= 1.0, "{v}: {actual} vs {oracle}");
        }
        assert_eq!(out[i * 4 + 3], 255);
    }
    unsafe { dr_doc_free(converted) };

    let mut outcome = -1;
    let adopted =
        unsafe { dr_doc_adopt_working_space(byte, srgb.as_ptr(), srgb.len(), &mut outcome) };
    assert!(!adopted.is_null());
    assert_eq!(outcome, 1, "DR_ADOPT_CONVERTED");
    unsafe { dr_doc_free(adopted) };

    let byte_encode = |request: &str| {
        export_outcome(
            |json, err| unsafe { dr_doc_export_encode(byte, ptr::null(), json, ptr::null(), err) },
            request,
        )
    };
    let own_request =
        |extra: &str| format!(r#"{{"format":"png","profile":"file","profile_path":{own}{extra}}}"#);
    byte_encode(r#"{"format":"png","profile":"srgb"}"#).expect("byte export into sRGB");
    byte_encode(&own_request("")).expect("byte export into its own bytes");
    for extra in [
        r#","black_point_compensation":true"#,
        r#","intent":"absolute""#,
    ] {
        let reason = byte_encode(&own_request(extra)).expect_err(extra);
        assert_eq!(
            Some(reason.as_str()),
            describe(&bad)["export_refusal"].as_str(),
            "{extra}"
        );
    }
    unsafe { dr_doc_free(byte) };

    // Typed document: the same profile assigned over an sRGB u16 document.
    let plain = srgb_u16_document();
    let mut err = ptr::null_mut();
    let typed = unsafe {
        dr_precision_doc_assign_profile(
            plain,
            bad.as_ptr(),
            bad.len(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!typed.is_null(), "{:?}", unsafe { text(err) });
    unsafe { doctored_core::ffi_precision::dr_precision_doc_free(plain) };
    assert!(typed_preflight_accepts(typed, &srgb));
    let mut err = ptr::null_mut();
    let converted = unsafe {
        dr_precision_doc_convert_to_profile(
            typed,
            srgb.as_ptr(),
            srgb.len(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!converted.is_null(), "{:?}", unsafe { text(err) });
    unsafe { doctored_core::ffi_precision::dr_precision_doc_free(converted) };

    let typed_encode = |request: &str| {
        export_outcome(
            |json, err| unsafe {
                dr_precision_doc_export_encode(
                    typed,
                    ptr::null(),
                    json,
                    REFERENCE_WORKING_BYTES,
                    0,
                    ptr::null(),
                    err,
                )
            },
            request,
        )
    };
    for depth in [16, 8] {
        let depth_key = format!(r#","bit_depth":{depth}"#);
        typed_encode(&format!(
            r#"{{"format":"png","profile":"srgb"{depth_key}}}"#
        ))
        .unwrap_or_else(|e| panic!("typed {depth} into sRGB: {e}"));
        typed_encode(&own_request(&depth_key))
            .unwrap_or_else(|e| panic!("typed {depth} into its own bytes: {e}"));
        let reason = typed_encode(&own_request(&format!(
            r#"{depth_key},"black_point_compensation":true"#
        )))
        .expect_err("typed own bytes with BPC");
        assert_eq!(
            Some(reason.as_str()),
            describe(&bad)["export_refusal"].as_str(),
            "{depth}"
        );
    }
    unsafe { doctored_core::ffi_precision::dr_precision_doc_free(typed) };
}
