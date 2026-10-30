//! A one-shot destructive filter under a host job control has no work
//! ceiling and is admitted by memory alone; without a control it keeps
//! `MAX_DESTRUCTIVE_WORK` (24e9 units) in the same words, and every plan
//! carries no work refusal. Black-box through the C boundary, on the three
//! paths the rule covers: the byte `dr_image_filter_op`, the 16-bit
//! `dr_precision_doc_selected_edit` and a scalar-plane `scalar_edit` filter
//! through `dr_precision_doc_edit`. The oracles are the published refusal
//! sentences and the documented work formulas: Dust & Scratches r64 at
//! 480×320 is n·(16k + 364) ≈ 3.16e10 units (`sampling_rank::plan_bounded`,
//! k the r64 disc), and Surface Blur r100/T255 over a 1×1,000,000 column is
//! above the ceiling by its slide term alone. Each controlled run is cut short
//! by `entry_then_cancel` once its kernel has reported past its entry, which
//! proves it ran past the uncontrolled ceiling's refusal.
use crate::common::probe::entry_then_cancel;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_string_free};
use doctored_core::ffi_filters::{dr_image_filter_op, dr_image_filter_op_scratch_bytes};
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::ffi_selected_adjustment::{
    dr_precision_doc_selected_edit, dr_precision_doc_selected_edit_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::DrImage;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const RANK_REFUSAL: &str = "rank source filter exceeds the bounded pixel-work limit";
const SURFACE_REFUSAL: &str = "surface blur exceeds the bounded pixel-work limit";

/// The two over-ceiling cases: (width, height, op, params, uncontrolled refusal).
fn cases() -> [(u32, u32, &'static str, Value, &'static str); 2] {
    [
        (
            480,
            320,
            "dust_and_scratches",
            json!({"radius": 64}),
            RANK_REFUSAL,
        ),
        (
            1,
            1_000_000,
            "surface_blur",
            json!({"radius": 100, "threshold": 255}),
            SURFACE_REFUSAL,
        ),
    ]
}

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

unsafe fn text(p: *mut c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    s
}

/// An opaque splitmix RGBA8 image.
fn byte_image(w: u32, h: u32) -> *mut DrImage {
    let mut state = 0x454_u64;
    let bytes: Vec<u8> = (0..u64::from(w) * u64::from(h))
        .flat_map(|_| {
            let v = splitmix(&mut state);
            [v as u8, (v >> 8) as u8, (v >> 16) as u8, 255]
        })
        .collect();
    let img = unsafe { dr_image_from_rgba(bytes.as_ptr(), w, h) };
    assert!(!img.is_null());
    img
}

/// An opaque splitmix 16-bit one-layer document.
fn precision_doc(w: u32, h: u32) -> PrecisionDocument {
    let mut state = 0x454_u64;
    let pixels: Vec<[u16; 4]> = (0..u64::from(w) * u64::from(h))
        .map(|_| {
            let v = splitmix(&mut state);
            [v as u16, (v >> 16) as u16, (v >> 32) as u16, 65535]
        })
        .collect();
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

/// (b) and (c) for one case: the uncontrolled `run(null)` errs with exactly
/// `refusal`; the controlled run reports past `entry` before it is cancelled.
fn uncontrolled_refuses_and_controlled_runs(
    what: &str,
    refusal: &str,
    entry: &dyn Fn(f64, u32, u64) -> bool,
    run: &(dyn Fn(*const DrJobControl) -> (bool, String) + Sync),
) {
    let (ok, message) = run(ptr::null());
    assert!(!ok, "{what}: the uncontrolled run was admitted");
    assert_eq!(message, refusal, "{what}: uncontrolled");
    let (at, _) = entry_then_cancel(entry, |token| run(token as *const DrJobControl));
    assert!(at.progress > 0.0, "{what}: {at:?}");
}

#[test]
fn byte_filter_op_runs_past_the_ceiling_under_a_control() {
    for (w, h, op, params, refusal) in cases() {
        let what = format!("byte {op} at {w}x{h}");
        let img = byte_image(w, h);
        let name = CString::new(op).unwrap();
        let params = CString::new(params.to_string()).unwrap();
        // (a) The plan carries no work refusal.
        let mut err: *mut c_char = ptr::null_mut();
        let scratch = unsafe {
            dr_image_filter_op_scratch_bytes(img, name.as_ptr(), params.as_ptr(), &mut err)
        };
        assert_eq!(unsafe { text(err) }, "", "{what}: plan");
        assert!(scratch > 0, "{what}: plan");
        let image = img as usize;
        let run = |control: *const DrJobControl| {
            let mut err: *mut c_char = ptr::null_mut();
            let out = unsafe {
                dr_image_filter_op(
                    image as *const DrImage,
                    name.as_ptr(),
                    params.as_ptr(),
                    control,
                    &mut err,
                )
            };
            let ok = !out.is_null();
            unsafe { dr_image_free(out) };
            (ok, unsafe { text(err) })
        };
        uncontrolled_refuses_and_controlled_runs(
            &what,
            refusal,
            &|progress, _, _| progress > 0.0,
            &run,
        );
        unsafe { dr_image_free(img) };
    }
}

#[test]
fn selected_edit_runs_past_the_ceiling_under_a_control() {
    for (w, h, op, params, refusal) in cases() {
        let what = format!("16-bit selected {op} at {w}x{h}");
        let d = precision_doc(w, h);
        let request =
            CString::new(json!({"layer": 0, "op": op, "params": params}).to_string()).unwrap();
        // (a) The plan carries no work refusal.
        let mut err: *mut c_char = ptr::null_mut();
        let plan = unsafe {
            dr_precision_doc_selected_edit_plan(&d, request.as_ptr(), LIMIT, 0, &mut err)
        };
        assert_eq!(unsafe { text(err) }, "", "{what}: plan");
        assert!(!plan.is_null(), "{what}: plan");
        unsafe { dr_string_free(plan) };
        let run = |control: *const DrJobControl| {
            let mut err: *mut c_char = ptr::null_mut();
            let out = unsafe {
                dr_precision_doc_selected_edit(
                    &d,
                    request.as_ptr(),
                    ptr::null(),
                    0,
                    LIMIT,
                    0,
                    control,
                    &mut err,
                )
            };
            let ok = !out.is_null();
            if ok {
                drop(unsafe { Box::from_raw(out) });
            }
            (ok, unsafe { text(err) })
        };
        // The plane stage is the run's 0.05..0.65 range.
        uncontrolled_refuses_and_controlled_runs(
            &what,
            refusal,
            &|progress, _, _| progress > 0.05,
            &run,
        );
    }
}

#[test]
fn scalar_plane_filter_runs_past_the_ceiling_under_a_control() {
    let (w, h) = (480_u32, 320_u32);
    let what = "scalar red-plane dust_and_scratches r64";
    let d = precision_doc(w, h);
    let request = CString::new(
        json!({"op": "scalar_edit", "target": "red", "index": 0, "selection": false,
               "operation": {"kind": "filter",
                             "edit": {"layer": 0, "op": "dust_and_scratches",
                                      "params": {"radius": 64}}}})
        .to_string(),
    )
    .unwrap();
    // (a) The plan carries no work refusal.
    let mut err: *mut c_char = ptr::null_mut();
    let plan = unsafe { dr_precision_doc_edit_plan(&d, request.as_ptr(), LIMIT, 0, &mut err) };
    assert_eq!(unsafe { text(err) }, "", "{what}: plan");
    assert!(!plan.is_null(), "{what}: plan");
    unsafe { dr_string_free(plan) };
    let run = |control: *const DrJobControl| {
        let mut err: *mut c_char = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_edit(
                &d,
                request.as_ptr(),
                ptr::null(),
                0,
                LIMIT,
                0,
                control,
                &mut err,
            )
        };
        let ok = !out.is_null();
        if ok {
            drop(unsafe { Box::from_raw(out) });
        }
        (ok, unsafe { text(err) })
    };
    uncontrolled_refuses_and_controlled_runs(
        what,
        RANK_REFUSAL,
        &|progress, _, _| progress > 0.0,
        &run,
    );
}
