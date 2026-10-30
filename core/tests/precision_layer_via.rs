//! Layer Via Copy / Layer Via Cut at 16 bits, through
//! `dr_precision_doc_edit` / `dr_precision_doc_edit_plan`'s `layer_via`
//! request.
//!
//! The oracle is written out here in integers: a copied pixel keeps its RGB
//! and takes alpha `round(a · c / 65535)`; a cut leaves the source alpha
//! `round(a · (65535 − c) / 65535)` and its RGB untouched. Structure is read
//! back from the records themselves, refusals by their sentence.
//!
//! The admission record (`admission_measure`, ignored;
//! `plans_cover_the_measured_peaks`, default) is the module doc of
//! `precision_layer_via.rs`.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::layer_insert::InsertPosition;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_capability::capability;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// The COPY is exact: integer arithmetic on both sides.
const COPY_TOLERANCE: i64 = 0;
/// The CUT's erase runs in f32 (`blend::erase_alpha`, one rounding to u16)
/// while the oracle is the exact integer quotient, so a value on a rounding
/// boundary may land one code either side.
const ERASE_TOLERANCE: i64 = 1;
/// `DrLockFlags`, restated rather than read out of the core.
const LOCK_TRANSPARENCY: u32 = 1;
const LOCK_PIXELS: u32 = 2;

// ------------------------------------------------------------- the oracle --

/// `round(alpha · coverage / 65535)` in integers.
fn covered(alpha: u16, coverage: u16) -> u16 {
    ((u64::from(alpha) * u64::from(coverage) * 2 + 65535) / (2 * 65535)) as u16
}

/// `round(alpha · (65535 − coverage) / 65535)` in integers.
fn erased(alpha: u16, coverage: u16) -> u16 {
    covered(alpha, 65535 - coverage)
}

// ------------------------------------------------------------- plumbing --

fn take_error(error: *mut c_char) -> Option<String> {
    if error.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    Some(text)
}

fn edit_at(
    document: &PrecisionDocument,
    request: &Value,
    samples: &[u16],
    limit: u64,
) -> Result<Option<Box<PrecisionDocument>>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_edit(
            document,
            text.as_ptr(),
            if samples.is_empty() {
                ptr::null()
            } else {
                samples.as_ptr()
            },
            samples.len(),
            limit,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if let Some(error) = take_error(error) {
        assert!(result.is_null(), "a refusal leaves no document");
        return Err(error);
    }
    Ok((!result.is_null()).then(|| unsafe { Box::from_raw(result) }))
}

fn edit(
    document: &PrecisionDocument,
    request: &Value,
    samples: &[u16],
) -> Result<Option<Box<PrecisionDocument>>, String> {
    edit_at(document, request, samples, LIMIT)
}

fn must(document: &PrecisionDocument, request: &Value, samples: &[u16]) -> Box<PrecisionDocument> {
    match edit(document, request, samples) {
        Ok(Some(out)) => out,
        Ok(None) => panic!("{request} reported no change"),
        Err(message) => panic!("{request}: {message}"),
    }
}

fn refusal(document: &PrecisionDocument, request: &Value, samples: &[u16]) -> String {
    match edit(document, request, samples) {
        Err(message) => message,
        Ok(_) => panic!("{request} must be refused"),
    }
}

fn plan_at(document: &PrecisionDocument, request: &Value, limit: u64) -> Result<Value, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe { dr_precision_doc_edit_plan(document, text.as_ptr(), limit, 0, &mut error) };
    if let Some(error) = take_error(error) {
        return Err(error);
    }
    assert!(!out.is_null(), "a plan with no error");
    let value = unsafe { CStr::from_ptr(out) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(out) };
    Ok(serde_json::from_str(&value).expect("plan JSON"))
}

fn via(layer: usize, cut: bool, selection: bool) -> Value {
    json!({"op": "layer_via", "layer": layer, "cut": cut, "name": "Via", "selection": selection})
}

fn samples(document: &PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    document
        .layer_source(index)
        .expect("a raster record")
        .to_rgba16()
        .expect("u16 storage")
}

fn names(document: &PrecisionDocument) -> Vec<String> {
    (0..document.layer_count())
        .map(|i| document.layer(i).unwrap().name().to_string())
        .collect()
}

// ------------------------------------------------------------ the fixture --

const W: u32 = 6;
const H: u32 = 4;

