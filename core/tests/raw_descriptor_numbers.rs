//! Independent IEEE-754 bit patterns exercise the C descriptor boundary and
//! repeated native save/open. Expected bits are explicit mathematical constants,
//! not numbers obtained by asking the JSON parser under test to parse itself.
use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::{dr_doc_open, dr_doc_save_native};
use doctored_core::ffi_precision::{dr_precision_doc_open, dr_precision_doc_save_native};
use doctored_core::ffi_raw_source::{dr_doc_attach_raw_source, dr_precision_doc_attach_raw_source};
use doctored_core::ffi_retained_source::{dr_doc_source_info, dr_precision_doc_source_info};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::retained_source::{SourceEdit, SourceInput};
use image::{Rgba, RgbaImage};
use serde_json::Value;
use std::ffi::{c_char, CStr, CString};
use std::ptr;

const CASES: &[(&str, u64)] = &[
    ("11.999950408935547", 0x4027fff980000000),
    ("9.773509979248047", 0x40238c0980000000),
    ("-14.251788139343262", 0xc02c80ea60000000),
    ("0.3037501573562622", 0x3fd370a480000000),
    ("0.8999999761581421", 0x3fecccccc0000000),
    ("1.0000000000000002", 0x3ff0000000000001),
    ("0.1", 0x3fb999999999999a),
    // Exactly halfway between1 and its next value chooses the even mantissa;
    // the following decimal is just above that same midpoint.
    (
        "1.00000000000000011102230246251565404236316680908203125",
        0x3ff0000000000000,
    ),
    (
        "1.00000000000000011102230246251565404236316680908203126",
        0x3ff0000000000001,
    ),
    ("2.2250738585072014e-308", 0x0010000000000000),
    ("4.9406564584124654e-324", 0x0000000000000001),
    ("1.7976931348623157e308", 0x7fefffffffffffff),
    ("-0", 0x8000000000000000),
    ("-0.0", 0x8000000000000000),
    ("18446744073709551617.0", 0x43f0000000000000),
];
const ORIGINAL: &[u8] = b"opaque camera original\0\xff";
fn descriptor() -> CString {
    let samples = CASES
        .iter()
        .map(|(literal, _)| *literal)
        .collect::<Vec<_>>()
        .join(",");
    CString::new(format!(r#"{{"schema_version":1,"original_filename":"camera.dng","content_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","decoder":{{"identifier":"unavailable","version":"1","implementation":"saved"}},"recipe":{{"values":{{"tint":11.999950408935547}},"capabilities":{{"defaults":{{"tint":9.773509979248047}}}},"samples":[{samples}],"largest_integer":18446744073709551615,"smallest_integer":-9223372036854775808,"text":"11.999950408935547","enabled":true}}}}"#)).unwrap()
}
fn source() -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            2,
            1,
            vec![[13, 257, 1027, 65535], [1, 32769, 65534, 0]],
            IccProfile::display_p3(),
            None,
        )
        .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}
unsafe fn take_text(pointer: *mut c_char) -> String {
    assert!(!pointer.is_null());
    let text = unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .unwrap()
        .to_owned();
    unsafe { dr_string_free(pointer) };
    text
}
fn check(pointer: *mut c_char, error: *mut c_char) -> Value {
    assert!(error.is_null(), "{}", unsafe { take_text(error) });
    let value: Value = serde_json::from_str(&unsafe { take_text(pointer) }).unwrap();
    let raw = &value["raw"];
    let recipe = &raw["recipe"];
    assert_eq!(
        recipe["values"]["tint"].as_f64().unwrap().to_bits(),
        CASES[0].1
    );
    assert_eq!(
        recipe["capabilities"]["defaults"]["tint"]
            .as_f64()
            .unwrap()
            .to_bits(),
        CASES[1].1
    );
    for ((literal, expected), actual) in CASES.iter().zip(recipe["samples"].as_array().unwrap()) {
        assert_eq!(
            actual.as_f64().unwrap().to_bits(),
            *expected,
            "decimal literal {literal} changed"
        );
    }
    assert_eq!(recipe["largest_integer"].as_u64(), Some(u64::MAX));
    assert_eq!(recipe["smallest_integer"].as_i64(), Some(i64::MIN));
    assert_eq!(recipe["text"], "11.999950408935547");
    assert_eq!(recipe["enabled"], true);
    raw.clone()
}

