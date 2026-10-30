//! #547 phase 4, geometry, warps, selected pixels and selections on a
//! Grayscale document, through the FFI. The oracle is the RGB TWIN
//! (`gray_ops::twin`: the Gray document converted to RGB through its own
//! profile bytes, which keeps every sample) running the same op: at 8, 16
//! and 32 bits every result equals the twin's bit for bit on every channel
//! and keeps R = G = B, and stays a Gray document. Covered: Image Size,
//! Crop, Canvas Size, Straighten, Rotate Canvas, the quarter turns and
//! flips, Free Transform (affine and perspective, a layer and its mask, and
//! the sets), Perspective Crop, Trim (the kept rectangle of the projection,
//! then Crop), Layer ▸ Warp, Puppet Warp, Perspective Warp, Liquify, the
//! selected pixels (move, transform, puppet, perspective), the Magic Wand
//! and Color Range. A Gray warp keeps no binding (`warp_info` null), and
//! Duplicate to Document is a Gray document. A colour-plane target of the
//! selected pixels and a `color` selection refuse with the colour-plane
//! sentence. Bounds: exact (declared before measuring).

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_perspective_warp::dr_precision_doc_perspective_warp;
use doctored_core::ffi_precision::dr_precision_doc_descriptor;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit_plan, dr_precision_doc_select};
use doctored_core::ffi_puppet::dr_precision_doc_puppet_warp;
use doctored_core::ffi_trim::dr_precision_image_trim_rect;
use doctored_core::ffi_warp::{dr_precision_doc_warp_edit, dr_precision_doc_warp_info};
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::gray_ops::{
    assert_neutral, edit, free, gray_doc, layer, plan_json, plan_samples, selected, twin, LIMIT,
};

const W: u32 = 24;
const H: u32 = 17;
const DEPTHS: [u32; 3] = [8, 16, 32];
const PLANE: &str = "A colour plane is unavailable in a Grayscale document: its one colour channel, Gray, is the layer, and Doctored offers the Red, Green, Blue and Alpha targets on RGB documents only.";

fn descriptor(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_descriptor(doc, &mut err) };
    plan_json(out, err).unwrap()
}

/// A rotation by `degrees` and a scale about `centre`, as an affine matrix.
fn turn(degrees: f64, scale: f64, centre: (f64, f64)) -> [f64; 6] {
    let (s, c) = degrees.to_radians().sin_cos();
    let (a, b) = (c * scale, s * scale);
    [
        a,
        b,
        -b,
        a,
        centre.0 - a * centre.0 + b * centre.1,
        centre.1 - b * centre.0 - a * centre.1,
    ]
}

/// A rectangle of full coverage with a partial right edge, canvas-sized.
fn selection(x: u32, y: u32, w: u32, h: u32) -> Vec<u16> {
    (0..W * H)
        .map(|i| {
            let (px, py) = (i % W, i / W);
            if py < y || py >= y + h || px < x || px > x + w {
                0
            } else if px == x + w {
                30000
            } else {
                65535
            }
        })
        .collect()
}

/// The gray result `a` against the twin's `b` (module doc); frees both.
fn assert_twin(a: *mut PrecisionDocument, b: *mut PrecisionDocument, depth: u32, what: &str) {
    assert_eq!(descriptor(a)["color_mode"], "gray", "{what}");
    let (da, db) = unsafe { (&*a, &*b) };
    assert_eq!(da.dimensions(), db.dimensions(), "{what}: canvas");
    assert_eq!(da.layer_count(), db.layer_count(), "{what}: records");
    for i in 0..da.layer_count() {
        let pixels = layer(a, i, depth);
        assert_neutral(&pixels, what);
        assert_eq!(pixels, layer(b, i, depth), "{what}: record {i}");
    }
    free(a);
    free(b);
}

/// One edit on the gray document and its twin; both must change.
fn both(
    gray: *const PrecisionDocument,
    rgb: *const PrecisionDocument,
    request: &Value,
    input: &[u16],
    depth: u32,
) {
    let what = format!("{request} at {depth} bits");
    let a = edit(gray, request, input)
        .unwrap_or_else(|e| panic!("{what}: {e}"))
        .unwrap_or_else(|| panic!("{what}: no change"));
    let b = edit(rgb, request, input).unwrap().unwrap();
    assert_twin(a, b, depth, &what);
}

type Call = unsafe extern "C" fn(
    *const PrecisionDocument,
    *const c_char,
    u64,
    u64,
    *const DrJobControl,
    *mut *mut c_char,
) -> *mut PrecisionDocument;

fn call(f: Call, doc: *const PrecisionDocument, request: &Value) -> *mut PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { f(doc, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    out
}

fn warp_set() -> Value {
    let mut displacements = vec![[0.0, 0.0]; 9];
    displacements[4] = [2.0, 1.5];
    json!({"op":"set","layer":0,"target":"pixels","columns":3,"rows":3,
        "displacements":displacements,"filter":"bicubic"})
}

fn warp_info(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_warp_info(doc, 0, &mut err) };
    plan_json(out, err).unwrap()
}

