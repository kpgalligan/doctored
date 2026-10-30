//! Photoshop's Gradient Fill layer record `GdFl`, read and
//! written: every `GdFl` body — its decode, the settings a Doctored gradient
//! fill cannot hold (named as losses), the layer description, the render
//! components, the export parse, the encoded body and the report sentences.
//! The record walk, the forms and the admission are `psd_fill_layer`'s, which
//! dispatches here.
//!
//! # The body
//!
//! `u32 16` then a descriptor of class `null`: `gradientsInterpolationMethod`
//! (enum `gradientInterpolationMethodType`, Classic [`CLASSIC`]), `Angl`,
//! `Type`, `Scl `, `Grad` (a `Grdn` holding `Nm  `, `GrdF` `CstS`, `Intr`
//! (Smoothness, 4096 at 100 %), the `Clrs` colour stops and the `Trns`
//! transparency stops, each with `Lctn` and `Mdpn`) and, where they differ
//! from Photoshop's defaults, `Rvrs` (false), `Algn` (true), `Dthr` (false)
//! and `Ofst` (0, 0). take1 read exactly the first five keys
//! on every Photoshop-written record.
//!
//! # Reading
//!
//! [`decode`] reads the stops and the gradient's own keys through
//! `psd_layer_effects_fx::gradient` — the ONE `Grdn` reader, whose decline
//! reasons (not a custom gradient, the stop count, transparency stops that do
//! not match the colour stops, a colour of the wrong model) stay its own —
//! with each stop's off-grid colour kept as its wide `linear`. The
//! stops are sorted (stably) by position, as the style parser sorts them, and
//! every per-stop list here follows that order. In an RGB file a stop read off
//! the sRGB byte grid makes the carry keep every stop's linear sRGB colour
//! ([`GradientCarry::linear`], the others from their hex); in a CMYK file
//! every stop must be a `CMYC` ink ([`NOT_INK`]). The gradient then parses
//! Strict through `style_json::gradient` (its error is the decline reason;
//! Shape Burst is a stroke-only style and declines there), with its
//! Smoothness (`Intr`, read by the `Grdn` reader), its stops'
//! midpoints (`Mdpn` on `Clrt` and `TrnS`, read by the `Grdn` reader,
//! which gives our stop `i` Photoshop stop `i + 1`'s)
//! and its Align with Layer (`Algn`, absent = true) as
//! `align_with_layer`; `dither` is false. A stop's `Clry` type is not read.
//!
//! What the fill cannot hold is named, never applied
//! ([`GradientCarry::losses`]): an interpolation method other than Classic,
//! an Offset and Dither.
//!
//! # Description and render
//!
//! [`describe`] writes the host's `FillLayerPayload`: version 1 RGB
//! `"fill":"gradient"` with `stop_linears` (one linear sRGB triple per stop)
//! when the carry holds them, or version 2 `"ink":"cmyk"` with `stop_inks`,
//! each stop's `#rrggbb` its ink's sRGB under the policy
//! (`cmyk_style::hex_of`). [`component`] and [`cmyk_component`] are the
//! host's own drawing: one `gradient` component whose `colors` are the stops'
//! hex / 255 and whose `linear` is the carry's, or whose `inks` are its inks,
//! with the aligned layer's `box` (`psd_fill_layer::align_frame`, the mask's
//! coverage bounds by `fill_align_box::align_box`) when one is given.
//!
//! # Writing
//!
//! [`exported`] recognizes a version-1 RGB or version-2 CMYK gradient fill
//! description; [`items`] is its body: the Classic method, then
//! `psd_layer_effects_fx::gradient_items` (the ONE `Grdn` writer) with the
//! description's `Algn` and `Intr`, each stop's colour in the document's numbers (a wide stop
//! through its linear, a CMYK stop as its `CMYC`). Photoshop's form is
//! `psd_fill_layer`'s: pixels at 8 bits, pixel-less at 16
//! (`psd_fill_layer::Exported::pixel_less`).

use serde_json::{Map, Value};

use crate::authored_color::{srgb_linear, LINEAR_LIMIT};
use crate::cmyk_document::CmykPolicy;
use crate::psd_descriptor::{Descriptor, Item};
use crate::psd_layer_effects_fx::{gradient, gradient_items, FxColours};
use crate::style::{GradientFill, Strictness};

/// The record's adjustment key.
pub(crate) const KEY: [u8; 4] = *b"GdFl";

/// The record's kind in the import report.
pub(crate) const KIND: &str = "Gradient Fill";

