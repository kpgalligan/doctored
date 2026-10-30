//! #547 phase 6a-ii, layer comps, links and the snapshot round trip on a
//! Grayscale document (`gray_comps`), through the FFI. A comp saved on a Gray
//! document and applied after hiding, re-blending, fading and moving a layer
//! restores each layer's visibility, opacity, blend mode and offset and the
//! composite EXACTLY (16 and 32 bits); a second apply changes nothing; update,
//! reorder and delete change the reported names list as asked. The `.rz`
//! round trip (8, 16, 32) keeps the mode, depth, profile name, every layer's
//! samples and link, a mask, an alpha channel and the comps. Linking and
//! unlinking give the link values the RGB TWIN gives. RGB → Gray carries a
//! comp whose recorded drop shadow is red: its colour becomes the gray hex
//! of red's luminance (sRGB's published row `SRGB_Y_ROW[0]`, IEC-encoded,
//! within 1 code), and applying it on the Gray document skips the recorded
//! Hue (the layer keeps Normal) and sets the greyed shadow. Bounds declared
//! before measuring.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_comps::dr_precision_doc_layer_comps_json;
use doctored_core::ffi_gray::dr_precision_doc_convert_to_gray;
use doctored_core::ffi_precision::{
    dr_precision_doc_descriptor, dr_precision_doc_open, dr_precision_doc_render,
    dr_precision_doc_save_native, dr_precision_doc_set_layer_opacity,
};
use doctored_core::ffi_precision_conversion::dr_precision_doc_promote_to_float;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit_plan;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::gray_fixture::{srgb_encode, SRGB_Y_ROW};
use crate::gray_ops::{edit, free, layer, plan_json, result, twin, LIMIT};

const W: u32 = 8;
const H: u32 = 6;
/// Blend mode wire codes (`BlendMode::to_c`).
const NORMAL: i32 = 0;
const MULTIPLY: i32 = 1;
const HUE: i32 = 23;

/// An sRGB layer of coloured, varied pixels.
fn coloured_layer(name: &str, seed: u32) -> Layer {
    let pixels: Vec<[u16; 4]> = (0..W * H)
        .map(|i| {
            let r = ((i * 37 + seed * 11) % 256) * 257;
            let g = ((i * 91 + seed * 53) % 256) * 257;
            let b = ((i * 13 + seed * 7) % 256) * 257;
            let a = if i % 5 == 2 { 40_000 } else { 65_535 };
            [r as u16, g as u16, b as u16, a]
        })
        .collect();
    Layer::raster(
        name,
        PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap(),
        (0, 0),
        1.0,
        true,
    )
    .unwrap()
}

/// A two-layer sRGB 16-bit document.
fn rgb_doc() -> *mut PrecisionDocument {
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![coloured_layer("Base", 1), coloured_layer("Top", 2)],
    )
    .unwrap();
    Box::into_raw(Box::new(doc))
}

fn to_gray(doc: *const PrecisionDocument, options: &str) -> *mut PrecisionDocument {
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_gray(
            doc,
            ptr::null(),
            0,
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "to gray: {}", take_err_string(err));
    out
}

/// The two-layer document as Gray Gamma 2.2 at `depth`.
fn two_layer_gray(depth: u32) -> *mut PrecisionDocument {
    let rgb = rgb_doc();
    let gray = if depth == 32 {
        let mut err = ptr::null_mut();
        let float =
            unsafe { dr_precision_doc_promote_to_float(rgb, LIMIT, 0, ptr::null(), &mut err) };
        assert!(!float.is_null(), "{}", take_err_string(err));
        let gray = to_gray(float, r#"{"space":"gray_gamma_22"}"#);
        free(float);
        gray
    } else {
        to_gray(
            rgb,
            &format!(r#"{{"space":"gray_gamma_22","depth":{depth}}}"#),
        )
    };
    free(rgb);
    assert!(unsafe { &*gray }.is_gray());
    gray
}

/// An edit that must change the document.
fn must(doc: *const PrecisionDocument, request: &Value) -> *mut PrecisionDocument {
    edit(doc, request, &[])
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request} changed nothing"))
}

/// An edit whose payload is the plan's `input_samples`, sample `i` = `f(i)`.
fn must_with(
    doc: *const PrecisionDocument,
    request: &Value,
    f: impl Fn(usize) -> u16,
) -> *mut PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let plan = plan_json(
        unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) },
        err,
    )
    .unwrap_or_else(|e| panic!("{request}: {e}"));
    let n = plan["input_samples"].as_u64().unwrap() as usize;
    let input: Vec<u16> = (0..n).map(f).collect();
    edit(doc, request, &input)
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request} changed nothing"))
}

fn opacity(doc: *const PrecisionDocument, index: usize, value: f32) -> *mut PrecisionDocument {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_set_layer_opacity(doc, index, value, &mut err) };
    result(out, err).unwrap().expect("the opacity changed")
}

