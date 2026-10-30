//! Work that runs under a job control is no longer refused for CPU
//! work. `precision_render::MAX_WORK_UNITS` (2·10⁹) stopped being a refusal
//! for a render, a mask feather, a selection source, a selected-pixels
//! transform and every typed geometry edit that is not a preview; memory
//! admission and Cancel bound them instead. Black-box through the C boundary
//! on uniform `dr_precision_doc_create_blank` documents, at the 2 GiB
//! reference limit.
//!
//! Every oracle is independent of the core: a refusal sentence written out
//! here, a work figure by arithmetic, and one run whose result is known
//! without running anything (the alpha of an opaque layer is 65535
//! everywhere). Nothing here measures time: the runs that would take seconds
//! are entered with their control already cancelled and must answer
//! `operation cancelled`, not a work refusal.
use crate::common;
use common::take_err_string;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_color::{dr_builtin_profile, dr_builtin_profile_len};
use doctored_core::ffi_jobs::{dr_job_control_cancel, dr_job_control_create, dr_job_control_free};
use doctored_core::ffi_precision::{dr_precision_doc_descriptor, dr_precision_doc_free};
use doctored_core::ffi_precision_edit::{
    dr_precision_doc_edit, dr_precision_doc_edit_plan, dr_precision_doc_select,
};
use doctored_core::ffi_precision_input::dr_precision_doc_create_blank;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;

use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// The bound these requests once refused at, written out.
const OLD_BOUND: u64 = 2_000_000_000;
const WHITE: [u16; 4] = [65535; 4];
const CANCELLED: &str = "operation cancelled";
/// The one work refusal a typed geometry plan still carries: a preview's.
const PREVIEW_REFUSAL: &str = "typed geometry exceeds the bounded pixel-work limit";

/// An owned blank document, freed on drop.
struct Blank(*mut PrecisionDocument);

impl Drop for Blank {
    fn drop(&mut self) {
        unsafe { dr_precision_doc_free(self.0) };
    }
}

/// An opaque white `w`×`h` 16-bit sRGB document of one layer.
fn blank(w: u32, h: u32) -> Blank {
    let len = dr_builtin_profile_len(0);
    let mut profile = vec![0u8; len];
    assert!(unsafe { dr_builtin_profile(0, profile.as_mut_ptr(), len) });
    let mut err: *mut c_char = ptr::null_mut();
    let document = unsafe {
        dr_precision_doc_create_blank(
            w,
            h,
            WHITE.as_ptr(),
            profile.as_ptr(),
            profile.len(),
            ptr::null(),
            LIMIT,
            0,
            &mut err,
        )
    };
    assert!(!document.is_null(), "{}", take_err_string(err));
    Blank(document)
}

fn owned_json(out: *mut c_char) -> Value {
    let text = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_owned();
    unsafe { dr_string_free(out) };
    serde_json::from_str(&text).unwrap()
}

/// `dr_precision_doc_edit_plan`: the plan JSON, or the core's sentence.
fn plan(document: &Blank, request: &Value) -> Result<Value, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_precision_doc_edit_plan(document.0, text.as_ptr(), LIMIT, 0, &mut err) };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    Ok(owned_json(out))
}

/// `dr_precision_doc_edit` under `control`: the result (owned), or the
/// core's sentence.
fn edit(
    document: &Blank,
    request: &Value,
    samples: &[u16],
    control: *const DrJobControl,
) -> Result<Option<Blank>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            document.0,
            text.as_ptr(),
            if samples.is_empty() {
                ptr::null()
            } else {
                samples.as_ptr()
            },
            samples.len(),
            LIMIT,
            0,
            control,
            &mut err,
        )
    };
    if !err.is_null() {
        assert!(out.is_null());
        return Err(take_err_string(err));
    }
    Ok((!out.is_null()).then(|| Blank(out)))
}

/// The run entered with its control already cancelled.
fn entered_cancelled(document: &Blank, request: &Value, samples: &[u16]) -> Option<String> {
    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let result = edit(document, request, samples, control);
    unsafe { dr_job_control_free(control) };
    result.err()
}

/// A turn by `degrees` about (`cx`, `cy`), `CGAffineTransform` order.
fn turn(degrees: f64, cx: f64, cy: f64) -> [f64; 6] {
    let (s, c) = degrees.to_radians().sin_cos();
    [c, s, -s, c, cx - cx * c + cy * s, cy - cx * s - cy * c]
}

#[test]
fn a_45_mp_resize_plans_past_the_old_bound_at_bicubic_and_lanczos3() {
    // 8192×5504 (45.1 MP) to 8000×5376: Resize is not a one-shot job, so
    // it once kept the interactive bound and refused at both filters.
    let document = blank(8192, 5504);
    for filter in ["bicubic", "lanczos3"] {
        let request = json!({"op":"resize","width":8000,"height":5376,"filter":filter});
        let plan = plan(&document, &request)
            .unwrap_or_else(|e| panic!("{filter}: the resize plan refused: {e}"));
        let work = plan["work_units"].as_u64().unwrap();
        assert!(
            work > OLD_BOUND,
            "{filter}: {work} units is within the old bound"
        );
        // The run is entered: no work refusal precedes the control.
        assert_eq!(
            entered_cancelled(&document, &request, &[]).as_deref(),
            Some(CANCELLED),
            "{filter}"
        );
    }
}

