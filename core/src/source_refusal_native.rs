//! Conditional `RSPR` table: the profile-refusal sentence the core's
//! open reported for a placed or replaced ordinary source, kept on its
//! immutable [`SourceAsset`] so it survives a save at both depths.
//!
//! Byte word 56 / typed word 57 is the FIRST word inside the identity wrapper
//! (45/46), ahead of every other conditional word:
//! `45 -> 56 -> 53 -> 51 -> 49 -> …` and `46 -> 57 -> 55 -> 54 -> 52 -> 47 -> …`.
//! Tables close in reverse, so `RSPR` is the last one before `LIDS`:
//! byte `… RSPJ -> WARP -> RSPR -> LIDS`, typed `… RSPJ -> WARP -> LOPN ->
//! RSPR -> LIDS`.
//!
//! Wire format: `RSPR`, u32 schema=1, u32 count, then per entry a u32 asset
//! index (the positional RSRC asset order, strictly ascending), a u32 byte
//! length and that many UTF-8 bytes (1…4096, no NUL). The reader refuses a
//! count of 0 or past the asset count, an index out of range, duplicated or
//! unsorted, a length of 0 or over the cap, and text that is not UTF-8 or
//! holds a NUL — all before any fallback PNG decodes, because it only attaches
//! borrowed text to the pending assets. A word with no RSRC table (a
//! source-free body) refuses through the count check against zero assets.
//!
//! No existing record could carry the sentence: `ATCH` holds one generic
//! slot per asset and that slot is the host-media/RAW-mask contract, `ADSC`
//! excludes source bindings, a byte source's layer `meta` must be empty,
//! the RAW descriptor is RAW-only, `LFLT` is per-binding filter state and
//! the RSRC asset framing is fixed. A document none of whose assets carries a
//! refusal writes neither the word nor the table, so its bytes are exactly
//! the prior writer's. An older BYTE build refuses a 56 file by name
//! ("unsupported DRDC version 56"); an older TYPED build refuses a `46, 57`
//! file with its generic "not a supported … high-depth DRDC document"
//! sentence.
//!
//! conditional byte word 56 / typed word 57, the FIRST word inside identity 45/46, and its `RSPR`
//! table (u32 schema 1, u32 count, then ascending u32 RSRC asset index, u32 length, 1–4096 UTF-8
//! bytes without NUL) as the last table before `LIDS`. `parse` attaches borrowed text to
//! `PendingAsset.profile_refusal` before any PNG decodes and refuses a bad
//! tag/schema/count/index/length/text; the writer re-checks the same caps, and the typed plan
//! forecasts `encoded_len` + 4. A refusal-free file is pinned against stored 53/55-writer files
//! (`tests/fixtures/rz_byte_53/`, `rz_typed_55/`).

use std::sync::Arc;

use crate::drdc::Reader;
use crate::native_sources::PendingSources;
use crate::source_asset::{validate_profile_refusal, SourceAsset, MAX_PROFILE_REFUSAL_BYTES};

pub(crate) const BYTE_VERSION: u32 = 56;
pub(crate) const PRECISION_VERSION: u32 = 57;

/// `(asset index, sentence)` for every asset that carries a refusal, in
/// ascending asset index — the order the writer's RSRC table lists them.
pub(crate) fn entries(assets: &[Arc<SourceAsset>]) -> Vec<(u32, &str)> {
    assets
        .iter()
        .enumerate()
        .filter_map(|(index, asset)| {
            asset
                .profile_refusal
                .as_deref()
                .map(|text| (index as u32, text))
        })
        .collect()
}

/// The table's encoded size; 0 when there is nothing to write.
pub(crate) fn encoded_len(entries: &[(u32, &str)]) -> u64 {
    if entries.is_empty() {
        0
    } else {
        12 + entries
            .iter()
            .map(|(_, text)| 8 + text.len() as u64)
            .sum::<u64>()
    }
}

/// The table, or an empty Vec when there is nothing to write. It re-checks
/// the reader's caps so the writer never emits a file its reader refuses.
pub(crate) fn encode(entries: &[(u32, &str)]) -> Result<Vec<u8>, String> {
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let count = u32::try_from(entries.len()).map_err(|_| "too many source profile refusals")?;
    let mut output = Vec::with_capacity(encoded_len(entries) as usize);
    output.extend_from_slice(b"RSPR");
    output.extend_from_slice(&1_u32.to_le_bytes());
    output.extend_from_slice(&count.to_le_bytes());
    let mut previous = None;
    for &(index, text) in entries {
        if previous.is_some_and(|last| index <= last) {
            return Err("source profile refusal indices must be unique and sorted".into());
        }
        validate_profile_refusal(text)?;
        output.extend_from_slice(&index.to_le_bytes());
        output.extend_from_slice(&(text.len() as u32).to_le_bytes());
        output.extend_from_slice(text.as_bytes());
        previous = Some(index);
    }
    Ok(output)
}

/// Borrow each sentence onto its pending asset. Nothing here allocates
/// beyond the reader's own slices, and no pixel has been decoded yet.
pub(crate) fn parse<'a>(
    reader: &mut Reader<'a>,
    sources: &mut PendingSources<'a>,
) -> Result<(), String> {
    if reader.take(4)? != b"RSPR" || reader.u32()? != 1 {
        return Err("invalid source profile refusal table tag or schema".into());
    }
    let count = reader.u32()? as usize;
    if count == 0 || count > sources.assets.len() {
        return Err("invalid source profile refusal table count".into());
    }
    let mut last = None;
    for _ in 0..count {
        let index = reader.u32()? as usize;
        if index >= sources.assets.len() || last.is_some_and(|prior| index <= prior) {
            return Err("invalid or duplicate source profile refusal asset".into());
        }
        let length = reader.u32()? as usize;
        if length == 0 || length > MAX_PROFILE_REFUSAL_BYTES {
            return Err("source profile refusal length is out of range".into());
        }
        let text = std::str::from_utf8(reader.take(length)?)
            .map_err(|_| "source profile refusal is not UTF-8")?;
        validate_profile_refusal(text)?;
        sources.assets[index].profile_refusal = Some(text);
        last = Some(index);
    }
    Ok(())
}