/// A full-canvas mask of a horizontal ramp on layer 0.
fn masked(doc: *mut PrecisionDocument) -> *mut PrecisionDocument {
    let request = json!({"op":"set_mask","layer":0,"width":W,"height":H,"x":0,"y":0,
        "linked":true});
    let ramp: Vec<u16> = (0..W * H)
        .map(|i| ((i % W) * 65535 / (W - 1)) as u16)
        .collect();
    let out = edit(doc, &request, &ramp).unwrap().unwrap();
    free(doc);
    out
}

#[test]
fn every_geometry_edit_equals_the_twin_at_every_depth() {
    let turn9 = turn(9.0, 0.9, (12.0, 8.5));
    let quad = [2.0, 1.0, 22.0, 3.0, 20.0, 15.0, 3.0, 13.0];
    let requests = [
        json!({"op":"resize","width":31,"height":22,"filter":"bicubic"}),
        json!({"op":"resize","width":13,"height":9,"filter":"lanczos3"}),
        json!({"op":"crop","x":3,"y":2,"width":15,"height":11}),
        json!({"op":"canvas_size","width":30,"height":20,"x":4,"y":-2}),
        json!({"op":"straighten","matrix":turn9,"filter":"bilinear","x":2,"y":2,"width":18,
            "height":12}),
        json!({"op":"rotate_canvas","degrees":23.0,"filter":"bicubic"}),
        json!({"op":"rotate90"}),
        json!({"op":"rotate180"}),
        json!({"op":"rotate270"}),
        json!({"op":"flip_horizontal"}),
        json!({"op":"flip_vertical"}),
        json!({"op":"affine","layer":0,"target":"pixels","matrix":turn9,"filter":"lanczos3"}),
        json!({"op":"affine","layer":0,"target":"mask","matrix":turn9,"filter":"bilinear"}),
        json!({"op":"perspective","layer":0,"target":"pixels","quad":quad,"filter":"bicubic"}),
        json!({"op":"affine_set","layers":[0],"matrix":turn(-12.0, 1.1, (12.0, 8.0)),
            "filter":"bilinear"}),
        json!({"op":"perspective_set","layers":[0],"source_rect":[0.0,0.0,24.0,17.0],
            "quad":quad,"filter":"bilinear"}),
        json!({"op":"affine_set","layers":[0],"matrix":[1.0,0.0,0.0,1.0,-5.0,3.0],
            "filter":"nearest"}),
        json!({"op":"perspective_crop","quad":[1.0,1.0,21.0,2.0,20.0,15.0,2.0,14.0],
            "width":18,"height":12,"filter":"bilinear"}),
    ];
    for depth in DEPTHS {
        let gray = masked(gray_doc(W, H, depth));
        let rgb = twin(gray);
        for request in &requests {
            both(gray, rgb, request, &[], depth);
        }
        free(rgb);
        free(gray);
    }
}

#[test]
fn trim_keeps_the_twins_rectangle() {
    for depth in DEPTHS {
        let base = gray_doc(W, H, depth);
        // Transparent margins around the layer.
        let gray = edit(
            base,
            &json!({"op":"canvas_size","width":W + 6,"height":H + 4,"x":2,"y":3}),
            &[],
        )
        .unwrap()
        .unwrap();
        free(base);
        let rgb = twin(gray);
        let rect = |doc: *const PrecisionDocument| {
            let render = unsafe { &*doc }.render(LIMIT, 0, None).unwrap();
            let mut rect = [0u32; 4];
            assert!(unsafe { dr_precision_image_trim_rect(&render, 0, 15, rect.as_mut_ptr()) });
            rect
        };
        let kept = rect(gray);
        assert_eq!(kept, rect(rgb), "{depth} bits");
        assert_eq!(kept, [2, 3, W, H], "{depth} bits");
        let crop = json!({"op":"crop","x":kept[0],"y":kept[1],"width":kept[2],"height":kept[3]});
        both(gray, rgb, &crop, &[], depth);
        free(rgb);
        free(gray);
    }
}

