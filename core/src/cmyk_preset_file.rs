//! Photoshop's Levels and Curves preset files with ink channels (#1 phase
//! 5): an `.alv` or `.acv` loaded into a CMYK document's Levels or Curves
//! (From Photoshop Preset…, MCP `read_photoshop_adjustment_preset` with
//! `color_mode:"cmyk"`), decoded to the `{op, params}` of `cmyk_adjust`'s
//! schema.
//!
//! * **`.alv`** is a `levl` record body: `u16 version = 2`, then 29 records
//!   of `(in_floor, in_ceil, out_floor, out_ceil, gamma × 100)`. In a CMYK
//!   preset records 0–4 are the composite and C, M, Y, K, in the stored `L`
//!   domain Levels already runs in (`cmyk_adjust`), so each converts to a
//!   stage's keys through `psd_adjustment::levels_stage` (the RGB import's
//!   own conversion): record 0 the top-level stage, records 1–4 `c`, `m`,
//!   `y`, `k` when not the identity. Each of the five is checked as the
//!   RGB decode checks records 0–3 (ceiling above floor, gamma 0.10–9.99).
//! * **`.acv`** is `u16 version (1|4), u16 count`, then `count` curves of
//!   `u16 n` and `n × (u16 output, u16 input)`, read with
//!   `adjustment_preset_file`'s own reader. A CMYK preset's curves 1–5 are
//!   the composite and C, M, Y, K, stored in the `L` domain; the ink axes
//!   `cmyk_adjust` takes are their point reflection `(255 − in, 255 − out)`
//!   (lossless: the natural spline commutes with it, `cmyk_adjust`'s module
//!   doc). A file with fewer than five curves is an RGB preset and refuses
//!   by name, as does a curve past the fifth that is not the two-point
//!   identity.
//!
//! Every other extension refuses: "A .<ext> preset is unavailable in a CMYK
//! document". Files are capped as the RGB decode caps them.

use serde_json::{json, Map, Value};

use crate::adjustment_preset_file::{too_large, u16_at, MAX_CURVES, MAX_FILE_BYTES, TRUNCATED};
use crate::cmyk_document::refused_sentence;
use crate::psd_adjustment::{levels_stage, LEVELS_IDENTITY};

/// The channel keys of a CMYK preset, after the composite.
const INKS: [&str; 4] = ["c", "m", "y", "k"];

/// Records (and curves) a CMYK preset uses: the composite and four inks.
const CMYK_RECORDS: usize = 5;

/// Decodes a CMYK `.alv` or `.acv` (module doc): `{"op","params"}`.
pub(crate) fn decode_cmyk(bytes: &[u8], extension: &str) -> Result<Value, String> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(too_large(bytes.len()));
    }
    match extension.to_ascii_lowercase().as_str() {
        "alv" => levels(bytes),
        "acv" => curves(bytes),
        other => Err(refused_sentence(&format!("A .{other} preset"))),
    }
}

fn stage_object(record: [u16; 5]) -> Map<String, Value> {
    levels_stage(record)
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

fn levels(bytes: &[u8]) -> Result<Value, String> {
    let truncated = "levels preset is truncated";
    let word = |at: usize| u16_at(bytes, at).map_err(|_| truncated.to_string());
    if word(0)? != 2 {
        return Err("this Photoshop Levels preset cannot be used: its version is not 2".into());
    }
    let mut records = [[0u16; 5]; CMYK_RECORDS];
    for (r, record) in records.iter_mut().enumerate() {
        for (f, field) in record.iter_mut().enumerate() {
            *field = word(2 + (r * 5 + f) * 2)?;
        }
    }
    // The rest of the 29 records must be present, as in a `levl` body.
    word(2 + (29 * 5 - 1) * 2)?;
    for [floor, ceil, _, _, gamma] in &records {
        if ceil <= floor {
            return Err("this Photoshop Levels preset cannot be used: its input ceiling does not exceed its floor".into());
        }
        if !(10..=999).contains(gamma) {
            return Err(
                "this Photoshop Levels preset cannot be used: its gamma is outside 0.10…9.99"
                    .into(),
            );
        }
    }
    let mut params = stage_object(records[0]);
    for (name, record) in INKS.iter().zip(&records[1..]) {
        if *record != LEVELS_IDENTITY {
            params.insert((*name).to_string(), Value::Object(stage_object(*record)));
        }
    }
    Ok(json!({"op":"levels","params":params}))
}

fn curves(bytes: &[u8]) -> Result<Value, String> {
    let version = u16_at(bytes, 0)?;
    let count = u16_at(bytes, 2)?;
    if !matches!(version, 1 | 4) {
        return Err(format!("curves preset version {version} is not 1 or 4"));
    }
    if !(1..=MAX_CURVES).contains(&count) {
        return Err(format!(
            "curves preset holds {count} curves; expected 1 to {MAX_CURVES}"
        ));
    }
    if usize::from(count) < CMYK_RECORDS {
        return Err(format!(
            "this curves preset holds {count} curves, an RGB preset; a CMYK document needs a CMYK preset of five curves"
        ));
    }
    let mut params = Map::new();
    let mut at = 4usize;
    for index in 0..usize::from(count) {
        let n = usize::from(u16_at(bytes, at)?);
        let start = at + 2;
        let end = start + n * 4;
        bytes.get(start..end).ok_or(TRUNCATED)?;
        let mut points: Vec<(u16, u16)> = (0..n)
            .map(|p| {
                let output = u16_at(bytes, start + p * 4)?;
                let input = u16_at(bytes, start + p * 4 + 2)?;
                Ok((input, output))
            })
            .collect::<Result<_, String>>()?;
        at = end;
        if index >= CMYK_RECORDS {
            if points != [(0, 0), (255, 255)] {
                return Err(format!(
                    "curves preset curve {} is not identity; a CMYK preset holds five curves",
                    index + 1
                ));
            }
            continue;
        }
        if points.iter().any(|(i, o)| *i > 255 || *o > 255) {
            return Err(format!(
                "curves preset curve {} has a point outside 0…255",
                index + 1
            ));
        }
        if points == [(0, 0), (255, 255)] {
            continue;
        }
        // The point reflection onto the ink axes (module doc).
        points.reverse();
        let reflected: Vec<Value> = points
            .iter()
            .map(|(i, o)| json!([255 - i, 255 - o]))
            .collect();
        let key = if index == 0 { "cmyk" } else { INKS[index - 1] };
        params.insert(key.to_string(), Value::Array(reflected));
    }
    Ok(json!({"op":"curves","params":params}))
}