/// A CMYK file's `GdFl` with a stop that is not a `CMYC` ink.
pub(crate) const NOT_INK: &str = "gradient stop colour is not a CMYK colour";

/// A `GdFl` with no `Grad`.
const NO_GRADIENT: &str = "gradient fill holds no gradient";

/// The interpolation method key and its enum type.
const METHOD: &[u8] = b"gradientsInterpolationMethod";
const METHOD_TYPE: &[u8] = b"gradientInterpolationMethodType";

/// The Classic interpolation method, the one Doctored draws.
const CLASSIC: &[u8] = b"Gcls";

/// A decoded `GdFl`, or a gradient fill description the export writes as one.
pub(crate) struct GradientCarry {
    /// Strict-parsed; stops in stable position order; `align_with_layer`
    /// the file's `Algn` (absent = true) or the description's, `dither`
    /// false, every stop's `ink` `None`.
    pub fill: GradientFill,
    /// RGB only: one linear sRGB triple per stop, present when any stop read
    /// off the sRGB byte grid (module doc's Reading).
    pub linear: Option<Vec<[f32; 3]>>,
    /// CMYK only: one straight ink per stop (`CMYC` / 100).
    pub inks: Option<Vec<[f32; 4]>>,
    /// Fragments read after "whose" in the import report.
    pub losses: Vec<String>,
}

/// A `GdFl` descriptor as a carry (module doc's Reading); `colours` is the
/// file's colour crossing. `Err` is the decline reason.
pub(crate) fn decode(d: &Descriptor, colours: FxColours<'_>) -> Result<GradientCarry, String> {
    let Some(Value::Object(mut read)) = gradient(d, colours, true)? else {
        return Err(NO_GRADIENT.into());
    };
    let mut stops: Vec<Map<String, Value>> = read
        .remove("stops")
        .and_then(|stops| serde_json::from_value(stops).ok())
        .ok_or(NO_GRADIENT)?;
    let position =
        |stop: &Map<String, Value>| stop.get("position").and_then(Value::as_f64).unwrap_or(0.0);
    stops.sort_by(|a, b| position(a).total_cmp(&position(b)));
    let triple = |value: Option<&Value>| -> Option<Vec<f32>> {
        value?
            .as_array()?
            .iter()
            .map(|v| v.as_f64().map(|v| v as f32))
            .collect()
    };
    let cmyk = matches!(colours, FxColours::Cmyk);
    let mut linear = Vec::with_capacity(stops.len());
    let mut inks = Vec::with_capacity(stops.len());
    let mut wide = false;
    for stop in &mut stops {
        let ink = triple(stop.remove("ink").as_ref());
        let own = triple(stop.remove("linear").as_ref());
        if cmyk {
            let ink = ink.filter(|ink| ink.len() == 4).ok_or(NOT_INK)?;
            inks.push([ink[0], ink[1], ink[2], ink[3]]);
            continue;
        }
        let hex = stop
            .get("color")
            .and_then(Value::as_str)
            .and_then(crate::style_names::parse_color)
            .unwrap_or_default();
        linear.push(match own.filter(|own| own.len() == 3) {
            Some(own) => {
                wide = true;
                [own[0], own[1], own[2]]
            }
            None => srgb_linear(hex.map(|v| f32::from(v) / 255.0)),
        });
    }
    let aligned = read
        .remove("align_with_layer")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    read.remove("dither");
    read.insert(
        "stops".into(),
        Value::from(stops.into_iter().map(Value::Object).collect::<Vec<_>>()),
    );
    let mut holder = Map::new();
    holder.insert("gradient".into(), Value::Object(read));
    let mut fill = crate::style_json::gradient(&holder, "", "gradient", Strictness::Strict)?;
    fill.align_with_layer = aligned;
    fill.dither = false;
    Ok(GradientCarry {
        fill,
        linear: wide.then_some(linear),
        inks: cmyk.then_some(inks),
        losses: losses(d)?,
    })
}

/// The settings a Doctored gradient fill cannot hold (module doc's
/// Reading).
fn losses(d: &Descriptor) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    d.object(b"Grad")?.ok_or(NO_GRADIENT)?;
    if let Some(method) = d.enumeration(METHOD)?.filter(|id| *id != CLASSIC) {
        out.push(format!(
            "gradient uses the {} interpolation method, which Doctored draws as Classic",
            String::from_utf8_lossy(method).trim_end()
        ));
    }
    if let Some(offset) = d.object(b"Ofst")? {
        let h = offset.unit(b"Hrzn")?.unwrap_or(0.0);
        let v = offset.unit(b"Vrtc")?.unwrap_or(0.0);
        if h != 0.0 || v != 0.0 {
            let shown = |x: f64| (x * 100.0).round() / 100.0;
            out.push(format!(
                "gradient Offset is ({} %, {} %)",
                shown(h),
                shown(v)
            ));
        }
    }
    if d.bool(b"Dthr")? == Some(true) {
        out.push("gradient is dithered".to_string());
    }
    Ok(out)
}