#[test]
fn the_warps_and_liquify_equal_the_twin_and_keep_no_binding() {
    let puppet = json!({"op":"puppet_warp","layer":0,"pins":[
        {"from":[8.0,8.0],"to":[10.0,9.0]},{"from":[17.0,8.0],"to":[17.0,7.0]}]});
    let perspective = json!({"op":"perspective_warp","layer":0,"quads":[{
        "from":[3.0,2.0,21.0,2.0,21.0,15.0,3.0,15.0],
        "to":[4.0,3.0,22.0,1.0,20.0,16.0,2.0,14.0]}]});
    let liquify = json!({"layer":0,"op":"liquify","params":{"strokes":[
        {"tool":"forward_warp","size":10,"strength":0.8,"points":[[8.0,8.0],[14.0,9.0]]},
        {"tool":"twirl_clockwise","size":8,"points":[[17.0,6.0],[17.0,6.0]]}]}});
    for depth in DEPTHS {
        let gray = gray_doc(W, H, depth);
        let rgb = twin(gray);
        let warped = call(dr_precision_doc_warp_edit, gray, &warp_set());
        assert_eq!(warp_info(warped)["field"], Value::Null, "{depth} bits");
        assert_twin(
            warped,
            call(dr_precision_doc_warp_edit, rgb, &warp_set()),
            depth,
            "warp",
        );
        for (f, request) in [
            (dr_precision_doc_puppet_warp as Call, &puppet),
            (dr_precision_doc_perspective_warp as Call, &perspective),
        ] {
            assert_twin(
                call(f, gray, request),
                call(f, rgb, request),
                depth,
                &request.to_string(),
            );
        }
        let a = selected(gray, &liquify, &[]).unwrap().unwrap();
        let b = selected(rgb, &liquify, &[]).unwrap().unwrap();
        assert_twin(a, b, depth, "liquify");
        free(rgb);
        free(gray);
    }
}

#[test]
fn the_selected_pixels_equal_the_twin() {
    let coverage = selection(5, 4, 10, 8);
    let requests = [
        json!({"op":"move_selected","layer":0,"dx":4,"dy":3}),
        json!({"op":"move_selected","layer":0,"dx":-3,"dy":2,"duplicate":true}),
        json!({"op":"transform_selected","layer":0,"matrix":turn(13.0, 1.1, (10.0, 8.0)),
            "filter":"bilinear"}),
        json!({"op":"puppet_selected","layer":0,"pins":[
            {"from":[7.0,6.0],"to":[9.0,7.0]},{"from":[13.0,9.0],"to":[13.0,8.0]}]}),
        json!({"op":"perspective_selected","layer":0,"quads":[{
            "from":[5.0,4.0,15.0,4.0,15.0,12.0,5.0,12.0],
            "to":[6.0,5.0,16.0,3.0,15.0,13.0,4.0,11.0]}]}),
        json!({"op":"move_selected","layer":0,"dx":2,"dy":-1,"target":"mask"}),
    ];
    for depth in DEPTHS {
        let gray = masked(gray_doc(W, H, depth));
        let rgb = twin(gray);
        for request in &requests {
            assert_eq!(
                plan_samples(gray, request).unwrap(),
                coverage.len(),
                "{request}"
            );
            both(gray, rgb, request, &coverage, depth);
        }
        // A colour plane is no Gray target.
        let red = json!({"op":"move_selected","layer":0,"dx":2,"dy":1,"target":"red"});
        assert_eq!(edit(gray, &red, &coverage).unwrap_err(), PLANE);
        free(rgb);
        free(gray);
    }
}

/// `dr_precision_doc_select` of `operation` over the composite.
fn select(doc: *const PrecisionDocument, operation: &Value) -> Result<Vec<u16>, String> {
    let request = json!({"op":"selection","operation":operation}).to_string();
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let plan = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    if plan.is_null() {
        return Err(take_err_string(err));
    }
    let planned: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(plan) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(plan) };
    let mut out = vec![0u16; planned["output_samples"].as_u64().unwrap() as usize];
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
fn the_magic_wand_and_color_range_equal_the_twin() {
    let operations = [
        json!({"kind":"magic_wand","x":3,"y":2,"tolerance":40,"contiguous":true}),
        json!({"kind":"magic_wand","x":11,"y":9,"tolerance":25,"contiguous":false}),
        json!({"kind":"color_range","params":{"mode":"sampled",
            "samples":[[30000,30000,30000]],"tolerance":25,"softness":10}}),
        json!({"kind":"luminance"}),
    ];
    for depth in DEPTHS {
        let gray = gray_doc(W, H, depth);
        let rgb = twin(gray);
        for operation in &operations {
            let a = select(gray, operation).unwrap_or_else(|e| panic!("{operation}: {e}"));
            assert_eq!(
                a,
                select(rgb, operation).unwrap(),
                "{operation} at {depth} bits"
            );
            assert!(a.iter().any(|&c| c > 0), "{operation} selects something");
        }
        assert_eq!(
            select(gray, &json!({"kind":"color","channel":"red"})).unwrap_err(),
            PLANE
        );
        free(rgb);
        free(gray);
    }
}

#[test]
fn duplicate_to_document_is_a_gray_document_of_the_same_policy() {
    for depth in DEPTHS {
        let gray = gray_doc(W, H, depth);
        let out = edit(
            gray,
            &json!({"op":"duplicate_to_new_document","layers":[0]}),
            &[],
        )
        .unwrap()
        .unwrap();
        let (a, b) = (descriptor(out), descriptor(gray));
        assert_eq!(a["color_mode"], "gray");
        assert_eq!(a["gray"], b["gray"], "{depth} bits");
        assert_eq!(layer(out, 0, depth), layer(gray, 0, depth));
        free(out);
        free(gray);
    }
}
