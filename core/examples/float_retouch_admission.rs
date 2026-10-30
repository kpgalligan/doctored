//! One case of the float retouch admission measurement: the working
//! memory `retouch_plan` charges a 32-bit float document for a Clone Stamp
//! stroke (`retouch_clone`), a heal (`retouch_heal`), a spot heal
//! (`spot_heal`), a Content-Aware Fill (`content_aware_fill`, in place and
//! onto a new layer) and Red Eye (`red_eye`) on a 24 MP float
//! layer, against what the retouch really
//! holds. The caller measures this PROCESS's peak `phys_footprint` from
//! outside (`/usr/bin/time -l`, `MallocLargeCache=0`), one case per process.
//!
//! ```text
//! float_retouch_admission <clone|heal|spot|fill|fill_new|red_eye> <run|twin>
//! ```
//!
//! The document is one opaque 6000×4000 float layer (the RZCB route of
//! `examples/float_paint_admission.rs`). Each case is one request its op's
//! parse admits:
//!
//! - `clone`: canvas-wide coverage of 65535, `matrix` a translation by
//!   (64, 0), so every pixel with a source changes.
//! - `heal`: coverage 65535 over the 1000×1000 box at (2500, 1500), the box
//!   as `x` / `y` / `width` / `height`, the same translation.
//! - `spot`, `fill`: the same box coverage and bounds, `ring` 0, `seed` 7.
//! - `fill_new`: `fill` with `output` `new_layer` and `sample` `all`.
//! - `red_eye`: the whole canvas as the rect and canvas-wide coverage of
//!   65535, pupil size 1 and darken 0.5: the largest encoded copy it takes.
//!
//! `twin` builds the document and the coverage and makes the call once under
//! a working limit of ONE byte, so it is refused before anything is
//! allocated; `run` does the same and then makes the call for real and
//! asserts a document came back, so a request that changes nothing fails the
//! run. The plan is read from `dr_precision_doc_edit_plan`: `scratch_bytes`
//! is the scratch the retouch charges and `plan_bytes` is that plus the
//! retained document. Both modes print `footprint_before_bytes`, this
//! process's `phys_footprint` read just before the first call: the run's peak
//! less that is the call's own peak, the figure held against `scratch_bytes`
//! with the 1.25 margin (`float_retouch`'s module doc, `# Admission`).
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
/// The healed, spot-healed and filled box: x, y, width, height.
const BOX: (u32, u32, u32, u32) = (2500, 1500, 1000, 1000);
/// A working limit no plan reaches.
const UNLIMITED: u64 = u64::MAX / 4;

fn usage() -> ! {
    eprintln!("usage: float_retouch_admission <clone|heal|spot|fill|fill_new|red_eye> <run|twin>");
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

/// The scratch the retouch charges for `text` on `document`: the plan
/// export's `scratch_bytes`.
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
    let (x, y, width, height) = BOX;
    let translate = [1.0, 0.0, 0.0, 1.0, 64.0, 0.0];
    match case {
        "clone" => json!({"op":"retouch_clone","layer":0,"matrix":translate}),
        "heal" => json!({"op":"retouch_heal","layer":0,"matrix":translate,
            "x":x,"y":y,"width":width,"height":height}),
        "spot" => json!({"op":"spot_heal","layer":0,
            "x":x,"y":y,"width":width,"height":height,"ring":0,"seed":7}),
        "fill" => json!({"op":"content_aware_fill","layer":0,
            "x":x,"y":y,"width":width,"height":height,"ring":0,"seed":7}),
        "fill_new" => json!({"op":"content_aware_fill","layer":0,
            "x":x,"y":y,"width":width,"height":height,"ring":0,"seed":7,
            "output":"new_layer","sample":"all"}),
        "red_eye" => json!({"op":"red_eye","layer":0,"x":0,"y":0,"width":W,"height":H,
            "pupil_size":1.0,"darken":0.5}),
        _ => usage(),
    }
}

/// The case's coverage, every sample written here so the twin holds it as
/// the run does: the whole canvas for `clone` and `red_eye`, the box for the
/// others.
fn coverage(case: &str) -> Vec<u16> {
    if case == "clone" || case == "red_eye" {
        return vec![u16::MAX; W as usize * H as usize];
    }
    let (x, y, width, height) = BOX;
    (0..W as usize * H as usize)
        .map(|i| {
            let (px, py) = ((i % W as usize) as u32, (i / W as usize) as u32);
            if (x..x + width).contains(&px) && (y..y + height).contains(&py) {
                u16::MAX
            } else {
                0
            }
        })
        .collect()
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
    let document = float_doc();
    let coverage = coverage(case);
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
    let (out, refusal) = edit(1);
    assert!(out.is_null(), "a one-byte limit refuses");
    assert!(refusal.contains("working limit"), "{refusal}");
    let retained = document.retained_bytes();
    let mut result_bytes = 0;
    if run {
        let (out, error) = edit(UNLIMITED);
        assert!(!out.is_null(), "{error}");
        let retouched = unsafe { Box::from_raw(out) };
        result_bytes = retouched.retained_bytes();
    }
    println!(
        "{}",
        json!({
            "case": case,
            "mode": mode,
            "plan_bytes": retained + scratch,
            "retained_bytes": retained,
            "scratch_bytes": scratch,
            "footprint_before_bytes": before,
            "result_retained_bytes": result_bytes,
        })
    );
}
