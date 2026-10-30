//! The ONE codec for Photoshop's layer-effects and blending-options
//! constructs: the `lfx2` and `lmfx` descriptor blocks, the `iOpa` fill-opacity block and
//! the layer record's blending ranges, in BOTH directions and at BOTH depths.
//! A record carrying them imports as one Doctored `LayerStyle`; a Doctored
//! style exports as the same three constructs, and is rasterized
//! (`psd_export_rasterize`) only for what Photoshop's own dialog or a PSD mask
//! cannot hold. Per-effect descriptor reading and writing lives in
//! `psd_layer_effects_fx`; `psd_descriptor` is the framing and owns no key.
//!
//! # `lfx2`
//!
//! Body: `u32 0` then a `psd_descriptor` version-16 structure of class
//! `null`. A leading word other than 0, a descriptor that does not read, or
//! another class is NOT a decline — it is simply not an effects body, and the
//! record keeps its unchanged presence-only marker sentence.
//!
//! | key | value | mapping (← import, → export) |
//! |---|---|---|
//! | `Scl ` | UntF `#Prc` | → 100; ← a value ≠ 100 applies `LayerStyle::scaled(v / 100)` |
//! | `masterFXSwitch` | bool | → true; ← false declines |
//! | `DrSh` `IrSh` `OrGl` `IrGl` `ebbl` `ChFX` `SoFi` `GrFl` `FrFX` | Objc, class = key | one effect each: drop shadow, inner shadow, outer glow, inner glow, bevel & emboss, satin, colour overlay, gradient overlay, stroke |
//!
//! Common effect keys: `enab` ←→ `enabled`; `present`/`showInDialog` → true;
//! `Md  ` enum `BlnM` (one id table in `PSD_BLEND_KEYS` order, read through
//! `psd::map_psd_blend(i + 1)`); `Clr ` Objc `RGBC` {`Rd  ` `Grn ` `Bl  `
//! doub 0…255} in the DOCUMENT's RGB — a style colour is authored sRGB, so
//! `→` converts with `AuthoredColors::color` and `←` with the reverse
//! (`authored_color`: the matrix transform, or the LUT pipeline on a LUT
//! profile that converts both ways), at both depths; `Opct` `#Prc` =
//! opacity × 100.
//!
//! Per effect: DrSh/IrSh `uglg`, `lagl` `#Ang` (→ the RESOLVED angle), `Dstn`
//! `#Pxl`, `Ckmt` `#Prc` spread/choke × 100, `blur` `#Pxl` size, DrSh
//! `layerConceals`. OrGl/IrGl `Ckmt`, `blur`, IrGl `glwS` enum `IGSr`
//! `SrcE`/`SrcC`; a `Grad` in either declines. ebbl `hglM hglC hglO sdwM sdwC
//! sdwO`, `bvlS` enum `BESl` `InrB OtrB Embs PlEb` (`strokeEmboss` declines),
//! `bvlD` enum `BESs` `In  ` up / `Out ` down, `uglg`, `lagl`, `Lald` `#Ang`
//! altitude, `srgR` `#Prc` depth × 100, `blur` size, `Sftn` `#Pxl`. ChFX
//! `Invr`, `lagl`, `Dstn`, `blur`. GrFl `Grad`, `Angl` `#Ang`, `Type` enum
//! `GrdT` `Lnr ` `Rdl ` `Angl` `Rflc` `Dmnd` (and the stroke's string id
//! `shapeburst`), `Rvrs`, `Algn`, `Scl ` `#Prc` scale × 100, `Dthr`
//! bool dither. FrFX `Styl` enum `FStl` `OutF InsF CtrF`, `PntT` enum `FrFl`
//! `SClr`/`GrFl`/`Ptrn`, `Sz  ` `#Pxl`, then `Clr `, the GrFl gradient keys,
//! or the pattern fill's items (`Ptrn`, `Angl`, `Scl `, `Lnkd`, `phase`; its
//! tile in `Patt`/`Pat2`, `psd_pattern_fill`; a pattern the file does not
//! carry declines).
//!
//! `Grad` = Objc `Grdn` {`Nm  ` "", `GrdF` enum `GrdF` `CstS` (else
//! declines), `Intr` doub Smoothness × 4096 (read as `smoothness` =
//! `Intr` / 4096 when present and not 4096; written from it, 4096 at the
//! default 100 %), `Clrs` VlLs of Objc `Clrt` {`Clr ` RGBC,
//! `Type` enum `Clry` `UsrS`, `Lctn` long round(position × 4096), `Mdpn` long
//! round(midpoint × 100) of the PREVIOUS stop}, `Trns` VlLs of Objc `TrnS`
//! {`Opct` `#Prc`, `Lctn`, `Mdpn` long round(opacity_midpoint × 100) of the
//! previous stop}, one per stop at the same `Lctn`; stop 0's `Mdpn` is 50
//! (Photoshop's segment `i` reads stop `i + 1`'s `Mdpn`, so in
//! position order our stop `i`'s `midpoint` / `opacity_midpoint` = stop
//! `i + 1`'s `Mdpn` / 100 when present and not 50; 50 at the default; a
//! PSD or `.asl` an earlier build wrote, which put stop `i`'s own there,
//! re-reads its midpoints one stop earlier)}. A transparency list whose count or locations
//! differ from `Clrs`, or a stop count outside 2…32, declines.
//!
//! Contours (`psd_contour`), carried both ways: DrSh, IrSh, OrGl and
//! IrGl `TrnS` Objc `ShpC` → `contour`, `AntA` → `anti_aliased`; OrGl and
//! IrGl `Inpr` `#Prc` → `range` (as written, enabled or not); ChFX `MpgS` →
//! `contour`, `AntA` → `anti_aliased`; ebbl `TrnS` → `gloss_contour`,
//! `antialiasGloss` → `gloss_anti_aliased`, `useShape` → `contour_enabled`
//! and, only when it is true, `MpgS` → `contour`, `AntA` →
//! `contour_anti_aliased`, `Inpr` → `contour_range`. `ShpC` = {`Nm  `, optional
//! `Crv ` VlLs of `CrPt` {`Hrzn`, `Vrtc`, optional `Cnty` (false = a
//! corner)}}: Linear writes {`Nm  ` "Linear", `Crv ` [CrPt {0, 0}, {255,
//! 255}]} as before, another preset its `$$$/Contours/Defaults/…` name, a
//! custom curve `Nm  ` "Custom" and its `Crv `. `AntA` is written as
//! `anti_aliased || contour is Linear`; an `AntA`/`antialiasGloss` is read
//! only beside a contour that is not Linear; ebbl writes `AntA`/`Inpr` only
//! when `useShape` is true.
//!
//! `Nose` ↔ `noise` on the shadows and glows (read only
//! when above 0). On `OrGl`/`IrGl` (`style_glow_gradient`): `Grad` ↔
//! `fill_type` gradient with the `gradient`'s stops (written only for a
//! Gradient glow; `Clr ` is written and read in either mode) and `ShdN` ↔
//! `jitter` (read only when above 0). `useTexture` ↔ `texture_enabled` and,
//! when it is on, the texture's `Ptrn`, `Scl `, `textureDepth`, `InvT`,
//! `Algn` and `phase` ↔ `texture` on `ebbl`
//! (`psd_pattern_fill`). `bvlT` ↔ `technique` on `ebbl` (#578): `SfBL` smooth
//! (read as no key), `PrBL` chisel_hard, `Slmt` chisel_soft. Written-only
//! defaults, IGNORED on read: `GlwT` enum `BETE` `SfBL`, `Ofst` Objc `Pnt `
//! {`Hrzn` 0, `Vrtc` 0}. Unit tags are ignored on read.
//!
//! Import builds the whole style as JSON and parses it with
//! `LayerStyle::from_value(_, Strictness::Strict)`, so numbers clamp and
//! quantize exactly as every other style does and a parse error IS the
//! decline reason. An `lrFX` beside an `lfx2` or `lmfx` is Photoshop's
//! legacy duplicate and does not block the carry. A GROUP record's
//! unstacked effects are `lfxs` in Photoshop's files (the `lfx2` body under
//! a section key: `blend-if-multichannel-558/take1`'s p1–p3, and on a Normal
//! group `convert-smart-object-158/take2`'s `BI_event.psd`), so import reads
//! the first `lfx2` or `lfxs` of a record and export writes `lfxs` on every
//! group record's unstacked style.
//!
//! # `lmfx`: stacked instances
//!
//! A style holding two or more instances of one kind (`style_instances`) is
//! Photoshop's `lmfx` block, and Photoshop writes it INSTEAD of `lfx2`
//! (`stacked-effects-557/take1`: every stacked case's record holds `lmfx`
//! and `lrFX`, no `lfx2`). Its body is the `lfx2` framing, `u32 0` then a
//! version-16 `null` descriptor, keyed in Photoshop's order: `Scl `,
//! `masterFXSwitch`, `dropShadowMulti`, `innerShadowMulti`, `OrGl`,
//! `solidFillMulti`, `gradientFillMulti`, `frameFXMulti`, `IrGl`, `ebbl`,
//! `ChFX`, `numModifyingFX` (long: the count of enabled effects). Each of the
//! five repeating kinds is a VlLs of the `lfx2` effect objects ([`MULTI`]),
//! the other four single objects. Photoshop's list holds the instances TOP
//! FIRST (take1: list[0] draws on top) and Doctored's array bottom first, so
//! [`listed`] is the ONE place either direction reverses. The writer omits a
//! kind the style does not hold (Photoshop writes a one-item list of a
//! disabled default there; the `lfx2` writer omits it too) and writes a
//! repeating kind it holds once as a one-item list, as Photoshop does. An
//! unstacked style writes exactly the `lfx2` it always did ([`effect_blocks`]).
//!
//! On read the effects come from the record's FIRST `lmfx` when it frames,
//! else from its `lfx2`; within the chosen descriptor a kind's `<kind>Multi`
//! list applies when present, else its single key. A list of more than ten
//! declines ([`CAPPED`]), as does an item that is not an effect object.
//!
//! # `iOpa` and the blending ranges
//!
//! `iOpa` = `[round(fill × 255), 0, 0, 0]`; it is carried back only in
//! exactly that framing, and 255 is the 100 % default, not a carry. A pixel
//! record in Color Burn, Color Dodge or Difference writes it at 255 too
//! ([`encode_fill_for`], #578): without it Photoshop reads the record as a
//! legacy layer.
//!
//! The record's blending ranges are 40 bytes: the pairs gray, red, green,
//! blue, alpha of `[lo0, lo1, hi0, hi1]` this layer then `[…]` underlying,
//! with every pair Doctored does not use at its unrestricted
//! `[0, 0, 255, 255]` default. They are carried back when the section's
//! length is at least 8 and a multiple of 8 and every nondefault slot is one
//! of 0…3 (gray, red, green, blue): each nondefault slot becomes one range of
//! a multichannel Blend If (a nondefault alpha slot 4 or any
//! later slot is not carried, as before).
//!
//! A CMYK file's (#547) are the same 40 bytes over the pairs gray, cyan,
//! magenta, yellow and black (Photoshop take1, `cmyk-styles-547/take1/psd`:
//! a Cyan This Layer `[0, 0, 60, 140]` is the second pair's first word, the
//! slider numbers as set), carried back from every nondefault slot of 0…4,
//! and written by [`encode_ranges_for`], each range in its own slot. An effect colour there is a
//! `CMYC` ink (`psd_layer_effects_fx::FxColours::Cmyk`).
//!
//! At 32 bits an effect colour's `RGBC` numbers are the document's LINEAR light
//! × 255 (`float-styles-473/take5`; `FxColours::Float` writes them through
//! `style_float::FloatStyle::descriptor_numbers`, and the 32-bit open reads them through the
//! profile's linear twin), never the encoded numbers an 8- or 16-bit file holds.
//!
//! # `knko` and `infx`
//!
//! Blending Options > Knockout and "Blend Interior Effects as Group" ride the
//! record's ONE style, on rasters and OPENING folder records at both depths.
//! `knko` = `[k, 0, 0, 0]` with k 0/1/2 = none/shallow/deep; `infx` =
//! `[0 | 1, 0, 0, 0]`. Any other framing — a different length, a nonzero pad
//! byte, an unrecognized k — is NOT carried and keeps the record's unchanged
//! presence-only marker sentence, exactly as `iOpa` does. `clbl` is
//! observation-only and is never interpreted. A CLOSING divider (`lsct` 3)
//! never carries either key, because its decoded style is attached to nothing.
//! A lone `infx` describes an identity style and stays `Absent`.
//!
//! They are WRITTEN only when non-default and only beside a style that is
//! written rather than baked, after `iOpa` and before `lclr`, `infx` first.
//!
//! `psd_layer_effects.rs` is the ONE codec for Photoshop's layer-effects and blending-options
//! constructs (`lfx2`, `iOpa` and the record's blending ranges), both directions and both depths,
//! (and `lmfx`'s `<kind>Multi` lists of stacked instances),
//! with `psd_layer_effects_fx.rs` holding the per-effect read/write for the nine `Objc` classes,
//! the `Grdn` gradient, the `ShpC` contour and the `RGBC` colour. A record's three constructs
//! decode into ONE `LayerStyle` built as style JSON and parsed with `LayerStyle::from_value(_,
//! Strict)`, attached to a raster or a folder at 8 bits and through `with_layer_style` at 16 (a
//! style the typed rules refuse (the padded-plane limit) is reported and dropped; the
//! file's 1037/1049 light becomes the document's own at both depths, so carried effects keep
//! `use_global_light`); `FeatureMarkers::carry_style` clears exactly the markers that were carried,
//! an identity result stays `Absent`, a body that is not an effects descriptor keeps its
//! presence-only marker sentence byte-for-byte, and an `lmfx` carries stacked instances in place
//! of the `lfx2`.

