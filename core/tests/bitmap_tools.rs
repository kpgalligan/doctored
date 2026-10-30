//! #1 Bitmap phase 3 through the FFI: the tools phase 3 built on a Bitmap
//! document (the Gradient, the Clone Stamp, the History Brush, Liquify,
//! Perspective Crop, Copy, the layer row's name, label and lock, and the
//! source selections) and the refusals it made for good, in their three
//! tiers.
//!
//! The oracle of each built pixel tool is independent of the adapter: the
//! same op on the Gray twin (Image ▸ Mode ▸ Grayscale at Size Ratio 1,
//! `dr_precision_doc_convert_bitmap_to_gray`), then this file's own
//! threshold of its canvas window, written from the rule's definition
//! (`bitmap_edits::thresholded`: white iff `g·a + bg·(255 − a) ≥ 128·255`,
//! bg white).

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_clipboard::{
    dr_clipboard_payload_free, dr_precision_doc_clipboard_copy, dr_precision_doc_clipboard_paste,
    dr_precision_doc_from_clipboard, DrClipboardPayload,
};
use doctored_core::ffi_precision::dr_precision_doc_save_native;
use doctored_core::ffi_precision_edit::dr_precision_doc_select;
use doctored_core::ffi_retouch::dr_precision_doc_retouch_from;
use doctored_core::ffi_retouch_brush::{
    dr_precision_doc_art_history_brush, dr_precision_doc_history_brush, dr_precision_doc_smudge,
};
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_edit;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::bitmap_edits::{bitmap_of, scene, scene_bits, thresholded, H, W};
use crate::bitmap_mode::{bits, to_gray};
use crate::common::{cpath, take_err_string};
use crate::indexed_fixture::{descriptor, edit_plan, free, open_rgb, paint, window, LIMIT};

const N: usize = (W * H) as usize;

const BLEND_REFUSAL: &str = "The Brush, Fill, Stroke, the Paint Bucket, the Gradient and the \
                             Clone Stamp paint in Normal, Darken or Lighten mode in a Bitmap \
                             document.";

/// The three refusal tiers (`bitmap_document::refused_sentence`).
fn disabled(op: &str) -> String {
    format!("{op} is not available in Bitmap mode.")
}
fn one_layer(op: &str) -> String {
    format!("{op} needs more than one layer, and a Bitmap document holds one.")
}
fn unoffered(op: &str) -> String {
    format!(
        "{op} is unavailable in a Bitmap document: Doctored offers it on Grayscale and colour \
         documents only; convert to Grayscale first."
    )
}

/// The Gray twin of `doc` at Size Ratio 1.
fn twin(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    to_gray(doc, &json!({})).unwrap()
}

/// The twin's result thresholded over white.
fn expected(twin_out: *const PrecisionDocument) -> Vec<u8> {
    thresholded(&window(twin_out), 255)
}

/// `request` on `doc` and on its Gray twin, both changing something: the
/// Bitmap result, and the twin's thresholded.
fn both(
    doc: *const PrecisionDocument,
    request: &Value,
    input: &[u16],
) -> (*mut PrecisionDocument, Vec<u8>) {
    let gray = twin(doc);
    let out = paint(doc, request, input)
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request}: no bit moved"));
    let twin_out = paint(gray, request, input)
        .unwrap_or_else(|e| panic!("{request} on Gray: {e}"))
        .unwrap_or_else(|| panic!("{request} on Gray: nothing changed"));
    let oracle = expected(twin_out);
    free(twin_out);
    free(gray);
    (out, oracle)
}

fn gradient(opacity: f64, blend_mode: i64) -> Value {
    json!({"op":"gradient","layer":0,"start":[0.0,0.0,0.0,1.0],"end":[1.0,1.0,1.0,1.0],
           "color_space":"document","opacity":opacity,"blend_mode":blend_mode,
           "from":[0.0,0.0],"to":[f64::from(W),0.0],"radial":false})
}

