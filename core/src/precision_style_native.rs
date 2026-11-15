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
//!
//! An entry whose style holds native pattern samples (`pattern_samples_native`, word 115) may
//! reach `MAX_SAMPLES_STYLE_JSON_LEN`; the reader admits a body past `MAX_STYLE_JSON_LEN` only in
//! a file that claims word 115, and only when the parsed style holds samples, so every body the
//! writer emits reads back and no other body grows.
use std::sync::Arc;

use crate::drdc::Reader;
use crate::job_control::JobContext;
use crate::pattern_samples_native::{holds_samples, MAX_SAMPLES_STYLE_JSON_LEN};
use crate::precision_document::PrecisionDocument;
use crate::precision_native::append;
use crate::style::LayerStyle;

pub const PRECISION_VERSION: u32 = 36;
/// The largest canonical style body an entry may carry: the effects with
/// 32-stop gradients are a few kilobytes, and a Pattern Overlay's custom
/// tile is at most 512² × 4 bytes as base64, 1,398,104 bytes; 2 MiB
/// holds that with every other effect beside it. A style holding native
/// pattern samples takes [`MAX_SAMPLES_STYLE_JSON_LEN`] instead (module doc).
pub(crate) const MAX_STYLE_JSON_LEN: usize = 2 * 1024 * 1024;

/// The cap `style`'s canonical body is held to (module doc).
pub(crate) fn body_cap(style: &LayerStyle) -> usize {
    if holds_samples(style) {
        MAX_SAMPLES_STYLE_JSON_LEN
    } else {
        MAX_STYLE_JSON_LEN
    }
}

/// Postorder indices, canonical JSON and body cap ([`body_cap`]) of every
/// record that carries a style, ascending.
pub(crate) fn precision_entries(document: &PrecisionDocument) -> Vec<(usize, String, usize)> {
    (0..document.layer_count())
        .filter_map(|i| {
            let style = document.layer(i)?.style()?;
            Some((i, style.to_json(), body_cap(style)))
        })
        .collect()
}
pub(crate) fn header_bytes(entries: &[(usize, String, usize)]) -> u64 {
    if entries.is_empty() {
        0
    } else {
        16 + entries
            .iter()
            .map(|(_, s, _)| 8 + s.len() as u64)
            .sum::<u64>()
    }
}
pub(crate) fn encode(
    entries: &[(usize, String, usize)],
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
    for (index, json, cap) in entries {
        if json.is_empty() || json.len() > *cap {
            return Err("native layer style body exceeds its byte limit".into());
        }
        for value in [*index as u32, json.len() as u32] {
            append(out, &value.to_le_bytes(), bound, job)?;
        }
        append(out, json.as_bytes(), bound, job)?;
    }
    Ok(())
}
/// The `LSTY` table of a file with `records` records; `samples` is whether
/// the file claims word 115 (module doc).
pub(crate) fn parse(
    reader: &mut Reader<'_>,
    records: usize,
    samples: bool,
    job: JobContext<'_>,
) -> Result<Vec<(usize, Arc<LayerStyle>)>, String> {
    let cap = if samples {
        MAX_SAMPLES_STYLE_JSON_LEN
    } else {
        MAX_STYLE_JSON_LEN
    };
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
        if length == 0 || length > cap {
            return Err("invalid native layer style body length".into());
        }
        let bytes = reader.take(length)?;
        let text = std::str::from_utf8(bytes)
            .map_err(|_| "native layer style body is not UTF-8".to_string())?;
        let style = LayerStyle::from_json(text)
            .map_err(|e| format!("invalid native layer style body: {e}"))?;
        if length > body_cap(&style) {
            return Err("invalid native layer style body length".into());
        }
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