use serde_json::{json, Map, Value};

use crate::blend::BlendMode;
use crate::psd_descriptor::{self, Descriptor, Item};
use crate::psd_layer_effects_fx as fx;
pub(crate) use crate::psd_layer_effects_fx::FxColours;
use crate::style::{
    is_stacked, BlendIf, BlendIfChannel, BlendIfRange, Effect, EffectKind, GlobalLight, Knockout,
    LayerStyle, Strictness, FULL_RAMP, MAX_INSTANCES,
};
use crate::style_names::{enum_name, KNOCKOUTS};

const SWITCHED_OFF: &str = "layer effects are switched off";
/// A `<kind>Multi` list past [`MAX_INSTANCES`] (module doc, `lmfx`).
const CAPPED: &str = "list holds more than 10 instances of one effect";
/// A `<kind>Multi` item that is not an effect object.
const NOT_AN_EFFECT: &str = "stacked effect list holds an item that is not an effect";

/// The `lmfx` list key of each repeating kind (module doc,
/// `stacked-effects-557/take1`).
const MULTI: [(EffectKind, &str); 5] = [
    (EffectKind::DropShadow, "dropShadowMulti"),
    (EffectKind::InnerShadow, "innerShadowMulti"),
    (EffectKind::Stroke, "frameFXMulti"),
    (EffectKind::ColorOverlay, "solidFillMulti"),
    (EffectKind::GradientOverlay, "gradientFillMulti"),
];

