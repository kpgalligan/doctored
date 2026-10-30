//! The Channel Mixer record body (`mixr`; a CMYK document's since the
//! #1 styles batch, [`decode_cmyk_body`] / [`encode_cmyk_body`]) — the private half of
//! `psd_adjustment`, which is the ONE codec entry point and the only caller
//! of this module (the `psd_layer_effects_fx` precedent). A `.cha` preset
//! file is this body verbatim, so `adjustment_preset_file` reaches it through
//! `psd_adjustment::decode` too.
//!
//! # Layout
//!
//! Adobe's "Additional Layer Information" table gives `mixr` as "2 — Version
//! ( = 1); 2 — Monochrome; 20 — RGB or CMYK color plus constant for the mixer
//! settings. 4 * 2 bytes of color with 2 bytes of constant." The "20" is
//! wrong: every body Photoshop 2025 (26.10.0) writes, and every shipped
//! `.cha`, is 44 bytes, big-endian —
//!
//! - `u16 version` (= 1), `u16 monochrome` (≠ 0 = on);
//! - then 4 records × 5 × `i16` percent: `[source R (C), source G (M),
//!   source B (Y), source 4th (CMYK's K), constant]`.
//!
//! What the records mean was MEASURED against Photoshop-authored files
//! (`tests/fixtures/psd_photoshop/channel_mixer/`, the probe's `author` job
//! with the `channel_mixer` kind), not taken from Adobe's text:
//!
//! - Monochrome off: records 0, 1, 2 are the Red, Green and Blue outputs, in
//!   that order, each `[R, G, B, 0, constant]`; record 3 (CMYK's fourth
//!   output) is all zero.
//! - Monochrome on: record 0 is the Gray output. Photoshop keeps Gray and Red
//!   in the SAME record, and when a layer was given green/blue rows as well it
//!   leaves them in records 1 and 2; with only a Gray row they are zero.
//!   Records 1–3 are not read under Monochrome — the checkbox means only the
//!   gray mix applies — so such hidden rows are not carried.
//! - The six shipped `.cha` presets are all Monochrome on with records 1–3
//!   zero.
//!
//! # Decisions
//!
//! - Decode order: length ≥ 44 (`None`, the caller's SHORT), then version
//!   1, then CMYK, then range; a tail past 44 bytes is ignored like every
//!   other record's.
//! - A CMYK body or preset is DECLINED, never misread: with Monochrome off the
//!   K-source field of records 0–2 and every field of record 3 must be zero;
//!   with Monochrome on, record 0's K-source field.
//! - Every field that is read (indices 0, 1, 2 and 4 of the records used)
//!   must lie in −200…200, the app's `[-2, 2]` as whole percent; ±200 itself
//!   is accepted (the shipped Infrared preset uses 200). Nothing is clamped.
//! - Values are fractions, `percent / 100`; a row that is not read is omitted,
//!   so its schema default applies.
//! - Encode writes exactly 44 bytes from the VALIDATED rows (`adjust_parse`
//!   with `adjust_mix`'s row constants, the one schema), each field
//!   `round(v·100)`: Monochrome on writes Gray into record 0 and zeros
//!   elsewhere; off writes Red, Green and Blue into records 0–2 with K = 0
//!   and record 3 zero. So a 44-byte RGB body whose unread parts are zero —
//!   every shape Photoshop's presets carry — round-trips byte for byte.
//! - The app keeps the row Monochrome hides (gray while off; red/green/blue
//!   while on) so the checkbox can toggle without losing either. Doctored
//!   writes only the visible rows: under Monochrome only Gray, which
//!   Photoshop keeps in the Red record, so Red has no room at all; without
//!   it, Gray has no record of its own. `hidden_row_loss` names the
//!   loss when that hidden row differs from its default at whole percent, and
//!   the export preflight reports it (layered mode only).

use serde_json::{json, Map, Value};

use crate::adjust_mix::{MIXER_GRAY, MIXER_KEYS, MIXER_ROWS};
use crate::adjust_parse::nested_f64;
use crate::adjust_parse::{boolean, nested};

/// The body's length: two words, then four five-word records.
pub(crate) const BODY_LEN: usize = 44;

const RECORDS: usize = 4;
const FIELDS: usize = 5;
/// The record field that is CMYK's fourth source (K).
const K_SOURCE: usize = 3;
/// The record fields an RGB mixer reads: R, G, B sources and the constant.
const READ_FIELDS: [usize; 4] = [0, 1, 2, 4];
/// The schema's `[-2, 2]`, as whole percent.
const PERCENT_LIMIT: i16 = 200;
const SCHEMA_RANGE: std::ops::RangeInclusive<f64> = -2.0..=2.0;

