//! One case of the float plane filter admission measurement:
//! the working memory `float_plane_filter` charges for a filter on one plane
//! of a 24 MP 32-bit float document, through `precision_scalar_edit`'s
//! `scalar_edit` request, against what the run really holds. The caller
//! measures this PROCESS's peak `phys_footprint` from outside
//! (`MallocLargeCache=0`), one case per process.
//!
//! ```text
//! float_plane_filter_admission <red_blur|red_surface|mask_blur> <run|twin>
//! ```
//!
//! The document is one opaque 6000×4000 float layer (the RZCB route of
//! `tests/selected_color_planes_24mp.rs`). `red_blur` is `blur` σ 2 on its
//! Red plane (linear), `red_surface` is `surface_blur` radius 5, threshold 15
//! on Red (the display-encoded copy), and `mask_blur` is `blur` σ 2 on a
//! canvas-sized mask holding a ramp, added before the footprint is read. No
//! selection: the whole plane is written. `twin` builds the document and
//! makes the call once under a working limit of ONE byte, so it is refused
//! before anything is allocated; `run` does the same and then makes the call
//! for real. The plan is read from `dr_precision_doc_edit_plan`:
//! `scratch_bytes` is the scratch the run charges and `plan_bytes` is that
//! plus the retained document. Both modes print `footprint_before_bytes`,
//! this process's `phys_footprint` read just before the first call,
//! `interval_peak_bytes`, its peak over the interval that starts there
//! (`proc_reset_footprint_interval`), and `e_bytes`, their difference: the
//! call's own peak, the figure held against `scratch_bytes` with the 1.25
//! margin (`float_plane_filter`'s module doc, `# Admission`).
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::precision_clipboard::ClipboardPayload;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::{json, Value};

const W: u32 = 6000;
const H: u32 = 4000;
/// A working limit no plan reaches.
const UNLIMITED: u64 = u64::MAX / 4;

fn usage() -> ! {
    eprintln!("usage: float_plane_filter_admission <red_blur|red_surface|mask_blur> <run|twin>");
    std::process::exit(2);
}

fn taken(pointer: *mut c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let text = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    text
}

/// The scratch the walk charges for `text` on `document`: the plan export's
/// `scratch_bytes`.
fn scratch_of(document: &PrecisionDocument, text: &CStr) -> u64 {
    let mut error = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_edit_plan(document, text.as_ptr(), UNLIMITED, 0, &mut error) };
    assert!(!out.is_null(), "{}", taken(error));
    let plan: Value = serde_json::from_str(&taken(out)).expect("the plan's JSON");
    plan["scratch_bytes"].as_u64().expect("the plan's scratch")
}

/// An opaque extended-range sample that varies with position.
fn unit(i: u64) -> [f32; 4] {
    let (x, y) = ((i % u64::from(W)) as f32, (i / u64::from(W)) as f32);
    [
        -0.25 + 4.0 * x / W as f32,
        0.02 + 2.0 * y / H as f32,
        ((i * 2654435761) % 1000) as f32 / 500.0,
        1.0,
    ]
}

/// One opaque `W × H` float layer, through an RZCB v3 payload.
fn float_doc() -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let mut bytes = b"RZCB".to_vec();
    for word in [3u32, 2, W, H, 0, 0] {
        bytes.extend(word.to_le_bytes());
    }
    bytes.extend(72f32.to_le_bytes());
    bytes.extend(72f32.to_le_bytes());
    bytes.extend((profile.bytes().len() as u32).to_le_bytes());
    bytes.extend(profile.bytes().iter());
    bytes.push(0);
    bytes.extend(0u32.to_le_bytes());
    bytes.extend(4f32.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes.reserve(W as usize * H as usize * 16);
    for i in 0..u64::from(W) * u64::from(H) {
        unit(i).iter().for_each(|v| bytes.extend(v.to_le_bytes()));
    }
    let payload = ClipboardPayload::decode(&bytes, REFERENCE_WORKING_BYTES, 0).expect("a payload");
    drop(bytes);
    let doc = PrecisionDocument::from_clipboard(&payload, REFERENCE_WORKING_BYTES, 0, None)
        .expect("a float document");
    assert!(doc.is_float());
    doc
}