#[test]
fn a_24_mp_lanczos3_selected_pixels_transform_plans_and_is_entered() {
    // A 10° turn of a 6000×4000 layer about its centre at Lanczos3: the
    // landing is most of the canvas, and 24,000,000 px × 6² taps × 4 channels
    // is 3.46·10⁹ at the whole canvas. Both the landing and the companion
    // selection transform (24,000,000 × 36 = 8.6·10⁸, under) were charged
    // against the old bound.
    let (w, h) = (6000u32, 4000u32);
    let document = blank(w, h);
    let request = json!({"op":"transform_selected","layer":0,
        "matrix":turn(10., 3000., 2000.),"filter":"lanczos3"});
    let plan = plan(&document, &request)
        .unwrap_or_else(|e| panic!("the selected-pixels plan refused: {e}"));
    let n = u64::from(w) * u64::from(h);
    assert_eq!(plan["input_samples"], n);
    // The plan charges the landing at 16 bytes a pixel: more than half the
    // canvas lands (the turned rect's box clipped to the canvas).
    assert!(plan["scratch_bytes"].as_u64().unwrap() > 16 * n / 2);
    let coverage = vec![65535u16; n as usize];
    assert_eq!(
        entered_cancelled(&document, &request, &coverage).as_deref(),
        Some(CANCELLED)
    );
}

#[test]
fn an_80_mp_feathered_mask_no_longer_refuses_the_render_plan() {
    // A canvas-sized mask with any feather charges 32 units a padded pixel
    // before its taps: 80,000,000 × 32 = 2.56·10⁹, so the document's render
    // plan once refused and the descriptor carried the sentence.
    let (w, h) = (10000u32, 8000u32);
    let document = blank(w, h);
    let samples = vec![65535u16; w as usize * h as usize];
    let request = json!({"op":"set_mask","layer":0,"width":w,"height":h,"feather":2.0});
    let masked = edit(&document, &request, &samples, ptr::null())
        .unwrap_or_else(|e| panic!("set_mask refused: {e}"))
        .expect("a mask was added");
    drop(samples);
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_precision_doc_descriptor(masked.0, &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    let descriptor = owned_json(out);
    assert!(
        descriptor["render_refusal"].is_null(),
        "{}",
        descriptor["render_refusal"]
    );
    // The plan answered, so its figures are present, and they include the
    // feather's scratch: 32 bytes a padded mask pixel on top of the render.
    let scratch = descriptor["render_scratch_bytes"].as_u64().unwrap();
    assert!(scratch > 32 * u64::from(w) * u64::from(h), "{scratch}");
}

#[test]
fn an_alpha_source_selection_past_the_old_bound_completes() {
    // A source selection charged 40 units a canvas pixel, so the old bound
    // refused past 50,000,000 px. Its plan also charges 40 BYTES a pixel, so
    // at the 2 GiB reference limit memory admits up to about 53 MP (a
    // 10000×6000 canvas is refused for memory, correctly): 8000×6251 =
    // 50,008,000 px is the smallest such canvas over the old bound. The
    // layer source reads the layer's own samples (no composite render).
    let (w, h) = (8000u32, 6251u32);
    let n = w as usize * h as usize;
    assert!(n as u64 * 40 > OLD_BOUND);
    let document = blank(w, h);
    let request = json!({"op":"selection","source":"layer","layer":0,
        "operation":{"kind":"alpha"}});
    let plan =
        plan(&document, &request).unwrap_or_else(|e| panic!("the selection plan refused: {e}"));
    assert_eq!(plan["output_samples"], n as u64);
    assert_eq!(plan["input_samples"], 0);
    let text = CString::new(request.to_string()).unwrap();
    let mut out = vec![0u16; n];
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_select(
            document.0,
            text.as_ptr(),
            ptr::null(),
            0,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "{}", take_err_string(err));
    // The independent expectation: every pixel of the layer is opaque.
    let first_other = out.iter().position(|&v| v != 65535);
    assert_eq!(first_other, None, "a sample that is not 65535");
}

#[test]
fn an_interactive_transform_over_the_budget_still_refuses() {
    // The one geometry refusal kept: a ⌘T PREVIEW (`interactive`) of a 24 MP
    // layer at Catmull-Rom is 2.78·10⁹ units. Its sentence is unchanged; the
    // host answers it with a reduced proxy or the commit request as a
    // cancellable preview job. The commit twin plans.
    let document = blank(6000, 4000);
    let commit = json!({"op":"affine","layer":0,"target":"pixels",
        "matrix":turn(17., 3000., 2000.),"filter":"bicubic"});
    let planned = plan(&document, &commit)
        .unwrap_or_else(|e| panic!("the transform commit's plan refused: {e}"));
    assert!(planned["work_units"].as_u64().unwrap() > OLD_BOUND);
    let mut preview = commit;
    preview["interactive"] = json!(true);
    assert_eq!(
        plan(&document, &preview).err().as_deref(),
        Some(PREVIEW_REFUSAL)
    );
}
