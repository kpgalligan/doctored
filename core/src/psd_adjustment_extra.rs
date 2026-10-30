//! `psd_adjustment`'s private half for two records: Brightness/Contrast
//! (`brit` with its `CgEd` companion) and Selective Color (`selc`), both
//! directions. Reached only from `psd_adjustment`, which stays the ONE codec.
//!
//! A record is written only when every field's layout, unit and range is
//! fixed by a Photoshop-written file in the repo or by Adobe's published page
//! (https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/). That holds
//! for these two and for neither Gradient Map (`grdm`) nor Color Lookup
//! (`clrL`), which the export bakes instead (`psd_export_bake`).
//!
//! **Brightness/Contrast.** Measured from the Photoshop 2025 file
//! `tests/fixtures/psd_photoshop/depth16/source.psd` (record "Brightness"):
//! `brit` is 8 zero bytes and the values live in a `CgEd` block of the same
//! record — `u32 16`, descriptor class `null`, then `Vrsn` long 1, `Brgh`
//! long, `Cntr` long, `means` long 127, `Lab ` bool, `useLegacy` bool, `Auto`
//! bool. The legacy `brit` layout is Adobe's table: i16 brightness, i16
//! contrast, i16 mean (127), u8 Lab flag, 1 pad.
//!
//! | Doctored `bcs` | `brit` | `CgEd` |
//! |---|---|---|
//! | `legacy` true (the default) | `round(100·b)`, `round(100·c)`, 127, 0, pad | the same two values, `useLegacy` true |
//! | `legacy` false | 8 zero bytes | `Brgh = round(150·b)`, `Cntr = round(100·c)` for `c ≥ 0` and `round(50·c)` below, `useLegacy` false |
//!
//! Photoshop's modern sliders run −150…150 and −50…100, which is why the
//! scales differ by sign. Import inverts the same table; a record with no
//! `CgEd` is the legacy form read from `brit`. A `bcs` layer whose
//! `saturation` is not 0 has no record — Photoshop's Brightness/Contrast has
//! no saturation — and is baked. A decoded value outside Doctored's −1…1 is
//! declined rather than clamped.
//!
//! **Selective Color.** Adobe's "Selective color file format" table used as
//! the record body, the same identity `adjustment_preset_file` relies on for
//! six other kinds: u16 version 1, u16 method (0 relative, 1 absolute), then
//! ten records of 4 × i16 CMYK percent (−100…100) — the first all zero, then
//! reds, yellows, greens, cyans, blues, magentas, whites, neutrals, blacks —
//! 84 bytes. Each value is `round(100·v)` of the band's `{c, m, y, k}`. This
//! layout has not been opened in Photoshop.

use serde_json::{json, Map, Value};

use crate::psd_descriptor::{self, Descriptor, Item};

/// The nine bands in the record's order, after its reserved first record.
const BANDS: [&str; 9] = [
    "reds", "yellows", "greens", "cyans", "blues", "magentas", "whites", "neutrals", "blacks",
];
const INKS: [&str; 4] = ["c", "m", "y", "k"];
/// version + method + ten 4 × i16 records.
const SELECTIVE_LEN: usize = 4 + 10 * 8;

const VERSION: &str = "version is not 1";
const METHOD: &str = "method is not relative or absolute";
const PERCENT: &str = "percentage is outside -100…100";
const RANGE: &str = "brightness or contrast is outside Doctored's range";

