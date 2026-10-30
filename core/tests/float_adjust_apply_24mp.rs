//! A destructive Image ▸ Adjustments apply on a 24 MP float
//! layer is admitted under its measured plan. An opaque 6000×4000 float
//! layer in an sRGB document; Levels T1 and Photo Filter through
//! `selected_edit` (with and without a canvas coverage) and Levels through
//! `apply_adjustment`. One 24 MP document at a time: one `#[test]`.
//!
//! Every scratch is at least ⌈1.25 × the largest measured footprint E⌉ in
//! `float_adjust_apply`'s module doc (`# Admission`): one
//! case per process, three runs, `MallocLargeCache=0`, a baseline twin.
//!
//! The twenty-five float filters' plans (`float_filter_apply`)
//! are held the same way against [`measure`]'s footprints, one floor a case
//! ([`FILTER_FLOORS`]; the record is in that module's doc).
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_jobs::{dr_job_control_create, dr_job_control_free};
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_edit;
use doctored_core::icc::IccProfile;
use doctored_core::precision_clipboard::ClipboardPayload;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::selected_adjustment::SelectedEdit;
use serde_json::json;

const W: u32 = 6000;
const H: u32 = 4000;
/// ⌈1.25 × max E⌉ over S Levels, S Photo Filter, C Levels and A Levels, the
/// coverage built before the measured call.
const FLOOR: u64 = 962_499_250;
/// ⌈1.25 × max E⌉ of `measure`'s `levels_c`: the selected
/// Levels with its canvas coverage built INSIDE the measured interval, as the
/// host builds it after the plan. The selected plans, which charge that
/// coverage, are held to this one.
const COVERED_FLOOR: u64 = 1_022_546_640;
/// ⌈1.25 × max E⌉ of `measure`'s `vibrance_c`: the display-scaled
/// Vibrance under `levels_c`'s coverage, its preparation building its own
/// encoder (`adjust_float`). E 818,053,696 / 818,037,312 / 818,053,696 against
/// a plan of 1,030,501,774.
const VIBRANCE_COVERED_FLOOR: u64 = 1_022_567_120;

/// The size of [`small_document`]'s layer.
const SMALL: (u32, u32) = (2000, 1500);

