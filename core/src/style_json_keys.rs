//! The style JSON's KEY TABLE: every key the strict parser
//! reads, object by object, so the two style SETTERS (`dr_doc_set_layer_style`
//! and the typed `layer_style` edit) refuse a key it does not name — "unknown
//! layer style field effects[0].blend_mode" — instead of dropping it, as the
//! typed Blend If edit already does ("unknown native blend-if field").
//!
//! The table is derived from the parser function by function: `style_json`
//! (the root, `blend_if` and its `extra` ranges, every effect kind, the
//! gradient and its stops, the wide `linear` slots and the CMYK `ink` slots),
//! `style_contour_json` (a contour and its points) and `style_pattern_fill`
//! (a pattern fill, a bevel's texture, the pattern object). It also names
//! `version` and every key `style_json_write` emits, so the canonical JSON
//! the core hands out (`anchor` included) is always accepted back. A key the
//! parser reads but discards (a texture's `angle`, a glow gradient's
//! `dither` and `smoothness`) is still a schema key.
//!
//! Only the setters run [`unknown_key`]; the parser itself stays tolerant,
//! so the lenient readers (the DRDC reader, `adjust_map`), PSD and `.asl`
//! import, presets and `asl_write` never see this check.

use serde_json::{Map, Value};

const ROOT: &[&str] = &[
    "version",
    "fill_opacity",
    "blend_if",
    "knockout",
    "blend_interior_as_group",
    "effects",
];
const BLEND_IF: &[&str] = &["channel", "this_layer", "underlying", "extra"];
const BLEND_IF_RANGE: &[&str] = &["channel", "this_layer", "underlying"];
const GRADIENT: &[&str] = &[
    "stops",
    "style",
    "angle",
    "scale",
    "reverse",
    "align_with_layer",
    "dither",
    "smoothness",
];
const STOP: &[&str] = &[
    "position",
    "color",
    "opacity",
    "ink",
    "midpoint",
    "opacity_midpoint",
];
const CONTOUR: &[&str] = &["preset", "points"];
const POINT: &[&str] = &["x", "y", "corner"];
const PATTERN: &[&str] = &["preset", "name", "width", "height", "rgba"];
const FILL: &[&str] = &[
    "pattern",
    "angle",
    "scale",
    "link_with_layer",
    "phase",
    "anchor",
];
const TEXTURE: &[&str] = &[
    "pattern",
    "angle",
    "scale",
    "link_with_layer",
    "phase",
    "anchor",
    "depth",
    "invert",
];

const DROP_SHADOW: &[&str] = &[
    "type",
    "enabled",
    "blend",
    "color",
    "ink",
    "linear",
    "opacity",
    "angle",
    "use_global_light",
    "distance",
    "spread",
    "size",
    "layer_knocks_out",
    "contour",
    "anti_aliased",
    "noise",
];
const INNER_SHADOW: &[&str] = &[
    "type",
    "enabled",
    "blend",
    "color",
    "ink",
    "linear",
    "opacity",
    "angle",
    "use_global_light",
    "distance",
    "choke",
    "size",
    "contour",
    "anti_aliased",
    "noise",
];
const OUTER_GLOW: &[&str] = &[
    "type",
    "enabled",
    "blend",
    "color",
    "ink",
    "linear",
    "opacity",
    "spread",
    "size",
    "contour",
    "anti_aliased",
    "range",
    "noise",
    "fill_type",
    "gradient",
    "jitter",
];
const INNER_GLOW: &[&str] = &[
    "type",
    "enabled",
    "blend",
    "color",
    "ink",
    "linear",
    "opacity",
    "choke",
    "size",
    "source",
    "contour",
    "anti_aliased",
    "range",
    "noise",
    "fill_type",
    "gradient",
    "jitter",
];
const STROKE: &[&str] = &[
    "type",
    "enabled",
    "blend",
    "opacity",
    "size",
    "position",
    "fill_type",
    "color",
    "ink",
    "linear",
    "gradient",
    "pattern_fill",
];
const COLOR_OVERLAY: &[&str] = &[
    "type", "enabled", "blend", "color", "ink", "linear", "opacity",
];
const GRADIENT_OVERLAY: &[&str] = &["type", "enabled", "blend", "opacity", "gradient"];
const PATTERN_OVERLAY: &[&str] = &[
    "type",
    "enabled",
    "blend",
    "opacity",
    "pattern",
    "angle",
    "scale",
    "link_with_layer",
    "phase",
    "anchor",
];
const BEVEL_EMBOSS: &[&str] = &[
    "type",
    "enabled",
    "style",
    "technique",
    "depth",
    "direction",
    "size",
    "soften",
    "angle",
    "use_global_light",
    "altitude",
    "highlight_blend",
    "highlight_color",
    "highlight_ink",
    "highlight_linear",
    "highlight_opacity",
    "shadow_blend",
    "shadow_color",
    "shadow_ink",
    "shadow_linear",
    "shadow_opacity",
    "gloss_contour",
    "gloss_anti_aliased",
    "contour_enabled",
    "contour",
    "contour_anti_aliased",
    "contour_range",
    "texture_enabled",
    "texture",
];
const SATIN: &[&str] = &[
    "type",
    "enabled",
    "blend",
    "color",
    "ink",
    "linear",
    "opacity",
    "angle",
    "distance",
    "size",
    "invert",
    "contour",
    "anti_aliased",
];

