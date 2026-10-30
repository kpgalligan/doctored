//! `dr_image_filter_op`'s optional single-use `control` and
//! `dr_selection_resize`, the two core seams of the staged slider previews.
//! The cancel and controlled-versus-uncontrolled checks run on one
//! photographic op (`unsharp_mask`, through the shared Gaussian) and one
//! spatial op (`box_blur`); the uncontrolled output is the oracle, because the
//! kernels themselves are pinned by `sharpen_filters.rs` and
//! `spatial_filters.rs`. Mid-run cancels PARK the worker at a checkpoint taken
//! from a fresh twin's own report log, so nothing here depends on timing.
use crate::common;
use common::probe::{cancelled_parked_at, reports};
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba, dr_string_free};
use doctored_core::ffi_filters::dr_image_filter_op;
use doctored_core::ffi_jobs::{
    dr_job_control_cancel, dr_job_control_create, dr_job_control_free, dr_job_control_scratch_bytes,
};
use doctored_core::ffi_selection_tools::dr_selection_resize;
use doctored_core::job_control::DrJobControl;
use doctored_core::DrImage;
use std::ffi::{c_char, CStr, CString};
use std::ptr;

const OPS: [(&str, &str); 4] = [
    (
        "unsharp_mask",
        r#"{"radius":2.5,"amount":1.5,"threshold":0.01}"#,
    ),
    ("box_blur", r#"{"radius":4}"#),
    // Gaussian Blur below and above its σ4 switch.
    ("blur", r#"{"sigma":2}"#),
    ("blur", r#"{"sigma":6}"#),
];

fn plate(w: u32, h: u32) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            [
                ((i * 37 + 11) % 256) as u8,
                ((i * 13 + 5) % 256) as u8,
                ((i * 91 + 3) % 256) as u8,
                match i % 5 {
                    0 => 0,
                    1 => 255,
                    _ => ((i * 53 + 17) % 256) as u8,
                },
            ]
        })
        .collect()
}

fn image(w: u32, h: u32) -> *mut DrImage {
    let pixels = plate(w, h);
    let p = unsafe { dr_image_from_rgba(pixels.as_ptr(), w, h) };
    assert!(!p.is_null());
    p
}

/// `(pixels, message)`: `pixels` is None for a NULL result, `message` empty
/// when no error was written. Frees what it was handed.
fn run(
    img: *const DrImage,
    op: &str,
    params: &str,
    control: *const DrJobControl,
    count: usize,
) -> (Option<Vec<u8>>, String) {
    let op = CString::new(op).unwrap();
    let params = CString::new(params).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_image_filter_op(img, op.as_ptr(), params.as_ptr(), control, &mut err) };
    let pixels = (!out.is_null()).then(|| {
        let v =
            unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(out), count * 4) }.to_vec();
        unsafe { dr_image_free(out) };
        v
    });
    let message = if err.is_null() {
        String::new()
    } else {
        let m = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(err) };
        m
    };
    (pixels, message)
}

#[test]
fn controlled_output_equals_uncontrolled_output() {
    let (w, h) = (37, 29);
    let img = image(w, h);
    for (op, params) in OPS {
        let (plain, message) = run(img, op, params, ptr::null(), (w * h) as usize);
        assert!(message.is_empty(), "{op}: {message}");
        let control = unsafe { dr_job_control_create() };
        let (controlled, message) = run(img, op, params, control, (w * h) as usize);
        assert!(message.is_empty(), "{op}: {message}");
        assert!(
            plain.is_some() && plain == controlled,
            "{op}: controlled bytes differ"
        );
        assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
        // Single-use: the token is consumed by that one call.
        let (again, message) = run(img, op, params, control, (w * h) as usize);
        assert!(again.is_none());
        assert_eq!(message, "job control has already been used");
        unsafe { dr_job_control_free(control) };
    }
    unsafe { dr_image_free(img) };
}

