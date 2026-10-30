//! #547 Duotone's Photoshop take (`scripts/photoshop-probe/duotone_take.py`,
//! phase 2b): its record, re-judged. SKIPS (with an eprintln) when the take
//! folder `duotone-mode-547/take1` has no `decided.json`.
//!
//! Doctored authored the take's two- and three-ink Duotone PSDs; Photoshop
//! opened, converted and saved each, and walked its menus on the two-ink
//! document and on its Grayscale conversion. This re-applies the runner's
//! gate, fixed in its docstring before the smoke (per case, over the 26
//! patch centres: max |Δ| ≤ 3 and every channel's mean |Δ| ≤ 1 in 8-bit RGB
//! codes), to the receipt's per-case numbers; asserts that the decision is
//! the shipped candidate (`SHIPPED`: [A], `duotone_ink`'s declared mix); that
//! the Rust codec reads each Photoshop PSD's inks and overprints within the
//! file's quantization (Lab 0.01, curve points 0.1 %) and equal to the
//! receipt's Python parse (`photoshop.inks`); and that the walk is valid with
//! each fixed row in the state `WALK_ROWS` records, which
//! `app/Sources/DuotoneDocument.swift`'s tables follow.

use std::path::{Path, PathBuf};
use std::ptr;

use serde_json::Value;

const MAX_D: f64 = 3.0;
const MEAN_D: f64 = 1.0;
const LAB_STEP: f64 = 0.01;
const CURVE_STEP: f64 = 0.1;

/// `take1/decided.json`'s decision.
const SHIPPED: &str = "A";

/// `take1/decided.json`'s `walk.rows`: each fixed row and its state.
const WALK_ROWS: &[(&str, &str)] = &[
    ("profile", "DISABLED"),
    ("proof", "DISABLED"),
    ("hdr", "ENABLED"),
    ("bitmap", "FINDING (ENABLED)"),
    ("depth32", "CONFIRMED"),
];

fn take_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let folder = Path::new(&home).join("devel/doctored-fixtures/photoshop/duotone-mode-547/take1");
    if folder.join("decided.json").is_file() {
        Some(folder)
    } else {
        eprintln!("SKIP: {} has no decided.json", folder.display());
        None
    }
}