fn i16_at(bytes: &[u8], at: usize) -> Option<i16> {
    Some(i16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn number(params: &Map<String, Value>, key: &str) -> Option<f64> {
    match params.get(key) {
        None => Some(0.0),
        Some(value) => value.as_f64(),
    }
}

/// `round(scale · value)` after the schema's own clamp to −1…1.
fn scaled(value: f64, scale: f64) -> i32 {
    (value.clamp(-1.0, 1.0) * scale).round() as i32
}

// ---------------------------------------------------- Brightness/Contrast --

/// A `brit` record's params; `cged` is the same record's `CgEd` body. `None`
/// is a `brit` shorter than its layout; `Err` is the decline reason.
pub(crate) fn decode_brightness(
    brit: &[u8],
    cged: Option<&[u8]>,
) -> Option<Result<Map<String, Value>, String>> {
    let (brightness, contrast, legacy) = match cged {
        None => {
            let brightness = f64::from(i16_at(brit, 0)?) / 100.0;
            let contrast = f64::from(i16_at(brit, 2)?) / 100.0;
            (brightness, contrast, true)
        }
        Some(body) => {
            let read = || -> Result<(f64, f64, bool), String> {
                let descriptor = psd_descriptor::read(body)?;
                let brightness = f64::from(descriptor.long(b"Brgh")?.unwrap_or(0));
                let contrast = f64::from(descriptor.long(b"Cntr")?.unwrap_or(0));
                Ok(if descriptor.bool(b"useLegacy")?.unwrap_or(false) {
                    (brightness / 100.0, contrast / 100.0, true)
                } else {
                    let scale = if contrast >= 0.0 { 100.0 } else { 50.0 };
                    (brightness / 150.0, contrast / scale, false)
                })
            };
            match read() {
                Ok(values) => values,
                Err(reason) => return Some(Err(reason)),
            }
        }
    };
    if !(-1.0..=1.0).contains(&brightness) || !(-1.0..=1.0).contains(&contrast) {
        return Some(Err(RANGE.to_string()));
    }
    let mut params = Map::new();
    params.insert("brightness".into(), json!(brightness));
    params.insert("contrast".into(), json!(contrast));
    params.insert("legacy".into(), json!(legacy));
    Some(Ok(params))
}

/// The `brit` body and its `CgEd` companion for a `bcs` layer. The outer
/// `None` is a malformed param; the inner `None` is a layer with saturation,
/// which Photoshop's record cannot hold.
#[allow(clippy::type_complexity)]
pub(crate) fn encode_brightness(params: &Map<String, Value>) -> Option<Option<(Vec<u8>, Vec<u8>)>> {
    let brightness = number(params, "brightness")?;
    let contrast = number(params, "contrast")?;
    if number(params, "saturation")? != 0.0 {
        return Some(None);
    }
    let legacy = match params.get("legacy") {
        None => true,
        Some(value) => value.as_bool()?,
    };
    let (brit, values) = if legacy {
        let (b, c) = (scaled(brightness, 100.0), scaled(contrast, 100.0));
        let mut brit = Vec::with_capacity(8);
        brit.extend_from_slice(&(b as i16).to_be_bytes());
        brit.extend_from_slice(&(c as i16).to_be_bytes());
        brit.extend_from_slice(&127i16.to_be_bytes());
        brit.extend_from_slice(&[0, 0]);
        (brit, (b, c))
    } else {
        let c = if contrast >= 0.0 {
            scaled(contrast, 100.0)
        } else {
            scaled(contrast, 50.0)
        };
        (vec![0; 8], (scaled(brightness, 150.0), c))
    };
    let mut cged = psd_descriptor::write(&Descriptor {
        class: b"null".to_vec(),
        items: vec![
            (b"Vrsn".to_vec(), Item::Long(1)),
            (b"Brgh".to_vec(), Item::Long(values.0)),
            (b"Cntr".to_vec(), Item::Long(values.1)),
            (b"means".to_vec(), Item::Long(127)),
            (b"Lab ".to_vec(), Item::Bool(false)),
            (b"useLegacy".to_vec(), Item::Bool(legacy)),
            (b"Auto".to_vec(), Item::Bool(false)),
        ],
    });
    if !cged.len().is_multiple_of(2) {
        cged.push(0);
    }
    Some(Some((brit, cged)))
}

// -------------------------------------------------------- Selective Color --

/// A `selc` body's params; `None` is a body shorter than its layout, `Err`
/// the decline reason. A band at all zeros is omitted (the schema default).
pub(crate) fn decode_selective(body: &[u8]) -> Option<Result<Map<String, Value>, String>> {
    let body = body.get(..SELECTIVE_LEN)?;
    if i16_at(body, 0)? != 1 {
        return Some(Err(VERSION.to_string()));
    }
    let method = match i16_at(body, 2)? {
        0 => "relative",
        1 => "absolute",
        _ => return Some(Err(METHOD.to_string())),
    };
    let mut params = Map::new();
    params.insert("method".into(), json!(method));
    for (index, band) in BANDS.iter().enumerate() {
        let at = 4 + (index + 1) * 8;
        let mut inks = Map::new();
        let mut any = false;
        for (slot, ink) in INKS.iter().enumerate() {
            let percent = i16_at(body, at + slot * 2)?;
            if !(-100..=100).contains(&percent) {
                return Some(Err(PERCENT.to_string()));
            }
            any |= percent != 0;
            inks.insert((*ink).into(), json!(f64::from(percent) / 100.0));
        }
        if any {
            params.insert((*band).into(), Value::Object(inks));
        }
    }
    Some(Ok(params))
}

/// The 84-byte `selc` body; `None` when a param has the wrong type.
pub(crate) fn encode_selective(params: &Map<String, Value>) -> Option<Vec<u8>> {
    let method: u16 = match params.get("method") {
        None => 0,
        Some(value) => match value.as_str()? {
            "relative" => 0,
            "absolute" => 1,
            _ => return None,
        },
    };
    let mut out = Vec::with_capacity(SELECTIVE_LEN);
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&method.to_be_bytes());
    out.extend_from_slice(&[0; 8]);
    let empty = Map::new();
    for band in BANDS {
        let inks = match params.get(band) {
            None => &empty,
            Some(value) => value.as_object()?,
        };
        for ink in INKS {
            out.extend_from_slice(&(scaled(number(inks, ink)?, 100.0) as i16).to_be_bytes());
        }
    }
    Some(out)
}
