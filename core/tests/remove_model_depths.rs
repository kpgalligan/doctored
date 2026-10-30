//! The Remove tool's model engine on a 32-bit float and a CMYK document
//! (M3a phase 2), through `dr_precision_doc_remove_model_*` with a STUB
//! plane in place of the model.
//!
//! The build table keeps both refusing by name; phase 2's Photoshop take
//! (`scripts/photoshop-probe/remove_tool_take.py`, take1) found no depth or
//! mode where Photoshop's Remove Tool ran, so it holds no reference for
//! either. The oracle is the header's contract: every export, every
//! sampling mode and both outputs answer the depth's own sentence, before
//! the layer index is looked at, and a refused input writes nothing into the
//! caller's buffers.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_remove_model::*;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;

use crate::cmyk_fixture::press_profile;
use crate::common::take_err_string;
use crate::float_fixture::float_doc;
use crate::sky_replacement::{build, Depth, Doc, LIMIT};

const SIDE: u32 = 64;
const W: u32 = 80;
const H: u32 = 60;
const NOT_FLOAT: &str = "remove's model is not available in a 32-bit float document";
const NOT_CMYK: &str = "remove's model is not available in a CMYK document";
const SAMPLES: [&str; 3] = ["current", "current_and_below", "all"];

fn disc() -> Vec<u16> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (f64::from(i % W) + 0.5, f64::from(i / W) + 0.5);
            if (x - 40.0).powi(2) + (y - 30.0).powi(2) <= 49.0 {
                65535
            } else {
                0
            }
        })
        .collect()
}

/// The stub model's plane: a flat mid grey.
fn stub_plane() -> Vec<f32> {
    vec![0.5; 3 * (SIDE * SIDE) as usize]
}

fn plan(doc: &PrecisionDocument, layer: usize, sample: &str, new_layer: bool) -> String {
    let name = CString::new(sample).unwrap();
    let mut err = ptr::null_mut();
    let bytes = unsafe {
        dr_precision_doc_remove_model_plan(doc, layer, name.as_ptr(), SIDE, new_layer, &mut err)
    };
    assert_eq!(bytes, 0);
    take_err_string(err)
}

/// The input's refusal, and whether the caller's buffers stayed untouched.
fn input(doc: &PrecisionDocument, layer: usize, sample: &str, coverage: &[u16]) -> (String, bool) {
    let name = CString::new(sample).unwrap();
    let cells = (SIDE * SIDE) as usize;
    let mut crop = [7u32; 4];
    let mut image = vec![-1f32; 3 * cells];
    let mut mask = vec![-1f32; cells];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_remove_model_input(
            doc,
            layer,
            name.as_ptr(),
            false,
            coverage.as_ptr(),
            coverage.len(),
            SIDE,
            crop.as_mut_ptr(),
            image.as_mut_ptr(),
            image.len(),
            mask.as_mut_ptr(),
            mask.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!ok);
    let untouched =
        crop == [7; 4] && image.iter().all(|&v| v == -1.0) && mask.iter().all(|&v| v == -1.0);
    (take_err_string(err), untouched)
}

fn apply(doc: &PrecisionDocument, layer: usize, coverage: &[u16], new_layer: bool) -> String {
    let plane = stub_plane();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_remove_model_apply(
            doc,
            layer,
            coverage.as_ptr(),
            coverage.len(),
            SIDE,
            plane.as_ptr(),
            plane.len(),
            new_layer,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    take_err_string(err)
}

fn assert_refuses(doc: &PrecisionDocument, sentence: &str, what: &str) {
    let coverage = disc();
    // A layer out of range still answers the depth first.
    for layer in [0, 5] {
        for sample in SAMPLES {
            for new_layer in [false, true] {
                assert_eq!(plan(doc, layer, sample, new_layer), sentence, "{what} plan");
                assert_eq!(
                    apply(doc, layer, &coverage, new_layer),
                    sentence,
                    "{what} apply"
                );
            }
            let (refusal, untouched) = input(doc, layer, sample, &coverage);
            assert_eq!(refusal, sentence, "{what} input {sample}");
            assert!(
                untouched,
                "{what} input {sample} wrote into the caller's buffers"
            );
        }
    }
}

#[test]
fn a_float_document_refuses_the_model_by_name_everywhere() {
    let n = (W * H) as usize;
    let float = float_doc(
        W,
        H,
        &IccProfile::srgb(),
        &[vec![[0.5, 0.4, 0.3, 1.0]; n], vec![[2.0, 0.1, 0.1, 1.0]; n]],
    );
    assert_refuses(&float, NOT_FLOAT, "float");
}

#[test]
fn a_cmyk_document_refuses_the_model_by_name_everywhere() {
    let pixels = vec![[0.5, 0.4, 0.3, 1.0]; (W * H) as usize];
    let rgb = match build(Depth::Sixteen, W, H, &[pixels]) {
        Doc::Typed(d) => Box::into_raw(Box::new(d)),
        Doc::Byte(_) => unreachable!(),
    };
    let profile = press_profile("Press Remove Depths");
    let mut err = ptr::null_mut();
    let cmyk = unsafe {
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
    assert!(!cmyk.is_null(), "{}", take_err_string(err));
    assert_refuses(unsafe { &*cmyk }, NOT_CMYK, "cmyk");
    // The 16-bit RGB twin the CMYK document came from runs the same calls.
    let coverage = disc();
    let twin = unsafe { &*rgb };
    let name = CString::new("all").unwrap();
    let mut err = ptr::null_mut();
    let bytes = unsafe {
        dr_precision_doc_remove_model_plan(twin, 0, name.as_ptr(), SIDE, false, &mut err)
    };
    assert!(bytes > 0, "{}", take_err_string(err));
    let plane = stub_plane();
    let out = unsafe {
        dr_precision_doc_remove_model_apply(
            twin,
            0,
            coverage.as_ptr(),
            coverage.len(),
            SIDE,
            plane.as_ptr(),
            plane.len(),
            false,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    unsafe {
        dr_precision_doc_free(out);
        dr_precision_doc_free(cmyk);
        dr_precision_doc_free(rgb);
    }
}
