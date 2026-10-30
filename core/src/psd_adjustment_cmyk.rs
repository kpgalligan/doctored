//! `psd_adjustment`'s private half for a CMYK document's `levl`, `curv` and
//! (the styles batch) `phfl` bodies (#1), both directions; reached only through
//! `psd_adjustment::decode_cmyk` / `encode_cmyk`, which own the kinds table,
//! the decline reasons and the `hue2` arm (the RGB body, unchanged).
//!
//! The layouts are the RGB rows of `psd_adjustment`'s module table; only the
//! channel slots differ. Measured against Photoshop 26.10.0's own CMYK file
//! (`doctored-fixtures/photoshop/cmyk-psd-547/take1/adjust/adjust16.psd`):
//!
//! * `levl` records 0–4 are the composite and C, M, Y, K, each in the stored
//!   `L = 1 − ink` domain Levels already runs in (`cmyk_adjust`), so a record
//!   is a stage through `levels_stage` / `levels_record` exactly as the RGB
//!   body's — and exactly as a CMYK `.alv` preset (`cmyk_preset_file`).
//! * `curv` bits 0–4 are the composite and C, M, Y, K, its points stored in
//!   that `L` domain; the ink points `cmyk_adjust` takes are their point
//!   reflection `(255 − in, 255 − out)`, the `.acv` preset's rule. A two-point
//!   identity curve is no key (as in the preset), and the encoder writes the
//!   composite bit always (an identity curve when `cmyk` is absent), as the
//!   RGB encoder does. A drawn (mapped) curve declines: a CMYK document has
//!   no mapped curves.
//! * `phfl` is the RGB layout; its colour is the CMYK params' `ink`. Version
//!   2 space 2 (CMYK) components are `65535·(1 − ink)` (Adobe's colour
//!   structure: 0 is 100 % ink), read exactly (quantized to four decimals);
//!   space 0 (RGB, `c / 257`) and 7 (Lab hundredths, the space Photoshop
//!   26.10.0 writes even for a `CMYC` colour — take1's `adjust9.psd`) go to
//!   an sRGB byte triple and then through the import's `to_ink` (the file's
//!   CMYK profile, relative colorimetric with BPC). The encoder writes
//!   version 2, space 2; with no `ink` the kernel's default colour's ink.

use serde_json::{json, Map, Value};

use crate::psd_adjustment::{levels_record, levels_stage, LEVELS_IDENTITY};

/// The composite and four ink keys, in record (and bit) order.
const STAGES: [&str; 5] = ["", "c", "m", "y", "k"];
const CURVES: [&str; 5] = ["cmyk", "c", "m", "y", "k"];

/// A mapped `curv` body's decline reason (it reads after "whose").
pub(crate) const MAPPED: &str = "curve is drawn (mapped), which has no CMYK form in Doctored";

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u8(&mut self) -> Option<u8> {
        let out = *self.bytes.get(self.at)?;
        self.at += 1;
        Some(out)
    }
    fn u16(&mut self) -> Option<u16> {
        let out = self.bytes.get(self.at..self.at + 2)?;
        self.at += 2;
        Some(u16::from_be_bytes([out[0], out[1]]))
    }
    fn u32(&mut self) -> Option<u32> {
        let out = self.bytes.get(self.at..self.at + 4)?;
        self.at += 4;
        Some(u32::from_be_bytes([out[0], out[1], out[2], out[3]]))
    }
}

