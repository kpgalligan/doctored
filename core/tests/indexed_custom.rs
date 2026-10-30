//! #1 Indexed Color phase 1c's palettes and direct conversions through the
//! FFI: the Custom palette against an independent nearest-entry oracle (ties
//! to the lowest index) and its refusals; Previous (the host's: the last
//! conversion's palette minus its transparent entry, used as Custom)
//! against the palette it names; and Indexed → CMYK and Lab Color, the
//! palette expanded exactly (`dr_precision_doc_convert_indexed_to_rgb`) and
//! converted by the existing routes, against the same conversions of an
//! independently built RGB document of `palette[i] × 257`.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_indexed::dr_precision_doc_convert_indexed_to_rgb;
use doctored_core::ffi_lab::dr_precision_doc_convert_to_lab;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::cmyk_fixture::press_profile;
use crate::common::take_err_string;
use crate::indexed_fixture::{
    color_table, descriptor, free, indices, layer16, opaque, plan, rgb_doc, to_indexed, LIMIT,
};

const TABLE_SIZE: &str = "a colour table holds 2 to 256 colours";
const CUSTOM_FULL: &str = "The Custom palette holds 256 colours, leaving no room for the \
                           transparent colour; turn Transparency off or use 255 colours.";

/// The nearest entry of `table` to `rgb`: least squared distance, ties to
/// the lowest index (written independently of the core's memo).
fn nearest(table: &[[u8; 3]], rgb: [u8; 3]) -> i32 {
    let distance = |entry: &[u8; 3]| -> i32 {
        (0..3)
            .map(|c| (i32::from(entry[c]) - i32::from(rgb[c])).pow(2))
            .sum()
    };
    let mut best = 0usize;
    for (i, entry) in table.iter().enumerate() {
        if distance(entry) < distance(&table[best]) {
            best = i;
        }
    }
    best as i32
}

fn pixels() -> Vec<[u8; 3]> {
    vec![
        [0, 0, 0],
        [255, 255, 255],
        [130, 20, 20],
        [20, 130, 20],
        // Equidistant from entries 0 and 3.
        [64, 64, 64],
        [200, 100, 100],
        [12, 200, 240],
        [128, 128, 128],
    ]
}

fn table() -> Vec<[u8; 3]> {
    vec![
        [0, 0, 0],
        [255, 255, 255],
        [150, 0, 0],
        [128, 128, 128],
        [0, 150, 0],
    ]
}

#[test]
fn custom_maps_every_pixel_to_its_nearest_table_entry() {
    let source = pixels();
    let rgb = rgb_doc(4, 2, &opaque(&source));
    let indexed = to_indexed(
        rgb,
        &json!({"palette":"custom","table":table(),"dither":"none"}),
    )
    .unwrap();
    assert_eq!(
        color_table(indexed),
        json!({"colors": table(), "transparent_index": null})
    );
    let expected: Vec<i32> = source.iter().map(|&c| nearest(&table(), c)).collect();
    assert_eq!(indices(indexed), expected);
    // The tie at [64,64,64] (distance 3·64² to both [0,0,0] and
    // [128,128,128]) goes to the lower index.
    assert_eq!(expected[4], 0);
    // Duplicates are allowed and keep the lower index.
    let mut doubled = table();
    doubled.push([150, 0, 0]);
    let dup = to_indexed(
        rgb,
        &json!({"palette":"custom","table":doubled,"dither":"none"}),
    )
    .unwrap();
    assert_eq!(indices(dup), expected);
    // Diffusion runs over the same table.
    let dithered = to_indexed(rgb, &json!({"palette":"custom","table":table()})).unwrap();
    assert_eq!(color_table(dithered)["colors"], json!(table()));
    for doc in [dithered, dup, indexed, rgb] {
        free(doc);
    }
}

