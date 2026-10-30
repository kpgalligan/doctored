//! Apply Layer Mask and Layer Via Copy / Cut in a 32-bit float document,
//! through `dr_precision_doc_edit` / `dr_precision_doc_edit_plan`'s
//! `apply_mask` and `layer_via` requests.
//!
//! The oracles are written out here in f64 and never read off the core: an
//! applied mask leaves alpha `a · m / 65535`; a copied pixel takes alpha
//! `a · c / 65535`; a cut leaves the source alpha `a · (1 − c / 65535)`. Every
//! RGB component is compared BIT FOR BIT, samples above white and below zero
//! included, and full and zero coverage are exact. Structure is read back
//! from the records, refusals by their sentence.
//!
//! The admission records (`admission_measure`, ignored;
//! `plans_cover_the_measured_peaks`, default) are the module docs of
//! `precision_mask_apply.rs` and `precision_layer_via.rs`.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_artwork::{
    dr_precision_doc_artwork_description, dr_precision_doc_artwork_metadata,
};
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::float_document::float_operators_json;
use doctored_core::icc::IccProfile;
use doctored_core::precision_capability::capability;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::{json, Value};

use crate::float_fixture::{float_doc, float_doc_at};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// One f32 product against the f64 oracle: a few ulps of a value in [0, 1].
const ALPHA_TOLERANCE: f64 = 1e-6;
/// `DrLockFlags`, restated rather than read out of the core.
const LOCK_TRANSPARENCY: u32 = 1;
const LOCK_PIXELS: u32 = 2;

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

fn apply_mask(layer: usize) -> Value {
    json!({"op": "apply_mask", "layer": layer})
}

fn floats(document: &PrecisionDocument, index: usize) -> Vec<[f32; 4]> {
    document
        .layer_source(index)
        .expect("a raster record")
        .to_rgba32()
        .expect("float samples")
}

fn names(document: &PrecisionDocument) -> Vec<String> {
    (0..document.layer_count())
        .map(|i| document.layer(i).unwrap().name().to_string())
        .collect()
}

fn rgb_bits(pixel: [f32; 4]) -> [u32; 3] {
    [pixel[0].to_bits(), pixel[1].to_bits(), pixel[2].to_bits()]
}

fn assert_alpha(got: f32, want: f64, what: &str) {
    assert!(
        (f64::from(got) - want).abs() <= ALPHA_TOLERANCE,
        "{what}: alpha {got} against {want}"
    );
}

// ------------------------------------------------------------ the fixture --

const W: u32 = 6;
const H: u32 = 4;

/// A varied straight float pixel per index: RGB above white and below zero,
/// alpha opaque, transparent and fractional.
fn pixel(i: u32) -> [f32; 4] {
    let alpha = match i % 5 {
        0 => 1.0,
        1 => 0.0,
        2 => 0.625,
        3 => 1.0 / 65535.0,
        _ => 0.2,
    };
    [
        -0.5 + 0.375 * i as f32,
        8.0 - 0.7 * i as f32,
        0.01 * i as f32 - 0.1,
        alpha,
    ]
}

fn pixels(w: u32, h: u32, seed: u32) -> Vec<[f32; 4]> {
    (0..w * h).map(|i| pixel(i + seed)).collect()
}

/// One canvas-sized float record, "Background", at index 0.
fn fixture() -> PrecisionDocument {
    float_doc(W, H, &IccProfile::srgb(), &[pixels(W, H, 0)])
}

/// A canvas plane with zeros, full codes and fractions.
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

/// `document` with `plane` as record `layer`'s canvas-sized mask.
fn masked(
    document: &PrecisionDocument,
    layer: usize,
    plane: &[u16],
    enabled: bool,
) -> Box<PrecisionDocument> {
    let (w, h) = document.dimensions();
    must(
        document,
        &json!({"op": "set_mask", "layer": layer, "width": w, "height": h, "enabled": enabled}),
        plane,
    )
}

