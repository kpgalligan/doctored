//! The conversion preflights and `dr_doc_flattened` take a job
//! control. A preserve-appearance preflight renders one baking projection per
//! encoded adjustment, so a Convert sheet's check can now be cancelled; the
//! 8-bit proof flatten runs under the request's control.
//!
//! Every assertion is load-independent (core/CLAUDE.md, "Load robustness"):
//!
//! - a NULL control and a fresh control give the IDENTICAL preflight JSON, at
//!   8 and 16 bits;
//! - a control cancelled before the call gives NULL + `operation cancelled`;
//! - a cancel with the worker PARKED halfway through the bakes' checkpoints
//!   (`common::probe::entry_count` for the entry, a fresh twin's own
//!   checkpoint total for the midpoint, `cancelled_parked_at` under its
//!   `Released` guard) gives `operation cancelled`, in the preflight at both
//!   depths and in `dr_doc_convert_to_profile_with`'s own preflight;
//! - `dr_doc_flattened` under a control equals the uncontrolled bytes, a
//!   cancel gives NULL, and a reused control gives `job control has already
//!   been used`.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_jobs::{dr_job_control_cancel, dr_job_control_create, dr_job_control_free};
use doctored_core::ffi_output_color::*;
use doctored_core::ffi_precision::*;
use doctored_core::ffi_precision_color::dr_precision_doc_add_adjustment;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use tempfile::TempDir;

use crate::common;
use common::probe::{cancelled_parked_at, entry_count};
use common::*;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 48;
const H: u32 = 32;
const PRESERVE: &str = r#"{"adjustments":"preserve_appearance"}"#;
const CURVES: &str = r#"{"rgb":[[0,0],[128,160],[255,255]]}"#;

fn p3() -> Vec<u8> {
    IccProfile::display_p3().bytes().to_vec()
}

fn opaque() -> RgbaImage {
    RgbaImage::from_fn(W, H, |x, y| {
        Rgba([(x * 5) as u8, (y * 7) as u8, ((x + y) * 3) as u8, 255])
    })
}

/// An opaque raster under three Curves adjustments: three bakes.
fn byte_doc(dir: &TempDir) -> *mut DrDocument {
    let mut doc = doc_from(dir, "base.png", &opaque());
    for i in 0..3 {
        doc = add_layer(dir, &format!("c{i}.png"), doc, i, &opaque(), "Curves");
        doc = set_meta(doc, 1 + i, &adjust_meta("curves", CURVES));
    }
    doc
}

/// The same stack at 16 bits.
fn typed_doc() -> *mut PrecisionDocument {
    let pixels: Vec<[u16; 4]> = opaque()
        .pixels()
        .map(|p| [0, 1, 2].map(|c| u16::from(p[c]) * 257))
        .map(|[r, g, b]| [r, g, b, u16::MAX])
        .collect();
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    let doc =
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap();
    let mut handle = Box::into_raw(Box::new(doc));
    let name = CString::new("Curves").unwrap();
    let description = CString::new(format!(
        r#"{{"type":"adjust","evaluation":"precision_v1","op":"curves","params":{CURVES}}}"#
    ))
    .unwrap();
    for _ in 0..3 {
        let mut err = ptr::null_mut();
        let next = unsafe {
            dr_precision_doc_add_adjustment(
                handle,
                name.as_ptr(),
                description.as_ptr(),
                1.0,
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!next.is_null(), "{}", take_err_string(err));
        unsafe { dr_precision_doc_free(handle) };
        handle = next;
    }
    handle
}

fn outcome(json: *mut c_char, err: *mut c_char) -> Result<String, String> {
    if json.is_null() {
        Err(take_err_string(err))
    } else {
        assert!(err.is_null());
        Ok(take_err_string(json))
    }
}

fn byte_preflight(doc: *const DrDocument, control: *const DrJobControl) -> Result<String, String> {
    let profile = p3();
    let options = CString::new(PRESERVE).unwrap();
    let mut err = ptr::null_mut();
    let json = unsafe {
        dr_doc_conversion_preflight(
            doc,
            profile.as_ptr(),
            profile.len(),
            options.as_ptr(),
            control,
            &mut err,
        )
    };
    outcome(json, err)
}

fn typed_preflight(
    doc: *const PrecisionDocument,
    control: *const DrJobControl,
) -> Result<String, String> {
    let profile = p3();
    let options = CString::new(PRESERVE).unwrap();
    let mut err = ptr::null_mut();
    let json = unsafe {
        dr_precision_doc_conversion_preflight(
            doc,
            profile.as_ptr(),
            profile.len(),
            options.as_ptr(),
            LIMIT,
            0,
            control,
            &mut err,
        )
    };
    outcome(json, err)
}

fn byte_convert(doc: *const DrDocument, control: *const DrJobControl) -> Result<(), String> {
    let profile = p3();
    let options = CString::new(PRESERVE).unwrap();
    let mut report = ptr::null_mut();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_convert_to_profile_with(
            doc,
            profile.as_ptr(),
            profile.len(),
            options.as_ptr(),
            control,
            &mut report,
            &mut err,
        )
    };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    if !report.is_null() {
        unsafe { dr_string_free(report) };
    }
    unsafe { dr_doc_free(out) };
    Ok(())
}

