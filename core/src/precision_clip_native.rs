//! Conditional typed clipping wrapper. DRDC32 inserts its u32 version before
//! the exact prior version/body (mask29, a blend version or a body version).
//! After every prior tail, including `MOUT`: `LCLP`, u32 schema=1, u32 count,
//! strictly ascending u32 postorder record indexes. Each entry means the record
//! is a clipping member (`clipped = true`); absent entries mean false. Any
//! record kind may carry the flag; a baseless flag is stored, as in byte
//! documents. Empty/noncanonical tables refuse before decoded pixels allocate.
//! Documents without a flag keep their exact prior writer.
//!
//! the typed `clipped` flag's conditional native v32 wrapper: the outermost version word, then the
//! `LCLP` schema1 table of ascending postorder indices after MOUT (`32 → 29 → 31 → body/tails →
//! LBLD → MOUT → LCLP`). Any record kind may carry the flag; empty/noncanonical tables refuse
//! before PNG decode; releasing the last flag restores the prior writer's exact bytes. Clip units
//! render in `precision_render.rs` through `PrecisionLayer::with_clipped_members` with the byte
//! `composite_clip_group_into` rule, derived per level in `precision_document::render_layers` from
//! `clipped` alone.
use crate::drdc::Reader;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_native::append;

pub const PRECISION_VERSION: u32 = 32;

/// Postorder indices of every clipped record, ascending.
pub(crate) fn precision_indices(document: &PrecisionDocument) -> Vec<usize> {
    (0..document.layer_count())
        .filter(|&i| document.layer(i).is_some_and(|l| l.clipped()))
        .collect()
}
pub(crate) fn header_bytes(count: usize) -> u64 {
    if count == 0 {
        0
    } else {
        16 + count as u64 * 4
    }
}
pub(crate) fn encode(
    indices: &[usize],
    out: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    if indices.is_empty() {
        return Ok(());
    }
    append(out, b"LCLP", bound, job)?;
    for value in [1, indices.len() as u32] {
        append(out, &value.to_le_bytes(), bound, job)?;
    }
    for &index in indices {
        append(out, &(index as u32).to_le_bytes(), bound, job)?;
    }
    Ok(())
}
pub(crate) fn parse(
    reader: &mut Reader<'_>,
    records: usize,
    job: JobContext<'_>,
) -> Result<Vec<usize>, String> {
    if reader.take(4)? != b"LCLP" || reader.u32()? != 1 {
        return Err("unsupported native clipping schema".into());
    }
    let count = reader.u32()? as usize;
    if count == 0 || count > records {
        return Err("invalid native clipping count".into());
    }
    let mut indices = Vec::with_capacity(count);
    for _ in 0..count {
        job.check()?;
        let index = reader.u32()? as usize;
        if index >= records || indices.last().is_some_and(|&previous| previous >= index) {
            return Err("invalid native clipping reference order or index".into());
        }
        indices.push(index);
    }
    Ok(indices)
}
