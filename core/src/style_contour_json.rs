//! The contour keys of the style JSON (`style_contour`): the
//! contour object's parse and canonical write, the conditional writers of
//! the flag and range keys, and [`carries_contour`], the one predicate of
//! "this effect writes a contour-family key" that the writer and `.rz` word
//! 96 (`contour_native`) share.
//!
//! A contour object is `{"preset": id}` (an id of `style_names::
//! CONTOUR_PRESETS`; absent = `linear`) or `{"points": [{"x", "y",
//! "corner"?}, …]}`: 2..=16 points, each coordinate clamped into 0..=255 and
//! four-decimal quantized, stable-sorted by `x`, a duplicate `x` keeping the
//! first point; fewer than 2 distinct `x` values (or more than 16 points) is
//! `effects[i].contour.points must hold 2 to 16 points`. `points` wins over
//! `preset` when both are present. An unknown preset id is malformed when
//! strict and Linear when lenient. Every key is written only when it is not
//! at its default (Linear, false, 0.5), so every style authored without a
//! contour is byte-identical to what the writer before contours produced.

use serde_json::{Map, Value};

use crate::style::{Effect, Strictness};
use crate::style_contour::{
    Contour, ContourCurve, ContourPoint, ContourPreset, DEFAULT_RANGE, MAX_CONTOUR_POINTS,
};
use crate::style_json::{at, boolean, string_enum, Range};
use crate::style_json_write::number;
use crate::style_names::{enum_name, CONTOUR_PRESETS};

/// A custom point's coordinate range.
const POINT: Range = Range { lo: 0.0, hi: 255.0 };

/// `obj[key]` as a contour (module doc); Linear when absent.
pub(crate) fn parse_contour(
    obj: &Map<String, Value>,
    path: &str,
    key: &str,
    strictness: Strictness,
) -> Result<Contour, String> {
    let Some(value) = obj.get(key) else {
        return Ok(Contour::default());
    };
    let cpath = at(path, key);
    let o = value
        .as_object()
        .ok_or_else(|| format!("{cpath} must be an object"))?;
    let Some(points) = o.get("points") else {
        return Ok(Contour::Preset(string_enum(
            o,
            &cpath,
            "preset",
            ContourPreset::Linear,
            &CONTOUR_PRESETS,
            strictness,
        )?));
    };
    let ppath = at(&cpath, "points");
    let refusal = || format!("{ppath} must hold 2 to {MAX_CONTOUR_POINTS} points");
    let list = points
        .as_array()
        .ok_or_else(|| format!("{ppath} must be an array"))?;
    if !(2..=MAX_CONTOUR_POINTS).contains(&list.len()) {
        return Err(refusal());
    }
    let mut parsed = Vec::with_capacity(list.len());
    for (i, entry) in list.iter().enumerate() {
        let epath = format!("{ppath}[{i}]");
        let e = entry
            .as_object()
            .ok_or_else(|| format!("{epath} must be an object"))?;
        let coordinate = |k: &str| {
            e.get(k)
                .and_then(Value::as_f64)
                .filter(|f| f.is_finite())
                .map(|f| POINT.apply(f))
                .ok_or_else(|| format!("{} must be a number", at(&epath, k)))
        };
        parsed.push(ContourPoint {
            x: coordinate("x")?,
            y: coordinate("y")?,
            corner: boolean(e, &epath, "corner", false)?,
        });
    }
    parsed.sort_by(|a, b| a.x.total_cmp(&b.x));
    parsed.dedup_by(|later, earlier| later.x == earlier.x);
    ContourCurve::new(&parsed)
        .map(Contour::Custom)
        .ok_or_else(refusal)
}

/// The canonical contour object (module doc).
pub(crate) fn contour_value(contour: &Contour) -> Value {
    let mut o = Map::new();
    match contour {
        Contour::Preset(p) => {
            o.insert(
                "preset".into(),
                Value::from(enum_name(&CONTOUR_PRESETS, *p)),
            );
        }
        Contour::Custom(curve) => {
            let points = curve
                .points()
                .iter()
                .map(|p| {
                    let mut po = Map::new();
                    po.insert("x".into(), number(p.x));
                    po.insert("y".into(), number(p.y));
                    if p.corner {
                        po.insert("corner".into(), Value::from(true));
                    }
                    Value::Object(po)
                })
                .collect();
            o.insert("points".into(), Value::Array(points));
        }
    }
    Value::Object(o)
}

/// `key` = the contour, written only when it is not Linear.
pub(crate) fn put_contour(o: &mut Map<String, Value>, key: &str, contour: &Contour) {
    if !contour.is_linear() {
        o.insert(key.into(), contour_value(contour));
    }
}

/// `key` = true, written only when true.
pub(crate) fn put_flag(o: &mut Map<String, Value>, key: &str, flag: bool) {
    if flag {
        o.insert(key.into(), Value::from(true));
    }
}

/// `key` = the range, written only when it is not 0.5.
pub(crate) fn put_range(o: &mut Map<String, Value>, key: &str, range: f32) {
    if range != DEFAULT_RANGE {
        o.insert(key.into(), number(range));
    }
}

/// Whether `effect` writes any contour-family key (module doc), enabled or
/// not.
pub(crate) fn carries_contour(effect: &Effect) -> bool {
    let off = |c: &Contour, aa: bool| !c.is_linear() || aa;
    match effect {
        Effect::DropShadow(e) => off(&e.contour, e.anti_aliased),
        Effect::InnerShadow(e) => off(&e.contour, e.anti_aliased),
        Effect::Satin(e) => off(&e.contour, e.anti_aliased),
        Effect::OuterGlow(e) => off(&e.contour, e.anti_aliased) || e.range != DEFAULT_RANGE,
        Effect::InnerGlow(e) => off(&e.contour, e.anti_aliased) || e.range != DEFAULT_RANGE,
        Effect::BevelEmboss(e) => {
            off(&e.gloss_contour, e.gloss_anti_aliased)
                || e.contour_enabled
                || off(&e.contour, e.contour_anti_aliased)
                || e.contour_range != DEFAULT_RANGE
        }
        Effect::Stroke(_)
        | Effect::ColorOverlay(_)
        | Effect::GradientOverlay(_)
        | Effect::PatternOverlay(_) => false,
    }
}
