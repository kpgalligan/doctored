//! One case of the float smart-selection admission measurement: the
//! working memory `precision_selection` charges a 32-bit float document for
//! the Magic Wand (`wand`), Color Range (`range`), Quick Selection (`quick`)
//! and Select and Mask's edge stage (`refine`) over the composite, each
//! comparing the R1 display copy, against
//! what each really holds. One case per process, run under
//! `MallocLargeCache=0` (and `/usr/bin/time -l`, whose whole-process peak is
//! the cross-check); the process reads its own peak over the call.
//!
//! ```text
//! float_selection_admission <wand|range|quick|refine> <run|twin>
//! ```
//!
//! The document is one opaque 6000×4000 float layer (the RZCB route of
//! `examples/float_paint_admission.rs`), so the composite render is part of
//! each call. `wand` is seeded at (0, 0) with tolerance 65535, contiguous, so
//! its flood covers the canvas; `range` is sampled at mid grey with
//! tolerance 50; `quick` is one seed at the centre, brush 201 (radius 100),
//! smooth 1 — the 16-bit record's case (`precision_selection`'s plan note);
//! `refine` is `refine_edge` at edge radius 8 with smart radius over a
//! centred rectangle of coverage (half the canvas each way).
//! The canvas-sized output buffer (and `refine`'s input coverage) is the
//! host's, allocated before the footprint is read.
//!
//! `twin` builds the document and the output and makes the call once under a
//! working limit of ONE byte, so it is refused before anything is allocated;
//! `run` does the same and then makes the call for real. Both modes print
//! `footprint_before_bytes`, `interval_peak_bytes` (the peak over the
//! interval that starts just before the first call,
//! `proc_reset_footprint_interval`) and `e_bytes`, their difference: the
//! call's own peak, held against `scratch_bytes` with the 1.25 margin.
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit_plan, dr_precision_doc_select};
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
    eprintln!("usage: float_selection_admission <wand|range|quick|refine> <run|twin>");
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

fn operation(case: &str) -> Value {
    match case {
        "wand" => json!({"kind":"magic_wand","x":0,"y":0,"tolerance":65535,"contiguous":true}),
        "range" => json!({"kind":"color_range","params":{"mode":"sampled",
            "samples":[[32768,32768,32768]],"tolerance":50,"softness":10}}),
        "quick" => json!({"kind":"quick","params":{"seeds":[[W / 2, H / 2]],"radius":100,
            "smooth":1}}),
        "refine" => json!({"kind":"refine_edge","edge_radius":8,"smart_radius":true}),
        _ => usage(),
    }
}

/// `rusage_info_v4` as `tests/float_adjust_apply_24mp.rs` reads it: a uuid,
/// then 35 words.
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

/// This process's `phys_footprint` or its peak since the interval was last
/// reset.
fn rusage(field: usize) -> u64 {
    let mut info = RusageInfoV4 {
        uuid: [0; 16],
        fields: [0; 35],
    };
    let status = unsafe { proc_pid_rusage(std::process::id() as i32, 4, &mut info) };
    assert_eq!(status, 0, "proc_pid_rusage");
    info.fields[field]
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
    let request = json!({"op":"selection","source":"composite","operation":operation(case)});
    let text = CString::new(request.to_string()).unwrap();
    let document = float_doc();
    let mut error = ptr::null_mut();
    let plan =
        unsafe { dr_precision_doc_edit_plan(&document, text.as_ptr(), UNLIMITED, 0, &mut error) };
    assert!(!plan.is_null(), "{}", taken(error));
    let plan: Value = serde_json::from_str(&taken(plan)).expect("the plan's JSON");
    let scratch = plan["scratch_bytes"].as_u64().expect("the plan's scratch");
    let mut output = vec![0u16; W as usize * H as usize];
    // `refine`'s coverage to refine: a centred rectangle half the canvas
    // each way.
    let input: Vec<u16> = if case == "refine" {
        (0..W as usize * H as usize)
            .map(|i| {
                let (x, y) = ((i % W as usize) as u32, (i / W as usize) as u32);
                let inside = (W / 4..3 * W / 4).contains(&x) && (H / 4..3 * H / 4).contains(&y);
                if inside {
                    u16::MAX
                } else {
                    0
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    let mut select = |limit: u64| {
        let mut error = ptr::null_mut();
        let ok = unsafe {
            dr_precision_doc_select(
                &document,
                text.as_ptr(),
                if input.is_empty() {
                    ptr::null()
                } else {
                    input.as_ptr()
                },
                input.len(),
                output.as_mut_ptr(),
                output.len(),
                limit,
                0,
                ptr::null(),
                &mut error,
            )
        };
        (ok, taken(error))
    };
    let before = rusage(PHYS_FOOTPRINT);
    unsafe { proc_reset_footprint_interval(std::process::id() as i32) };
    let (ok, refusal) = select(1);
    assert!(!ok, "a one-byte limit refuses");
    assert!(refusal.contains("working limit"), "{refusal}");
    if run {
        let (ok, error) = select(UNLIMITED);
        assert!(ok, "{error}");
    }
    let peak = rusage(INTERVAL_MAX_PHYS_FOOTPRINT);
    let selected = output.iter().filter(|v| **v > 0).count();
    let retained = document.retained_bytes();
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
            "selected_pixels": selected,
        })
    );
}
