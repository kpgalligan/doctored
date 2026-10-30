//! Style PRESETS (the Window ▸ Styles panel): the one canonicalizer a
//! preset's style passes through, and the twelve built-in presets.
//!
//! A preset is a layer style detached from any layer, so it keeps nothing a
//! layer contributed: [`preset_style`] parses strictly (the setter's rules,
//! `style`'s module doc), then drops every CMYK ink (`without_inks`: a preset
//! is applied to documents of every mode, and an inkless slot converts at
//! composite time) and every pattern-fill anchor (`without_anchors`: the
//! anchor is where the SOURCE layer was made, and the setter anchors the fill
//! afresh on the layer it lands on). Its canonical JSON (`to_json`) is the
//! form the host's library stores and the form its decoder demands back
//! unchanged.
//!
//! The built-ins are original looks, not Adobe's default set, written as
//! style JSON literals and run through the same canonicalizer, so the list
//! [`defaults_json`] returns is canonical by construction. None carries
//! blending (fill opacity, Blend If, knockout): a built-in changes a layer's
//! effects only.

use serde_json::{json, Value};

use crate::style::{LayerStyle, Strictness};

/// `json` as a preset's style (module doc): strict parse, no inks, no
/// anchors, normalized. `Err` is the parser's sentence, or `not valid JSON`.
pub(crate) fn preset_style(json: &str) -> Result<LayerStyle, String> {
    let value: Value = serde_json::from_str(json).map_err(|e| format!("not valid JSON: {e}"))?;
    let style = LayerStyle::from_value(&value, Strictness::Strict)?;
    Ok(style.without_inks().without_anchors().normalized())
}

/// The twelve built-ins as `(name, style JSON)`, in panel order.
const DEFAULTS: [(&str, &str); 12] = [
    (
        "Soft Drop",
        r##"{"effects":[{"type":"drop_shadow","color":"#000000","opacity":0.35,
            "angle":90,"use_global_light":false,"distance":6,"size":14}]}"##,
    ),
    (
        "Lifted Card",
        r##"{"effects":[
            {"type":"drop_shadow","opacity":0.22,"angle":90,"use_global_light":false,
             "distance":2,"size":3},
            {"type":"drop_shadow","opacity":0.18,"angle":90,"use_global_light":false,
             "distance":12,"size":24},
            {"type":"stroke","position":"inside","size":1,"color":"#d5d9de"}]}"##,
    ),
    (
        "Hairline Frame",
        r##"{"effects":[{"type":"stroke","position":"outside","size":1,
            "color":"#262a30","opacity":0.9}]}"##,
    ),
    (
        "Inset Panel",
        r##"{"effects":[
            {"type":"color_overlay","color":"#e6e9ed"},
            {"type":"inner_shadow","opacity":0.45,"angle":90,"use_global_light":false,
             "distance":3,"size":6}]}"##,
    ),
    (
        "Pressed Button",
        r##"{"effects":[
            {"type":"stroke","position":"inside","size":1,"color":"#8f969e"},
            {"type":"gradient_overlay","gradient":{"angle":90,"stops":[
                {"position":0,"color":"#c9ced4","opacity":1},
                {"position":1,"color":"#f2f4f6","opacity":1}]}},
            {"type":"inner_shadow","opacity":0.5,"angle":90,"use_global_light":false,
             "distance":2,"size":4}]}"##,
    ),
    (
        "Neon Edge",
        r##"{"effects":[
            {"type":"outer_glow","blend":"normal","color":"#2ee88a","opacity":0.8,
             "spread":0.1,"size":16},
            {"type":"stroke","position":"center","size":2,"color":"#2ee88a"},
            {"type":"color_overlay","color":"#0d2a1c"},
            {"type":"inner_glow","blend":"screen","color":"#9cffd0","opacity":0.7,
             "size":6}]}"##,
    ),
    (
        "Warm Glow",
        r##"{"effects":[
            {"type":"outer_glow","blend":"normal","color":"#ff9440","opacity":0.65,
             "spread":0.05,"size":22},
            {"type":"color_overlay","color":"#ffd9a8"}]}"##,
    ),
    (
        "Embossed Label",
        r##"{"effects":[
            {"type":"drop_shadow","color":"#ffffff","blend":"normal","opacity":0.6,
             "angle":90,"use_global_light":false,"distance":1,"size":0},
            {"type":"color_overlay","color":"#b9bec4"},
            {"type":"bevel_emboss","style":"inner_bevel","depth":1.5,"size":3,
             "soften":1,"angle":120,"use_global_light":false,"altitude":35}]}"##,
    ),
    (
        "Brushed Metal",
        r##"{"effects":[
            {"type":"pattern_overlay","blend":"overlay","opacity":0.18,
             "pattern":{"preset":"stripes"},"scale":0.5},
            {"type":"gradient_overlay","gradient":{"angle":90,"stops":[
                {"position":0,"color":"#7b8188","opacity":1},
                {"position":0.5,"color":"#e4e7ea","opacity":1},
                {"position":1,"color":"#8a9097","opacity":1}]}},
            {"type":"bevel_emboss","style":"inner_bevel","depth":1,"size":3,
             "angle":120,"use_global_light":false}]}"##,
    ),
    (
        "Glass Pill",
        r##"{"effects":[
            {"type":"drop_shadow","opacity":0.25,"angle":90,"use_global_light":false,
             "distance":3,"size":8},
            {"type":"stroke","position":"inside","size":1,"color":"#ffffff","opacity":0.6},
            {"type":"gradient_overlay","blend":"screen","gradient":{"angle":90,"stops":[
                {"position":0,"color":"#ffffff","opacity":0},
                {"position":0.55,"color":"#ffffff","opacity":0.15},
                {"position":1,"color":"#ffffff","opacity":0.7}]}},
            {"type":"color_overlay","color":"#5b8fd6"},
            {"type":"inner_glow","blend":"screen","color":"#d8e8ff","opacity":0.5,
             "size":8}]}"##,
    ),
    (
        "Sticker Cut",
        r##"{"effects":[
            {"type":"drop_shadow","opacity":0.3,"angle":90,"use_global_light":false,
             "distance":4,"size":6},
            {"type":"stroke","position":"outside","size":7,"color":"#3a3f45"},
            {"type":"stroke","position":"outside","size":5,"color":"#ffffff"}]}"##,
    ),
    (
        "Sunset Fade",
        r##"{"effects":[
            {"type":"drop_shadow","color":"#5a2a1a","opacity":0.3,"angle":90,
             "use_global_light":false,"distance":4,"size":10},
            {"type":"gradient_overlay","gradient":{"angle":90,"stops":[
                {"position":0,"color":"#e8505b","opacity":1},
                {"position":0.5,"color":"#f99a5b","opacity":1},
                {"position":1,"color":"#ffd56b","opacity":1}]}}]}"##,
    ),
];

/// The built-ins as `[{"name", "style"}]` in panel order, each style the
/// canonical object [`preset_style`] produces (module doc). A literal that
/// did not parse would be left out; `style_presets` tests that all twelve do.
pub(crate) fn defaults_json() -> String {
    let list: Vec<Value> = DEFAULTS
        .iter()
        .filter_map(|(name, literal)| {
            let style = preset_style(literal).ok()?;
            let style: Value = serde_json::from_str(&style.to_json()).ok()?;
            Some(json!({"name": name, "style": style}))
        })
        .collect();
    Value::Array(list).to_string()
}
