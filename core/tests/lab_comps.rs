//! #547 Lab phase 6a-ii: layer comps, links and the snapshot round trip on
//! a Lab Color document (`lab_comps`), through the FFI. Bounds, declared
//! before measuring:
//!
//! * (a) a comp saved on a two-layer Lab document at 8 and 16 bits and
//!   applied after hiding, re-blending (Multiply), fading and moving a
//!   layer restores each layer's visibility, opacity, blend mode and offset
//!   and the composite EXACTLY; a second apply changes nothing; update,
//!   reorder and delete change the reported names as asked;
//! * (b) the `.rz` round trip (`dr_precision_doc_save_native` / `_open`) at
//!   8 and 16 bits keeps `is_lab`, the `lab` descriptor, every layer's
//!   samples and link, a mask, an alpha channel and the comps JSON;
//! * (c) `link_layers` and `unlink_layers` give the link values the RGB TWIN
//!   (`lab_geometry`'s `from_parts` document without the Lab policy) gives;
//! * (d) RGB → Lab carries the comps JSON unchanged; applying a comp that
//!   recorded a #0000ff drop shadow and Darken sets the layer's style JSON
//!   byte-equal to the style the same layer carries when it holds that
//!   shadow on RGB and is converted (the layer's own carried style), and
//!   keeps Normal (Darken is not a Lab mode); a comp whose recorded style
//!   holds a Gray Blend If is skipped whole, the style JSON unchanged; Lab →
//!   RGB keeps the comps JSON.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_comps::dr_precision_doc_layer_comps_json;
use doctored_core::ffi_lab::{
    dr_precision_doc_convert_lab_to_rgb, dr_precision_doc_convert_to_lab,
};
use doctored_core::ffi_precision::{
    dr_precision_doc_open, dr_precision_doc_render, dr_precision_doc_save_native,
    dr_precision_doc_set_layer_opacity, dr_precision_image_free,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::gray_ops::{edit, free, layer, result, LIMIT};
use crate::lab_geometry::{descriptor, document, pair, ramp, twin_profile, H, W};

const DEPTHS: [u32; 2] = [8, 16];
/// Blend mode wire codes (`BlendMode::to_c`).
const NORMAL: i32 = 0;
const MULTIPLY: i32 = 1;
const DARKEN: i32 = 6;

/// A two-layer Lab document at `bits` (no mask): the ramp Background and a
/// 14 × 10 Top at (5, 3), 80 %.
fn two_layer(bits: u32) -> *mut PrecisionDocument {
    let twin = twin_profile();
    let base = PrecisionImage::from_rgba16(W, H, ramp(W, H, bits, 0), twin.clone(), None).unwrap();
    let top = PrecisionImage::from_rgba16(14, 10, ramp(14, 10, bits, 5), twin, None).unwrap();
    let records = [
        Layer::raster("Background", base, (0, 0), 1.0, true).unwrap(),
        Layer::raster("Top", top, (5, 3), 0.8, true).unwrap(),
    ];
    document(&records, bits, true)
}

/// An edit that must change the document.
fn must(doc: *const PrecisionDocument, request: &Value) -> *mut PrecisionDocument {
    edit(doc, request, &[])
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request} changed nothing"))
}