/// The kinds in the order Photoshop keys them in `lmfx` (module doc).
const LMFX_ORDER: [EffectKind; 10] = [
    EffectKind::DropShadow,
    EffectKind::InnerShadow,
    EffectKind::OuterGlow,
    EffectKind::ColorOverlay,
    EffectKind::GradientOverlay,
    EffectKind::PatternOverlay,
    EffectKind::Stroke,
    EffectKind::InnerGlow,
    EffectKind::BevelEmboss,
    EffectKind::Satin,
];

/// The `lmfx` list key of `kind`, `None` for a kind that holds one.
fn multi_key(kind: EffectKind) -> Option<&'static str> {
    MULTI.iter().find(|(k, _)| *k == kind).map(|(_, key)| *key)
}

/// THE ONE list-order map (module doc): Photoshop's `<kind>Multi` list is
/// top first, Doctored's instances bottom first, so either direction is a
/// reversal (`stacked-effects-557/take1`, TOPFIRST).
fn listed<T>(mut items: Vec<T>) -> Vec<T> {
    items.reverse();
    items
}
const STROKE_MINIMUM: &str = "stroke size is below Photoshop's 1 px minimum";
const SATIN_RANGE: &str = "satin distance is outside Photoshop's 1–250 px range";
const MASK: &str = "enabled mask has density or feather, which a PSD mask cannot store";