fn flattened(doc: *const DrDocument, control: *const DrJobControl) -> Result<Vec<u8>, String> {
    let mut err = ptr::null_mut();
    let image = unsafe { dr_doc_flattened(doc, 0, control, &mut err) };
    if image.is_null() {
        return Err(take_err_string(err));
    }
    let out = img_pixels(image);
    unsafe { dr_image_free(image) };
    Ok(out)
}

/// Runs `call` under a fresh control and answers its result and the
/// control's checkpoint total.
fn controlled<T>(call: impl FnOnce(*const DrJobControl) -> T) -> (T, u64) {
    let control = unsafe { dr_job_control_create() };
    let out = call(control);
    let total = unsafe { &*control }.checkpoints();
    unsafe { dr_job_control_free(control) };
    (out, total)
}

/// Runs `call` under a control cancelled before the call.
fn cancelled_before<T>(call: impl FnOnce(*const DrJobControl) -> T) -> T {
    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let out = call(control);
    unsafe { dr_job_control_free(control) };
    out
}

/// The parked midpoint of a run: the entry (`begin`'s report, from a fresh
/// twin's report log) plus half of what the twin polls after it.
fn midpoint(twin_entry: u64, twin_total: u64) -> u64 {
    assert!(
        twin_total > twin_entry + 2,
        "the run polls too few checkpoints to park inside it: {twin_entry}..{twin_total}"
    );
    twin_entry + 1 + (twin_total - twin_entry) / 2
}

fn token_control(token: usize) -> *const DrJobControl {
    token as *const DrJobControl
}

fn verdict<T>(result: Result<T, String>) -> (bool, String) {
    match result {
        Ok(_) => (true, String::new()),
        Err(message) => (false, message),
    }
}

#[test]
fn a_null_and_a_fresh_control_give_the_same_preflight_at_both_depths() {
    let dir = TempDir::new().unwrap();
    let doc = byte_doc(&dir);
    let plain = byte_preflight(doc, ptr::null()).expect("the byte preflight");
    let (with, _) = controlled(|c| byte_preflight(doc, c));
    assert_eq!(with.expect("the controlled byte preflight"), plain);
    assert_eq!(plain.matches("\"baked\"").count(), 3, "{plain}");
    unsafe { dr_doc_free(doc) };

    let typed = typed_doc();
    let plain = typed_preflight(typed, ptr::null()).expect("the typed preflight");
    let (with, _) = controlled(|c| typed_preflight(typed, c));
    assert_eq!(with.expect("the controlled typed preflight"), plain);
    assert_eq!(plain.matches("\"baked\"").count(), 3, "{plain}");
    unsafe { dr_precision_doc_free(typed) };
}

#[test]
fn a_cancelled_control_refuses_each_preflight() {
    let dir = TempDir::new().unwrap();
    let doc = byte_doc(&dir);
    let message = cancelled_before(|c| byte_preflight(doc, c)).unwrap_err();
    assert_eq!(message, "operation cancelled");
    unsafe { dr_doc_free(doc) };
    let typed = typed_doc();
    let message = cancelled_before(|c| typed_preflight(typed, c)).unwrap_err();
    assert_eq!(message, "operation cancelled");
    unsafe { dr_precision_doc_free(typed) };
}