#[test]
fn custom_appends_the_transparent_entry_and_refuses_a_full_table_beside_it() {
    let mut codes = opaque(&pixels());
    codes[7] = [1, 2, 3, 0];
    let rgb = rgb_doc(4, 2, &codes);
    let indexed = to_indexed(
        rgb,
        &json!({"palette":"custom","table":table(),"dither":"none"}),
    )
    .unwrap();
    let mut with_white = table();
    with_white.push([255, 255, 255]);
    assert_eq!(
        color_table(indexed),
        json!({"colors": with_white, "transparent_index": 5})
    );
    assert_eq!(indices(indexed)[7], 5);
    // The matte is the appended entry when one is set.
    let matted = to_indexed(
        rgb,
        &json!({"palette":"custom","table":table(),"dither":"none","matte":[9,9,9]}),
    )
    .unwrap();
    assert_eq!(color_table(matted)["colors"][5], json!([9, 9, 9]));
    // 256 entries beside a transparent pixel: the plan's refusal and the
    // conversion's error; without Transparency it converts.
    let full: Vec<[u8; 3]> = (0..=255u8).map(|i| [i, i, i]).collect();
    let options = json!({"palette":"custom","table":full,"dither":"none"});
    assert_eq!(plan(rgb, &options)["refusal"], CUSTOM_FULL);
    assert_eq!(to_indexed(rgb, &options).unwrap_err(), CUSTOM_FULL);
    let opaque_options =
        json!({"palette":"custom","table":full,"dither":"none","transparency":false});
    assert!(plan(rgb, &opaque_options)["refusal"].is_null());
    let flat = to_indexed(rgb, &opaque_options).unwrap();
    assert_eq!(descriptor(flat)["indexed"]["colors"], 256);
    // 255 entries leave room.
    let fewer: Vec<[u8; 3]> = full[..255].to_vec();
    let room = to_indexed(
        rgb,
        &json!({"palette":"custom","table":fewer,"dither":"none"}),
    )
    .unwrap();
    assert_eq!(color_table(room)["transparent_index"], 255);
    for doc in [room, flat, matted, indexed, rgb] {
        free(doc);
    }
}

#[test]
fn custom_options_refuse_by_sentence() {
    let rgb = rgb_doc(2, 1, &opaque(&[[0, 0, 0], [255, 255, 255]]));
    let one = json!({"palette":"custom","table":[[0,0,0]]});
    assert_eq!(to_indexed(rgb, &one).unwrap_err(), TABLE_SIZE);
    let many: Vec<[u8; 3]> = vec![[1, 1, 1]; 257];
    assert_eq!(
        to_indexed(rgb, &json!({"palette":"custom","table":many})).unwrap_err(),
        TABLE_SIZE
    );
    assert_eq!(
        to_indexed(
            rgb,
            &json!({"palette":"custom","table":[[0,0,0],[1,1,1]],"colors":2})
        )
        .unwrap_err(),
        "mode option colors does not apply to the Custom palette: its table sets the count"
    );
    assert_eq!(
        to_indexed(rgb, &json!({"palette":"web","table":[[0,0,0],[1,1,1]]})).unwrap_err(),
        "mode option table applies to the Custom palette only"
    );
    assert!(to_indexed(rgb, &json!({"palette":"custom"}))
        .unwrap_err()
        .contains("table"));
    assert!(
        to_indexed(rgb, &json!({"palette":"custom","table":[[0,0,0],[1,1]]}))
            .unwrap_err()
            .contains("[r, g, b]")
    );
    assert_eq!(
        to_indexed(
            rgb,
            &json!({"palette":"custom","table":[[0,0,0],[1,1,1]],"forced":"black_white"})
        )
        .unwrap_err(),
        "Forced colours apply to the Uniform and Adaptive palettes only."
    );
    free(rgb);
}