#[test]
fn byte_raw_descriptor_binary64_values_survive_ffi_and_repeated_native_roundtrips() {
    let input = source();
    let base = DrDocument::from_pixels(RgbaImage::from_pixel(2, 1, Rgba([0; 4])));
    let request=SourceEdit::parse(r#"{"op":"place","source_id":"camera","name":"Camera","x":0,"y":0,"depth_conversion":"convert_fallback"}"#).unwrap();
    let placed = base
        .source_edit_input(
            Some(SourceInput::Typed(&input)),
            &request,
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let mut error = ptr::null_mut();
    let descriptor = descriptor();
    let raw = unsafe {
        dr_doc_attach_raw_source(
            &placed,
            1,
            ORIGINAL.as_ptr(),
            ORIGINAL.len(),
            descriptor.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!raw.is_null(), "{}", unsafe { take_text(error) });
    assert!(error.is_null());
    let mut raw = *unsafe { Box::from_raw(raw) };
    let expected = check(unsafe { dr_doc_source_info(&raw, 1, &mut error) }, error);
    let folder = tempfile::TempDir::new().unwrap();
    let file = CString::new(folder.path().join("camera.rz").to_str().unwrap()).unwrap();
    for _ in 0..8 {
        assert!(
            unsafe { dr_doc_save_native(&raw, file.as_ptr(), &mut error) },
            "{}",
            unsafe { take_text(error) }
        );
        assert!(error.is_null());
        let opened = unsafe { dr_doc_open(file.as_ptr(), &mut error) };
        assert!(!opened.is_null(), "{}", unsafe { take_text(error) });
        assert!(error.is_null());
        raw = *unsafe { Box::from_raw(opened) };
        assert_eq!(
            check(unsafe { dr_doc_source_info(&raw, 1, &mut error) }, error),
            expected
        );
        let master = raw.source_master16(1).unwrap();
        assert_eq!(
            master.to_rgba16().ok(),
            input.layer_source(0).unwrap().to_rgba16().ok()
        );
    }
}
#[test]
fn typed_raw_descriptor_binary64_values_survive_ffi_and_repeated_native_roundtrips() {
    let input = source();
    let request = SourceEdit::parse(r#"{"op":"retain","source_id":"camera"}"#).unwrap();
    let placed = input
        .source_edit(None, &request, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .unwrap();
    let mut error = ptr::null_mut();
    let descriptor = descriptor();
    let raw = unsafe {
        dr_precision_doc_attach_raw_source(
            &placed,
            0,
            ORIGINAL.as_ptr(),
            ORIGINAL.len(),
            descriptor.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!raw.is_null(), "{}", unsafe { take_text(error) });
    assert!(error.is_null());
    let mut raw = *unsafe { Box::from_raw(raw) };
    let expected = check(
        unsafe { dr_precision_doc_source_info(&raw, 0, &mut error) },
        error,
    );
    let folder = tempfile::TempDir::new().unwrap();
    let file = CString::new(folder.path().join("camera.rz").to_str().unwrap()).unwrap();
    for _ in 0..8 {
        assert!(
            unsafe {
                dr_precision_doc_save_native(
                    &raw,
                    file.as_ptr(),
                    REFERENCE_WORKING_BYTES,
                    0,
                    ptr::null(),
                    &mut error,
                )
            },
            "{}",
            unsafe { take_text(error) }
        );
        assert!(error.is_null());
        let opened = unsafe {
            dr_precision_doc_open(
                file.as_ptr(),
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            )
        };
        assert!(!opened.is_null(), "{}", unsafe { take_text(error) });
        assert!(error.is_null());
        raw = *unsafe { Box::from_raw(opened) };
        assert_eq!(
            check(
                unsafe { dr_precision_doc_source_info(&raw, 0, &mut error) },
                error
            ),
            expected
        );
        let master = raw.source_master16(0).unwrap();
        assert_eq!(
            master.to_rgba16().ok(),
            input.layer_source(0).unwrap().to_rgba16().ok()
        );
    }
}