/// The four Blend If channels in blending-ranges slot order.
const CHANNEL_SLOTS: [(BlendIfChannel, &str); 4] = [
    (BlendIfChannel::Gray, "gray"),
    (BlendIfChannel::Red, "red"),
    (BlendIfChannel::Green, "green"),
    (BlendIfChannel::Blue, "blue"),
];

/// A CMYK file's five Blend If channels in slot order (module doc).
const CMYK_CHANNEL_SLOTS: [(BlendIfChannel, &str); 5] = [
    (BlendIfChannel::Gray, "gray"),
    (BlendIfChannel::Cyan, "cyan"),
    (BlendIfChannel::Magenta, "magenta"),
    (BlendIfChannel::Yellow, "yellow"),
    (BlendIfChannel::Black, "black"),
];

/// The slot table of a file's colour mode.
fn channel_slots(cmyk: bool) -> &'static [(BlendIfChannel, &'static str)] {
    if cmyk {
        &CMYK_CHANNEL_SLOTS
    } else {
        &CHANNEL_SLOTS
    }
}

/// One record's carried style, with what each construct contributed — the
/// import report names them and `FeatureMarkers::carry_style` clears exactly
/// those markers.
pub(crate) struct Carried {
    pub style: LayerStyle,
    pub effects: Vec<&'static str>,
    pub fill: bool,
    pub ranges: bool,
    /// The parsed style kept the record's `knko`.
    pub knockout: bool,
    /// The parsed style kept the record's `infx`.
    pub interior: bool,
}

pub(crate) enum StyleBody {
    /// No effects body, no nondefault fill opacity and no carried ranges —
    /// or the three together describe an identity style.
    Absent,
    Carried(Carried),
    /// `reason` reads after "whose" in the import report.
    Declined {
        reason: String,
    },
}

/// The three constructs of ONE layer record, as `psd_group_records` framed
/// them: `effects` the FIRST `lfx2` body, `stacked` the FIRST `lmfx` body.
pub(crate) struct RecordStyle<'a> {
    pub effects: Option<&'a [u8]>,
    pub fill: Option<&'a [u8]>,
    pub ranges: &'a [u8],
    pub stacked: Option<&'a [u8]>,
    /// The FIRST `knko` body, verbatim, and `None` on a closing divider.
    pub knockout: Option<&'a [u8]>,
    /// The FIRST `infx` body, verbatim, and `None` on a closing divider.
    pub interior: Option<&'a [u8]>,
    /// The FIRST `fxrp` body, verbatim (`psd_pattern_fill`).
    pub reference: Option<&'a [u8]>,
    /// The record rectangle's top left (x, y): a pattern anchor's base.
    pub top_left: (i32, i32),
    /// The file's pattern table (`psd_patterns`).
    pub patterns: &'a crate::psd_patterns::PatternTable,
}

/// What a `patternFill` needs beside its descriptor (`psd_pattern_fill::read`).
pub(crate) struct PatternContext<'a> {
    pub patterns: &'a crate::psd_patterns::PatternTable,
    pub reference: Option<(f64, f64)>,
    pub top_left: (i32, i32),
}

// -------------------------------------------------------------- decoding --

/// `effect` (read by `psd_layer_effects_fx::read`) with the pattern fill
/// its class nests, through the file's pattern table and the record's
/// `fxrp`: an `FrFX` whose `PntT` is `Ptrn` gains its `pattern_fill`
/// (`psd_pattern_fill::read_fill` with the stroke's link key `Lnkd`), and an
/// `ebbl` its `texture_enabled` and `texture`
/// (`psd_pattern_fill::read_texture`; `useTexture` true with no texture read
/// declines). Every other effect comes back as it is.
fn with_pattern_fill(
    effect: Value,
    key: &[u8; 4],
    object: &Descriptor,
    pattern: &PatternContext<'_>,
) -> Result<Value, String> {
    if key == b"ebbl" {
        let used = object.bool(b"useTexture")?.unwrap_or(false);
        let texture = crate::psd_pattern_fill::read_texture(
            object,
            pattern.patterns,
            pattern.reference,
            pattern.top_left,
        )?;
        if used && texture.is_none() {
            return Err(crate::psd_pattern_fill::TEXTURE_MISSING.to_string());
        }
        let Value::Object(mut o) = effect else {
            return Ok(effect);
        };
        if used {
            o.insert("texture_enabled".into(), Value::Bool(true));
        }
        if let Some(texture) = texture {
            o.insert("texture".into(), Value::Object(texture));
        }
        return Ok(Value::Object(o));
    }
    if key != b"FrFX" || object.enumeration(b"PntT")?.is_none_or(|id| id != b"Ptrn") {
        return Ok(effect);
    }
    let fill = crate::psd_pattern_fill::read_fill(
        object,
        b"Lnkd",
        pattern.patterns,
        pattern.reference,
        pattern.top_left,
        crate::psd_pattern_fill::STROKE_MISSING,
    )?;
    let Value::Object(mut o) = effect else {
        return Ok(effect);
    };
    o.insert("pattern_fill".into(), Value::Object(fill));
    Ok(Value::Object(o))
}

