//! #547 Lab phase 2e, layer effects on a Lab Color document, through the
//! FFI. A Color Overlay of sRGB #ff0000 on an opaque layer composites to the
//! Lab codes of sRGB red: each rendered code ÷ 257 within 1 of the 8-bit
//! codes of the hand-computed CIE L\*a\*b\* (`lab_fixture::srgb_to_lab`:
//! 255·L\*/100, a\* + 128, b\* + 128); a Darken drop shadow refuses and a
//! Gray Blend If refuses by the Lab Blend If list (phase 2f);
//! RGB → Lab carries a Multiply drop shadow (its style JSON byte-equal) and
//! Lab → RGB carries it back. Patterns on Lab are `lab_pattern_stroke.rs`'s.
//! Bounds declared before measuring.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_lab::{
    dr_precision_doc_convert_lab_to_rgb, dr_precision_doc_convert_to_lab_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::gray_ops::{edit, free, plan_json, result, LIMIT};
use crate::lab_fixture::srgb_to_lab;
use crate::lab_targets::{pattern, srgb_doc, to_lab};

fn styled(doc: *const PrecisionDocument, style: Value) -> Result<*mut PrecisionDocument, String> {
    let request = json!({"op":"layer_style","version":1,"layer":0,"style":style});
    edit(doc, &request, &[]).map(|d| d.expect("the style changed nothing"))
}

fn effect(effect: Value) -> Value {
    json!({ "effects": [effect] })
}

fn lab_doc() -> *mut PrecisionDocument {
    let rgb = srgb_doc(pattern());
    let lab = to_lab(rgb, r#"{"depth":16}"#).unwrap();
    free(rgb);
    lab
}

fn style_json(doc: *const PrecisionDocument) -> Option<String> {
    unsafe { &*doc }.layer(0).unwrap().reported_style()
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

fn to_lab_plan(doc: *const PrecisionDocument) -> Value {
    let options = CString::new("{}").unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_convert_to_lab_plan(doc, options.as_ptr(), &mut err) };
    plan_json(out, err).unwrap()
}

#[test]
fn a_red_color_overlay_composites_to_the_lab_codes_of_srgb_red() {
    let lab = lab_doc();
    let overlaid = styled(
        lab,
        effect(json!({"type":"color_overlay","color":"#ff0000"})),
    )
    .unwrap();
    let red = srgb_to_lab([1.0, 0.0, 0.0]);
    let want = [255.0 * red[0] / 100.0, red[1] + 128.0, red[2] + 128.0];
    let render = unsafe { &*overlaid }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap();
    for (i, p) in render.iter().enumerate() {
        for c in 0..3 {
            let got = f64::from(p[c]) / 257.0;
            assert!(
                (got - want[c]).abs() <= 1.0,
                "pixel {i} channel {c}: {got} against {}",
                want[c]
            );
        }
    }
    free(overlaid);
    free(lab);
}

#[test]
fn a_darken_shadow_and_blend_if_refuse_by_name() {
    let lab = lab_doc();
    for (style, sentence) in [
        (
            effect(json!({"type":"drop_shadow","blend":"darken","distance":3,"size":2})),
            "The Darken blend mode is unavailable in a Lab Color document.",
        ),
        (
            json!({"blend_if":{"channel":"gray"}}),
            "Blend If on the Gray channel is unavailable in a Lab Color document: its Blend If channels are Lightness, a and b.",
        ),
    ] {
        assert_eq!(styled(lab, style.clone()).unwrap_err(), sentence, "{style}");
    }
    free(lab);
}

#[test]
fn a_multiply_drop_shadow_carries_both_ways() {
    let rgb = srgb_doc(pattern());
    let shadowed = styled(
        rgb,
        effect(json!({"type":"drop_shadow","blend":"multiply","distance":3,"size":2})),
    )
    .unwrap();
    let style = style_json(shadowed);
    assert!(style.is_some());
    let plan = to_lab_plan(shadowed);
    assert!(plan["refusal"].is_null(), "{plan}");
    let lab = to_lab(shadowed, "{}").unwrap();
    assert!(unsafe { &*lab }.is_lab());
    assert_eq!(style_json(lab), style, "RGB → Lab");
    let back = to_srgb(lab);
    assert!(!unsafe { &*back }.is_lab());
    assert_eq!(style_json(back), style, "Lab → RGB");
    for doc in [back, lab, shadowed] {
        free(doc);
    }
    free(rgb);
}
