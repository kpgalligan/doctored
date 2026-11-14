//! The PSD export preflight's document-level losses for guides and layer
//! comps, at both depths and in both modes.
//!
//! The 16-bit preflight once reported no guides at all, and neither
//! depth named a document's layer comps. Each case here edits a plain
//! document through the FFI and asks for the export report again; the oracle
//! is that the edited document's losses are the plain document's losses with
//! the expected sentences appended, restated literally below.

use std::ffi::{c_char, c_int, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_comps::dr_doc_edit_layer_comps;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_guide::{
    dr_doc_add_guide, dr_doc_set_ruler_origin, dr_precision_doc_add_guide,
    dr_precision_doc_set_ruler_origin,
};
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{dr_doc_psd_export_report, dr_precision_doc_psd_export_report};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::RgbaImage;
use serde_json::{json, Value};

use crate::common;
use common::take_err_string;

/// `DrGuideOrientation`'s horizontal value, mirrored here.
const HORIZONTAL: c_int = 0;
const LAYERS: c_int = 0;
const FLATTENED: c_int = 1;

const GUIDES: &str = "Guides and the ruler origin are not written.";
const ONE_COMP: &str = "1 layer comp is not written; this export stores no Photoshop layer comps.";
const TWO_COMPS: &str =
    "2 layer comps are not written; this export stores no Photoshop layer comps.";

#[derive(Clone, Copy)]
enum Edit {
    Guide,
    Origin,
    Comp(&'static str),
}

fn comp_request(name: &str) -> CString {
    CString::new(json!({"op": "layer_comp", "action": "save", "name": name}).to_string()).unwrap()
}

fn take_report(json: *mut c_char, err: *mut c_char) -> Value {
    assert!(!json.is_null(), "{}", take_err_string(err));
    assert!(err.is_null());
    let value = serde_json::from_str(unsafe { CStr::from_ptr(json) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(json) };
    value
}

fn byte_reports(edits: &[Edit]) -> [Value; 2] {
    let base = DrDocument::from_pixels(RgbaImage::new(8, 8));
    let mut current: *mut DrDocument = ptr::null_mut();
    for edit in edits {
        let input: *const DrDocument = if current.is_null() { &base } else { current };
        let mut err = ptr::null_mut();
        let next = unsafe {
            match edit {
                Edit::Guide => dr_doc_add_guide(input, HORIZONTAL, 2.0),
                Edit::Origin => dr_doc_set_ruler_origin(input, 3.0, 4.0),
                Edit::Comp(name) => {
                    let request = comp_request(name);
                    dr_doc_edit_layer_comps(input, request.as_ptr(), &mut err)
                }
            }
        };
        assert!(!next.is_null(), "byte edit: {}", take_err_string(err));
        if !current.is_null() {
            unsafe { dr_doc_free(current) };
        }
        current = next;
    }
    let doc: *const DrDocument = if current.is_null() { &base } else { current };
    let reports = [LAYERS, FLATTENED].map(|mode| {
        let mut err = ptr::null_mut();
        take_report(
            unsafe { dr_doc_psd_export_report(doc, 0, mode, &mut err) },
            err,
        )
    });
    if !current.is_null() {
        unsafe { dr_doc_free(current) };
    }
    reports
}

fn typed_reports(edits: &[Edit]) -> [Value; 2] {
    let pixels = vec![[12345u16, 23456, 34567, 65535]; 64];
    let image = PrecisionImage::from_rgba16(8, 8, pixels, IccProfile::srgb(), None).unwrap();
    let base =
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap();
    let mut current: *mut PrecisionDocument = ptr::null_mut();
    for edit in edits {
        let input: *const PrecisionDocument = if current.is_null() { &base } else { current };
        let mut err = ptr::null_mut();
        let next = unsafe {
            match edit {
                Edit::Guide => dr_precision_doc_add_guide(input, HORIZONTAL, 2.0),
                Edit::Origin => dr_precision_doc_set_ruler_origin(input, 3.0, 4.0),
                Edit::Comp(name) => {
                    let request = comp_request(name);
                    dr_precision_doc_edit(
                        input,
                        request.as_ptr(),
                        ptr::null(),
                        0,
                        REFERENCE_WORKING_BYTES,
                        0,
                        ptr::null(),
                        &mut err,
                    )
                }
            }
        };
        assert!(!next.is_null(), "typed edit: {}", take_err_string(err));
        if !current.is_null() {
            unsafe { dr_precision_doc_free(current) };
        }
        current = next;
    }
    let doc: *const PrecisionDocument = if current.is_null() { &base } else { current };
    let reports = [LAYERS, FLATTENED].map(|mode| {
        let mut err = ptr::null_mut();
        take_report(
            unsafe { dr_precision_doc_psd_export_report(doc, 0, mode, &mut err) },
            err,
        )
    });
    if !current.is_null() {
        unsafe { dr_precision_doc_free(current) };
    }
    reports
}

fn strings(report: &Value, key: &str) -> Vec<String> {
    report[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} missing in {report}"))
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect()
}

/// At both depths and in both modes, the edited document's losses are the
/// plain document's losses followed by exactly `expected`.
fn assert_appended(edits: &[Edit], expected: &[&str]) {
    assert_appended_by_mode(edits, expected, expected);
}

/// [`assert_appended`] with the LAYERED mode's sentences apart from the
/// flattened mode's.
fn assert_appended_by_mode(edits: &[Edit], layered: &[&str], flattened: &[&str]) {
    let depths = [
        ("8-bit", byte_reports(&[]), byte_reports(edits)),
        ("16-bit", typed_reports(&[]), typed_reports(edits)),
    ];
    for (depth, plain, edited) in depths {
        for (mode, (plain, edited)) in ["layers", "flattened"]
            .iter()
            .zip(plain.iter().zip(&edited))
        {
            assert_eq!(edited["mode"], *mode);
            let expected = if *mode == "layers" {
                layered
            } else {
                flattened
            };
            let mut want = strings(plain, "losses");
            want.extend(expected.iter().map(|s| s.to_string()));
            assert_eq!(strings(edited, "losses"), want, "{depth} {mode}");
        }
    }
}

#[test]
fn a_guide_alone_is_disclosed_at_both_depths() {
    assert_appended(&[Edit::Guide], &[GUIDES]);
}

#[test]
fn a_moved_ruler_origin_alone_is_disclosed_at_both_depths() {
    assert_appended(&[Edit::Origin], &[GUIDES]);
}

#[test]
fn one_layer_comp_is_disclosed_in_the_singular() {
    // The layered mode writes the comp whole: no sentence.
    assert_appended_by_mode(&[Edit::Comp("A")], &[], &[ONE_COMP]);
}

#[test]
fn two_layer_comps_are_counted() {
    assert_appended_by_mode(&[Edit::Comp("A"), Edit::Comp("B")], &[], &[TWO_COMPS]);
}

#[test]
fn a_guide_and_a_comp_are_disclosed_in_order() {
    assert_appended_by_mode(
        &[Edit::Guide, Edit::Comp("A")],
        &[GUIDES],
        &[GUIDES, ONE_COMP],
    );
}

#[test]
fn plain_documents_name_neither_and_limitations_name_layer_comps() {
    for (depth, reports) in [("8-bit", byte_reports(&[])), ("16-bit", typed_reports(&[]))] {
        for report in &reports {
            let losses = strings(report, "losses");
            assert!(!losses.iter().any(|s| s == GUIDES), "{depth}: {losses:?}");
            assert!(
                !losses.iter().any(|s| s.contains("layer comp")),
                "{depth}: {losses:?}"
            );
            assert!(
                strings(report, "general_limitations")
                    .iter()
                    .any(|s| s.contains("layer comps")),
                "{depth}: {report}"
            );
        }
    }
}
