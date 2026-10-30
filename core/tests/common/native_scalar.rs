#![allow(dead_code)]
//! Native scalar/source/selection qualification through additive C contracts.
//! Numerical expectations use original codes, direct integer/f64 arithmetic,
//! brute-force distances and a separately constructed Gaussian kernel.
use doctored_core::ffi_precision_edit::*;
use doctored_core::ffi_selection_u16::*;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};
use std::ffi::{CStr, CString};
use std::ptr;
pub const LIMIT: u64 = REFERENCE_WORKING_BYTES;
pub fn doc(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
        Default::default(),
        Default::default(),
    )
    .unwrap()
}
pub fn text(v: Value) -> CString {
    CString::new(v.to_string()).unwrap()
}
pub unsafe fn error(e: *mut std::ffi::c_char) -> String {
    if e.is_null() {
        return String::new();
    }
    let s = unsafe { CStr::from_ptr(e) }.to_string_lossy().into_owned();
    unsafe { doctored_core::ffi::dr_string_free(e) };
    s
}
pub fn plan(d: &PrecisionDocument, v: Value) -> Value {
    let t = text(v);
    let mut e = ptr::null_mut();
    let p = unsafe { dr_precision_doc_edit_plan(d, t.as_ptr(), LIMIT, 0, &mut e) };
    assert!(!p.is_null(), "{}", unsafe { error(e) });
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { doctored_core::ffi::dr_string_free(p) };
    serde_json::from_str(&s).unwrap()
}
pub fn edit(d: &PrecisionDocument, v: Value, input: &[u16]) -> Option<PrecisionDocument> {
    let p = plan(d, v.clone());
    assert_eq!(p["input_samples"].as_u64().unwrap() as usize, input.len());
    let t = text(v);
    let mut e = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            d,
            t.as_ptr(),
            input.as_ptr(),
            input.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    assert!(e.is_null(), "{}", unsafe { error(e) });
    if out.is_null() {
        None
    } else {
        Some(*unsafe { Box::from_raw(out) })
    }
}
pub fn scalar(
    d: &PrecisionDocument,
    target: &str,
    operation: Value,
    selected: bool,
    input: &[u16],
) -> Option<PrecisionDocument> {
    edit(
        d,
        json!({"op":"scalar_edit","target":target,"index":0,"operation":operation,"selection":selected}),
        input,
    )
}
pub fn plane(d: &PrecisionDocument, target: &str, canvas: bool) -> Vec<u16> {
    let req = json!({"op":"read_plane","target":target,"index":0,"canvas":canvas});
    let p = plan(d, req.clone());
    let mut out = vec![999; p["output_samples"].as_u64().unwrap() as usize];
    let mut e = ptr::null_mut();
    let t = text(req);
    let ok = unsafe {
        dr_precision_doc_read_plane(
            d,
            t.as_ptr(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    assert!(ok, "{}", unsafe { error(e) });
    out
}
pub fn select(d: &PrecisionDocument, operation: Value) -> Vec<u16> {
    let req = json!({"op":"selection","source":"layer","layer":0,"operation":operation});
    let p = plan(d, req.clone());
    let mut out = vec![999; p["output_samples"].as_u64().unwrap() as usize];
    let mut e = ptr::null_mut();
    let t = text(req);
    let ok = unsafe {
        dr_precision_doc_select(
            d,
            t.as_ptr(),
            ptr::null(),
            0,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    assert!(ok, "{}", unsafe { error(e) });
    out
}
pub fn selection(w: u32, h: u32, req: Value, input: &[u16]) -> Vec<u16> {
    let t = text(req);
    let mut e = ptr::null_mut();
    let p = unsafe { dr_selection_u16_plan(w, h, t.as_ptr(), LIMIT, 0, &mut e) };
    assert!(!p.is_null(), "{}", unsafe { error(e) });
    let v: Value = serde_json::from_slice(unsafe { CStr::from_ptr(p) }.to_bytes()).unwrap();
    unsafe { doctored_core::ffi::dr_string_free(p) };
    assert_eq!(v["input_samples"].as_u64().unwrap() as usize, input.len());
    let mut out = vec![999; (w * h) as usize];
    let ok = unsafe {
        dr_selection_u16(
            w,
            h,
            t.as_ptr(),
            input.as_ptr(),
            input.len(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    assert!(ok, "{}", unsafe { error(e) });
    out
}
pub fn q(v: f64) -> u16 {
    v.clamp(0., 65535.).round() as u16
}
