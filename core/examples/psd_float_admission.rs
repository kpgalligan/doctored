//! One case of the 32-bit PSD admission measurement: the working
//! memory `psd_export_float::save_psd_float` and `psd_import_float::open32`
//! charge against what they really hold. One case per process, Release,
//! `MallocLargeCache=0`.
//!
//! ```text
//! psd_float_admission <save|save-tiff|open|twin|twin-tiff> <width> <height> <file>
//! ```
//!
//! `save` builds one opaque `width × height` float layer (an RZCB payload,
//! extended-range samples that vary with position) and its projection, then
//! writes it as a layered 32-bit PSD to `file`; its plan is the save's own
//! admission, `worst_case_bytes + 28 · w · h` (no wide colour). `save-tiff`
//! and `twin-tiff` are `save` and `twin` for the 32-bit layered TIFF
//! (container 2), whose report gives that container's worst case. `open` opens
//! that file (written by an earlier `save` process) through Import Layers and
//! holds the document; its plan is `open32`'s for this one-record file,
//! restated here: retained `16 · w · h` plus the shared `peak_forecast` at four
//! bytes a sample and four planes. `twin` builds what `save` builds and makes
//! the save under a one-byte limit, which refuses before anything is
//! allocated. Every mode prints `footprint_before_bytes` (this process's
//! `phys_footprint` read just before the call), `interval_peak_bytes` (its
//! peak over the interval that starts there, `proc_reset_footprint_interval`)
//! and `e_bytes`, their difference, beside `plan_bytes`.
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_psd::{
    dr_precision_doc_open_psd, dr_precision_doc_psd_export_report, dr_precision_doc_save_psd,
    dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::precision_clipboard::ClipboardPayload;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

/// A working limit no plan reaches.
const UNLIMITED: u64 = u64::MAX / 4;

fn usage() -> ! {
    eprintln!(
        "usage: psd_float_admission <save|save-tiff|open|twin|twin-tiff> <width> <height> <file>"
    );
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

/// One opaque `w × h` float layer, through an RZCB v3 payload.
fn float_doc(w: u32, h: u32) -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let mut bytes = b"RZCB".to_vec();
    for word in [3u32, 2, w, h, 0, 0] {
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
    bytes.reserve(w as usize * h as usize * 16);
    for i in 0..u64::from(w) * u64::from(h) {
        let (x, y) = ((i % u64::from(w)) as f32, (i / u64::from(w)) as f32);
        let pixel = [
            -0.25 + 4.0 * x / w as f32,
            0.02 + 2.0 * y / h as f32,
            ((i * 2654435761) % 1000) as f32 / 500.0,
            1.0,
        ];
        pixel.iter().for_each(|v| bytes.extend(v.to_le_bytes()));
    }
    let payload = ClipboardPayload::decode(&bytes, UNLIMITED, 0).expect("a payload");
    drop(bytes);
    let doc =
        PrecisionDocument::from_clipboard(&payload, UNLIMITED, 0, None).expect("a float document");
    assert!(doc.is_float());
    doc
}

/// This process's current `phys_footprint`.
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

/// `rusage_info_v4` as `examples/float_paint_admission.rs` reads it: a uuid,
/// then 35 words; word 33 is the interval's peak footprint.
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

fn interval_peak() -> u64 {
    let mut info = RusageInfoV4 {
        uuid: [0; 16],
        fields: [0; 35],
    };
    let status = unsafe { proc_pid_rusage(std::process::id() as i32, 4, &mut info) };
    assert_eq!(status, 0, "proc_pid_rusage");
    info.fields[INTERVAL_MAX_PHYS_FOOTPRINT]
}

/// `open32`'s plan for a one-record file of `w × h` (module doc), restating
/// `peak_forecast`: source ×4, metadata ×8, retained and largest planes at
/// 16 B a pixel, the largest encoded channel ×2, the canvas ×4 and 64 MiB.
fn open_plan(w: u32, h: u32, file_bytes: u64) -> u64 {
    let pixels = u64::from(w) * u64::from(h);
    let encoded = pixels * 4 + 2;
    let metadata = 4096;
    let retained = pixels * 16;
    let scratch = file_bytes * 4
        + metadata * 8
        + pixels * 16
        + pixels * 16
        + encoded * 2
        + pixels * 4
        + 64 * 1024 * 1024;
    retained + scratch
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [mode, width, height, file] = &arguments[..] else {
        usage()
    };
    let (w, h): (u32, u32) = (width.parse().unwrap(), height.parse().unwrap());
    // The measurement's process limit (the 8 GiB of `float_promote_24mp`), so
    // the open's own decode envelope admits a 24 MP float file.
    assert!(doctored_core::ffi_resources::dr_set_process_working_limit(
        8 * 1024 * 1024 * 1024
    ));
    let path = CString::new(file.as_str()).unwrap();
    let mut error = ptr::null_mut();
    let (before, peak, plan) = match mode.as_str() {
        "save" | "twin" | "save-tiff" | "twin-tiff" => {
            let container = if mode.ends_with("-tiff") { 2 } else { 0 };
            let document = float_doc(w, h);
            let projection = document.render(UNLIMITED, 0, None).expect("projection");
            let report: Value = serde_json::from_str(&taken(unsafe {
                dr_precision_doc_psd_export_report(&document, container, 0, &mut error)
            }))
            .expect("report");
            let plan =
                report["worst_case_bytes"].as_u64().unwrap() + u64::from(w) * u64::from(h) * 28;
            let saves = mode.starts_with("save");
            let limit = if saves { UNLIMITED } else { 1 };
            let before = footprint();
            unsafe { proc_reset_footprint_interval(std::process::id() as i32) };
            let ok = unsafe {
                dr_precision_doc_save_psd(
                    &document,
                    &projection,
                    path.as_ptr(),
                    container,
                    0,
                    limit,
                    0,
                    std::ptr::null(),
                    &mut error,
                )
            };
            let peak = interval_peak();
            let message = taken(error);
            assert_eq!(ok, saves, "{message}");
            (before, peak, plan)
        }
        "open" => {
            let report: Value = serde_json::from_str(&taken(unsafe {
                dr_psd_import_report(path.as_ptr(), &mut error)
            }))
            .expect("import report");
            assert_eq!(report["import_precision"], "rgba32");
            let fingerprint = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
            let file_bytes = std::fs::metadata(file).unwrap().len();
            let before = footprint();
            unsafe { proc_reset_footprint_interval(std::process::id() as i32) };
            let document = unsafe {
                dr_precision_doc_open_psd(
                    path.as_ptr(),
                    0,
                    fingerprint.as_ptr(),
                    UNLIMITED,
                    0,
                    ptr::null(),
                    &mut error,
                )
            };
            // Held until the peak is read, as the app holds the result.
            let peak = interval_peak();
            assert!(!document.is_null(), "{}", taken(error));
            unsafe { doctored_core::ffi_precision::dr_precision_doc_free(document) };
            (before, peak, open_plan(w, h, file_bytes))
        }
        _ => usage(),
    };
    println!(
        "{}",
        json!({
            "mode": mode, "width": w, "height": h,
            "footprint_before_bytes": before,
            "interval_peak_bytes": peak,
            "e_bytes": peak.saturating_sub(before),
            "plan_bytes": plan,
        })
    );
}
