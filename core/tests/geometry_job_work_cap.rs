//! Typed geometry: a one-shot commit (a filtered turn, a transform
//! commit) under a host job control has no work ceiling and is admitted by
//! memory alone; an uncontrolled run keeps `MAX_DESTRUCTIVE_WORK` (24e9
//! units) in the same words, the job-less plan carries no work refusal, and a
//! preview (`interactive`) keeps `MAX_WORK_UNITS`. The fixture is a 6000×4000
//! 16-bit document of K pixel-sharing duplicate layers turned by Rotate
//! Canvas 17° at Lanczos3, K the smallest whose plan is over 24e9 units (each
//! layer is about 5.86e9: 6908×5580 destination pixels × (6² taps + 2 visits)
//! × 4 channels; at bicubic, about 2.78e9 as in `precision_geometry_budget`'s
//! R17, nine layers would pass the native file cap). The oracles are
//! the published refusal sentence and the plan's own `work_units`; the runs
//! go through `dr_precision_doc_edit` at a 64 GiB limit, so admission never
//! decides (the plan precedes it). A binary of its own with exactly one test:
//! the typed plan's native reopen envelope reads the PROCESS working limit,
//! which refuses any K ≥ 2 of these layers at the 2 GiB reference, so the test
//! raises it to the same 64 GiB.
mod common;
use common::probe::entry_then_cancel;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::ffi_resources::dr_set_process_working_limit;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;

const LIMIT: u64 = 64 << 30;
/// `sampling_gaussian::MAX_DESTRUCTIVE_WORK`.
const DESTRUCTIVE_WORK: u64 = 24_000_000_000;
const REFUSAL: &str = "typed geometry exceeds the bounded pixel-work limit";

unsafe fn text(p: *mut c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    s
}

fn photo16(w: u32, h: u32) -> PrecisionDocument {
    let pixels = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            [
                ((x * 11) % 65536) as u16,
                ((y * 17) % 65536) as u16,
                (((x + y) * 7) % 65536) as u16,
                65535,
            ]
        })
        .collect();
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution { x: 72., y: 72. },
    )
    .unwrap()
}

fn edit(request: &Value) -> GeometryEdit {
    GeometryEdit::parse(&request.to_string()).unwrap()
}

/// The FFI plan at `LIMIT`: the plan JSON's `work_units`, or the error.
fn ffi_plan(doc: &PrecisionDocument, request: &CString) -> Result<u64, String> {
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_precision_doc_edit_plan(doc, request.as_ptr(), LIMIT, 0, &mut err) };
    let error = unsafe { text(err) };
    if out.is_null() {
        return Err(error);
    }
    let plan: Value = serde_json::from_str(&unsafe { text(out) }).expect("plan JSON");
    plan["work_units"]
        .as_u64()
        .ok_or_else(|| format!("the geometry plan carries no work_units: {plan}"))
}

#[test]
fn a_controlled_geometry_commit_runs_past_the_ceiling() {
    assert!(dr_set_process_working_limit(LIMIT));
    let rotate = json!({"op": "rotate_canvas", "degrees": 17, "filter": "lanczos3"});
    // The smallest K over the ceiling, by the plan's own units.
    let mut doc = photo16(6000, 4000);
    let mut layers = 1;
    let units = loop {
        let plan = doc.geometry_plan(&edit(&rotate)).unwrap();
        if plan.work_units > DESTRUCTIVE_WORK {
            break plan.work_units;
        }
        doc = doc.duplicate_layer(0).unwrap().expect("a duplicate");
        layers += 1;
        assert!(layers <= 64, "the plan never passed the ceiling");
    };
    assert!(layers > 1, "one layer is already over the ceiling: {units}");
    println!("K = {layers}, {units} work units");

    // (a) The FFI plan is Ok: it used to be this refusal. Its JSON
    // reports the plan's own work units (the host sizes a reduced
    // preview from them).
    let request = CString::new(rotate.to_string()).unwrap();
    assert_eq!(ffi_plan(&doc, &request), Ok(units));

    let d = &doc;
    let run = |control: *const DrJobControl| {
        let mut err: *mut c_char = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_edit(
                d,
                request.as_ptr(),
                ptr::null(),
                0,
                LIMIT,
                0,
                control,
                &mut err,
            )
        };
        let ok = !out.is_null();
        if ok {
            drop(unsafe { Box::from_raw(out) });
        }
        (ok, unsafe { text(err) })
    };
    // (b) Uncontrolled: the ceiling, in its words.
    let (ok, message) = run(ptr::null());
    assert!(!ok, "the uncontrolled turn was admitted");
    assert_eq!(message, REFUSAL);
    // (c) Controlled: the turn runs and reports progress before the cancel.
    let (at, _) = entry_then_cancel(&|progress, _, _| progress > 0.0, |token| {
        run(token as *const DrJobControl)
    });
    assert!(at.progress > 0.0, "{at:?}");

    // (d) A preview keeps `MAX_WORK_UNITS`. Rotate Canvas has no preview
    // form, so the twin is the same 17° turn as a Straighten (whose
    // `interactive` form is the Crop tool's preview): the commit plans, its
    // `interactive` twin is still refused.
    let (s, c) = 17_f64.to_radians().sin_cos();
    let (cx, cy) = (3000., 2000.);
    let matrix = [c, s, -s, c, cx - cx * c + cy * s, cy - cx * s - cy * c];
    let straighten = json!({"op": "straighten", "matrix": matrix, "filter": "bicubic",
        "x": 300, "y": 200, "width": 5400, "height": 3600});
    let commit = CString::new(straighten.to_string()).unwrap();
    let planned = ffi_plan(&doc, &commit);
    assert!(planned.is_ok(), "straighten commit: {planned:?}");
    let mut preview = straighten;
    preview["interactive"] = json!(true);
    let preview = CString::new(preview.to_string()).unwrap();
    assert_eq!(ffi_plan(&doc, &preview), Err(REFUSAL.to_string()));
}