/// The description's gradient keys into `o` (module doc's Description):
/// version 1 with `stop_linears` when wide, or with `policy` version 2 ink.
/// An inked carry without `policy` errs.
pub(crate) fn describe(
    carry: &GradientCarry,
    o: &mut Map<String, Value>,
    policy: Option<&CmykPolicy>,
) -> Result<(), String> {
    o.insert("fill".into(), Value::from("gradient"));
    let mut fill = carry.fill.clone();
    match &carry.inks {
        Some(inks) => {
            let policy = policy.ok_or("a CMYK gradient fill needs its document's ink policy")?;
            for (stop, ink) in fill.stops.iter_mut().zip(inks) {
                stop.color = crate::cmyk_style::hex_of(*ink, policy)?;
            }
            let rows: Vec<Value> = inks
                .iter()
                .map(|ink| Value::from(ink.map(crate::style_json_write::number).to_vec()))
                .collect();
            o.insert("ink".into(), Value::from("cmyk"));
            o.insert("stop_inks".into(), Value::from(rows));
            o.insert("version".into(), Value::from(2));
        }
        None => {
            if let Some(linear) = &carry.linear {
                let rows: Vec<Value> = linear
                    .iter()
                    .map(|l| Value::from(l.map(f64::from).to_vec()))
                    .collect();
                o.insert("stop_linears".into(), Value::from(rows));
            }
            o.insert("version".into(), Value::from(1));
        }
    }
    o.insert(
        "gradient".into(),
        crate::style_json_write::gradient_value(&fill),
    );
    Ok(())
}

/// The RGB render's `gradient` component (module doc's Description) over
/// `frame` (`[x, y, w, h]`, the aligned layer's box; `None` the canvas); an
/// inked carry errs.
pub(crate) fn component(
    carry: &GradientCarry,
    frame: Option<[i32; 4]>,
) -> Result<crate::artwork_render::Component, String> {
    if carry.inks.is_some() {
        return Err("a CMYK gradient fill has no RGB rendering".into());
    }
    Ok(crate::artwork_render::Component::Gradient {
        gradient: crate::style_json_write::gradient_value(&carry.fill),
        colors: carry
            .fill
            .stops
            .iter()
            .map(|stop| stop.color.map(|v| f32::from(v) / 255.0))
            .collect(),
        linear: carry.linear.clone(),
        frame: frame.map(|f| f.map(i64::from)),
    })
}

/// The CMYK render's `gradient` component of the carry's inks over `frame`
/// (as [`component`]'s); a carry without inks errs.
pub(crate) fn cmyk_component(
    carry: &GradientCarry,
    frame: Option<[i32; 4]>,
) -> Result<crate::cmyk_artwork_render::CmykComponent, String> {
    let inks = carry
        .inks
        .clone()
        .ok_or("an RGB gradient fill has no CMYK rendering")?;
    Ok(crate::cmyk_artwork_render::CmykComponent::Gradient {
        gradient: crate::style_json_write::gradient_value(&carry.fill),
        inks,
        frame: frame.map(|f| f.map(i64::from)),
    })
}

