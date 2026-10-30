//! #1 Lab phase 2f, Blend If on a Lab Color document and the channel
//! carries (`lab_style`, `lab_convert`), through the FFI. A Lab document's
//! Blend If channels are Lightness, a and b, stored as red, green and blue.
//! Oracles written here:
//!
//! - a hard green (a\*) ramp `this_layer [128, 128, 255, 255]` on a top
//!   layer of red and green patches over a gray one composites, hand-computed,
//!   to the top layer where its a\* code is at least 32896 + 257 and to the
//!   bottom where it is at most 32896 − 257, within 1 code;
//! - Gray refuses by name in `layer_blend_if` and `layer_style`, an identity
//!   Gray range included;
//! - RGB → Lab and back carry a red/green/blue Blend If unchanged, and a Gray
//!   one is named in the plan's refusal and the conversion; RGB → Lab and back
//!   carry a channel Levels and Curves record byte-equal.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_lab::{
    dr_precision_doc_convert_lab_to_rgb, dr_precision_doc_convert_to_lab,
    dr_precision_doc_convert_to_lab_plan,
};
use doctored_core::ffi_precision_inspect::dr_precision_doc_adjustment_description;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::gray_ops::{edit, free, plan_json, result, LIMIT};

const W: u32 = 16;
const H: u32 = 8;
const NEUTRAL: u16 = 32896;
const SENTENCE: &str = "Blend If on the Gray channel is unavailable in a Lab Color document: its Blend If channels are Lightness, a and b.";

/// Bottom: opaque mid gray. Top: opaque patches, red-ish (a* > 0) in even
/// 4-pixel columns, green-ish (a* < 0) in odd ones.
fn srgb_two(extra: Option<PrecisionDocumentLayer>) -> *mut PrecisionDocument {
    let n = (W * H) as usize;
    let bottom = vec![[30000, 30000, 30000, 65535]; n];
    let top: Vec<[u16; 4]> = (0..W * H)
        .map(|i| {
            let x = i % W;
            if (x / 4).is_multiple_of(2) {
                [52000, 12000 + x as u16 * 900, 15000, 65535]
            } else {
                [9000, 50000, 14000 + x as u16 * 700, 65535]
            }
        })
        .collect();
    let raster = |name: &str, pixels: Vec<[u16; 4]>| {
        let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
        PrecisionDocumentLayer::raster(name, image, (0, 0), 1.0, true).unwrap()
    };
    let mut layers = vec![raster("Bottom", bottom), raster("Top", top)];
    layers.extend(extra);
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap();
    Box::into_raw(Box::new(doc))
}

fn to_lab(doc: *const PrecisionDocument) -> Result<*mut PrecisionDocument, String> {
    let options = CString::new("{}").unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_lab(doc, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    result(out, err).map(|d| d.expect("to Lab answered nothing"))
}

fn to_lab_plan(doc: *const PrecisionDocument) -> Value {
    let options = CString::new("{}").unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_convert_to_lab_plan(doc, options.as_ptr(), &mut err) };
    plan_json(out, err).unwrap()
}

fn to_srgb(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let srgb = IccProfile::srgb();
    let icc = srgb.bytes();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_lab_to_rgb(
            doc,
            icc.as_ptr(),
            icc.len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err).unwrap().expect("to RGB answered nothing")
}

fn blend_if(
    doc: *const PrecisionDocument,
    layer: usize,
    value: Value,
) -> Result<*mut PrecisionDocument, String> {
    let request = json!({"op":"layer_blend_if","version":1,"layer":layer,"blend_if":value});
    edit(doc, &request, &[]).map(|d| d.expect("the Blend If changed nothing"))
}

