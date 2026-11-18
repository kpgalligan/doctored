//! `layer_detail`'s two forms are admitted under their measured plans at
//! 24 MP: a 6000×4000 two-layer document, layer 1 (the target) detailed
//! against layer 0 (the source), both opaque and canvas-sized. The DIFFERENCE
//! form runs in a float sRGB document, the DECODED form in a 16-bit sRGB
//! document evaluated in linear light. One 24 MP document at a time.
//!
//! Each plan's scratch is at least ⌈1.25 × the largest measured footprint E⌉
//! of its form in `precision_layer_detail`'s module doc (`# Admission`): one
//! case per process, three runs, `MallocLargeCache=0`, a baseline twin.
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_evaluation::dr_precision_doc_set_evaluation;
use doctored_core::ffi_jobs::{dr_job_control_create, dr_job_control_free};
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;

#[path = "common/float_fixture.rs"]
mod float_fixture;

const W: u32 = 6000;
const H: u32 = 4000;
/// 8 GiB: `dr_precision_doc_edit_plan` and `dr_precision_doc_edit` admit
/// retained + scratch at the limit they are given, and the difference
/// document alone retains 768,000,000 bytes, so the 2 GiB pin could refuse
/// before any figure is read. Neither `scratch_bytes` nor the edit's work
/// depends on the limit.
const LIMIT: u64 = 8 << 30;
/// ⌈1.25 × max E⌉ of `measure`'s `difference`: E 769,933,888 / 769,933,888
/// / 769,901,120, the baseline twin reading E 0.
const DIFFERENCE_FLOOR: u64 = 962_417_360;
/// ⌈1.25 × max E⌉ of `measure`'s `decoded`: E 385,024,264 in each of three
/// runs, the baseline twin reading E 0.
const DECODED_FLOOR: u64 = 481_280_330;
const REQUEST: &str = r#"{"op":"layer_detail","version":1,"layer":1,"source":0}"#;

/// The target's pixel `i` at (`x`, `y`) as unit values: ramps across x and
/// y and a hashed blue.
fn target_unit(x: u32, y: u32, i: u64) -> [f32; 3] {
    [
        x as f32 / (W - 1) as f32,
        y as f32 / (H - 1) as f32,
        ((i * 2654435761) % 1000) as f32 / 999.0,
    ]
}

/// The source's pixel `i` as unit values: a hashed pattern unrelated to the
/// target.
fn source_unit(x: u32, y: u32, i: u64) -> [f32; 3] {
    [
        ((i * 40503) % 997) as f32 / 996.0,
        1.0 - y as f32 / (H - 1) as f32,
        ((x / 7 + y / 5) % 2) as f32 * 0.5 + ((i * 69069) % 251) as f32 / 500.0,
    ]
}

fn pixels<T>(f: impl Fn(u32, u32, u64) -> T) -> Vec<T> {
    let mut out = Vec::with_capacity(W as usize * H as usize);
    for y in 0..H {
        for x in 0..W {
            out.push(f(x, y, u64::from(y) * u64::from(W) + u64::from(x)));
        }
    }
    out
}

/// The float document: the target's ramps over [−0.25, 3.75] and
/// [0.02, 2.02] and a hashed blue in [0, 2] (`float_adjust_apply_24mp`'s
/// picture), the source over [0, 1.5].
fn difference_document() -> PrecisionDocument {
    let source = pixels(|x, y, i| {
        let [r, g, b] = source_unit(x, y, i);
        [1.5 * r, 1.5 * g, 1.5 * b, 1.0]
    });
    let target = pixels(|x, y, i| {
        let [r, g, b] = target_unit(x, y, i);
        [-0.25 + 4.0 * r, 0.02 + 2.0 * g, 2.0 * b, 1.0]
    });
    let doc = float_fixture::float_doc_at(LIMIT, W, H, &IccProfile::srgb(), &[source, target]);
    assert!(doc.is_float());
    doc
}

/// The 16-bit document: the same two pictures as codes over the full range,
/// evaluated in linear light.
fn decoded_document() -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let code = |v: f32| (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
    let layer = |name: &str, unit: fn(u32, u32, u64) -> [f32; 3]| {
        let codes = pixels(|x, y, i| {
            let [r, g, b] = unit(x, y, i);
            [code(r), code(g), code(b), u16::MAX]
        });
        let image =
            PrecisionImage::from_rgba16_unseeded(W, H, codes, profile.clone(), None).unwrap();
        PrecisionDocumentLayer::raster(name, image, (0, 0), 1.0, true).unwrap()
    };
    let layers = vec![layer("source", source_unit), layer("target", target_unit)];
    let encoded = PrecisionDocument::from_parts(
        W,
        H,
        profile.clone(),
        Default::default(),
        Default::default(),
        layers,
    )
    .unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_precision_doc_set_evaluation(&encoded, 1, &mut err) };
    assert!(!out.is_null(), "linear light: {}", take_err(err));
    drop(encoded);
    let doc = *unsafe { Box::from_raw(out) };
    assert!(!doc.is_float());
    doc
}

