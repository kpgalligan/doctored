//! A styled CMYK document through the stack operations (#1, the styles
//! batch): flatten, Merge Visible, Stamp Visible, Merge Down (a deep
//! knockout layer's included: the merge is seeded from the source stack),
//! Merge Layers and Copy Merged within 1/65535 of the projection; a layers
//! paste keeping the style, and converting its inks through the paste's leg
//! across profiles; both mode conversions carrying the style and the light;
//! Convert to Profile mapping every ink as it maps a raster's identical ink;
//! Assign re-canonicalising the hex, which a later RGB Color keeps; and the
//! `.rz` word 86 (bit-exact round trip, a Fill-only style staying 84, an 86
//! file relabelled 84 refused).

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::cmyk_clipboard_codec::CmykClipboardPayload;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_clipboard::{
    dr_clipboard_payload_bytes, dr_clipboard_payload_free, dr_precision_doc_clipboard_copy,
};
use doctored_core::ffi_cmyk::{
    dr_cmyk_color, dr_precision_doc_convert_cmyk_to_rgb, dr_precision_doc_convert_to_cmyk,
};
use doctored_core::ffi_cmyk_colour::dr_precision_doc_cmyk_profile_edit;
use doctored_core::ffi_layer_clipboard::dr_precision_doc_paste_layers;
use doctored_core::ffi_precision::dr_precision_doc_descriptor;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::cmyk_fixture::{press_profile, press_profile_b};
use crate::cmyk_structure_fixture::{composite, free, Ink, LIMIT};
use crate::cmyk_style_fixture::{
    backdrop, edit, flat, hard_rect, layer, percent, soft_rect, styled,
};
use crate::common::take_err_string;

const W: u32 = 32;
const H: u32 = 24;
const COLUMNS: [[f64; 4]; 4] = [
    [0.0, 0.0, 0.0, 0.0],
    [50.0, 0.0, 0.0, 0.0],
    [20.0, 40.0, 60.0, 10.0],
    [0.0, 0.0, 0.0, 50.0],
];
const E1: [f64; 4] = [75.0, 68.0, 67.0, 90.0];
const E2: [f64; 4] = [100.0, 0.0, 0.0, 0.0];

/// The postorder index of the record named `name`.
fn index(doc: *const PrecisionDocument, name: &str) -> usize {
    let doc = unsafe { &*doc };
    (0..doc.layer_count())
        .find(|&i| doc.layer(i).unwrap().name() == name)
        .unwrap_or_else(|| panic!("no record {name}"))
}

fn shape_style() -> Value {
    json!({"effects":[
        {"type":"drop_shadow","angle":120,"use_global_light":true,"distance":3,"size":2,"ink":percent(E1),"blend":"normal","opacity":0.8},
        {"type":"stroke","size":2,"position":"outside","ink":percent(E2)},
        {"type":"color_overlay","ink":percent([0.0, 40.0, 0.0, 0.0]),"opacity":0.5}]})
}

/// The backdrop, an "Under" layer, a styled "Shape", a "Probe" layer of
/// exactly E1 and, with `knock`, a deep-knockout "Knock" at Fill 0.3. Every
/// effect is Normal (the drop shadow's and inner glow's defaults are not): a
/// merged raster composites once at Normal over what lies below, so a merge
/// is exact only for effects whose mode associates with it (D7).
fn scene(knock: bool) -> *mut PrecisionDocument {
    let doc = layer(
        backdrop(W, H, 16, &COLUMNS),
        "Under",
        [0.0, 70.0, 20.0, 5.0],
        &hard_rect(W, H, (2, 3, 30, 21)),
    );
    let doc = layer(
        doc,
        "Shape",
        [10.0, 60.0, 30.0, 0.0],
        &soft_rect(W, H, (8, 6, 22, 16)),
    );
    let doc = styled(doc, index(doc, "Shape"), &shape_style());
    let doc = layer(doc, "Probe", E1, &hard_rect(W, H, (0, 22, 2, 24)));
    if !knock {
        return doc;
    }
    let doc = layer(
        doc,
        "Knock",
        [0.0, 0.0, 100.0, 0.0],
        &soft_rect(W, H, (12, 8, 28, 20)),
    );
    let style = json!({"knockout":"deep","fill_opacity":0.3,
        "effects":[{"type":"inner_glow","size":3,"ink":percent(E2),"blend":"normal"}]});
    styled(doc, index(doc, "Knock"), &style)
}