/// A varied straight RGBA16 pixel per index, alpha included, with a few
/// fully transparent and fully opaque samples.
fn pixel(i: u32) -> [u16; 4] {
    let alpha = match i % 5 {
        0 => 65535,
        1 => 0,
        2 => 40000,
        3 => 1,
        _ => 12345,
    };
    [
        (i * 2711 + 300) as u16,
        (i * 911 + 7000) as u16,
        65535u32.wrapping_sub(i * 1777) as u16,
        alpha,
    ]
}

fn image(w: u32, h: u32, seed: u32) -> PrecisionImage {
    let px: Vec<[u16; 4]> = (0..w * h).map(|i| pixel(i + seed)).collect();
    PrecisionImage::from_rgba16(w, h, px, IccProfile::srgb(), None).unwrap()
}

fn document(canvas: (u32, u32), layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        canvas.0,
        canvas.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

/// `[Base, Source]`: the source canvas-sized, at half opacity, with a mask,
/// so the copy's "opacity 1, no mask" is a visible claim.
fn fixture() -> PrecisionDocument {
    let mask = PrecisionMask::new(
        PrecisionPlane::from_u16(W, H, vec![30000; (W * H) as usize], None).unwrap(),
        Default::default(),
        true,
    )
    .unwrap();
    document(
        (W, H),
        vec![
            Layer::raster("Base", image(W, H, 100), (0, 0), 1.0, true).unwrap(),
            Layer::raster("Source", image(W, H, 0), (0, 0), 0.5, true)
                .unwrap()
                .with_mask(Some(mask)),
        ],
    )
}

/// A canvas coverage plane with zeros, full codes and fractions.
fn coverage() -> Vec<u16> {
    (0..W * H)
        .map(|i| match i % 4 {
            0 => 65535,
            1 => 0,
            2 => 32768,
            _ => (i * 2500) as u16,
        })
        .collect()
}

fn assert_alpha(got: u16, want: u16, tolerance: i64, what: &str) {
    assert!(
        (i64::from(got) - i64::from(want)).abs() <= tolerance,
        "{what}: alpha {got} against {want} (tolerance {tolerance})"
    );
}

// --------------------------------------------------------------- the cases --

#[test]
fn copy_weights_alpha_by_the_selection_and_keeps_rgb() {
    let doc = fixture();
    let plane = coverage();
    let out = must(&doc, &via(1, false, true), &plane);
    assert_eq!(names(&out), ["Base", "Source", "Via"]);
    let source = samples(&doc, 1);
    assert_eq!(samples(&out, 1), source, "a copy leaves the source alone");
    let copy = samples(&out, 2);
    for (i, (got, src)) in copy.iter().zip(&source).enumerate() {
        assert_eq!(got[..3], src[..3], "pixel {i}: RGB is copied exactly");
        assert_alpha(
            got[3],
            covered(src[3], plane[i]),
            COPY_TOLERANCE,
            &format!("pixel {i}"),
        );
    }
    let record = out.layer(2).unwrap();
    assert_eq!(record.offset(), (0, 0));
    assert_eq!(record.source().unwrap().dimensions(), (W, H));
    assert!(record.mask().is_none(), "the copy carries no mask");
    assert_eq!(record.opacity(), 1.0);
    assert!(record.visible() && !record.clipped());
    assert!(record.style().is_none() && record.reported_style().is_none());
    assert_ne!(out.layer_id(2), out.layer_id(1), "a newly minted id");
}

#[test]
fn cut_erases_the_source_under_the_same_coverage() {
    let doc = fixture();
    let plane = coverage();
    let out = must(&doc, &via(1, true, true), &plane);
    assert_eq!(names(&out), ["Base", "Source", "Via"]);
    let source = samples(&doc, 1);
    let after = samples(&out, 1);
    let copy = samples(&out, 2);
    for i in 0..source.len() {
        assert_eq!(copy[i][..3], source[i][..3], "pixel {i}: copied RGB");
        assert_alpha(
            copy[i][3],
            covered(source[i][3], plane[i]),
            COPY_TOLERANCE,
            &format!("copy pixel {i}"),
        );
        assert_eq!(
            after[i][..3],
            source[i][..3],
            "pixel {i}: the erase keeps RGB"
        );
        assert_alpha(
            after[i][3],
            erased(source[i][3], plane[i]),
            ERASE_TOLERANCE,
            &format!("source pixel {i}"),
        );
    }
    // The source keeps its own mask and opacity.
    assert!(out.layer(1).unwrap().mask().is_some());
    assert_eq!(out.layer(1).unwrap().opacity(), 0.5);
}

/// A 160×3 layer at (−100, 0) on a 160×3 canvas: its left 100 columns hang
/// off the canvas and the canvas's right 100 columns are not covered by it.
fn hanging() -> PrecisionDocument {
    document(
        (160, 3),
        vec![Layer::raster("Source", image(160, 3, 7), (-100, 0), 1.0, true).unwrap()],
    )
}

#[test]
fn a_whole_layer_copy_keeps_the_off_canvas_columns() {
    let doc = hanging();
    let out = must(&doc, &via(0, false, false), &[]);
    assert_eq!(names(&out), ["Source", "Via"]);
    assert_eq!(out.layer(1).unwrap().offset(), (-100, 0));
    assert_eq!(
        samples(&out, 1),
        samples(&doc, 0),
        "every pixel, on or off the canvas"
    );
    assert_eq!(samples(&out, 0), samples(&doc, 0));
}

#[test]
fn a_full_selection_zeroes_the_off_canvas_columns() {
    let doc = hanging();
    let plane = vec![65535u16; 160 * 3];
    let out = must(&doc, &via(0, false, true), &plane);
    let source = samples(&doc, 0);
    let copy = samples(&out, 1);
    assert_eq!(out.layer(1).unwrap().offset(), (-100, 0));
    for (i, (got, src)) in copy.iter().zip(&source).enumerate() {
        let lx = i % 160;
        assert_eq!(got[..3], src[..3], "pixel {i}: RGB");
        let want = if lx < 100 { 0 } else { src[3] };
        assert_alpha(got[3], want, COPY_TOLERANCE, &format!("column {lx}"));
    }
}

#[test]
fn a_whole_layer_cut_empties_the_canvas_part_only() {
    let doc = hanging();
    let out = must(&doc, &via(0, true, false), &[]);
    let source = samples(&doc, 0);
    assert_eq!(samples(&out, 1), source, "the copy holds every pixel");
    let after = samples(&out, 0);
    for (i, (got, src)) in after.iter().zip(&source).enumerate() {
        let lx = i % 160;
        assert_eq!(got[..3], src[..3], "pixel {i}: the erase keeps RGB");
        let want = if lx < 100 { src[3] } else { 0 };
        assert_alpha(got[3], want, COPY_TOLERANCE, &format!("column {lx}"));
    }
}

#[test]
fn the_copy_lands_above_its_source_at_the_source_level_unclipped() {
    let inner = Layer::raster("Inner", image(W, H, 3), (0, 0), 1.0, true).unwrap();
    let clipped = Layer::raster("Clipped", image(W, H, 9), (0, 0), 1.0, true)
        .unwrap()
        .with_clipped(true);
    let doc = document(
        (W, H),
        vec![
            Layer::raster("Base", image(W, H, 100), (0, 0), 1.0, true).unwrap(),
            Layer::isolated_normal("Group", vec![inner, clipped], 1.0, true).unwrap(),
            Layer::raster("Top", image(W, H, 50), (0, 0), 1.0, true).unwrap(),
        ],
    );
    // Postorder: Base 0, Inner 1, Clipped 2, Group 3, Top 4.
    assert_eq!(names(&doc), ["Base", "Inner", "Clipped", "Group", "Top"]);
    let out = must(&doc, &via(1, false, false), &[]);
    assert_eq!(
        names(&out),
        ["Base", "Inner", "Via", "Clipped", "Group", "Top"]
    );
    let group = &out.root_layers()[1];
    let children: Vec<&str> = group.children().unwrap().iter().map(|l| l.name()).collect();
    assert_eq!(
        children,
        ["Inner", "Via", "Clipped"],
        "inside the source's group"
    );
    assert!(
        !out.layer(2).unwrap().clipped(),
        "unclipped beside a clipped sibling"
    );
}

#[test]
fn refusals_name_what_is_wrong() {
    let doc = fixture();
    let plane = coverage();
    assert_eq!(
        refusal(&doc, &via(9, false, true), &plane),
        "high-depth layer via index is out of range"
    );
    let grouped = document(
        (W, H),
        vec![Layer::isolated_normal(
            "Group",
            vec![Layer::raster("Inner", image(W, H, 3), (0, 0), 1.0, true).unwrap()],
            1.0,
            true,
        )
        .unwrap()],
    );
    assert_eq!(
        refusal(&grouped, &via(1, false, false), &[]),
        "Layer Via requires a raster layer; layer 1 is a group"
    );
    let description =
        json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}).to_string();
    let adjusted = doc
        .add_adjustment_at(
            "Invert",
            &description,
            1.0,
            1,
            InsertPosition::Above,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        refusal(&adjusted, &via(2, false, false), &[]),
        "Layer Via requires a raster layer; layer 2 is an adjustment layer"
    );
    // Every key is required and no other is accepted.
    for request in [
        json!({"op": "layer_via", "layer": 1, "cut": false, "name": "Via"}),
        json!({"op": "layer_via", "layer": 1, "cut": false, "selection": false}),
        json!({"op": "layer_via", "layer": 1, "name": "Via", "selection": false}),
        json!({"op": "layer_via", "cut": false, "name": "Via", "selection": false}),
        json!({"op": "layer_via", "layer": 1, "cut": false, "name": "Via", "selection": false,
               "extra": 1}),
    ] {
        let message = refusal(&doc, &request, &[]);
        assert!(
            message.starts_with("invalid high-depth layer via request"),
            "{request}: {message}"
        );
    }
    // A selection request with no plane, and a whole-layer one with a plane.
    let mismatch = "high-depth input sample length does not match the core plan";
    assert_eq!(refusal(&doc, &via(1, false, true), &[]), mismatch);
    assert_eq!(refusal(&doc, &via(1, false, false), &plane), mismatch);
}

