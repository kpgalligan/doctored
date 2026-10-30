//! The PERSISTED LAYER IDENTITY wrapper. DRDC 45 (byte) / 46 (typed) is the
//! OUTERMOST version word — it inserts its u32 before the exact prior
//! version/body, 43/44 included, so every earlier optional feature keeps its
//! own framing — and appends one table after every prior tail: `LIDS`, u32
//! schema = 1, u32 count, then count u64 little-endian ids in index order
//! (byte: the bottom-first layer index; typed: the postorder record index).
//! `comps_native.rs` is the shape this follows.
//!
//! **The table is ALWAYS written**, which is the one place this wrapper
//! departs from every conditional table beside it. An id is a property of
//! EVERY layer, not of a layer that happens to carry a feature, so a
//! conditional table would make identity survive a save only when some
//! unrelated state was set — and a document that merely lost its last colour
//! label would silently lose its ids as well. "A document without the table"
//! therefore means a FILE with no 45/46 word: every such file (every file any
//! earlier build wrote) reads exactly as it did before, with freshly minted
//! ids. Older builds refuse 45/46 by name ("unsupported DRDC version"), which
//! is the same forward contract every other wrapper carries.
//!
//! Reopen → resave stays byte-identical because the reader ADOPTS the file's
//! ids rather than minting over them, so the writer emits the same table it
//! read.
//!
//! Uniqueness is per DOCUMENT: two files may carry the same ids, and nothing
//! here tries to make them globally unique across sessions. What the reader
//! DOES guarantee is that a fresh mint never collides with an id it just
//! read — [`crate::doc_identity::reserve_layer_ids_through`] raises the
//! process counter past the largest id in the file.
//!
//! A bad tag or schema, a count that is not the record count, a zero id (0 is
//! "no layer" across the whole FFI and can never be a live id) and a duplicate
//! id all refuse BEFORE a single PNG is decoded, as every other table does.
//!
//! The stated cost is that every `.rz` this build writes has a new outermost word, so no file is
//! byte-identical to the 43/44 writer's. The reader ADOPTS the ids (reopen → resave is
//! byte-identical) and the WRITER makes the same two structural checks the reader does — no zero,
//! no duplicate — so it never emits a file its own reader refuses. A bad tag or schema, a count
//! that is not the record count, a zero id, a duplicate and a trailing word all refuse before a
//! single PNG decodes. `adopt` assigns typed ids in postorder by cloning each group's
//! `Arc<Vec<_>>`, so every pixel, mask and style stays the allocation the reader built.

use std::sync::Arc;

use crate::doc::DrDocument;
use crate::drdc::Reader;
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_native::append;

pub const BYTE_VERSION: u32 = 45;
pub const PRECISION_VERSION: u32 = 46;

/// Every byte entry's id, in the bottom-first layer order the records are
/// written in.
pub(crate) fn byte_ids(document: &DrDocument) -> Vec<u64> {
    document.layers.iter().map(|layer| layer.id).collect()
}

/// Every typed record's id, in the postorder numbering `PrecisionDocument::layer`
/// indexes — the same order `LLBL` and `LCLP` use.
pub(crate) fn precision_ids(document: &PrecisionDocument) -> Vec<u64> {
    (0..document.layer_count())
        .map(|i| document.layer(i).map_or(0, |layer| layer.id()))
        .collect()
}

/// The table's own size: the outermost version word the writer emits
/// alongside it, then the tag, the schema word and the count word ahead of
/// the ids. Unconditional, because the table is.
pub(crate) fn header_bytes(count: usize) -> u64 {
    16 + 8 * count as u64
}

pub(crate) fn encode(
    ids: &[u64],
    out: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    // The writer must never emit a file its own reader refuses, so the two
    // structural claims [`parse`] makes are checked here too.
    for (index, id) in ids.iter().enumerate() {
        if *id == 0 || ids[..index].contains(id) {
            return Err("layer identities must be unique and non-zero".into());
        }
    }
    let count = u32::try_from(ids.len()).map_err(|_| "too many native layer ids".to_string())?;
    append(out, b"LIDS", bound, job)?;
    for value in [1u32, count] {
        append(out, &value.to_le_bytes(), bound, job)?;
    }
    for id in ids {
        append(out, &id.to_le_bytes(), bound, job)?;
    }
    Ok(())
}

/// Reads the table and raises the process counter past every id it carries,
/// so a layer minted after this open can never collide with one it read.
pub(crate) fn parse(
    reader: &mut Reader<'_>,
    expected: usize,
    job: JobContext<'_>,
) -> Result<Vec<u64>, String> {
    if reader.take(4)? != b"LIDS" || reader.u32()? != 1 {
        return Err("unsupported native layer identity schema".into());
    }
    let count = reader.u32()? as usize;
    if count != expected {
        return Err("invalid native layer identity count".into());
    }
    let mut ids: Vec<u64> = Vec::with_capacity(count);
    let mut highest = 0u64;
    for _ in 0..count {
        job.check()?;
        let id = reader.u64()?;
        // 0 means "no layer" everywhere this crate carries an id, and a
        // repeated id would make `layer_index_by_id` answer the wrong record
        // — both are structural claims, so both refuse rather than repair.
        if id == 0 || ids.contains(&id) {
            return Err("invalid native layer identity value".into());
        }
        highest = highest.max(id);
        ids.push(id);
    }
    crate::doc_identity::reserve_layer_ids_through(highest);
    Ok(ids)
}

/// Gives each typed record the id at its postorder position. The tree is
/// rebuilt by cloning each group's `Arc<Vec<_>>`, assigning inside and
/// re-wrapping, so a record's pixels, masks and style stay the very
/// allocations the reader built.
pub(crate) fn adopt(document: &mut PrecisionDocument, ids: &[u64]) -> Result<(), String> {
    fn assign(
        layers: &mut [PrecisionDocumentLayer],
        ids: &[u64],
        next: &mut usize,
    ) -> Result<(), String> {
        for layer in layers.iter_mut() {
            if let Content::Group(children) | Content::PassThrough(children) = &mut layer.content {
                let mut owned = children.as_ref().clone();
                assign(&mut owned, ids, next)?;
                *children = Arc::new(owned);
            }
            let id = *ids
                .get(*next)
                .ok_or("native layer identity count does not match the records")?;
            layer.id = id;
            *next += 1;
        }
        Ok(())
    }
    let mut layers = document.layers.as_ref().clone();
    let mut next = 0;
    assign(&mut layers, ids, &mut next)?;
    if next != ids.len() {
        return Err("native layer identity count does not match the records".into());
    }
    document.layers = Arc::new(layers);
    Ok(())
}
