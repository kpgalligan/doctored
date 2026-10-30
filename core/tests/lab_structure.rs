//! #1 Lab Color phase 1b, the layer structure: the 19 blend modes of
//! `LAB_BLEND_MODES` (measured (26.10.0) by phase 2's take, which dropped
//! 1b's Subtract and Divide; the six phase 3 confirmed) composite
//! through the one compositor's Lab arms on the L\*a\*b\* codes, each
//! channel within 2 u16 codes of the independent oracle
//! (`common/lab_blend_oracle.rs`); the eight modes outside that set refuse
//! for good; a mask composites as `b + (s − b)·α·m`; Edit ▸ Fill of sRGB
//! red lands on the hand-computed CIE L\*a\*b\* (`common/lab_fixture.rs`) —
//! ≤ 32/65535 at 16 bits, on the ×257 grid within 1 code at 8; a Fill
//! Opacity style and (since phase 2e) a Multiply drop shadow are admitted,
//! a Darken one refuses and a Gray Blend If refuses by the Lab Blend If
//! list (phase 2f);
//! merges, stamps, groups, clipping and the alpha channels each give one
//! result; the `.rz` round trip of a group, a mask, Multiply, a clip and
//! Fill Opacity; and the gate's sentences, none naming a phase since the
//! close. Bounds declared before
//! measuring.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_lab::*;
use doctored_core::ffi_precision::{
    dr_precision_doc_descriptor, dr_precision_doc_free, dr_precision_doc_open,
    dr_precision_doc_save_native,
};
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::lab_blend_oracle;
use crate::lab_fixture::srgb_to_lab;
use crate::native_blend_oracle::{self, DISSOLVE_BAND, HARD_MIX_BAND};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

/// The 19 modes a Lab document offers, as their C codes, menu order.
const LAB_MODES: [i32; 19] = [
    0, 14, 1, 15, 16, 2, 12, 17, 3, 4, 5, 18, 19, 20, 21, 23, 24, 25, 26,
];

/// (backdrop, source) sRGB pairs, unit: saturated and neutral colours, so
/// a\* and b\* range away from and across 0.
const PAIRS: [([f64; 3], [f64; 3]); 8] = [
    ([0.80, 0.20, 0.30], [0.10, 0.60, 0.90]),
    ([0.25, 0.70, 0.40], [0.90, 0.85, 0.10]),
    ([0.50, 0.50, 0.50], [0.30, 0.30, 0.30]),
    ([0.15, 0.25, 0.75], [0.70, 0.20, 0.60]),
    ([0.95, 0.60, 0.20], [0.40, 0.75, 0.70]),
    ([0.35, 0.10, 0.55], [0.85, 0.85, 0.85]),
    ([0.60, 0.80, 0.90], [0.20, 0.40, 0.10]),
    ([0.05, 0.45, 0.30], [0.55, 0.15, 0.25]),
];

const OPACITY: f64 = 0.6;

fn json_of(text: *mut std::ffi::c_char, err: *mut std::ffi::c_char) -> Value {
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn descriptor(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    json_of(unsafe { dr_precision_doc_descriptor(doc, &mut err) }, err)
}

fn free(doc: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(doc) };
}

fn edit_with(
    doc: *const PrecisionDocument,
    request: &Value,
    input: &[u16],
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            if input.is_empty() {
                ptr::null()
            } else {
                input.as_ptr()
            },
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

fn edit(doc: *const PrecisionDocument, request: &Value) -> Result<*mut PrecisionDocument, String> {
    edit_with(doc, request, &[])
}

/// An edit whose payload is full coverage of the plan's `input_samples`.
fn edit_covered(doc: *const PrecisionDocument, request: &Value) -> *mut PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let plan = json_of(
        unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) },
        err,
    );
    let n = plan["input_samples"].as_u64().unwrap() as usize;
    edit_with(doc, request, &vec![65535u16; n]).unwrap()
}

