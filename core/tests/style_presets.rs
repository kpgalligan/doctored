//! Style PRESETS (the Styles panel) through the C API:
//! `dr_style_preset_defaults_json` (the twelve built-ins: strict, canonical,
//! non-identity, uniquely named, inkless and anchorless) and
//! `dr_style_preset_canonical` (the one canonicalizer: strict parse, every
//! CMYK ink and every pattern-fill anchor dropped, a malformed style refused
//! by the key it names).

use std::collections::HashSet;
use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::ffi_style::{dr_style_preset_canonical, dr_style_preset_defaults_json};
use doctored_core::style::LayerStyle;
use serde_json::Value;

use crate::common::take_err_string;

fn canonical(json: &str) -> Result<String, String> {
    let style = CString::new(json).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_style_preset_canonical(style.as_ptr(), &mut err) };
    if raw.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(take_err_string(raw))
    }
}

fn defaults() -> Vec<Value> {
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_style_preset_defaults_json(&mut err) };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    let list: Value = serde_json::from_str(&take_err_string(raw)).expect("defaults JSON");
    list.as_array().expect("an array").clone()
}

#[test]
fn the_twelve_defaults_are_strict_canonical_non_identity_and_uniquely_named() {
    let list = defaults();
    assert_eq!(list.len(), 12, "{list:?}");
    let mut names = HashSet::new();
    for entry in &list {
        let name = entry["name"].as_str().expect("a name");
        assert!(!name.is_empty() && !name.contains('/'), "{name}");
        assert!(names.insert(name.to_string()), "{name} repeats");
        let written = entry["style"].to_string();
        let style = LayerStyle::from_json(&written)
            .unwrap_or_else(|e| panic!("{name} does not parse strictly: {e}"));
        assert!(!style.is_identity(), "{name} renders nothing");
        // Canonical: the writer's own form, and the canonicalizer's fixed point.
        assert_eq!(style.to_json(), written, "{name} is not canonical");
        assert_eq!(canonical(&written).unwrap(), written, "{name}");
        // No blending, no inks, no anchors: a built-in changes effects only.
        assert_eq!(entry["style"]["fill_opacity"], Value::from(1.0), "{name}");
        assert!(entry["style"]["blend_if"].is_null(), "{name}");
        assert!(!written.contains("\"ink\""), "{name}");
        assert!(!written.contains("\"anchor\""), "{name}");
    }
    let expected = [
        "Soft Drop",
        "Lifted Card",
        "Hairline Frame",
        "Inset Panel",
        "Pressed Button",
        "Neon Edge",
        "Warm Glow",
        "Embossed Label",
        "Brushed Metal",
        "Glass Pill",
        "Sticker Cut",
        "Sunset Fade",
    ];
    let got: Vec<&str> = list.iter().map(|e| e["name"].as_str().unwrap()).collect();
    assert_eq!(got, expected);
}

#[test]
fn sticker_cut_holds_two_stacked_strokes_and_a_shadow() {
    let list = defaults();
    let sticker = list
        .iter()
        .find(|e| e["name"] == "Sticker Cut")
        .expect("Sticker Cut");
    let types: Vec<&str> = sticker["style"]["effects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, ["drop_shadow", "stroke", "stroke"]);
}

#[test]
fn the_canonicalizer_drops_inks_and_anchors_and_keeps_everything_else() {
    let input = r##"{"effects":[
        {"type":"drop_shadow","color":"#102030","ink":[0.1,0.2,0.3,0.4],"size":9},
        {"type":"pattern_overlay","pattern":{"preset":"dots"},"anchor":[5,7],"phase":[2,3]},
        {"type":"stroke","fill_type":"pattern","pattern_fill":{"pattern":{"preset":"grid"},
         "anchor":[1,1]}}]}"##;
    let out = canonical(input).expect("a valid style");
    let value: Value = serde_json::from_str(&out).unwrap();
    for effect in value["effects"].as_array().unwrap() {
        assert!(effect.get("ink").is_none(), "{effect}");
        assert!(effect.get("anchor").is_none(), "{effect}");
        if let Some(fill) = effect.get("pattern_fill") {
            assert!(fill.get("anchor").is_none(), "{fill}");
        }
    }
    // Everything else is the strict parser's canonical form of the input
    // with those keys removed.
    let stripped = r##"{"effects":[
        {"type":"drop_shadow","color":"#102030","size":9},
        {"type":"pattern_overlay","pattern":{"preset":"dots"},"phase":[2,3]},
        {"type":"stroke","fill_type":"pattern","pattern_fill":{"pattern":{"preset":"grid"}}}]}"##;
    assert_eq!(out, LayerStyle::from_json(stripped).unwrap().to_json());
    assert_eq!(canonical(&out).unwrap(), out, "a fixed point");
}

#[test]
fn a_malformed_style_is_refused_by_the_key_it_names() {
    let err = canonical(r#"{"effects":[{"type":"drop_shadow","size":"big"}]}"#).unwrap_err();
    assert_eq!(err, "style: effects[0].size must be a number");
    let err = canonical("not json").unwrap_err();
    assert!(err.starts_with("style: not valid JSON"), "{err}");
    let err = canonical(r#"{"effects":[{"type":"sparkle"}]}"#).unwrap_err();
    assert!(err.contains("effects[0]"), "{err}");
}
