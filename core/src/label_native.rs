//! Conditional layer-label wrappers. DRDC41 (byte) /42 (typed) is the
//! OUTERMOST version word — it inserts its u32 before the exact prior
//! version/body, 38/40 included, so every earlier optional feature keeps its
//! own framing — and appends one table after every prior tail: `LLBL`, u32
//! schema=1, u32 count, then count pairs of u32 record index (strictly
//! ascending) and u32 label code (1..=7). `evaluation_native.rs` is the shape
//! this follows.
//!
//! The byte index is the bottom-first layer index (`MOUT`'s convention); the
//! typed index is the postorder record index (`LCLP`'s). The table is written
//! ONLY when some entry carries a label: a document with none is byte-for-byte
//! the file the prior writer made, first word included. Code 0 is therefore
//! never written and is refused on read — an entry in the one table that
//! exists to say "labelled" cannot say "unlabelled". An empty or noncanonical
//! table, a code above 7 and a trailing word all refuse BEFORE a single PNG is
//! decoded, as every other conditional table does.
//!
//! `label_native` is the outermost conditional `.rz` wrapper — byte 41 / typed 42 before every
//! prior version word, 38/40 included, and one `LLBL` table after every prior tail (`u32` schema 1,
//! `u32` count, then count pairs of ascending `u32` index and `u32` code 1…7) — following
//! `evaluation_native.rs`'s shape. The byte index is the bottom-first layer index (`MOUT`'s
//! convention), the typed one the postorder record index (`LCLP`'s). Written ONLY when some entry
//! carries a label, so an unlabelled document's bytes are exactly the prior writer's and releasing
//! the last label restores them; code 0, an empty or noncanonical table, a code above 7 and a
//! trailing word refuse before any PNG decodes.
use crate::doc::DrDocument;
use crate::doc_label::MAX_LABEL;
use crate::drdc::Reader;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_native::append;

pub const BYTE_VERSION: u32 = 41;
pub const PRECISION_VERSION: u32 = 42;

/// Bottom-first (index, code) pairs for every labelled byte entry, ascending.
pub(crate) fn byte_entries(document: &DrDocument) -> Vec<(usize, u8)> {
    document
        .layers
        .iter()
        .enumerate()
        .filter(|(_, layer)| layer.label != 0)
        .map(|(i, layer)| (i, layer.label))
        .collect()
}

/// Postorder (index, code) pairs for every labelled typed record, ascending.
pub(crate) fn precision_entries(document: &PrecisionDocument) -> Vec<(usize, u8)> {
    (0..document.layer_count())
        .filter_map(|i| {
            let label = document.layer(i).map_or(0, |l| l.label());
            (label != 0).then_some((i, label))
        })
        .collect()
}

/// The table's own size, or 0 when it is not written.
pub(crate) fn header_bytes(count: usize) -> u64 {
    if count == 0 {
        0
    } else {
        16 + count as u64 * 8
    }
}

pub(crate) fn encode(
    entries: &[(usize, u8)],
    out: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    if entries.is_empty() {
        return Ok(());
    }
    append(out, b"LLBL", bound, job)?;
    for value in [1, entries.len() as u32] {
        append(out, &value.to_le_bytes(), bound, job)?;
    }
    for &(index, label) in entries {
        append(out, &(index as u32).to_le_bytes(), bound, job)?;
        append(out, &u32::from(label).to_le_bytes(), bound, job)?;
    }
    Ok(())
}

pub(crate) fn parse(
    reader: &mut Reader<'_>,
    records: usize,
    job: JobContext<'_>,
) -> Result<Vec<(usize, u8)>, String> {
    if reader.take(4)? != b"LLBL" || reader.u32()? != 1 {
        return Err("unsupported native layer label schema".into());
    }
    let count = reader.u32()? as usize;
    if count == 0 || count > records {
        return Err("invalid native layer label count".into());
    }
    let mut entries: Vec<(usize, u8)> = Vec::with_capacity(count);
    for _ in 0..count {
        job.check()?;
        let index = reader.u32()? as usize;
        if index >= records
            || entries
                .last()
                .is_some_and(|&(previous, _)| previous >= index)
        {
            return Err("invalid native layer label reference order or index".into());
        }
        let code = reader.u32()?;
        if code == 0 || code > u32::from(MAX_LABEL) {
            return Err("invalid native layer label code".into());
        }
        entries.push((index, code as u8));
    }
    Ok(entries)
}