/// The `lfx2` body's descriptor, or `None` when the body is not one at all.
fn framed(body: &[u8]) -> Option<Descriptor> {
    let version = body.get(..4)?;
    if u32::from_be_bytes(version.try_into().ok()?) != 0 {
        return None;
    }
    let descriptor = psd_descriptor::read(&body[4..]).ok()?;
    (descriptor.class == b"null").then_some(descriptor)
}

/// The fill opacity an `iOpa` body carries, or `None` when it is not in
/// Photoshop's exact framing or is the 100 % default.
fn decode_fill(body: &[u8]) -> Option<f32> {
    if body.len() != 4 || body[1..] != [0, 0, 0] || body[0] == 255 {
        return None;
    }
    Some(f32::from(body[0]) / 255.0)
}

/// The knockout a `knko` body carries, or `None` when it is not in
/// Photoshop's exact framing or is the `none` default. Adobe specifies one
/// value byte followed by three padding bytes.
fn decode_knockout(body: &[u8]) -> Option<Knockout> {
    if body.len() != 4 || body[1..] != [0, 0, 0] {
        return None;
    }
    match body[0] {
        1 => Some(Knockout::Shallow),
        2 => Some(Knockout::Deep),
        _ => None,
    }
}

/// Whether an `infx` body carries "Blend Interior Effects as Group", or
/// `None` when it is not in Photoshop's exact framing or is the off default.
fn decode_interior(body: &[u8]) -> Option<bool> {
    (body.len() == 4 && body[1..] == [0, 0, 0] && body[0] == 1).then_some(true)
}

/// Every nondefault channel pair of a blending-ranges section as one
/// canonical Blend If, or `None` (no nondefault pair, a malformed length, or
/// a nondefault pair outside the mode's slot table); a CMYK file's slots are
/// its five (module doc).
fn decode_ranges(bytes: &[u8], cmyk: bool) -> Option<BlendIf> {
    if bytes.len() < 8 || !bytes.len().is_multiple_of(8) {
        return None;
    }
    let table = channel_slots(cmyk);
    let mut ranges = Vec::new();
    for (index, slot) in bytes.chunks_exact(8).enumerate() {
        if slot[..4] == FULL_RAMP && slot[4..] == FULL_RAMP {
            continue;
        }
        let (channel, _) = table.get(index)?;
        ranges.push(BlendIfRange {
            channel: *channel,
            this_layer: slot[..4].try_into().ok()?,
            underlying: slot[4..].try_into().ok()?,
        });
    }
    let (first, rest) = ranges.split_first()?;
    BlendIf::from_ranges(*first, rest).ok()
}

/// The channels named in an import report sentence: `"gray"`, `"gray and
/// red"`, `"gray, red and blue"` (the slot names, [`channel_name`]).
pub(crate) fn channel_list(channels: &[BlendIfChannel]) -> String {
    let names: Vec<&str> = channels.iter().map(|c| channel_name(*c)).collect();
    match names.split_last() {
        None => String::new(),
        Some((last, [])) => (*last).to_string(),
        Some((last, init)) => format!("{} and {last}", init.join(", ")),
    }
}

/// Photoshop's title of the effect a `fx::CLASSES` type (or
/// `pattern_overlay`) names, for a dropped-effect sentence.
pub(crate) fn effect_title(name: &str) -> &'static str {
    match name {
        "drop_shadow" => "Drop Shadow",
        "inner_shadow" => "Inner Shadow",
        "outer_glow" => "Outer Glow",
        "inner_glow" => "Inner Glow",
        "bevel_emboss" => "Bevel & Emboss",
        "satin" => "Satin",
        "color_overlay" => "Color Overlay",
        "gradient_overlay" => "Gradient Overlay",
        "stroke" => "Stroke",
        _ => "Pattern Overlay",
    }
}

/// The instances one `fx::CLASSES` kind contributes: its `<kind>Multi` list
/// when present (Photoshop's top-first order reversed, [`listed`]), else its
/// single key, else none. `dropped` Some reads leniently: an instance whose
/// read errs is skipped and named, and so is a list past [`MAX_INSTANCES`]
/// or an item that is not an effect object.
fn read_kind(
    d: &Descriptor,
    key: [u8; 4],
    name: &'static str,
    colors: FxColours<'_>,
    pattern: &PatternContext<'_>,
    dropped: &mut Option<&mut Vec<String>>,
) -> Result<Vec<Value>, String> {
    let mut keep = |read: Result<Value, String>| -> Result<Option<Value>, String> {
        match (read, dropped.as_deref_mut()) {
            (Ok(effect), _) => Ok(Some(effect)),
            (Err(reason), Some(dropped)) => {
                dropped.push(format!("{}: {reason}", effect_title(name)));
                Ok(None)
            }
            (Err(reason), None) => Err(reason),
        }
    };
    let multi = EffectKind::from_type_name(name).and_then(multi_key);
    let items = match multi.map(|k| d.list(k.as_bytes())).transpose() {
        Ok(items) => items.flatten(),
        Err(reason) => return Ok(keep(Err(reason))?.into_iter().collect()),
    };
    if let Some(items) = items {
        if items.len() > MAX_INSTANCES {
            return Ok(keep(Err(CAPPED.to_string()))?.into_iter().collect());
        }
        let mut read = Vec::with_capacity(items.len());
        for item in items {
            let effect = match item {
                Item::Object(object) => fx::read(&key, name, object, colors)
                    .and_then(|effect| with_pattern_fill(effect, &key, object, pattern)),
                _ => Err(NOT_AN_EFFECT.to_string()),
            };
            read.extend(keep(effect)?);
        }
        return Ok(listed(read));
    }
    let object = match d.object(&key) {
        Ok(Some(object)) => object,
        Ok(None) => return Ok(Vec::new()),
        Err(reason) => return Ok(keep(Err(reason))?.into_iter().collect()),
    };
    let effect = fx::read(&key, name, object, colors)
        .and_then(|effect| with_pattern_fill(effect, &key, object, pattern));
    Ok(keep(effect)?.into_iter().collect())
}

