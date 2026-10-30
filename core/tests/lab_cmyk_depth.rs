//! #1 Lab phase 5a: Image ▸ Mode ▸ CMYK Color from a Lab Color document
//! and 8/16 Bits/Channel on one, through the FFI.
//!
//! Lab → CMYK runs the twin into 16-bit ProPhoto RGB (relative colorimetric
//! with BPC, never narrowed), then RGB → CMYK. The oracles are independent
//! of that composition: the two existing exports run one after the other,
//! and a ProPhoto document written here from the CIE formulas
//! (`common/lab_fixture.rs`'s `lab_to_prophoto`) through RGB → CMYK. The
//! depth oracle is the 8-bit grid `(v + 128) / 257 · 257`, written here.
//!
//! Bounds, declared before measuring:
//!
//! * (a) a Lab document at 8 or 16 bits → CMYK at depth 8 or 16 equals,
//!   sample for sample on every leaf, `dr_precision_doc_convert_lab_to_rgb`
//!   into ProPhoto RGB (`{"intent":"relative","black_point_compensation":
//!   true}`) followed by `dr_precision_doc_convert_to_cmyk` into the same
//!   press under the same options; a mask, a group, clipping and an alpha
//!   channel carry;
//! * (b) every patch's ink is within 1/255 (257 codes) of RGB → CMYK of the
//!   ProPhoto document built from `lab_to_prophoto(decode(code))`;
//! * (c) an adjustment layer and a Blend If on Lightness refuse by name with
//!   `flatten_required`, the Blend If sentence naming "Lightness"; `flatten:
//!   true` converts to one record;
//! * (d) 16 → 8 puts every raster, alpha, mask and alpha-channel sample on
//!   the grid and declares depth 8; 8 → 16 keeps every sample and declares
//!   16; the same depth is NULL with no error and a plan with `changes`
//!   false; bits 32 and a non-Lab document refuse with the exact sentences;
//! * (e) 16 → 8 → 16 is the grid of the source and 8 → 16 → 8 the identity.

