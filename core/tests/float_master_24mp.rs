//! A 32-bit float master's place, re-render, save and reopen in
//! a 24 MP float document stay admitted at or above their measured floors. A
//! 6000×4000 float sRGB document; 6000×4000 f32 masters in sRGB and Display
//! P3, each placed at 0,0 (`dr_precision_doc_mixed_source_plan` /
//! `dr_precision_doc_mixed_source_edit`); the measured 140 % Bilinear
//! `affine_set` about the centre; the typed native plan of the document with
//! the sRGB master placed.
//!
//! Each floor is ⌈1.25 × the largest measured footprint E⌉ in
//! `retained_source_float`'s module doc (place, transform),
//! `# Measurement record (32-bit float master)`, and `float_native`'s (save,
//! reopen), `# Measurement record (phase 1)`: one case per process, three
//! runs, `MallocLargeCache=0`, a baseline twin.
//! One 24 MP document at a time: every `#[test]` holds `ONE_AT_A_TIME`.
//!
//! Also, an f32 master (the scene's profile, and Display P3)
//! placed with `convert_fallback` into white 6000×4000 16- and 8-bit sRGB
//! documents, the 16-bit record's 140 % transform, the save and reopen plans
//! of both, and the narrowing to 16 bits of a float document holding the
//! master stay at or above the floors in `float_native`'s module doc,
//! `# Measurement record (phase 3a)`.
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_retained_source::{
    dr_doc_mixed_source_edit, dr_doc_mixed_source_plan, dr_precision_doc_mixed_source_edit,
    dr_precision_doc_mixed_source_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_conversion_narrow::NarrowTarget;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{set_process_working_limit, PrecisionImage};
use doctored_core::precision_native::open_native_observed;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

#[path = "common/float_fixture.rs"]
mod float_fixture;

const W: u32 = 6000;
const H: u32 = 4000;
/// The working limit the measurement planned at (a 16 GB Mac's).
const LIMIT: u64 = 8_589_934_592;
/// The measured re-render's scale (150 % exceeded the native decoded cap,
/// 1.6 GB when this was measured; 8 GiB now).
const SCALE: f64 = 1.4;
/// ⌈1.25 × max E⌉ of the measured MCP `place_image`: sRGB, Display P3
/// (max E 963,232,512 and 1,348,797,448).
const PLACE_FLOOR: [u64; 2] = [1_204_040_640, 1_685_996_810];
/// ⌈1.25 × max E⌉ of the measured `transform_layer` at 140 % (max E
/// 1,321,255,872).
const TRANSFORM_FLOOR: u64 = 1_651_569_840;
/// ⌈1.25 × max E⌉ of the measured typed `save_native` / `open_native` of the
/// streamed writer and reader (`stream_measure` below; max E 49,152
/// and 1,160,069,960; `precision_native_plan`'s `# Streamed reader and
/// writer`). The save case's E sits below the writer's own buffers, so
/// the binding save measurement is `native_file_size::stream_measure`.
const SAVE_FLOOR: u64 = 61_440;
const REOPEN_FLOOR: u64 = 1_450_087_450;
/// ⌈1.25 × max E⌉ of the measured low-depth calls (`float_native`'s
/// record): the places at 16 bits (scene profile, Display P3) and at 8, the
/// 16-bit transform, save and reopen, the 8-bit save and reopen, and the
/// narrowing to 16 bits.
const PLACE16_FLOOR: [u64; 2] = [1_447_588_980, 1_447_629_940];
const PLACE8_FLOOR: u64 = 1_450_230_870;
const TRANSFORM16_FLOOR: u64 = 952_955_570;
/// The 16-bit save and reopen floors, re-measured streamed (max E
/// 376,832 and 388,415,896); the binding save measurement is
/// `native_file_size::stream_measure`, as for `SAVE_FLOOR`.
const SAVE16_FLOOR: u64 = 471_040;
const REOPEN16_FLOOR: u64 = 485_519_870;
const SAVE8_FLOOR: u64 = 481_812_840;
const REOPEN8_FLOOR: u64 = 601_805_220;
const NARROW16_FLOOR: u64 = 719_462_940;
const SCENE: &str = "tests/fixtures/float_11f/scene.tiff";

static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

fn place_request() -> CString {
    CString::new(json!({"op":"place","source_id":"m","name":"Placed","x":0,"y":0}).to_string())
        .unwrap()
}

/// A 6000×4000 f32 document in `profile`: an x ramp, a y ramp, blue steps.
fn float_document(profile: &std::sync::Arc<IccProfile>) -> PrecisionDocument {
    let pixels = (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            [
                x as f32 / W as f32,
                y as f32 / H as f32,
                ((x / 8) % 8) as f32 * 0.25,
                1.0,
            ]
        })
        .collect();
    float_fixture::float_doc(W, H, profile, &[pixels])
}