/// The picture's top-left 2000×1500 block on the measured canvas: the block
/// opened as its own document, then the canvas grown to 6000×4000 around it
/// (Canvas Size keeps a layer's size). The layer is an eighth of its canvas,
/// so a selected edit's canvas coverage outweighs its pixels.
fn small_document() -> PrecisionDocument {
    let block = PrecisionDocument::from_clipboard(
        &payload_of(SMALL.0, SMALL.1),
        REFERENCE_WORKING_BYTES,
        0,
        None,
    )
    .unwrap();
    let edit = GeometryEdit::parse(
        &json!({"op": "canvas_size", "width": W, "height": H, "x": 0, "y": 0}).to_string(),
    )
    .unwrap();
    let doc = block
        .geometry(&edit, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .expect("the canvas grows");
    assert!(doc.is_float());
    assert_eq!(doc.dimensions(), (W, H));
    assert_eq!(doc.layer_source(0).unwrap().dimensions(), SMALL);
    doc
}

/// The measured document: RZCB v3 f32, opaque, ramps and a hashed blue.
fn document() -> PrecisionDocument {
    let payload = payload_of(W, H);
    let doc =
        PrecisionDocument::from_clipboard(&payload, REFERENCE_WORKING_BYTES, 0, None).unwrap();
    assert!(doc.is_float());
    doc
}

/// The picture's top-left `w × h` pixels as a float clipboard payload.
fn payload_of(w: u32, h: u32) -> ClipboardPayload {
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
    for y in 0..u64::from(h) {
        for x in 0..u64::from(w) {
            // The full picture's pixel index, whatever part of it is written.
            let i = y * u64::from(W) + x;
            let (x, y) = (x as f32, y as f32);
            let pixel = [
                -0.25 + 4.0 * x / W as f32,
                0.02 + 2.0 * y / H as f32,
                ((i * 2654435761) % 1000) as f32 / 500.0,
                1.0,
            ];
            pixel.iter().for_each(|v| bytes.extend(v.to_le_bytes()));
        }
    }
    ClipboardPayload::decode(&bytes, REFERENCE_WORKING_BYTES, 0).unwrap()
}

/// The measured filter cases: `(case, op, params, a canvas coverage)`.
/// `unsharp_c` runs under a coverage of the canvas's size (half full, half at
/// 32768), the one filter case that pays the plan's `canvas·2`. `blur2` is
/// the float FIR; `blur8`, `blur25` and `blur64` (the 16-bit
/// slider's top) run the extended-box Gaussian Blur. `measure` also takes
/// `levels_c`, the Levels adjustment under the same coverage, and
/// `unsharp_small_c`.
fn filter_cases() -> Vec<(&'static str, &'static str, serde_json::Value, bool)> {
    vec![
        ("blur2", "blur", json!({"sigma": 2}), false),
        ("blur8", "blur", json!({"sigma": 8}), false),
        ("blur25", "blur", json!({"sigma": 25}), false),
        ("blur64", "blur", json!({"sigma": 64}), false),
        ("box", "box_blur", json!({"radius": 1000}), false),
        (
            "motion",
            "motion_blur",
            json!({"angle": 30, "distance": 24}),
            false,
        ),
        ("radial", "radial_blur", json!({"amount": 3}), false),
        (
            "lens_blur",
            "lens_blur",
            json!({"radius": 100, "brightness": 50, "threshold": 200}),
            false,
        ),
        (
            "offset",
            "offset",
            json!({"horizontal": 100, "vertical": 50}),
            false,
        ),
        (
            "lens_correction",
            "lens_correction",
            json!({"distortion": 40, "chromatic_red_cyan": 60, "chromatic_blue_yellow": -40,
                "vignette_amount": -30, "vertical_perspective": -30,
                "horizontal_perspective": 20, "angle": 3, "scale": 105}),
            false,
        ),
        (
            "liquify",
            "liquify",
            json!({"strokes": [{"tool": "forward_warp", "size": 600, "strength": 1,
                "density": 1, "points": [[1000, 2000], [5000, 2000]]}]}),
            false,
        ),
        ("unsharp_c", "unsharp_mask", unsharp_params(), true),
        // The display-encoded and the two linear filters added after the
        // first eleven.
        (
            "surface",
            "surface_blur",
            json!({"radius": 5, "threshold": 15}),
            false,
        ),
        (
            "dust",
            "dust_and_scratches",
            json!({"radius": 4, "threshold": 0.05}),
            false,
        ),
        (
            "reduce_noise",
            "reduce_noise",
            json!({"radius": 2, "luminance": 0.05, "detail": 0.4, "color": 0.6}),
            false,
        ),
        (
            "add_noise",
            "add_noise",
            json!({"amount": 0.2, "seed": 7}),
            false,
        ),
        (
            "add_grain",
            "add_grain",
            json!({"amount": 60, "size": 1.5, "roughness": 30, "seed": 7}),
            false,
        ),
        ("clouds", "clouds", json!({"seed": 5}), false),
        (
            "diff_clouds",
            "difference_clouds",
            json!({"seed": 5}),
            false,
        ),
        ("emboss", "emboss", json!({}), false),
        ("edge", "edge_detect", json!({}), false),
        ("pixelate", "pixelate", json!({"block": 8}), false),
        ("sharpen", "sharpen", json!({"amount": 1.5}), false),
        (
            "presence",
            "presence",
            json!({"texture": 50, "clarity": 50}),
            false,
        ),
        (
            "develop_filter",
            "develop_filter",
            json!({"temperature": 20, "exposure": 0.3, "contrast": 25, "whites": 15,
                "blacks": -10, "texture": 30, "clarity": 20}),
            false,
        ),
        // Dehaze on the encoded copy (rule D), and the Camera
        // Raw Filter with every control, Highlights/Shadows (rule H) and
        // Vibrance/Saturation (rule V) included.
        (
            "presence_dehaze",
            "presence",
            json!({"texture": 50, "clarity": 50, "dehaze": 50}),
            false,
        ),
        (
            "develop_filter_all",
            "develop_filter",
            json!({"temperature": 20, "tint": 10, "exposure": 0.3, "contrast": 25,
                "highlights": -30, "shadows": 40, "whites": 15, "blacks": -10,
                "texture": 30, "clarity": 20, "dehaze": 40, "vibrance": 20,
                "saturation": -10}),
            false,
        ),
        // Smart Sharpen on the encoded copy, every stage on (Reduce
        // Noise, both fade classifiers, the lens disc).
        (
            "smart_sharpen",
            "smart_sharpen",
            json!({"amount": 200, "radius": 4, "noise": 50, "remove": "lens",
                "shadow_fade": 50, "shadow_tonal_width": 50, "shadow_radius": 8,
                "highlight_fade": 50, "highlight_tonal_width": 50, "highlight_radius": 8}),
            false,
        ),
    ]
}

/// ⌈1.25 × max E⌉ of each measured filter case ([`measure`], three sets of
/// three runs; the record is in `float_filter_apply`'s module doc).
const FILTER_FLOORS: [(&str, u64); 28] = [
    ("blur2", 962_519_760),
    ("blur8", 962_171_570),
    ("blur25", 961_270_450),
    ("blur64", 960_533_170),
    ("box", 962_478_770),
    ("motion", 962_499_250),
    ("radial", 962_581_170),
    ("lens_blur", 962_478_770),
    ("offset", 962_499_250),
    ("lens_correction", 962_499_250),
    ("liquify", 960_594_640),
    ("unsharp_c", 1_022_526_190),
    ("surface", 1_440_830_490),
    ("dust", 1_440_830_490),
    ("reduce_noise", 1_440_830_490),
    ("add_noise", 962_540_210),
    ("add_grain", 1_440_830_520),
    ("clouds", 1_440_830_490),
    ("diff_clouds", 1_440_830_490),
    ("emboss", 1_440_830_490),
    ("edge", 1_440_830_490),
    ("pixelate", 962_519_730),
    ("sharpen", 1_440_830_490),
    ("presence", 1_440_871_450),
    ("develop_filter", 1_801_155_880),
    ("presence_dehaze", 1_440_871_450),
    ("develop_filter_all", 1_921_168_740),
    ("smart_sharpen", SMART_SHARPEN_FLOOR),
];
/// ⌈1.25 × max E⌉ of `measure`'s `smart_sharpen`: E 1,152,648,008 /
/// 1,152,664,416 / 1,152,664,416 at `DR_FF_LIMIT` 8 GiB, the baseline twin
/// reading E 0, against a plan of 1,514,238,611.
const SMART_SHARPEN_FLOOR: u64 = 1_440_830_520;
/// ⌈1.25 × max E⌉ of `measure`'s `unsharp_small_c`: `unsharp_c`'s request on
/// [`small_document`]. It is the case that says where the coverage's margin
/// belongs: on the coverage, which is the canvas's, not on the layer's pixels.
const SMALL_COVERED_FLOOR: u64 = 180_183_190;

fn filter_request(op: &str, params: &serde_json::Value) -> String {
    json!({"layer": 0, "op": op, "params": params}).to_string()
}

fn filter_plan(doc: &PrecisionDocument, op: &str, params: &serde_json::Value) -> u64 {
    plan_of(doc, &filter_request(op, params))
}

fn plan_of(doc: &PrecisionDocument, request: &str) -> u64 {
    let edit = SelectedEdit::parse(request).unwrap();
    doc.selected_edit_plan(&edit).unwrap()["scratch_bytes"]
        .as_u64()
        .unwrap()
}

fn description(op: &str) -> String {
    let params = match op {
        "levels" => json!({"black": 20.0 / 255.0, "white": 230.0 / 255.0, "gamma": 1.4,
            "output_black": 10.0 / 255.0, "output_white": 245.0 / 255.0,
            "r": {"white": 220.0 / 255.0}}),
        // A display-scaled form, whose preparation builds its own
        // encoder (`adjust_float`).
        "vibrance" => json!({"vibrance": 0.4}),
        _ => json!({"color": "#ec8a00", "density": 0.25, "preserve_luminosity": true}),
    };
    json!({"type":"adjust","evaluation":"precision_v1","op":op,"params":params}).to_string()
}

fn admitted(name: &str, retained: u64, scratch: u64, floor: u64) {
    println!("{name}: retained {retained} scratch {scratch}");
    assert!(scratch >= floor, "{name}: {scratch} < floor {floor}");
}

#[test]
fn a_float_apply_is_admitted_at_24mp() {
    let doc = document();
    let retained = doc.retained_bytes();
    let canvas = u64::from(W) * u64::from(H);
    for op in ["levels", "photo_filter", "vibrance"] {
        let request = json!({"layer":0,"op":"adjustment","description":description(op)});
        let edit = SelectedEdit::parse(&request.to_string()).unwrap();
        let plan = doc.selected_edit_plan(&edit).unwrap();
        assert_eq!(plan["retained_bytes"], retained);
        assert_eq!(plan["output_bytes"], canvas * 16);
        admitted(
            op,
            retained,
            plan["scratch_bytes"].as_u64().unwrap(),
            if op == "vibrance" {
                VIBRANCE_COVERED_FLOOR
            } else {
                COVERED_FLOOR
            },
        );
    }
    let levels = description("levels");
    admitted(
        "apply levels",
        retained,
        doc.adjustment_scratch_bytes(Some(0), &levels, true)
            .unwrap(),
        FLOOR,
    );
    // The measured C case: half the canvas fully covered, half at 32768.
    let coverage: Vec<u16> = (0..W * H)
        .map(|i| if i % W < W / 2 { u16::MAX } else { 32768 })
        .collect();
    let request = json!({"layer":0,"op":"adjustment","description":levels});
    let edit = SelectedEdit::parse(&request.to_string()).unwrap();
    let out = doc
        .selected_edit(&edit, &coverage, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap();
    assert!(out.is_some(), "every tile changes");
    drop((out, coverage));
    let out = doc
        .apply_adjustment(0, &levels, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap();
    assert!(out.is_some(), "every tile changes");
    drop(out);
    assert_eq!(filter_cases().len(), FILTER_FLOORS.len());
    for ((case, op, params, _), (name, floor)) in filter_cases().iter().zip(FILTER_FLOORS) {
        assert_eq!(*case, name);
        let scratch = filter_plan(&doc, op, params);
        println!("{case}: retained {retained} scratch {scratch}");
        assert!(scratch >= floor, "{case}: {scratch} < floor {floor}");
    }
    drop(doc);
    let small = small_document();
    let scratch = plan_of(&small, &filter_request("unsharp_mask", &unsharp_params()));
    println!("unsharp_small_c: scratch {scratch}");
    assert!(
        scratch >= SMALL_COVERED_FLOOR,
        "unsharp_small_c: {scratch} < floor {SMALL_COVERED_FLOOR}"
    );
}

fn unsharp_params() -> serde_json::Value {
    json!({"radius": 2, "amount": 1.5})
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

/// One case per process, selected by `DR_FF_CASE` (a name of
/// [`filter_cases`], `levels_c`, `vibrance_c` (the display-scaled
/// Vibrance under the same coverage), `unsharp_small_c` on
/// [`small_document`], or
/// `base`: the document built and nothing run). The
/// footprint is read with the document built, the interval peak reset, and
/// the filter run through `dr_precision_doc_selected_edit` under a job
/// control, as the app runs it (a controlled run has no work ceiling), with
/// its result held until E = interval peak − pre is read. A coverage is built
/// inside the interval: the plan's `canvas·2` is the host's borrowed
/// coverage. `plan` is the figure the default test asserts. `DR_FF_LIMIT`
/// (bytes) replaces the 2 GiB working limit for the call.
#[test]
#[ignore = "measurement: DR_FF_CASE=base|blur2|blur8|blur25|blur64|box|motion|radial|lens_blur|offset|lens_correction|liquify|unsharp_c|surface|dust|reduce_noise|add_noise|add_grain|clouds|diff_clouds|emboss|edge|pixelate|sharpen|presence|develop_filter|presence_dehaze|develop_filter_all|smart_sharpen|unsharp_small_c|levels_c|vibrance_c, Release, MallocLargeCache=0"]
fn measure() {
    let case = std::env::var("DR_FF_CASE").expect("DR_FF_CASE");
    let doc = if case == "unsharp_small_c" {
        small_document()
    } else {
        document()
    };
    let cases = filter_cases();
    // (request, a canvas coverage)
    let chosen: Option<(String, bool)> = if case == "levels_c" || case == "vibrance_c" {
        let op = if case == "levels_c" {
            "levels"
        } else {
            "vibrance"
        };
        let request = json!({"layer":0,"op":"adjustment","description":description(op)});
        Some((request.to_string(), true))
    } else if case == "unsharp_small_c" {
        let request = filter_request("unsharp_mask", &unsharp_params());
        Some((request, true))
    } else {
        cases
            .iter()
            .find(|(name, ..)| *name == case)
            .map(|(_, op, params, covered)| (filter_request(op, params), *covered))
    };
    assert!(chosen.is_some() || case == "base", "unknown case {case}");
    let plan = chosen
        .as_ref()
        .map_or(0, |(request, _)| plan_of(&doc, request));
    // `DR_FF_LIMIT` raises the working limit for a case whose plan with the
    // retained document passes the 2 GiB test pin (`develop_filter` with Texture
    // and Clarity); the footprint does not depend on the limit.
    let limit = std::env::var("DR_FF_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(REFERENCE_WORKING_BYTES);
    let pre = rusage().fields[PHYS_FOOTPRINT];
    unsafe { proc_reset_footprint_interval(std::process::id() as i32) };
    let started = std::time::Instant::now();
    let held = chosen.map(|(request, covered)| {
        let coverage: Vec<u16> = if covered {
            (0..W * H)
                .map(|i| if i % W < W / 2 { u16::MAX } else { 32768 })
                .collect()
        } else {
            Vec::new()
        };
        let request = CString::new(request).unwrap();
        let control = unsafe { dr_job_control_create() };
        let mut err: *mut c_char = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_selected_edit(
                &doc,
                request.as_ptr(),
                if coverage.is_empty() {
                    ptr::null()
                } else {
                    coverage.as_ptr()
                },
                coverage.len(),
                limit,
                0,
                control,
                &mut err,
            )
        };
        unsafe { dr_job_control_free(control) };
        if !err.is_null() {
            let message = unsafe { CStr::from_ptr(err) }
                .to_string_lossy()
                .into_owned();
            unsafe { dr_string_free(err) };
            panic!("{case}: {message}");
        }
        assert!(!out.is_null(), "{case}: no change");
        (unsafe { Box::from_raw(out) }, coverage)
    });
    let seconds = started.elapsed().as_secs_f64();
    let peak = rusage().fields[INTERVAL_MAX_PHYS_FOOTPRINT];
    drop(held);
    let e = peak.saturating_sub(pre);
    let floor = (e * 5).div_ceil(4);
    println!(
        "FF_MEASURE case={case} plan={plan} pre={pre} peak={peak} E={e} floor={floor} seconds={seconds:.1}"
    );
    assert!(
        case == "base" || plan >= floor,
        "{case}: plan {plan} < floor {floor}"
    );
}
