//! Conditional outside-mask wrappers. DRDC28 (byte) /29 (typed) inserts its
//! u32 version before the exact prior version/body. After every prior tail:
//! `MOUT`, u32 schema=1, u32 count, strictly ascending u32 record indexes.
//! Each entry means raw outside coverage255; absent entries mean0. Every
//! referenced record must own a mask. Empty/noncanonical tables refuse before
//! decoded pixels allocate. Default-only documents keep their exact old writer.
//!
//! Conditional byte28/typed29 MOUT wrappers retain outside255 after compatible prior tails,
//! preflight before pixels and restore exact prior writers for default-only masks.
use crate::drdc::Reader;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_native::append;

pub const BYTE_VERSION: u32 = 28;
pub const PRECISION_VERSION: u32 = 29;

pub(crate) fn byte_indices(document: &crate::doc::DrDocument) -> Vec<usize> {
    document
        .layers
        .iter()
        .enumerate()
        .filter_map(|(i, layer)| {
            (layer.mask.is_some() && layer.mask_properties.outside_coverage == 255).then_some(i)
        })
        .collect()
}
pub(crate) fn precision_indices(document: &PrecisionDocument) -> Vec<usize> {
    (0..document.layer_count())
        .filter(|&i| {
            document
                .layer(i)
                .and_then(|l| l.mask())
                .is_some_and(|m| m.properties().outside_coverage == 255)
        })
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
    append(out, b"MOUT", bound, job)?;
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
    if reader.take(4)? != b"MOUT" || reader.u32()? != 1 {
        return Err("unsupported native mask outside schema".into());
    }
    let count = reader.u32()? as usize;
    if count == 0 || count > records {
        return Err("invalid native mask outside count".into());
    }
    let mut indices = Vec::with_capacity(count);
    for _ in 0..count {
        job.check()?;
        let index = reader.u32()? as usize;
        if index >= records || indices.last().is_some_and(|&previous| previous >= index) {
            return Err("invalid native mask outside reference order or index".into());
        }
        indices.push(index);
    }
    Ok(indices)
}