/// The place plan's figure: the decoded master plus the plan's scratch.
fn place_plan(base: &PrecisionDocument, master: &PrecisionDocument) -> u64 {
    let request = place_request();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_mixed_source_plan(base, ptr::null(), master, request.as_ptr(), &mut err)
    };
    if let Some(message) = owned(err) {
        panic!("place plan: {message}");
    }
    let value: Value = serde_json::from_str(&owned(out).expect("a plan")).unwrap();
    u64::from(W) * u64::from(H) * 16 + value["scratch_bytes"].as_u64().expect("scratch_bytes")
}

fn placed(base: &PrecisionDocument, master: &PrecisionDocument) -> PrecisionDocument {
    let request = place_request();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_mixed_source_edit(
            base,
            ptr::null(),
            master,
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if let Some(message) = owned(err) {
        panic!("place: {message}");
    }
    assert!(!out.is_null(), "the place changes the document");
    *unsafe { Box::from_raw(out) }
}

/// `dr_precision_doc_mixed_source_edit` / `_plan` of `request` with a typed
/// input: the plan's scratch, or the edit's result.
fn mixed(
    doc: &PrecisionDocument,
    s16: &PrecisionDocument,
    request: &Value,
    plan: bool,
) -> Result<(u64, Option<PrecisionDocument>), String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    if plan {
        let out = unsafe {
            dr_precision_doc_mixed_source_plan(doc, ptr::null(), s16, text.as_ptr(), &mut err)
        };
        if let Some(message) = owned(err) {
            return Err(message);
        }
        let value: Value = serde_json::from_str(&owned(out).expect("a plan")).unwrap();
        return Ok((
            value["scratch_bytes"].as_u64().expect("scratch_bytes"),
            None,
        ));
    }
    let out = unsafe {
        dr_precision_doc_mixed_source_edit(
            doc,
            ptr::null(),
            s16,
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if let Some(message) = owned(err) {
        return Err(message);
    }
    assert!(!out.is_null(), "the edit changes the document");
    Ok((0, Some(*unsafe { Box::from_raw(out) })))
}

fn white16() -> PrecisionDocument {
    let pixels = vec![[65535u16; 4]; (W * H) as usize];
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).expect("an image"),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

/// `dr_doc_mixed_source_plan` / `_edit` of `request` over a byte document
/// with a typed input: the plan's scratch, or the edit's result.
fn mixed8(
    doc: &DrDocument,
    s16: &PrecisionDocument,
    request: &Value,
    plan: bool,
) -> (u64, Option<DrDocument>) {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    if plan {
        let out =
            unsafe { dr_doc_mixed_source_plan(doc, ptr::null(), s16, text.as_ptr(), &mut err) };
        if let Some(message) = owned(err) {
            panic!("byte plan: {message}");
        }
        let value: Value = serde_json::from_str(&owned(out).expect("a plan")).unwrap();
        return (
            value["scratch_bytes"].as_u64().expect("scratch_bytes"),
            None,
        );
    }
    let out = unsafe {
        dr_doc_mixed_source_edit(
            doc,
            ptr::null(),
            s16,
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if let Some(message) = owned(err) {
        panic!("byte edit: {message}");
    }
    assert!(!out.is_null(), "the edit changes the document");
    (0, Some(*unsafe { Box::from_raw(out) }))
}

fn admitted(name: &str, figure: u64, floor: u64) {
    println!("{name}: figure {figure} floor {floor}");
    assert!(figure >= floor, "{name}: {figure} < floor {floor}");
}

#[test]
fn a_float_master_place_transform_save_and_reopen_are_admitted_at_24mp() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    assert!(set_process_working_limit(LIMIT));
    let base = float_document(&IccProfile::srgb());
    {
        let p3 = float_document(&IccProfile::display_p3());
        admitted("place32p3", place_plan(&base, &p3), PLACE_FLOOR[1]);
    }
    let master = float_document(&IccProfile::srgb());
    admitted("place32", place_plan(&base, &master), PLACE_FLOOR[0]);
    let doc = placed(&base, &master);
    drop((base, master));
    let (cx, cy) = (f64::from(W) / 2.0, f64::from(H) / 2.0);
    let edit = GeometryEdit::parse(
        &json!({"op":"affine_set","layers":[1],
            "matrix":[SCALE,0.0,0.0,SCALE,cx - SCALE * cx,cy - SCALE * cy],"filter":"bilinear"})
        .to_string(),
    )
    .unwrap();
    let geometry = doc.geometry_plan(&edit).expect("the transform plan");
    admitted("transform32", geometry.scratch_bytes, TRANSFORM_FLOOR);
    let native = doc.native_plan().expect("the native plan");
    admitted("save32", native.write_scratch_bytes, SAVE_FLOOR);
    admitted(
        "reopen32",
        native.reopen_retained_bytes + native.decode_scratch_bytes,
        REOPEN_FLOOR,
    );
    drop(doc);

    // An f32 master in 16- and 8-bit sRGB documents.
    let scene = PrecisionDocument::open_preserving(SCENE, LIMIT, 0, None)
        .expect("the scene")
        .profile()
        .clone();
    let decoded = u64::from(W) * u64::from(H) * 16;
    let place = json!({"op":"place","source_id":"m","name":"Placed","x":0,"y":0,
                       "depth_conversion":"convert_fallback"});
    let white = white16();
    {
        let p3 = float_document(&IccProfile::display_p3());
        let (scratch, _) = mixed(&white, &p3, &place, true).expect("place16p3");
        admitted("place16p3", decoded + scratch, PLACE16_FLOOR[1]);
    }
    let master = float_document(&scene);
    let (scratch, _) = mixed(&white, &master, &place, true).expect("place16");
    admitted("place16", decoded + scratch, PLACE16_FLOOR[0]);
    let doc = mixed(&white, &master, &place, false)
        .expect("the 16-bit place")
        .1
        .expect("a result");
    drop(white);
    let geometry = doc.geometry_plan(&edit).expect("the transform plan");
    admitted("transform16", geometry.scratch_bytes, TRANSFORM16_FLOOR);
    let native = doc.native_plan().expect("the native plan");
    admitted("save16", native.write_scratch_bytes, SAVE16_FLOOR);
    admitted(
        "reopen16",
        native.reopen_retained_bytes + native.decode_scratch_bytes,
        REOPEN16_FLOOR,
    );
    drop(doc);

    let white8 = DrDocument::from_pixels(RgbaImage::from_pixel(W, H, Rgba([255; 4])));
    let (scratch, _) = mixed8(&white8, &master, &place, true);
    admitted("place8", decoded + scratch, PLACE8_FLOOR);
    let doc = mixed8(&white8, &master, &place, false).1.expect("a result");
    drop(white8);
    admitted(
        "save8",
        doc.native_save_scratch_bytes().expect("the save plan"),
        SAVE8_FLOOR,
    );
    admitted(
        "reopen8",
        doc.native_reopen_plan()
            .expect("the reopen plan")
            .decode_bytes,
        REOPEN8_FLOOR,
    );
    drop(doc);

    let doc = placed(&float_document(&IccProfile::srgb()), &master);
    drop(master);
    let plan = doc
        .narrow_structure_plan(NarrowTarget::Rgba16)
        .expect("the narrow plan");
    admitted(
        "narrow16",
        plan.scratch_bytes + plan.output_bytes,
        NARROW16_FLOOR,
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

/// The typed save and reopen floors' shapes (the streamed reader and
/// writer), one case per process, selected by `DR_FM_CASE`: `save32` and
/// `reopen32`, the float document with the sRGB f32 master placed (the
/// first test's `doc`); `save16` and `reopen16`, the white 16-bit document
/// with the scene-profile master placed `convert_fallback`; `base`, the
/// float shape with nothing run. The document is built in setup (for a
/// reopen, built and saved by a child process, `setup_in_child`); then the
/// footprint is read, the interval
/// peak reset and the measured call run with its result held until E =
/// interval peak − pre is read. H is the plan's figure; A what the call
/// admitted: a save's `write_scratch_bytes`, a reopen's reservation read
/// inside `open_native_observed`'s hook. The record is
/// `precision_native_plan`'s `# Streamed reader and writer`.
#[test]
#[ignore = "measurement: DR_FM_CASE=base|save32|reopen32|save16|reopen16, Release, MallocLargeCache=0"]
fn stream_measure() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let case = std::env::var("DR_FM_CASE").expect("DR_FM_CASE");
    assert!(set_process_working_limit(LIMIT));
    let build = || match case.as_str() {
        "base" | "save32" | "reopen32" => {
            let master = float_document(&IccProfile::srgb());
            placed(&float_document(&IccProfile::srgb()), &master)
        }
        "save16" | "reopen16" => {
            let scene = PrecisionDocument::open_preserving(SCENE, LIMIT, 0, None)
                .expect("the scene")
                .profile()
                .clone();
            let place = json!({"op":"place","source_id":"m","name":"Placed","x":0,"y":0,
                               "depth_conversion":"convert_fallback"});
            mixed(&white16(), &float_document(&scene), &place, false)
                .expect("the 16-bit place")
                .1
                .expect("a result")
        }
        other => panic!("unknown DR_FM_CASE {other}"),
    };
    let reopen = case.starts_with("reopen");
    if let Ok(path) = std::env::var("DR_FM_SETUP") {
        // The setup child (`setup_in_child`): save the file and its H.
        let doc = build();
        let plan = doc.native_plan().expect("the plan");
        doc.save_native(&path, LIMIT, 0, None)
            .expect("the setup save");
        let h = plan.reopen_retained_bytes + plan.decode_scratch_bytes;
        std::fs::write(format!("{path}.h"), h.to_string()).unwrap();
        return;
    }
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("measured.rz");
    let path = path.to_str().unwrap();
    let (doc, h, mut a) = if reopen {
        setup_in_child("DR_FM_SETUP", path);
        let h = std::fs::read_to_string(format!("{path}.h"))
            .unwrap()
            .parse::<u64>()
            .unwrap();
        (None, h, 0)
    } else {
        let doc = build();
        let write = doc.native_plan().expect("the plan").write_scratch_bytes;
        (Some(doc), write, write)
    };
    let control = DrJobControl::default();
    let pre = rusage().fields[PHYS_FOOTPRINT];
    unsafe { proc_reset_footprint_interval(std::process::id() as i32) };
    let held = if reopen {
        Some(
            open_native_observed(path, LIMIT, 0, Some(&control), &mut || {
                a = control.scratch_bytes();
            })
            .expect("the reopen is admitted"),
        )
    } else if case.starts_with("save") {
        doc.as_ref()
            .unwrap()
            .save_native(path, LIMIT, 0, None)
            .expect("the save is admitted");
        None
    } else {
        None
    };
    let peak = rusage().fields[INTERVAL_MAX_PHYS_FOOTPRINT];
    drop(held);
    let e = peak.saturating_sub(pre);
    println!(
        "FM_MEASURE case={case} H={h} A={a} pre={pre} peak={peak} E={e} floor={}",
        (e * 5).div_ceil(4)
    );
    assert!(h >= a, "{case}: H {h} < A {a}");
}

/// Runs this binary's `stream_measure` again as a child with `DR_FM_SETUP`
/// set to `path`: the child builds the case's document and saves it there,
/// with its plan figure H beside it, so the measured process has never held
/// the document (a freed document's tiles stay in the allocator's free
/// lists, counted in the footprint and reused by the reopen, which would
/// then read short).
fn setup_in_child(variable: &str, path: &str) {
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "stream_measure", "--nocapture"])
        .env(variable, path)
        .status()
        .unwrap();
    assert!(status.success(), "the setup child");
}