fn meets(candidate: &Value) -> bool {
    candidate["max_abs"].as_f64().unwrap() <= MAX_D
        && candidate["channel_mean_abs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m.as_f64().unwrap() <= MEAN_D)
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn near(a: &Value, b: &Value, bound: f64, what: &str) {
    let d = a.as_f64().unwrap() - b.as_f64().unwrap();
    assert!(d.abs() <= bound + 1e-9, "{what}: {a} against {b}");
}

/// The import report of the PSD at `psd`, through the C API.
fn import_report(psd: &Path) -> Value {
    let path = crate::common::cpath(psd);
    let mut err = ptr::null_mut();
    let report = unsafe { doctored_core::ffi_psd::dr_psd_import_report(path.as_ptr(), &mut err) };
    assert!(!report.is_null(), "{}", crate::common::take_err_string(err));
    let text = unsafe { std::ffi::CStr::from_ptr(report) }
        .to_str()
        .unwrap()
        .to_string();
    unsafe { doctored_core::ffi::dr_string_free(report) };
    serde_json::from_str(&text).unwrap()
}

/// The Grayscale gate (`duotone_gray`'s module doc), fixed before comparing:
/// every pixel's 8-bit result within this many codes of each Photoshop code
/// recorded for its source code.
const GRAY_GATE: i64 = 1;

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

/// The 8-bit codes of `doc`'s composite (R).
fn codes8(doc: *const doctored_core::precision_document::PrecisionDocument) -> Vec<i64> {
    unsafe { &*doc }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
        .iter()
        .map(|p| ((f64::from(p[0]) / 257.0).round()) as i64)
        .collect()
}

/// One case of the gate: take1's `inputs/gray.png` opened in Dot Gain 20%,
/// the case's inks set, Duotone → Grayscale; the largest |Δ| against any
/// Photoshop code of the pixel's source code, and the pixels judged.
fn gray_case(folder: &Path, key: &str, decided: &Value) -> (i64, usize) {
    use doctored_core::ffi_duotone::{
        dr_precision_doc_convert_duotone_to_gray, dr_precision_doc_set_duotone,
    };
    use doctored_core::ffi_gray::dr_precision_doc_open_gray;
    use doctored_core::ffi_precision::dr_precision_doc_free;
    let inputs = folder.join("inputs");
    let path = crate::common::cpath(&inputs.join("gray.png"));
    let working = std::ffi::CString::new("dot_gain_20").unwrap();
    let mut err = ptr::null_mut();
    let gray = unsafe {
        dr_precision_doc_open_gray(
            path.as_ptr(),
            working.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!gray.is_null(), "{}", crate::common::take_err_string(err));
    let file = if key == "two" {
        "options2.json"
    } else {
        "options3.json"
    };
    let options = std::ffi::CString::new(read_json(&inputs.join(file)).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_set_duotone(gray, options.as_ptr(), &mut err) };
    assert!(!doc.is_null(), "{}", crate::common::take_err_string(err));
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_duotone_to_gray(doc, false, LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!out.is_null(), "{}", crate::common::take_err_string(err));
    let by_code = decided["cases"][key]["photoshop"]["gray_tif_by_code"]
        .as_object()
        .unwrap();
    let (source, result) = (codes8(gray), codes8(out));
    let (mut worst, mut judged) = (0i64, 0usize);
    for (s, o) in source.iter().zip(&result) {
        let Some(theirs) = by_code.get(&s.to_string()) else {
            continue;
        };
        judged += 1;
        for t in theirs.as_array().unwrap() {
            worst = worst.max((o - t.as_i64().unwrap()).abs());
        }
    }
    unsafe {
        dr_precision_doc_free(out);
        dr_precision_doc_free(doc);
        dr_precision_doc_free(gray);
    }
    (worst, judged)
}

#[test]
fn photoshops_grayscale_of_a_duotone_is_the_inks_luminance() {
    let Some(folder) = take_dir() else {
        return;
    };
    let decided = read_json(&folder.join("decided.json"));
    for key in ["two", "three"] {
        let (worst, judged) = gray_case(&folder, key, &decided);
        eprintln!("gray gate {key}: max |Δ| {worst} over {judged} pixels");
        assert!(judged > 0, "{key}: no pixel judged");
        assert!(
            worst <= GRAY_GATE,
            "{key}: max |Δ| {worst} against Photoshop's gray.tif"
        );
    }
}

#[test]
fn the_take_confirms_the_shipped_model_and_the_codec_reads_photoshops_psds() {
    let Some(folder) = take_dir() else {
        return;
    };
    let decided = read_json(&folder.join("decided.json"));
    assert_eq!(decided["verdict"], "VALID", "the take is complete");
    let cases = decided["cases"].as_object().unwrap();
    assert_eq!(cases.len(), 2, "both cases");
    let confirms_a = cases.values().all(|case| meets(&case["candidates"]["A"]));
    let confirms_b = cases.values().all(|case| meets(&case["candidates"]["B"]));
    assert_eq!(
        decided["confirms"]["A"], confirms_a,
        "the gate re-applied to [A]"
    );
    assert_eq!(
        decided["confirms"]["B"], confirms_b,
        "the gate re-applied to [B]"
    );
    assert!(
        confirms_a,
        "[A], the shipped model, confirms under the fixed gate"
    );
    assert_eq!(decided["decision"], SHIPPED);
    let measured = read_json(&folder.join("measured.json"));
    for (key, case) in cases {
        let job = &measured["cases"][key];
        let psd =
            Path::new(job["folder"].as_str().unwrap()).join(job["report"]["psd"].as_str().unwrap());
        let duotone = &import_report(&psd)["duotone"];
        let recorded = &case["photoshop"]["inks"];
        let ours = duotone["inks"].as_array().unwrap();
        let theirs = recorded["inks"].as_array().unwrap();
        assert_eq!(ours.len(), theirs.len(), "{key}: the ink count");
        for (a, b) in ours.iter().zip(theirs) {
            assert_eq!(a["name"], b["name"], "{key}");
            for c in 0..3 {
                near(&a["lab"][c], &b["lab"][c], LAB_STEP, &format!("{key} Lab"));
            }
            let (ca, cb) = (
                a["curve"].as_array().unwrap(),
                b["curve"].as_array().unwrap(),
            );
            assert_eq!(ca.len(), 13);
            for (x, y) in ca.iter().zip(cb) {
                assert_eq!(x.is_null(), y.is_null(), "{key}: blank slots {ca:?} {cb:?}");
                if !x.is_null() {
                    near(x, y, CURVE_STEP, &format!("{key} curve"));
                }
            }
        }
        // The recorded overprints are in `overprint_subsets` order, which the
        // overprint line names ("1+2", "1+3", ...); the report keys them.
        let display = duotone["overprint_display"].as_object().unwrap();
        let stored = recorded["overprints"].as_array().unwrap();
        let subsets = case["overprint_line"]["compact"].as_array().unwrap();
        assert_eq!(display.len(), stored.len(), "{key}: the overprint count");
        assert_eq!(subsets.len(), stored.len(), "{key}: the overprint line");
        for (subset, theirs) in subsets.iter().zip(stored) {
            let Some(lab) = theirs.as_array() else {
                continue;
            };
            let name = subset["subset"].as_str().unwrap();
            let ours = &display[name];
            for c in 0..3 {
                near(
                    &ours[c],
                    &lab[c],
                    LAB_STEP,
                    &format!("{key} overprint {name}"),
                );
            }
        }
    }
    let walk = &decided["walk"];
    assert_eq!(walk["valid"], true, "the walk is valid: {}", walk["checks"]);
    let rows = walk["rows"].as_array().unwrap();
    assert_eq!(rows.len(), WALK_ROWS.len());
    for (row, (name, state)) in rows.iter().zip(WALK_ROWS) {
        assert_eq!(row["name"], *name);
        assert_eq!(row["state"], *state, "{name}");
    }
}