fn stage_object(record: [u16; 5]) -> Value {
    Value::Object(
        levels_stage(record)
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

/// A CMYK `levl` body's params: `None` for a body shorter than its layout,
/// `Some(Err)` for a decline reason.
pub(crate) fn decode_levels(body: &[u8]) -> Option<Result<Map<String, Value>, String>> {
    let mut r = Reader { bytes: body, at: 0 };
    let version = r.u16()?;
    let mut records = [[0u16; 5]; 29];
    for record in &mut records {
        for field in record.iter_mut() {
            *field = r.u16()?;
        }
    }
    if version != 2 {
        return Some(Err("version is not 2".into()));
    }
    for [floor, ceil, _, _, gamma] in &records[..=4] {
        if ceil <= floor {
            return Some(Err("input ceiling does not exceed its floor".into()));
        }
        if !(10..=999).contains(gamma) {
            return Some(Err("gamma is outside 0.10…9.99".into()));
        }
    }
    let mut params: Map<String, Value> = levels_stage(records[0])
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect();
    for (name, record) in STAGES[1..].iter().zip(&records[1..=4]) {
        if *record != LEVELS_IDENTITY {
            params.insert((*name).to_string(), stage_object(*record));
        }
    }
    Some(Ok(params))
}

/// A CMYK `curv` body's params (module doc), with the same `None` / `Err`
/// meanings as [`decode_levels`].
pub(crate) fn decode_curves(body: &[u8]) -> Option<Result<Map<String, Value>, String>> {
    let mut r = Reader { bytes: body, at: 0 };
    let is_map = r.u8()?;
    let version = r.u16()?;
    let bits = r.u32()?;
    if is_map == 1 {
        return Some(Err(MAPPED.into()));
    }
    if is_map > 1 {
        return Some(Err("map flag is not 0 or 1".into()));
    }
    if !matches!(version, 1 | 4) {
        return Some(Err("version is not 1 or 4".into()));
    }
    if bits & !0x1F != 0 {
        return Some(Err("channel bits beyond black are set".into()));
    }
    let mut params = Map::new();
    for (bit, name) in CURVES.iter().enumerate() {
        if bits & (1 << bit) == 0 {
            continue;
        }
        let count = r.u16()?;
        if !(2..=16).contains(&count) {
            return Some(Err("point count is outside 2…16".into()));
        }
        let mut stored = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            let output = r.u16()?;
            let input = r.u16()?;
            if input > 255 || output > 255 {
                return Some(Err("point value exceeds 255".into()));
            }
            stored.push((input, output));
        }
        if stored == [(0, 0), (255, 255)] {
            continue;
        }
        // The point reflection onto the ink axes, ascending again.
        let ink: Vec<Value> = stored
            .iter()
            .rev()
            .map(|(i, o)| json!([255 - i, 255 - o]))
            .collect();
        params.insert((*name).to_string(), Value::Array(ink));
    }
    Some(Ok(params))
}

/// The CMYK `levl` body (292 bytes, the RGB encoder's length): record 0 from
/// the top-level stage, records 1–4 from `c`, `m`, `y`, `k` (identity when
/// absent), records 5–28 identity. `None` when a present stage is not an
/// object of numbers.
pub(crate) fn encode_levels(params: &Map<String, Value>) -> Option<Vec<u8>> {
    let empty = Map::new();
    let mut out = 2u16.to_be_bytes().to_vec();
    let mut stages = vec![params];
    for name in &STAGES[1..] {
        stages.push(match params.get(*name) {
            None => &empty,
            Some(value) => value.as_object()?,
        });
    }
    for stage in stages {
        for field in levels_record(stage)? {
            out.extend_from_slice(&field.to_be_bytes());
        }
    }
    for _ in 5..29 {
        for field in LEVELS_IDENTITY {
            out.extend_from_slice(&field.to_be_bytes());
        }
    }
    Some(out)
}

/// One ink curve stored back in the `L` domain, ascending: the inverse of
/// [`decode_curves`]' reflection; the identity when the key is absent.
fn stored_points(params: &Map<String, Value>, key: &str) -> Option<Vec<(u16, u16)>> {
    let Some(value) = params.get(key) else {
        return Some(vec![(0, 0), (255, 255)]);
    };
    let mut points = Vec::new();
    for entry in value.as_array()? {
        let pair = entry.as_array()?;
        if pair.len() != 2 {
            return None;
        }
        let code = |v: &Value| v.as_f64().map(|v| v.round().clamp(0.0, 255.0) as u16);
        points.push((255 - code(&pair[0])?, 255 - code(&pair[1])?));
    }
    points.reverse();
    Some(points)
}

/// The CMYK point-form `curv` body (module doc), unpadded: is_map 0,
/// version 1, the composite bit always, then each present ink.
pub(crate) fn encode_curves(params: &Map<String, Value>) -> Option<Vec<u8>> {
    let mut bits = 1u32;
    for (bit, name) in CURVES.iter().enumerate().skip(1) {
        if params.contains_key(*name) {
            bits |= 1 << bit;
        }
    }
    let mut out = vec![0];
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&bits.to_be_bytes());
    for (bit, name) in CURVES.iter().enumerate() {
        if bits & (1 << bit) == 0 {
            continue;
        }
        let points = stored_points(params, name)?;
        out.extend_from_slice(&(points.len() as u16).to_be_bytes());
        for (input, output) in points {
            out.extend_from_slice(&output.to_be_bytes());
            out.extend_from_slice(&input.to_be_bytes());
        }
    }
    Some(out)
}

