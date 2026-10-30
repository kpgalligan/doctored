//! One case of the float retouch-brush admission measurement: the
//! working memory the plans charge a 32-bit float document for a Blur stroke,
//! a Sharpen stroke and a Sponge (Saturate) stroke (`retouch_brush`), a
//! Smudge stroke (`dr_precision_doc_smudge`) and a History Brush stroke
//! (`dr_precision_doc_history_brush`) on a 24 MP float layer, against what
//! each really holds. One case per process, run under `MallocLargeCache=0`
//! (and `/usr/bin/time -l`, whose whole-process peak is the cross-check); the
//! process reads its own peak over the call.
//!
//! ```text
//! float_retouch_brush_admission <blur|sharpen|sponge|smudge|smudge1000|smudgethin|history|blursal|smudgesal> <run|twin>
//! ```
//!
//! The document is one opaque 6000×4000 float layer (the RZCB route of
//! `examples/float_retouch_admission.rs`). Each case is one call its plan
//! admits:
//!
//! - `blur`, `sharpen`, `sponge` (`saturate`): canvas-wide coverage of 65535
//!   at strength 1, so the walk rect is the whole layer.
//! - `history`: the same coverage at opacity 0.5, painting from a second
//!   document of the same size whose samples differ everywhere. That document
//!   is built before the footprint is read, as the app holds a history state
//!   before a stroke starts.
//! - `smudge`: the 16-bit record's stroke (`precision_smudge`), a hard
//!   size-200 tip from (1000, 1000) to (1800, 1800) at strength 0.5.
//! - `smudge1000`: a hard size-1000 tip moved 150 px, from (2000, 2000) to
//!   (2150, 2000): a 1150×1000 footprint. A hard size-1000 sub-step costs
//!   about 1.58 million work units (its box plus the classified edge band), so
//!   162 px is the longest move inside the 256 million work bound and 200 px
//!   is refused.
//! - `smudgethin`: a hard size-20 tip from (100, 250) to (5900, 250): a
//!   5820×20 footprint across the tile-row boundary at y = 256, so the
//!   write-back rewrites 48 whole tiles under 116,400 footprint pixels.
//! - `blursal`, `smudgesal`: `blur` and `smudge` with Sample All
//!   Layers, so the plan adds the sampled picture (the canvas render of the
//!   layer, `sampled_plate_plan`) to the walk's term.
//!
//! `twin` builds the document and the inputs and makes the call once under a
//! working limit of ONE byte, so it is refused before anything is allocated;
//! `run` does the same and then makes the call for real and asserts a
//! document came back, so a request that changes nothing fails the run. The
//! plan is read from the case's own plan export: `scratch_bytes` is the
//! scratch the brush charges and `plan_bytes` is that plus the retained
//! document. Both modes print `footprint_before_bytes`, this process's
//! `phys_footprint` read just before the first call, `interval_peak_bytes`,
//! its peak over the interval that starts there
//! (`proc_reset_footprint_interval`, so building the document is outside it
//! and the twin reads no rise), and `e_bytes`, their difference: the call's
//! own peak, the figure held against `scratch_bytes` with the 1.25 margin
//! (`float_retouch_brush`'s module doc, `# Admission`).
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::ffi_retouch_brush::{
    dr_precision_doc_history_brush, dr_precision_doc_history_brush_plan, dr_precision_doc_smudge,
    dr_precision_doc_smudge_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::precision_clipboard::ClipboardPayload;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::{json, Value};

const W: u32 = 6000;
const H: u32 = 4000;
/// A working limit no plan reaches.
const UNLIMITED: u64 = u64::MAX / 4;
const HARD: &str =
    r#"{"version":1,"hardness":1.0,"roundness":1.0,"angle_degrees":0.0,"pixel":false}"#;
/// The smudge stroke's two dabs: x, y, diameter, opacity.
const DABS: [f64; 8] = [1000.0, 1000.0, 200.0, 1.0, 1800.0, 1800.0, 200.0, 1.0];
/// `smudge1000`: a size-1000 tip moved 150 px.
const DABS_1000: [f64; 8] = [2000.0, 2000.0, 1000.0, 1.0, 2150.0, 2000.0, 1000.0, 1.0];
/// `smudgethin`: a size-20 tip along y = 250, across the tile rows' boundary.
const DABS_THIN: [f64; 8] = [100.0, 250.0, 20.0, 1.0, 5900.0, 250.0, 20.0, 1.0];

fn usage() -> ! {
    eprintln!(
        "usage: float_retouch_brush_admission \
         <blur|sharpen|sponge|smudge|smudge1000|smudgethin|history|blursal|smudgesal> \
         <run|twin>"
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

/// `scratch_bytes` of a plan export's JSON.
fn scratch_of(out: *mut c_char, error: *mut c_char) -> u64 {
    assert!(!out.is_null(), "{}", taken(error));
    let plan: Value = serde_json::from_str(&taken(out)).expect("the plan's JSON");
    plan["scratch_bytes"].as_u64().expect("the plan's scratch")
}

/// An opaque extended-range sample that varies with position; `salt` makes a
/// second document differ from the first everywhere.
fn unit(i: u64, salt: f32) -> [f32; 4] {
    let (x, y) = ((i % u64::from(W)) as f32, (i / u64::from(W)) as f32);
    [
        -0.25 + 4.0 * x / W as f32 + salt,
        0.02 + 2.0 * y / H as f32 + salt,
        ((i * 2654435761) % 1000) as f32 / 500.0 + salt,
        1.0,
    ]
}

/// One opaque `W × H` float layer, through an RZCB v3 payload.
fn float_doc(salt: f32) -> PrecisionDocument {
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
        unit(i, salt)
            .iter()
            .for_each(|v| bytes.extend(v.to_le_bytes()));
    }
    let payload = ClipboardPayload::decode(&bytes, REFERENCE_WORKING_BYTES, 0).expect("a payload");
    drop(bytes);
    let doc = PrecisionDocument::from_clipboard(&payload, REFERENCE_WORKING_BYTES, 0, None)
        .expect("a float document");
    assert!(doc.is_float());
    doc
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

/// This process's `phys_footprint` (the figure whose peak `/usr/bin/time -l`
/// reports) or its peak since the interval was last reset.
fn rusage(field: usize) -> u64 {
    let mut info = RusageInfoV4 {
        uuid: [0; 16],
        fields: [0; 35],
    };
    let status = unsafe { proc_pid_rusage(std::process::id() as i32, 4, &mut info) };
    assert_eq!(status, 0, "proc_pid_rusage");
    info.fields[field]
}

/// One case's plan and call, each over inputs built before the footprint is
/// read.
struct Case {
    scratch: u64,
    call: Box<dyn Fn(u64) -> (*mut PrecisionDocument, String)>,
}

fn brush(document: &'static PrecisionDocument, kind: &str, sample: bool) -> Case {
    let text = CString::new(
        json!({"op":"retouch_brush","layer":0,"kind":kind,"strength":1.0,
            "sample_all_layers":sample})
        .to_string(),
    )
    .unwrap();
    let coverage = vec![u16::MAX; W as usize * H as usize];
    let mut error = ptr::null_mut();
    let plan =
        unsafe { dr_precision_doc_edit_plan(document, text.as_ptr(), UNLIMITED, 0, &mut error) };
    Case {
        scratch: scratch_of(plan, error),
        call: Box::new(move |limit| {
            let mut error = ptr::null_mut();
            let out = unsafe {
                dr_precision_doc_edit(
                    document,
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
        }),
    }
}

fn smudge(document: &'static PrecisionDocument, dabs: &'static [f64; 8], sample: bool) -> Case {
    let request = CString::new(HARD).unwrap();
    let mut error = ptr::null_mut();
    let plan = unsafe {
        dr_precision_doc_smudge_plan(
            document,
            0,
            request.as_ptr(),
            dabs.as_ptr(),
            dabs.len() / 4,
            false,
            sample,
            UNLIMITED,
            0,
            &mut error,
        )
    };
    Case {
        scratch: scratch_of(plan, error),
        call: Box::new(move |limit| {
            let mut error = ptr::null_mut();
            let out = unsafe {
                dr_precision_doc_smudge(
                    document,
                    0,
                    request.as_ptr(),
                    dabs.as_ptr(),
                    dabs.len() / 4,
                    0.5,
                    ptr::null(),
                    0,
                    sample,
                    limit,
                    0,
                    ptr::null(),
                    &mut error,
                )
            };
            (out, taken(error))
        }),
    }
}

fn history(document: &'static PrecisionDocument) -> Case {
    let source: &'static PrecisionDocument = Box::leak(Box::new(float_doc(0.5)));
    let coverage = vec![u16::MAX; W as usize * H as usize];
    let mut error = ptr::null_mut();
    let plan = unsafe {
        dr_precision_doc_history_brush_plan(document, 0, source, 0, false, UNLIMITED, 0, &mut error)
    };
    Case {
        scratch: scratch_of(plan, error),
        call: Box::new(move |limit| {
            let mut error = ptr::null_mut();
            let out = unsafe {
                dr_precision_doc_history_brush(
                    document,
                    0,
                    source,
                    0,
                    0.5,
                    coverage.as_ptr(),
                    coverage.len(),
                    limit,
                    0,
                    ptr::null(),
                    &mut error,
                )
            };
            (out, taken(error))
        }),
    }
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
    let document: &'static PrecisionDocument = Box::leak(Box::new(float_doc(0.0)));
    let measured = match case.as_str() {
        "blur" | "sharpen" => brush(document, case, false),
        "sponge" => brush(document, "saturate", false),
        "smudge" => smudge(document, &DABS, false),
        "smudge1000" => smudge(document, &DABS_1000, false),
        "smudgethin" => smudge(document, &DABS_THIN, false),
        "blursal" => brush(document, "blur", true),
        "smudgesal" => smudge(document, &DABS, true),
        "history" => history(document),
        _ => usage(),
    };
    let before = rusage(PHYS_FOOTPRINT);
    unsafe { proc_reset_footprint_interval(std::process::id() as i32) };
    let (out, refusal) = (measured.call)(1);
    assert!(out.is_null(), "a one-byte limit refuses");
    assert!(refusal.contains("working limit"), "{refusal}");
    let retained = document.retained_bytes();
    let mut result_bytes = 0;
    // Held until the interval's peak is read, as the app holds the result.
    let mut held = None;
    if run {
        let (out, error) = (measured.call)(UNLIMITED);
        assert!(!out.is_null(), "{error}");
        let edited = unsafe { Box::from_raw(out) };
        result_bytes = edited.retained_bytes();
        held = Some(edited);
    }
    let peak = rusage(INTERVAL_MAX_PHYS_FOOTPRINT);
    drop(held);
    println!(
        "{}",
        json!({
            "case": case,
            "mode": mode,
            "plan_bytes": retained + measured.scratch,
            "retained_bytes": retained,
            "scratch_bytes": measured.scratch,
            "footprint_before_bytes": before,
            "interval_peak_bytes": peak,
            "e_bytes": peak.saturating_sub(before),
            "result_retained_bytes": result_bytes,
        })
    );
}
