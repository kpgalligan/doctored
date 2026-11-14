//! The per-effect half of the ONE `lfx2` codec: one reader and one writer for
//! each of Photoshop's nine effect `Objc` classes, plus the `Grdn` gradient,
//! the `ShpC` contour and the `RGBC` colour they nest. The framing, the
//! `masterFXSwitch`/`Scl ` envelope, the blend-id table, the blending ranges
//! and `iOpa` live in `psd_layer_effects`, whose module doc is the mapping
//! table; nothing else in the crate reads or writes one of these classes.
//!
//! Colours cross through [`FxColours`]: an RGB document's `RGBC` in its own
//! space — its doubles read beyond the byte grid as a slot's wide colour
//! (`linear`: `authored_color::wide_out`, kept only when it is not an
//! sRGB byte, so an sRGB file's integers add no key) and a slot's wide colour
//! written as the document's numbers (`wide_into`: the byte grid in an 8-bit
//! file, `Rgb16`'s fractional numbers in a 16-bit one) — or (#1) a CMYK document's `CMYC` ink {`Cyn ` `Mgnt` `Ylw ` `Blck`
//! doub 0…100} to and from the slot's `ink` — an `RGBC` in a CMYK file reads
//! as an sRGB hex with no ink, and a slot with no ink writes its hex as
//! `RGBC` (the CMYK export first gives every slot one,
//! `cmyk_style::with_all_inks`) — or (#1 phase 3a) a gray file's `Grsc`
//! {`Gry ` doub, Photoshop's K percent} to and from the slot's gray
//! (`gray_style::grsc_colour` / `grsc_k_percent`, the latter the Grayscale
//! export's, #1 phase 3b; an `RGBC` in a gray file reads as an sRGB hex,
//! which the gray open greys) — or (#1 Lab phase 3a) a Lab file's `LbCl`
//! {`Lmnc` L\*, `A   ` a\*, `B   ` b\*} read as the sRGB hex of that Lab
//! colour (`psd_import_lab::lbcl_srgb`), an `RGBC` there as an sRGB hex (a
//! Lab document's style colours are authored sRGB, `lab_style`), and (#1
//! Lab phase 3b) written as the `LbCl` of the slot's hex
//! (`psd_export_lab::srgb_lbcl`, the read inverted).
//!
//! Reading produces one style-JSON effect object per class
//! (`style`'s schema): every key the class carries is inserted, absent keys
//! are simply left out so the schema default applies, and every number is
//! handed over raw — `LayerStyle::from_value` clamps and quantizes it. Writing
//! emits every key Photoshop's dialog reads plus the fixed
//! written-only defaults (the glows' technique, offset), which the reader
//! ignores; a bevel's `bvlT` crosses both ways as `technique` (#2: read
//! only when it is a Chisel, so a Smooth `SfBL` adds no key); a glow's `Grad` and `ShdN` cross both ways as its `fill_type`
//! gradient, `gradient` stops and `jitter` (`style_glow_gradient`: a
//! `Grad` makes the glow a Gradient one, `ShdN` is read only when above 0,
//! and `Clr ` is written and read in either mode); a bevel's `useTexture` and texture keys cross both ways through
//! `psd_pattern_fill` (read in `psd_layer_effects`, beside the pattern
//! table); the shadows' and glows' `Nose` crosses both ways as `noise` (read
//! only when above 0, so a `Nose` 0 adds no key). The contour-family keys
//! (`TrnS`/`MpgS`, `AntA`, `Inpr`, `useShape`, `antialiasGloss`) cross both
//! ways through `psd_contour`.

use serde_json::{json, Map, Value};

use crate::authored_color::{hex_equivalent, wide_into, wide_out, AuthoredColors};
use crate::blend::BlendMode;
use crate::psd_contour::{anti_alias_item, shpc_item};
use crate::psd_descriptor::{Descriptor, Item};
use crate::style::{
    BevelDirection, BevelStyle, BevelTechnique, Contour, Effect, GlobalLight, GlowFill, GlowSource,
    GradientFill, GradientStyle, StrokeFill, StrokePosition,
};
use crate::style_names::{blend_mode_name, color_hex};

/// The nine classes: the `lfx2` key (which is also the nested descriptor's
/// class) and the style JSON `type` it maps to, in Photoshop's own key order.
pub(crate) const CLASSES: [([u8; 4], &str); 9] = [
    (*b"DrSh", "drop_shadow"),
    (*b"IrSh", "inner_shadow"),
    (*b"OrGl", "outer_glow"),
    (*b"IrGl", "inner_glow"),
    (*b"ebbl", "bevel_emboss"),
    (*b"ChFX", "satin"),
    (*b"SoFi", "color_overlay"),
    (*b"GrFl", "gradient_overlay"),
    (*b"FrFX", "stroke"),
];

/// The `BlnM` enum ids in `psd_group_records::PSD_BLEND_KEYS` order: entry `i`
/// is `psd::map_psd_blend(i + 1)`, so this crate keeps ONE blend table for the
/// record keys and one for the descriptor ids, read in both directions here.
const BLEND_IDS: [&[u8]; 27] = [
    b"Nrml",
    b"Dslv",
    b"Drkn",
    b"Mltp",
    b"CBrn",
    b"linearBurn",
    b"darkerColor",
    b"Lghn",
    b"Scrn",
    b"CDdg",
    b"linearDodge",
    b"lighterColor",
    b"Ovrl",
    b"SftL",
    b"HrdL",
    b"vividLight",
    b"linearLight",
    b"pinLight",
    b"hardMix",
    b"Dfrn",
    b"Xclu",
    b"blendSubtraction",
    b"blendDivide",
    b"H   ",
    b"Strt",
    b"Clr ",
    b"Lmns",
];

/// One enum vocabulary: Photoshop's ids and the style JSON names they map to.
type Ids = [(&'static [u8], &'static str)];

