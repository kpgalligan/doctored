//! The ONE codec for Photoshop's binary- and descriptor-body adjustment
//! records: a pixel-less `nvrt`/`thrs`/`post`/`levl`/`curv`/`blnc`/`hue2`/
//! `phfl`/`expA`/`vibA`/`blwh`/`mixr`/`brit`/`selc` record decodes to a
//! Doctored adjustment meta (`adjust.rs`'s byte-form schema), and a Doctored
//! `invert`/`threshold`/`posterize`/`levels`/`curves`/`color_balance`/
//! `hue_saturation`/`photo_filter`/`exposure`/`vibrance`/`black_and_white`/
//! `channel_mixer`/`bcs`/`selective_color` layer encodes back to that record.
//! Nothing else in the crate parses or writes one of these bodies —
//! `psd_adjustment_mixr` is this module's private half for `mixr` and
//! `psd_adjustment_extra` for `brit` (with its `CgEd` companion block,
//! `Encoded::companion`) and `selc`, both reached only from here, and
//! `adjustment_preset_file` (Photoshop's `.acv`/`.alv`/`.ahu`/`.eap`/`.blw`/
//! `.cha` preset files) is the one other caller of `decode`, handing it the
//! file's body as a record body — and nothing else inserts the typed
//! `evaluation` key:
//! `typed_description` is the only adapter from the byte-form meta to the
//! 16-bit document's form. `vibA` and `blwh` are descriptor bodies (`u32
//! version = 16` then a descriptor structure); `psd_descriptor` is their
//! framing and this module owns every class and key. `expA` is a BINARY body
//! per Adobe's table — the phase plan's "descriptor" label for it was an
//! error.
//!
//! Layouts follow Adobe's "Additional Layer Information" table
//! (https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/), big-endian,
//! `i16` signed. Import accepts a body whose framed length is at least the
//! listed size and ignores any tail; a shorter body is declined. Export writes
//! exactly the listed size — except `curv`, whose natural length
//! `7 + Σ(2 + 4·count)` is always odd, so it appends one zero pad byte (the
//! importer refuses odd block lengths) and `decode` ignores it as a tail;
//! `encode` pads any other odd body the same way.
//!
//! | key | body | mapping (import ←; export →) |
//! |---|---|---|
//! | nvrt | none | `invert` |
//! | thrs | u16 level, 2 pad (import also accepts len 2) | `level = L/255` ← ; `L = round(level*255).clamp(1,255)` → |
//! | post | u16 levels, 2 pad | `levels = N`; declined when N > 64 |
//! | blnc | 3×3 i16 shadows/midtones/highlights × (cyan_red, magenta_green, yellow_blue), u8 preserve_luminosity, 1 pad (20) | `v/100`, flag≠0 ← ; `round(v*100)`, the bool → |
//! | levl | u16 version=2; 29 × (u16 in_floor, in_ceil, out_floor, out_ceil, gamma×100) (292); measured against Photoshop 2025 (`tests/fixtures/psd_photoshop/adjustments_channels/`): record 0 is the composite and records 1, 2, 3 red, green, blue, and every body is 632 bytes — the 292 plus a 340-byte tail (`Lvls`, u16 version 3, u16 total record count 62, 33 more identity records, 2 pad) that import ignores; Photoshop applies a per-channel record BEFORE the composite one (out = composite(channel(v))), which is `adjust_levels`' order | records 0..=3 are each checked in order ("input ceiling does not exceed its floor", "gamma is outside 0.10…9.99"); record 0 → the master `black=floor/255, white=ceil/255, gamma=g/100`, plus `output_black=out_floor/255, output_white=out_ceil/255` when not (0, 255); records 1, 2, 3 → `r`, `g`, `b` objects of the same keys when not `[0,255,0,255,100]`; records 4..=28 and the `Lvls` tail are ignored ← ; record 0 from the master and records 1..=3 from `r`/`g`/`b` (identity when absent), each floor=`round(black*255).min(253)`, ceil=`round(white*255).clamp(floor+1,255)`, g=`round(gamma*100).clamp(10,999)`, output=`round(o*255).clamp(0,255)`; records 4..=28 identity → |
//! | curv | u8 is_map, u16 version (1\|4), u32 channel bitmask; per set bit ascending: u16 count, count × (u16 OUTPUT, u16 INPUT); Doctored's encoder then writes ONE zero pad byte. Measured against Photoshop 2025 (`adjustments_channels/`): a point curve is `is_map` 0, version 1, and its bitmask holds ONLY the channels given (composite + red is 3, no identity green/blue), followed DIRECTLY (no pad byte; zero padding only at the body's end) by a `Crv ` tail at version 4 naming the same curves by channel index; a `Mpng` (mapped) curve is `is_map` 1, version 1, bitmask 1 and 256 map bytes, then a `Crv ` tail at version 3; Photoshop applies a per-channel curve BEFORE the composite one | bit0→`rgb`, 1→`r`, 2→`g`, 3→`b`, points `[in,out]`; `is_map` 1 reads 256 bytes per set bit, ascending, into `rgb_map`/`r_map`/`g_map`/`b_map` (every set bit kept, identity or not); `is_map` ∉ {0,1} is declined ("map flag is not 0 or 1"); the `Crv ` tail is ignored ← ; point form: is_map 0, version 1, bit 0 always present (identity when `rgb` absent); map form (any `*_map` key, `curve_map_body`): is_map 1, version 1, bits `1` plus each present `r_map`/`g_map`/`b_map`, 256 bytes per set bit (identity when `rgb_map` absent), then the tail as Photoshop writes it — `Crv `, u16 3, u32 set-bit count, per set bit u16 bit index + the same 256 bytes — and the generic pad (532 bytes composite-only, byte-identical to Photoshop's) → |
//! | hue2 | u16 version=2, u8 colorize, 1 pad, 3×i16 colorization (hue,sat,light), 3×i16 master, 6 × (4×i16 range a,b,c,d; 3×i16 hue,sat,light) (100) | master `hue=h, saturation=s/100, lightness=l/100`; `colorize=flag≠0`, `colorize_hue=h mod 360`; band i: `inner=arc(b,c)/2`, `center=(b+inner) mod 360`, `falloff=(arc(a,b)+arc(c,d))/2` ← ; a=c0−inner−falloff, b=c0−inner, c=c0+inner, d=c0+inner+falloff, each `round() mod 360` → |
//! | phfl | u16 version; v2: u16 colour_space + 4×u16 components; v3: 3×u32 XYZ; then u32 density, u8 preserve_luminosity, 3 pad (v2 20, v3 22) | v2 space 0: `color="#rrggbb"` from `round(c/257)`; v2 space 7 (Lab, the space Photoshop 2025 actually writes): components `[L, a, b]` as **i16** hundredths → `lab::lab_to_srgb8` (declines if the built-in sRGB profile has no model) → `to_document`; both then `density=d/100`, flag≠0 ← ; v2, space 0, `c*257`, `round(density*100).clamp(1,100)`, the bool → |
//! | expA | u16 version=1, f32 exposure, f32 offset, f32 gamma, 2 pad (16; import accepts 14) | `exposure=e, offset=o, gamma=g` PLUS `evaluation="document_v2"`, so the layer evaluates through the document profile at both depths ← ; version 1, the three as f32 (defaults 0, 0, 1), `evaluation` ignored → |
//! | vibA | u32 version=16, descriptor class `null`: `vibrance` long, `Strt` long (58) | `vibrance=v/100`, `saturation=Strt/100`; a missing key omits the param (schema default) ← ; `round(v*100)`, `round(s*100)` → |
//! | mixr | u16 version=1, u16 monochrome, 4 × (5 × i16 percent: R, G, B, K, constant) (44); measured against Photoshop-written files, see `psd_adjustment_mixr` | off: records 0–2 → `red`/`green`/`blue` `{r, g, b, constant}` = `v/100`; on: record 0 → `gray`; declined for version ≠ 1, a CMYK body (nonzero K source or record 3) or a read field outside −200…200 ← ; version 1, the visible rows as `round(v*100)`, K and every unwritten record 0; a hidden row that differs from its default is an export loss (`Encoded::unwritten`) → |
//! | brit | 8 bytes, with the values in the record's `CgEd` descriptor; layout and mapping in `psd_adjustment_extra` | `bcs` with `saturation` 0; a `bcs` layer with saturation has no record and is baked by the export |
//! | selc | Adobe's 84-byte selective-colour table; layout and mapping in `psd_adjustment_extra` | `selective_color` |
//! | blwh | u32 version=16, descriptor class `null`: `Rd  ` `Yllw` `Grn ` `Cyn ` `Bl  ` `Mgnt` long; `useTint` bool; `tintColor` Objc class `RGBC` {`Rd  `, `Grn `, `Bl  ` doub 0…255}; `bwPresetKind` long; `blackAndWhitePresetFileName` TEXT (294) | weights `/100` as `reds`…`magentas`, `tint=useTint`, `tint_color` from the rounded RGBC doubles; a `tintColor` that is not an `RGBC` object of three finite doubles is declined; preset kind and file name ignored ← ; `round(w*100)`, the bool, RGBC doubles from `tint_color`, preset kind 1, name "" → |
//!
//! A CMYK document (#547 phase 3 and the styles batch) reads and writes twelve
//! kinds through [`decode_cmyk`] / [`encode_cmyk`], its description the
//! `cmyk_adjust` schema with the typed `evaluation` key (`cmyk_adjust_layer`):
//!
//! | key | CMYK body | mapping (import ←; export →) |
//! |---|---|---|
//! | levl | the RGB layout; records 0–4 = composite, C, M, Y, K in the stored `L = 1 − ink` domain | record 0 → the top-level stage, records 1–4 → `c`/`m`/`y`/`k` when not the identity, each through `levels_stage` ← ; `levels_record` per stage, records 5–28 identity → (`psd_adjustment_cmyk`) |
//! | curv | the RGB point layout; bits 0–4 = composite, C, M, Y, K, points in `L` | each point reflected `(255 − in, 255 − out)` onto ink as `cmyk`/`c`/`m`/`y`/`k`, a two-point identity omitted; a mapped curve declines ← ; the reflection back, the composite bit always → (`psd_adjustment_cmyk`) |
//! | hue2 | the RGB body | the RGB arm both ways |
//! | nvrt, thrs, post, blnc, selc | the RGB bodies | the RGB arms both ways: `invert`, `threshold` `level`, `posterize` `levels`, `color_balance`, `selective_color` (the CMYK schema is the RGB one) |
//! | brit + CgEd | the RGB pair | `bcs` without `saturation` (a plate has none) ← ; the RGB pair → |
//! | phfl | the RGB layout | `photo_filter` `ink`: space 2 CMYK exactly, space 0/7 through `to_ink` ← ; version 2 space 2 → (`psd_adjustment_cmyk`) |
//! | mixr | the 44-byte body | records 0–3 the Cyan…Black outputs `{c, m, y, k, constant}`, Monochrome's Gray in record 0 (`psd_adjustment_mixr`) |
//! | grdm | Adobe's gradient settings | `gradient_map` stops `{position, ink, opacity}`, reverse, dither (`psd_adjustment_grdm`) |
//!
//! Every other kind (Exposure, Vibrance, Black & White, which a CMYK
//! document refuses) declines on a CMYK document with [`CMYK_KIND`], and a decoded
//! body that `cmyk_adjust` refuses declines as out of range.
//!
//! `to_document` is the sRGB→DOCUMENT conversion `psd_import::parse` builds
//! from the file's own profile (`None` for an sRGB file and for a profile this
//! crate cannot model). Only the `phfl` Lab arm reads it, for the same reason
//! `style_composite` converts an authored style colour: the decoded colour is
//! authored sRGB and the layer will be evaluated in the document's space.
//!
//! A present descriptor key of the wrong type is declined with the key named;
//! any other descriptor decline reason comes from `psd_descriptor::read`.
//! After the per-kind checks, every `Carried` meta has passed
//! `Adjustment::from_meta`; a body inside its layout whose values Doctored's
//! schema rejects is declined with one fixed reason.
//!
//! `psd_adjustment.rs` is the ONE codec for Photoshop's binary- and descriptor-body adjustment
//! records (`nvrt`/`thrs`/`post`/`levl`/`curv`/`blnc`/`hue2`/`phfl`/`expA`/`vibA`/`blwh`/`mixr`;
//! `KINDS` adds `brit` and `selc` to those twelve, fourteen entries, and `psd_adjustment_mixr.rs` is its private half for the 44-byte
//! Channel Mixer body, whose layout —
//! records 0–2 Red/Green/Blue, Monochrome's Gray in record 0 — was measured against
//! Photoshop-written files in `tests/fixtures/psd_photoshop/channel_mixer/`; a CMYK body or a field
//! outside −200…200 declines, and a hidden row not at its default is an export loss through
//! `Encoded::unwritten`): a pixel-less record of one of those keys decodes to a byte-form
//! `adjust.rs` meta and imports as a carried adjustment layer on both depths
//! (`GroupRecords::carried`, `typed_description` adds the typed `evaluation` key; an `expA` import
//! authors `document_v2` so it evaluates identically at both depths), a malformed or unsupported
//! body is declined and omitted with its reason, and the twelve matching Doctored ops encode back
//! to those records on export (any odd body is padded to an even length inside the codec).