/// A gradient fill description as the export writes it (module doc's
/// Writing): on an RGB document (`cmyk` false) version 1 with no `ink` and
/// an optional `stop_linears` (one triple per stop, finite within
/// ±`LINEAR_LIMIT`); on a CMYK one version 2 with `ink` "cmyk" and
/// `stop_inks` (one four-number ink per stop in 0…1, quantized to four
/// decimals). Both lists are in STORED order and follow their stops into the
/// stable position order. `None` for anything else.
pub(crate) fn exported(root: &Map<String, Value>, cmyk: bool) -> Option<GradientCarry> {
    let version = root.get("version")?.as_u64()?;
    let ink = root.get("ink").map(|ink| ink.as_str());
    let valid = if cmyk {
        version == 2 && ink == Some(Some("cmyk")) && !root.contains_key("stop_linears")
    } else {
        version == 1 && ink.is_none() && !root.contains_key("stop_inks")
    };
    if !valid {
        return None;
    }
    let gradient = root.get("gradient")?;
    let positions: Vec<f64> = gradient
        .get("stops")?
        .as_array()?
        .iter()
        .map(|stop| stop.get("position")?.as_f64())
        .collect::<Option<_>>()?;
    let mut order: Vec<usize> = (0..positions.len()).collect();
    order.sort_by(|a, b| positions[*a].total_cmp(&positions[*b]));
    let mut holder = Map::new();
    holder.insert("gradient".into(), gradient.clone());
    let mut fill = crate::style_json::gradient(&holder, "", "gradient", Strictness::Strict).ok()?;
    if fill.stops.len() != order.len() {
        return None;
    }
    // The description's own `align_with_layer`, true only when it
    // says so, as `fill_align_native::describes_align` reads it (the host
    // always spells the key).
    fill.align_with_layer = gradient.get("align_with_layer").and_then(Value::as_bool) == Some(true);
    fill.dither = false;
    for stop in &mut fill.stops {
        stop.ink = None;
    }
    // One row of `width` numbers per stop, each `accept`ed, in sorted order.
    let rows = |key: &str, width: usize, accept: &dyn Fn(f64) -> bool| -> Option<Vec<Vec<f64>>> {
        let listed = root.get(key)?.as_array()?;
        if listed.len() != order.len() {
            return None;
        }
        let rows: Vec<Vec<f64>> = listed
            .iter()
            .map(|row| {
                let row = row.as_array()?;
                (row.len() == width).then_some(())?;
                row.iter()
                    .map(|v| v.as_f64().filter(|v| v.is_finite() && accept(*v)))
                    .collect::<Option<Vec<f64>>>()
            })
            .collect::<Option<_>>()?;
        Some(order.iter().map(|&i| rows[i].clone()).collect())
    };
    let (linear, inks) = if cmyk {
        let inks = rows("stop_inks", 4, &|v| (0.0..=1.0).contains(&v))?
            .into_iter()
            .map(|r| std::array::from_fn(|c| ((r[c] * 1e4).round() / 1e4) as f32))
            .collect();
        (None, Some(inks))
    } else if root.contains_key("stop_linears") {
        let limit = f64::from(LINEAR_LIMIT);
        let linear = rows("stop_linears", 3, &|v| v.abs() <= limit)?
            .into_iter()
            .map(|r| [r[0] as f32, r[1] as f32, r[2] as f32])
            .collect();
        (Some(linear), None)
    } else {
        (None, None)
    };
    Some(GradientCarry {
        fill,
        linear,
        inks,
        losses: Vec::new(),
    })
}

/// The `GdFl` descriptor's items (module doc's Writing): the Classic method,
/// then the `Grdn` writer's keys with the carry's `Algn` and `Intr`, each
/// stop's colour in the document's numbers (`colours`).
pub(crate) fn items(carry: &GradientCarry, colours: FxColours<'_>) -> Vec<(Vec<u8>, Item)> {
    let mut fill = carry.fill.clone();
    if let Some(inks) = &carry.inks {
        for (stop, ink) in fill.stops.iter_mut().zip(inks) {
            stop.ink = Some(*ink);
        }
    }
    let mut out = vec![(
        METHOD.to_vec(),
        Item::Enum(METHOD_TYPE.to_vec(), CLASSIC.to_vec()),
    )];
    out.extend(gradient_items(&fill, colours, carry.linear.as_deref()));
    out
}

/// The import report's lines for a carried `GdFl` (module doc's Reading):
/// the observation (`clause` is the record's form,
/// `psd_fill_layer::Form::clause`) and one loss per setting the fill cannot
/// hold.
pub(crate) fn report(
    index: usize,
    name: &str,
    carry: &GradientCarry,
    clause: &str,
    observations: &mut Vec<String>,
    losses: &mut Vec<String>,
) {
    let layer = format!("Layer {index} (“{name}”) is a Photoshop {KIND} layer (GdFl)");
    observations.push(format!(
        "{layer}{clause}; layered import carries it as a Doctored gradient fill layer with its stops, style, angle, scale, reverse, mask, opacity, blend mode and clipping.{}",
        if carry.linear.is_some() {
            " Its stop colours are kept as wide colours."
        } else {
            ""
        }
    ));
    for fragment in &carry.losses {
        losses.push(format!("{layer} whose {fragment}; layered import carries the fill without it, so its appearance can differ from Photoshop's."));
    }
}