fn request(case: &str) -> Value {
    let (target, op, params) = match case {
        "red_blur" => ("red", "blur", json!({"sigma": 2})),
        "red_surface" => ("red", "surface_blur", json!({"radius": 5, "threshold": 15})),
        "mask_blur" => ("mask", "blur", json!({"sigma": 2})),
        _ => usage(),
    };
    json!({"op":"scalar_edit","target":target,"index":0,"selection":false,
        "operation":{"kind":"filter","edit":{"layer":0,"op":op,"params":params}}})
}

/// `document` with a canvas-sized mask on its layer holding a ramp.
fn masked(document: &PrecisionDocument) -> PrecisionDocument {
    let text = CString::new(
        json!({"op":"set_mask","layer":0,"width":W,"height":H,"x":0,"y":0,"linked":true})
            .to_string(),
    )
    .unwrap();
    let ramp: Vec<u16> = (0..u64::from(W) * u64::from(H))
        .map(|i| (i % 65536) as u16)
        .collect();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            document,
            text.as_ptr(),
            ramp.as_ptr(),
            ramp.len(),
            UNLIMITED,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!out.is_null(), "{}", taken(error));
    *unsafe { Box::from_raw(out) }
}

/// This process's current `phys_footprint`, the figure whose peak
/// `/usr/bin/time -l` reports.
fn footprint() -> u64 {
    let mut info: libc::rusage_info_v0 = unsafe { std::mem::zeroed() };
    let status = unsafe {
        libc::proc_pid_rusage(
            std::process::id() as libc::c_int,
            libc::RUSAGE_INFO_V0,
            (&mut info as *mut libc::rusage_info_v0).cast(),
        )
    };
    assert_eq!(status, 0, "proc_pid_rusage");
    info.ri_phys_footprint
}

/// `rusage_info_v4` as `examples/float_retouch_brush_admission.rs` reads
/// it: a uuid, then 35 words; word 33 is the interval's peak footprint.
#[repr(C)]
struct RusageInfoV4 {
    uuid: [u8; 16],
    fields: [u64; 35],
}
const INTERVAL_MAX_PHYS_FOOTPRINT: usize = 33;

extern "C" {
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut RusageInfoV4) -> i32;
    fn proc_reset_footprint_interval(pid: i32) -> i32;
}

/// This process's peak `phys_footprint` since the interval was last reset.
fn interval_peak() -> u64 {
    let mut info = RusageInfoV4 {
        uuid: [0; 16],
        fields: [0; 35],
    };
    let status = unsafe { proc_pid_rusage(std::process::id() as i32, 4, &mut info) };
    assert_eq!(status, 0, "proc_pid_rusage");
    info.fields[INTERVAL_MAX_PHYS_FOOTPRINT]
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let (Some(case), Some(mode)) = (arguments.first(), arguments.get(1)) else {
        usage()
    };
    let run = match mode.as_str() {
        "run" => true,
        "twin" => false,
        _ => usage(),
    };
    let text = CString::new(request(case).to_string()).unwrap();
    let mut document = float_doc();
    if case == "mask_blur" {
        document = masked(&document);
    }
    // No selection: the request takes no input samples.
    let coverage: Vec<u16> = Vec::new();
    let edit = |limit: u64| {
        let mut error = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_edit(
                &document,
                text.as_ptr(),
                coverage.as_ptr(),
                coverage.len(),
                limit,
                0,
                ptr::null(),
                &mut error,
            )
        };
        (out, taken(error))
    };
    let scratch = scratch_of(&document, &text);
    let before = footprint();
    unsafe { proc_reset_footprint_interval(std::process::id() as i32) };
    let (out, refusal) = edit(1);
    assert!(out.is_null(), "a one-byte limit refuses");
    assert!(refusal.contains("working limit"), "{refusal}");
    let retained = document.retained_bytes();
    let mut result_bytes = 0;
    // Held until the interval's peak is read, as the app holds the result.
    let mut held = None;
    if run {
        let (out, error) = edit(UNLIMITED);
        assert!(!out.is_null(), "{error}");
        let filtered = unsafe { Box::from_raw(out) };
        result_bytes = filtered.retained_bytes();
        held = Some(filtered);
    }
    let peak = interval_peak();
    drop(held);
    println!(
        "{}",
        json!({
            "case": case,
            "mode": mode,
            "plan_bytes": retained + scratch,
            "retained_bytes": retained,
            "scratch_bytes": scratch,
            "footprint_before_bytes": before,
            "interval_peak_bytes": peak,
            "e_bytes": peak.saturating_sub(before),
            "result_retained_bytes": result_bytes,
        })
    );
}