use serde_json::{json, Map, Value};

use crate::adjust::Adjustment;
use crate::adjust_curves::{parse_map, MAP_KEYS};
use crate::authored_color::AuthoredColors;
use crate::lab::lab_to_srgb8;
use crate::psd_adjustment_cmyk;
use crate::psd_adjustment_extra;
use crate::psd_adjustment_grdm;
use crate::psd_adjustment_mixr;
use crate::psd_descriptor::{self, Descriptor, Item};
use crate::style_names::{color_hex, parse_color};

/// The fourteen kinds: file key, report name, Doctored op.
const KINDS: [([u8; 4], &str, &str); 14] = [
    (*b"nvrt", "Invert", "invert"),
    (*b"thrs", "Threshold", "threshold"),
    (*b"post", "Posterize", "posterize"),
    (*b"levl", "Levels", "levels"),
    (*b"curv", "Curves", "curves"),
    (*b"blnc", "Color Balance", "color_balance"),
    (*b"hue2", "Hue/Saturation", "hue_saturation"),
    (*b"phfl", "Photo Filter", "photo_filter"),
    (*b"expA", "Exposure", "exposure"),
    (*b"vibA", "Vibrance", "vibrance"),
    (*b"blwh", "Black & White", "black_and_white"),
    (*b"mixr", "Channel Mixer", "channel_mixer"),
    (*b"brit", "Brightness/Contrast", "bcs"),
    (*b"selc", "Selective Color", "selective_color"),
];