fn render16(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn codes(doc: *const PrecisionDocument, layer: usize) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(layer)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn new_lab(depth: u32, w: u32, h: u32) -> *mut PrecisionDocument {
    let request = CString::new(format!(r#"{{"depth":{depth},"fill_srgb":[1,1,1,1]}}"#)).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_new_lab(w, h, request.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

fn srgb_layer(name: &str, colours: &[[f64; 3]], opacity: f32) -> Layer {
    let pixels: Vec<[u16; 4]> = colours
        .iter()
        .map(|c| {
            let code = |v: f64| (v * 65535.0).round() as u16;
            [code(c[0]), code(c[1]), code(c[2]), 65535]
        })
        .collect();
    Layer::raster(
        name,
        PrecisionImage::from_rgba16(pixels.len() as u32, 1, pixels, IccProfile::srgb(), None)
            .unwrap(),
        (0, 0),
        opacity,
        true,
    )
    .unwrap()
}

/// A 16-bit Lab document of one row: the backdrop and the source at 0.6
/// opacity, each pair of `PAIRS` per pixel, through Mode ▸ Lab Color.
fn two_layer_lab() -> *mut PrecisionDocument {
    let base: Vec<[f64; 3]> = PAIRS.iter().map(|p| p.0).collect();
    let top: Vec<[f64; 3]> = PAIRS.iter().map(|p| p.1).collect();
    let rgb = PrecisionDocument::from_parts(
        PAIRS.len() as u32,
        1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            srgb_layer("Backdrop", &base, 1.0),
            srgb_layer("Source", &top, OPACITY as f32),
        ],
    )
    .unwrap();
    let rgb = Box::into_raw(Box::new(rgb));
    let options = CString::new(r#"{"depth":16}"#).unwrap();
    let mut err = ptr::null_mut();
    let lab = unsafe {
        dr_precision_doc_convert_to_lab(rgb, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!lab.is_null(), "to Lab: {}", take_err_string(err));
    free(rgb);
    assert_eq!(descriptor(lab)["color_mode"], "lab");
    lab
}

fn blend(layer: usize, mode: i32) -> Value {
    json!({"op":"layer_blend","version":1,"layer":layer,"blend_mode":mode})
}

fn unit(code: u16) -> f64 {
    f64::from(code) / 65535.0
}

#[test]
fn every_lab_mode_composites_each_lab_channel_within_two_codes_of_the_oracle() {
    let lab = two_layer_lab();
    let base = codes(lab, 0);
    let top = codes(lab, 1);
    for mode in LAB_MODES {
        // Normal is the source's own mode: the edit changes nothing.
        let doc = if mode == 0 {
            lab
        } else {
            edit(lab, &blend(1, mode)).unwrap_or_else(|e| panic!("mode {mode}: {e}"))
        };
        let out = render16(doc);
        for x in 0..PAIRS.len() {
            assert_eq!(out[x][3], 65535, "mode {mode} x {x} alpha");
            if mode == 14 {
                // Dissolve: the source shown whole, or the backdrop untouched.
                let t = native_blend_oracle::dissolve_threshold(x as i64, 0);
                let shown = out[x][..3] == top[x][..3];
                assert!(shown || out[x][..3] == base[x][..3], "dissolve x {x}");
                if (OPACITY - t).abs() >= DISSOLVE_BAND {
                    assert_eq!(shown, t < OPACITY, "dissolve x {x} t {t}");
                }
                continue;
            }
            let cb: [f64; 3] = std::array::from_fn(|c| unit(base[x][c]));
            let cs: [f64; 3] = std::array::from_fn(|c| unit(top[x][c]));
            if mode == 21 {
                // Hard Mix's 16-bit arm thresholds L̂ and ½ + a*/255.
                let y = |u: f64| 0.5 + lab_blend_oracle::star(u) / 255.0;
                let margins = [
                    (cb[0] + cs[0] - 1.).abs(),
                    (y(cb[1]) + y(cs[1]) - 1.).abs(),
                    (y(cb[2]) + y(cs[2]) - 1.).abs(),
                ];
                // An exact tie (both neutral) is the kernel's tie too: the
                // oracle and the kernel break it by the same order rule.
                assert!(
                    margins.iter().all(|m| *m > HARD_MIX_BAND || *m < 1e-9),
                    "x {x} near the tie {margins:?}"
                );
            }
            let mixed = lab_blend_oracle::composite(mode, false, cb, cs, OPACITY, 1.0);
            for c in 0..3 {
                let want = mixed[c] * 65535.0;
                let got = f64::from(out[x][c]);
                assert!(
                    (got - want).abs() <= 2.0,
                    "mode {mode} x {x} channel {c}: {got} against {want}"
                );
            }
        }
        assert_eq!(descriptor(doc)["color_mode"], "lab");
        if mode != 0 {
            free(doc);
        }
    }
    free(lab);
}

#[test]
fn the_eight_modes_lab_does_not_offer_refuse_by_name() {
    let lab = two_layer_lab();
    for (mode, name) in [
        (6, "Darken"),
        (11, "Color Burn"),
        (7, "Lighten"),
        (10, "Color Dodge"),
        (8, "Difference"),
        (9, "Exclusion"),
        (13, "Subtract"),
        (22, "Divide"),
    ] {
        assert_eq!(
            edit(lab, &blend(1, mode)).unwrap_err(),
            format!("The {name} blend mode is unavailable in a Lab Color document.")
        );
    }
    free(lab);
}

#[test]
fn a_masked_source_composites_by_its_mask_and_fill_of_the_mask_hides_it() {
    let lab = two_layer_lab();
    let base = codes(lab, 0);
    let top = codes(lab, 1);
    let mask: Vec<u16> = (0..PAIRS.len() as u16).map(|i| i * 9000).collect();
    let masked = edit_with(
        lab,
        &json!({"op":"set_mask","layer":1,"width":PAIRS.len(),"height":1,"x":0,"y":0,"linked":true}),
        &mask,
    )
    .unwrap();
    let out = render16(masked);
    for x in 0..PAIRS.len() {
        let m = f64::from(mask[x]) / 65535.0;
        for c in 0..3 {
            let (b, s) = (f64::from(base[x][c]), f64::from(top[x][c]));
            let want = b + (s - b) * OPACITY * m;
            let got = f64::from(out[x][c]);
            assert!(
                (got - want).abs() <= 2.0,
                "x {x} channel {c}: {got} against {want}"
            );
        }
    }
    let hidden = edit(
        masked,
        &json!({"op":"scalar_edit","target":"mask","index":1,
            "operation":{"kind":"fill","value":0,"opacity":1.0},"selection":false}),
    )
    .unwrap();
    let base_only = edit(
        lab,
        &json!({"op":"layer_visible","layer":1,"visible":false}),
    )
    .unwrap();
    assert_eq!(render16(hidden), render16(base_only));
    for doc in [base_only, hidden, masked, lab] {
        free(doc);
    }
}

fn fill_red(doc: *const PrecisionDocument, layer: usize) -> *mut PrecisionDocument {
    edit_covered(
        doc,
        &json!({"op":"paint_blend","layer":layer,"color":[1.0,0.0,0.0,1.0],"color_space":"srgb",
            "opacity":1.0,"blend_mode":0}),
    )
}

#[test]
fn fill_paints_the_colours_lab_by_mode_lab_colors_rule() {
    let lab = srgb_to_lab([1.0, 0.0, 0.0]);
    // The ICC v4 codes as units: L* / 100, (a* + 128) / 255.
    let units = [
        lab[0] / 100.0,
        (lab[1] + 128.0) / 255.0,
        (lab[2] + 128.0) / 255.0,
    ];
    // 16 bits: ≤ 32 of 65535.
    let doc = new_lab(16, 3, 2);
    let filled = fill_red(doc, 0);
    for p in codes(filled, 0) {
        for c in 0..3 {
            let want = units[c] * 65535.0;
            assert!(
                (f64::from(p[c]) - want).abs() <= 32.0,
                "16 channel {c}: {} against {want}",
                p[c]
            );
        }
        assert_eq!(p[3], 65535);
    }
    assert_eq!(descriptor(filled)["color_mode"], "lab");
    free(filled);
    free(doc);
    // 8 bits: on the ×257 grid, ≤ 1 code.
    let doc = new_lab(8, 3, 2);
    let filled = fill_red(doc, 0);
    for p in codes(filled, 0) {
        for c in 0..3 {
            assert_eq!(p[c] % 257, 0, "{p:?}");
            let want = units[c] * 255.0;
            assert!(
                (f64::from(p[c] / 257) - want).abs() <= 1.0,
                "8 channel {c}: {p:?} against {want}"
            );
        }
    }
    free(filled);
    // A refused mode refuses by name.
    assert_eq!(
        edit(
            doc,
            &json!({"op":"paint_blend","layer":0,"color":[1.0,0.0,0.0,1.0],"color_space":"srgb",
                "opacity":1.0,"blend_mode":6})
        )
        .unwrap_err(),
        "The Darken blend mode is unavailable in a Lab Color document."
    );
    free(doc);
}

fn style(layer: usize, style: Value) -> Value {
    json!({"op":"layer_style","version":1,"layer":layer,"style":style})
}

#[test]
fn blending_options_and_effects_are_admitted_and_structure_edits_give_one_result_each() {
    let lab = two_layer_lab();
    let filled = edit(
        lab,
        &style(1, json!({"fill_opacity":0.5,"knockout":"shallow"})),
    )
    .unwrap();
    assert_eq!(descriptor(filled)["color_mode"], "lab");
    // Phase 2e: a drop shadow in its default Multiply is admitted; one in
    // a mode the Lab list lacks refuses by that mode's sentence.
    let shadowed = edit(
        lab,
        &style(
            1,
            json!({"effects":[{"type":"drop_shadow","distance":3,"size":2}]}),
        ),
    )
    .unwrap();
    assert_eq!(descriptor(shadowed)["color_mode"], "lab");
    free(shadowed);
    assert_eq!(
        edit(
            lab,
            &style(
                1,
                json!({"effects":[{"type":"drop_shadow","distance":3,"size":2,"blend":"darken"}]})
            )
        )
        .unwrap_err(),
        "The Darken blend mode is unavailable in a Lab Color document."
    );
    assert_eq!(
        edit(lab, &style(1, json!({"blend_if":{"channel":"gray"}}))).unwrap_err(),
        "Blend If on the Gray channel is unavailable in a Lab Color document: its Blend If channels are Lightness, a and b."
    );
    let cleared = edit(filled, &style(1, Value::Null)).unwrap();
    free(cleared);
    // Multiply, then Merge Down, Stamp Visible, Merge Visible.
    let multiplied = edit(filled, &blend(1, 1)).unwrap();
    let merged = edit(multiplied, &json!({"op":"merge_down","layer":1})).unwrap();
    assert_eq!(descriptor(merged)["layer_count"], 1);
    assert_eq!(descriptor(merged)["color_mode"], "lab");
    let (a, b) = (render16(merged), render16(multiplied));
    for (x, (p, q)) in a.iter().zip(&b).enumerate() {
        for c in 0..4 {
            assert!(
                p[c].abs_diff(q[c]) <= 2,
                "merge down x {x}: {p:?} against {q:?}"
            );
        }
    }
    let stamped = edit(
        multiplied,
        &json!({"op":"stamp_visible","name":"Stamp","anchor":1}),
    )
    .unwrap();
    assert_eq!(descriptor(stamped)["layer_count"], 3);
    assert_eq!(descriptor(stamped)["color_mode"], "lab");
    let visible = edit(multiplied, &json!({"op":"merge_visible"})).unwrap();
    assert_eq!(descriptor(visible)["layer_count"], 1);
    // New Layer, a group, ungroup and a clip.
    let added = edit(
        lab,
        &json!({"op":"add_layer","name":"Layer 1","anchor":1,"position":"above"}),
    )
    .unwrap();
    assert_eq!(descriptor(added)["layer_count"], 3);
    let grouped = edit(
        added,
        &json!({"op":"group_layers","layers":[1,2],"name":"Group","pass_through":false}),
    )
    .unwrap();
    assert_eq!(descriptor(grouped)["color_mode"], "lab");
    let clipped = edit(
        added,
        &json!({"op":"layer_clipped","layer":2,"clipped":true}),
    )
    .unwrap();
    assert_eq!(descriptor(clipped)["color_mode"], "lab");
    // The alpha channels: New, Duplicate, Rename, Options, Fill, Delete.
    let plane: Vec<u16> = (0..PAIRS.len() as u16).map(|i| i * 8000).collect();
    let channel = edit_with(
        lab,
        &json!({"op":"set_channel","index":null,"name":"Alpha 1"}),
        &plane,
    )
    .unwrap();
    assert_eq!(unsafe { &*channel }.channels().len(), 1);
    let duplicated = edit(channel, &json!({"op":"duplicate_channel","index":0})).unwrap();
    let renamed = edit(
        duplicated,
        &json!({"op":"rename_channel","index":1,"name":"Matte"}),
    )
    .unwrap();
    let options = edit(
        renamed,
        &json!({"op":"channel_properties","index":1,"overlay_color":[0,200,40],"overlay_opacity":0.5}),
    )
    .unwrap();
    let painted = edit(
        options,
        &json!({"op":"scalar_edit","target":"channel","index":1,"operation":{"kind":"fill","value":20000}}),
    )
    .unwrap();
    let removed = edit(painted, &json!({"op":"remove_channel","index":0})).unwrap();
    assert_eq!(unsafe { &*removed }.channels().len(), 1);
    assert_eq!(descriptor(removed)["color_mode"], "lab");
    for doc in [
        removed, painted, options, renamed, duplicated, channel, clipped, grouped, added, visible,
        stamped, merged, multiplied, filled, lab,
    ] {
        free(doc);
    }
}

#[test]
fn a_group_mask_multiply_clip_and_fill_opacity_round_trip_through_rz() {
    let lab = two_layer_lab();
    let added = edit(
        lab,
        &json!({"op":"add_layer","name":"Clip","anchor":1,"position":"above"}),
    )
    .unwrap();
    let painted = fill_red(added, 2);
    let mask: Vec<u16> = (0..PAIRS.len() as u16).map(|i| 65535 - i * 7000).collect();
    let masked = edit_with(
        painted,
        &json!({"op":"set_mask","layer":1,"width":PAIRS.len(),"height":1,"x":0,"y":0,"linked":true}),
        &mask,
    )
    .unwrap();
    let multiplied = edit(masked, &blend(1, 1)).unwrap();
    let clipped = edit(
        multiplied,
        &json!({"op":"layer_clipped","layer":2,"clipped":true}),
    )
    .unwrap();
    let styled = edit(clipped, &style(1, json!({"fill_opacity":0.5}))).unwrap();
    let grouped = edit(
        styled,
        &json!({"op":"group_layers","layers":[1,2],"name":"Group"}),
    )
    .unwrap();
    let before = render16(grouped);
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("structure.rz");
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_native(grouped, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
        },
        "{}",
        take_err_string(err)
    );
    let reopened = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!reopened.is_null(), "{}", take_err_string(err));
    let d = descriptor(reopened);
    assert_eq!(d["color_mode"], "lab");
    assert_eq!(d["lab"]["depth"], 16);
    assert_eq!(d["layer_count"], descriptor(grouped)["layer_count"]);
    assert_eq!(render16(reopened), before);
    for doc in [
        reopened, grouped, styled, clipped, multiplied, masked, painted, added, lab,
    ] {
        free(doc);
    }
}

#[test]
fn no_remaining_refusal_names_a_phase() {
    let lab = two_layer_lab();
    let targets = |op: &str| {
        format!(
            "{op} on a Lab Color document takes target mask, channel, red (Lightness), green \
             (a), blue (b) or alpha."
        )
    };
    for request in [
        json!({"op":"read_plane","target":"layer","index":0}),
        json!({"op":"scalar_statistics","target":"layer","index":0}),
    ] {
        let op = request["op"].as_str().unwrap().to_string();
        assert_eq!(edit(lab, &request).unwrap_err(), targets(&op));
    }
    // An op no parser knows answers as on RGB.
    for request in [json!({"op":"sample"}), json!({"op":"adjustment"})] {
        assert_eq!(
            edit(lab, &request).unwrap_err(),
            "operation is unavailable in the high-depth editing request API"
        );
    }
    // Phase 5b: no document profile.
    assert_eq!(
        edit(lab, &json!({"op":"color_convert"})).unwrap_err(),
        "color_convert is unavailable in a Lab Color document, which has no document profile."
    );
    assert_eq!(
        edit(
            lab,
            &json!({"op":"scalar_edit","target":"layer","index":0,"operation":{"kind":"fill","value":0}})
        )
        .unwrap_err(),
        targets("scalar_edit")
    );
    let operations = descriptor(lab)["supported_operations"].clone();
    for family in [
        "layer_add",
        "layer_blend",
        "mask_set",
        "fill_stroke",
        "export",
        "channel_set",
    ] {
        assert!(
            operations.as_array().unwrap().contains(&family.into()),
            "{family}: {operations}"
        );
    }
    free(lab);
}