/// The effect objects of an effects descriptor `d` (an `lfx2`/`lmfx` body,
/// or an `.asl` style's `Lefx`, `asl`) as style JSON values in
/// `fx::CLASSES` order, then the single `patternFill`. `dropped` None is
/// strict (the PSD importer: the first error is the record's decline
/// reason); Some is lenient: an effect whose read errs is skipped and
/// pushed as "<Title>: <reason>" ([`effect_title`]).
pub(crate) fn read_effect_list(
    d: &Descriptor,
    colors: FxColours<'_>,
    pattern: &PatternContext<'_>,
    mut dropped: Option<&mut Vec<String>>,
) -> Result<Vec<Value>, String> {
    let mut list = Vec::new();
    for (key, name) in fx::CLASSES {
        list.extend(read_kind(d, key, name, colors, pattern, &mut dropped)?);
    }
    // Pattern Overlay: one single key in both bodies (`psd_pattern_fill`),
    // carried at every mode (a CMYK record converts its tile to ink at
    // composite time, `cmyk_style::effect_inks`).
    let overlay = d.object(b"patternFill").and_then(|object| {
        object
            .map(|object| {
                crate::psd_pattern_fill::read(
                    object,
                    pattern.patterns,
                    pattern.reference,
                    pattern.top_left,
                )
            })
            .transpose()
    });
    match (overlay, dropped) {
        (Ok(effect), _) => list.extend(effect),
        (Err(reason), Some(dropped)) => {
            dropped.push(format!("{}: {reason}", effect_title("pattern_overlay")))
        }
        (Err(reason), None) => return Err(reason),
    }
    Ok(list)
}

/// The whole record as one style JSON, parsed strictly. `Ok(None)` is an
/// identity result — nothing to carry.
fn build(
    effects: Option<&Descriptor>,
    fill: Option<f32>,
    ranges: Option<BlendIf>,
    keys: (Option<Knockout>, Option<bool>),
    colors: FxColours<'_>,
    pattern: &PatternContext<'_>,
) -> Result<Option<Carried>, String> {
    let (knockout, interior) = keys;
    let mut root = Map::new();
    root.insert("fill_opacity".into(), json!(fill.unwrap_or(1.0)));
    if let Some(knockout) = knockout {
        root.insert(
            "knockout".into(),
            json!(enum_name(&KNOCKOUTS, knockout).to_string()),
        );
    }
    if interior == Some(true) {
        root.insert("blend_interior_as_group".into(), json!(true));
    }
    if let Some(blend_if) = ranges {
        root.insert(
            "blend_if".into(),
            crate::style_json_write::blend_if_value(&blend_if),
        );
    }
    if let Some(descriptor) = effects {
        if descriptor.bool(b"masterFXSwitch")? == Some(false) {
            return Err(SWITCHED_OFF.to_string());
        }
        let list = read_effect_list(descriptor, colors, pattern, None)?;
        root.insert("effects".into(), Value::Array(list));
    }
    let mut style = LayerStyle::from_value(&Value::Object(root), Strictness::Strict)?;
    if let Some(scale) = effects.map(|d| d.unit(b"Scl ")).transpose()?.flatten() {
        if scale.is_finite() && (scale - 100.0).abs() > f64::EPSILON {
            style = style.scaled(scale / 100.0);
        }
    }
    let style = style.normalized();
    if style.is_identity() {
        return Ok(None);
    }
    // One name a kind: stacked instances are adjacent after the sort.
    let mut names: Vec<&'static str> = style.effects.iter().map(|e| e.kind().type_name()).collect();
    names.dedup();
    Ok(Some(Carried {
        effects: names,
        fill: fill.is_some(),
        ranges: ranges.is_some() && style.blend_if.is_some(),
        knockout: knockout.is_some() && style.knockout != Knockout::None,
        interior: interior == Some(true) && style.blend_interior_as_group,
        style,
    }))
}

/// The record's three constructs as one Doctored style. `colors` is how an
/// effect colour crosses (`FxColours`: an RGB file's conversion back to the
/// sRGB every authored style colour is stored in, or a CMYK file's ink).
/// Never panics.
pub(crate) fn decode(record: RecordStyle<'_>, colors: FxColours<'_>) -> StyleBody {
    // The stacked `lmfx` when it frames, else the `lfx2` (module doc).
    let effects = record
        .stacked
        .and_then(framed)
        .or_else(|| record.effects.and_then(framed));
    let fill = record.fill.and_then(decode_fill);
    let ranges = decode_ranges(record.ranges, matches!(colors, FxColours::Cmyk));
    let knockout = record.knockout.and_then(decode_knockout);
    let interior = record.interior.and_then(decode_interior);
    if effects.is_none()
        && fill.is_none()
        && ranges.is_none()
        && knockout.is_none()
        && interior.is_none()
    {
        return StyleBody::Absent;
    }
    let pattern = PatternContext {
        patterns: record.patterns,
        reference: record
            .reference
            .and_then(crate::psd_pattern_fill::reference_of),
        top_left: record.top_left,
    };
    match build(
        effects.as_ref(),
        fill,
        ranges,
        (knockout, interior),
        colors,
        &pattern,
    ) {
        Ok(None) => StyleBody::Absent,
        Ok(Some(carried)) => StyleBody::Carried(carried),
        Err(reason) => StyleBody::Declined { reason },
    }
}