/// An 8×3 record on a 3×3 canvas at (−5, 0): its left 5 columns hang off the
/// canvas (a float crop keeps every allocation).
fn hanging() -> Box<PrecisionDocument> {
    let wide = float_doc(8, 3, &IccProfile::srgb(), &[pixels(8, 3, 7)]);
    let out = must(
        &wide,
        &json!({"op": "crop", "x": 5, "y": 0, "width": 3, "height": 3}),
        &[],
    );
    assert_eq!(out.dimensions(), (3, 3));
    assert_eq!(out.layer(0).unwrap().offset(), (-5, 0));
    assert_eq!(out.layer_source(0).unwrap().dimensions(), (8, 3));
    out
}

// ------------------------------------------------------------- apply mask --

#[test]
fn applying_a_mask_multiplies_alpha_and_keeps_rgb_bits() {
    let doc = fixture();
    let plane = coverage();
    let source = floats(&doc, 0);
    for enabled in [true, false] {
        // A disabled mask still applies: Apply is the explicit bake.
        let with_mask = masked(&doc, 0, &plane, enabled);
        assert!(with_mask.layer(0).unwrap().mask().is_some());
        let out = must(&with_mask, &apply_mask(0), &[]);
        let record = out.layer(0).unwrap();
        assert!(record.mask().is_none(), "the mask is consumed");
        assert_eq!(record.offset(), (0, 0));
        assert!(out.is_float());
        let after = floats(&out, 0);
        assert_eq!(after.len(), source.len());
        for (i, (got, src)) in after.iter().zip(&source).enumerate() {
            assert_eq!(rgb_bits(*got), rgb_bits(*src), "pixel {i}: RGB bits");
            let want = f64::from(src[3]) * f64::from(plane[i]) / 65535.0;
            assert_alpha(got[3], want, &format!("pixel {i} (enabled {enabled})"));
            match plane[i] {
                65535 => assert_eq!(got[3].to_bits(), src[3].to_bits(), "pixel {i}: full"),
                0 => assert_eq!(got[3], 0.0, "pixel {i}: none"),
                _ => {}
            }
        }
    }
}

#[test]
fn applying_a_mask_refuses_a_transparency_lock_a_group_and_a_missing_mask() {
    let doc = fixture();
    let plane = coverage();
    let with_mask = masked(&doc, 0, &plane, true);
    let frozen = with_mask
        .with_layer_locks(0, LOCK_TRANSPARENCY)
        .unwrap()
        .unwrap();
    let message = refusal(&frozen, &apply_mask(0), &[]);
    assert!(message.contains("lock"), "{message}");

    assert_eq!(
        refusal(&doc, &apply_mask(0), &[]),
        "native layer has no mask to apply"
    );

    let grouped = must(
        &doc,
        &json!({"op": "group_layers", "layers": [0], "name": "G", "pass_through": false}),
        &[],
    );
    assert_eq!(names(&grouped), ["Background", "G"]);
    let group_mask = masked(&grouped, 1, &plane, true);
    assert_eq!(
        refusal(&group_mask, &apply_mask(1), &[]),
        "applying a mask requires raster pixels; groups have no pixels to bake"
    );
}

// --------------------------------------------------------------- layer via --

#[test]
fn copy_weights_alpha_by_the_selection_and_keeps_rgb_bits() {
    let doc = fixture();
    let plane = coverage();
    let out = must(&doc, &via(0, false, true), &plane);
    assert_eq!(names(&out), ["Background", "Via"]);
    let source = floats(&doc, 0);
    assert_eq!(
        floats(&out, 0)
            .iter()
            .map(|p| p.map(f32::to_bits))
            .collect::<Vec<_>>(),
        source
            .iter()
            .map(|p| p.map(f32::to_bits))
            .collect::<Vec<_>>(),
        "a copy leaves the source alone"
    );
    let copy = floats(&out, 1);
    for (i, (got, src)) in copy.iter().zip(&source).enumerate() {
        assert_eq!(rgb_bits(*got), rgb_bits(*src), "pixel {i}: RGB bits");
        let want = f64::from(src[3]) * f64::from(plane[i]) / 65535.0;
        assert_alpha(got[3], want, &format!("pixel {i}"));
        if plane[i] == 65535 {
            assert_eq!(got[3].to_bits(), src[3].to_bits(), "pixel {i}: full");
        }
    }
    let record = out.layer(1).unwrap();
    assert_eq!(record.offset(), (0, 0));
    assert_eq!(record.source().unwrap().dimensions(), (W, H));
    assert!(record.mask().is_none(), "the copy carries no mask");
    assert_eq!(record.opacity(), 1.0);
    assert!(record.visible() && !record.clipped());
    assert!(record.style().is_none() && record.reported_style().is_none());
    assert_ne!(out.layer_id(1), out.layer_id(0), "a newly minted id");
    assert!(out.is_float());
}

