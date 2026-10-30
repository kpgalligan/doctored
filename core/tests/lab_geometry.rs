//! #547 Lab phases 4a and 4a-ii: the resamples, transforms, warps,
//! Liquify, Align and Distribute and the Move tool on a Lab Color document,
//! through the FFI. Every one is the RGB kernel run on the L\*a\*b\* codes,
//! so the oracle is the RGB TWIN: the identical `from_parts` document
//! without the Lab policy (the same codes in the same twin profile, read as
//! RGB) running the same op. Bounds, declared before measuring:
//!
//! * (a) every Lab result stays a Lab document of the same depth, with the
//!   twin's canvas and record count, and every record's u16 samples, offset
//!   and mask equal the twin's bit for bit, at 8 and 16 bits;
//! * (b) a neutral layer (a\* = b\* = 0, the 0x8080 code) stays neutral
//!   through every op at every filter it takes: every pixel with alpha
//!   65535 keeps G = B = 0x8080 exactly, and under nearest and bilinear (and
//!   the filterless ops) every pixel with alpha > 0 does. A negative-lobe
//!   filter's partial-alpha edge pixel is covered by (a) only, because
//!   cancellation there can move the unpremultiplied value for RGB too;
//! * (c) a crop past the canvas pads the Background with the sRGB colour
//!   converted by Mode ▸ Lab Color's rule: within 0.1 L\* and 0.1 a\*/b\* of
//!   the hand-computed `srgb_to_lab` at 16 bits, within 0.6 L\* and 1.0
//!   a\*/b\* with every code on the ×257 grid at 8; the old canvas keeps its
//!   codes exactly and the record stays opaque;
//! * (d) the descriptor lists the 12 phase 4a families, the three 4a-ii
//!   warp families and 4c's four clipboard families, and Duplicate to New
//!   Document is admitted (4b-i's selected pixels are `lab_selection.rs`'s;
//!   the clipboard's pixels are `lab_clipboard.rs`'s);
//! * (e) Warp (a layer's pixels and a mask), Puppet Warp, Perspective Warp
//!   and Liquify equal the twin's result bit for bit as (a) says, at 8 and
//!   16 bits, and every Lab warp keeps no binding (`warp_info` field null);
//! * (f) Align, Distribute and the Move tool's integer `affine_set` on a
//!   three-layer document change it and equal the twin bit for bit as (a)
//!   says, at 8 and 16 bits;
//! * (g) a neutral layer stays neutral through every op of (e): every pixel
//!   with alpha 65535 keeps G = B = 0x8080 exactly, and under the bilinear
//!   warp every pixel with alpha > 0 does. Align, Distribute and the Move
//!   tool are integer translations, covered bit for bit by (f);
//! * (h) a small Lab canvas's view runs under a job limited to the
//!   document's `display_scratch_bytes`.

use std::ffi::{c_char, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::ffi_jobs::{
    dr_job_control_create, dr_job_control_free, dr_job_control_set_scratch_limit,
};
use doctored_core::ffi_lab::{dr_precision_doc_new_lab, dr_precision_image_lab_display};
use doctored_core::ffi_perspective_warp::dr_precision_doc_perspective_warp;
use doctored_core::ffi_precision::{dr_precision_doc_descriptor, dr_precision_image_free};
use doctored_core::ffi_puppet::dr_precision_doc_puppet_warp;
use doctored_core::ffi_warp::{dr_precision_doc_warp_edit, dr_precision_doc_warp_info};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::gray_ops::{edit, free, layer, plan_json, selected, LIMIT};
use crate::lab_fixture::{decode, srgb_to_lab};

pub(crate) const W: u32 = 24;
pub(crate) const H: u32 = 17;
pub(crate) const DEPTHS: [u32; 2] = [8, 16];
pub(crate) const NEUTRAL: u32 = 0x8080;
const FILTERS: [&str; 4] = ["nearest", "bilinear", "bicubic", "lanczos3"];

pub(crate) fn descriptor(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_descriptor(doc, &mut err) };
    plan_json(out, err).unwrap()
}