// -------------------------------------------------------------- encoding --

/// Why this style cannot be written as Photoshop layer effects at all, from
/// the STYLE alone — the settings Photoshop's own dialog has no room for.
/// `None` means [`encode`] succeeds.
pub(crate) fn bake_reason(style: &LayerStyle) -> Option<String> {
    for effect in style.enabled_effects() {
        match effect {
            Effect::Stroke(stroke) if stroke.size < 1.0 => return Some(STROKE_MINIMUM.to_string()),
            Effect::Satin(satin) if !(1.0..=250.0).contains(&satin.distance) => {
                return Some(SATIN_RANGE.to_string())
            }
            _ => {}
        }
    }
    None
}

/// Why an ENABLED mask cannot travel beside a preserved style: a PSD mask
/// stores neither density nor feather, and dropping them silently would
/// change the picture. `Some` puts the whole style in the bake, where the
/// mask is applied to the pixels instead.
pub(crate) fn mask_bake_reason(masked: bool, density: f32, feather: f32) -> Option<String> {
    (masked && (density != 1.0 || feather != 0.0)).then(|| MASK.to_string())
}

/// One additional layer block: its key and body.
pub(crate) type Block = ([u8; 4], Vec<u8>);

/// The effects block(s) a record writes for this style (module doc): the
/// `lfx2` an unstacked style always wrote (`lfxs` on a group record,
/// `section`), or a stacked style's `lmfx` alone, Photoshop's form — or the
/// [`bake_reason`] that refuses it. `light` resolves every
/// `use_global_light` angle; `colors` is how a colour crosses (`FxColours`).
pub(crate) fn effect_blocks(
    style: &LayerStyle,
    light: GlobalLight,
    colors: FxColours<'_>,
    section: bool,
) -> Result<Vec<Block>, String> {
    let items = effect_items(style, Some(light), colors)?;
    let key = if is_stacked(style) {
        *b"lmfx"
    } else if section {
        *b"lfxs"
    } else {
        *b"lfx2"
    };
    Ok(vec![(key, framed_body(items))])
}

/// The items [`effect_blocks`] frames — an unstacked style's `lfx2` items or
/// a stacked style's `lmfx` ones — or the [`bake_reason`] that refuses the
/// style. The `.asl` writer (`asl_write`) nests the same items as a preset's
/// `Lefx`. `light` is `fx::write`'s: `Some` resolves every
/// `use_global_light` angle, `None` keeps each effect's own.
pub(crate) fn effect_items(
    style: &LayerStyle,
    light: Option<GlobalLight>,
    colors: FxColours<'_>,
) -> Result<Vec<(Vec<u8>, Item)>, String> {
    if let Some(reason) = bake_reason(style) {
        return Err(reason);
    }
    if is_stacked(style) {
        return Ok(stacked_items(style, light, colors));
    }
    let mut items = header_items();
    for effect in &style.effects {
        items.push(fx::write(effect, light, colors));
    }
    Ok(items)
}

/// The `Scl ` and `masterFXSwitch` items both bodies open with.
fn header_items() -> Vec<(Vec<u8>, Item)> {
    vec![
        (b"Scl ".to_vec(), Item::Unit(*b"#Prc", 100.0)),
        (b"masterFXSwitch".to_vec(), Item::Bool(true)),
    ]
}

/// A stacked style's `lmfx` items (module doc): the kinds in Photoshop's key
/// order, each repeating kind a list of its instances top first.
fn stacked_items(
    style: &LayerStyle,
    light: Option<GlobalLight>,
    colors: FxColours<'_>,
) -> Vec<(Vec<u8>, Item)> {
    let mut items = header_items();
    for kind in LMFX_ORDER {
        let held: Vec<&Effect> = style.effects.iter().filter(|e| e.kind() == kind).collect();
        if held.is_empty() {
            continue;
        }
        match multi_key(kind) {
            Some(key) => {
                let list = held.iter().map(|e| fx::write(e, light, colors).1).collect();
                items.push((key.as_bytes().to_vec(), Item::List(listed(list))));
            }
            None => items.extend(held.iter().map(|e| fx::write(e, light, colors))),
        }
    }
    let enabled = style.enabled_effects().count();
    items.push((
        b"numModifyingFX".to_vec(),
        Item::Long(i32::try_from(enabled).unwrap_or(i32::MAX)),
    ));
    items
}

/// `u32 0` then the version-16 `null` descriptor of `items`, padded even.
fn framed_body(items: Vec<(Vec<u8>, Item)>) -> Vec<u8> {
    let mut out = 0u32.to_be_bytes().to_vec();
    out.extend_from_slice(&psd_descriptor::write(&Descriptor {
        class: b"null".to_vec(),
        items,
    }));
    // An additional layer block must have an even length; the reader ignores
    // the descriptor's tail.
    if !out.len().is_multiple_of(2) {
        out.push(0);
    }
    out
}