/// An edit whose payload is `input`; it must change the document.
fn must_with(
    doc: *const PrecisionDocument,
    request: &Value,
    input: &[u16],
) -> *mut PrecisionDocument {
    edit(doc, request, input)
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

/// The composite's u16 codes through `dr_precision_doc_render`.
fn composite(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    let mut err = ptr::null_mut();
    let image = unsafe { dr_precision_doc_render(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!image.is_null(), "{}", take_err_string(err));
    let pixels = unsafe { &*image }.to_rgba16().unwrap();
    unsafe { dr_precision_image_free(image) };
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

fn links(doc: *const PrecisionDocument) -> Vec<u32> {
    let doc = unsafe { &*doc };
    (0..doc.layer_count())
        .map(|i| doc.layer(i).unwrap().link())
        .collect()
}

#[test]
fn a_lab_comp_round_trip_restores_the_document_exactly() {
    for bits in DEPTHS {
        let lab = two_layer(bits);
        let mut save = comp("save");
        save["name"] = json!("A");
        let saved = must(lab, &save);
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
            "{bits}: the edits moved layer 1"
        );
        assert_ne!(
            changed[1].3, before_props[1].3,
            "{bits}: the translate moved it"
        );
        let mut apply = comp("apply");
        apply["comp"] = json!(0);
        let applied = must(moved, &apply);
        assert!(unsafe { &*applied }.is_lab());
        assert_eq!(properties(applied), before_props, "{bits}");
        assert_eq!(composite(applied), before_render, "{bits}");
        assert!(
            edit(applied, &apply, &[]).unwrap().is_none(),
            "{bits}: a second apply is a no-op"
        );
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
        assert!(unsafe { &*deleted }.is_lab());
        assert_eq!(descriptor(deleted)["lab"]["depth"], bits);
        for doc in [
            deleted, reordered, renamed, two, applied, moved, faded, multiplied, hidden, saved, lab,
        ] {
            free(doc);
        }
    }
}

#[test]
fn a_snapshot_round_trip_keeps_the_lab_document() {
    let dir = TempDir::new().unwrap();
    // On the ×257 grid, so the 8-bit document holds them as they are.
    let mask: Vec<u16> = (0..W * H).map(|i| ((i * 37) % 256 * 257) as u16).collect();
    let channel: Vec<u16> = (0..W * H)
        .map(|i| (255 - (i * 11) % 256) as u16 * 257)
        .collect();
    for bits in DEPTHS {
        let lab = two_layer(bits);
        let masked = must_with(
            lab,
            &json!({"op":"set_mask","layer":0,"width":W,"height":H,"x":0,"y":0,"linked":true}),
            &mask,
        );
        let channelled = must_with(
            masked,
            &json!({"op":"set_channel","index":null,"name":"Alpha 1"}),
            &channel,
        );
        let linked = must(channelled, &json!({"op":"link_layers","layers":[0,1]}));
        let mut save = comp("save");
        save["name"] = json!("Snap");
        let doc = must(linked, &save);
        let file = dir.path().join(format!("snapshot-{bits}.rz"));
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
        assert!(b.is_lab(), "{bits}");
        assert_eq!(descriptor(back)["lab"], descriptor(doc)["lab"], "{bits}");
        assert_eq!(descriptor(back)["lab"]["depth"], bits, "{bits}");
        assert_eq!(b.layer_count(), d.layer_count());
        for i in 0..d.layer_count() {
            assert_eq!(
                layer(back, i, bits),
                layer(doc, i, bits),
                "{bits}: layer {i}"
            );
            assert_eq!(b.layer(i).unwrap().link(), d.layer(i).unwrap().link());
            assert_ne!(d.layer(i).unwrap().link(), 0, "{bits}: layer {i} is linked");
        }
        assert_eq!(
            b.layer(0).unwrap().mask().unwrap().plane().samples(),
            d.layer(0).unwrap().mask().unwrap().plane().samples(),
            "{bits}: mask"
        );
        assert_eq!(b.channels().len(), 1);
        assert_eq!(
            b.channels()[0].plane().samples(),
            d.channels()[0].plane().samples(),
            "{bits}: channel"
        );
        assert_eq!(comps(back), comps(doc), "{bits}: comps");
        assert_eq!(names(back), ["Snap"]);
        for doc in [back, doc, linked, channelled, masked, lab] {
            free(doc);
        }
    }
}

#[test]
fn lab_linking_gives_the_twins_link_values() {
    let (lab, rgb) = pair(16);
    let link = json!({"op":"link_layers","layers":[0,1]});
    let unlink = json!({"op":"unlink_layers","layers":[1]});
    let l1 = must(lab, &link);
    let r1 = must(rgb, &link);
    assert_eq!(links(l1), links(r1));
    assert!(links(l1).iter().all(|l| *l != 0));
    let l2 = must(l1, &unlink);
    let r2 = must(r1, &unlink);
    assert_eq!(links(l2), links(r2));
    assert!(unsafe { &*l2 }.is_lab());
    for doc in [l2, r2, l1, r1, rgb, lab] {
        free(doc);
    }
}

// ------------------------------------------------------------- (d) --

/// An sRGB layer of varied, opaque pixels.
fn srgb_layer(name: &str, seed: u32) -> Layer {
    let pixels: Vec<[u16; 4]> = (0..W * H)
        .map(|i| {
            let r = ((i * 37 + seed * 11) % 256) * 257;
            let g = ((i * 91 + seed * 53) % 256) * 257;
            let b = ((i * 13 + seed * 7) % 256) * 257;
            [r as u16, g as u16, b as u16, 65535]
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

fn srgb_doc() -> *mut PrecisionDocument {
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![srgb_layer("Base", 1), srgb_layer("Top", 2)],
    )
    .unwrap();
    Box::into_raw(Box::new(doc))
}

fn to_lab(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let options = CString::new(r#"{"depth":16}"#).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_lab(doc, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!out.is_null(), "to Lab: {}", take_err_string(err));
    out
}

fn to_rgb(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let icc = IccProfile::srgb().bytes().to_vec();
    let options = CString::new("{}").unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_lab_to_rgb(
            doc,
            icc.as_ptr(),
            icc.len(),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "to RGB: {}", take_err_string(err));
    out
}

fn shadow(color: &str) -> Value {
    json!({"op":"layer_style","version":1,"layer":1,"style":{"effects":[
        {"type":"drop_shadow","color":color,"distance":2,"size":1}]}})
}

fn style_of(doc: *const PrecisionDocument, index: usize) -> Option<String> {
    unsafe { &*doc }.layer(index).unwrap().reported_style()
}

#[test]
fn rgb_to_lab_carries_comps_and_apply_skips_what_lab_refuses() {
    let rgb = srgb_doc();
    let red = must(rgb, &shadow("#ff0000"));
    let blue = must(red, &shadow("#0000ff"));
    let darken = must(
        blue,
        &json!({"op":"layer_blend","version":1,"layer":1,"blend_mode":DARKEN}),
    );
    let appearance = |name: &str| {
        json!({"op":"layer_comp","action":"save","name":name,"visibility":false,
            "position":false,"appearance":true})
    };
    let one = must(darken, &appearance("Blue"));
    let normal = must(
        one,
        &json!({"op":"layer_blend","version":1,"layer":1,"blend_mode":NORMAL}),
    );
    let red_again = must(normal, &shadow("#ff0000"));
    let gray_if = must(
        red_again,
        &json!({"op":"layer_blend_if","version":1,"layer":1,
            "blend_if":{"channel":"gray","this_layer":[0,40,255,255]}}),
    );
    let two = must(gray_if, &appearance("GrayIf"));
    let reset = must(
        two,
        &json!({"op":"layer_blend_if","version":1,"layer":1,"blend_if":null}),
    );
    assert_eq!(names(reset), ["Blue", "GrayIf"]);
    let lab = to_lab(reset);
    assert!(unsafe { &*lab }.is_lab());
    assert_eq!(
        comps(lab),
        comps(reset),
        "RGB → Lab carries the comps unchanged"
    );
    // The oracle: the layer's own carried style, the #0000ff shadow set on
    // RGB and converted.
    let blue_rgb = must(reset, &shadow("#0000ff"));
    let blue_lab = to_lab(blue_rgb);
    let want = style_of(blue_lab, 1).expect("the carried style");
    let red_style = style_of(lab, 1);
    assert_ne!(red_style.as_deref(), Some(want.as_str()));
    let applied = must(lab, &json!({"op":"layer_comp","action":"apply","comp":0}));
    let top = unsafe { &*applied }.layer(1).unwrap();
    assert_eq!(top.blend_mode().to_c(), NORMAL, "Darken is skipped");
    assert_eq!(style_of(applied, 1).as_deref(), Some(want.as_str()));
    // The Gray Blend If comp is skipped whole: nothing it moves changes.
    match edit(
        lab,
        &json!({"op":"layer_comp","action":"apply","comp":1}),
        &[],
    )
    .unwrap()
    {
        None => {}
        Some(out) => {
            assert_eq!(
                style_of(out, 1),
                red_style,
                "the Gray Blend If style is skipped"
            );
            free(out);
        }
    }
    let back = to_rgb(lab);
    assert_eq!(comps(back), comps(lab), "Lab → RGB keeps the comps");
    for doc in [
        back, applied, blue_lab, blue_rgb, lab, reset, two, gray_if, red_again, normal, one,
        darken, blue, red, rgb,
    ] {
        free(doc);
    }
}
