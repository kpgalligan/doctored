//! The conditional BACKGROUND MARK word and table (`doc_background`): which
//! layer the document's mark names, and its role. `spot_channel_native.rs`
//! is the shape this follows.
//!
//! Word 113 is one number at both depths: in a byte file the FIRST word
//! inside 45 (ahead of 112), in a typed file the word right after the CMYK
//! slot (ahead of 112). Its table, `LBKG`, is u32 schema 1, u32 record index,
//! u32 role (1 Background, 2 Ordinary): in a byte file after `SPOT`, before
//! LIDS; in a typed file after `SPOT`, before the CMYK table. The byte index
//! is the bottom-first layer index, the typed one the postorder record index
//! (`LREF`'s).
//!
//! Both are written ONLY when the mark names a layer the document still
//! holds, so every other document is byte-for-byte the prior writer's file
//! and a mark whose layer is gone is dropped here. A pre-113 reader refuses
//! the word by its number (`DRDC_VERSION` stays 89). A bad schema, index or
//! role refuses before any pixel decodes, as every other conditional table
//! does; the reader binds the entry to the record's final id once LIDS is
//! read.
use crate::doc::DrDocument;
use crate::doc_background::BackgroundRole;
use crate::drdc::Reader;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_native::append;

/// The conditional `.doctored` word at both depths (module doc).
pub(crate) const WORD: u32 = 113;

/// The one table entry: a record index and the role.
pub(crate) type Entry = (usize, BackgroundRole);

/// The byte document's entry: the bottom-first index of the marked layer.
pub(crate) fn byte_entry(document: &DrDocument) -> Option<Entry> {
    let mark = document.background?;
    let index = document.layers.iter().position(|l| l.id == mark.layer)?;
    Some((index, mark.role))
}

/// The typed document's entry: the postorder record index of the marked
/// layer.
pub(crate) fn precision_entry(document: &PrecisionDocument) -> Option<Entry> {
    let mark = document.background?;
    let index = document.layer_index_by_id(mark.layer)?;
    Some((index, mark.role))
}

/// The word and the table's size together (4 + 16), or 0 when neither is
/// written.
pub(crate) fn header_bytes(entry: Option<Entry>) -> u64 {
    if entry.is_some() {
        20
    } else {
        0
    }
}

pub(crate) fn encode(
    entry: Option<Entry>,
    out: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    let Some((index, role)) = entry else {
        return Ok(());
    };
    append(out, b"LBKG", bound, job)?;
    for value in [1, index as u32, role.code()] {
        append(out, &value.to_le_bytes(), bound, job)?;
    }
    Ok(())
}

/// The table's entry; `records` is the file's record count.
pub(crate) fn parse(reader: &mut Reader<'_>, records: usize) -> Result<Entry, String> {
    if reader.take(4)? != b"LBKG" || reader.u32()? != 1 {
        return Err("unsupported native background mark schema".into());
    }
    let index = reader.u32()? as usize;
    if index >= records {
        return Err("invalid native background mark index".into());
    }
    let role =
        BackgroundRole::from_code(reader.u32()?).ok_or("invalid native background mark role")?;
    Ok((index, role))
}