fn comps(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_layer_comps_json(doc, &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn names(doc: *const PrecisionDocument) -> Vec<String> {
    comps(doc)["comps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_string())
        .collect()
}

fn descriptor(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_descriptor(doc, &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

/// The composite through `dr_precision_doc_render`: u16 codes, or the f32
/// bits of a float document's.
fn composite(doc: *const PrecisionDocument) -> Vec<[u32; 4]> {
    let mut err = ptr::null_mut();
    let image = unsafe { dr_precision_doc_render(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!image.is_null(), "{}", take_err_string(err));
    let pixels: Vec<[u32; 4]> = match unsafe { &*image }.to_rgba16() {
        Ok(pixels) => pixels.iter().map(|p| p.map(u32::from)).collect(),
        Err(_) => unsafe { &*image }
            .to_rgba32()
            .unwrap()
            .iter()
            .map(|p| p.map(f32::to_bits))
            .collect(),
    };
    unsafe { doctored_core::ffi_precision::dr_precision_image_free(image) };
    pixels
}

/// Each layer's visibility, opacity, blend mode and offset.
fn properties(doc: *const PrecisionDocument) -> Vec<(bool, f32, i32, (i32, i32))> {
    let doc = unsafe { &*doc };
    (0..doc.layer_count())
        .map(|i| {
            let l = doc.layer(i).unwrap();
            (l.visible(), l.opacity(), l.blend_mode().to_c(), l.offset())
        })
        .collect()
}

fn comp(action: &str) -> Value {
    json!({"op":"layer_comp","action":action})
}

#[test]
fn a_comp_round_trip_restores_the_document_exactly() {
    for depth in [16, 32] {
        let gray = two_layer_gray(depth);
        let mut save = comp("save");
        save["name"] = json!("A");
        let saved = must(gray, &save);
        let before_props = properties(saved);
        let before_render = composite(saved);
        let hidden = must(
            saved,
            &json!({"op":"layer_visible","layer":1,"visible":false}),
        );
        let multiplied = must(
            hidden,
            &json!({"op":"layer_blend","version":1,"layer":1,"blend_mode":MULTIPLY}),
        );
        let faded = opacity(multiplied, 1, 0.5);
        let moved = must(
            faded,
            &json!({"op":"affine","layer":1,"target":"pixels","matrix":[1,0,0,1,2,1],
                "filter":"nearest"}),
        );
        let changed = properties(moved);
        assert_ne!(
            changed[1], before_props[1],
            "{depth}: the edits moved layer 1"
        );
        assert_ne!(
            changed[1].3, before_props[1].3,
            "{depth}: the translate moved it"
        );
        let mut apply = comp("apply");
        apply["comp"] = json!(0);
        let applied = must(moved, &apply);
        assert!(unsafe { &*applied }.is_gray());
        assert_eq!(properties(applied), before_props, "{depth}");
        assert_eq!(composite(applied), before_render, "{depth}");
        assert!(
            edit(applied, &apply, &[]).unwrap().is_none(),
            "{depth}: a second apply is a no-op"
        );
        // Update (rename), reorder and delete.
        let mut second = comp("save");
        second["name"] = json!("B");
        let two = must(applied, &second);
        assert_eq!(names(two), ["A", "B"]);
        let renamed = must(
            two,
            &json!({"op":"layer_comp","action":"update","comp":0,"name":"A2"}),
        );
        assert_eq!(names(renamed), ["A2", "B"]);
        let reordered = must(
            renamed,
            &json!({"op":"layer_comp","action":"reorder","comp":1,"to":0}),
        );
        assert_eq!(names(reordered), ["B", "A2"]);
        let deleted = must(
            reordered,
            &json!({"op":"layer_comp","action":"delete","comp":0}),
        );
        assert_eq!(names(deleted), ["A2"]);
        assert!(unsafe { &*deleted }.is_gray());
        for doc in [
            deleted, reordered, renamed, two, applied, moved, faded, multiplied, hidden, saved,
            gray,
        ] {
            free(doc);
        }
    }
}

#[test]
fn a_snapshot_round_trip_keeps_the_gray_document() {
    let dir = TempDir::new().unwrap();
    for depth in [8u32, 16, 32] {
        let gray = two_layer_gray(depth);
        let masked = must_with(
            gray,
            &json!({"op":"set_mask","layer":0,"width":W,"height":H,"x":0,"y":0,"linked":true}),
            |i| (i as u16).wrapping_mul(1361),
        );
        let channelled = must_with(
            masked,
            &json!({"op":"set_channel","index":null,"name":"Alpha 1"}),
            |i| 65535 - (i as u16).wrapping_mul(977),
        );
        let linked = must(channelled, &json!({"op":"link_layers","layers":[0,1]}));
        let mut save = comp("save");
        save["name"] = json!("Snap");
        let doc = must(linked, &save);
        let file = dir.path().join(format!("snapshot-{depth}.rz"));
        let c = cpath(&file);
        let mut err = ptr::null_mut();
        assert!(
            unsafe {
                dr_precision_doc_save_native(doc, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
            },
            "{}",
            take_err_string(err)
        );
        let back = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
        assert!(!back.is_null(), "{}", take_err_string(err));
        let (d, b) = (unsafe { &*doc }, unsafe { &*back });
        assert!(b.is_gray(), "{depth}");
        assert_eq!(descriptor(back)["gray"], descriptor(doc)["gray"], "{depth}");
        assert_eq!(descriptor(back)["gray"]["depth"], depth, "{depth}");
        assert_eq!(b.layer_count(), d.layer_count());
        for i in 0..d.layer_count() {
            assert_eq!(
                layer(back, i, depth),
                layer(doc, i, depth),
                "{depth}: layer {i}"
            );
            assert_eq!(b.layer(i).unwrap().link(), d.layer(i).unwrap().link());
            assert_ne!(
                d.layer(i).unwrap().link(),
                0,
                "{depth}: layer {i} is linked"
            );
        }
        assert_eq!(
            b.layer(0).unwrap().mask().unwrap().plane().samples(),
            d.layer(0).unwrap().mask().unwrap().plane().samples(),
            "{depth}: mask"
        );
        assert_eq!(b.channels().len(), 1);
        assert_eq!(
            b.channels()[0].plane().samples(),
            d.channels()[0].plane().samples(),
            "{depth}: channel"
        );
        assert_eq!(comps(back), comps(doc), "{depth}: comps");
        assert_eq!(names(back), ["Snap"]);
        for doc in [back, doc, linked, channelled, masked, gray] {
            free(doc);
        }
    }
}

#[test]
fn linking_gives_the_twins_link_values() {
    let gray = two_layer_gray(16);
    let rgb = twin(gray);
    let links = |doc: *const PrecisionDocument| -> Vec<u32> {
        let doc = unsafe { &*doc };
        (0..doc.layer_count())
            .map(|i| doc.layer(i).unwrap().link())
            .collect()
    };
    let link = json!({"op":"link_layers","layers":[0,1]});
    let unlink = json!({"op":"unlink_layers","layers":[1]});
    let g1 = must(gray, &link);
    let r1 = must(rgb, &link);
    assert_eq!(links(g1), links(r1));
    assert!(links(g1).iter().all(|l| *l != 0));
    let g2 = must(g1, &unlink);
    let r2 = must(r1, &unlink);
    assert_eq!(links(g2), links(r2));
    assert!(unsafe { &*g2 }.is_gray());
    for doc in [g2, r2, g1, r1, rgb, gray] {
        free(doc);
    }
}

/// An sRGB hex's three bytes.
fn hex(text: &str) -> [u8; 3] {
    let digits = text.trim_start_matches('#');
    std::array::from_fn(|k| u8::from_str_radix(&digits[2 * k..2 * k + 2], 16).unwrap())
}

/// The drop shadow colour of a style JSON string.
fn shadow(style: &str) -> [u8; 3] {
    let value: Value = serde_json::from_str(style).unwrap();
    let effect = value["effects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["type"] == "drop_shadow")
        .unwrap();
    hex(effect["color"].as_str().unwrap())
}

#[test]
fn rgb_to_gray_carries_a_comp_with_its_style_greyed() {
    let rgb = rgb_doc();
    let styled = must(
        rgb,
        &json!({"op":"layer_style","version":1,"layer":1,"style":{"effects":[
            {"type":"drop_shadow","color":"#ff0000","distance":2,"size":1}]}}),
    );
    let hue = must(
        styled,
        &json!({"op":"layer_blend","version":1,"layer":1,"blend_mode":HUE}),
    );
    let saved = must(
        hue,
        &json!({"op":"layer_comp","action":"save","name":"Red","visibility":false,
            "position":false,"appearance":true}),
    );
    let normal = must(
        saved,
        &json!({"op":"layer_blend","version":1,"layer":1,"blend_mode":NORMAL}),
    );
    let plain = must(
        normal,
        &json!({"op":"layer_style","version":1,"layer":1,"style":null}),
    );
    let gray = to_gray(plain, r#"{"space":"gray_gamma_22","depth":16}"#);
    let listed = comps(gray);
    assert_eq!(names(gray), ["Red"]);
    let entries = listed["comps"][0]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    for entry in entries {
        assert_eq!(entry["stale"], false, "{entry}");
        assert!(entry["layer"].is_u64(), "{entry}");
    }
    let top = entries.iter().find(|e| e["layer"] == 1).unwrap();
    assert_eq!(top["blend"], HUE);
    let recorded = shadow(top["style"].as_str().unwrap());
    let want = (255.0 * srgb_encode(SRGB_Y_ROW[0])).round();
    assert!(
        recorded[0] == recorded[1] && recorded[1] == recorded[2],
        "{recorded:?}"
    );
    assert!(
        (f64::from(recorded[0]) - want).abs() <= 1.0,
        "{recorded:?} against {want}"
    );
    let applied = must(gray, &json!({"op":"layer_comp","action":"apply","comp":0}));
    let layer1 = unsafe { &*applied }.layer(1).unwrap();
    assert_eq!(layer1.blend_mode().to_c(), NORMAL, "the Hue is skipped");
    assert_eq!(shadow(&layer1.reported_style().unwrap()), recorded);
    assert!(unsafe { &*applied }.is_gray());
    for doc in [applied, gray, plain, normal, saved, hue, styled, rgb] {
        free(doc);
    }
}