#[test]
fn cut_erases_the_source_under_the_same_coverage() {
    let doc = fixture();
    let plane = coverage();
    let out = must(&doc, &via(0, true, true), &plane);
    assert_eq!(names(&out), ["Background", "Via"]);
    let source = floats(&doc, 0);
    let after = floats(&out, 0);
    let copy = floats(&out, 1);
    for i in 0..source.len() {
        let c = f64::from(plane[i]) / 65535.0;
        let a = f64::from(source[i][3]);
        assert_eq!(rgb_bits(copy[i]), rgb_bits(source[i]), "pixel {i}: copy");
        assert_alpha(copy[i][3], a * c, &format!("copy pixel {i}"));
        assert_eq!(
            rgb_bits(after[i]),
            rgb_bits(source[i]),
            "pixel {i}: the erase keeps RGB"
        );
        assert_alpha(after[i][3], a * (1.0 - c), &format!("source pixel {i}"));
        if plane[i] == 0 {
            assert_eq!(after[i][3].to_bits(), source[i][3].to_bits(), "pixel {i}");
        }
        if plane[i] == 65535 {
            assert_eq!(after[i][3], 0.0, "pixel {i}: fully cut");
        }
    }
}

#[test]
fn a_whole_layer_copy_keeps_the_off_canvas_columns() {
    let doc = hanging();
    let out = must(&doc, &via(0, false, false), &[]);
    assert_eq!(names(&out), ["Background", "Via"]);
    assert_eq!(out.layer(1).unwrap().offset(), (-5, 0));
    let bits = |d: &PrecisionDocument, i: usize| -> Vec<[u32; 4]> {
        floats(d, i).iter().map(|p| p.map(f32::to_bits)).collect()
    };
    assert_eq!(
        bits(&out, 1),
        bits(&doc, 0),
        "every pixel, on or off the canvas"
    );
    assert_eq!(bits(&out, 0), bits(&doc, 0));
}

#[test]
fn a_full_selection_zeroes_the_off_canvas_columns() {
    let doc = hanging();
    let plane = vec![65535u16; 9];
    let out = must(&doc, &via(0, false, true), &plane);
    let source = floats(&doc, 0);
    let copy = floats(&out, 1);
    assert_eq!(out.layer(1).unwrap().offset(), (-5, 0));
    for (i, (got, src)) in copy.iter().zip(&source).enumerate() {
        let lx = i % 8;
        assert_eq!(rgb_bits(*got), rgb_bits(*src), "pixel {i}: RGB");
        if lx < 5 {
            assert_eq!(got[3], 0.0, "column {lx}: never selected");
        } else {
            assert_eq!(got[3].to_bits(), src[3].to_bits(), "column {lx}");
        }
    }
}

#[test]
fn a_whole_layer_cut_empties_the_canvas_part_only() {
    let doc = hanging();
    let out = must(&doc, &via(0, true, false), &[]);
    let source = floats(&doc, 0);
    let copy = floats(&out, 1);
    let after = floats(&out, 0);
    for (i, src) in source.iter().enumerate() {
        let lx = i % 8;
        assert_eq!(
            copy[i].map(f32::to_bits),
            src.map(f32::to_bits),
            "the copy holds every pixel"
        );
        assert_eq!(rgb_bits(after[i]), rgb_bits(*src), "pixel {i}: RGB kept");
        if lx < 5 {
            assert_eq!(after[i][3].to_bits(), src[3].to_bits(), "column {lx}");
        } else {
            assert_eq!(after[i][3], 0.0, "column {lx}: erased");
        }
    }
}