fn codes(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(index)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

#[test]
fn a_hard_a_star_ramp_shows_the_top_layer_only_where_a_is_not_negative() {
    let rgb = srgb_two(None);
    let lab = to_lab(rgb).unwrap();
    let ramp = json!({"channel":"green","this_layer":[128,128,255,255],"underlying":[0,0,255,255]});
    let blended = blend_if(lab, 1, ramp).unwrap();
    let render = unsafe { &*blended }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap();
    let (bottom, top) = (codes(lab, 0), codes(lab, 1));
    let (mut shown, mut hidden) = (0, 0);
    for (i, got) in render.iter().enumerate() {
        let a = top[i][1];
        let want = if a >= NEUTRAL + 257 {
            shown += 1;
            top[i]
        } else if a <= NEUTRAL - 257 {
            hidden += 1;
            bottom[i]
        } else {
            continue;
        };
        for c in 0..4 {
            assert!(
                got[c].abs_diff(want[c]) <= 1,
                "pixel {i} channel {c}: {got:?} against {want:?}"
            );
        }
    }
    assert!(shown > 20 && hidden > 20, "{shown} shown, {hidden} hidden");
    for doc in [blended, lab, rgb] {
        free(doc);
    }
}

#[test]
fn gray_blend_if_refuses_by_name() {
    let rgb = srgb_two(None);
    let lab = to_lab(rgb).unwrap();
    for value in [
        json!({"channel":"gray","this_layer":[0,40,255,255]}),
        json!({"channel":"green","this_layer":[128,128,255,255],
            "extra":[{"channel":"gray","this_layer":[0,0,255,255],"underlying":[0,0,255,255]}]}),
    ] {
        assert_eq!(
            blend_if(lab, 1, value.clone()).unwrap_err(),
            SENTENCE,
            "{value}"
        );
        let style = json!({"op":"layer_style","version":1,"layer":1,"style":{"blend_if":value}});
        assert_eq!(
            edit(lab, &style, &[]).unwrap_err(),
            SENTENCE,
            "{value} style"
        );
    }
    // Lightness, a and b are admitted, as the layer's and a style's.
    for channel in ["red", "green", "blue"] {
        let value = json!({"channel":channel,"this_layer":[20,60,255,255]});
        free(blend_if(lab, 1, value.clone()).unwrap());
        let style = json!({"op":"layer_style","version":1,"layer":1,"style":{"blend_if":value}});
        free(edit(lab, &style, &[]).unwrap().unwrap());
    }
    free(lab);
    free(rgb);
}

#[test]
fn rgb_and_lab_carry_a_channel_blend_if_and_name_a_gray_one() {
    let rgb = srgb_two(None);
    let ramp = json!({"channel":"blue","this_layer":[10,40,200,250],"underlying":[0,0,255,255],
        "extra":[{"channel":"red","this_layer":[0,0,255,255],"underlying":[30,60,255,255]}]});
    let ramped = blend_if(rgb, 1, ramp).unwrap();
    let stored = unsafe { &*ramped }.layer(1).unwrap().blend_if();
    assert!(stored.is_some());
    assert!(to_lab_plan(ramped)["refusal"].is_null());
    let lab = to_lab(ramped).unwrap();
    assert_eq!(
        unsafe { &*lab }.layer(1).unwrap().blend_if(),
        stored,
        "RGB → Lab"
    );
    let back = to_srgb(lab);
    assert_eq!(
        unsafe { &*back }.layer(1).unwrap().blend_if(),
        stored,
        "Lab → RGB"
    );
    for doc in [back, lab, ramped] {
        free(doc);
    }
    let gray = blend_if(
        rgb,
        1,
        json!({"channel":"gray","this_layer":[0,40,255,255]}),
    )
    .unwrap();
    let sentence =
        format!("“Top” cannot convert to Lab Color: {SENTENCE} Flatten the image to convert it.");
    let plan = to_lab_plan(gray);
    assert_eq!(plan["refusal"], sentence.as_str(), "{plan}");
    assert_eq!(plan["flatten_required"], true);
    assert_eq!(to_lab(gray).unwrap_err(), sentence);
    free(gray);
    free(rgb);
}

fn description(doc: *const PrecisionDocument, index: usize) -> Option<String> {
    let mut err = ptr::null_mut();
    let out: *mut c_char = unsafe { dr_precision_doc_adjustment_description(doc, index, &mut err) };
    if out.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_string();
    unsafe { dr_string_free(out) };
    Some(text)
}

#[test]
fn channel_levels_and_curves_carry_byte_equal_both_ways() {
    let r_map: Vec<u32> = (0..256).map(|v| 255 - v).collect();
    for meta in [
        json!({"type":"adjust","evaluation":"precision_v1","op":"levels",
            "params":{"g":{"gamma":1.4},"b":{"black":0.1}}}),
        json!({"type":"adjust","evaluation":"precision_v1","op":"curves",
            "params":{"r_map":r_map}}),
    ] {
        let meta = meta.to_string();
        let layer =
            PrecisionDocumentLayer::adjustment("Channel", &meta, 1.0, true, &IccProfile::srgb())
                .unwrap();
        let rgb = srgb_two(Some(layer));
        assert!(to_lab_plan(rgb)["refusal"].is_null(), "{meta}");
        let lab = to_lab(rgb).unwrap();
        assert_eq!(
            description(lab, 2).as_deref(),
            Some(meta.as_str()),
            "RGB → Lab"
        );
        let back = to_srgb(lab);
        assert_eq!(
            description(back, 2).as_deref(),
            Some(meta.as_str()),
            "Lab → RGB"
        );
        for doc in [back, lab, rgb] {
            free(doc);
        }
    }
}
