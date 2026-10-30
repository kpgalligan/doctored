//! Conditional typed layer-style wrapper. DRDC36 inserts its u32 version
//! before the exact prior version/body (Blend If35, clipping32, mask29, a
//! blend version or a body version) and is never nested inside any of them.
//! After every prior tail, including `LBIF`: `LSTY`, u32 schema=1, u32 count
//! ≥ 1, then entries of u32 postorder index (strictly ascending), u32 length
//! (1..=`MAX_STYLE_JSON_LEN`) and that many bytes of the style's CANONICAL
//! JSON (`LayerStyle::to_json`: `fill_opacity`, `blend_if` null, the effects
//! with every key). Entries belong to raster or isolated-group records only;
//! a non-canonical body (unknown key, unknown `type`, reordered or shortened
//! keys), a Blend If inside the style, an identity style, an empty or
//! non-ascending table and a plane past `MAX_PIXELS` (checked at the attach
//! walk against the record's declared dimensions) refuse before decoded
//! pixels allocate. Documents without a style keep their exact prior writer.
//!
//! Native v36 is the outermost word, never nested, then the `LSTY` schema1 table after `LBIF` (`36
//! → 35 → 32 → 29 → 34 → body/tails → … → LBIF → LSTY`) of canonical `to_json` bodies (`to_json()
//! == stored` is the frozen whitelist); releasing the last style restores the prior writer's exact
//! bytes.
use std::sync::Arc;

use crate::drdc::Reader;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_native::append;
use crate::style::LayerStyle;

pub const PRECISION_VERSION: u32 = 36;
/// The largest canonical style body an entry may carry: the effects with
/// 32-stop gradients are a few kilobytes, and a Pattern Overlay's custom
/// tile is at most 512² × 4 bytes as base64, 1,398,104 bytes; 2 MiB
/// holds that with every other effect beside it.
pub(crate) const MAX_STYLE_JSON_LEN: usize = 2 * 1024 * 1024;

/// Postorder indices and canonical JSON of every record that carries a
/// style, ascending.
pub(crate) fn precision_entries(document: &PrecisionDocument) -> Vec<(usize, String)> {
    (0..document.layer_count())
        .filter_map(|i| Some((i, document.layer(i)?.style()?.to_json())))
        .collect()
}
pub(crate) fn header_bytes(entries: &[(usize, String)]) -> u64 {
    if entries.is_empty() {
        0
    } else {
        16 + entries.iter().map(|(_, s)| 8 + s.len() as u64).sum::<u64>()
    }
}
pub(crate) fn encode(
    entries: &[(usize, String)],
    out: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    if entries.is_empty() {
        return Ok(());
    }
    append(out, b"LSTY", bound, job)?;
    for value in [1, entries.len() as u32] {
        append(out, &value.to_le_bytes(), bound, job)?;
    }
    for (index, json) in entries {
        if json.is_empty() || json.len() > MAX_STYLE_JSON_LEN {
            return Err("native layer style body exceeds its byte limit".into());
        }
        for value in [*index as u32, json.len() as u32] {
            append(out, &value.to_le_bytes(), bound, job)?;
        }
        append(out, json.as_bytes(), bound, job)?;
    }
    Ok(())
}
pub(crate) fn parse(
    reader: &mut Reader<'_>,
    records: usize,
    job: JobContext<'_>,
) -> Result<Vec<(usize, Arc<LayerStyle>)>, String> {
    if reader.take(4)? != b"LSTY" || reader.u32()? != 1 {
        return Err("unsupported native layer style schema".into());
    }
    let count = reader.u32()? as usize;
    if count == 0 || count > records {
        return Err("invalid native layer style count".into());
    }
    let mut out: Vec<(usize, Arc<LayerStyle>)> = Vec::with_capacity(count);
    for _ in 0..count {
        job.check()?;
        let index = reader.u32()? as usize;
        if index >= records || out.last().is_some_and(|&(previous, _)| previous >= index) {
            return Err("invalid native layer style reference order or index".into());
        }
        let length = reader.u32()? as usize;
        if length == 0 || length > MAX_STYLE_JSON_LEN {
            return Err("invalid native layer style body length".into());
        }
        let bytes = reader.take(length)?;
        let text = std::str::from_utf8(bytes)
            .map_err(|_| "native layer style body is not UTF-8".to_string())?;
        let style = LayerStyle::from_json(text)
            .map_err(|e| format!("invalid native layer style body: {e}"))?;
        if style.blend_if.is_some() {
            return Err("native layer style body carries a Blend If".into());
        }
        if style.is_identity() {
            return Err("invalid native layer style identity entry".into());
        }
        if style.to_json().as_bytes() != bytes {
            return Err("native layer style body is not canonical".into());
        }
        out.push((index, Arc::new(style)));
    }
    Ok(out)
}