#[test]
fn nothing_copied_and_nothing_erased_answer_no_change() {
    let doc = fixture();
    let none = vec![0u16; (W * H) as usize];
    assert!(edit(&doc, &via(1, false, true), &none).unwrap().is_none());
    assert!(edit(&doc, &via(1, true, true), &none).unwrap().is_none());
    // A fully transparent layer has nothing to copy, whole or selected.
    let clear = PrecisionImage::from_rgba16(
        W,
        H,
        vec![[9000, 8000, 7000, 0]; (W * H) as usize],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let empty = document(
        (W, H),
        vec![Layer::raster("Clear", clear, (0, 0), 1.0, true).unwrap()],
    );
    assert!(edit(&empty, &via(0, false, false), &[]).unwrap().is_none());
    assert!(edit(&empty, &via(0, true, false), &[]).unwrap().is_none());
}

#[test]
fn locks_gate_the_cut_and_not_the_copy() {
    let doc = fixture();
    let plane = coverage();
    let locked = doc.with_layer_locks(1, LOCK_PIXELS).unwrap().unwrap();
    // The copy adds a record and writes none.
    let copied = must(&locked, &via(1, false, true), &plane);
    assert_eq!(names(&copied), ["Base", "Source", "Via"]);
    let message = refusal(&locked, &via(1, true, true), &plane);
    assert!(message.contains("lock"), "{message}");
    // A transparency lock makes the erase change nothing, so the cut is the
    // exact no-op the byte op answers too.
    let frozen = doc.with_layer_locks(1, LOCK_TRANSPARENCY).unwrap().unwrap();
    assert!(edit(&frozen, &via(1, true, true), &plane)
        .unwrap()
        .is_none());
    assert!(edit(&frozen, &via(1, false, true), &plane)
        .unwrap()
        .is_some());
}

#[test]
fn the_plan_names_its_input_and_the_family_covers_16_bits_and_float() {
    let doc = fixture();
    let selected = plan_at(&doc, &via(1, false, true), LIMIT).unwrap();
    assert_eq!(selected["input_samples"], json!(W * H));
    assert_eq!(selected["input_width"], json!(W));
    assert_eq!(selected["input_height"], json!(H));
    assert_eq!(selected["input_planes"], json!(1));
    let whole = plan_at(&doc, &via(1, false, false), LIMIT).unwrap();
    assert_eq!(whole["input_samples"], json!(0));
    // A cut plans more than a copy, and a whole-layer cut its full plane too.
    let scratch = |cut: bool, selection: bool| {
        plan_at(&doc, &via(1, cut, selection), LIMIT).unwrap()["scratch_bytes"]
            .as_u64()
            .unwrap()
    };
    assert!(scratch(true, true) > scratch(false, true));
    assert_eq!(
        scratch(true, false),
        scratch(true, true) + u64::from(W * H) * 2,
        "the core-built coverage plane"
    );
    // The plan is the threshold: one byte under it refuses, plan and edit.
    let retained = selected["retained_bytes"].as_u64().unwrap();
    let total = retained + selected["scratch_bytes"].as_u64().unwrap();
    assert!(plan_at(&doc, &via(1, false, true), total).is_ok());
    assert!(plan_at(&doc, &via(1, false, true), total - 1).is_err());
    assert!(edit_at(&doc, &via(1, false, true), &coverage(), total - 1).is_err());
    assert!(edit_at(&doc, &via(1, false, true), &coverage(), total)
        .unwrap()
        .is_some());
    assert_eq!(capability("layer_via", "raster", 16)["supported"], true);
    assert_eq!(capability("layer_via", "group", 16)["supported"], false);
    assert_eq!(capability("layer_via", "raster", 32)["supported"], true);
}

// ---------------------------------------------------- measured admission --

const MW: u32 = 4000;
const MH: u32 = 3000;
/// The selection cases' coverage rect, at 65535.
const MRECT: (u32, u32, u32, u32) = (200, 150, 3600, 2700);
/// The measurement runs at an 8 GiB working limit.
const MLIMIT: u64 = 8_589_934_592;

fn measure_fixture() -> PrecisionDocument {
    let px: Vec<[u16; 4]> = (0..MW * MH)
        .map(|i| {
            let (x, y) = (i % MW, i / MW);
            [
                (x * 13) as u16,
                (y * 17) as u16,
                ((x ^ y) * 7) as u16,
                65535,
            ]
        })
        .collect();
    let image = PrecisionImage::from_rgba16(MW, MH, px, IccProfile::srgb(), None).unwrap();
    document(
        (MW, MH),
        vec![Layer::raster("Base", image, (0, 0), 1.0, true).unwrap()],
    )
}

fn measure_coverage() -> Vec<u16> {
    let (rx, ry, rw, rh) = MRECT;
    let mut plane = vec![0u16; (MW * MH) as usize];
    for y in ry..ry + rh {
        let row = (y * MW) as usize;
        plane[row + rx as usize..row + (rx + rw) as usize].fill(65535);
    }
    plane
}

/// `(cut, selection)` of a measured case.
fn measure_case(case: &str) -> (bool, bool) {
    match case {
        "copy" => (false, true),
        "cut" => (true, true),
        "copy_whole" => (false, false),
        "cut_whole" => (true, false),
        other => panic!("unknown DR_LV_CASE {other}"),
    }
}

/// Max E per case, bytes, from `admission_measure` (the module doc of
/// `precision_layer_via.rs` holds the full record).
const MEASURED_MAX_E: [(&str, u64); 4] = [
    ("copy", 195_002_512),
    ("cut", 287_588_568),
    ("copy_whole", 192_594_064),
    ("cut_whole", 312_721_648),
];

#[test]
fn plans_cover_the_measured_peaks() {
    let doc = measure_fixture();
    for (case, max_e) in MEASURED_MAX_E {
        let (cut, selection) = measure_case(case);
        let scratch = plan_at(&doc, &via(0, cut, selection), MLIMIT).unwrap()["scratch_bytes"]
            .as_u64()
            .unwrap();
        let floor = (max_e * 5).div_ceil(4);
        assert!(
            scratch >= floor,
            "{case}: plan {scratch} under ⌈1.25 × {max_e}⌉ = {floor}"
        );
    }
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

/// One case per process, selected by `DR_LV_CASE` (`baseline`, `copy`,
/// `cut`, `copy_whole`, `cut_whole`): build the fixture (and the coverage
/// plane, which is the host's), read the footprint, reset the interval peak,
/// run the edit through the FFI with the plan's own admission, and print
/// E = interval peak − pre-edit footprint beside the plan's scratch, with the
/// result held until E is read. `baseline` builds the same fixture and
/// coverage and runs nothing: the twin's E is the harness's own noise.
#[test]
#[ignore = "measurement: DR_LV_CASE=baseline|copy|cut|copy_whole|cut_whole, Release, MallocLargeCache=0"]
fn admission_measure() {
    let case = std::env::var("DR_LV_CASE").expect("DR_LV_CASE");
    let doc = measure_fixture();
    let plane = measure_coverage();
    let (cut, selection) = if case == "baseline" {
        (false, true)
    } else {
        measure_case(&case)
    };
    let request = via(0, cut, selection);
    let scratch = plan_at(&doc, &request, MLIMIT).unwrap()["scratch_bytes"]
        .as_u64()
        .unwrap();
    let input: &[u16] = if selection { &plane } else { &[] };
    let pre = rusage().fields[PHYS_FOOTPRINT];
    unsafe { proc_reset_footprint_interval(std::process::id() as i32) };
    let out = if case == "baseline" {
        None
    } else {
        Some(
            edit_at(&doc, &request, input, MLIMIT)
                .expect("the edit is admitted")
                .expect("the edit changes the document"),
        )
    };
    let peak = rusage().fields[INTERVAL_MAX_PHYS_FOOTPRINT];
    assert_eq!(out.is_none(), case == "baseline");
    drop(out);
    println!(
        "LV_MEASURE case={case} scratch={scratch} pre={pre} peak={peak} E={}",
        peak.saturating_sub(pre)
    );
}