#[test]
fn the_copy_lands_above_its_source_at_the_source_level() {
    let doc = fixture();
    let grouped = must(
        &doc,
        &json!({"op": "group_layers", "layers": [0], "name": "G", "pass_through": false}),
        &[],
    );
    assert_eq!(names(&grouped), ["Background", "G"]);
    let out = must(&grouped, &via(0, false, false), &[]);
    assert_eq!(names(&out), ["Background", "Via", "G"]);
    let children: Vec<&str> = out.root_layers()[0]
        .children()
        .unwrap()
        .iter()
        .map(|l| l.name())
        .collect();
    assert_eq!(children, ["Background", "Via"], "inside the source's group");
    assert_eq!(
        refusal(&grouped, &via(1, false, false), &[]),
        "Layer Via requires a raster layer; layer 1 is a group"
    );
    assert_eq!(
        refusal(&doc, &via(9, false, false), &[]),
        "high-depth layer via index is out of range"
    );
}

#[test]
fn nothing_copied_and_nothing_erased_answer_no_change() {
    let doc = fixture();
    let none = vec![0u16; (W * H) as usize];
    assert!(edit(&doc, &via(0, false, true), &none).unwrap().is_none());
    assert!(edit(&doc, &via(0, true, true), &none).unwrap().is_none());
    // A fully transparent layer has nothing to copy, whole or selected.
    let clear = float_doc(
        W,
        H,
        &IccProfile::srgb(),
        &[vec![[2.0, -0.5, 0.25, 0.0]; (W * H) as usize]],
    );
    assert!(edit(&clear, &via(0, false, false), &[]).unwrap().is_none());
    assert!(edit(&clear, &via(0, true, false), &[]).unwrap().is_none());
}

#[test]
fn locks_gate_the_cut_and_not_the_copy() {
    let doc = fixture();
    let plane = coverage();
    let locked = doc.with_layer_locks(0, LOCK_PIXELS).unwrap().unwrap();
    // The copy adds a record and writes none.
    let copied = must(&locked, &via(0, false, true), &plane);
    assert_eq!(names(&copied), ["Background", "Via"]);
    let message = refusal(&locked, &via(0, true, true), &plane);
    assert!(message.contains("lock"), "{message}");
    // A transparency lock makes the erase change nothing, so the cut is the
    // exact no-op it is at 16 bits.
    let frozen = doc.with_layer_locks(0, LOCK_TRANSPARENCY).unwrap().unwrap();
    assert!(edit(&frozen, &via(0, true, true), &plane)
        .unwrap()
        .is_none());
    assert!(edit(&frozen, &via(0, false, true), &plane)
        .unwrap()
        .is_some());
}

/// Record `index`'s description through
/// `dr_precision_doc_artwork_description`, NULL as `None`.
fn description(document: &PrecisionDocument, index: usize) -> Option<String> {
    // The same owned-string read as an error's.
    take_error(unsafe { dr_precision_doc_artwork_description(document, index) })
}

