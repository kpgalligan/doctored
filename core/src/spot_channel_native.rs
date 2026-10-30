//! The conditional SPOT CHANNEL word and table (#1, `spot_channel`): which
//! channels are spot channels, and each one's ink. The plane and name are
//! the channel record's own, written as every alpha channel's are.
//! `reference_native.rs` is the shape this follows; `gradient_map_method_native`
//! the placement.
//!
//! Word 112 is one number at both depths: in a byte file the FIRST word
//! inside 45 (ahead of 111), in a typed file the word right after the CMYK
//! slot (ahead of 111, a CMYK file's too). Its table, `SPOT`, is u32 schema
//! 1, u32 count (1..=channels), then count × (u32 channel index strictly
//! ascending, f64 L, f64 a, f64 b little-endian, u8 solidity): in a byte
//! file after `LREF`, before LIDS; in a typed file after `LREF`, before the
//! CMYK table. The ink is D50 Lab, solidity a percent (`SpotInk`).
//!
//! Both are written ONLY when some channel is a spot channel, so every other
//! document is byte-for-byte the prior writer's file. A pre-112 reader
//! refuses the word by its number (`DRDC_VERSION` stays 89). A bad schema,
//! count, order, index or ink refuses before any pixel decodes, as every
//! other conditional table does. The typed reader applies the entries after
//! the CHPR block, which replaces each channel's whole metadata.
use crate::doc_channel::Channel;
use crate::drdc::Reader;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_native::append;
use crate::spot_channel::SpotInk;

/// The conditional `.rz` word at both depths (module doc).
pub(crate) const WORD: u32 = 112;

/// One table entry: a channel index and its ink.
pub(crate) type Entry = (usize, SpotInk);

/// Bytes an entry takes: index, three f64 and the solidity byte.
const ENTRY_BYTES: u64 = 4 + 3 * 8 + 1;

/// (index, ink) for every byte spot channel, ascending.
pub(crate) fn byte_entries(channels: &[Channel]) -> Vec<Entry> {
    channels
        .iter()
        .enumerate()
        .filter_map(|(i, c)| c.spot.map(|ink| (i, ink)))
        .collect()
}

/// (index, ink) for every typed spot channel, ascending.
pub(crate) fn precision_entries(document: &PrecisionDocument) -> Vec<Entry> {
    document
        .channels()
        .iter()
        .enumerate()
        .filter_map(|(i, c)| c.metadata().spot.map(|ink| (i, ink)))
        .collect()
}

/// The word and the table's size together (4 + 12 + 29 · count), or 0 when
/// neither is written.
pub(crate) fn header_bytes(count: usize) -> u64 {
    if count == 0 {
        0
    } else {
        16 + count as u64 * ENTRY_BYTES
    }
}

pub(crate) fn encode(
    entries: &[Entry],
    out: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    if entries.is_empty() {
        return Ok(());
    }
    append(out, b"SPOT", bound, job)?;
    for value in [1, entries.len() as u32] {
        append(out, &value.to_le_bytes(), bound, job)?;
    }
    for &(index, ink) in entries {
        append(out, &(index as u32).to_le_bytes(), bound, job)?;
        for v in ink.lab() {
            append(out, &v.to_le_bytes(), bound, job)?;
        }
        append(out, &[ink.solidity()], bound, job)?;
    }
    Ok(())
}

pub(crate) fn parse(
    reader: &mut Reader<'_>,
    channels: usize,
    job: JobContext<'_>,
) -> Result<Vec<Entry>, String> {
    if reader.take(4)? != b"SPOT" || reader.u32()? != 1 {
        return Err("unsupported native spot channel schema".into());
    }
    let count = reader.u32()? as usize;
    if count == 0 || count > channels {
        return Err("invalid native spot channel count".into());
    }
    let mut entries: Vec<Entry> = Vec::with_capacity(count);
    for _ in 0..count {
        job.check()?;
        let index = reader.u32()? as usize;
        if index >= channels
            || entries
                .last()
                .is_some_and(|&(previous, _)| previous >= index)
        {
            return Err("invalid native spot channel order or index".into());
        }
        let lab = [reader.f64()?, reader.f64()?, reader.f64()?];
        let ink = SpotInk::new(lab, reader.u8()?)
            .map_err(|_| "invalid native spot channel ink".to_string())?;
        entries.push((index, ink));
    }
    Ok(entries)
}
