//! The conditional typed LOCKS/LINKS table. DRDC **47** is the one version
//! word that sits INSIDE the identity wrapper: `46 → 47 → 44 → …`, and its
//! `LLNK` table lands after every prior tail but BEFORE `LIDS`
//! (`… → LLBL → LCMP → LLNK → LIDS`).
//!
//! That is deliberate and it is the one place this file departs from the
//! `label_native.rs` shape it otherwise follows. 12E-1 fixed two invariants —
//! 46 is the FIRST word of every typed `.rz` this build writes, and `LIDS` is
//! the LAST table — and the host's only reader of the version word
//! (`RasterDocument+PrecisionMetadata.swift`'s sniffer) together with
//! `tests/common/identity.rs`' identity peel both rest on them. Wrapping 46
//! would have broken both for a feature that needs neither. An older build
//! still refuses a 47 body word by name ("invalid native layer identity body
//! version"), which is the same forward contract every wrapper carries.
//!
//! The table is written ONLY when some record carries a lock or a link, so a
//! document with neither is byte-for-byte the file the 12E-1 writer made —
//! and releasing the last lock restores those bytes exactly. Reopen → resave
//! is byte-identical because the reader adopts what it reads.
//!
//! Framing: `LLNK`, u32 schema = 1, u32 count (which is the RECORD count, not
//! a subset — a pair per record is 8 bytes and the walk stays a slice), then
//! per record u32 locks and u32 link, in the POSTORDER numbering `LLBL`,
//! `LCLP` and `LIDS` use. Locks are MASKED with `doc_lock::LOCK_ALL` on read,
//! exactly as the byte reader and the setter mask them, so a file from a
//! newer build loads with the locks this one understands. A bad tag, a bad
//! schema and a count that is not the record count all refuse BEFORE a single
//! PNG is decoded, as every other conditional table does.
//!
//! The BYTE depth needs no table: `Layer::locks` and `Layer::link` have been
//! in the v7 layer record since long before this.
//!
//! reserved lock bits are MASKED on read, never refused.

use crate::doc_lock::LOCK_ALL;
use crate::drdc::Reader;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_native::append;

pub const PRECISION_VERSION: u32 = 47;

/// `(locks, link)` per postorder record, or an EMPTY vector when every record
/// carries neither — which is what makes the table conditional.
pub(crate) fn precision_entries(document: &PrecisionDocument) -> Vec<(u32, u32)> {
    let entries: Vec<(u32, u32)> = (0..document.layer_count())
        .map(|i| {
            document
                .layer(i)
                .map_or((0, 0), |layer| (layer.locks() & LOCK_ALL, layer.link()))
        })
        .collect();
    if entries.iter().all(|&(locks, link)| locks == 0 && link == 0) {
        return Vec::new();
    }
    entries
}

/// The table's own size, or 0 when it is not written: the version word the
/// writer emits alongside it, then the tag, the schema word and the count
/// word ahead of the pairs.
pub(crate) fn header_bytes(count: usize) -> u64 {
    if count == 0 {
        0
    } else {
        16 + count as u64 * 8
    }
}

pub(crate) fn encode(
    entries: &[(u32, u32)],
    out: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    if entries.is_empty() {
        return Ok(());
    }
    // The writer must never emit a file its own reader refuses, so the one
    // structural claim `parse` makes about a value is checked here too.
    if entries.iter().any(|&(locks, _)| locks & !LOCK_ALL != 0) {
        return Err("native layer lock flags exceed the known bits".into());
    }
    let count = u32::try_from(entries.len()).map_err(|_| "too many native layer locks")?;
    append(out, b"LLNK", bound, job)?;
    for value in [1u32, count] {
        append(out, &value.to_le_bytes(), bound, job)?;
    }
    for &(locks, link) in entries {
        append(out, &locks.to_le_bytes(), bound, job)?;
        append(out, &link.to_le_bytes(), bound, job)?;
    }
    Ok(())
}

pub(crate) fn parse(
    reader: &mut Reader<'_>,
    records: usize,
    job: JobContext<'_>,
) -> Result<Vec<(u32, u32)>, String> {
    if reader.take(4)? != b"LLNK" || reader.u32()? != 1 {
        return Err("unsupported native layer lock schema".into());
    }
    let count = reader.u32()? as usize;
    if count != records {
        return Err("invalid native layer lock count".into());
    }
    let mut entries: Vec<(u32, u32)> = Vec::with_capacity(count);
    for _ in 0..count {
        job.check()?;
        // Masked, never refused: bits 3..31 are reserved, and a file from a
        // newer build loads with the locks this one understands rather than
        // being rejected outright (`doc_lock`'s module doc).
        let locks = reader.u32()? & LOCK_ALL;
        let link = reader.u32()?;
        entries.push((locks, link));
    }
    Ok(entries)
}