const HUE_WHEEL: [&str; 6] = ["reds", "yellows", "greens", "cyans", "blues", "magentas"];
const TONES: [&str; 3] = ["shadows", "midtones", "highlights"];
const TONE_KEYS: [&str; 3] = ["cyan_red", "magenta_green", "yellow_blue"];
const CURVE_CHANNELS: [&str; 4] = ["rgb", "r", "g", "b"];
const BW_KEYS: [&[u8; 4]; 6] = [b"Rd  ", b"Yllw", b"Grn ", b"Cyn ", b"Bl  ", b"Mgnt"];
const BW_DEFAULTS: [f64; 6] = [0.4, 0.6, 0.4, 0.6, 0.2, 0.8];
const RGB_KEYS: [&[u8; 4]; 3] = [b"Rd  ", b"Grn ", b"Bl  "];
const TINT: &str = "tint colour is not an RGB descriptor";
const SHORT: &str = "body is shorter than its layout";
const OUT_OF_RANGE: &str = "parameters fall outside Doctored's ranges";

/// A decoded record: the file key, its report name and the byte-form meta
/// (`{"type":"adjust","op":..,"params":{..}}`), which parses.
pub(crate) struct Carried {
    pub key: [u8; 4],
    pub kind: &'static str,
    pub meta: String,
}

impl Carried {
    /// The Doctored op the meta carries.
    pub fn op(&self) -> &'static str {
        if self.key == GRADIENT_MAP.0 {
            return GRADIENT_MAP.2;
        }
        KINDS
            .iter()
            .find(|(key, _, _)| *key == self.key)
            .map_or("", |(_, _, op)| op)
    }
}

pub(crate) enum AdjustmentBody {
    /// The key is not one of the fourteen.
    Absent,
    Carried(Carried),
    /// One of the fourteen whose body cannot be carried; `reason` reads after
    /// "whose" in the import report.
    Declined {
        key: [u8; 4],
        kind: &'static str,
        reason: String,
    },
}

/// One encoded record: the file key, its report name and an even-length body.
/// `unwritten` is the clause for layer state the record cannot hold — today
/// only a Channel Mixer's hidden row (`psd_adjustment_mixr::hidden_row_loss`)
/// — which the export preflight reports as a loss.
pub(crate) struct Encoded {
    pub key: [u8; 4],
    pub kind: &'static str,
    pub body: Vec<u8>,
    pub unwritten: Option<&'static str>,
    /// A second block the record needs directly after this one — today only
    /// Brightness/Contrast's `CgEd` descriptor (`psd_adjustment_extra`).
    pub companion: Option<([u8; 4], Vec<u8>)>,
}

/// The meta with the typed root key the 16-bit document requires.
pub(crate) fn typed_description(meta: &str) -> String {
    let mut value: Value = serde_json::from_str(meta).unwrap_or(Value::Null);
    if let Some(root) = value.as_object_mut() {
        root.insert("evaluation".into(), Value::String("precision_v1".into()));
    }
    value.to_string()
}

/// The `op` of an adjustment meta, or `None` when it is not one.
pub(crate) fn op(meta: &str) -> Option<String> {
    let value: Value = serde_json::from_str(meta).ok()?;
    let root = value.as_object()?;
    if root.get("type")?.as_str()? != "adjust" {
        return None;
    }
    Some(root.get("op")?.as_str()?.to_string())
}

// -------------------------------------------------------------- decoding --

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let out = self.bytes.get(self.at..self.at.checked_add(count)?)?;
        self.at += count;
        Some(out)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.take(2)?.try_into().ok()?))
    }
    fn i16(&mut self) -> Option<i16> {
        Some(self.u16()? as i16)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }
}

enum Outcome {
    Params(Map<String, Value>),
    Declined(String),
}

fn declined(reason: &str) -> Option<Outcome> {
    Some(Outcome::Declined(reason.to_string()))
}

