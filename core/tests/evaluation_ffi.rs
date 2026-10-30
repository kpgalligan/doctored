//! Phase 11E (F): the new exports' NULL behaviour and the two image
//! comparisons. The per-depth null sweeps in `doc_tests` and `precision_ffi`
//! carry the same calls; this file adds the ones that need a real pair of
//! images to say anything.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::*;
use doctored_core::ffi_evaluation::*;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::DrImage;
use image::{Rgba, RgbaImage};

fn take(pointer: *mut c_char) -> String {
    assert!(!pointer.is_null());
    let text = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    text
}

fn image(f: impl Fn(u32) -> [u8; 4]) -> *mut DrImage {
    let pixels = RgbaImage::from_fn(4, 1, |x, _| Rgba(f(x)));
    let bytes = pixels.as_raw();
    unsafe { dr_image_from_rgba(bytes.as_ptr(), 4, 1) }
}

#[test]
fn every_new_export_tolerates_null() {
    let null_doc: *const DrDocument = ptr::null();
    let null_typed: *const PrecisionDocument = ptr::null();
    let null_image: *const DrImage = ptr::null();
    let null_typed_image: *const PrecisionImage = ptr::null();
    unsafe {
        assert_eq!(dr_evaluation_from_name(ptr::null()), -1);
        let table = dr_evaluation_operators();
        assert!(!table.is_null());
        dr_string_free(table);

        assert_eq!(dr_doc_evaluation(null_doc), 0);
        assert!(dr_doc_linear_light_refusal(null_doc).is_null());
        let mut err: *mut c_char = ptr::null_mut();
        assert!(dr_doc_set_evaluation(null_doc, 1, &mut err).is_null());
        assert!(!take(err).is_empty(), "a NULL handle is an error");
        assert!(dr_doc_set_evaluation(null_doc, 1, ptr::null_mut()).is_null());
        let mut err: *mut c_char = ptr::null_mut();
        assert!(dr_doc_set_evaluation(null_doc, 7, &mut err).is_null());
        assert!(take(err).contains("policy"), "an unknown policy is named");

        assert_eq!(dr_precision_doc_evaluation(null_typed), 0);
        assert!(dr_precision_doc_linear_light_refusal(null_typed).is_null());
        let mut err: *mut c_char = ptr::null_mut();
        assert!(dr_precision_doc_set_evaluation(null_typed, 1, &mut err).is_null());
        assert!(!take(err).is_empty());
        assert!(dr_precision_doc_set_evaluation(null_typed, 1, ptr::null_mut()).is_null());

        let mut err: *mut c_char = ptr::null_mut();
        assert!(dr_image_compare(null_image, null_image, &mut err).is_null());
        assert!(!take(err).is_empty());
        assert!(dr_image_compare(null_image, null_image, ptr::null_mut()).is_null());
        let mut err: *mut c_char = ptr::null_mut();
        assert!(dr_precision_image_compare(null_typed_image, null_typed_image, &mut err).is_null());
        assert!(!take(err).is_empty());
        assert!(
            dr_precision_image_compare(null_typed_image, null_typed_image, ptr::null_mut())
                .is_null()
        );
    }
}

#[test]
fn compare_reports_equal_different_and_mismatched_images() {
    let a = image(|x| [(x * 10) as u8, 20, 30, 255]);
    let b = image(|x| [(x * 10) as u8, 20, 30, 255]);
    let c = image(|x| [(x * 10) as u8 + 3, 20, 30, 255]);
    unsafe {
        let mut err: *mut c_char = ptr::null_mut();
        let equal: serde_json::Value =
            serde_json::from_str(&take(dr_image_compare(a, b, &mut err))).unwrap();
        assert!(err.is_null());
        assert_eq!(equal["depth"], 8);
        assert_eq!(equal["width"], 4);
        assert_eq!(equal["max_abs"], 0);
        assert_eq!(equal["mean_abs"], 0.0);
        assert_eq!(equal["changed_fraction"], 0.0);

        let mut err: *mut c_char = ptr::null_mut();
        let different: serde_json::Value =
            serde_json::from_str(&take(dr_image_compare(a, c, &mut err))).unwrap();
        assert_eq!(different["max_abs"], 3);
        // One of four samples per pixel moved, by 3 each.
        assert_eq!(different["changed_fraction"], 0.25);
        assert!((different["mean_abs"].as_f64().unwrap() - 0.75).abs() < 1e-9);

        // A dimension mismatch is an ERROR, not a result.
        let tall = {
            let pixels = RgbaImage::from_pixel(4, 2, Rgba([1, 2, 3, 4]));
            dr_image_from_rgba(pixels.as_raw().as_ptr(), 4, 2)
        };
        let mut err: *mut c_char = ptr::null_mut();
        assert!(dr_image_compare(a, tall, &mut err).is_null());
        assert!(take(err).contains("dimensions"));

        for handle in [a, b, c, tall] {
            dr_image_free(handle);
        }
    }
}

#[test]
fn typed_compare_measures_codes_of_65535() {
    let profile = doctored_core::icc::IccProfile::srgb();
    let make = |shift: u16| {
        PrecisionImage::from_rgba16(
            4,
            1,
            (0..4u16)
                .map(|x| [x * 1000 + shift, 2000, 3000, 65535])
                .collect(),
            profile.clone(),
            None,
        )
        .expect("a typed fixture")
    };
    assert_eq!(make(0).storage(), PrecisionStorage::Rgba16Unorm);
    let a = Box::into_raw(Box::new(make(0)));
    let b = Box::into_raw(Box::new(make(0)));
    let c = Box::into_raw(Box::new(make(5)));
    unsafe {
        let mut err: *mut c_char = ptr::null_mut();
        let equal: serde_json::Value =
            serde_json::from_str(&take(dr_precision_image_compare(a, b, &mut err))).unwrap();
        assert_eq!(equal["depth"], 16);
        assert_eq!(equal["max_abs"], 0);

        let mut err: *mut c_char = ptr::null_mut();
        let different: serde_json::Value =
            serde_json::from_str(&take(dr_precision_image_compare(a, c, &mut err))).unwrap();
        assert_eq!(different["max_abs"], 5);
        assert_eq!(different["changed_fraction"], 0.25);
        for handle in [a, b, c] {
            drop(Box::from_raw(handle));
        }
    }
    let _ = REFERENCE_WORKING_BYTES;
    let _ = CString::new("unused").unwrap();
}
