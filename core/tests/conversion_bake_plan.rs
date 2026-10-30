//! The 16-bit preserve-appearance Convert's own admission plan,
//! `dr_precision_doc_conversion_plan`. A 2000×1500 opaque u16 raster under
//! three Curves (three bakes):
//!
//! - its `scratch_bytes` is `precision_color_policy`'s `# Admission`
//!   formula, recomputed here from the descriptor's render scratch, the
//!   convert colour plan and the canvas plane — once with the raster
//!   canvas-sized, and once with a 1000×750 raster on the 2000×1500 canvas,
//!   where the canvas's excess over the largest converted plane is charged;
//! - a control whose scratch limit is that plan lets the Convert land, with
//!   three layers reported `"baked"` and named `… (baked)`;
//! - a control at the colour plan's scratch refuses with the working-memory
//!   sentence (the original refusal, now the oracle that the colour plan is
//!   too small);
//! - at keep and at the default options it is the convert colour plan, JSON
//!   for JSON.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::ffi_jobs::{
    dr_job_control_create, dr_job_control_free, dr_job_control_set_scratch_limit,
};
use doctored_core::ffi_output_color::dr_precision_doc_convert_to_profile_with;
use doctored_core::ffi_precision::{dr_precision_doc_descriptor, dr_precision_doc_free};
use doctored_core::ffi_precision_color::{
    dr_precision_doc_add_adjustment, dr_precision_doc_color_plan, dr_precision_doc_conversion_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::Value;

use crate::common::take_err_string;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 2000;
const H: u32 = 1500;
const PRESERVE: &str = r#"{"adjustments":"preserve_appearance"}"#;
const KEEP: &str = r#"{"adjustments":"keep"}"#;
const CURVES: &str = r#"{"rgb":[[0,0],[128,160],[255,255]]}"#;
/// `precision_image::BOOKKEEPING_BYTES`, the plan exports' fixed allowance.
const BOOKKEEPING_BYTES: u64 = 1024 * 1024;

fn p3() -> Vec<u8> {
    IccProfile::display_p3().bytes().to_vec()
}

/// An opaque canvas-sized u16 raster under three Curves adjustments.
fn typed_doc() -> *mut PrecisionDocument {
    typed_doc_with_raster(W, H)
}

/// An opaque `rw`×`rh` u16 raster at the origin of the W×H canvas, under
/// three Curves adjustments.
fn typed_doc_with_raster(rw: u32, rh: u32) -> *mut PrecisionDocument {
    let pixels: Vec<[u16; 4]> = (0..rw * rh)
        .map(|i| {
            let (x, y) = (i % rw, i / rw);
            [
                (x * 65535 / (rw - 1)) as u16,
                (y * 65535 / (rh - 1)) as u16,
                ((u64::from(i) * 2_654_435_761) % 65536) as u16,
                u16::MAX,
            ]
        })
        .collect();
    let image = PrecisionImage::from_rgba16(rw, rh, pixels, IccProfile::srgb(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("Background", image, (0, 0), 1.0, true).unwrap();
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap();
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

fn json(out: *mut c_char, err: *mut c_char) -> String {
    assert!(!out.is_null(), "{}", take_err_string(err));
    take_err_string(out)
}

fn conversion_plan(doc: *const PrecisionDocument, options: Option<&str>) -> String {
    let profile = p3();
    let options = options.map(|o| CString::new(o).unwrap());
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_conversion_plan(
            doc,
            profile.as_ptr(),
            profile.len(),
            options.as_ref().map_or(ptr::null(), |o| o.as_ptr()),
            LIMIT,
            0,
            &mut err,
        )
    };
    json(out, err)
}

fn color_plan(doc: *const PrecisionDocument) -> String {
    let profile = p3();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_color_plan(
            doc,
            profile.as_ptr(),
            profile.len(),
            true,
            LIMIT,
            0,
            &mut err,
        )
    };
    json(out, err)
}

fn scratch(text: &str) -> u64 {
    let value: Value = serde_json::from_str(text).unwrap();
    value["scratch_bytes"].as_u64().unwrap()
}

/// The preserve Convert to Display P3 under a control whose scratch limit is
/// `limit`: the converted document and its report, or the refusal.
fn convert(
    doc: *const PrecisionDocument,
    limit: u64,
) -> Result<(*mut PrecisionDocument, Value), String> {
    let profile = p3();
    let options = CString::new(PRESERVE).unwrap();
    let control = unsafe { dr_job_control_create() };
    assert!(unsafe { dr_job_control_set_scratch_limit(control, limit) });
    let mut report = ptr::null_mut();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_profile_with(
            doc,
            profile.as_ptr(),
            profile.len(),
            options.as_ptr(),
            LIMIT,
            0,
            control,
            &mut report,
            &mut err,
        )
    };
    unsafe { dr_job_control_free(control) };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    assert!(!report.is_null());
    let text = take_err_string(report);
    Ok((out, serde_json::from_str(&text).unwrap()))
}

#[test]
fn the_preserve_plan_is_the_documented_formula() {
    assert_documented_formula(W, H);
}

#[test]
fn a_raster_smaller_than_the_canvas_charges_the_canvas_excess() {
    assert_documented_formula(W / 2, H / 2);
}

/// The `# Admission` formula for a `rw`×`rh` raster on the W×H
/// canvas: B_max is the raster's plane, the largest the colour plan converts,
/// so the conversion phase adds B − B_max (0 when the raster fills the
/// canvas).
fn assert_documented_formula(rw: u32, rh: u32) {
    let doc = typed_doc_with_raster(rw, rh);
    let profile_allowance = 5 * p3().len() as u64 + BOOKKEEPING_BYTES;
    let mut err = ptr::null_mut();
    let descriptor: Value = serde_json::from_str(&json(
        unsafe { dr_precision_doc_descriptor(doc, &mut err) },
        err,
    ))
    .unwrap();
    let render = descriptor["render_scratch_bytes"].as_u64().unwrap();
    let color = scratch(&color_plan(doc)) - profile_allowance;
    let canvas = u64::from(W) * u64::from(H) * 8;
    let largest = u64::from(rw) * u64::from(rh) * 8;
    let k = 3;
    // The colour plan already charges the rebuild's contiguous plane at the
    // largest converted plane; the canvas adds only its excess over it.
    let excess = canvas - largest;
    if (rw, rh) != (W, H) {
        assert!(excess > 0);
    }
    let bake = render + (k - 1) * canvas;
    let conversion = color + 2 * k * canvas + excess;
    let peak = bake.max(conversion);
    let expected = peak + peak.div_ceil(4) + profile_allowance;
    let plan: Value = serde_json::from_str(&conversion_plan(doc, Some(PRESERVE))).unwrap();
    assert_eq!(plan["scratch_bytes"].as_u64(), Some(expected), "{plan}");
    let colour: Value = serde_json::from_str(&color_plan(doc)).unwrap();
    assert_eq!(plan["retained_bytes"], colour["retained_bytes"]);
    assert_eq!(
        plan["converted_pixel_bytes"].as_u64(),
        Some(colour["converted_pixel_bytes"].as_u64().unwrap() + k * canvas)
    );
    unsafe { dr_precision_doc_free(doc) };
}

#[test]
fn the_preserve_convert_lands_under_its_own_plan() {
    let doc = typed_doc();
    let plan = scratch(&conversion_plan(doc, Some(PRESERVE)));
    let (converted, report) = convert(doc, plan).expect("the Convert lands under its plan");
    let baked = report
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["action"] == "baked")
        .count();
    assert_eq!(baked, 3, "{report}");
    let document = unsafe { &*converted };
    let names: Vec<&str> = (0..document.layer_count())
        .map(|i| document.layer(i).unwrap().name())
        .collect();
    assert_eq!(
        names.iter().filter(|n| n.ends_with(" (baked)")).count(),
        3,
        "{names:?}"
    );
    unsafe { dr_precision_doc_free(converted) };
    unsafe { dr_precision_doc_free(doc) };
}

#[test]
fn the_colour_plan_alone_is_refused() {
    let doc = typed_doc();
    let colour = scratch(&color_plan(doc));
    let refusal = convert(doc, colour).map(|(d, _)| unsafe { dr_precision_doc_free(d) });
    let message = refusal.expect_err("the colour plan leaves out the bakes");
    assert!(message.contains("operation needs up to"), "{message}");
    unsafe { dr_precision_doc_free(doc) };
}

#[test]
fn keep_and_the_defaults_plan_as_the_colour_plan() {
    let doc = typed_doc();
    let colour = color_plan(doc);
    assert_eq!(conversion_plan(doc, Some(KEEP)), colour);
    assert_eq!(conversion_plan(doc, None), colour);
    unsafe { dr_precision_doc_free(doc) };
}