fn params(pairs: Vec<(&str, Value)>) -> Option<Outcome> {
    Some(Outcome::Params(
        pairs
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    ))
}

/// An identity `levl` record: `(in_floor, in_ceil, out_floor, out_ceil,
/// gamma×100)`.
pub(crate) const LEVELS_IDENTITY: [u16; 5] = [0, 255, 0, 255, 100];

/// One `levl` record as a stage's canonical keys: the input points and gamma
/// always, the output pair only when it is not the full range.
pub(crate) fn levels_stage(record: [u16; 5]) -> Vec<(&'static str, Value)> {
    let [floor, ceil, out_floor, out_ceil, gamma] = record;
    let mut pairs = vec![
        ("black", json!(f64::from(floor) / 255.0)),
        ("white", json!(f64::from(ceil) / 255.0)),
        ("gamma", json!(f64::from(gamma) / 100.0)),
    ];
    if (out_floor, out_ceil) != (0, 255) {
        pairs.push(("output_black", json!(f64::from(out_floor) / 255.0)));
        pairs.push(("output_white", json!(f64::from(out_ceil) / 255.0)));
    }
    pairs
}

/// `((y - x) mod 360 + 360) mod 360`: the ascending arc from x to y.
fn arc(x: i32, y: i32) -> i32 {
    (y - x).rem_euclid(360)
}

/// `key` as a long scaled by 1/100, when present.
fn hundredths_of(descriptor: &Descriptor, key: &[u8]) -> Result<Option<Value>, String> {
    Ok(descriptor
        .long(key)?
        .map(|value| json!(f64::from(value) / 100.0)))
}

/// The `vibA` descriptor's params; `Err` is the decline reason.
fn vibrance_params(body: &[u8]) -> Result<Vec<(&'static str, Value)>, String> {
    let descriptor = psd_descriptor::read(body)?;
    let mut out = Vec::new();
    if let Some(value) = hundredths_of(&descriptor, b"vibrance")? {
        out.push(("vibrance", value));
    }
    if let Some(value) = hundredths_of(&descriptor, b"Strt")? {
        out.push(("saturation", value));
    }
    Ok(out)
}

/// The `blwh` descriptor's params; `Err` is the decline reason.
fn black_and_white_params(body: &[u8]) -> Result<Vec<(&'static str, Value)>, String> {
    let descriptor = psd_descriptor::read(body)?;
    let mut out = Vec::new();
    for (key, name) in BW_KEYS.iter().zip(HUE_WHEEL) {
        if let Some(value) = hundredths_of(&descriptor, *key)? {
            out.push((name, value));
        }
    }
    if let Some(tint) = descriptor.bool(b"useTint")? {
        out.push(("tint", json!(tint)));
    }
    if let Some(color) = descriptor
        .object(b"tintColor")
        .map_err(|_| TINT.to_string())?
    {
        if color.class != b"RGBC" {
            return Err(TINT.to_string());
        }
        let mut rgb = [0u8; 3];
        for (slot, key) in rgb.iter_mut().zip(RGB_KEYS) {
            let value = color
                .double(key)
                .ok()
                .flatten()
                .filter(|value| value.is_finite())
                .ok_or_else(|| TINT.to_string())?;
            *slot = value.round().clamp(0.0, 255.0) as u8;
        }
        out.push(("tint_color", json!(color_hex(rgb))));
    }
    Ok(out)
}