/// The CMYK kernel's colour with no `ink`: the RGB default `#ec8a00` read as
/// `L` (`cmyk_adjust_rgb::InkRgb`), i.e. `1 − code / 255`, K paper.
const DEFAULT_FILTER_INK: [f64; 4] = [1.0 - 236.0 / 255.0, 1.0 - 138.0 / 255.0, 1.0, 0.0];

fn q4(v: f64) -> f64 {
    (v * 1e4).round() / 1e4
}

/// A `phfl` body as CMYK `photo_filter` params (module doc): `None` is a
/// body shorter than its layout, `Err` the decline reason.
pub(crate) fn decode_photo_filter(
    body: &[u8],
    to_ink: Option<&dyn Fn([u8; 3]) -> Option<[f32; 4]>>,
) -> Option<Result<Map<String, Value>, String>> {
    let mut r = Reader { bytes: body, at: 0 };
    let version = r.u16()?;
    match version {
        3 => return Some(Err("XYZ colour form is not mapped".into())),
        2 => {}
        _ => return Some(Err("version is not 2 or 3".into())),
    }
    let space = r.u16()?;
    let components = [r.u16()?, r.u16()?, r.u16()?, r.u16()?];
    let density = r.u32()?;
    let preserve = r.u8()? != 0;
    let converted = |rgb: [u8; 3]| -> Result<[f64; 4], String> {
        to_ink
            .and_then(|f| f(rgb))
            .map(|ink| ink.map(|v| q4(f64::from(v))))
            .ok_or_else(|| "colour cannot be converted into the file's CMYK profile".to_string())
    };
    let ink = match space {
        2 => Ok(components.map(|c| q4(1.0 - f64::from(c) / 65535.0))),
        0 => converted(std::array::from_fn(|i| {
            (f64::from(components[i]) / 257.0).round().clamp(0.0, 255.0) as u8
        })),
        7 => {
            let lab = std::array::from_fn(|i| f32::from(components[i] as i16) * 0.01);
            match crate::lab::lab_to_srgb8(lab) {
                Some(rgb) => converted(rgb),
                None => Err("the built-in sRGB profile has no model".into()),
            }
        }
        _ => Err("colour space is not RGB, CMYK or Lab".into()),
    };
    let ink = match ink {
        Ok(ink) => ink,
        Err(reason) => return Some(Err(reason)),
    };
    let mut params = Map::new();
    params.insert("ink".into(), json!(ink));
    params.insert("density".into(), json!(f64::from(density) / 100.0));
    params.insert("preserve_luminosity".into(), json!(preserve));
    Some(Ok(params))
}

/// CMYK `photo_filter` params as a version-2, space-2 `phfl` body (module
/// doc), 20 bytes; `None` when a param is the wrong JSON type.
pub(crate) fn encode_photo_filter(params: &Map<String, Value>) -> Option<Vec<u8>> {
    let ink: [f64; 4] = match params.get("ink") {
        None => DEFAULT_FILTER_INK,
        Some(value) => {
            let list = value.as_array().filter(|l| l.len() == 4)?;
            let values: Vec<f64> = list.iter().map(Value::as_f64).collect::<Option<_>>()?;
            [values[0], values[1], values[2], values[3]]
        }
    };
    let density = match params.get("density") {
        None => 0.25,
        Some(value) => value.as_f64()?,
    };
    let preserve = match params.get("preserve_luminosity") {
        None => true,
        Some(value) => value.as_bool()?,
    };
    let mut out = Vec::with_capacity(20);
    out.extend_from_slice(&2u16.to_be_bytes());
    out.extend_from_slice(&2u16.to_be_bytes());
    for v in ink {
        let code = ((1.0 - v.clamp(0.0, 1.0)) * 65535.0).round() as u16;
        out.extend_from_slice(&code.to_be_bytes());
    }
    out.extend_from_slice(&((density * 100.0).round().clamp(1.0, 100.0) as u32).to_be_bytes());
    out.push(u8::from(preserve));
    out.extend_from_slice(&[0, 0, 0]);
    Some(out)
}