/// A new Lab document of `fill_srgb` at `bits`.
fn new_lab(w: u32, h: u32, bits: u32, fill: [f64; 4]) -> *mut PrecisionDocument {
    let request = CString::new(json!({"depth":bits,"fill_srgb":fill}).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_new_lab(w, h, request.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

/// The Lab twin profile, from a new Lab document.
pub(crate) fn twin_profile() -> Arc<IccProfile> {
    let doc = new_lab(1, 1, 16, [1.0, 1.0, 1.0, 1.0]);
    let profile = unsafe { &*doc }.profile().clone();
    free(doc);
    profile
}

/// One code: `byte` on the ×257 grid at 8 bits, nudged off it at 16.
fn code(byte: u32, nudge: u32, bits: u32) -> u16 {
    let wide = byte.min(255) * 257;
    if bits == 8 {
        wide as u16
    } else {
        wide.saturating_sub(nudge % 250) as u16
    }
}

/// A coloured ramp: L\* across, a\* down, b\* diagonal, every seventh pixel
/// at partial alpha.
pub(crate) fn ramp(w: u32, h: u32, bits: u32, seed: u32) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let l = x * 255 / (w - 1);
            let a = 30 + y * 190 / (h - 1);
            let b = 30 + (x * 11 + y * 29 + seed) % 200;
            let alpha = if (i + seed) % 7 == 3 { 156 } else { 255 };
            let n = i * 37 + seed;
            [
                code(l, n, bits),
                code(a, n + 11, bits),
                code(b, n + 23, bits),
                if bits == 8 || alpha == 255 {
                    (alpha * 257) as u16
                } else {
                    40_000
                },
            ]
        })
        .collect()
}

/// The records' document: Lab at `bits` (`lab` true) or its RGB twin.
pub(crate) fn document(records: &[Layer], bits: u32, lab: bool) -> *mut PrecisionDocument {
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        records[0].source().unwrap().profile().clone(),
        Metadata::default(),
        Resolution::default(),
        records.to_vec(),
    )
    .unwrap();
    let doc = if lab {
        doc.with_lab_depth(bits).unwrap()
    } else {
        doc
    };
    Box::into_raw(Box::new(doc))
}

/// `doc` with layer 1's canvas-sized ramp mask; frees `doc`.
fn masked(doc: *mut PrecisionDocument) -> *mut PrecisionDocument {
    let request = json!({"op":"set_mask","layer":1,"width":W,"height":H,"x":0,"y":0,
        "linked":true});
    let ramp: Vec<u16> = (0..W * H)
        .map(|i| ((i % W) * 65535 / (W - 1)) as u16)
        .collect();
    let out = edit(doc, &request, &ramp).unwrap().unwrap();
    free(doc);
    out
}