use std::ffi::{c_char, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::ffi_cmyk::{
    dr_precision_doc_convert_to_cmyk, dr_precision_doc_convert_to_cmyk_plan,
};
use doctored_core::ffi_lab::{
    dr_precision_doc_convert_lab_to_rgb, dr_precision_doc_lab_depth_edit,
    dr_precision_doc_lab_depth_plan, dr_precision_doc_new_lab,
};
use doctored_core::ffi_precision::dr_precision_doc_descriptor;
use doctored_core::ffi_precision_color::dr_precision_doc_add_adjustment;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::cmyk_fixture::press_profile;
use crate::common::take_err_string;
use crate::gray_ops::{edit, free, plan_json, result, LIMIT};
use crate::lab_fixture::{decode, lab_to_prophoto};

const W: u32 = 8;
const H: u32 = 3;

/// L\*, a\* and b\* as 8-bit Lab codes (L\* · 255/100, a\* + 128, b\* + 128),
/// moderate colours well inside ProPhoto RGB, one neutral.
const PATCHES: [[u32; 3]; 8] = [
    [128, 128, 128],
    [200, 150, 100],
    [90, 100, 170],
    [160, 180, 190],
    [60, 128, 128],
    [230, 120, 140],
    [140, 90, 60],
    [30, 140, 110],
];

const DEPTH_REFUSAL: &str = "A Lab Color document is 8 or 16 bits per channel.";
const NOT_LAB: &str = "the document is not Lab Color";

/// The 8-bit grid (module doc), written here.
fn on_grid(v: u16) -> u16 {
    ((u32::from(v) + 128) / 257 * 257) as u16
}

fn descriptor(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    plan_json(unsafe { dr_precision_doc_descriptor(doc, &mut err) }, err).unwrap()
}

/// The Lab twin profile, from a new Lab document.
fn twin() -> Arc<IccProfile> {
    let request = CString::new(r#"{"depth":16,"fill_srgb":[1,1,1,1]}"#).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_new_lab(1, 1, request.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    let profile = unsafe { &*doc }.profile().clone();
    free(doc);
    profile
}

/// One pixel of layer `seed`: a patch's codes (each byte ×257, nudged off
/// the grid at 16 bits), every fifth pixel at partial alpha.
fn pixel(i: u32, seed: u32, bits: u32) -> [u16; 4] {
    let patch = PATCHES[((i + seed) % PATCHES.len() as u32) as usize];
    let nudge = |c: u32| -> u16 {
        let wide = (patch[c as usize] * 257) as i64;
        if bits == 8 {
            wide as u16
        } else {
            (wide + ((i * 37 + seed * 11 + c * 53) % 120) as i64 - 60).clamp(0, 65535) as u16
        }
    };
    let alpha = if (i + seed) % 5 == 2 {
        if bits == 8 {
            160 * 257
        } else {
            40_123
        }
    } else {
        65535
    };
    [nudge(0), nudge(1), nudge(2), alpha]
}

fn lab_layer(name: &str, seed: u32, bits: u32, profile: &Arc<IccProfile>) -> Layer {
    let pixels: Vec<[u16; 4]> = (0..W * H).map(|i| pixel(i, seed, bits)).collect();
    let image = PrecisionImage::from_rgba16(W, H, pixels, profile.clone(), None).unwrap();
    Layer::raster(name, image, (0, 0), 1.0, true).unwrap()
}

/// A Lab document of `names.len()` layers at `bits`.
fn lab_doc(bits: u32, names: &[&str]) -> *mut PrecisionDocument {
    let profile = twin();
    let layers = names
        .iter()
        .enumerate()
        .map(|(i, name)| lab_layer(name, i as u32 * 3, bits, &profile))
        .collect();
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        profile,
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
    .with_lab_depth(bits)
    .unwrap();
    Box::into_raw(Box::new(doc))
}

/// `doc` with an off-grid mask on layer `layer` and one off-grid alpha
/// channel; frees `doc`.
fn with_planes(doc: *mut PrecisionDocument, layer: usize) -> *mut PrecisionDocument {
    let mask: Vec<u16> = (0..W * H)
        .map(|i| ((i * 977 + 13) % 65536) as u16)
        .collect();
    let masked = edit(
        doc,
        &json!({"op":"set_mask","layer":layer,"width":W,"height":H,"x":0,"y":0,"linked":true}),
        &mask,
    )
    .unwrap()
    .unwrap();
    let channel: Vec<u16> = (0..W * H)
        .map(|i| ((i * 4099 + 7) % 65536) as u16)
        .collect();
    let out = edit(
        masked,
        &json!({"op":"set_channel","index":null,"name":"Alpha 1"}),
        &channel,
    )
    .unwrap()
    .unwrap();
    free(masked);
    free(doc);
    out
}

fn changed(doc: *const PrecisionDocument, request: &Value) -> *mut PrecisionDocument {
    edit(doc, request, &[])
        .unwrap()
        .expect("the edit changes the document")
}

fn to_cmyk(
    doc: *const PrecisionDocument,
    press: &[u8],
    options: &Value,
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(options.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
            doc,
            press.as_ptr(),
            press.len(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err).map(|d| d.expect("a CMYK conversion always yields a document"))
}

fn cmyk_plan(doc: *const PrecisionDocument, press: &[u8], options: &Value) -> Value {
    let text = CString::new(options.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk_plan(
            doc,
            press.as_ptr(),
            press.len(),
            text.as_ptr(),
            &mut err,
        )
    };
    plan_json(out, err).unwrap()
}

/// The two-step oracle's first step: Lab → 16-bit ProPhoto RGB under the
/// stage's options.
fn to_prophoto(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let icc = IccProfile::prophoto_rgb();
    let options = CString::new(r#"{"intent":"relative","black_point_compensation":true}"#).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_lab_to_rgb(
            doc,
            icc.bytes().as_ptr(),
            icc.bytes().len(),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "to ProPhoto: {}", take_err_string(err));
    out
}

fn inks(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 5]> {
    unsafe { &*doc }
        .cmyk_layer_samples(index)
        .expect("a CMYK record")
}

fn codes(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(index)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn mask_samples(doc: *const PrecisionDocument, index: usize) -> Vec<u16> {
    let layer = unsafe { &*doc }.layer(index).unwrap();
    layer.mask().unwrap().plane().samples().to_vec()
}

fn channel_samples(doc: *const PrecisionDocument) -> Vec<u16> {
    unsafe { &*doc }.channels()[0].plane().samples().to_vec()
}

fn depth_plan(doc: *const PrecisionDocument, bits: u32) -> Result<Value, String> {
    let mut err = ptr::null_mut();
    plan_json(
        unsafe { dr_precision_doc_lab_depth_plan(doc, bits, &mut err) },
        err,
    )
}

fn depth_edit(
    doc: *const PrecisionDocument,
    bits: u32,
) -> Result<Option<*mut PrecisionDocument>, String> {
    let mut err: *mut c_char = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_lab_depth_edit(doc, bits, LIMIT, 0, ptr::null(), &mut err) };
    result(out, err)
}

fn depth_changed(doc: *const PrecisionDocument, bits: u32) -> *mut PrecisionDocument {
    depth_edit(doc, bits)
        .unwrap()
        .expect("a depth change changes the document")
}

#[test]
fn lab_to_cmyk_equals_lab_to_prophoto_then_cmyk_and_carries_the_tree() {
    let press = press_profile("Press Test");
    for bits in [8, 16] {
        // A mask on Source, Top clipped to it, both grouped, one channel.
        let lab = with_planes(lab_doc(bits, &["Background", "Source", "Top"]), 1);
        let clipped = changed(lab, &json!({"op":"layer_clipped","layer":2,"clipped":true}));
        let grouped = changed(
            clipped,
            &json!({"op":"group_layers","layers":[1,2],"name":"Group","pass_through":false}),
        );
        let stage = to_prophoto(grouped);
        for depth in [8, 16] {
            let options = json!({ "depth": depth });
            let plan = cmyk_plan(grouped, &press, &options);
            assert_eq!(plan["refusal"], Value::Null, "Lab {bits} → CMYK {depth}");
            assert_eq!(plan["flatten_required"], false);
            assert!(
                plan["working_memory_plan_bytes"].as_u64().unwrap()
                    > cmyk_plan(stage, &press, &options)["working_memory_plan_bytes"]
                        .as_u64()
                        .unwrap(),
                "the Lab plan adds the stage"
            );
            let direct = to_cmyk(grouped, &press, &options).unwrap();
            let oracle = to_cmyk(stage, &press, &options).unwrap();
            let d = descriptor(direct);
            assert_eq!(d["color_mode"], "cmyk");
            assert_eq!(d["layer_count"], descriptor(grouped)["layer_count"]);
            for index in 0..3 {
                assert_eq!(
                    inks(direct, index),
                    inks(oracle, index),
                    "Lab {bits} → CMYK {depth}, leaf {index}"
                );
            }
            let doc = unsafe { &*direct };
            assert_eq!(doc.root_layers().len(), 2);
            assert_eq!(doc.root_layers()[1].children().unwrap().len(), 2);
            assert!(doc.layer(2).unwrap().clipped());
            assert_eq!(mask_samples(direct, 1), mask_samples(grouped, 1));
            assert_eq!(channel_samples(direct), channel_samples(grouped));
            free(oracle);
            free(direct);
        }
        for doc in [stage, grouped, clipped, lab] {
            free(doc);
        }
    }
}

#[test]
fn lab_to_cmyk_lands_on_the_cie_prophoto_oracle() {
    let press = press_profile("Press Test");
    for bits in [8, 16] {
        let lab = lab_doc(bits, &["Background"]);
        let source = codes(lab, 0);
        // The oracle document: each pixel's L*a*b* through the CIE formulas
        // into encoded ProPhoto RGB, alpha kept.
        let pixels: Vec<[u16; 4]> = source
            .iter()
            .map(|p| {
                let rgb = lab_to_prophoto(decode(*p));
                let c = |v: f64| (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
                [c(rgb[0]), c(rgb[1]), c(rgb[2]), p[3]]
            })
            .collect();
        let image =
            PrecisionImage::from_rgba16(W, H, pixels, IccProfile::prophoto_rgb(), None).unwrap();
        let rgb = Box::into_raw(Box::new(
            PrecisionDocument::from_image(image, Metadata::default(), Resolution::default())
                .unwrap(),
        ));
        let options = json!({"depth":16});
        let direct = to_cmyk(lab, &press, &options).unwrap();
        let oracle = to_cmyk(rgb, &press, &options).unwrap();
        for (i, (got, want)) in inks(direct, 0).iter().zip(inks(oracle, 0)).enumerate() {
            for c in 0..5 {
                let delta = (i32::from(got[c]) - i32::from(want[c])).abs();
                assert!(
                    delta <= 257,
                    "Lab {bits}, pixel {i}, component {c}: {} against {}",
                    got[c],
                    want[c]
                );
            }
        }
        for doc in [oracle, direct, rgb, lab] {
            free(doc);
        }
    }
}

#[test]
fn an_adjustment_layer_and_a_lightness_blend_if_refuse_by_name_and_flatten_converts() {
    let press = press_profile("Press Test");
    let lab = lab_doc(16, &["Background", "Source"]);
    let name = CString::new("Adjustment").unwrap();
    let meta = CString::new(
        json!({"type":"adjust","evaluation":"precision_v1","op":"levels","params":{"gamma":1.2}})
            .to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let adjusted = unsafe {
        dr_precision_doc_add_adjustment(
            lab,
            name.as_ptr(),
            meta.as_ptr(),
            1.0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    let adjusted = result(adjusted, err).unwrap().unwrap();
    let blend_if = changed(
        lab,
        &json!({"op":"layer_blend_if","version":1,"layer":1,
            "blend_if":{"channel":"red","this_layer":[20,60,255,255]}}),
    );
    let cases = [
        (
            adjusted,
            "“Adjustment” is an adjustment layer, which a CMYK document cannot hold yet; \
             flatten the image to convert it",
        ),
        (
            blend_if,
            "“Source” is a layer with Blend If on the Lightness channel, which a CMYK \
             document cannot hold yet; flatten the image to convert it",
        ),
    ];
    for (doc, sentence) in cases {
        let plan = cmyk_plan(doc, &press, &json!({}));
        assert_eq!(plan["refusal"], sentence);
        assert_eq!(plan["flatten_required"], true);
        assert_eq!(plan["flattens"], false);
        assert_eq!(to_cmyk(doc, &press, &json!({})).unwrap_err(), sentence);
        let flattening = cmyk_plan(doc, &press, &json!({"flatten":true}));
        assert_eq!(flattening["refusal"], Value::Null);
        assert_eq!(flattening["flattens"], true);
        let flat = to_cmyk(doc, &press, &json!({"flatten":true})).unwrap();
        assert_eq!(descriptor(flat)["color_mode"], "cmyk");
        assert_eq!(unsafe { &*flat }.root_layers().len(), 1);
        free(flat);
    }
    for doc in [blend_if, adjusted, lab] {
        free(doc);
    }
}

#[test]
fn bits_per_channel_rounds_to_the_grid_and_relabels() {
    let sixteen = with_planes(lab_doc(16, &["Background", "Top"]), 0);
    let plan = depth_plan(sixteen, 8).unwrap();
    assert_eq!(plan["refusal"], Value::Null);
    assert_eq!(plan["changes"], true);
    assert!(plan["working_memory_plan_bytes"].as_u64().unwrap() > 0);
    let eight = depth_changed(sixteen, 8);
    let d = descriptor(eight);
    assert_eq!(d["color_mode"], "lab");
    assert_eq!(d["lab"]["depth"], 8);
    for index in 0..2 {
        let want: Vec<[u16; 4]> = codes(sixteen, index)
            .iter()
            .map(|p| p.map(on_grid))
            .collect();
        assert_eq!(codes(eight, index), want, "layer {index}");
    }
    let grid = |v: Vec<u16>| -> Vec<u16> { v.into_iter().map(on_grid).collect() };
    assert_eq!(mask_samples(eight, 0), grid(mask_samples(sixteen, 0)));
    assert_eq!(channel_samples(eight), grid(channel_samples(sixteen)));
    // 8 → 16 relabels and keeps every sample.
    let back = depth_changed(eight, 16);
    assert_eq!(descriptor(back)["lab"]["depth"], 16);
    for index in 0..2 {
        assert_eq!(codes(back, index), codes(eight, index));
    }
    assert_eq!(mask_samples(back, 0), mask_samples(eight, 0));
    assert_eq!(channel_samples(back), channel_samples(eight));
    // The same depth changes nothing.
    for (doc, bits) in [(sixteen, 16), (eight, 8)] {
        let plan = depth_plan(doc, bits).unwrap();
        assert_eq!(plan["changes"], false);
        assert_eq!(plan["refusal"], Value::Null);
        assert!(depth_edit(doc, bits).unwrap().is_none());
    }
    // 32 bits and a non-Lab document refuse.
    assert_eq!(depth_plan(sixteen, 32).unwrap()["refusal"], DEPTH_REFUSAL);
    assert_eq!(depth_edit(sixteen, 32).unwrap_err(), DEPTH_REFUSAL);
    let rgb = to_prophoto(sixteen);
    assert_eq!(depth_plan(rgb, 8).unwrap()["refusal"], NOT_LAB);
    assert_eq!(depth_edit(rgb, 8).unwrap_err(), NOT_LAB);
    for doc in [rgb, back, eight, sixteen] {
        free(doc);
    }
}

#[test]
fn depth_round_trips_are_the_grid_and_the_identity() {
    let sixteen = with_planes(lab_doc(16, &["Background"]), 0);
    let down = depth_changed(sixteen, 8);
    let up = depth_changed(down, 16);
    let grid: Vec<[u16; 4]> = codes(sixteen, 0).iter().map(|p| p.map(on_grid)).collect();
    assert_eq!(codes(up, 0), grid);
    // 8 → 16 → 8 from an 8-bit document whose every plane is on the grid
    // (`down`), and from one built at 8 bits.
    let narrow = depth_changed(up, 8);
    assert_eq!(codes(narrow, 0), codes(down, 0));
    assert_eq!(mask_samples(narrow, 0), mask_samples(down, 0));
    assert_eq!(channel_samples(narrow), channel_samples(down));
    assert_eq!(descriptor(narrow)["lab"]["depth"], 8);
    let eight = lab_doc(8, &["Background"]);
    let wide = depth_changed(eight, 16);
    let again = depth_changed(wide, 8);
    assert_eq!(codes(again, 0), codes(eight, 0));
    for doc in [again, wide, eight, narrow, up, down, sixteen] {
        free(doc);
    }
}