const VERSION: &str = "version is not 1";
const CMYK: &str = "body holds CMYK channel settings";
const RANGE: &str = "source or constant percentage is outside -200…200";

const HIDDEN_COLOR_ROWS: &str = "keeps Red, Green and Blue output rows hidden under Monochrome; Doctored writes only the gray row (Photoshop keeps Gray in the Red record), so they are not written and reopen at their defaults.";
const HIDDEN_GRAY_ROW: &str = "keeps a Monochrome gray row hidden while Monochrome is off; a Photoshop mixr record holds only the colour rows, so it is not written and reopens at its defaults.";

/// `None` = body shorter than `BODY_LEN` (the caller reports SHORT);
/// `Some(Err(reason))` = declined; `Some(Ok(params))` = byte-form
/// `channel_mixer` params. Ignores any tail past 44 bytes.
pub(crate) fn decode(body: &[u8]) -> Option<Result<Map<String, Value>, String>> {
    let words = body.get(..BODY_LEN)?;
    let word = |index: usize| i16::from_be_bytes([words[2 * index], words[2 * index + 1]]);
    let version = word(0) as u16;
    let monochrome = word(1) != 0;
    let records: [[i16; FIELDS]; RECORDS] =
        std::array::from_fn(|r| std::array::from_fn(|f| word(2 + r * FIELDS + f)));
    if version != 1 {
        return Some(Err(VERSION.to_string()));
    }
    let used = if monochrome { 1 } else { 3 };
    let cmyk = records[..used].iter().any(|record| record[K_SOURCE] != 0)
        || (!monochrome && records[3].iter().any(|field| *field != 0));
    if cmyk {
        return Some(Err(CMYK.to_string()));
    }
    let out_of_range = records[..used].iter().any(|record| {
        READ_FIELDS
            .iter()
            .any(|&field| !(-PERCENT_LIMIT..=PERCENT_LIMIT).contains(&record[field]))
    });
    if out_of_range {
        return Some(Err(RANGE.to_string()));
    }
    let row = |record: &[i16; FIELDS]| -> Value {
        let mut object = Map::new();
        for (key, field) in MIXER_KEYS.iter().zip(READ_FIELDS) {
            object.insert((*key).into(), json!(f64::from(record[field]) / 100.0));
        }
        Value::Object(object)
    };
    let mut params = Map::new();
    params.insert("monochrome".into(), json!(monochrome));
    if monochrome {
        params.insert("gray".into(), row(&records[0]));
    } else {
        for ((name, _), record) in MIXER_ROWS.iter().zip(&records) {
            params.insert((*name).into(), row(record));
        }
    }
    Some(Ok(params))
}

/// One validated row as whole percent: `round(v · 100)` per key.
fn percents(row: [f32; 4]) -> [i16; 4] {
    row.map(|value| (f64::from(value) * 100.0).round() as i16)
}

/// The parsed rows: Monochrome, then red/green/blue and gray, each validated
/// against the schema exactly as `ChannelMixer::parse` reads them.
fn rows(params: &Map<String, Value>) -> Option<(bool, [[f32; 4]; 3], [f32; 4])> {
    let monochrome = boolean(params, "monochrome", false)?;
    let mut colour = [[0.0f32; 4]; 3];
    for (row, (name, defaults)) in colour.iter_mut().zip(MIXER_ROWS) {
        *row = nested(params, name, MIXER_KEYS, SCHEMA_RANGE, defaults)?;
    }
    let gray = nested(params, "gray", MIXER_KEYS, SCHEMA_RANGE, MIXER_GRAY)?;
    Some((monochrome, colour, gray))
}

/// Exactly `BODY_LEN` bytes; `None` when a row or the flag is the wrong JSON
/// type or outside the schema range.
pub(crate) fn encode(params: &Map<String, Value>) -> Option<Vec<u8>> {
    let (monochrome, colour, gray) = rows(params)?;
    let mut records = [[0i16; FIELDS]; RECORDS];
    let written: Vec<[f32; 4]> = if monochrome {
        vec![gray]
    } else {
        colour.to_vec()
    };
    for (record, row) in records.iter_mut().zip(written) {
        let [r, g, b, constant] = percents(row);
        *record = [r, g, b, 0, constant];
    }
    let mut out = Vec::with_capacity(BODY_LEN);
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&u16::from(monochrome).to_be_bytes());
    for field in records.iter().flatten() {
        out.extend_from_slice(&field.to_be_bytes());
    }
    Some(out)
}