#[test]
fn previous_is_the_last_palette_without_its_transparent_entry() {
    // Document A: an Adaptive conversion with a transparent pixel; its
    // palette minus the transparent entry is what Previous names.
    let mut codes = opaque(&pixels());
    codes[0] = [0, 0, 0, 0];
    let a = rgb_doc(4, 2, &codes);
    let a_indexed = to_indexed(a, &json!({"palette":"adaptive","colors":5})).unwrap();
    let table_a = color_table(a_indexed);
    let t = table_a["transparent_index"].as_u64().unwrap() as usize;
    let mut previous: Vec<Value> = table_a["colors"].as_array().unwrap().clone();
    previous.remove(t);
    assert_eq!(previous.len(), 4);
    // Document B (opaque) takes it as Custom: the same palette, each pixel
    // its nearest entry.
    let source: Vec<[u8; 3]> = pixels().iter().rev().copied().collect();
    let b = rgb_doc(4, 2, &opaque(&source));
    let b_indexed = to_indexed(
        b,
        &json!({"palette":"custom","table":previous,"dither":"none"}),
    )
    .unwrap();
    assert_eq!(
        color_table(b_indexed),
        json!({"colors": previous, "transparent_index": null})
    );
    let entries: Vec<[u8; 3]> = previous
        .iter()
        .map(|v| serde_json::from_value(v.clone()).unwrap())
        .collect();
    let expected: Vec<i32> = source.iter().map(|&c| nearest(&entries, c)).collect();
    assert_eq!(indices(b_indexed), expected);
    for doc in [b_indexed, b, a_indexed, a] {
        free(doc);
    }
}

fn expand(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_convert_indexed_to_rgb(doc, &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    out
}

fn to_cmyk(doc: *const PrecisionDocument, icc: &[u8]) -> *mut PrecisionDocument {
    let options = CString::new(r#"{"depth":8}"#).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
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
    assert!(!out.is_null(), "to CMYK: {}", take_err_string(err));
    out
}

fn to_lab(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let options = CString::new(r#"{"depth":8}"#).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_lab(doc, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!out.is_null(), "to Lab: {}", take_err_string(err));
    out
}

/// An Indexed document of a Uniform palette (dithered, so many entries are
/// used) with a transparent pixel, and the independent RGB document of each
/// pixel's `palette[i] × 257`, alpha 0 at the transparent index.
fn indexed_and_oracle() -> (*mut PrecisionDocument, *mut PrecisionDocument) {
    let (w, h) = (8u32, 4u32);
    let mut codes: Vec<[u8; 4]> = (0..w * h)
        .map(|i| {
            let v = (i * 8) as u8;
            [v, 255 - v, (v / 2).wrapping_add(40), 255]
        })
        .collect();
    codes[5] = [10, 10, 10, 0];
    let rgb = rgb_doc(w, h, &codes);
    let indexed = to_indexed(rgb, &json!({"palette":"uniform","colors":64})).unwrap();
    free(rgb);
    let table = color_table(indexed);
    let palette: Vec<[u8; 3]> = serde_json::from_value(table["colors"].clone()).unwrap();
    let t = table["transparent_index"].as_i64();
    let oracle_codes: Vec<[u8; 4]> = indices(indexed)
        .iter()
        .map(|&i| {
            let c = palette[i as usize];
            let alpha = if Some(i64::from(i)) == t { 0 } else { 255 };
            [c[0], c[1], c[2], alpha]
        })
        .collect();
    assert!(t.is_some());
    (indexed, rgb_doc(w, h, &oracle_codes))
}

#[test]
fn indexed_to_cmyk_is_the_expanded_palette_converted() {
    let (indexed, oracle) = indexed_and_oracle();
    let profile = press_profile("Press Test");
    let expanded = expand(indexed);
    let direct = to_cmyk(expanded, &profile);
    let reference = to_cmyk(oracle, &profile);
    assert_eq!(descriptor(direct)["color_mode"], "cmyk");
    let samples = |doc: *mut PrecisionDocument| unsafe { &*doc }.cmyk_layer_samples(0).unwrap();
    assert_eq!(samples(direct), samples(reference));
    for doc in [reference, direct, expanded, oracle, indexed] {
        free(doc);
    }
}

#[test]
fn indexed_to_lab_is_the_expanded_palette_converted() {
    let (indexed, oracle) = indexed_and_oracle();
    let expanded = expand(indexed);
    let direct = to_lab(expanded);
    let reference = to_lab(oracle);
    assert_eq!(descriptor(direct)["color_mode"], "lab");
    assert_eq!(layer16(direct), layer16(reference));
    for doc in [reference, direct, expanded, oracle, indexed] {
        free(doc);
    }
}
