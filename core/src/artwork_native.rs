//! Typed v20 follows the v13 record body, then a u32 embedded-source schema
//! (13/15/17/19), its existing optional RSRC table, and `ADSC`, u32 count,
//! repeated u32 postorder raster index + u32 UTF-8 length + description bytes.
//! Description slots are bounded at 64 KiB, or at the 16 MiB meta cap when the file
//! claims `.rz` word 102 (`large_description_native`), and all references
//! validate before PNG.
//!
//! Validate all records before pixel decode. Description-free files retain older compatible
//! writers. Raw raster replacement, geometry and Assign invalidate descriptions; only the host can
//! restore a corrected description after a native rerender. Shared description allocations count in
//! resource and serialized-capacity plans.
use crate::artwork_document::validate;
use crate::drdc::Reader;
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_native::append;
use std::sync::Arc;

pub(crate) fn header_bytes(doc: &PrecisionDocument) -> u64 {
    let mut bytes = 0;
    for i in 0..doc.layer_count() {
        if let Some(s) = doc.artwork_description(i) {
            bytes += 8 + s.len() as u64;
        }
    }
    if bytes == 0 {
        0
    } else {
        12 + bytes
    }
}
pub(crate) fn encode(
    doc: &PrecisionDocument,
    out: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    let count = (0..doc.layer_count())
        .filter(|&i| doc.artwork_description(i).is_some())
        .count();
    append(out, b"ADSC", bound, job)?;
    append(out, &(count as u32).to_le_bytes(), bound, job)?;
    for index in 0..doc.layer_count() {
        if let Some(text) = doc.artwork_description(index) {
            validate(text)?;
            append(out, &(index as u32).to_le_bytes(), bound, job)?;
            append(out, &(text.len() as u32).to_le_bytes(), bound, job)?;
            append(out, text.as_bytes(), bound, job)?;
        }
    }
    Ok(())
}
/// The ADSC table, each entry at most `cap` bytes (`MAX_DESCRIPTION`, or
/// `MAX_LARGE_DESCRIPTION` under word 102).
pub(crate) fn parse<'a>(
    reader: &mut Reader<'a>,
    records: usize,
    cap: usize,
) -> Result<Vec<(usize, &'a str)>, String> {
    if reader.take(4)? != b"ADSC" {
        return Err("missing typed artwork description table".into());
    }
    let count = reader.u32()? as usize;
    if count > records {
        return Err("typed artwork description count exceeds layer count".into());
    }
    let mut out = Vec::with_capacity(count);
    let mut previous = None;
    for _ in 0..count {
        let index = reader.u32()? as usize;
        let size = reader.u32()? as usize;
        if index >= records || previous.is_some_and(|old| index <= old) || size > cap {
            return Err("invalid typed artwork description reference or size".into());
        }
        let text = std::str::from_utf8(reader.take(size)?)
            .map_err(|_| "typed artwork description is not UTF-8")?;
        out.push((index, text));
        previous = Some(index);
    }
    Ok(out)
}
pub(crate) fn attach(
    layers: &mut [PrecisionDocumentLayer],
    descriptions: &[(usize, &str)],
    index: &mut usize,
) -> Result<(), String> {
    for layer in layers {
        if let Content::Group(children) | Content::PassThrough(children) = &mut layer.content {
            attach(Arc::make_mut(children).as_mut_slice(), descriptions, index)?;
        }
        if let Ok(found) = descriptions.binary_search_by_key(index, |(i, _)| *i) {
            if (layer.source().is_none() && layer.cmyk_source().is_none())
                || layer.retained_source.is_some()
            {
                return Err("artwork description requires an ordinary raster fallback".into());
            }
            layer.description = Some(Arc::new(descriptions[found].1.to_owned()));
        }
        *index += 1;
    }
    Ok(())
}
