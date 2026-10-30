//! #1 Lab phase 2a, painting a Lab Color document: a Brush stroke
//! (`paint_blend`), Fill's mode-less `paint`, the Paint Bucket
//! (`bucket_fill`), a two-colour Gradient and the Eraser (`erase_native`)
//! at 8 and 16 bits. The oracles are independent of the Lab path: a covered
//! pixel at full coverage decodes (`lab_fixture::decode`) to the colour's
//! hand-computed CIE L\*a\*b\* (`lab_fixture::srgb_to_lab`) within 0.1 L\*
//! and 0.1 a\*/b\* at 16 bits, and within 0.6 L\* and 1.0 a\*/b\* at 8 with
//! every code on the ×257 grid; a stroke at opacity 0.5 is the code lerp of
//! the backdrop and the full stroke within 1 code; the Eraser at 0.5 halves
//! alpha within 1 code; a black-to-white Gradient's ends are black's and
//! white's L\*a\*b\* and its L\* never falls. Multiply and (since phase 3)
//! Hue are admitted, Darken refuses and (since phase 2e) a Brush
//! stroke on a colour plane passes the gate to the RGB edit's own rule.
//! Bounds declared before measuring.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_lab::dr_precision_doc_new_lab;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::lab_fixture::{decode, srgb_to_lab};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const W: u32 = 8;
const H: u32 = 2;
const RED: [f64; 4] = [1.0, 0.0, 0.0, 1.0];
const GREEN: [f64; 4] = [0.0, 1.0, 0.0, 1.0];
const BLUE: [f64; 4] = [0.0, 0.0, 1.0, 1.0];
const BLACK: [f64; 4] = [0.0, 0.0, 0.0, 1.0];
const WHITE: [f64; 4] = [1.0, 1.0, 1.0, 1.0];

const DARKEN: &str = "The Darken blend mode is unavailable in a Lab Color document.";

fn free(doc: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(doc) };
}

