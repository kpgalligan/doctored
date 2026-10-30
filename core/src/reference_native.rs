//! The conditional layer REFERENCE POINT word and table: Photoshop's
//! `fxrp`, each layer's point moved by its translations and mapped by Image
//! Size, Canvas Size, Crop, the angled crop, turns, flips and Free Transform
//! (`reference_point`; `Layer::reference`,
//! the typed `PrecisionDocumentLayer::reference`), which
//! is where a linked Pattern Overlay set without an anchor anchors
//! (`style_pattern`'s Placement). `label_native.rs` is the shape this follows.
//!
//! Word 91 is one number at both depths: in a byte file the FIRST word inside
//! 45 (ahead of 90), in a typed file the word right after the CMYK slot
//! (ahead of 90). Its table, `LREF`, is u32 schema 1, u32 count (1..=records),
//! then count × (u32 record index strictly ascending, i32 x, i32 y), never
//! (0, 0): in a byte file the last table before LIDS, in a typed file after
//! every prior tail but CMYK's and LIDS. The byte index is the bottom-first
//! layer index, the typed one the postorder record index (`LLBL`'s).
//!
//! Both are written ONLY when some record's reference is not (0, 0), so a
//! document whose layers never moved is byte-for-byte the prior writer's
//! file. A pre-91 reader refuses the word by its number (`DRDC_VERSION` stays
//! 89, as with word 90). A bad schema, count, order, index or a (0, 0) point
//! refuses before any pixel decodes, as every other conditional table does.
use crate::doc::DrDocument;
use crate::drdc::Reader;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_native::append;

pub const BYTE_VERSION: u32 = 91;
pub const PRECISION_VERSION: u32 = 91;

/// One table entry: a record index and its reference point.
pub(crate) type Entry = (usize, (i32, i32));

/// Bottom-first (index, reference) pairs for every moved byte entry, ascending.
pub(crate) fn byte_entries(document: &DrDocument) -> Vec<Entry> {
    document
        .layers
        .iter()
        .enumerate()
        .filter(|(_, layer)| layer.reference != (0, 0))
        .map(|(i, layer)| (i, layer.reference))
        .collect()
}

/// Postorder (index, reference) pairs for every moved typed record, ascending.
pub(crate) fn precision_entries(document: &PrecisionDocument) -> Vec<Entry> {
    (0..document.layer_count())
        .filter_map(|i| {
            let reference = document.layer(i).map_or((0, 0), |l| l.reference());
            (reference != (0, 0)).then_some((i, reference))
        })
        .collect()
}

/// The word and the table's size together (4 + 12 + 12 · count), or 0 when
/// neither is written.
pub(crate) fn header_bytes(count: usize) -> u64 {
    if count == 0 {
        0
    } else {
        16 + count as u64 * 12
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
    append(out, b"LREF", bound, job)?;
    for value in [1, entries.len() as u32] {
        append(out, &value.to_le_bytes(), bound, job)?;
    }
    for &(index, (x, y)) in entries {
        append(out, &(index as u32).to_le_bytes(), bound, job)?;
        append(out, &x.to_le_bytes(), bound, job)?;
        append(out, &y.to_le_bytes(), bound, job)?;
    }
    Ok(())
}

pub(crate) fn parse(
    reader: &mut Reader<'_>,
    records: usize,
    job: JobContext<'_>,
) -> Result<Vec<Entry>, String> {
    if reader.take(4)? != b"LREF" || reader.u32()? != 1 {
        return Err("unsupported native layer reference schema".into());
    }
    let count = reader.u32()? as usize;
    if count == 0 || count > records {
        return Err("invalid native layer reference count".into());
    }
    let mut entries: Vec<Entry> = Vec::with_capacity(count);
    for _ in 0..count {
        job.check()?;
        let index = reader.u32()? as usize;
        if index >= records
            || entries
                .last()
                .is_some_and(|&(previous, _)| previous >= index)
        {
            return Err("invalid native layer reference order or index".into());
        }
        let point = (reader.i32()?, reader.i32()?);
        if point == (0, 0) {
            return Err("invalid native layer reference point".into());
        }
        entries.push((index, point));
    }
    Ok(entries)
}