#[test]
fn a_cut_drops_the_source_description_and_a_copy_keeps_it() {
    let base = fixture();
    let text =
        CString::new(r#"{"type":"shape","version":4,"artwork":{"ink":"linear_srgb"}}"#).unwrap();
    let mut error = ptr::null_mut();
    let described =
        unsafe { dr_precision_doc_artwork_metadata(&base, 0, text.as_ptr(), LIMIT, 0, &mut error) };
    assert_eq!(take_error(error), None);
    assert!(!described.is_null(), "the description is set");
    let described = unsafe { Box::from_raw(described) };
    let before = description(&described, 0).expect("a description");
    let plane = coverage();

    // The copy writes no source pixel, so the source is still what its
    // description says; the new record is plain pixels.
    let copied = must(&described, &via(0, false, true), &plane);
    assert_eq!(description(&copied, 0).as_deref(), Some(before.as_str()));
    assert_eq!(description(&copied, 1), None, "the copy carries none");

    // The cut rewrites the source's alpha, so the description no longer
    // describes its pixels and goes in the same edit.
    let cut = must(&described, &via(0, true, true), &plane);
    assert_eq!(description(&cut, 0), None, "the cut source is rasterized");
    assert_eq!(description(&cut, 1), None, "the copy carries none");
    let source = floats(&described, 0);
    let after = floats(&cut, 0);
    for i in 0..source.len() {
        let c = f64::from(plane[i]) / 65535.0;
        assert_eq!(rgb_bits(after[i]), rgb_bits(source[i]), "pixel {i}: RGB");
        assert_alpha(
            after[i][3],
            f64::from(source[i][3]) * (1.0 - c),
            &format!("source pixel {i}"),
        );
    }
}

#[test]
fn a_cut_detaches_a_bound_source_and_a_copy_keeps_the_binding() {
    use crate::float_placed_image::{info, mixed, p3_codes, place, typed_master};
    // A 3×2 16-bit Display P3 master bound to record 1 at the canvas origin.
    let master = typed_master(3, 2, p3_codes(3, 2), IccProfile::display_p3());
    let placed = mixed(&fixture(), None, Some(&master), place("bound", 0, 0))
        .unwrap()
        .expect("placed");
    assert_eq!(names(&placed), ["Background", "bound"]);
    assert_eq!(info(&placed, 1)["valid"], true);
    let plane = coverage();

    // The copy writes no source pixel: the binding stays, and the new record
    // is plain pixels with no master.
    let copied = must(&placed, &via(1, false, true), &plane);
    assert_eq!(names(&copied), ["Background", "bound", "Via"]);
    assert_eq!(info(&copied, 1)["valid"], true, "the source stays bound");
    assert_eq!(info(&copied, 2)["valid"], false, "the copy has no master");

    // The cut erases the source's render, which is then no longer its
    // master's: the binding goes in the same edit, as a 16-bit erase's does.
    let cut = must(&placed, &via(1, true, true), &plane);
    assert_eq!(names(&cut), ["Background", "bound", "Via"]);
    assert_eq!(info(&cut, 1)["valid"], false, "the cut source is detached");
    assert_eq!(info(&cut, 2)["valid"], false, "the copy has no master");
    let source = floats(&placed, 1);
    let after = floats(&cut, 1);
    let copy = floats(&cut, 2);
    assert_eq!(source.len(), 6);
    for (i, src) in source.iter().enumerate() {
        // The record sits at (0, 0), so its pixel (x, y) is canvas (x, y).
        let c = f64::from(plane[(i / 3) * W as usize + i % 3]) / 65535.0;
        let a = f64::from(src[3]);
        assert_eq!(rgb_bits(after[i]), rgb_bits(*src), "pixel {i}: RGB kept");
        assert_alpha(after[i][3], a * (1.0 - c), &format!("source pixel {i}"));
        assert_eq!(rgb_bits(copy[i]), rgb_bits(*src), "pixel {i}: copy RGB");
        assert_alpha(copy[i][3], a * c, &format!("copy pixel {i}"));
    }
}

#[test]
fn the_plan_is_the_threshold_and_the_tables_name_the_float_families() {
    let doc = fixture();
    let selected = plan_at(&doc, &via(0, false, true), LIMIT).unwrap();
    assert_eq!(selected["input_samples"], json!(W * H));
    let whole = plan_at(&doc, &via(0, false, false), LIMIT).unwrap();
    assert_eq!(whole["input_samples"], json!(0));
    let scratch = |cut: bool, selection: bool| {
        plan_at(&doc, &via(0, cut, selection), LIMIT).unwrap()["scratch_bytes"]
            .as_u64()
            .unwrap()
    };
    // A cut plans the erase's copy of the record on top of the copy's plan,
    // and a whole-layer cut its coverage plane too.
    assert!(scratch(true, true) >= scratch(false, true) + u64::from(W * H) * 16);
    assert_eq!(
        scratch(true, false),
        scratch(true, true) + u64::from(W * H) * 2,
        "the core-built coverage plane"
    );
    // The plan is the threshold: one byte under it refuses, plan and edit.
    let retained = selected["retained_bytes"].as_u64().unwrap();
    let total = retained + selected["scratch_bytes"].as_u64().unwrap();
    assert!(plan_at(&doc, &via(0, false, true), total).is_ok());
    assert!(plan_at(&doc, &via(0, false, true), total - 1).is_err());
    assert!(edit_at(&doc, &via(0, false, true), &coverage(), total - 1).is_err());
    assert!(edit_at(&doc, &via(0, false, true), &coverage(), total)
        .unwrap()
        .is_some());
    // The same for an applied mask.
    let with_mask = masked(&doc, 0, &coverage(), true);
    let plan = plan_at(&with_mask, &apply_mask(0), LIMIT).unwrap();
    let total = plan["retained_bytes"].as_u64().unwrap() + plan["scratch_bytes"].as_u64().unwrap();
    assert!(plan["scratch_bytes"].as_u64().unwrap() >= u64::from(W * H) * 48);
    assert!(edit_at(&with_mask, &apply_mask(0), &[], total - 1).is_err());
    assert!(edit_at(&with_mask, &apply_mask(0), &[], total)
        .unwrap()
        .is_some());

    assert_eq!(capability("layer_via", "raster", 32)["supported"], true);
    assert_eq!(capability("layer_via", "group", 32)["supported"], false);
    assert_eq!(
        capability("color_assign", "document", 32)["supported"],
        true
    );
    assert_eq!(
        capability("color_convert", "document", 32)["supported"],
        true
    );
    let table: Value = serde_json::from_str(&float_operators_json()).unwrap();
    let admitted: Vec<&str> = table["admitted"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for row in ["apply_layer_mask", "layer_via"] {
        assert!(admitted.contains(&row), "{row} is admitted");
    }
    let refused: Vec<&str> = table["refused"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["operator"].as_str())
        .collect();
    assert!(refused.contains(&"Apply Image"), "{refused:?}");
}

// ---------------------------------------------------- measured admission --

pub(crate) const MW: u32 = 4000;
pub(crate) const MH: u32 = 3000;
/// The selection cases' coverage rect, at 65535.
const MRECT: (u32, u32, u32, u32) = (200, 150, 3600, 2700);
/// The measurement runs at an 8 GiB working limit.
pub(crate) const MLIMIT: u64 = 8_589_934_592;

/// A 4000×3000 sRGB float document of one opaque, non-uniform record, with
/// values above white and below zero.
pub(crate) fn measure_fixture() -> PrecisionDocument {
    let px: Vec<[f32; 4]> = (0..MW * MH)
        .map(|i| {
            let (x, y) = (i % MW, i / MW);
            [
                (x % 1000) as f32 / 250.0 - 0.5,
                (y % 777) as f32 / 777.0,
                ((x ^ y) & 1023) as f32 / 512.0,
                1.0,
            ]
        })
        .collect();
    float_doc_at(MLIMIT, MW, MH, &IccProfile::srgb(), &[px])
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

/// The request of a measured case, and whether it reads the coverage plane.
fn measure_case(case: &str) -> (Value, bool) {
    match case {
        "apply_mask" => (apply_mask(0), false),
        "via_copy" => (via(0, false, true), true),
        "via_cut" => (via(0, true, true), true),
        "via_copy_whole" => (via(0, false, false), false),
        "via_cut_whole" => (via(0, true, false), false),
        other => panic!("unknown DR_FLOAT_CASE {other}"),
    }
}

/// The document a measured case edits: the fixture, and for `apply_mask` a
/// mask that is 32768 inside the coverage rect and 0 outside, so the apply
/// changes every pixel's alpha and no tile of the result is shared with the
/// source.
fn measure_document(case: &str, plane: &[u16]) -> PrecisionDocument {
    let doc = measure_fixture();
    if case != "apply_mask" {
        return doc;
    }
    let mask: Vec<u16> = plane.iter().map(|&c| c / 2 + c % 2).collect();
    let request = json!({"op": "set_mask", "layer": 0, "width": MW, "height": MH});
    *edit_at(&doc, &request, &mask, MLIMIT)
        .expect("the mask is admitted")
        .expect("the mask is set")
}

/// Max E per case, bytes, from `admission_measure` (the module docs of
/// `precision_mask_apply.rs` and `precision_layer_via.rs` hold the records).
const MEASURED_MAX_E: [(&str, u64); 5] = [
    ("apply_mask", 384_991_496),
    ("via_copy", 387_399_992),
    ("via_cut", 572_555_672),
    ("via_copy_whole", 385_007_928),
    ("via_cut_whole", 601_129_392),
];

#[test]
fn plans_cover_the_measured_peaks() {
    let plane = measure_coverage();
    let base = measure_fixture();
    let with_mask = measure_document("apply_mask", &plane);
    for (case, max_e) in MEASURED_MAX_E {
        let (request, _) = measure_case(case);
        let doc = if case == "apply_mask" {
            &with_mask
        } else {
            &base
        };
        let scratch = plan_at(doc, &request, MLIMIT).unwrap()["scratch_bytes"]
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
pub(crate) struct RusageInfoV4 {
    uuid: [u8; 16],
    pub(crate) fields: [u64; 35],
}
pub(crate) const PHYS_FOOTPRINT: usize = 7;
pub(crate) const INTERVAL_MAX_PHYS_FOOTPRINT: usize = 33;

extern "C" {
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut RusageInfoV4) -> i32;
    fn proc_reset_footprint_interval(pid: i32) -> i32;
}

pub(crate) fn rusage() -> RusageInfoV4 {
    let mut info = RusageInfoV4 {
        uuid: [0; 16],
        fields: [0; 35],
    };
    let rc = unsafe { proc_pid_rusage(std::process::id() as i32, 4, &mut info) };
    assert_eq!(rc, 0, "proc_pid_rusage");
    info
}

/// The footprint now, with the interval peak reset to it.
pub(crate) fn footprint_reset() -> u64 {
    let pre = rusage().fields[PHYS_FOOTPRINT];
    unsafe { proc_reset_footprint_interval(std::process::id() as i32) };
    pre
}

/// One case per process, selected by `DR_FLOAT_CASE` (`baseline`,
/// `apply_mask`, `via_copy`, `via_cut`, `via_copy_whole`, `via_cut_whole`):
/// build the fixture (and the coverage plane, which is the host's), read the
/// footprint, reset the interval peak, run the edit through the FFI with the
/// plan's own admission, and print E = interval peak − pre-edit footprint
/// beside the plan's scratch, with the result held until E is read.
/// `baseline` builds the same fixture and coverage and runs nothing: the
/// twin's E is the harness's own noise.
#[test]
#[ignore = "measurement: DR_FLOAT_CASE=baseline|apply_mask|via_copy|via_cut|via_copy_whole|via_cut_whole, Release, MallocLargeCache=0"]
fn admission_measure() {
    let case = std::env::var("DR_FLOAT_CASE").expect("DR_FLOAT_CASE");
    let plane = measure_coverage();
    let baseline = case == "baseline";
    let doc = measure_document(if baseline { "via_copy" } else { &case }, &plane);
    let (request, selection) = if baseline {
        measure_case("via_copy")
    } else {
        measure_case(&case)
    };
    let scratch = plan_at(&doc, &request, MLIMIT).unwrap()["scratch_bytes"]
        .as_u64()
        .unwrap();
    let input: &[u16] = if selection { &plane } else { &[] };
    let pre = footprint_reset();
    let out = if baseline {
        None
    } else {
        Some(
            edit_at(&doc, &request, input, MLIMIT)
                .expect("the edit is admitted")
                .expect("the edit changes the document"),
        )
    };
    let peak = rusage().fields[INTERVAL_MAX_PHYS_FOOTPRINT];
    assert_eq!(out.is_none(), baseline);
    drop(out);
    println!(
        "FLOAT_MEASURE case={case} scratch={scratch} pre={pre} peak={peak} E={}",
        peak.saturating_sub(pre)
    );
}