#[test]
fn the_gradient_is_the_gray_ramp_thresholded() {
    let doc = scene();
    let full = vec![65535u16; N];
    let (linear, oracle) = both(doc, &gradient(1.0, 0), &full);
    assert_eq!(bits(linear), oracle);
    // The ramp crosses 128 halfway: the left half black, the right white.
    assert!(oracle[..(W / 2 - 1) as usize].iter().all(|&b| b == 0));
    assert!(oracle[(W / 2 + 1) as usize..W as usize]
        .iter()
        .all(|&b| b == 1));
    assert_eq!(descriptor(linear)["color_mode"], json!("bitmap"));
    let radial = json!({"op":"gradient","layer":0,"start":[0.0,0.0,0.0,1.0],
        "end":[0.0,0.0,0.0,1.0],"color_space":"document","opacity":1.0,"blend_mode":0,
        "from":[6.0,4.0],"to":[12.0,4.0],"radial":true,"stops":[
            {"position":0.0,"color":[0.0,0.0,0.0,1.0]},
            {"position":0.5,"color":[1.0,1.0,1.0,1.0]},
            {"position":1.0,"color":[0.0,0.0,0.0,1.0]}]});
    let (ringed, oracle) = both(doc, &radial, &full);
    assert_eq!(bits(ringed), oracle);
    assert_ne!(bits(ringed), scene_bits());
    // At 0.4 the ramp's darkest end over white is 153: no bit moves.
    let white = bitmap_of(W, H, &[1; N]);
    assert_eq!(paint(white, &gradient(0.4, 0), &full), Ok(None));
    assert_eq!(
        paint(doc, &gradient(1.0, 1), &full),
        Err(BLEND_REFUSAL.into())
    );
    for d in [white, ringed, linear, doc] {
        free(d);
    }
}

fn band_coverage(value: u16) -> Vec<u16> {
    (0..N as u32)
        .map(|i| if (6..10).contains(&(i % W)) { value } else { 0 })
        .collect()
}

#[test]
fn the_clone_stamp_copies_bits_and_its_soft_edge_is_thresholded() {
    let doc = scene();
    let before = bits(doc);
    let request = json!({"op":"retouch_clone","layer":0,"matrix":[1,0,0,1,-6,0]});
    // Hard, full opacity: the black band (with the white marker at (2, 2))
    // copied onto the white one.
    let out = paint(doc, &request, &band_coverage(65535))
        .unwrap()
        .expect("the clone moves bits");
    let copied: Vec<u8> = (0..N)
        .map(|i| {
            if (6..10).contains(&(i as u32 % W)) {
                before[i - 6]
            } else {
                before[i]
            }
        })
        .collect();
    assert_eq!(bits(out), copied);
    free(out);
    // A soft stroke: black at 61 % over white is 99, so it lands.
    let (soft, oracle) = both(doc, &request, &band_coverage(40000));
    assert_eq!(bits(soft), oracle);
    assert_ne!(bits(soft), before);
    free(soft);
    let mut multiply = request.clone();
    multiply["blend_mode"] = json!(1);
    assert_eq!(
        paint(doc, &multiply, &band_coverage(65535)),
        Err(BLEND_REFUSAL.into())
    );
    let mut layered = request.clone();
    layered["output"] = json!("new_layer");
    assert_eq!(
        paint(doc, &layered, &band_coverage(65535)),
        Err(disabled("add_layer"))
    );
    assert_eq!(bits(doc), before);
    free(doc);
}

