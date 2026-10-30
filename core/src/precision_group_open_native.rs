//! The conditional typed GROUP DISCLOSURE table. DRDC **55** sits INSIDE the
//! identity wrapper as its first inner word after 57 when present — `46 →
//! (57) → 55 → 54 → 52 → 47 → 48 → 50 → 44 → …` — and its `LOPN` table is
//! the last one before RSPR or LIDS (`… → LLNK → LGDE → (projective) → WARP
//! → LOPN → (RSPR) → LIDS`).
//!
//! The slot is `precision_lock_native`'s and is taken for the same reason:
//! 12E-1 fixed two invariants — 46 is the FIRST word of every typed `.rz`
//! this build writes, and `LIDS` is the LAST table — and the host's only
//! reader of the version word (`RasterDocument+PrecisionMetadata.swift`'s
//! sniffer) together with `tests/common/identity.rs`' identity peel both rest
//! on them. A disclosure needs neither broken. An older build refuses a 55
//! body word by name, which is the same forward contract every wrapper
//! carries.
//!
//! The disclosure is the byte `Layer::open` twin: panel bookkeeping only,
//! meaningful on groups alone, and saved because the host counts a change
//! when it toggles. Every group opens by default, so the table lists only the
//! CLOSED groups and is written only when there is one: a document whose
//! groups are all open is byte-for-byte the file the 54 writer made, and
//! reopening the last closed group restores those bytes exactly. Reopen →
//! resave is byte-identical because the reader adopts what it reads.
//!
//! Framing: `LOPN`, u32 schema = 1, u32 count ≥ 1, then count u32 indices of
//! the closed groups in the POSTORDER numbering `LLBL`, `LCLP` and `LIDS` use,
//! strictly ascending and below the record count. A bad tag or schema, an
//! empty, unordered or out-of-range list and an index naming a non-group
//! record all refuse BEFORE a single PNG is decoded, as every other
//! conditional table does.
//!
//! The BYTE depth needs no table: `Layer::open` has been in the v7 layer
//! record since groups arrived.
//!
//! `PrecisionDocumentLayer.open` (the byte `Layer::open` twin, panel bookkeeping, `true` on every
//! non-group record), set by the lock-exempt `{op:"layer_open"}` `StructureEdit` (groups only; same
//! state answers NULL) and read as the info row's `open`. Carried by every group rebuild, 8↔16
//! conversion (`group_open_state` is PRESERVED) and 16-bit PSD import/export (divider 1 open / 2
//! closed). Persisted by the conditional word **55**, the first word inside 46 after 58 and 57 when
//! present (`46 → (58) → (57) → 55 → 54 → 52 → 47 → …`), and the `LOPN` table (u32 schema 1, u32
//! count ≥ 1, strictly ascending postorder indices of the CLOSED groups), the last table before
//! RSPR or `LIDS`; a document whose groups are all open writes the 54 writer's bytes exactly;
//! pinned against a stored 54-writer file (`tests/fixtures/rz_typed_54/`). A bad tag/schema,
//! an empty/unordered/out-of-range list or a non-group index refuses by name before any PNG
//! decodes.

use crate::drdc::Reader;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_native::append;

pub const PRECISION_VERSION: u32 = 55;

/// The postorder indices of the CLOSED groups, ascending, or an EMPTY vector
/// when every group is open — which is what makes the table conditional.
pub(crate) fn precision_entries(document: &PrecisionDocument) -> Vec<usize> {
    (0..document.layer_count())
        .filter(|&i| {
            document
                .layer(i)
                .is_some_and(|layer| layer.children().is_some() && !layer.open())
        })
        .collect()
}

/// The table's own size, or 0 when it is not written: the version word the
/// writer emits alongside it, then the tag, the schema word and the count
/// word ahead of the indices.
pub(crate) fn header_bytes(count: usize) -> u64 {
    if count == 0 {
        0
    } else {
        16 + count as u64 * 4
    }
}

pub(crate) fn encode(
    entries: &[usize],
    out: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    if entries.is_empty() {
        return Ok(());
    }
    // The writer must never emit a file its own reader refuses, so the
    // ordering claim `parse` makes is checked here too.
    if entries.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err("invalid native group open table".into());
    }
    let count = u32::try_from(entries.len()).map_err(|_| "too many native group open entries")?;
    append(out, b"LOPN", bound, job)?;
    for value in [1u32, count] {
        append(out, &value.to_le_bytes(), bound, job)?;
    }
    for &index in entries {
        let index = u32::try_from(index).map_err(|_| "invalid native group open table")?;
        append(out, &index.to_le_bytes(), bound, job)?;
    }
    Ok(())
}

/// Reads the closed-group indices. Whether each one names a GROUP is checked
/// where the pending records are attached (`precision_native_read`), which is
/// the only place the record kinds are known.
pub(crate) fn parse(
    reader: &mut Reader<'_>,
    records: usize,
    job: JobContext<'_>,
) -> Result<Vec<usize>, String> {
    if reader.take(4)? != b"LOPN" || reader.u32()? != 1 {
        return Err("unsupported native group open schema".into());
    }
    let count = reader.u32()? as usize;
    if count == 0 || count > records {
        return Err("invalid native group open table".into());
    }
    let mut entries: Vec<usize> = Vec::with_capacity(count);
    for _ in 0..count {
        job.check()?;
        let index = reader.u32()? as usize;
        if index >= records || entries.last().is_some_and(|&last| index <= last) {
            return Err("invalid native group open table".into());
        }
        entries.push(index);
    }
    Ok(entries)
}