/// The body of one of the fourteen keys as byte-form params; `None` is a body
/// shorter than its layout.
fn decode_params(
    key: [u8; 4],
    body: &[u8],
    to_document: Option<&AuthoredColors>,
) -> Option<Outcome> {
    let mut r = Reader { bytes: body, at: 0 };
    match &key {
        b"nvrt" => params(vec![]),
        b"thrs" => {
            let level = r.u16()?;
            params(vec![("level", json!(f64::from(level) / 255.0))])
        }
        b"post" => {
            let levels = r.u16()?;
            r.take(2)?;
            if levels > 64 {
                return declined("level count exceeds 64");
            }
            params(vec![("levels", json!(levels))])
        }
        b"blnc" => {
            let mut tones = Vec::with_capacity(3);
            for tone in TONES {
                let mut object = Map::new();
                for component in TONE_KEYS {
                    object.insert(component.into(), json!(f64::from(r.i16()?) / 100.0));
                }
                tones.push((tone, Value::Object(object)));
            }
            let preserve = r.u8()? != 0;
            r.take(1)?;
            tones.push(("preserve_luminosity", json!(preserve)));
            params(tones)
        }
        b"levl" => {
            let version = r.u16()?;
            let mut records = [[0u16; 5]; 29];
            for record in &mut records {
                for field in record.iter_mut() {
                    *field = r.u16()?;
                }
            }
            if version != 2 {
                return declined("version is not 2");
            }
            for [floor, ceil, _, _, gamma] in &records[..=3] {
                if ceil <= floor {
                    return declined("input ceiling does not exceed its floor");
                }
                if !(10..=999).contains(gamma) {
                    return declined("gamma is outside 0.10…9.99");
                }
            }
            let mut pairs = levels_stage(records[0]);
            for (name, record) in ["r", "g", "b"].into_iter().zip(&records[1..=3]) {
                if *record != LEVELS_IDENTITY {
                    let object: Map<String, Value> = levels_stage(*record)
                        .into_iter()
                        .map(|(key, value)| (key.to_string(), value))
                        .collect();
                    pairs.push((name, Value::Object(object)));
                }
            }
            params(pairs)
        }
        b"curv" => {
            let is_map = r.u8()?;
            let version = r.u16()?;
            let bits = r.u32()?;
            if is_map > 1 {
                return declined("map flag is not 0 or 1");
            }
            if !matches!(version, 1 | 4) {
                return declined("version is not 1 or 4");
            }
            if bits & !0xF != 0 {
                return declined("channel bits beyond blue are set");
            }
            let mut channels = Vec::new();
            if is_map == 1 {
                for (bit, name) in MAP_KEYS.iter().enumerate() {
                    if bits & (1 << bit) != 0 {
                        let map = r.take(256)?.iter().map(|&v| json!(v)).collect();
                        channels.push((*name, Value::Array(map)));
                    }
                }
                return params(channels);
            }
            for (bit, name) in CURVE_CHANNELS.iter().enumerate() {
                if bits & (1 << bit) == 0 {
                    continue;
                }
                let count = r.u16()?;
                if !(2..=16).contains(&count) {
                    return declined("point count is outside 2…16");
                }
                let mut points = Vec::with_capacity(usize::from(count));
                for _ in 0..count {
                    let output = r.u16()?;
                    let input = r.u16()?;
                    if input > 255 || output > 255 {
                        return declined("point value exceeds 255");
                    }
                    points.push(json!([input, output]));
                }
                channels.push((*name, Value::Array(points)));
            }
            params(channels)
        }
        b"hue2" => {
            let version = r.u16()?;
            let colorize = r.u8()? != 0;
            r.take(1)?;
            let mut triples = [[0i16; 3]; 2];
            for triple in &mut triples {
                for value in triple.iter_mut() {
                    *value = r.i16()?;
                }
            }
            let mut ranges = [[0i16; 7]; 6];
            for range in &mut ranges {
                for value in range.iter_mut() {
                    *value = r.i16()?;
                }
            }
            if version != 2 {
                return declined("version is not 2");
            }
            let [tint, master] = triples;
            let mut bands = Map::new();
            for (name, range) in HUE_WHEEL.iter().zip(ranges) {
                let [a, b, c, d, hue, sat, light] = range.map(i32::from);
                let inner = f64::from(arc(b, c)) / 2.0;
                let center = (f64::from(b) + inner).rem_euclid(360.0);
                let falloff = f64::from(arc(a, b) + arc(c, d)) / 2.0;
                bands.insert(
                    (*name).into(),
                    json!({
                        "hue": f64::from(hue),
                        "saturation": f64::from(sat) / 100.0,
                        "lightness": f64::from(light) / 100.0,
                        "center": center,
                        "inner": inner,
                        "falloff": falloff,
                    }),
                );
            }
            params(vec![
                ("hue", json!(f64::from(master[0]))),
                ("saturation", json!(f64::from(master[1]) / 100.0)),
                ("lightness", json!(f64::from(master[2]) / 100.0)),
                ("bands", Value::Object(bands)),
                ("colorize", json!(colorize)),
                ("colorize_hue", json!(f64::from(tint[0].rem_euclid(360)))),
                ("colorize_saturation", json!(f64::from(tint[1]) / 100.0)),
                ("colorize_lightness", json!(f64::from(tint[2]) / 100.0)),
            ])
        }
        b"phfl" => {
            let version = r.u16()?;
            match version {
                3 => return declined("XYZ colour form is not mapped"),
                2 => {}
                _ => return declined("version is not 2 or 3"),
            }
            let space = r.u16()?;
            let mut components = [0u16; 4];
            for component in &mut components {
                *component = r.u16()?;
            }
            let density = r.u32()?;
            let preserve = r.u8()? != 0;
            r.take(3)?;
            let color: [u8; 3] = match space {
                0 => std::array::from_fn(|i| {
                    (f64::from(components[i]) / 257.0).round().clamp(0.0, 255.0) as u8
                }),
                // Photoshop 2025 writes every photo filter — RGB picker, Lab
                // picker, preset and default alike — as space 7, whose four
                // components are the PSD colour structure's Lab: SIGNED 16-bit
                // hundredths, so a and b carry their two's-complement negative
                // half. The colour is authored sRGB once it is out of Lab, and
                // converts into the document's space exactly as a style colour
                // does.
                7 => {
                    let lab = std::array::from_fn(|i| f32::from(components[i] as i16) * 0.01);
                    let Some(rgb) = lab_to_srgb8(lab) else {
                        return declined("the built-in sRGB profile has no model");
                    };
                    to_document.map_or(rgb, |transform| transform.color(rgb))
                }
                _ => return declined("colour space is not RGB or Lab"),
            };
            params(vec![
                ("color", json!(color_hex(color))),
                ("density", json!(f64::from(density) / 100.0)),
                ("preserve_luminosity", json!(preserve)),
            ])
        }
        b"expA" => {
            let version = r.u16()?;
            let exposure = r.f32()?;
            let offset = r.f32()?;
            let gamma = r.f32()?;
            if version != 1 {
                return declined("version is not 1");
            }
            params(vec![
                ("exposure", json!(f64::from(exposure))),
                ("offset", json!(f64::from(offset))),
                ("gamma", json!(f64::from(gamma))),
                ("evaluation", json!("document_v2")),
            ])
        }
        b"vibA" => match vibrance_params(body) {
            Ok(pairs) => params(pairs),
            Err(reason) => declined(&reason),
        },
        b"blwh" => match black_and_white_params(body) {
            Ok(pairs) => params(pairs),
            Err(reason) => declined(&reason),
        },
        b"mixr" => match psd_adjustment_mixr::decode(body)? {
            Ok(params) => Some(Outcome::Params(params)),
            Err(reason) => declined(&reason),
        },
        // With no `CgEd` beside it, a `brit` body is the legacy form.
        b"brit" => brightness_outcome(body, None),
        b"selc" => match psd_adjustment_extra::decode_selective(body)? {
            Ok(params) => Some(Outcome::Params(params)),
            Err(reason) => declined(&reason),
        },
        _ => None,
    }
}

fn brightness_outcome(brit: &[u8], cged: Option<&[u8]>) -> Option<Outcome> {
    match psd_adjustment_extra::decode_brightness(brit, cged)? {
        Ok(params) => Some(Outcome::Params(params)),
        Err(reason) => declined(&reason),
    }
}

/// One of the fourteen keys with its decode outcome, as the record walk
/// carries it: a meta that parses, or the reason it does not.
fn carried(key: [u8; 4], kind: &'static str, op: &str, outcome: Option<Outcome>) -> AdjustmentBody {
    let reason = match outcome {
        None => SHORT.to_string(),
        Some(Outcome::Declined(reason)) => reason,
        Some(Outcome::Params(params)) => {
            let meta = json!({"type": "adjust", "op": op, "params": params}).to_string();
            if Adjustment::from_meta(&meta).is_some() {
                return AdjustmentBody::Carried(Carried { key, kind, meta });
            }
            OUT_OF_RANGE.to_string()
        }
    };
    AdjustmentBody::Declined { key, kind, reason }
}