/// The two-layer Lab document at `bits` and its RGB twin: layer 0 the full
/// canvas, layer 1 offset at 80 % with a ramp mask.
pub(crate) fn pair(bits: u32) -> (*mut PrecisionDocument, *mut PrecisionDocument) {
    let twin = twin_profile();
    let base = PrecisionImage::from_rgba16(W, H, ramp(W, H, bits, 0), twin.clone(), None).unwrap();
    let top = PrecisionImage::from_rgba16(14, 10, ramp(14, 10, bits, 5), twin, None).unwrap();
    let records = [
        Layer::raster("Background", base, (0, 0), 1.0, true).unwrap(),
        Layer::raster("Top", top, (5, 3), 0.8, true).unwrap(),
    ];
    (
        masked(document(&records, bits, true)),
        masked(document(&records, bits, false)),
    )
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

const QUAD: [f64; 8] = [2.0, 1.0, 22.0, 3.0, 20.0, 15.0, 3.0, 13.0];

/// Every 4a op that takes a filter, at `filter`; `masks` adds the mask
/// target (a document whose layer 1 has a mask).
fn filtered(filter: &str, masks: bool) -> Vec<Value> {
    let turn9 = turn(9.0, 0.9, (12.0, 8.5));
    let set = if masks { json!([0, 1]) } else { json!([0]) };
    let mut out = vec![
        json!({"op":"resize","width":31,"height":22,"filter":filter}),
        json!({"op":"resize","width":13,"height":9,"filter":filter}),
        json!({"op":"straighten","matrix":turn9,"filter":filter,"x":2,"y":2,"width":18,
            "height":12}),
        json!({"op":"rotate_canvas","degrees":23.0,"filter":filter}),
        json!({"op":"affine","layer":0,"target":"pixels","matrix":turn9,"filter":filter}),
        json!({"op":"perspective","layer":0,"target":"pixels","quad":QUAD,"filter":filter}),
        json!({"op":"affine_set","layers":set,"matrix":turn(-12.0, 1.1, (12.0, 8.0)),
            "filter":filter}),
        json!({"op":"perspective_set","layers":[0],"source_rect":[0.0,0.0,24.0,17.0],
            "quad":QUAD,"filter":filter}),
        json!({"op":"perspective_crop","quad":[1.0,1.0,21.0,2.0,20.0,15.0,2.0,14.0],
            "width":18,"height":12,"filter":filter}),
    ];
    if masks {
        out.push(
            json!({"op":"affine","layer":1,"target":"mask","matrix":turn9,
            "filter":filter}),
        );
        out.push(
            json!({"op":"perspective","layer":1,"target":"pixels","quad":QUAD,
            "filter":filter}),
        );
    }
    out
}

/// The 4a ops that take no filter, and the integer translation of a set.
fn exact(masks: bool) -> Vec<Value> {
    let set = if masks { json!([0, 1]) } else { json!([0]) };
    vec![
        json!({"op":"crop","x":3,"y":2,"width":15,"height":11}),
        json!({"op":"canvas_size","width":30,"height":20,"x":4,"y":-2}),
        json!({"op":"rotate90"}),
        json!({"op":"rotate180"}),
        json!({"op":"rotate270"}),
        json!({"op":"flip_horizontal"}),
        json!({"op":"flip_vertical"}),
        json!({"op":"affine_set","layers":set,"matrix":[1.0,0.0,0.0,1.0,-5.0,3.0],
            "filter":"nearest"}),
    ]
}

/// One edit; it must change the document.
fn must(doc: *const PrecisionDocument, request: &Value, what: &str) -> *mut PrecisionDocument {
    edit(doc, request, &[])
        .unwrap_or_else(|e| panic!("{what}: {e}"))
        .unwrap_or_else(|| panic!("{what}: no change"))
}

/// The Lab result `a` against the twin's `b` (module doc (a)); frees both.
pub(crate) fn assert_twin(
    a: *mut PrecisionDocument,
    b: *mut PrecisionDocument,
    bits: u32,
    what: &str,
) {
    let d = descriptor(a);
    assert_eq!(d["color_mode"], "lab", "{what}");
    assert_eq!(d["lab"]["depth"], bits, "{what}");
    let (da, db) = unsafe { (&*a, &*b) };
    assert_eq!(da.dimensions(), db.dimensions(), "{what}: canvas");
    assert_eq!(da.layer_count(), db.layer_count(), "{what}: records");
    for i in 0..da.layer_count() {
        assert_eq!(layer(a, i, bits), layer(b, i, bits), "{what}: record {i}");
        let (la, lb) = (da.layer(i).unwrap(), db.layer(i).unwrap());
        assert_eq!(la.offset(), lb.offset(), "{what}: record {i} offset");
        match (la.mask(), lb.mask()) {
            (None, None) => {}
            (Some(ma), Some(mb)) => {
                assert_eq!(
                    ma.plane().samples(),
                    mb.plane().samples(),
                    "{what}: record {i} mask"
                );
                assert_eq!(ma.properties().offset, mb.properties().offset, "{what}");
            }
            _ => panic!("{what}: record {i} mask presence differs"),
        }
    }
    free(a);
    free(b);
}

#[test]
fn lab_geometry_equals_the_rgb_kernel_on_the_codes() {
    for bits in DEPTHS {
        let (lab, rgb) = pair(bits);
        let mut requests = exact(true);
        requests.extend(filtered("bicubic", true));
        requests.push(json!({"op":"resize","width":31,"height":22,"filter":"lanczos3"}));
        requests.push(json!({"op":"affine","layer":0,"target":"pixels",
            "matrix":turn(9.0, 0.9, (12.0, 8.5)),"filter":"lanczos3"}));
        requests.push(json!({"op":"affine","layer":1,"target":"mask",
            "matrix":turn(9.0, 0.9, (12.0, 8.5)),"filter":"bilinear"}));
        requests.push(json!({"op":"affine_set","layers":[0, 1],
            "matrix":turn(-12.0, 1.1, (12.0, 8.0)),"filter":"bilinear"}));
        for request in &requests {
            let what = format!("{request} at {bits} bits");
            let a = must(lab, request, &what);
            let b = must(rgb, request, &what);
            assert_twin(a, b, bits, &what);
        }
        free(rgb);
        free(lab);
    }
}

/// An opaque full-canvas layer: an L\* ramp at a\* = b\* = 0.
fn neutral(bits: u32) -> *mut PrecisionDocument {
    let pixels = (0..W * H)
        .map(|i| {
            [
                code((i % W) * 255 / (W - 1), i * 37, bits),
                0x8080,
                0x8080,
                65535,
            ]
        })
        .collect();
    let image = PrecisionImage::from_rgba16(W, H, pixels, twin_profile(), None).unwrap();
    let record = Layer::raster("Background", image, (0, 0), 1.0, true).unwrap();
    document(&[record], bits, true)
}

fn assert_neutral(doc: *mut PrecisionDocument, bits: u32, every_alpha: bool, what: &str) {
    assert_eq!(descriptor(doc)["color_mode"], "lab", "{what}");
    for i in 0..unsafe { &*doc }.layer_count() {
        for (k, p) in layer(doc, i, bits).iter().enumerate() {
            if p[3] == 65535 || (every_alpha && p[3] > 0) {
                assert!(
                    p[1] == NEUTRAL && p[2] == NEUTRAL,
                    "{what}: record {i} pixel {k}: {p:?}"
                );
            }
        }
    }
    free(doc);
}

#[test]
fn a_neutral_lab_layer_stays_neutral() {
    for bits in DEPTHS {
        let doc = neutral(bits);
        for request in exact(false) {
            let what = format!("{request} at {bits} bits");
            assert_neutral(must(doc, &request, &what), bits, true, &what);
        }
        for filter in FILTERS {
            let every_alpha = matches!(filter, "nearest" | "bilinear");
            for request in filtered(filter, false) {
                let what = format!("{request} at {bits} bits");
                assert_neutral(must(doc, &request, &what), bits, every_alpha, &what);
            }
        }
        free(doc);
    }
}

#[test]
fn crop_past_the_canvas_pads_the_background_in_lab() {
    let color = [0.2, 0.4, 0.8];
    let want = srgb_to_lab(color);
    for bits in DEPTHS {
        let doc = new_lab(W, H, bits, [1.0, 1.0, 1.0, 1.0]);
        let white = layer(doc, 0, bits)[0];
        let request = json!({"op":"straighten","matrix":[1.0,0.0,0.0,1.0,0.0,0.0],
            "filter":"nearest","x":-3,"y":-2,"width":W + 6,"height":H + 4,
            "delete_cropped":true,
            "background":{"color":color,"color_space":"srgb"}});
        let what = format!("{request} at {bits} bits");
        let out = must(doc, &request, &what);
        let d = descriptor(out);
        assert_eq!(d["color_mode"], "lab", "{what}");
        assert_eq!(d["lab"]["depth"], bits, "{what}");
        let record = unsafe { &*out }.layer(0).unwrap();
        assert_eq!(record.offset(), (0, 0), "{what}");
        let pixels = layer(out, 0, bits);
        assert_eq!(pixels.len() as u32, (W + 6) * (H + 4), "{what}");
        let (bound_l, bound_ab) = if bits == 16 { (0.1, 0.1) } else { (0.6, 1.0) };
        for (k, p) in pixels.iter().enumerate() {
            let (i, j) = (k as u32 % (W + 6), k as u32 / (W + 6));
            assert_eq!(p[3], 65535, "{what}: pixel ({i},{j}) opaque");
            let old = (3..W + 3).contains(&i) && (2..H + 2).contains(&j);
            if old {
                assert_eq!(*p, white, "{what}: old pixel ({i},{j})");
                continue;
            }
            let sample = p.map(|v| v as u16);
            let got = decode(sample);
            assert!(
                (got[0] - want[0]).abs() <= bound_l
                    && (got[1] - want[1]).abs() <= bound_ab
                    && (got[2] - want[2]).abs() <= bound_ab,
                "{what}: pad ({i},{j}) {got:?} against {want:?}"
            );
            if bits == 8 {
                assert!(sample[..3].iter().all(|c| c % 257 == 0), "{what}: {p:?}");
            }
        }
        free(out);
        free(doc);
    }
}

#[test]
fn the_gate_lifts_4a_and_4c() {
    let doc = neutral(16);
    let operations = descriptor(doc)["supported_operations"].clone();
    for family in [
        "geometry_affine",
        "geometry_perspective",
        "geometry_affine_set",
        "geometry_perspective_set",
        "geometry_resize",
        "geometry_crop",
        "geometry_straighten",
        "geometry_rotate_canvas",
        "geometry_canvas_size",
        "geometry_rotate",
        "geometry_flip",
        "mask_transform",
        "geometry_warp",
        "geometry_puppet",
        "geometry_perspective_warp",
        "clipboard_copy",
        "clipboard_cut",
        "clipboard_copy_merged",
        "clipboard_paste",
    ] {
        assert!(
            operations.as_array().unwrap().contains(&family.into()),
            "{family}: {operations}"
        );
    }
    let out = edit(
        doc,
        &json!({"op":"duplicate_to_new_document","layers":[0]}),
        &[],
    )
    .unwrap()
    .unwrap();
    assert_eq!(descriptor(out)["color_mode"], "lab");
    free(out);
    free(doc);
}

type Call = unsafe extern "C" fn(
    *const PrecisionDocument,
    *const c_char,
    u64,
    u64,
    *const DrJobControl,
    *mut *mut c_char,
) -> *mut PrecisionDocument;

/// One warp-family export call; it must answer a document.
fn call(f: Call, doc: *const PrecisionDocument, request: &Value) -> *mut PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { f(doc, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    out
}

/// Layer ▸ Warp's set: 3×3, the centre node moved (2, 1.5).
fn warp_set(layer: usize, target: &str, filter: &str) -> Value {
    let mut displacements = vec![[0.0, 0.0]; 9];
    displacements[4] = [2.0, 1.5];
    json!({"op":"set","layer":layer,"target":target,"columns":3,"rows":3,
        "displacements":displacements,"filter":filter})
}

fn warp_info(doc: *const PrecisionDocument, layer: usize) -> Value {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_warp_info(doc, layer, &mut err) };
    plan_json(out, err).unwrap()
}

fn puppet() -> Value {
    json!({"op":"puppet_warp","layer":0,"pins":[
        {"from":[8.0,8.0],"to":[10.0,9.0]},{"from":[17.0,8.0],"to":[17.0,7.0]}]})
}

fn perspective() -> Value {
    json!({"op":"perspective_warp","layer":0,"quads":[{
        "from":[3.0,2.0,21.0,2.0,21.0,15.0,3.0,15.0],
        "to":[4.0,3.0,22.0,1.0,20.0,16.0,2.0,14.0]}]})
}

fn liquify() -> Value {
    json!({"layer":0,"op":"liquify","params":{"strokes":[
        {"tool":"forward_warp","size":10,"strength":0.8,"points":[[8.0,8.0],[14.0,9.0]]},
        {"tool":"twirl_clockwise","size":8,"points":[[17.0,6.0],[17.0,6.0]]}]}})
}

/// Liquify through the selected edit; it must change the document.
fn liquified(doc: *const PrecisionDocument, what: &str) -> *mut PrecisionDocument {
    selected(doc, &liquify(), &[])
        .unwrap_or_else(|e| panic!("{what}: {e}"))
        .unwrap_or_else(|| panic!("{what}: no change"))
}

#[test]
fn lab_warps_equal_the_rgb_kernel_on_the_codes() {
    for bits in DEPTHS {
        let (lab, rgb) = pair(bits);
        for (layer, target) in [(0, "pixels"), (1, "mask")] {
            let request = warp_set(layer, target, "bicubic");
            let what = format!("{request} at {bits} bits");
            let a = call(dr_precision_doc_warp_edit, lab, &request);
            assert_eq!(warp_info(a, layer)["field"], Value::Null, "{what}");
            let b = call(dr_precision_doc_warp_edit, rgb, &request);
            assert_twin(a, b, bits, &what);
        }
        for (f, request) in [
            (dr_precision_doc_puppet_warp as Call, puppet()),
            (dr_precision_doc_perspective_warp as Call, perspective()),
        ] {
            let what = format!("{request} at {bits} bits");
            let a = call(f, lab, &request);
            assert_eq!(warp_info(a, 0)["field"], Value::Null, "{what}");
            assert_twin(a, call(f, rgb, &request), bits, &what);
        }
        let what = format!("liquify at {bits} bits");
        assert_twin(liquified(lab, &what), liquified(rgb, &what), bits, &what);
        free(rgb);
        free(lab);
    }
}

/// The three-layer Lab document at `bits` and its RGB twin: `pair`'s
/// records (layer 1 with its ramp mask) plus a 10×8 ramp at (14, 6).
fn trio(bits: u32) -> (*mut PrecisionDocument, *mut PrecisionDocument) {
    let twin = twin_profile();
    let base = PrecisionImage::from_rgba16(W, H, ramp(W, H, bits, 0), twin.clone(), None).unwrap();
    let top =
        PrecisionImage::from_rgba16(14, 10, ramp(14, 10, bits, 5), twin.clone(), None).unwrap();
    let third = PrecisionImage::from_rgba16(10, 8, ramp(10, 8, bits, 9), twin, None).unwrap();
    let records = [
        Layer::raster("Background", base, (0, 0), 1.0, true).unwrap(),
        Layer::raster("Top", top, (5, 3), 0.8, true).unwrap(),
        Layer::raster("Third", third, (14, 6), 1.0, true).unwrap(),
    ];
    (
        masked(document(&records, bits, true)),
        masked(document(&records, bits, false)),
    )
}

#[test]
fn lab_align_distribute_and_move_equal_the_twin() {
    let requests = [
        json!({"op":"align_set","layers":[0,1,2],"edge":"left","to_canvas":false}),
        json!({"op":"align_set","layers":[1,2],"edge":"bottom","to_canvas":true}),
        json!({"op":"distribute_set","layers":[0,1,2],"vertical":false}),
        json!({"op":"affine_set","layers":[1,2],"matrix":[1.0,0.0,0.0,1.0,3.0,-2.0],
            "filter":"nearest"}),
    ];
    for bits in DEPTHS {
        let (lab, rgb) = trio(bits);
        for request in &requests {
            let what = format!("{request} at {bits} bits");
            let a = must(lab, request, &what);
            let b = must(rgb, request, &what);
            assert_twin(a, b, bits, &what);
        }
        free(rgb);
        free(lab);
    }
}

#[test]
fn warps_and_alignment_keep_a_neutral_lab_layer_neutral() {
    for bits in DEPTHS {
        let doc = neutral(bits);
        for filter in ["bicubic", "bilinear"] {
            let request = warp_set(0, "pixels", filter);
            let what = format!("{request} at {bits} bits");
            let out = call(dr_precision_doc_warp_edit, doc, &request);
            assert_neutral(out, bits, filter == "bilinear", &what);
        }
        for (f, request) in [
            (dr_precision_doc_puppet_warp as Call, puppet()),
            (dr_precision_doc_perspective_warp as Call, perspective()),
        ] {
            let what = format!("{request} at {bits} bits");
            assert_neutral(call(f, doc, &request), bits, false, &what);
        }
        let what = format!("liquify at {bits} bits");
        assert_neutral(liquified(doc, &what), bits, false, &what);
        free(doc);
    }
}

/// (h) A small Lab canvas's view fits its own display plan: the host bounds
/// each display stage by `display_scratch_bytes` (`PreparedDocumentEdit`),
/// so the view (`dr_precision_image_lab_display`) must run under a job
/// limited to it. Before 4a-ii the plan was the preview's alone and the
/// view's tables were refused on any canvas this small, which refused Warp
/// and Puppet Warp on it.
#[test]
fn a_small_lab_canvas_view_fits_its_display_plan() {
    for bits in DEPTHS {
        let doc = neutral(bits);
        let plan = descriptor(doc)["display_scratch_bytes"].as_u64().unwrap();
        let image = unsafe { &*doc }.layer(0).unwrap().source().unwrap() as *const _;
        let control = unsafe { dr_job_control_create() };
        assert!(unsafe { dr_job_control_set_scratch_limit(control, plan) });
        let mut err = ptr::null_mut();
        let view = unsafe { dr_precision_image_lab_display(image, LIMIT, 0, control, &mut err) };
        assert!(
            !view.is_null(),
            "{bits} bits, plan {plan}: {}",
            take_err_string(err)
        );
        unsafe {
            dr_precision_image_free(view);
            dr_job_control_free(control);
        }
        free(doc);
    }
}