/// The bytes [`effect_blocks`] writes for this style, framing included (12
/// per block); 0 when it refuses.
pub(crate) fn effect_block_bytes(
    style: &LayerStyle,
    light: GlobalLight,
    colors: FxColours<'_>,
) -> u64 {
    // `lfxs` and `lfx2` frame the same body, so the count holds for both.
    effect_blocks(style, light, colors, false).map_or(0, |blocks| {
        blocks.iter().map(|(_, body)| 12 + body.len() as u64).sum()
    })
}

/// The record's 40-byte blending ranges: each range's two ramps in its
/// channel's slot, and the unrestricted default everywhere else.
pub(crate) fn encode_ranges(blend_if: &BlendIf) -> [u8; 40] {
    let mut out = [0u8; 40];
    out.copy_from_slice(&encode_ranges_for(blend_if, false));
    out
}

/// The blending ranges of an RGB (`cmyk` false: [`encode_ranges`]'s 40
/// bytes) or a CMYK file (its five pairs, also 40 bytes — take1's length,
/// module doc).
pub(crate) fn encode_ranges_for(blend_if: &BlendIf, cmyk: bool) -> Vec<u8> {
    let mut out = vec![0u8; 40];
    for slot in out.chunks_exact_mut(4) {
        slot.copy_from_slice(&FULL_RAMP);
    }
    for range in blend_if.ranges() {
        let index = channel_slots(cmyk)
            .iter()
            .position(|(channel, _)| *channel == range.channel)
            .unwrap_or(0);
        out[index * 8..index * 8 + 4].copy_from_slice(&range.this_layer);
        out[index * 8 + 4..index * 8 + 8].copy_from_slice(&range.underlying);
    }
    out
}

/// The `iOpa` body for this fill opacity; `None` at the 100 % default, which
/// Photoshop writes no block for.
pub(crate) fn encode_fill(fill_opacity: f32) -> Option<[u8; 4]> {
    let byte = crate::opacity_byte::code(fill_opacity);
    (byte != 255).then_some([byte, 0, 0, 0])
}

/// The blend modes whose PIXEL records always carry `iOpa`, even at 255:
/// Photoshop reads a Color Burn, Color Dodge or Difference record WITHOUT one
/// as a pre-Fill "legacy" layer — its opacity reopens as Fill and its
/// semi-transparent pixels fade toward the mode's neutral (#578 take1: op60
/// written 153 / 255 read back 255 / 153; the same modes' Fill tiles, whose
/// records carried `iOpa`, read back as written).
pub(crate) const LEGACY_FILL_MODES: [BlendMode; 3] = [
    BlendMode::ColorBurn,
    BlendMode::ColorDodge,
    BlendMode::Difference,
];

/// The `iOpa` body a PIXEL record in `mode` writes for this fill opacity:
/// [`encode_fill`]'s, and `[255, 0, 0, 0]` at 100 % too for a mode in
/// [`LEGACY_FILL_MODES`]. A 255 body reopens as no style ([`decode_fill`]).
pub(crate) fn encode_fill_for(mode: BlendMode, fill_opacity: f32) -> Option<[u8; 4]> {
    let byte = crate::opacity_byte::code(fill_opacity);
    (byte != 255 || LEGACY_FILL_MODES.contains(&mode)).then_some([byte, 0, 0, 0])
}

/// The `infx` and `knko` blocks this style needs, in the order Photoshop
/// writes them. Empty at the two defaults, because Photoshop writes no block
/// for either one — which is what keeps a key-free document's bytes exactly
/// what the previous writer produced.
pub(crate) fn knockout_blocks(style: &LayerStyle) -> Vec<([u8; 4], Vec<u8>)> {
    let mut out = Vec::new();
    if style.blend_interior_as_group {
        out.push((*b"infx", vec![1, 0, 0, 0]));
    }
    let code = match style.knockout {
        Knockout::None => 0u8,
        Knockout::Shallow => 1,
        Knockout::Deep => 2,
    };
    if code != 0 {
        out.push((*b"knko", vec![code, 0, 0, 0]));
    }
    out
}

/// What the import and export reports CALL these two keys, worded once:
/// `knockout (shallow)` / `knockout (deep)`, `Blend Interior Effects as
/// Group`, or the two joined by ` and `. `None` at both defaults.
pub(crate) fn knockout_clause(knockout: Knockout, interior: bool) -> Option<String> {
    let mut parts = Vec::new();
    match knockout {
        Knockout::None => {}
        Knockout::Shallow => parts.push("knockout (shallow)".to_string()),
        Knockout::Deep => parts.push("knockout (deep)".to_string()),
    }
    if interior {
        parts.push("Blend Interior Effects as Group".to_string());
    }
    (!parts.is_empty()).then(|| parts.join(" and "))
}

/// The Photoshop records those two keys are written as, worded once:
/// `knko record`, `infx record` or `knko and infx records`.
pub(crate) fn knockout_keys(knockout: Knockout, interior: bool) -> String {
    match (knockout != Knockout::None, interior) {
        (true, true) => "knko and infx records".to_string(),
        (true, false) => "knko record".to_string(),
        _ => "infx record".to_string(),
    }
}

/// The name the import report gives a carried Blend If channel.
pub(crate) fn channel_name(channel: BlendIfChannel) -> &'static str {
    CHANNEL_SLOTS
        .iter()
        .chain(CMYK_CHANNEL_SLOTS.iter())
        .find(|(candidate, _)| *candidate == channel)
        .map_or("gray", |(_, name)| *name)
}