/// `Absent` ONLY for a key outside the fourteen; otherwise `Carried` with a
/// meta that parses, or `Declined` with the reason. `to_document` is the
/// module doc's sRGB→document conversion. Never panics.
pub(crate) fn decode(
    key: [u8; 4],
    body: &[u8],
    to_document: Option<&AuthoredColors>,
) -> AdjustmentBody {
    let Some((_, kind, op)) = KINDS.iter().find(|(candidate, _, _)| *candidate == key) else {
        return AdjustmentBody::Absent;
    };
    carried(key, kind, op, decode_params(key, body, to_document))
}

/// A `brit` record with the `CgEd` body of the SAME record, when it has one:
/// Photoshop keeps the values there and leaves `brit` zeroed
/// (`psd_adjustment_extra`). The record walk is the only caller; a `brit`
/// body through [`decode`] is read as the legacy form.
pub(crate) fn decode_brightness(brit: &[u8], cged: Option<&[u8]>) -> AdjustmentBody {
    carried(
        *b"brit",
        "Brightness/Contrast",
        "bcs",
        brightness_outcome(brit, cged),
    )
}

/// The decline reason of a kind a CMYK document has no adjustment layer of.
pub(crate) const CMYK_KIND: &str = "kind has no CMYK adjustment layer in Doctored";

/// The CMYK Gradient Map record (`psd_adjustment_grdm`), which the RGB
/// document does not carry and so is not one of the fourteen.
const GRADIENT_MAP: ([u8; 4], &str, &str) = (*b"grdm", "Gradient Map", "gradient_map");

/// An RGB decode's outcome as the CMYK arms' `Result`.
fn as_result(outcome: Option<Outcome>) -> Option<Result<Map<String, Value>, String>> {
    match outcome? {
        Outcome::Params(params) => Some(Ok(params)),
        Outcome::Declined(reason) => Some(Err(reason)),
    }
}

/// A CMYK file's adjustment body (module doc): `Absent` ONLY for a key
/// outside the fourteen and `grdm`; `Carried` with a typed CMYK description
/// (`meta`, evaluation `precision_v1`) that `cmyk_adjust` parses, or
/// `Declined`. `companion` is the record's `CgEd` (Brightness/Contrast) and
/// `to_ink` the file's sRGB → ink conversion (a `phfl` colour Photoshop wrote
/// as RGB or Lab). Never panics.
pub(crate) fn decode_cmyk(
    key: [u8; 4],
    body: &[u8],
    companion: Option<&[u8]>,
    to_ink: Option<&dyn Fn([u8; 3]) -> Option<[f32; 4]>>,
) -> AdjustmentBody {
    let (kind, op) = if key == GRADIENT_MAP.0 {
        (GRADIENT_MAP.1, GRADIENT_MAP.2)
    } else {
        let Some((_, kind, op)) = KINDS.iter().find(|(candidate, _, _)| *candidate == key) else {
            return AdjustmentBody::Absent;
        };
        (*kind, *op)
    };
    let outcome = match &key {
        b"levl" => psd_adjustment_cmyk::decode_levels(body),
        b"curv" => psd_adjustment_cmyk::decode_curves(body),
        // The RGB layouts, whose params are the CMYK schema's own.
        b"hue2" | b"nvrt" | b"thrs" | b"post" | b"blnc" | b"selc" => {
            as_result(decode_params(key, body, None))
        }
        // Brightness/Contrast on separate plates has no Saturation.
        b"brit" => as_result(brightness_outcome(body, companion)).map(|outcome| {
            outcome.map(|mut params| {
                params.remove("saturation");
                params
            })
        }),
        b"phfl" => psd_adjustment_cmyk::decode_photo_filter(body, to_ink),
        b"mixr" => psd_adjustment_mixr::decode_cmyk_body(body),
        b"grdm" => psd_adjustment_grdm::decode(body),
        _ => {
            return AdjustmentBody::Declined {
                key,
                kind,
                reason: CMYK_KIND.to_string(),
            }
        }
    };
    let reason = match outcome {
        None => SHORT.to_string(),
        Some(Err(reason)) => reason,
        Some(Ok(params)) => {
            let meta = json!({"type": "adjust", "evaluation": "precision_v1", "op": op,
                              "params": params})
            .to_string();
            match crate::cmyk_adjust_layer::CmykAdjustmentState::new(&meta) {
                Ok(state) => {
                    return AdjustmentBody::Carried(Carried {
                        key,
                        kind,
                        meta: state.description().to_string(),
                    })
                }
                Err(_) => OUT_OF_RANGE.to_string(),
            }
        }
    };
    AdjustmentBody::Declined { key, kind, reason }
}

/// A CMYK adjustment record's description (`cmyk_adjust_layer`) as its
/// Photoshop record (module doc), or `None` for an op with none. Reads only
/// `op` and `params`; the body is always even-length.
pub(crate) fn encode_cmyk(description: &str) -> Option<Encoded> {
    let value: Value = serde_json::from_str(description).ok()?;
    let root = value.as_object()?;
    let op = root.get("op")?.as_str()?;
    let empty = Map::new();
    let params = match root.get("params") {
        None => &empty,
        Some(value) => value.as_object()?,
    };
    let mut companion = None;
    let mut body = match op {
        "levels" => psd_adjustment_cmyk::encode_levels(params)?,
        "curves" => psd_adjustment_cmyk::encode_curves(params)?,
        "hue_saturation" | "invert" | "threshold" | "posterize" | "color_balance"
        | "selective_color" => encode_params(op, params)?,
        "bcs" => {
            let (brit, cged) = psd_adjustment_extra::encode_brightness(params)??;
            companion = Some((*b"CgEd", cged));
            brit
        }
        "photo_filter" => psd_adjustment_cmyk::encode_photo_filter(params)?,
        "channel_mixer" => psd_adjustment_mixr::encode_cmyk_body(params)?,
        "gradient_map" => psd_adjustment_grdm::encode(params)?,
        _ => return None,
    };
    if !body.len().is_multiple_of(2) {
        body.push(0);
    }
    let (key, kind) = if op == GRADIENT_MAP.2 {
        (GRADIENT_MAP.0, GRADIENT_MAP.1)
    } else {
        let (key, kind, _) = KINDS.iter().find(|(_, _, candidate)| *candidate == op)?;
        (*key, *kind)
    };
    Some(Encoded {
        key,
        kind,
        body,
        unwritten: None,
        companion,
    })
}

