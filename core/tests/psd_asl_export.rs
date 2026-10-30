//! Photoshop style libraries WRITTEN (`.asl`) through `dr_asl_encode`,
//! read back through `dr_asl_decode`.
//!
//! The oracle for a written preset is the preset itself: its style
//! canonicalized as a preset (`dr_style_preset_canonical`) and its blending.
//! A library that `dr_asl_decode` reads back to exactly those is a library
//! the reader and the writer agree on, effect keys, patterns, the blending
//! options and Blend If included. A wide colour is held to declared bounds:
//! inside sRGB's gamut each `linear` component within 1e-5 of the one
//! written (the library stores sRGB numbers as doubles from f32), outside it
//! its clamped sRGB byte and no `linear` (a library carries no document
//! space; the recorded residual).

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::ffi_style::{dr_asl_decode, dr_asl_encode, dr_style_preset_canonical};
use serde_json::{json, Value};

use crate::common::take_err_string;

// ------------------------------------------------------------- C plumbing --

fn encode(presets: &Value) -> Result<(Vec<u8>, Vec<String>), String> {
    let c = CString::new(presets.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_asl_encode(c.as_ptr(), &mut err) };
    if raw.is_null() {
        return Err(take_err_string(err));
    }
    let out: Value = serde_json::from_str(&take_err_string(raw)).unwrap();
    let skipped = out["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    Ok((base64(out["asl"].as_str().unwrap()), skipped))
}

fn decode(bytes: &[u8]) -> Value {
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_asl_decode(bytes.as_ptr(), bytes.len(), &mut err) };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(raw)).unwrap()
}

fn canonical(style: &Value) -> Value {
    let c = CString::new(style.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_style_preset_canonical(c.as_ptr(), &mut err) };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(raw)).unwrap()
}

/// Standard base64 (RFC 4648, padded) decoded, written here so the test
/// does not lean on the crate's codec.
fn base64(text: &str) -> Vec<u8> {
    let value = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            other => panic!("not base64: {other}"),
        }
    };
    let mut out = Vec::new();
    for chunk in text.as_bytes().chunks(4) {
        let pad = chunk.iter().filter(|&&c| c == b'=').count();
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= if c == b'=' { 0 } else { value(c) } << (18 - 6 * i);
        }
        let bytes = n.to_be_bytes();
        out.extend_from_slice(&bytes[1..4 - pad]);
    }
    out
}