#[test]
fn a_cancelled_control_refuses_before_validation_and_a_null_image_never_reads_it() {
    let (w, h) = (16, 12);
    let n = (w * h) as usize;
    let img = image(w, h);
    for (op, params) in OPS
        .into_iter()
        .chain([("gaussian_blur", "{}"), ("box_blur", "{")])
    {
        let control = unsafe { dr_job_control_create() };
        unsafe { dr_job_control_cancel(control) };
        let (out, message) = run(img, op, params, control, n);
        assert!(out.is_none());
        assert_eq!(message, "operation cancelled", "{op} {params}");
        unsafe { dr_job_control_free(control) };
    }
    // The control is claimed before the op and parameters are validated.
    let control = unsafe { dr_job_control_create() };
    let (_, message) = run(img, "gaussian_blur", "{}", control, n);
    assert!(message.contains("dr_image_filter_op supports"), "{message}");
    let (_, message) = run(img, "box_blur", "{}", control, n);
    assert_eq!(message, "job control has already been used");
    unsafe { dr_job_control_free(control) };
    // A NULL image is a silent refusal that leaves the token unused, and a
    // bypass under a control is NULL with no message.
    let control = unsafe { dr_job_control_create() };
    let (out, message) = run(ptr::null(), "box_blur", "{}", control, n);
    assert!(out.is_none() && message.is_empty());
    let (out, message) = run(img, "unsharp_mask", r#"{"amount":0}"#, control, n);
    assert!(out.is_none() && message.is_empty(), "{message}");
    unsafe { dr_job_control_free(control) };
    unsafe { dr_image_free(img) };
}

#[test]
fn a_mid_run_cancel_returns_operation_cancelled_and_nothing_else() {
    // 400 rows are four 128-row bands, so Gaussian Blur, which reports once
    // per band, has more than two stage-6 reports to park between.
    let (w, h) = (240, 400);
    let n = (w * h) as usize;
    let img = image(w, h) as usize;
    for (op, params) in OPS {
        let render = |token: usize| {
            let (out, message) = run(
                img as *const DrImage,
                op,
                params,
                token as *const DrJobControl,
                n,
            );
            (out.is_some(), message)
        };
        let log = reports(render);
        // Park inside the kernel's own plane stage (6), half way through it.
        let inside: Vec<u64> = log
            .iter()
            .filter(|r| r.stage == 6)
            .map(|r| r.checkpoints)
            .collect();
        assert!(inside.len() > 2, "{op}: the plane stage reports progress");
        let count = inside[inside.len() / 2];
        let parked = cancelled_parked_at(count, &|_, stage| stage == 6, render);
        assert_eq!(parked.count, count, "{op}");
    }
    unsafe { dr_image_free(img as *mut DrImage) };
}

/// `(written, message)`.
fn resize(mask: &[u8], w: u32, h: u32, ow: u32, oh: u32, filter: i32) -> (Option<Vec<u8>>, String) {
    let mut out = vec![7_u8; (ow as usize) * (oh as usize)];
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_selection_resize(
            mask.as_ptr(),
            w,
            h,
            ow,
            oh,
            filter,
            out.as_mut_ptr(),
            &mut err,
        )
    };
    let message = if err.is_null() {
        String::new()
    } else {
        let m = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(err) };
        m
    };
    if ok {
        (Some(out), message)
    } else {
        assert!(
            out.iter().all(|&v| v == 7),
            "a refusal leaves out_mask untouched"
        );
        (None, message)
    }
}

#[test]
fn selection_resize_is_the_document_plane_kernel() {
    let (w, h) = (61, 43);
    let mask: Vec<u8> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if (x as i64 - 30).pow(2) + (y as i64 - 21).pow(2) < 300 {
                255
            } else {
                ((x * 7 + y * 3) % 64) as u8
            }
        })
        .collect();
    let gray = image::GrayImage::from_raw(w, h, mask.clone()).unwrap();
    for (filter, kind) in [
        (0, image::imageops::FilterType::Nearest),
        (1, image::imageops::FilterType::Triangle),
        (2, image::imageops::FilterType::CatmullRom),
        (3, image::imageops::FilterType::Lanczos3),
    ] {
        for (ow, oh) in [(6, 4), (61, 43), (122, 86), (17, 40)] {
            let (out, message) = resize(&mask, w, h, ow, oh, filter);
            assert!(message.is_empty(), "{message}");
            let expected = image::imageops::resize(&gray, ow, oh, kind).into_raw();
            assert_eq!(out.unwrap(), expected, "filter {filter} to {ow}x{oh}");
        }
    }
    // Full and empty coverage stay full and empty under the bilinear proxy.
    for level in [0_u8, 255] {
        let flat = vec![level; (w * h) as usize];
        let (out, _) = resize(&flat, w, h, 6, 4, 1);
        assert!(out.unwrap().iter().all(|&v| v == level));
    }
}