fn take_err(err: *mut c_char) -> String {
    if err.is_null() {
        return String::new();
    }
    let message = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    message
}

/// `(retained_bytes, scratch_bytes)` of [`REQUEST`]'s plan at `limit`.
fn plan(doc: &PrecisionDocument, limit: u64) -> (u64, u64) {
    let request = CString::new(REQUEST).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_precision_doc_edit_plan(doc, request.as_ptr(), limit, 0, &mut err) };
    assert!(!out.is_null(), "plan: {}", take_err(err));
    let text = unsafe { CStr::from_ptr(out) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(out) };
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    (
        value["retained_bytes"].as_u64().unwrap(),
        value["scratch_bytes"].as_u64().unwrap(),
    )
}

#[test]
fn layer_detail_plans_cover_the_measured_floors_at_24mp() {
    let doc = difference_document();
    let (retained, scratch) = plan(&doc, LIMIT);
    println!("difference: retained {retained} scratch {scratch}");
    assert!(
        scratch >= DIFFERENCE_FLOOR,
        "difference: {scratch} < floor {DIFFERENCE_FLOOR}"
    );
    drop(doc);
    let doc = decoded_document();
    let (retained, scratch) = plan(&doc, LIMIT);
    println!("decoded: retained {retained} scratch {scratch}");
    assert!(
        scratch >= DECODED_FLOOR,
        "decoded: {scratch} < floor {DECODED_FLOOR}"
    );
}

#[repr(C)]
struct RusageInfoV4 {
    uuid: [u8; 16],
    fields: [u64; 35],
}
const PHYS_FOOTPRINT: usize = 7;
const INTERVAL_MAX_PHYS_FOOTPRINT: usize = 33;

extern "C" {
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut RusageInfoV4) -> i32;
    fn proc_reset_footprint_interval(pid: i32) -> i32;
}

fn rusage() -> RusageInfoV4 {
    let mut info = RusageInfoV4 {
        uuid: [0; 16],
        fields: [0; 35],
    };
    let rc = unsafe { proc_pid_rusage(std::process::id() as i32, 4, &mut info) };
    assert_eq!(rc, 0, "proc_pid_rusage");
    info
}

/// One case per process, selected by `DR_LD_CASE`: `difference` or
/// `decoded`, or `base_difference` / `base_decoded`, the form's document
/// built and nothing run. The footprint is read with the document built, the
/// interval peak reset, and the edit run through `dr_precision_doc_edit`
/// under a job control, as the app runs it, with its result held until
/// E = interval peak − pre is read. `plan` is the figure the default test
/// asserts. `DR_LD_LIMIT` (bytes) replaces [`LIMIT`] for the plan and the
/// edit.
#[test]
#[ignore = "measurement: DR_LD_CASE=difference|decoded|base_difference|base_decoded, Release, MallocLargeCache=0"]
fn measure() {
    let case = std::env::var("DR_LD_CASE").expect("DR_LD_CASE");
    let (form, base) = match case.as_str() {
        "difference" => ("difference", false),
        "decoded" => ("decoded", false),
        "base_difference" => ("difference", true),
        "base_decoded" => ("decoded", true),
        _ => panic!("unknown case {case}"),
    };
    let limit = std::env::var("DR_LD_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(LIMIT);
    let doc = if form == "difference" {
        difference_document()
    } else {
        decoded_document()
    };
    let (retained, plan) = plan(&doc, limit);
    let pre = rusage().fields[PHYS_FOOTPRINT];
    unsafe { proc_reset_footprint_interval(std::process::id() as i32) };
    let started = std::time::Instant::now();
    let held = (!base).then(|| {
        let request = CString::new(REQUEST).unwrap();
        let control = unsafe { dr_job_control_create() };
        let mut err: *mut c_char = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_edit(
                &doc,
                request.as_ptr(),
                ptr::null(),
                0,
                limit,
                0,
                control,
                &mut err,
            )
        };
        unsafe { dr_job_control_free(control) };
        if !err.is_null() {
            panic!("{case}: {}", take_err(err));
        }
        assert!(!out.is_null(), "{case}: no change");
        unsafe { Box::from_raw(out) }
    });
    let seconds = started.elapsed().as_secs_f64();
    let peak = rusage().fields[INTERVAL_MAX_PHYS_FOOTPRINT];
    drop(held);
    let e = peak.saturating_sub(pre);
    let floor = (e * 5).div_ceil(4);
    println!(
        "LD_MEASURE case={case} retained={retained} plan={plan} pre={pre} peak={peak} E={e} floor={floor} seconds={seconds:.1}"
    );
    assert!(base || plan >= floor, "{case}: plan {plan} < floor {floor}");
}