fn srgb_decode(v: f64) -> f64 {
    if v <= 0.040_45 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

// ---------------------------------------------------------------- presets --

const TILE: &str = "/wAA/wD/AP8AAP//////gA==";

fn presets() -> Value {
    json!([
        {"name": "Stacked", "blending": null, "style": {"effects": [
            {"type":"drop_shadow","color":"#112233","opacity":0.5,"angle":45,
             "use_global_light":true,"distance":6,"size":9},
            {"type":"drop_shadow","color":"#445566","opacity":0.3,"angle":-60,
             "use_global_light":false,"distance":2,"size":3,"noise":0.2},
            {"type":"inner_shadow","color":"#223344","angle":45,"use_global_light":true,
             "distance":3,"choke":0.15,"size":5},
            {"type":"stroke","position":"inside","size":2,"color":"#d5d9de"},
            {"type":"stroke","position":"outside","size":4,"fill_type":"gradient",
             "gradient":{"stops":[{"position":0,"color":"#ff0000","opacity":1},
                                  {"position":1,"color":"#0000ff","opacity":0.25}],
                         "style":"reflected","angle":30,"scale":1.4}},
            {"type":"color_overlay","blend":"soft_light","color":"#00ff88","opacity":0.7},
            {"type":"outer_glow","color":"#ffee00","opacity":0.4,"spread":0.1,"size":11,
             "contour":{"preset":"cone"},"range":0.4},
            {"type":"inner_glow","color":"#ccddee","choke":0.2,"size":7,"source":"center",
             "jitter":0.3},
            {"type":"satin","color":"#334455","opacity":0.45,"angle":25,"distance":12,
             "size":18,"invert":false}]}},
        {"name": "Patterned", "blending": {"mode":"multiply","opacity":0.7}, "style": {
            "fill_opacity": 0.4, "knockout": "shallow", "blend_interior_as_group": true,
            "blend_if": {"channel":"red","this_layer":[0,10,240,255],"underlying":[5,15,235,250]},
            "effects": [
                {"type":"pattern_overlay","pattern":{"preset":"dots"},"scale":1.5,"angle":20},
                {"type":"bevel_emboss","style":"pillow_emboss","depth":2.5,"size":8,"soften":3,
                 "angle":45,"use_global_light":true,"altitude":50,
                 "highlight_color":"#fafbfc","shadow_color":"#030201",
                 "texture_enabled":true,
                 "texture":{"pattern":{"name":"Tile","width":2,"height":2,"rgba":TILE},
                            "depth":-2.5,"invert":true}}]}},
        {"name": "Gray Blend If", "blending": {"mode":"screen","opacity":1}, "style": {
            "blend_if": {"channel":"gray","this_layer":[20,30,200,220],"underlying":[0,0,255,255]},
            "effects": [{"type":"color_overlay","color":"#808080","opacity":0.5}]}}
    ])
}

#[test]
fn a_written_library_reads_back_as_its_canonical_presets() {
    let input = presets();
    let (bytes, skipped) = encode(&input).expect("a library");
    assert!(skipped.is_empty(), "{skipped:?}");
    assert_eq!(&bytes[..8], b"\x00\x028BSL\x00\x03");
    let report = decode(&bytes);
    let styles = report["styles"].as_array().unwrap();
    assert_eq!(styles.len(), 3);
    for (got, want) in styles.iter().zip(input.as_array().unwrap()) {
        assert_eq!(got["name"], want["name"]);
        assert_eq!(got["dropped"], json!([]), "{}", got["name"]);
        assert_eq!(got["style"], canonical(&want["style"]), "{}", got["name"]);
        assert_eq!(got["blending"]["mode"], want["blending"]["mode"]);
        assert_eq!(
            got["blending"]["opacity"].as_f64(),
            want["blending"]["opacity"].as_f64(),
            "{}",
            got["name"]
        );
    }
    // The global-light effects keep their OWN stored angle and altitude.
    let bevel = styles[1]["style"]["effects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["type"] == "bevel_emboss")
        .unwrap()
        .clone();
    assert_eq!(bevel["angle"].as_f64(), Some(45.0));
    assert_eq!(bevel["altitude"].as_f64(), Some(50.0));
    assert_eq!(bevel["use_global_light"], true);
    // The same presets write the same bytes (stable ids).
    assert_eq!(encode(&input).unwrap().0, bytes);
}

#[test]
fn a_multichannel_blend_if_writes_every_range_and_reads_back() {
    let blend_if = json!({"channel":"gray","this_layer":[10,40,200,230],
        "underlying":[20,60,180,240],
        "extra":[{"channel":"red","this_layer":[5,30,220,250],"underlying":[3,25,235,252]}]});
    let input = json!([{"name":"Gray and red","blending":null,"style":{
        "blend_if": blend_if, "effects":[{"type":"drop_shadow"}]}}]);
    let (bytes, skipped) = encode(&input).expect("a library");
    assert!(skipped.is_empty(), "{skipped:?}");
    let report = decode(&bytes);
    let got = &report["styles"][0];
    assert_eq!(got["dropped"], json!([]), "{got}");
    assert_eq!(got["style"], canonical(&input[0]["style"]));
    assert_eq!(got["style"]["blend_if"], blend_if);
    // A CMYK range among several still skips the preset.
    let plated = json!([
        {"name":"Plated","style":{"blend_if":{"channel":"gray","this_layer":[0,20,255,255],
            "extra":[{"channel":"cyan","this_layer":[0,20,255,255]}]},
            "effects":[{"type":"drop_shadow"}]}},
        {"name":"Kept","style":{"effects":[{"type":"drop_shadow"}]}}
    ]);
    let (_, skipped) = encode(&plated).expect("one preset is left");
    assert_eq!(
        skipped,
        vec!["Plated: Blend If on a CMYK plate cannot be written to a style library".to_string()]
    );
}

fn wide_preset(linear: [f64; 3]) -> Value {
    json!([{"name":"Wide","blending":null,"style":{"effects":[
        {"type":"color_overlay","color":"#000000","linear":linear},
        {"type":"bevel_emboss","shadow_linear":linear}]}}])
}

fn effects_of(report: &Value) -> Vec<Value> {
    report["styles"][0]["style"]["effects"]
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn an_in_gamut_wide_colour_returns_within_its_bound() {
    let numbers = [100.3 / 255.0, 50.5 / 255.0, 200.7 / 255.0];
    let linear = numbers.map(srgb_decode);
    let input = wide_preset(linear);
    let want = canonical(&input[0]["style"]);
    let (bytes, _) = encode(&input).unwrap();
    let report = decode(&bytes);
    assert_eq!(report["styles"][0]["dropped"], json!([]));
    let got = effects_of(&report);
    for (key, color_key, index) in [
        ("linear", "color", 0usize),
        ("shadow_linear", "shadow_color", 1),
    ] {
        let g = &got[index];
        let w = &want["effects"][index];
        assert_eq!(g[color_key], w[color_key], "{key}");
        let back = g[key].as_array().unwrap_or_else(|| panic!("{key}: {g}"));
        for c in 0..3 {
            let (a, b) = (back[c].as_f64().unwrap(), w[key][c].as_f64().unwrap());
            assert!((a - b).abs() <= 1e-5, "{key}[{c}]: {a} against {b}");
        }
    }
}

#[test]
fn an_out_of_gamut_wide_colour_returns_as_its_clamped_byte() {
    let input = wide_preset([1.2249, -0.0421, -0.0196]);
    let (bytes, _) = encode(&input).unwrap();
    let got = effects_of(&decode(&bytes));
    let overlay = got.iter().find(|e| e["type"] == "color_overlay").unwrap();
    assert_eq!(overlay["color"], "#ff0000");
    assert!(overlay.get("linear").is_none(), "{overlay}");
    let bevel = got.iter().find(|e| e["type"] == "bevel_emboss").unwrap();
    assert_eq!(bevel["shadow_color"], "#ff0000");
    assert!(bevel.get("shadow_linear").is_none(), "{bevel}");
}

#[test]
fn presets_that_cannot_be_written_are_named_and_the_rest_written() {
    let input = json!([
        {"name":"Far satin","style":{"effects":[{"type":"satin","distance":300}]}},
        {"name":"Thin stroke","style":{"effects":[{"type":"stroke","size":0.5}]}},
        {"name":"Broken","style":{"effects":[{"type":"drop_shadow","size":"big"}]}},
        {"name":"Plated","style":{"blend_if":{"channel":"cyan","this_layer":[0,20,255,255]},
            "effects":[{"type":"drop_shadow"}]}},
        {"name":"Bad blend","style":{"effects":[{"type":"drop_shadow"}]},
         "blending":{"mode":"sideways","opacity":1}},
        {"name":"Kept","style":{"effects":[{"type":"drop_shadow"}]}}
    ]);
    let (bytes, skipped) = encode(&input).expect("one preset is left");
    assert_eq!(
        skipped,
        vec![
            "Broken: effects[0].size must be a number".to_string(),
            "Bad blend: blending.mode must be a blend mode name".to_string(),
            "Far satin: satin distance is outside Photoshop's 1\u{2013}250 px range".to_string(),
            "Thin stroke: stroke size is below Photoshop's 1 px minimum".to_string(),
            "Plated: Blend If on a CMYK plate cannot be written to a style library".to_string(),
        ]
    );
    let report = decode(&bytes);
    let names: Vec<&str> = report["styles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Kept"]);
}

#[test]
fn null_malformed_empty_and_oversized_inputs_are_refused() {
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_asl_encode(ptr::null(), &mut err) };
    assert!(raw.is_null());
    assert!(!take_err_string(err).is_empty());
    let message = encode(&json!({"name":"x"})).unwrap_err();
    assert_eq!(
        message,
        "presets must be an array of {name, style, blending}"
    );
    let message = encode(&json!([])).unwrap_err();
    assert_eq!(message, "there are no style presets to write");
    let message =
        encode(&json!([{"name":"Far","style":{"effects":[{"type":"satin","distance":300}]}}]))
            .unwrap_err();
    assert!(
        message.starts_with("no style preset can be written to a style library"),
        "{message}"
    );
    let many: Vec<Value> = (0..1001)
        .map(|i| json!({"name": format!("P{i}"), "style": {"effects":[{"type":"drop_shadow"}]}}))
        .collect();
    let message = encode(&Value::Array(many)).unwrap_err();
    assert_eq!(
        message,
        "a style library holds at most 1000 styles, not 1001"
    );
    // A full thousand is written and reads back.
    let thousand: Vec<Value> = (0..1000)
        .map(|i| json!({"name": format!("P{i}"), "style": {"effects":[{"type":"drop_shadow"}]}}))
        .collect();
    let (bytes, _) = encode(&Value::Array(thousand)).unwrap();
    assert_eq!(decode(&bytes)["styles"].as_array().unwrap().len(), 1000);
}
