//! The Duotone document in the typed `.rz` (#547 Duotone phase 1a, DRDC
//! 109).
//!
//! Word 109 replaces the Gray word 106 in the CMYK slot — the FIRST word
//! inside the identity wrapper 46 — written iff the document is Duotone (a
//! Gray document with a description); the slot holds one of 82–88 or
//! 106–109. The body is the Gray document's own (u16 storage, its profile
//! the Gray profile's twin), and where the CMYK table goes the `GRAY` table
//! (`gray_native`) is followed by the `DUOT` table: `"DUOT"`, u32 schema 1,
//! u8 ink count 1–4; per ink a u16 name byte length and its UTF-8, 3 × f64
//! LE Lab, 13 × (u8 set flag + f64 LE output percent); then u8 overprint
//! count (2ⁿ − 1 − n for n inks) and per combination (`duotone_ink`'s
//! order) a u8 authored flag and 3 × f64 LE Lab (zeros when not authored).
//! The reader requires both tables iff the word, u16 storage ("native
//! Duotone document storage must be 8 or 16 bits") and a table
//! `DuotoneInks::new` accepts, else it refuses by name; the writer runs the
//! same [`check`] first. A reader that knows no 109 meets it where it
//! expects a body version and refuses the file; every earlier file reads
//! unchanged. `DRDC_VERSION` stays 89: 109 is a conditional word like 90 to
//! 108.

use crate::drdc::Reader;
use crate::duotone_ink::{overprint_subsets, DuotoneInk, DuotoneInks, MAX_INKS};
use crate::job_control::JobContext;
use crate::precision_native::append;

/// The Duotone document word.
pub(crate) const WORD: u32 = 109;

/// The refusal of float storage under the word.
pub(crate) const FLOAT_REFUSAL: &str = "native Duotone document storage must be 8 or 16 bits";

/// The `DUOT` table's bytes (module doc); the word replaces 106, so it adds
/// no word bytes of its own. 0 for every other document.
pub(crate) fn header_bytes(inks: Option<&DuotoneInks>) -> u64 {
    inks.map_or(0, |inks| table_bytes(inks).len() as u64)
}

/// The `DUOT` table's bytes (module doc).
pub(crate) fn table_bytes(inks: &DuotoneInks) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"DUOT");
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.push(inks.inks().len() as u8);
    for ink in inks.inks() {
        out.extend_from_slice(&(ink.name.len() as u16).to_le_bytes());
        out.extend_from_slice(ink.name.as_bytes());
        for v in ink.lab {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for slot in ink.curve {
            out.push(u8::from(slot.is_some()));
            out.extend_from_slice(&slot.unwrap_or(0.0).to_le_bytes());
        }
    }
    out.push(inks.overprints().len() as u8);
    for lab in inks.overprints() {
        out.push(u8::from(lab.is_some()));
        for v in lab.unwrap_or([0.0; 3]) {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out
}

/// Writes the `DUOT` table for a Duotone document; nothing otherwise.
pub(crate) fn encode(
    inks: Option<&DuotoneInks>,
    out: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    match inks {
        Some(inks) => append(out, &table_bytes(inks), bound, job),
        None => Ok(()),
    }
}

/// A set flag: 0 or 1, else a refusal.
fn flag(reader: &mut Reader<'_>) -> Result<bool, String> {
    match reader.u8()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err("native Duotone flag must be 0 or 1".into()),
    }
}

/// Reads the `DUOT` table. A schema, count, name, flag or value this build
/// does not admit refuses before any pixel decodes.
pub(crate) fn parse(reader: &mut Reader<'_>) -> Result<DuotoneInks, String> {
    if reader.take(4)? != b"DUOT" || reader.u32()? != 1 {
        return Err("unsupported native Duotone schema".into());
    }
    let count = usize::from(reader.u8()?);
    if !(1..=MAX_INKS).contains(&count) {
        return Err("native Duotone ink count must be 1 to 4".into());
    }
    let mut inks = Vec::with_capacity(count);
    for _ in 0..count {
        let length = usize::from(reader.u16()?);
        let name = std::str::from_utf8(reader.take(length)?)
            .map_err(|_| "native Duotone ink name is not UTF-8")?
            .to_string();
        let lab = [reader.f64()?, reader.f64()?, reader.f64()?];
        let mut curve = [None; 13];
        for slot in &mut curve {
            let set = flag(reader)?;
            let value = reader.f64()?;
            *slot = set.then_some(value);
        }
        inks.push(DuotoneInk { name, lab, curve });
    }
    let expected = overprint_subsets(count).len();
    if usize::from(reader.u8()?) != expected {
        return Err("native Duotone overprint count does not match its inks".into());
    }
    let mut overprints = Vec::with_capacity(expected);
    for _ in 0..expected {
        let authored = flag(reader)?;
        let lab = [reader.f64()?, reader.f64()?, reader.f64()?];
        overprints.push(authored.then_some(lab));
    }
    DuotoneInks::new(inks, overprints).map_err(|e| format!("native Duotone document: {e}"))
}

/// The reader's (and writer's) checks (module doc): u16 storage and a
/// description `DuotoneInks::new` accepts.
pub(crate) fn check(inks: &DuotoneInks, float: bool) -> Result<(), String> {
    if float {
        return Err(FLOAT_REFUSAL.into());
    }
    DuotoneInks::new(inks.inks().to_vec(), inks.overprints().to_vec())
        .map(|_| ())
        .map_err(|e| format!("native Duotone document: {e}"))
}