/// The clause for a hidden row `encode` drops whose whole-percent values
/// differ from its schema default; `None` when nothing hidden differs (or the
/// params do not parse).
pub(crate) fn hidden_row_loss(params: &Map<String, Value>) -> Option<&'static str> {
    let (monochrome, colour, gray) = rows(params)?;
    let differs = |row: [f32; 4], defaults: [f64; 4]| {
        percents(row) != percents(defaults.map(|value| value as f32))
    };
    if monochrome {
        colour
            .iter()
            .zip(MIXER_ROWS)
            .any(|(row, (_, defaults))| differs(*row, defaults))
            .then_some(HIDDEN_COLOR_ROWS)
    } else {
        differs(gray, MIXER_GRAY).then_some(HIDDEN_GRAY_ROW)
    }
}

// ------------------------------------------------------------------ CMYK --

/// A CMYK row's keys and the CMYK kernel's defaults (`cmyk_adjust_ink`'s
/// Channel Mixer): each output its own plate, Gray from K.
const CMYK_KEYS: [&str; 5] = ["c", "m", "y", "k", "constant"];
const CMYK_ROWS: [(&str, [f64; 5]); 4] = [
    ("cyan", [1.0, 0.0, 0.0, 0.0, 0.0]),
    ("magenta", [0.0, 1.0, 0.0, 0.0, 0.0]),
    ("yellow", [0.0, 0.0, 1.0, 0.0, 0.0]),
    ("black", [0.0, 0.0, 0.0, 1.0, 0.0]),
];
const CMYK_GRAY: [f64; 5] = [0.0, 0.0, 0.0, 1.0, 0.0];

/// A CMYK document's `mixr` body (#1; Photoshop take1's `adjust9.psd`):
/// Monochrome off, records 0–3 are the Cyan, Magenta, Yellow and Black
/// outputs, each `{c, m, y, k, constant}` = `percent / 100`; on, record 0 is
/// the Gray output. Every field read lies in −200…200, else declined; `None`
/// is a body shorter than 44 bytes.
pub(crate) fn decode_cmyk_body(body: &[u8]) -> Option<Result<Map<String, Value>, String>> {
    let words = body.get(..BODY_LEN)?;
    let word = |index: usize| i16::from_be_bytes([words[2 * index], words[2 * index + 1]]);
    if word(0) as u16 != 1 {
        return Some(Err(VERSION.to_string()));
    }
    let monochrome = word(1) != 0;
    let records: [[i16; FIELDS]; RECORDS] =
        std::array::from_fn(|r| std::array::from_fn(|f| word(2 + r * FIELDS + f)));
    let used = if monochrome { 1 } else { RECORDS };
    if records[..used]
        .iter()
        .flatten()
        .any(|field| !(-PERCENT_LIMIT..=PERCENT_LIMIT).contains(field))
    {
        return Some(Err(RANGE.to_string()));
    }
    let row = |record: &[i16; FIELDS]| -> Value {
        let mut object = Map::new();
        for (key, field) in CMYK_KEYS.iter().zip(record) {
            object.insert((*key).into(), json!(f64::from(*field) / 100.0));
        }
        Value::Object(object)
    };
    let mut params = Map::new();
    params.insert("monochrome".into(), json!(monochrome));
    if monochrome {
        params.insert("gray".into(), row(&records[0]));
    } else {
        for ((name, _), record) in CMYK_ROWS.iter().zip(&records) {
            params.insert((*name).into(), row(record));
        }
    }
    Some(Ok(params))
}

/// CMYK `channel_mixer` params as the 44-byte body ([`decode_cmyk_body`]'s
/// layout): Monochrome writes Gray into record 0 and zeros elsewhere, else
/// the four outputs. `None` when a row or the flag does not parse.
pub(crate) fn encode_cmyk_body(params: &Map<String, Value>) -> Option<Vec<u8>> {
    let monochrome = boolean(params, "monochrome", false)?;
    let mut records = [[0i16; FIELDS]; RECORDS];
    if monochrome {
        let gray = nested_f64(params, "gray", CMYK_KEYS, SCHEMA_RANGE, CMYK_GRAY)?;
        records[0] = gray.map(|v| (v * 100.0).round() as i16);
    } else {
        for (record, (name, defaults)) in records.iter_mut().zip(CMYK_ROWS) {
            let row = nested_f64(params, name, CMYK_KEYS, SCHEMA_RANGE, defaults)?;
            *record = row.map(|v| (v * 100.0).round() as i16);
        }
    }
    let mut out = Vec::with_capacity(BODY_LEN);
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&u16::from(monochrome).to_be_bytes());
    for field in records.iter().flatten() {
        out.extend_from_slice(&field.to_be_bytes());
    }
    Some(out)
}