fn history_brush(
    doc: *const PrecisionDocument,
    source: *const PrecisionDocument,
    coverage: &[u16],
) -> *mut PrecisionDocument {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_history_brush(
            doc,
            0,
            source,
            0,
            1.0,
            coverage.as_ptr(),
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    out
}

#[test]
fn the_history_brush_gives_the_earlier_bits_back() {
    let doc = scene();
    let before = bits(doc);
    let fill = json!({"op":"paint_blend","layer":0,"color":[1.0,1.0,1.0,1.0],
        "color_space":"document","opacity":1.0,"blend_mode":0});
    let painted = paint(doc, &fill, &[65535; N])
        .unwrap()
        .expect("the fill whitens the black half");
    assert_ne!(bits(painted), before);
    let restored = history_brush(painted, doc, &[65535; N]);
    assert_eq!(bits(restored), before);
    assert_eq!(descriptor(restored)["color_mode"], json!("bitmap"));
    // A soft stroke against the twins' own History Brush, thresholded.
    let soft = band_coverage(40000);
    let soft_coverage: Vec<u16> = (0..N as u32)
        .map(|i| if i % W < 4 { 40000 } else { soft[i as usize] })
        .collect();
    let partial = history_brush(painted, doc, &soft_coverage);
    let (painted_twin, source_twin) = (twin(painted), twin(doc));
    let twin_out = history_brush(painted_twin, source_twin, &soft_coverage);
    assert_eq!(bits(partial), expected(twin_out));
    assert_ne!(bits(partial), bits(painted));
    for d in [
        twin_out,
        source_twin,
        painted_twin,
        partial,
        restored,
        painted,
        doc,
    ] {
        free(d);
    }
}

fn selected_edit(
    doc: *const PrecisionDocument,
    request: &Value,
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() && !err.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

#[test]
fn liquify_is_the_gray_liquify_thresholded_and_the_rest_refuses() {
    // 32 × 16: black left of x = 16, white right of it.
    let (w, h) = (32u32, 16u32);
    let plane: Vec<u8> = (0..w * h).map(|i| u8::from(i % w >= 16)).collect();
    let doc = bitmap_of(w, h, &plane);
    let request = json!({"layer":0,"op":"liquify","params":{"strokes":[
        {"tool":"forward_warp","size":14,"strength":1.0,"points":[[10.0,8.0],[22.0,8.0]]}]}});
    let out = selected_edit(doc, &request).unwrap();
    assert!(!out.is_null(), "the warp moves bits");
    let gray = twin(doc);
    let twin_out = selected_edit(gray, &request).unwrap();
    assert!(!twin_out.is_null());
    assert_eq!(bits(out), expected(twin_out));
    assert_ne!(bits(out), plane);
    assert_eq!(descriptor(out)["color_mode"], json!("bitmap"));
    assert_eq!(
        selected_edit(
            doc,
            &json!({"layer":0,"op":"adjustment","description":"{}"})
        ),
        Err(disabled("adjustment"))
    );
    assert_eq!(
        selected_edit(
            doc,
            &json!({"layer":0,"op":"gaussian_blur","params":{"radius":2.0}})
        ),
        Err(disabled("filter"))
    );
    for d in [twin_out, gray, out, doc] {
        free(d);
    }
}

#[test]
fn perspective_crop_is_the_gray_crop_thresholded() {
    let doc = scene();
    let request = json!({"op":"perspective_crop","quad":[1.0,1.0,11.0,0.0,12.0,8.0,0.0,7.0],
        "width":10,"height":6,"filter":"bilinear"});
    let (out, oracle) = both(doc, &request, &[]);
    assert_eq!(unsafe { &*out }.dimensions(), (10, 6));
    assert_eq!(bits(out), oracle);
    assert_eq!(descriptor(out)["color_mode"], json!("bitmap"));
    free(out);
    free(doc);
}

#[test]
fn the_layer_row_keeps_the_plane_and_survives_rz() {
    let dir = TempDir::new().unwrap();
    let doc = scene();
    let before = bits(doc);
    let edited = |from: *const PrecisionDocument, request: Value| {
        paint(from, &request, &[])
            .unwrap_or_else(|e| panic!("{request}: {e}"))
            .unwrap_or_else(|| panic!("{request}: no change"))
    };
    let renamed = edited(doc, json!({"op":"rename_layer","layer":0,"name":"Plate"}));
    let labelled = edited(renamed, json!({"op":"layer_label","layer":0,"label":3}));
    let locked = edited(labelled, json!({"op":"layer_lock","layer":0,"locks":2}));
    for out in [renamed, labelled, locked] {
        assert_eq!(bits(out), before);
        assert_eq!(descriptor(out)["color_mode"], json!("bitmap"));
    }
    let path = dir.path().join("row.rz");
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_native(locked, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
        },
        "{}",
        take_err_string(err)
    );
    let back = open_rgb(&path).unwrap();
    let layer = unsafe { &*back }.layer(0).unwrap();
    assert_eq!(
        (layer.name(), layer.label(), layer.locks()),
        ("Plate", 3, 2)
    );
    assert_eq!(descriptor(back)["color_mode"], json!("bitmap"));
    assert_eq!(bits(back), before);
    // Lock Pixels refuses a paint with the lock's own sentence.
    let refused = paint(
        locked,
        &json!({"op":"paint_blend","layer":0,"color":[1.0,1.0,1.0,1.0],
                "color_space":"document","opacity":1.0,"blend_mode":0}),
        &[65535; N],
    )
    .unwrap_err();
    assert!(
        refused.contains("locked") && !refused.contains("Bitmap"),
        "{refused}"
    );
    for d in [back, locked, labelled, renamed, doc] {
        free(d);
    }
}

fn select(doc: *const PrecisionDocument, operation: Value) -> Result<Vec<u16>, String> {
    let text = CString::new(json!({"op":"selection","operation":operation}).to_string()).unwrap();
    let mut out = vec![0u16; N];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_select(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if ok {
        Ok(out)
    } else {
        Err(take_err_string(err))
    }
}

#[test]
fn the_source_selections_pass_and_the_disabled_ones_refuse() {
    let doc = scene();
    let before = bits(doc);
    let quick = select(doc, json!({"kind":"quick","params":{"seeds":[[0,0]]}})).unwrap();
    assert!(quick.iter().any(|&v| v > 0));
    let luminance = select(doc, json!({"kind":"luminance"})).unwrap();
    // White pixels select fully, black ones not at all.
    for (i, &bit) in before.iter().enumerate() {
        assert_eq!(luminance[i], if bit == 1 { 65535 } else { 0 }, "{i}");
    }
    assert!(edit_plan(
        doc,
        &json!({"op":"selection","operation":{"kind":"quick",
        "params":{"seeds":[[0,0]]}}})
    )
    .is_ok());
    assert_eq!(bits(doc), before);
    assert_eq!(
        select(doc, json!({"kind":"color_range","params":{}})),
        Err(disabled("color_range"))
    );
    assert_eq!(
        select(doc, json!({"kind":"refine_edge","edge_radius":2.0})),
        Err(disabled("refine_edge"))
    );
    free(doc);
}

fn copy_layer(doc: *const PrecisionDocument) -> *mut DrClipboardPayload {
    let request =
        CString::new(json!({"op":"copy","layer":0,"x":0,"y":0,"width":W,"height":H}).to_string())
            .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_clipboard_copy(
            doc,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    out
}

#[test]
fn copy_holds_the_bits_and_paste_and_retouch_from_refuse() {
    let doc = scene();
    let payload = copy_layer(doc);
    let mut err = ptr::null_mut();
    let made = unsafe { dr_precision_doc_from_clipboard(payload, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!made.is_null(), "{}", take_err_string(err));
    let copied: Vec<[u8; 4]> = scene_bits()
        .iter()
        .map(|&b| if b == 1 { [255; 4] } else { [0, 0, 0, 255] })
        .collect();
    assert_eq!(window(made), copied);
    let request = CString::new(
        json!({"name":"Pasted","anchor":0,"position":"above","placement":"centered"}).to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let pasted = unsafe {
        dr_precision_doc_clipboard_paste(
            doc,
            payload,
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(pasted.is_null());
    assert_eq!(take_err_string(err), disabled("paste"));
    let clone =
        CString::new(json!({"op":"retouch_clone","layer":0,"matrix":[1,0,0,1,-6,0]}).to_string())
            .unwrap();
    let coverage = band_coverage(65535);
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_retouch_from(
            doc,
            doc,
            clone.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(take_err_string(err), unoffered("content_aware_move"));
    unsafe { dr_clipboard_payload_free(payload) };
    free(made);
    free(doc);
}

#[test]
fn each_tier_refuses_by_its_own_sentence() {
    let doc = scene();
    let mut err = ptr::null_mut();
    let smudged = unsafe {
        dr_precision_doc_smudge(
            doc,
            0,
            ptr::null(),
            ptr::null(),
            0,
            0.5,
            ptr::null(),
            0,
            false,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(smudged.is_null());
    assert_eq!(take_err_string(err), unoffered("smudge"));
    let mut err = ptr::null_mut();
    let art = unsafe {
        dr_precision_doc_art_history_brush(
            doc,
            0,
            doc,
            0,
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(art.is_null());
    assert_eq!(take_err_string(err), unoffered("art_history_brush"));
    for (request, sentence) in [
        (
            json!({"op":"arrange_layer","layer":0}),
            one_layer("arrange_layer"),
        ),
        (
            json!({"op":"puppet_selected","layer":0}),
            disabled("puppet_selected"),
        ),
        (json!({"op":"layer_comp"}), unoffered("layer_comp")),
    ] {
        assert_eq!(paint(doc, &request, &[]), Err(sentence), "{request}");
    }
    free(doc);
}