#[test]
fn a_parked_mid_bake_cancel_ends_the_byte_preflight() {
    let dir = TempDir::new().unwrap();
    let entry = entry_count(&|_, _, _| true, |token| {
        let twin = byte_doc(&dir);
        let out = verdict(byte_preflight(twin, token_control(token)));
        unsafe { dr_doc_free(twin) };
        out
    });
    let twin = byte_doc(&dir);
    let (ok, total) = controlled(|c| byte_preflight(twin, c));
    ok.expect("the twin completes");
    unsafe { dr_doc_free(twin) };
    let doc = byte_doc(&dir);
    let handle = doc as usize;
    cancelled_parked_at(midpoint(entry, total), &|_, _| true, move |token| {
        verdict(byte_preflight(
            handle as *const DrDocument,
            token_control(token),
        ))
    });
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_parked_mid_bake_cancel_ends_the_typed_preflight() {
    let entry = entry_count(&|_, _, _| true, |token| {
        let twin = typed_doc();
        let out = verdict(typed_preflight(twin, token_control(token)));
        unsafe { dr_precision_doc_free(twin) };
        out
    });
    let twin = typed_doc();
    let (ok, total) = controlled(|c| typed_preflight(twin, c));
    ok.expect("the twin completes");
    unsafe { dr_precision_doc_free(twin) };
    let doc = typed_doc();
    let handle = doc as usize;
    cancelled_parked_at(midpoint(entry, total), &|_, _| true, move |token| {
        verdict(typed_preflight(
            handle as *const PrecisionDocument,
            token_control(token),
        ))
    });
    unsafe { dr_precision_doc_free(doc) };
}

#[test]
fn a_parked_cancel_ends_the_convert_inside_its_own_preflight() {
    // The convert's first checkpoints ARE its preflight's (it runs first,
    // under `checkpoints_only`), so the standalone preflight's midpoint
    // parks the convert inside the preflight: progress has not moved past
    // `begin`'s report.
    let dir = TempDir::new().unwrap();
    let entry = entry_count(&|_, _, _| true, |token| {
        let twin = byte_doc(&dir);
        let out = verdict(byte_preflight(twin, token_control(token)));
        unsafe { dr_doc_free(twin) };
        out
    });
    let twin = byte_doc(&dir);
    let (ok, total) = controlled(|c| byte_preflight(twin, c));
    ok.expect("the twin completes");
    unsafe { dr_doc_free(twin) };
    let doc = byte_doc(&dir);
    let handle = doc as usize;
    cancelled_parked_at(
        midpoint(entry, total),
        &|progress, stage| progress == 0.0 && stage == 1,
        move |token| {
            verdict(byte_convert(
                handle as *const DrDocument,
                token_control(token),
            ))
        },
    );
    unsafe { dr_doc_free(doc) };
}

#[test]
fn the_flattened_render_takes_a_single_use_control() {
    let dir = TempDir::new().unwrap();
    let doc = byte_doc(&dir);
    let plain = flattened(doc, ptr::null()).expect("the uncontrolled render");
    let (with, total) = controlled(|c| flattened(doc, c));
    assert_eq!(with.expect("the controlled render"), plain);

    let message = cancelled_before(|c| flattened(doc, c)).unwrap_err();
    assert_eq!(message, "operation cancelled");

    let control = unsafe { dr_job_control_create() };
    flattened(doc, control).expect("the first run");
    let message = flattened(doc, control).unwrap_err();
    assert_eq!(message, "job control has already been used");
    unsafe { dr_job_control_free(control) };

    // A cancel with the render parked halfway: NULL, never a partial image.
    let handle = doc as usize;
    cancelled_parked_at(midpoint(1, total), &|_, _| true, move |token| {
        verdict(flattened(handle as *const DrDocument, token_control(token)))
    });
    unsafe { dr_doc_free(doc) };
}