/// One object of the schema: its keys, and which of them hold a nested
/// object (or a list of them) that is checked in turn.
#[derive(Clone, Copy)]
enum Shape {
    Root,
    BlendIf,
    Range,
    Effect,
    Gradient,
    Stop,
    Contour,
    Point,
    Pattern,
    Fill,
    Texture,
}

impl Shape {
    /// The keys `object` may hold; `None` for an effect whose `type` the
    /// parser does not know (it has already refused one in strict mode).
    fn keys(self, object: &Map<String, Value>) -> Option<&'static [&'static str]> {
        Some(match self {
            Shape::Root => ROOT,
            Shape::BlendIf => BLEND_IF,
            Shape::Range => BLEND_IF_RANGE,
            Shape::Gradient => GRADIENT,
            Shape::Stop => STOP,
            Shape::Contour => CONTOUR,
            Shape::Point => POINT,
            Shape::Pattern => PATTERN,
            Shape::Fill => FILL,
            Shape::Texture => TEXTURE,
            Shape::Effect => match object.get("type").and_then(Value::as_str)? {
                "drop_shadow" => DROP_SHADOW,
                "inner_shadow" => INNER_SHADOW,
                "outer_glow" => OUTER_GLOW,
                "inner_glow" => INNER_GLOW,
                "stroke" => STROKE,
                "color_overlay" => COLOR_OVERLAY,
                "gradient_overlay" => GRADIENT_OVERLAY,
                "pattern_overlay" => PATTERN_OVERLAY,
                "bevel_emboss" => BEVEL_EMBOSS,
                "satin" => SATIN,
                _ => return None,
            },
        })
    }

    /// The shape of the value under `key`, when it is a nested object (or a
    /// list of objects) the parser reads.
    fn child(self, key: &str) -> Option<Shape> {
        match (self, key) {
            (Shape::Root, "blend_if") => Some(Shape::BlendIf),
            (Shape::Root, "effects") => Some(Shape::Effect),
            (Shape::BlendIf, "extra") => Some(Shape::Range),
            (Shape::Effect, "contour" | "gloss_contour") => Some(Shape::Contour),
            (Shape::Effect, "gradient") => Some(Shape::Gradient),
            (Shape::Effect, "pattern_fill") => Some(Shape::Fill),
            (Shape::Effect, "texture") => Some(Shape::Texture),
            (Shape::Effect | Shape::Fill | Shape::Texture, "pattern") => Some(Shape::Pattern),
            (Shape::Gradient, "stops") => Some(Shape::Stop),
            (Shape::Contour, "points") => Some(Shape::Point),
            _ => None,
        }
    }
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

fn walk(value: &Value, path: &str, shape: Shape) -> Option<String> {
    match value {
        Value::Array(list) => list
            .iter()
            .enumerate()
            .find_map(|(i, entry)| walk(entry, &format!("{path}[{i}]"), shape)),
        Value::Object(object) => {
            let keys = shape.keys(object)?;
            for (key, child) in object {
                let at = join(path, key);
                if !keys.contains(&key.as_str()) {
                    return Some(at);
                }
                if let Some(found) = shape.child(key).and_then(|s| walk(child, &at, s)) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

/// The first key the style schema does not name, as its parser path
/// (`blend_mode`, `effects[0].blend_mode`,
/// `effects[2].gradient.stops[1].colour`), else `None`. Run by the setters
/// after `LayerStyle::from_value` accepted `value`.
pub(crate) fn unknown_key(value: &Value) -> Option<String> {
    walk(value, "", Shape::Root)
}