/// A white Lab document of `depth` (File ▸ New ▸ Lab Color).
fn new_lab(depth: u32) -> *mut PrecisionDocument {
    let request = CString::new(format!(r#"{{"depth":{depth},"fill_srgb":[1,1,1,1]}}"#)).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_new_lab(W, H, request.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

fn plan_samples(doc: *const PrecisionDocument, request: &Value) -> Result<usize, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let plan = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    if plan.is_null() {
        return Err(take_err_string(err));
    }
    let value: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(plan) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(plan) };
    Ok(value["input_samples"].as_u64().unwrap() as usize)
}

/// `request` with every input sample at `cover`.
fn paint(
    doc: *const PrecisionDocument,
    request: &Value,
    cover: u16,
) -> Result<*mut PrecisionDocument, String> {
    let n = plan_samples(doc, request)?;
    let input = vec![cover; n];
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            input.as_ptr(),
            input.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

fn codes(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn brush(color: [f64; 4], opacity: f64, mode: u32) -> Value {
    json!({"op":"paint_blend","layer":0,"color":color,"color_space":"srgb","opacity":opacity,
        "blend_mode":mode})
}

fn fill(color: [f64; 4]) -> Value {
    json!({"op":"paint","layer":0,"color":color,"color_space":"srgb","opacity":1.0})
}

fn bucket(color: [f64; 4], mode: u32) -> Value {
    json!({"op":"bucket_fill","layer":0,"color":color,"color_space":"srgb","opacity":1.0,
        "blend_mode":mode,"x":0,"y":0,"tolerance":0,"contiguous":true})
}

/// Pixel 0 lies before `from` and pixel `W - 1` past `to`, so each takes an
/// end colour whole.
fn ramp(start: [f64; 4], end: [f64; 4], mode: u32) -> Value {
    json!({"op":"gradient","layer":0,"start":start,"end":end,"color_space":"srgb",
        "opacity":1.0,"blend_mode":mode,"from":[1.0,0.0],"to":[f64::from(W) - 1.0,0.0],
        "radial":false})
}

/// A full-coverage sample against the hand-computed L\*a\*b\* of `rgb`.
fn assert_lab(p: [u16; 4], rgb: [f64; 4], depth: u32, what: &str) {
    let want = srgb_to_lab([rgb[0], rgb[1], rgb[2]]);
    let got = decode(p);
    let (l_bound, ab_bound) = if depth == 16 { (0.1, 0.1) } else { (0.6, 1.0) };
    assert!(
        (got[0] - want[0]).abs() <= l_bound,
        "{what}: L* {} against {}",
        got[0],
        want[0]
    );
    for c in 1..3 {
        assert!(
            (got[c] - want[c]).abs() <= ab_bound,
            "{what}: channel {c} {} against {}",
            got[c],
            want[c]
        );
    }
    if depth == 8 {
        for (c, code) in p.iter().enumerate() {
            assert_eq!(
                code % 257,
                0,
                "{what}: channel {c} {p:?} is off the 8-bit grid"
            );
        }
    }
    assert_eq!(p[3], 65535, "{what}: alpha");
}

#[test]
fn every_paint_tool_paints_the_colours_lab_at_full_coverage() {
    for depth in [8, 16] {
        let lab = new_lab(depth);
        for (what, request, colour) in [
            ("brush", brush(RED, 1.0, 0), RED),
            ("fill", fill(GREEN), GREEN),
            ("bucket", bucket(BLUE, 0), BLUE),
        ] {
            let what = format!("{what} at {depth}");
            let out = paint(lab, &request, 65535).unwrap_or_else(|e| panic!("{what}: {e}"));
            for p in codes(out) {
                assert_lab(p, colour, depth, &what);
            }
            assert!(unsafe { &*out }.is_lab(), "{what}: the document stays Lab");
            free(out);
        }
        let what = format!("gradient at {depth}");
        let out = paint(lab, &ramp(BLACK, WHITE, 0), 65535).unwrap_or_else(|e| panic!("{e}"));
        let row: Vec<[u16; 4]> = codes(out)[..W as usize].to_vec();
        assert_lab(row[0], BLACK, depth, &what);
        assert_lab(row[W as usize - 1], WHITE, depth, &what);
        for pair in row.windows(2) {
            assert!(pair[1][0] >= pair[0][0], "{what}: L* falls {pair:?}");
        }
        free(out);
        free(lab);
    }
}

#[test]
fn a_half_opacity_stroke_is_the_code_lerp_and_the_eraser_halves_alpha() {
    for depth in [8, 16] {
        let lab = new_lab(depth);
        let base = codes(lab);
        let full = paint(lab, &brush(RED, 1.0, 0), 65535).unwrap();
        let ink = codes(full);
        let half = paint(lab, &brush(RED, 0.5, 0), 65535).unwrap();
        for (i, p) in codes(half).iter().enumerate() {
            for c in 0..3 {
                let want = (f64::from(base[i][c]) + f64::from(ink[i][c])) / 2.0;
                assert!(
                    (f64::from(p[c]) - want).abs() <= 1.0,
                    "{depth} pixel {i} channel {c}: {} against {want}",
                    p[c]
                );
            }
        }
        let erased = paint(
            lab,
            &json!({"op":"erase_native","layer":0,"opacity":0.5}),
            65535,
        )
        .unwrap();
        for (i, p) in codes(erased).iter().enumerate() {
            assert!(
                (f64::from(p[3]) - 65535.0 / 2.0).abs() <= 1.0,
                "{depth} pixel {i} alpha {}",
                p[3]
            );
        }
        for doc in [erased, half, full, lab] {
            free(doc);
        }
    }
}

#[test]
fn multiply_and_hue_are_admitted_darken_refuses_and_a_plane_is_admitted() {
    let lab = new_lab(16);
    for request in [brush(RED, 1.0, 1), bucket(RED, 1), ramp(RED, BLUE, 1)] {
        let out = paint(lab, &request, 65535).unwrap_or_else(|e| panic!("{request}: {e}"));
        free(out);
    }
    // Hue over the new document's neutral white changes nothing (its arm keeps
    // the backdrop's zero chroma), so admission is the plan's answer.
    for request in [brush(RED, 1.0, 23), bucket(RED, 23), ramp(RED, BLUE, 23)] {
        assert!(plan_samples(lab, &request).is_ok(), "{request}");
    }
    for request in [brush(RED, 1.0, 6), bucket(RED, 6), ramp(RED, BLUE, 6)] {
        assert_eq!(
            plan_samples(lab, &request).unwrap_err(),
            DARKEN,
            "{request}"
        );
    }
    // Phase 2e: the gate admits a colour plane (here a*); the stroke reaches
    // the edit's own rule, as on a 16-bit RGB document (a plane is painted
    // through scalar_edit, `lab_targets.rs`).
    let mut request = brush(RED, 1.0, 0);
    request["plane"] = json!("green");
    assert_eq!(
        plan_samples(lab, &request).unwrap_err(),
        "paint_blend's plane is a 32-bit float document's target; a 16-bit colour plane is a scalar_edit target",
        "{request}"
    );
    free(lab);
}