const GLOW_SOURCE_IDS: [(&[u8], &str); 2] = [(b"SrcE", "edge"), (b"SrcC", "center")];
const BEVEL_STYLE_IDS: [(&[u8], &str); 4] = [
    (b"OtrB", "outer_bevel"),
    (b"InrB", "inner_bevel"),
    (b"Embs", "emboss"),
    (b"PlEb", "pillow_emboss"),
];
const BEVEL_DIRECTION_IDS: [(&[u8], &str); 2] = [(b"In  ", "up"), (b"Out ", "down")];
/// The `bvlT` ids (#2): Smooth, Chisel Hard, Chisel Soft (the stringIDs
/// `softMatte`, `preciseMatte`, `slopeLimitMatte`).
const BEVEL_TECHNIQUE_IDS: [(&[u8], &str); 3] = [
    (b"SfBL", "smooth"),
    (b"PrBL", "chisel_hard"),
    (b"Slmt", "chisel_soft"),
];
/// The `GrdT` ids; Shape Burst (the Stroke's own) is the string id
/// `shapeburst`, as `stroke_fill_take.py`'s Photoshop PSD writes it.
const GRADIENT_STYLE_IDS: [(&[u8], &str); 6] = [
    (b"Lnr ", "linear"),
    (b"Rdl ", "radial"),
    (b"Angl", "angle"),
    (b"Rflc", "reflected"),
    (b"Dmnd", "diamond"),
    (b"shapeburst", "shape_burst"),
];
const STROKE_POSITION_IDS: [(&[u8], &str); 3] = [
    (b"OutF", "outside"),
    (b"InsF", "inside"),
    (b"CtrF", "center"),
];
/// The `FrFl` ids; a Pattern stroke's fill items are attached by
/// `psd_layer_effects::build` (`psd_pattern_fill`).
const STROKE_FILL_IDS: [(&[u8], &str); 3] = [
    (b"SClr", "color"),
    (b"GrFl", "gradient"),
    (b"Ptrn", "pattern"),
];

const STROKE_EMBOSS: &str = "bevel uses Stroke Emboss, which Doctored does not render";
const NOT_CUSTOM: &str = "gradient is not a custom colour gradient";
const STOP_COUNT: &str = "gradient does not have between 2 and 32 colour stops";
const TRANSPARENCY: &str = "gradient transparency stops do not match its colour stops";
const COLOUR: &str = "effect colour is not an RGB descriptor";
const INK: &str = "effect colour is not a CMYK descriptor";