// -------------------------------------------------------------- encoding --

fn number(params: &Map<String, Value>, key: &str, default: f64) -> Option<f64> {
    match params.get(key) {
        None => Some(default),
        Some(value) => value.as_f64(),
    }
}

fn flag(params: &Map<String, Value>, key: &str, default: bool) -> Option<bool> {
    match params.get(key) {
        None => Some(default),
        Some(value) => value.as_bool(),
    }
}

fn nested<'a>(params: &'a Map<String, Value>, key: &str) -> Option<&'a Map<String, Value>> {
    params.get(key).and_then(Value::as_object)
}

fn hundredths(value: f64) -> i16 {
    (value * 100.0).round().clamp(-32768.0, 32767.0) as i16
}

fn degrees(value: f64) -> i16 {
    value.round().rem_euclid(360.0) as i16
}

fn push_i16(out: &mut Vec<u8>, value: i16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn push_f32(out: &mut Vec<u8>, value: f64) {
    out.extend_from_slice(&(value as f32).to_be_bytes());
}

/// `round(value * 100)` as a descriptor long.
fn long_hundredths(value: f64) -> Item {
    Item::Long(
        (value * 100.0)
            .round()
            .clamp(-2_147_483_648.0, 2_147_483_647.0) as i32,
    )
}

/// One channel's curve points as `(input, output)` pairs, or the identity
/// when the channel is absent.
fn curve_points(params: &Map<String, Value>, key: &str) -> Option<Vec<(u16, u16)>> {
    let Some(value) = params.get(key) else {
        return Some(vec![(0, 0), (255, 255)]);
    };
    let list = value.as_array()?;
    let mut points = Vec::with_capacity(list.len());
    for entry in list {
        let pair = entry.as_array()?;
        if pair.len() != 2 {
            return None;
        }
        let code = |v: &Value| v.as_f64().map(|v| v.round().clamp(0.0, 255.0) as u16);
        points.push((code(&pair[0])?, code(&pair[1])?));
    }
    Some(points)
}

/// The map-form `curv` body (module doc): `None` when a present map is not
/// 256 integers 0…255. The caller pads it to an even length.
fn curve_map_body(params: &Map<String, Value>) -> Option<Vec<u8>> {
    let mut maps = Vec::with_capacity(4);
    let mut bits = 0u32;
    for (bit, name) in MAP_KEYS.iter().enumerate() {
        match params.get(*name) {
            Some(value) => {
                maps.push((bit as u16, parse_map(value)?));
                bits |= 1 << bit;
            }
            // The composite bit is always written, as for points.
            None if bit == 0 => {
                maps.push((0, std::array::from_fn(|i| i as u8)));
                bits |= 1;
            }
            None => {}
        }
    }
    let mut out = vec![1];
    push_u16(&mut out, 1);
    out.extend_from_slice(&bits.to_be_bytes());
    for (_, map) in &maps {
        out.extend_from_slice(map);
    }
    out.extend_from_slice(b"Crv ");
    push_u16(&mut out, 3);
    out.extend_from_slice(&(maps.len() as u32).to_be_bytes());
    for (bit, map) in &maps {
        push_u16(&mut out, *bit);
        out.extend_from_slice(map);
    }
    Some(out)
}

/// One stage's `levl` record; absent keys are the identity, so an absent
/// channel object writes the identity record.
pub(crate) fn levels_record(stage: &Map<String, Value>) -> Option<[u16; 5]> {
    let black = number(stage, "black", 0.0)?;
    let white = number(stage, "white", 1.0)?;
    let gamma = number(stage, "gamma", 1.0)?;
    let code = |value: f64| (value * 255.0).round().clamp(0.0, 255.0) as u16;
    let floor = ((black * 255.0).round().clamp(0.0, 253.0)) as u16;
    let ceil = ((white * 255.0).round()).clamp(f64::from(floor) + 1.0, 255.0) as u16;
    let g = (gamma * 100.0).round().clamp(10.0, 999.0) as u16;
    let out_floor = code(number(stage, "output_black", 0.0)?);
    let out_ceil = code(number(stage, "output_white", 1.0)?);
    Some([floor, ceil, out_floor, out_ceil, g])
}

fn encode_params(op: &str, params: &Map<String, Value>) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    match op {
        "invert" => {}
        "threshold" => {
            let level = number(params, "level", 0.5)?;
            push_u16(&mut out, (level * 255.0).round().clamp(1.0, 255.0) as u16);
            out.extend_from_slice(&[0, 0]);
        }
        "posterize" => {
            let levels = number(params, "levels", 4.0)?;
            push_u16(&mut out, levels.round().clamp(0.0, 65535.0) as u16);
            out.extend_from_slice(&[0, 0]);
        }
        "color_balance" => {
            let empty = Map::new();
            for tone in TONES {
                let object = nested(params, tone).unwrap_or(&empty);
                for component in TONE_KEYS {
                    push_i16(&mut out, hundredths(number(object, component, 0.0)?));
                }
            }
            out.push(u8::from(flag(params, "preserve_luminosity", true)?));
            out.push(0);
        }
        "levels" => {
            push_u16(&mut out, 2);
            let empty = Map::new();
            let mut stages = vec![params];
            for name in ["r", "g", "b"] {
                stages.push(match params.get(name) {
                    None => &empty,
                    Some(value) => value.as_object()?,
                });
            }
            for stage in stages {
                for field in levels_record(stage)? {
                    push_u16(&mut out, field);
                }
            }
            for _ in 4..29 {
                for field in LEVELS_IDENTITY {
                    push_u16(&mut out, field);
                }
            }
        }
        "curves" if MAP_KEYS.iter().any(|key| params.contains_key(*key)) => {
            out = curve_map_body(params)?;
        }
        "curves" => {
            out.push(0);
            push_u16(&mut out, 1);
            let mut bits = 1u32;
            for (bit, name) in CURVE_CHANNELS.iter().enumerate().skip(1) {
                if params.contains_key(*name) {
                    bits |= 1 << bit;
                }
            }
            out.extend_from_slice(&bits.to_be_bytes());
            for (bit, name) in CURVE_CHANNELS.iter().enumerate() {
                if bits & (1 << bit) == 0 {
                    continue;
                }
                let points = curve_points(params, name)?;
                push_u16(&mut out, points.len() as u16);
                for (input, output) in points {
                    push_u16(&mut out, output);
                    push_u16(&mut out, input);
                }
            }
            // The natural length is always odd; the block length must be even.
            out.push(0);
        }
        "hue_saturation" => {
            push_u16(&mut out, 2);
            out.push(u8::from(flag(params, "colorize", false)?));
            out.push(0);
            push_i16(&mut out, degrees(number(params, "colorize_hue", 0.0)?));
            push_i16(
                &mut out,
                hundredths(number(params, "colorize_saturation", 0.25)?),
            );
            push_i16(
                &mut out,
                hundredths(number(params, "colorize_lightness", 0.0)?),
            );
            push_i16(
                &mut out,
                number(params, "hue", 0.0)?.round().clamp(-180.0, 180.0) as i16,
            );
            push_i16(&mut out, hundredths(number(params, "saturation", 0.0)?));
            push_i16(&mut out, hundredths(number(params, "lightness", 0.0)?));
            let empty = Map::new();
            let bands = nested(params, "bands").unwrap_or(&empty);
            for (index, name) in HUE_WHEEL.iter().enumerate() {
                let band = nested(bands, name).unwrap_or(&empty);
                let center = number(band, "center", 60.0 * index as f64)?;
                let inner = number(band, "inner", 15.0)?;
                let falloff = number(band, "falloff", 30.0)?;
                for edge in [
                    center - inner - falloff,
                    center - inner,
                    center + inner,
                    center + inner + falloff,
                ] {
                    push_i16(&mut out, degrees(edge));
                }
                push_i16(
                    &mut out,
                    number(band, "hue", 0.0)?.round().clamp(-180.0, 180.0) as i16,
                );
                push_i16(&mut out, hundredths(number(band, "saturation", 0.0)?));
                push_i16(&mut out, hundredths(number(band, "lightness", 0.0)?));
            }
        }
        "photo_filter" => {
            let color = match params.get("color") {
                None => [0xec, 0x8a, 0x00],
                Some(value) => parse_color(value.as_str()?)?,
            };
            let density = number(params, "density", 0.25)?;
            push_u16(&mut out, 2);
            push_u16(&mut out, 0);
            for component in color {
                push_u16(&mut out, u16::from(component) * 257);
            }
            push_u16(&mut out, 0);
            out.extend_from_slice(
                &((density * 100.0).round().clamp(1.0, 100.0) as u32).to_be_bytes(),
            );
            out.push(u8::from(flag(params, "preserve_luminosity", true)?));
            out.extend_from_slice(&[0, 0, 0]);
        }
        "exposure" => {
            push_u16(&mut out, 1);
            push_f32(&mut out, number(params, "exposure", 0.0)?);
            push_f32(&mut out, number(params, "offset", 0.0)?);
            push_f32(&mut out, number(params, "gamma", 1.0)?);
            out.extend_from_slice(&[0, 0]);
        }
        "vibrance" => {
            let descriptor = Descriptor {
                class: b"null".to_vec(),
                items: vec![
                    (
                        b"vibrance".to_vec(),
                        long_hundredths(number(params, "vibrance", 0.0)?),
                    ),
                    (
                        b"Strt".to_vec(),
                        long_hundredths(number(params, "saturation", 0.0)?),
                    ),
                ],
            };
            out = psd_descriptor::write(&descriptor);
        }
        "black_and_white" => {
            let color = match params.get("tint_color") {
                None => [0x99, 0x8a, 0x66],
                Some(value) => parse_color(value.as_str()?)?,
            };
            let mut items = Vec::with_capacity(10);
            for ((key, name), default) in BW_KEYS.iter().zip(HUE_WHEEL).zip(BW_DEFAULTS) {
                items.push((
                    key.to_vec(),
                    long_hundredths(number(params, name, default)?),
                ));
            }
            items.push((
                b"useTint".to_vec(),
                Item::Bool(flag(params, "tint", false)?),
            ));
            items.push((
                b"tintColor".to_vec(),
                Item::Object(Descriptor {
                    class: b"RGBC".to_vec(),
                    items: RGB_KEYS
                        .iter()
                        .zip(color)
                        .map(|(key, component)| (key.to_vec(), Item::Double(f64::from(component))))
                        .collect(),
                }),
            ));
            items.push((b"bwPresetKind".to_vec(), Item::Long(1)));
            items.push((
                b"blackAndWhitePresetFileName".to_vec(),
                Item::Text(String::new()),
            ));
            out = psd_descriptor::write(&Descriptor {
                class: b"null".to_vec(),
                items,
            });
        }
        "channel_mixer" => out = psd_adjustment_mixr::encode(params)?,
        "selective_color" => out = psd_adjustment_extra::encode_selective(params)?,
        _ => return None,
    }
    if !out.len().is_multiple_of(2) {
        out.push(0);
    }
    Some(out)
}

