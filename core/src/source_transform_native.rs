//! Conditional RSPJ table for retained projective master mappings. The
//! legacy RSRC transform slot remains affine and is written as identity for
//! these bindings. This table is parsed before any fallback PNG allocation.
//!
//! A projective binding writes conditional byte word 51 or typed word 52 immediately inside
//! identity 45/46, with a strict `RSPJ` table before `LIDS`; affine/source-free files keep their
//! legacy bytes.

use crate::drdc::Reader;
use crate::native_sources::{Binding, PendingSources};
use crate::source_transform::{self, SourceTransform};

pub(crate) const BYTE_VERSION: u32 = 51;
pub(crate) const PRECISION_VERSION: u32 = 52;

pub(crate) fn entries(bindings: &[Binding]) -> Vec<(u32, [f64; 9])> {
    let mut entries: Vec<_> = bindings
        .iter()
        .filter_map(|binding| match binding.transform {
            SourceTransform::Projective(matrix) => Some((binding.layer as u32, matrix)),
            SourceTransform::Affine(_) => None,
        })
        .collect();
    entries.sort_unstable_by_key(|entry| entry.0);
    entries
}

pub(crate) fn encoded_len(count: usize) -> u64 {
    if count == 0 {
        0
    } else {
        12 + count as u64 * 76
    }
}

pub(crate) fn encode(entries: &[(u32, [f64; 9])]) -> Result<Vec<u8>, String> {
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let count = u32::try_from(entries.len()).map_err(|_| "too many projective source bindings")?;
    let mut output = Vec::with_capacity(encoded_len(entries.len()) as usize);
    output.extend_from_slice(b"RSPJ");
    output.extend_from_slice(&1_u32.to_le_bytes());
    output.extend_from_slice(&count.to_le_bytes());
    let mut previous = None;
    for &(index, matrix) in entries {
        if previous.is_some_and(|last| index <= last) {
            return Err("projective source binding indices must be unique and sorted".into());
        }
        SourceTransform::Projective(matrix).inverse()?;
        output.extend_from_slice(&index.to_le_bytes());
        for coefficient in matrix {
            output.extend_from_slice(&coefficient.to_le_bytes());
        }
        previous = Some(index);
    }
    Ok(output)
}

pub(crate) fn parse(
    reader: &mut Reader<'_>,
    sources: &mut PendingSources<'_>,
    records: usize,
) -> Result<(), String> {
    if reader.take(4)? != b"RSPJ" || reader.u32()? != 1 {
        return Err("invalid projective source table tag or schema".into());
    }
    let count = reader.u32()? as usize;
    if count == 0 || count > records || count > sources.bindings.len() {
        return Err("invalid projective source table count".into());
    }
    let mut last = None;
    for _ in 0..count {
        let index = reader.u32()? as usize;
        if index >= records || last.is_some_and(|prior| index <= prior) {
            return Err("invalid or duplicate projective source target".into());
        }
        let binding = sources
            .bindings
            .iter_mut()
            .find(|binding| binding.layer == index)
            .ok_or("projective source target has no retained binding")?;
        if binding.transform != SourceTransform::Affine([1., 0., 0., 1., 0., 0.]) {
            return Err("projective source legacy slot is not identity".into());
        }
        let mut matrix = [0.; 9];
        for coefficient in &mut matrix {
            *coefficient = reader.f64()?;
        }
        source_transform::bounds(
            sources.assets[binding.asset].dimensions,
            SourceTransform::Projective(matrix),
        )?;
        binding.transform = SourceTransform::Projective(matrix);
        last = Some(index);
    }
    Ok(())
}