#[test]
fn selection_resize_refuses_atomically() {
    let mask = vec![128_u8; 12];
    for (w, h, ow, oh, filter, fragment) in [
        (0, 4, 2, 2, 1, "dimensions"),
        (3, 4, 0, 2, 1, "dimensions"),
        (3, 4, 10_001, 10_000, 1, "dimensions"),
        (3, 4, 2, 2, 9, "filter"),
    ] {
        let (out, message) = resize(&mask, w, h, ow, oh, filter);
        assert!(out.is_none());
        assert!(message.contains(fragment), "{message}");
    }
}

#[test]
fn null_sweep() {
    let op = CString::new("unsharp_mask").unwrap();
    // A NULL image refuses before the control is read, with or without err_out.
    let control = unsafe { dr_job_control_create() };
    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe {
        dr_image_filter_op(ptr::null(), op.as_ptr(), ptr::null(), control, &mut err)
    }
    .is_null());
    assert!(err.is_null());
    assert!(unsafe {
        dr_image_filter_op(
            ptr::null(),
            op.as_ptr(),
            ptr::null(),
            control,
            ptr::null_mut(),
        )
    }
    .is_null());
    unsafe { dr_job_control_free(control) };
    // dr_selection_resize: NULL buffers return false and write nothing.
    let mask = [0_u8; 4];
    let mut out = [9_u8; 4];
    let mut err: *mut c_char = ptr::null_mut();
    assert!(!unsafe {
        dr_selection_resize(ptr::null(), 2, 2, 2, 2, 1, out.as_mut_ptr(), &mut err)
    });
    assert!(!unsafe {
        dr_selection_resize(mask.as_ptr(), 2, 2, 2, 2, 1, ptr::null_mut(), &mut err)
    });
    assert!(err.is_null() && out == [9; 4]);
    assert!(!unsafe {
        dr_selection_resize(ptr::null(), 2, 2, 2, 2, 1, ptr::null_mut(), ptr::null_mut())
    });
}

/// The seven 8-bit ops the Apply job runs under a control.
const SEVEN: [(&str, &str); 7] = [
    ("box_blur", r#"{"radius":4}"#),
    ("surface_blur", r#"{"radius":5,"threshold":15}"#),
    ("add_grain", r#"{"seed":1}"#),
    (
        "unsharp_mask",
        r#"{"radius":2.5,"amount":1.5,"threshold":0.01}"#,
    ),
    ("high_pass", r#"{"radius":3}"#),
    ("dust_and_scratches", r#"{"radius":2,"threshold":0}"#),
    (
        "reduce_noise",
        r#"{"radius":1,"luminance":0.01,"detail":0.5,"color":0.5}"#,
    ),
];

#[test]
fn every_byte_filter_op_reports_progress_and_cancels_mid_run() {
    let (w, h) = (240, 160);
    let n = (w * h) as usize;
    let img = image(w, h) as usize;
    for (op, params) in SEVEN {
        let (plain, message) = run(img as *const DrImage, op, params, ptr::null(), n);
        assert!(message.is_empty(), "{op}: {message}");
        assert!(plain.is_some(), "{op}: a bypass");
        let control = unsafe { dr_job_control_create() };
        let (controlled, message) = run(img as *const DrImage, op, params, control, n);
        unsafe { dr_job_control_free(control) };
        assert!(message.is_empty(), "{op}: {message}");
        assert!(plain == controlled, "{op}: controlled bytes differ");

        let render = |token: usize| {
            let (out, message) = run(
                img as *const DrImage,
                op,
                params,
                token as *const DrJobControl,
                n,
            );
            (out.is_some(), message)
        };
        let log = reports(render);
        assert!(
            log.iter().all(|r| (0.0..=1.0).contains(&r.progress)),
            "{op}: a progress outside [0, 1]"
        );
        assert!(
            log.iter()
                .any(|r| r.stage == 6 && r.progress > 0.0 && r.progress < 1.0),
            "{op}: no stage-6 report inside (0, 1)"
        );
        let last = log.last().expect("a report");
        assert!(
            last.progress == 1.0 && last.stage == 5,
            "{op}: the log ends at ({}, stage {})",
            last.progress,
            last.stage
        );
        let inside: Vec<u64> = log
            .iter()
            .filter(|r| r.stage == 6)
            .map(|r| r.checkpoints)
            .collect();
        assert!(inside.len() >= 2, "{op}: fewer than two stage-6 reports");
        // `checkpoint` parks before `report` stores its stage, so the second
        // stage-6 report parks with stage 6 already stored by the first.
        let count = inside[1];
        let parked = cancelled_parked_at(count, &|_, stage| stage == 6, render);
        assert_eq!(parked.count, count, "{op}");
    }
    unsafe { dr_image_free(img as *mut DrImage) };
}