/// `None` when the meta is not an adjustment, its op is not one of the
/// fourteen, or it is a `bcs` layer with saturation, which Photoshop's
/// Brightness/Contrast record cannot hold. Reads ONLY `op` and `params`; every other root key (the typed
/// `evaluation`, host versioning) is ignored. The body is always even-length.
pub(crate) fn encode(meta: &str) -> Option<Encoded> {
    let value: Value = serde_json::from_str(meta).ok()?;
    let root = value.as_object()?;
    if root.get("type")?.as_str()? != "adjust" {
        return None;
    }
    let op = root.get("op")?.as_str()?;
    let empty = Map::new();
    let params = match root.get("params") {
        None => &empty,
        Some(value) => value.as_object()?,
    };
    let (key, kind, _) = KINDS.iter().find(|(_, _, candidate)| *candidate == op)?;
    let (body, companion) = if op == "bcs" {
        let (brit, cged) = psd_adjustment_extra::encode_brightness(params)??;
        (brit, Some((*b"CgEd", cged)))
    } else {
        (encode_params(op, params)?, None)
    };
    let unwritten = if op == "channel_mixer" {
        psd_adjustment_mixr::hidden_row_loss(params)
    } else {
        None
    };
    Some(Encoded {
        key: *key,
        kind,
        body,
        unwritten,
        companion,
    })
}