/// How an effect's colours cross the file (module doc).
#[derive(Clone, Copy)]
pub(crate) enum FxColours<'a> {
    /// An RGB document: `RGBC` in the document's space, converted to and
    /// from the authored sRGB through the transform.
    Rgb(Option<&'a AuthoredColors>),
    /// A 16-bit RGB document: read as `Rgb`; a wide colour writes the
    /// document's fractional numbers.
    Rgb16(Option<&'a AuthoredColors>),
    /// A CMYK document: `CMYC` ink to and from the slot's ink (#1).
    Cmyk,
    /// A gray file: `Grsc` K percent read as the slot's gray through its
    /// Gray profile (#1 phase 3a).
    Gray(&'a crate::gray_document::GrayPolicy),
    /// A Lab file: `LbCl` read as the sRGB hex of its Lab colour, `RGBC` as
    /// an sRGB hex (#1 Lab phase 3a).
    Lab,
    /// A 32-bit float RGB document's WRITE: `RGBC` numbers are the
    /// document's linear light × 255
    /// (`style_float::FloatStyle::descriptor_numbers`), as Photoshop 26.10.0
    /// stores them at 32 bits. No reader constructs it: the 32-bit open reads
    /// through `Rgb` with the profile's linear twin, the same numbers.
    Float(&'a crate::style_float::FloatStyle),
}

/// The four `CMYC` keys, in Photoshop's order.
const INK_KEYS: [&[u8]; 4] = [b"Cyn ", b"Mgnt", b"Ylw ", b"Blck"];

/// The `Lctn` scale Photoshop stores a gradient stop position on.
const LOCATION: f64 = 4096.0;

// --------------------------------------------------------------- reading --

fn unknown(key: &[u8]) -> String {
    format!(
        "descriptor key {} has an unrecognized value",
        String::from_utf8_lossy(key).trim_end()
    )
}

/// One `#Prc` key as a 0..1 fraction.
pub(crate) fn fraction(d: &Descriptor, key: &[u8]) -> Result<Option<f64>, String> {
    Ok(d.unit(key)?.map(|value| value / 100.0))
}

/// One `enum` key as its style JSON name; an id outside the table declines.
fn named(d: &Descriptor, key: &[u8], table: &Ids) -> Result<Option<&'static str>, String> {
    let Some(id) = d.enumeration(key)? else {
        return Ok(None);
    };
    table
        .iter()
        .find(|(candidate, _)| *candidate == id)
        .map(|(_, name)| Some(*name))
        .ok_or_else(|| unknown(key))
}

/// One `BlnM` key as its style JSON blend name.
pub(crate) fn blend(d: &Descriptor, key: &[u8]) -> Result<Option<&'static str>, String> {
    let Some(id) = d.enumeration(key)? else {
        return Ok(None);
    };
    BLEND_IDS
        .iter()
        .position(|candidate| *candidate == id)
        .map(|index| Some(blend_mode_name(crate::psd::map_psd_blend(index as i32 + 1))))
        .ok_or_else(|| unknown(key))
}

/// One `BlnM` key as a blend mode, a group's `passThrough` included: a
/// layer comp's recorded mode (`psd_comp_records`).
pub(crate) fn blend_mode(d: &Descriptor, key: &[u8]) -> Result<Option<BlendMode>, String> {
    let Some(id) = d.enumeration(key)? else {
        return Ok(None);
    };
    if id == b"passThrough" {
        return Ok(Some(BlendMode::PassThrough));
    }
    BLEND_IDS
        .iter()
        .position(|candidate| *candidate == id)
        .map(|index| Some(crate::psd::map_psd_blend(index as i32 + 1)))
        .ok_or_else(|| unknown(key))
}

/// One colour slot as `(#rrggbb, ink)` (module doc): an RGB document's
/// `RGBC` converted from its space back to the sRGB every authored style
/// colour is stored in, with no ink; a CMYK document's `CMYC` as its ink
/// (percent / 100) beside a placeholder hex the import canonicalises, or an
/// `RGBC` there as an sRGB hex with no ink. An `RGBC` holds `Rd  `/`Grn `/
/// `Bl  ` in 0..255 or, as Photoshop 26 writes some gradient stops,
/// `redFloat`/`greenFloat`/`blueFloat` in 0..1 (a take read them on the
/// default Gradient Overlay's white stop).
/// A read colour: its hex, from a `CMYC` its ink (percent / 100), and from
/// an RGB document's `RGBC` off the sRGB byte grid its wide colour.
type ReadColour = (String, Option<[f64; 4]>, Option<[f32; 3]>);

fn colour(
    d: &Descriptor,
    key: &[u8],
    colours: FxColours<'_>,
) -> Result<Option<ReadColour>, String> {
    let Some(object) = d.object(key).map_err(|_| COLOUR.to_string())? else {
        return Ok(None);
    };
    let colors = match colours {
        FxColours::Rgb(colors) | FxColours::Rgb16(colors) => Some(colors),
        FxColours::Cmyk if object.class == b"CMYC" => {
            let ink = cmyc_of(object).ok_or_else(|| INK.to_string())?;
            return Ok(Some(("#000000".to_string(), Some(ink), None)));
        }
        FxColours::Cmyk if object.class == b"RGBC" => None,
        FxColours::Cmyk => return Err(INK.to_string()),
        FxColours::Gray(policy) if object.class == b"Grsc" => {
            let k = object
                .double(b"Gry ")
                .ok()
                .flatten()
                .filter(|value| value.is_finite())
                .ok_or_else(|| COLOUR.to_string())?;
            let (rgb, linear) = crate::gray_style::grsc_colour(policy, k);
            return Ok(Some((color_hex(rgb), None, linear)));
        }
        // An `RGBC` in a gray file stays an sRGB hex; the open greys it.
        FxColours::Gray(_) => None,
        FxColours::Lab if object.class == b"LbCl" => {
            let mut lab = [0f64; 3];
            for (slot, key) in lab.iter_mut().zip([&b"Lmnc"[..], b"A   ", b"B   "]) {
                *slot = object
                    .double(key)
                    .ok()
                    .flatten()
                    .filter(|value| value.is_finite())
                    .ok_or_else(|| COLOUR.to_string())?;
            }
            return Ok(Some((
                color_hex(crate::psd_import_lab::lbcl_srgb(lab)),
                None,
                None,
            )));
        }
        // An `RGBC` in a Lab file stays an sRGB hex.
        FxColours::Lab => None,
        // Write-only (its doc); were a reader to pass it, an sRGB hex.
        FxColours::Float(_) => None,
    };
    let values = rgbc_of(object).ok_or_else(|| COLOUR.to_string())?;
    let mut rgb = [0u8; 3];
    let mut numbers = [0f32; 3];
    for ((slot, number), value) in rgb.iter_mut().zip(&mut numbers).zip(values) {
        *slot = value.round().clamp(0.0, 255.0) as u8;
        *number = (value / 255.0).clamp(0.0, 1.0) as f32;
    }
    // A CMYK file's `RGBC` stays an sRGB hex (its ink is authoritative).
    let Some(colors) = colors else {
        return Ok(Some((color_hex(rgb), None, None)));
    };
    let linear = wide_out(colors, numbers);
    if let Some(byte) = hex_equivalent(linear) {
        return Ok(Some((color_hex(byte), None, None)));
    }
    if let Some(transform) = colors {
        rgb = transform.color(rgb);
    }
    Ok(Some((color_hex(rgb), None, Some(linear))))
}

/// An `RGBC` object's three components on Photoshop's 0…255 scale, finite
/// and unclamped: `Rd  `/`Grn `/`Bl  ` as they are or `redFloat`/
/// `greenFloat`/`blueFloat` · 255 (`colour`'s doc); `None` when one is
/// missing or not finite. The ONE `RGBC` reader (`colour`, and a `SoCo`
/// fill layer's colour in `psd_fill_layer`).
pub(crate) fn rgbc_of(object: &Descriptor) -> Option<[f64; 3]> {
    let components: [(&[u8], &[u8]); 3] = [
        (b"Rd  ", b"redFloat"),
        (b"Grn ", b"greenFloat"),
        (b"Bl  ", b"blueFloat"),
    ];
    let mut out = [0f64; 3];
    for (slot, (code, unit)) in out.iter_mut().zip(components) {
        *slot = match object.double(code).ok().flatten() {
            Some(value) => Some(value),
            None => object
                .double(unit)
                .ok()
                .flatten()
                .map(|value| value * 255.0),
        }
        .filter(|value| value.is_finite())?;
    }
    Some(out)
}

/// A `CMYC` object's four inks as fractions (percent / 100, clamped into
/// 0…1), in Photoshop's order; `None` when one is missing or not finite.
/// The ONE `CMYC` reader (`colour`, and a `SoCo` fill layer's ink).
pub(crate) fn cmyc_of(object: &Descriptor) -> Option<[f64; 4]> {
    let mut ink = [0f64; 4];
    for (slot, component) in ink.iter_mut().zip(INK_KEYS) {
        let value = object
            .double(component)
            .ok()
            .flatten()
            .filter(|value| value.is_finite())?;
        *slot = (value / 100.0).clamp(0.0, 1.0);
    }
    Some(ink)
}

/// One colour slot into the effect object: `color`, from a `CMYC` the ink
/// under `ink_key`, and a wide colour under `linear_key` (a gradient stop,
/// `None`, carries none).
fn put_colour(
    o: &mut Map<String, Value>,
    keys: (&str, &str, Option<&str>),
    slot: Option<ReadColour>,
) {
    let (color_key, ink_key, linear_key) = keys;
    if let Some((hex, ink, linear)) = slot {
        o.insert(color_key.to_string(), json!(hex));
        if let Some(ink) = ink {
            o.insert(ink_key.to_string(), json!(ink));
        }
        if let (Some(key), Some(linear)) = (linear_key, linear) {
            o.insert(key.to_string(), json!(linear.map(f64::from)));
        }
    }
}

pub(crate) fn put(object: &mut Map<String, Value>, key: &str, value: Option<impl Into<Value>>) {
    if let Some(value) = value {
        object.insert(key.to_string(), value.into());
    }
}

/// The `Grdn` stop list plus the effect's own gradient keys, as the style
/// JSON's GradientFill object. With `wide_stops` (a `GdFl` fill layer,
/// `psd_gradient_fill`) a stop's off-grid colour also adds its `linear`;
/// every effect passes false.
pub(crate) fn gradient(
    d: &Descriptor,
    colors: FxColours<'_>,
    wide_stops: bool,
) -> Result<Option<Value>, String> {
    let Some(grdn) = d.object(b"Grad")? else {
        return Ok(None);
    };
    if grdn.enumeration(b"GrdF")?.is_some_and(|id| id != b"CstS") {
        return Err(NOT_CUSTOM.to_string());
    }
    let list = grdn.list(b"Clrs")?.ok_or_else(|| STOP_COUNT.to_string())?;
    if !(2..=32).contains(&list.len()) {
        return Err(STOP_COUNT.to_string());
    }
    let mut stops = Vec::with_capacity(list.len());
    for item in list {
        let Item::Object(entry) = item else {
            return Err(STOP_COUNT.to_string());
        };
        let colour = colour(entry, b"Clr ", colors)?.ok_or_else(|| COLOUR.to_string())?;
        let location = entry.long(b"Lctn")?.unwrap_or(0);
        stops.push((location, colour, entry.long(b"Mdpn")?));
    }
    let transparency = grdn
        .list(b"Trns")?
        .ok_or_else(|| TRANSPARENCY.to_string())?;
    if transparency.len() != stops.len() {
        return Err(TRANSPARENCY.to_string());
    }
    let mut paired = Vec::with_capacity(stops.len());
    for (item, (location, colour, midpoint)) in transparency.iter().zip(&stops) {
        let Item::Object(entry) = item else {
            return Err(TRANSPARENCY.to_string());
        };
        if entry.long(b"Lctn")?.unwrap_or(0) != *location {
            return Err(TRANSPARENCY.to_string());
        }
        let opacity = fraction(entry, b"Opct")?.unwrap_or(1.0);
        paired.push((*location, colour, *midpoint, opacity, entry.long(b"Mdpn")?));
    }
    // Midpoints: Photoshop's segment `i` reads stop `i + 1`'s `Mdpn`
    // (`gradient_field`'s module doc), so our stop `i` takes the NEXT
    // stop's, colour and transparency alike, in POSITION order (a stable
    // sort by location, each stop carrying its own `Mdpn`); stop 0's governs
    // nothing. Added only when present and not 50, so a file at Photoshop's
    // default imports the gradient it always did.
    paired.sort_by_key(|p| p.0);
    let mut out = Vec::with_capacity(paired.len());
    for (i, (location, colour, _, opacity, _)) in paired.iter().enumerate() {
        let mut stop = Map::new();
        stop.insert("position".into(), json!(f64::from(*location) / LOCATION));
        put_colour(
            &mut stop,
            ("color", "ink", wide_stops.then_some("linear")),
            Some((*colour).clone()),
        );
        stop.insert("opacity".into(), json!(opacity));
        if let Some(next) = paired.get(i + 1) {
            for (key, mdpn) in [("midpoint", next.2), ("opacity_midpoint", next.4)] {
                if let Some(m) = mdpn.filter(|m| *m != 50) {
                    stop.insert(key.into(), json!(f64::from(m) / 100.0));
                }
            }
        }
        out.push(Value::Object(stop));
    }
    let mut fill = Map::new();
    fill.insert("stops".into(), Value::Array(out));
    // Smoothness: `Intr` over 4096 (100 %), added only when present
    // and not 4096, so a file at Photoshop's default imports the gradient it
    // always did.
    if let Some(intr) = grdn.double(b"Intr")?.filter(|v| *v != LOCATION) {
        fill.insert("smoothness".into(), json!(intr / LOCATION));
    }
    put(&mut fill, "style", named(d, b"Type", &GRADIENT_STYLE_IDS)?);
    put(&mut fill, "angle", d.unit(b"Angl")?);
    put(&mut fill, "scale", fraction(d, b"Scl ")?);
    put(&mut fill, "reverse", d.bool(b"Rvrs")?);
    put(&mut fill, "align_with_layer", d.bool(b"Algn")?);
    // Dither: only a true `Dthr` adds the key, which the style writes
    // only when true.
    if d.bool(b"Dthr")? == Some(true) {
        fill.insert("dither".into(), Value::Bool(true));
    }
    Ok(Some(Value::Object(fill)))
}

/// `Nose` as `noise`, inserted only when above 0 so a file written with
/// the fixed `Nose` 0 imports the style it always did.
fn put_noise(o: &mut Map<String, Value>, d: &Descriptor) -> Result<(), String> {
    if let Some(noise) = fraction(d, b"Nose")?.filter(|n| *n > 0.0) {
        o.insert("noise".into(), json!(noise));
    }
    Ok(())
}

/// One effect class as a style JSON effect object. `Err` is the decline
/// reason, read after "whose" in the import report.
pub(crate) fn read(
    class: &[u8; 4],
    name: &'static str,
    d: &Descriptor,
    colors: FxColours<'_>,
) -> Result<Value, String> {
    let mut o = Map::new();
    o.insert("type".into(), json!(name));
    put(&mut o, "enabled", d.bool(b"enab")?);
    if class != b"ebbl" {
        put(&mut o, "blend", blend(d, b"Md  ")?);
        put(&mut o, "opacity", fraction(d, b"Opct")?);
    }
    if !matches!(class, b"GrFl" | b"ebbl") {
        put_colour(
            &mut o,
            ("color", "ink", Some("linear")),
            colour(d, b"Clr ", colors)?,
        );
    }
    match class {
        b"DrSh" | b"IrSh" => {
            put(&mut o, "use_global_light", d.bool(b"uglg")?);
            put(&mut o, "angle", d.unit(b"lagl")?);
            put(&mut o, "distance", d.unit(b"Dstn")?);
            let spread = fraction(d, b"Ckmt")?;
            put(
                &mut o,
                if class == b"DrSh" { "spread" } else { "choke" },
                spread,
            );
            put(&mut o, "size", d.unit(b"blur")?);
            put_noise(&mut o, d)?;
            if class == b"DrSh" {
                put(&mut o, "layer_knocks_out", d.bool(b"layerConceals")?);
            }
        }
        b"OrGl" | b"IrGl" => {
            // A `Grad` makes the glow a Gradient one: its stops only
            // (`style_glow_gradient`); `Clr ` above is read in either mode.
            if let Some(Value::Object(fill)) = gradient(d, colors, false)? {
                o.insert("fill_type".into(), json!("gradient"));
                let stops = fill.get("stops").cloned().unwrap_or(Value::Null);
                o.insert("gradient".into(), json!({ "stops": stops }));
            }
            // `ShdN` as `jitter`, inserted only when above 0, as `Nose` is.
            if let Some(jitter) = fraction(d, b"ShdN")?.filter(|j| *j > 0.0) {
                o.insert("jitter".into(), json!(jitter));
            }
            let choke = fraction(d, b"Ckmt")?;
            put(
                &mut o,
                if class == b"OrGl" { "spread" } else { "choke" },
                choke,
            );
            put(&mut o, "size", d.unit(b"blur")?);
            put_noise(&mut o, d)?;
            if class == b"IrGl" {
                put(&mut o, "source", named(d, b"glwS", &GLOW_SOURCE_IDS)?);
            }
        }
        b"ebbl" => {
            put(&mut o, "highlight_blend", blend(d, b"hglM")?);
            put_colour(
                &mut o,
                ("highlight_color", "highlight_ink", Some("highlight_linear")),
                colour(d, b"hglC", colors)?,
            );
            put(&mut o, "highlight_opacity", fraction(d, b"hglO")?);
            put(&mut o, "shadow_blend", blend(d, b"sdwM")?);
            put_colour(
                &mut o,
                ("shadow_color", "shadow_ink", Some("shadow_linear")),
                colour(d, b"sdwC", colors)?,
            );
            put(&mut o, "shadow_opacity", fraction(d, b"sdwO")?);
            if d.enumeration(b"bvlS")?
                .is_some_and(|id| id == b"strokeEmboss")
            {
                return Err(STROKE_EMBOSS.to_string());
            }
            put(&mut o, "style", named(d, b"bvlS", &BEVEL_STYLE_IDS)?);
            put(
                &mut o,
                "direction",
                named(d, b"bvlD", &BEVEL_DIRECTION_IDS)?,
            );
            // Only a Chisel adds the key, so a Smooth record reads as before.
            put(
                &mut o,
                "technique",
                named(d, b"bvlT", &BEVEL_TECHNIQUE_IDS)?.filter(|name| *name != "smooth"),
            );
            put(&mut o, "use_global_light", d.bool(b"uglg")?);
            put(&mut o, "angle", d.unit(b"lagl")?);
            put(&mut o, "altitude", d.unit(b"Lald")?);
            put(&mut o, "depth", fraction(d, b"srgR")?);
            put(&mut o, "size", d.unit(b"blur")?);
            put(&mut o, "soften", d.unit(b"Sftn")?);
        }
        b"ChFX" => {
            put(&mut o, "angle", d.unit(b"lagl")?);
            put(&mut o, "distance", d.unit(b"Dstn")?);
            put(&mut o, "size", d.unit(b"blur")?);
            put(&mut o, "invert", d.bool(b"Invr")?);
        }
        b"GrFl" => {
            put(&mut o, "gradient", gradient(d, colors, false)?);
        }
        b"FrFX" => {
            put(&mut o, "position", named(d, b"Styl", &STROKE_POSITION_IDS)?);
            put(&mut o, "fill_type", named(d, b"PntT", &STROKE_FILL_IDS)?);
            put(&mut o, "size", d.unit(b"Sz  ")?);
            put(&mut o, "gradient", gradient(d, colors, false)?);
        }
        // `SoFi` carries the common keys only.
        _ => {}
    }
    crate::psd_contour::read_keys(class, d, &mut o)?;
    Ok(Value::Object(o))
}

// --------------------------------------------------------------- writing --

fn id(name: &[u8]) -> Vec<u8> {
    name.to_vec()
}

fn enumeration(type_id: &[u8], enum_id: &[u8]) -> Item {
    Item::Enum(id(type_id), id(enum_id))
}

pub(crate) fn blend_item(mode: crate::blend::BlendMode) -> Item {
    let index = (0..BLEND_IDS.len())
        .find(|index| crate::psd::map_psd_blend(*index as i32 + 1) == mode)
        .unwrap_or(0);
    enumeration(b"BlnM", BLEND_IDS[index])
}

/// [`blend_item`], and a group's pass-through as Photoshop writes it in a
/// layer comp's `blendOptions`: `passThrough`, a string id.
pub(crate) fn comp_blend_item(mode: BlendMode) -> Item {
    if mode == BlendMode::PassThrough {
        return enumeration(b"BlnM", b"passThrough");
    }
    blend_item(mode)
}

fn named_item(type_id: &[u8], table: &Ids, name: &str) -> Item {
    let value = table
        .iter()
        .find(|(_, candidate)| *candidate == name)
        .map_or(table[0].0, |(id, _)| *id);
    enumeration(type_id, value)
}

fn unit(tag: &[u8; 4], value: f64) -> Item {
    Item::Unit(*tag, value)
}

pub(crate) fn percent(value: f32) -> Item {
    unit(b"#Prc", f64::from(value) * 100.0)
}

fn pixels(value: f32) -> Item {
    unit(b"#Pxl", f64::from(value))
}

fn degrees(value: f32) -> Item {
    unit(b"#Ang", f64::from(value))
}

/// One colour slot as its descriptor (module doc): an RGB document's `RGBC`
/// in its own space — a wide colour as the document's numbers on the byte
/// grid (`Rgb`) or fractional (`Rgb16`) —; a CMYK document's `CMYC` of the
/// slot's ink (its four-decimal percent), or the hex's `RGBC` verbatim where
/// it has none; a gray file's `Grsc`; a Lab file's `LbCl` of the hex; a
/// 32-bit float document's `RGBC` of its linear light × 255 (`Float`).
pub(crate) fn colour_item(
    color: [u8; 3],
    ink: Option<[f32; 4]>,
    linear: Option<[f32; 3]>,
    colours: FxColours<'_>,
) -> Item {
    match (colours, ink) {
        (FxColours::Rgb(colors), _) => match linear {
            Some(linear) => rgbc_numbers(wide_into(colors, linear, false)),
            None => rgbc(color, colors),
        },
        // A Lab document's `LbCl` (#1 Lab phase 3b): the authored sRGB hex
        // as its L*, a*, b* (`psd_export_lab::srgb_lbcl`, the exact inverse
        // of the read's `lbcl_srgb`). The wide `linear` slot is not written:
        // a Lab document's style colours are authored sRGB bytes.
        (FxColours::Lab, _) => Item::Object(Descriptor {
            class: id(b"LbCl"),
            items: [&b"Lmnc"[..], b"A   ", b"B   "]
                .into_iter()
                .zip(crate::psd_export_lab::srgb_lbcl(color))
                .map(|(key, value)| (id(key), Item::Double(value)))
                .collect(),
        }),
        (FxColours::Rgb16(colors), _) => match linear {
            Some(linear) => rgbc_numbers(wide_into(colors, linear, true)),
            None => rgbc(color, colors),
        },
        // A 32-bit document's numbers are its linear light × 255.
        (FxColours::Float(style), _) => {
            rgbc_numbers(style.descriptor_numbers(crate::style_render::unit_color(color), linear))
        }
        (FxColours::Cmyk, Some(ink)) => Item::Object(Descriptor {
            class: id(b"CMYC"),
            items: INK_KEYS
                .into_iter()
                .zip(ink)
                .map(|(key, v)| (id(key), Item::Double((f64::from(v) * 1e6).round() / 1e4)))
                .collect(),
        }),
        (FxColours::Cmyk, None) => rgbc(color, None),
        (FxColours::Gray(policy), _) => Item::Object(Descriptor {
            class: id(b"Grsc"),
            items: vec![(
                id(b"Gry "),
                Item::Double(crate::gray_style::grsc_k_percent(policy, color, linear)),
            )],
        }),
    }
}

/// One authored sRGB colour as the `RGBC` descriptor of the DOCUMENT's space.
fn rgbc(color: [u8; 3], colors: Option<&AuthoredColors>) -> Item {
    let color = colors.map_or(color, |transform| transform.color(color));
    Item::Object(Descriptor {
        class: id(b"RGBC"),
        items: [&b"Rd  "[..], b"Grn ", b"Bl  "]
            .into_iter()
            .zip(color)
            .map(|(key, component)| (id(key), Item::Double(f64::from(component))))
            .collect(),
    })
}

/// Document numbers in 0…1 as an `RGBC` descriptor of `255 ·` each. A
/// component within 1e-4 of a code is written AS that code: the byte grid's
/// `k / 255` in f32 lands up to 2e-5 off `k` once widened and scaled, and an
/// 8-bit file's colour is an integer.
fn rgbc_numbers(numbers: [f32; 3]) -> Item {
    let code = |n: f32| {
        let v = f64::from(n) * 255.0;
        if (v - v.round()).abs() <= 1e-4 {
            v.round()
        } else {
            v
        }
    };
    Item::Object(Descriptor {
        class: id(b"RGBC"),
        items: [&b"Rd  "[..], b"Grn ", b"Bl  "]
            .into_iter()
            .zip(numbers)
            .map(|(key, n)| (id(key), Item::Double(code(n))))
            .collect(),
    })
}

fn point_zero() -> Item {
    Item::Object(Descriptor {
        class: id(b"Pnt "),
        items: vec![
            (id(b"Hrzn"), unit(b"#Prc", 0.0)),
            (id(b"Vrtc"), unit(b"#Prc", 0.0)),
        ],
    })
}

/// The `Grdn` descriptor: one `Clrt` colour stop and one `TrnS` transparency
/// stop per style stop, at the same `Lctn`, each with its `Mdpn` (the
/// PREVIOUS stop's `midpoint` and `opacity_midpoint` in whole percent, stop 0
/// 50: Photoshop's segment `i` reads stop `i + 1`'s). `linear[i]` (a `GdFl` fill
/// layer's wide stops, `psd_gradient_fill`) is `fill.stops[i]`'s wide colour;
/// every effect passes `None`.
pub(crate) fn gradient_items(
    fill: &GradientFill,
    colors: FxColours<'_>,
    linear: Option<&[[f32; 3]]>,
) -> Vec<(Vec<u8>, Item)> {
    let location = |position: f32| (f64::from(position) * LOCATION).round() as i32;
    let percent_of = |m: f32| (f64::from(m) * 100.0).round() as i32;
    // Photoshop stop `i + 1` carries our stop `i`'s midpoint (the reader's
    // shift); stop 0 writes 50. `fill.stops` is in position order.
    let shifted = |i: usize, of: fn(&crate::style::GradientStop) -> f32| {
        i.checked_sub(1)
            .map_or(50, |previous| percent_of(of(&fill.stops[previous])))
    };
    let grdn = Descriptor {
        class: id(b"Grdn"),
        items: vec![
            (id(b"Nm  "), Item::Text(String::new())),
            (id(b"GrdF"), enumeration(b"GrdF", b"CstS")),
            (
                id(b"Intr"),
                Item::Double(f64::from(fill.smoothness) * LOCATION),
            ),
            (
                id(b"Clrs"),
                Item::List(
                    fill.stops
                        .iter()
                        .enumerate()
                        .map(|(i, stop)| {
                            let wide = linear.and_then(|linear| linear.get(i).copied());
                            Item::Object(Descriptor {
                                class: id(b"Clrt"),
                                items: vec![
                                    (id(b"Clr "), colour_item(stop.color, stop.ink, wide, colors)),
                                    (id(b"Type"), enumeration(b"Clry", b"UsrS")),
                                    (id(b"Lctn"), Item::Long(location(stop.position))),
                                    (id(b"Mdpn"), Item::Long(shifted(i, |s| s.midpoint))),
                                ],
                            })
                        })
                        .collect(),
                ),
            ),
            (
                id(b"Trns"),
                Item::List(
                    fill.stops
                        .iter()
                        .enumerate()
                        .map(|(i, stop)| {
                            Item::Object(Descriptor {
                                class: id(b"TrnS"),
                                items: vec![
                                    (id(b"Opct"), percent(stop.opacity)),
                                    (id(b"Lctn"), Item::Long(location(stop.position))),
                                    (id(b"Mdpn"), Item::Long(shifted(i, |s| s.opacity_midpoint))),
                                ],
                            })
                        })
                        .collect(),
                ),
            ),
        ],
    };
    vec![
        (id(b"Grad"), Item::Object(grdn)),
        (id(b"Angl"), degrees(fill.angle)),
        (
            id(b"Type"),
            named_item(
                b"GrdT",
                &GRADIENT_STYLE_IDS,
                match fill.style {
                    GradientStyle::Linear => "linear",
                    GradientStyle::Radial => "radial",
                    GradientStyle::Angle => "angle",
                    GradientStyle::Reflected => "reflected",
                    GradientStyle::Diamond => "diamond",
                    GradientStyle::ShapeBurst => "shape_burst",
                },
            ),
        ),
        (id(b"Rvrs"), Item::Bool(fill.reverse)),
        (id(b"Algn"), Item::Bool(fill.align_with_layer)),
        (id(b"Scl "), percent(fill.scale)),
        (id(b"Dthr"), Item::Bool(fill.dither)),
        (id(b"Ofst"), point_zero()),
    ]
}

fn glow_source(source: GlowSource) -> Item {
    named_item(
        b"IGSr",
        &GLOW_SOURCE_IDS,
        match source {
            GlowSource::Edge => "edge",
            GlowSource::Center => "center",
        },
    )
}

fn bevel_style(style: BevelStyle) -> Item {
    named_item(
        b"BESl",
        &BEVEL_STYLE_IDS,
        match style {
            BevelStyle::OuterBevel => "outer_bevel",
            BevelStyle::InnerBevel => "inner_bevel",
            BevelStyle::Emboss => "emboss",
            BevelStyle::PillowEmboss => "pillow_emboss",
        },
    )
}

fn bevel_direction(direction: BevelDirection) -> Item {
    named_item(
        b"BESs",
        &BEVEL_DIRECTION_IDS,
        match direction {
            BevelDirection::Up => "up",
            BevelDirection::Down => "down",
        },
    )
}

fn bevel_technique(technique: BevelTechnique) -> Item {
    named_item(
        b"bvlT",
        &BEVEL_TECHNIQUE_IDS,
        match technique {
            BevelTechnique::Smooth => "smooth",
            BevelTechnique::ChiselHard => "chisel_hard",
            BevelTechnique::ChiselSoft => "chisel_soft",
        },
    )
}

fn stroke_position(position: StrokePosition) -> Item {
    named_item(
        b"FStl",
        &STROKE_POSITION_IDS,
        match position {
            StrokePosition::Outside => "outside",
            StrokePosition::Inside => "inside",
            StrokePosition::Center => "center",
        },
    )
}

fn stroke_fill(fill: StrokeFill) -> Item {
    named_item(
        b"FrFl",
        &STROKE_FILL_IDS,
        match fill {
            StrokeFill::Color => "color",
            StrokeFill::Gradient => "gradient",
            StrokeFill::Pattern => "pattern",
        },
    )
}

/// The shared head of every effect but the bevel: the switch, the blend and
/// the opacity.
pub(crate) fn head(
    enabled: bool,
    mode: crate::blend::BlendMode,
    opacity: f32,
) -> Vec<(Vec<u8>, Item)> {
    vec![
        (id(b"enab"), Item::Bool(enabled)),
        (id(b"Md  "), blend_item(mode)),
        (id(b"Opct"), percent(opacity)),
    ]
}

pub(crate) fn tail(items: &mut Vec<(Vec<u8>, Item)>) {
    items.push((id(b"present"), Item::Bool(true)));
    items.push((id(b"showInDialog"), Item::Bool(true)));
}

/// The drop and inner shadow's shared keys; `spread` is the drop shadow's
/// spread and the inner shadow's choke, which share the `Ckmt` key.
struct Shadow {
    enabled: bool,
    mode: crate::blend::BlendMode,
    color: [u8; 3],
    ink: Option<[f32; 4]>,
    linear: Option<[f32; 3]>,
    opacity: f32,
    use_global_light: bool,
    angle: f32,
    distance: f32,
    spread: f32,
    size: f32,
    contour: Contour,
    anti_aliased: bool,
    noise: f32,
}

fn shadow_items(shadow: Shadow, colors: FxColours<'_>) -> Vec<(Vec<u8>, Item)> {
    let mut items = head(shadow.enabled, shadow.mode, shadow.opacity);
    items.insert(
        2,
        (
            id(b"Clr "),
            colour_item(shadow.color, shadow.ink, shadow.linear, colors),
        ),
    );
    items.extend([
        (id(b"uglg"), Item::Bool(shadow.use_global_light)),
        (id(b"lagl"), degrees(shadow.angle)),
        (id(b"Dstn"), pixels(shadow.distance)),
        (id(b"Ckmt"), percent(shadow.spread)),
        (id(b"blur"), pixels(shadow.size)),
        (id(b"Nose"), percent(shadow.noise)),
        (
            id(b"AntA"),
            anti_alias_item(shadow.anti_aliased, &shadow.contour),
        ),
    ]);
    items
}

/// A glow's `Grad` (`style_glow_gradient`): the `Grdn` item of
/// [`gradient_items`] alone — a glow's gradient has no box, angle or style.
fn glow_gradient_item(fill: &GradientFill, colors: FxColours<'_>) -> (Vec<u8>, Item) {
    gradient_items(fill, colors, None).swap_remove(0)
}

#[allow(clippy::too_many_arguments)]
fn glow_items(
    enabled: bool,
    mode: crate::blend::BlendMode,
    color: ([u8; 3], Option<[f32; 4]>, Option<[f32; 3]>),
    opacity: f32,
    choke: f32,
    size: f32,
    noise: f32,
    fill: (GlowFill, &GradientFill, f32),
    contour: (&Contour, bool, f32),
    colors: FxColours<'_>,
) -> Vec<(Vec<u8>, Item)> {
    let (contour, anti_aliased, range) = contour;
    let (fill_type, gradient, jitter) = fill;
    let (color, ink, linear) = color;
    let mut items = head(enabled, mode, opacity);
    // `Clr ` in every mode, so a Gradient glow keeps its colour.
    items.insert(2, (id(b"Clr "), colour_item(color, ink, linear, colors)));
    items.extend([
        (id(b"Ckmt"), percent(choke)),
        (id(b"blur"), pixels(size)),
        (id(b"Nose"), percent(noise)),
        (id(b"ShdN"), percent(jitter)),
    ]);
    if fill_type == GlowFill::Gradient {
        items.push(glow_gradient_item(gradient, colors));
    }
    items.extend([
        (id(b"AntA"), anti_alias_item(anti_aliased, contour)),
        (id(b"GlwT"), enumeration(b"BETE", b"SfBL")),
        (id(b"Inpr"), percent(range)),
    ]);
    items
}

/// One effect as its `lfx2` item: the class key and the `Objc` value.
/// `light` `Some` writes each `use_global_light` effect's angle and altitude
/// resolved under it (a PSD record); `None` writes the effect's own stored
/// values (an `.asl` preset, which carries no document light).
pub(crate) fn write(
    effect: &Effect,
    light: Option<GlobalLight>,
    colors: FxColours<'_>,
) -> (Vec<u8>, Item) {
    let (key, mut items) = match effect {
        Effect::DropShadow(e) => {
            let mut items = shadow_items(
                Shadow {
                    enabled: e.enabled,
                    mode: e.blend,
                    color: e.color,
                    ink: e.ink,
                    linear: e.linear,
                    opacity: e.opacity,
                    use_global_light: e.use_global_light,
                    angle: light.map_or(e.angle, |light| e.resolved_angle(light)),
                    distance: e.distance,
                    spread: e.spread,
                    size: e.size,
                    contour: e.contour,
                    anti_aliased: e.anti_aliased,
                    noise: e.noise,
                },
                colors,
            );
            items.push((id(b"layerConceals"), Item::Bool(e.layer_knocks_out)));
            items.push((id(b"TrnS"), shpc_item(&e.contour)));
            (&b"DrSh"[..], items)
        }
        Effect::InnerShadow(e) => {
            let mut items = shadow_items(
                Shadow {
                    enabled: e.enabled,
                    mode: e.blend,
                    color: e.color,
                    ink: e.ink,
                    linear: e.linear,
                    opacity: e.opacity,
                    use_global_light: e.use_global_light,
                    angle: light.map_or(e.angle, |light| e.resolved_angle(light)),
                    distance: e.distance,
                    spread: e.choke,
                    size: e.size,
                    contour: e.contour,
                    anti_aliased: e.anti_aliased,
                    noise: e.noise,
                },
                colors,
            );
            items.push((id(b"TrnS"), shpc_item(&e.contour)));
            (&b"IrSh"[..], items)
        }
        Effect::OuterGlow(e) => {
            let mut items = glow_items(
                e.enabled,
                e.blend,
                (e.color, e.ink, e.linear),
                e.opacity,
                e.spread,
                e.size,
                e.noise,
                (e.fill_type, &e.gradient, e.jitter),
                (&e.contour, e.anti_aliased, e.range),
                colors,
            );
            items.push((id(b"TrnS"), shpc_item(&e.contour)));
            (&b"OrGl"[..], items)
        }
        Effect::InnerGlow(e) => {
            let mut items = glow_items(
                e.enabled,
                e.blend,
                (e.color, e.ink, e.linear),
                e.opacity,
                e.choke,
                e.size,
                e.noise,
                (e.fill_type, &e.gradient, e.jitter),
                (&e.contour, e.anti_aliased, e.range),
                colors,
            );
            items.push((id(b"glwS"), glow_source(e.source)));
            items.push((id(b"TrnS"), shpc_item(&e.contour)));
            (&b"IrGl"[..], items)
        }
        Effect::BevelEmboss(e) => {
            let mut items = vec![
                (id(b"enab"), Item::Bool(e.enabled)),
                (id(b"hglM"), blend_item(e.highlight_blend)),
                (
                    id(b"hglC"),
                    colour_item(
                        e.highlight_color,
                        e.highlight_ink,
                        e.highlight_linear,
                        colors,
                    ),
                ),
                (id(b"hglO"), percent(e.highlight_opacity)),
                (id(b"sdwM"), blend_item(e.shadow_blend)),
                (
                    id(b"sdwC"),
                    colour_item(e.shadow_color, e.shadow_ink, e.shadow_linear, colors),
                ),
                (id(b"sdwO"), percent(e.shadow_opacity)),
                (id(b"bvlS"), bevel_style(e.style)),
                (id(b"bvlD"), bevel_direction(e.direction)),
                (id(b"uglg"), Item::Bool(e.use_global_light)),
                (
                    id(b"lagl"),
                    degrees(light.map_or(e.angle, |light| e.resolved_angle(light))),
                ),
                (
                    id(b"Lald"),
                    degrees(light.map_or(e.altitude, |light| e.resolved_altitude(light))),
                ),
                (id(b"srgR"), percent(e.depth)),
                (id(b"blur"), pixels(e.size)),
                (id(b"Sftn"), pixels(e.soften)),
                (id(b"bvlT"), bevel_technique(e.technique)),
                (id(b"antialiasGloss"), Item::Bool(e.gloss_anti_aliased)),
                (id(b"useShape"), Item::Bool(e.contour_enabled)),
            ];
            // The Contour sub-panel's own keys only when it is on.
            if e.contour_enabled {
                items.push((
                    id(b"AntA"),
                    anti_alias_item(e.contour_anti_aliased, &e.contour),
                ));
                items.push((id(b"Inpr"), percent(e.contour_range)));
            }
            // The Texture sub-panel: its keys only when it is on, so
            // every untextured bevel writes the bytes it did.
            items.push((id(b"useTexture"), Item::Bool(e.texture_enabled)));
            if e.texture_enabled {
                items.extend(crate::psd_pattern_fill::texture_items(&e.texture));
            }
            items.extend([
                (id(b"TrnS"), shpc_item(&e.gloss_contour)),
                (id(b"MpgS"), shpc_item(&e.contour)),
            ]);
            (&b"ebbl"[..], items)
        }
        Effect::Satin(e) => {
            let mut items = head(e.enabled, e.blend, e.opacity);
            items.insert(
                2,
                (id(b"Clr "), colour_item(e.color, e.ink, e.linear, colors)),
            );
            items.extend([
                (id(b"lagl"), degrees(e.angle)),
                (id(b"Dstn"), pixels(e.distance)),
                (id(b"blur"), pixels(e.size)),
                (id(b"Invr"), Item::Bool(e.invert)),
                (id(b"AntA"), anti_alias_item(e.anti_aliased, &e.contour)),
                (id(b"MpgS"), shpc_item(&e.contour)),
            ]);
            (&b"ChFX"[..], items)
        }
        Effect::ColorOverlay(e) => {
            let mut items = head(e.enabled, e.blend, e.opacity);
            items.insert(
                2,
                (id(b"Clr "), colour_item(e.color, e.ink, e.linear, colors)),
            );
            (&b"SoFi"[..], items)
        }
        Effect::GradientOverlay(e) => {
            let mut items = head(e.enabled, e.blend, e.opacity);
            items.extend(gradient_items(&e.gradient, colors, None));
            (&b"GrFl"[..], items)
        }
        Effect::PatternOverlay(e) => return crate::psd_pattern_fill::write(e),
        Effect::Stroke(e) => {
            let mut items = vec![
                (id(b"enab"), Item::Bool(e.enabled)),
                (id(b"Styl"), stroke_position(e.position)),
                (id(b"PntT"), stroke_fill(e.fill_type)),
                (id(b"Md  "), blend_item(e.blend)),
                (id(b"Opct"), percent(e.opacity)),
                (id(b"Sz  "), pixels(e.size)),
            ];
            match e.fill_type {
                StrokeFill::Color => {
                    items.push((id(b"Clr "), colour_item(e.color, e.ink, e.linear, colors)))
                }
                StrokeFill::Gradient => items.extend(gradient_items(&e.gradient, colors, None)),
                StrokeFill::Pattern => items.extend(crate::psd_pattern_fill::fill_items(
                    &e.pattern_fill,
                    b"Lnkd",
                )),
            }
            (&b"FrFX"[..], items)
        }
    };
    tail(&mut items);
    (
        id(key),
        Item::Object(Descriptor {
            class: id(key),
            items,
        }),
    )
}