fn composites(doc: *const PrecisionDocument) -> Vec<[u16; 5]> {
    (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .map(|(x, y)| composite(doc, x as i32, y as i32))
        .collect()
}

fn assert_close(got: &[[u16; 5]], want: &[[u16; 5]], channels: usize, what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: length");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        for c in 0..channels {
            assert!(
                g[c].abs_diff(w[c]) <= 1,
                "{what}: pixel {i} channel {c}: got {g:?} want {w:?}"
            );
        }
    }
}

fn must(doc: *const PrecisionDocument, request: Value) -> *mut PrecisionDocument {
    let out = edit(doc, &request).unwrap_or_else(|e| panic!("{request}: {e}"));
    assert!(!out.is_null(), "{request} changed nothing");
    out
}

fn style_of(doc: *const PrecisionDocument, name: &str) -> Value {
    let reported = unsafe { &*doc }
        .layer(index(doc, name))
        .unwrap()
        .reported_style()
        .unwrap_or_else(|| panic!("{name} has no style"));
    serde_json::from_str(&reported).unwrap()
}

#[test]
fn every_stack_operation_meets_the_projection() {
    let doc = scene(true);
    let before = composites(doc);
    assert_close(&flat(doc), &before, 4, "flatten");
    for request in [
        json!({"op":"merge_visible"}),
        json!({"op":"merge_down","layer":index(doc, "Knock")}),
        json!({"op":"merge_layers","layers":[index(doc, "Shape"), index(doc, "Knock")]}),
        json!({"op":"merge_layers","layers":[index(doc, "Under"), index(doc, "Shape")]}),
    ] {
        let after = must(doc, request.clone());
        assert_close(&composites(after), &before, 5, &request.to_string());
        free(after);
    }
    let top = unsafe { &*doc }.layer_count() - 1;
    let stamped = must(
        doc,
        json!({"op":"stamp_visible","name":"Stamp","anchor":top}),
    );
    let stamp = unsafe { &*stamped }.cmyk_layer_samples(top + 1).unwrap();
    assert_close(&stamp, &before, 5, "stamp visible");
    free(stamped);
    let request = CString::new(format!(
        r#"{{"op":"copy_merged","x":0,"y":0,"width":{W},"height":{H}}}"#
    ))
    .unwrap();
    let mut err = ptr::null_mut();
    let payload = unsafe {
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
    assert!(!payload.is_null(), "copy merged: {}", take_err_string(err));
    let mut length = 0usize;
    let bytes = unsafe {
        std::slice::from_raw_parts(dr_clipboard_payload_bytes(payload, &mut length), length)
    }
    .to_vec();
    let merged = CmykClipboardPayload::decode(&bytes, LIMIT, 0).expect("a CMYK payload");
    assert_close(
        &merged.image().to_contiguous().unwrap(),
        &before,
        5,
        "copy merged",
    );
    unsafe { dr_clipboard_payload_free(payload) };
    // Merge Down of the knockout layer charges the seed's stack beside the
    // members' (`cmyk_merge_down_scratch_bytes`, D7).
    let plan = |doc: *const PrecisionDocument| {
        let mut err = ptr::null_mut();
        let text = unsafe { dr_precision_doc_descriptor(doc, &mut err) };
        assert!(!text.is_null(), "{}", take_err_string(err));
        let value: Value =
            serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
        unsafe { dr_string_free(text) };
        value["cmyk_merge_down_scratch_bytes"]
            .as_u64()
            .expect("a merge-down plan")
    };
    let plain = styled(scene(true), 4, &json!({"fill_opacity":0.3}));
    assert!(plan(doc) > plan(plain), "{} vs {}", plan(doc), plan(plain));
    free(plain);
    free(doc);
}

fn paste(doc: *const PrecisionDocument, clip: *const PrecisionDocument) -> *mut PrecisionDocument {
    let anchor = unsafe { &*doc }.layer_count() - 1;
    let (mut report, mut err) = (ptr::null_mut(), ptr::null_mut());
    let out = unsafe {
        dr_precision_doc_paste_layers(
            doc,
            clip,
            anchor,
            1,
            0,
            0,
            false,
            LIMIT,
            0,
            &mut report,
            &mut err,
        )
    };
    if !report.is_null() {
        unsafe { dr_string_free(report) };
    }
    assert!(!out.is_null(), "paste: {}", take_err_string(err));
    out
}

/// A paper-white CMYK document in `profile`.
fn paper(profile: &[u8]) -> *mut PrecisionDocument {
    let request = r#"{"depth":16,"fill":[0,0,0,0,1],"layer_name":"Paper"}"#;
    let doc = PrecisionDocument::new_cmyk(W, H, Some(profile), request, LIMIT, 0).unwrap();
    Box::into_raw(Box::new(doc))
}

fn ink_of(style: &Value, effect: usize) -> [f64; 4] {
    let ink = style["effects"][effect]["ink"].as_array().expect("an ink");
    [0, 1, 2, 3].map(|c| ink[c].as_f64().unwrap())
}

/// The Probe layer's one stored ink (it is exactly E1 before a conversion).
fn probe(doc: *const PrecisionDocument) -> [f64; 4] {
    let doc = unsafe { &*doc };
    let samples = doc.cmyk_layer_samples(index(doc, "Probe")).unwrap();
    let s = samples
        .iter()
        .find(|s| s[4] == 65535)
        .expect("an opaque probe pixel");
    [0, 1, 2, 3].map(|c| f64::from(s[c]) / 65535.0)
}

fn assert_ink(got: [f64; 4], want: [f64; 4], what: &str) {
    for c in 0..4 {
        assert!(
            (got[c] - want[c]).abs() <= 2e-4,
            "{what}: plate {c}: got {got:?} want {want:?}"
        );
    }
}

#[test]
fn a_layers_paste_keeps_the_style_and_converts_its_inks_across_profiles() {
    let clip = scene(false);
    let same = paste(paper(&press_profile("Press Test")), clip);
    assert_eq!(style_of(same, "Shape"), style_of(clip, "Shape"));
    let other = paste(paper(&press_profile_b("B")), clip);
    let style = style_of(other, "Shape");
    assert_ink(
        ink_of(&style, 0),
        probe(other),
        "the shadow's ink takes the raster's leg",
    );
    assert_ne!(
        style["effects"][0]["ink"],
        style_of(clip, "Shape")["effects"][0]["ink"]
    );
    for doc in [clip, same, other] {
        free(doc);
    }
}

fn to_rgb(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let options = CString::new("").unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_cmyk_to_rgb(
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
    assert!(!out.is_null(), "to RGB: {}", take_err_string(err));
    out
}

fn to_cmyk(doc: *const PrecisionDocument, icc: &[u8]) -> *mut PrecisionDocument {
    let options = CString::new("").unwrap();
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

#[test]
fn both_mode_conversions_carry_the_style_and_the_light() {
    let lit = must(
        scene(true),
        json!({"op":"global_light","angle":45.0,"altitude":60.0}),
    );
    let cmyk_style = style_of(lit, "Shape");
    let rgb = to_rgb(lit);
    let carried = style_of(rgb, "Shape");
    assert_eq!(
        unsafe { &*rgb }.global_light(),
        unsafe { &*lit }.global_light()
    );
    for (i, effect) in carried["effects"].as_array().unwrap().iter().enumerate() {
        assert!(
            effect.get("ink").is_none(),
            "an RGB style holds no ink: {effect}"
        );
        // The hex is the ink's canonical sRGB, so the RGB colour is the ink's.
        assert_eq!(effect["color"], cmyk_style["effects"][i]["color"]);
    }
    assert_eq!(style_of(rgb, "Knock")["knockout"], json!("deep"));
    let back = to_cmyk(rgb, &press_profile("Press Test"));
    assert_eq!(
        unsafe { &*back }.global_light(),
        unsafe { &*lit }.global_light()
    );
    assert_eq!(
        style_of(back, "Shape"),
        carried,
        "toward CMYK the hex rides as it is"
    );
    assert_eq!(style_of(back, "Knock")["knockout"], json!("deep"));
    for doc in [lit, rgb, back] {
        free(doc);
    }
}

fn profile_edit(
    doc: *const PrecisionDocument,
    icc: &[u8],
    request: &str,
) -> *mut PrecisionDocument {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_cmyk_profile_edit(
            doc,
            icc.as_ptr(),
            icc.len(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    out
}

/// The `#rrggbb` of `ink` under `doc`'s profile (`dr_cmyk_color`).
fn hex_under(doc: *const PrecisionDocument, ink: [f64; 4]) -> String {
    let text = CString::new(json!({"from":"ink","values":ink}).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_cmyk_color(doc, text.as_ptr(), &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    let value: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(out) };
    let srgb = value["srgb"].as_array().unwrap();
    let byte = |c: usize| (srgb[c].as_f64().unwrap() * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", byte(0), byte(1), byte(2))
}

#[test]
fn convert_maps_every_ink_and_assign_recanonicalises_the_hex() {
    let doc = scene(false);
    let b = press_profile_b("B");
    let converted = profile_edit(doc, &b, r#"{"op":"convert"}"#);
    let style = style_of(converted, "Shape");
    assert_ink(
        ink_of(&style, 0),
        probe(converted),
        "the shadow's ink takes the raster's leg",
    );
    assert_eq!(
        style["effects"][0]["color"],
        json!(hex_under(converted, ink_of(&style, 0)))
    );
    let assigned = profile_edit(doc, &b, r#"{"op":"assign"}"#);
    let style = style_of(assigned, "Shape");
    assert_eq!(ink_of(&style, 0), percent(E1), "Assign keeps the ink");
    let hex = hex_under(assigned, percent(E1));
    assert_eq!(style["effects"][0]["color"], json!(hex));
    assert_ne!(
        style_of(doc, "Shape")["effects"][0]["color"],
        json!(hex),
        "the profiles differ"
    );
    let rgb = to_rgb(assigned);
    assert_eq!(style_of(rgb, "Shape")["effects"][0]["color"], json!(hex));
    for doc in [doc, converted, assigned, rgb] {
        free(doc);
    }
}

fn word(bytes: &[u8]) -> u32 {
    crate::cmyk_structure_fixture::cmyk_word(bytes)
}

#[test]
fn word_86_round_trips_bit_exactly_and_a_fill_only_style_stays_84() {
    let doc = scene(true);
    let bytes = unsafe { &*doc }.encode_native(LIMIT, 0, None).unwrap();
    assert_eq!(word(&bytes), 86);
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
    assert_eq!(reopened.encode_native(LIMIT, 0, None).unwrap(), bytes);
    let reopened = Box::into_raw(Box::new(reopened));
    let (a, b): (Vec<Ink>, Vec<Ink>) = (
        composites(doc).iter().map(|s| s.map(f64::from)).collect(),
        composites(reopened)
            .iter()
            .map(|s| s.map(f64::from))
            .collect(),
    );
    assert_eq!(a, b);
    let mut relabelled = bytes.clone();
    relabelled[8..12].copy_from_slice(&84u32.to_le_bytes());
    assert_eq!(
        PrecisionDocument::parse_native(&relabelled, LIMIT, 0, None)
            .err()
            .as_deref(),
        Some("a CMYK layer style requires DRDC 86")
    );
    let filled = styled(
        layer(
            backdrop(W, H, 16, &COLUMNS),
            "Shape",
            E1,
            &hard_rect(W, H, (4, 4, 12, 12)),
        ),
        1,
        &json!({"fill_opacity":0.5}),
    );
    assert_eq!(
        word(&unsafe { &*filled }.encode_native(LIMIT, 0, None).unwrap()),
        84
    );
    for doc in [doc, reopened, filled] {
        free(doc);
    }
}
