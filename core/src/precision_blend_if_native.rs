//! Conditional typed Blend If wrapper. DRDC35 inserts its u32 version before
//! the exact prior version/body (clipping32, mask29, a blend version or a body
//! version) and is never nested inside any of them. After every prior tail,
//! including `LCLP`: `LBIF`, u32 schema=1, u32 count, then entries of u32
//! postorder index (strictly ascending), u32 channel (0 gray, 1 red, 2 green,
//! 3 blue; a CMYK document's 4 cyan, 5 magenta, 6 yellow, 7 black, #1 —
//! refused in any other document), `[u8; 4]` this-layer ramp and `[u8; 4]` underlying ramp. Entries
//! belong to raster or isolated-group records only; an identity entry, an
//! unordered ramp, an unknown channel and an empty/noncanonical table refuse
//! before decoded pixels allocate. Documents without a Blend If keep their
//! exact prior writer.
//!
//! SCHEMA 2 (written iff some entry holds more than one range;
//! otherwise schema 1, byte-identical): `LBIF`, u32 2, u32 count, entries of
//! u32 postorder index (strictly ascending), u32 n (1..=8), then n × (u32
//! channel code as in schema 1, `[u8; 4]` this-layer ramp, `[u8; 4]`
//! underlying ramp), channels strictly ascending, no identity range. It is
//! claimed by word 101 (`blend_if_channels_native`): schema 2 without the
//! word is the schema refusal ending in that word's refusal, a schema 2 table with no entry of n ≥ 2 is
//! "noncanonical native blend-if schema", an n out of range "invalid native
//! blend-if range count", a channel out of order "invalid native blend-if
//! channel order", and an identity range the schema 1 identity refusal —
//! every one before any PNG decodes.
//!
//! the conditional native v35 wrapper — the outermost version word, never nested, then the `LBIF`
//! schema1 table after `LCLP` (`35 → 32 → 29 → 34 → body/tails → LBLD6 → MOUT → LCLP → LBIF`);
//! empty/noncanonical/identity entries refuse before PNG decode (every record kind may hold one,
//! a Pass Through group included); releasing the last Blend If restores the prior writer's exact bytes.
use crate::drdc::Reader;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_native::append;
use crate::style::{BlendIf, BlendIfChannel, BlendIfRange, MAX_EXTRA_RANGES};

pub const PRECISION_VERSION: u32 = 35;

/// The channel codes: 0 gray, 1 red, 2 green, 3 blue and (#1, a CMYK
/// document's plates, under DRDC 86) 4 cyan, 5 magenta, 6 yellow, 7 black.
const CHANNELS: [BlendIfChannel; 8] = [
    BlendIfChannel::Gray,
    BlendIfChannel::Red,
    BlendIfChannel::Green,
    BlendIfChannel::Blue,
    BlendIfChannel::Cyan,
    BlendIfChannel::Magenta,
    BlendIfChannel::Yellow,
    BlendIfChannel::Black,
];

/// Postorder indices and Blend If of every record that carries one, ascending.
pub(crate) fn precision_entries(document: &PrecisionDocument) -> Vec<(usize, BlendIf)> {
    (0..document.layer_count())
        .filter_map(|i| Some((i, document.layer(i)?.blend_if()?)))
        .collect()
}
/// The `LBIF` table's bound: schema 1 (`multichannel` false) is 16 bytes of
/// header plus 16 an entry; schema 2 bounds an entry at its index, its range
/// count and eight ranges of 12 bytes (`16 + count·(8 + 12·8)`).
pub(crate) fn header_bytes(count: usize, multichannel: bool) -> u64 {
    if count == 0 {
        0
    } else if multichannel {
        16 + count as u64 * (8 + 12 * MAX_RANGES as u64)
    } else {
        16 + count as u64 * 16
    }
}

/// The most ranges one schema 2 entry holds: one per channel.
const MAX_RANGES: usize = MAX_EXTRA_RANGES + 1;

/// Whether some entry needs schema 2 (more than one range).
pub(crate) fn needs_schema2(entries: &[(usize, BlendIf)]) -> bool {
    entries.iter().any(|(_, b)| b.is_multichannel())
}

fn channel_code(channel: BlendIfChannel) -> Result<u32, String> {
    Ok(CHANNELS
        .iter()
        .position(|&c| c == channel)
        .ok_or("unknown native blend-if channel")? as u32)
}

/// Schema 1 when every entry holds one range (the prior writer's exact
/// bytes), else schema 2 (module doc). The writer enforces the reader's
/// rules: a schema 2 entry's ranges are canonical (channels strictly
/// ascending, none identity), so every table it writes reads back.
pub(crate) fn encode(
    entries: &[(usize, BlendIf)],
    out: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    if entries.is_empty() {
        return Ok(());
    }
    let schema2 = needs_schema2(entries);
    append(out, b"LBIF", bound, job)?;
    for value in [if schema2 { 2 } else { 1 }, entries.len() as u32] {
        append(out, &value.to_le_bytes(), bound, job)?;
    }
    for &(index, blend_if) in entries {
        if !schema2 {
            let channel = channel_code(blend_if.channel)?;
            for value in [index as u32, channel] {
                append(out, &value.to_le_bytes(), bound, job)?;
            }
            append(out, &blend_if.this_layer, bound, job)?;
            append(out, &blend_if.underlying, bound, job)?;
            continue;
        }
        let ranges: Vec<BlendIfRange> = blend_if.ranges().collect();
        let canonical = ranges.windows(2).all(|w| w[0].channel < w[1].channel)
            && ranges.iter().all(|r| !r.is_identity());
        if !canonical {
            return Err("noncanonical native blend-if entry".into());
        }
        for value in [index as u32, ranges.len() as u32] {
            append(out, &value.to_le_bytes(), bound, job)?;
        }
        for range in ranges {
            append(out, &channel_code(range.channel)?.to_le_bytes(), bound, job)?;
            append(out, &range.this_layer, bound, job)?;
            append(out, &range.underlying, bound, job)?;
        }
    }
    Ok(())
}
fn ordered(ramp: [u8; 4]) -> bool {
    ramp[0] <= ramp[1] && ramp[1] <= ramp[2] && ramp[2] <= ramp[3]
}

/// One range's channel code and two ramps.
fn read_range(reader: &mut Reader<'_>) -> Result<BlendIfRange, String> {
    let channel = *CHANNELS
        .get(reader.u32()? as usize)
        .ok_or("unsupported native blend-if channel")?;
    let mut ramps = [[0u8; 4]; 2];
    for ramp in &mut ramps {
        ramp.copy_from_slice(reader.take(4)?);
        if !ordered(*ramp) {
            return Err("invalid native blend-if ramp order".into());
        }
    }
    Ok(BlendIfRange {
        channel,
        this_layer: ramps[0],
        underlying: ramps[1],
    })
}

/// The `LBIF` table. Schema 2 needs word 101 (`multichannel`,
/// `blend_if_channels_native`); every refusal lands before any PNG decodes
/// (module doc).
pub(crate) fn parse(
    reader: &mut Reader<'_>,
    records: usize,
    multichannel: bool,
    job: JobContext<'_>,
) -> Result<Vec<(usize, BlendIf)>, String> {
    if reader.take(4)? != b"LBIF" {
        return Err("unsupported native blend-if schema".into());
    }
    let schema2 = match reader.u32()? {
        1 => false,
        2 if multichannel => true,
        // Worded as the schema refusal an older reader gives too, ending in
        // the word's own refusal.
        2 => {
            return Err(format!(
                "unsupported native blend-if schema: {}",
                crate::blend_if_channels_native::WORD_REFUSAL
            ))
        }
        _ => return Err("unsupported native blend-if schema".into()),
    };
    let count = reader.u32()? as usize;
    if count == 0 || count > records {
        return Err("invalid native blend-if count".into());
    }
    let mut out: Vec<(usize, BlendIf)> = Vec::with_capacity(count);
    for _ in 0..count {
        job.check()?;
        let index = reader.u32()? as usize;
        if index >= records || out.last().is_some_and(|&(previous, _)| previous >= index) {
            return Err("invalid native blend-if reference order or index".into());
        }
        let n = if schema2 { reader.u32()? as usize } else { 1 };
        if !(1..=MAX_RANGES).contains(&n) {
            return Err("invalid native blend-if range count".into());
        }
        let mut ranges: Vec<BlendIfRange> = Vec::with_capacity(n);
        for _ in 0..n {
            let range = read_range(reader)?;
            if ranges
                .last()
                .is_some_and(|last| last.channel >= range.channel)
            {
                return Err("invalid native blend-if channel order".into());
            }
            if range.is_identity() {
                return Err("invalid native blend-if identity entry".into());
            }
            ranges.push(range);
        }
        let (first, rest) = ranges
            .split_first()
            .ok_or("invalid native blend-if range count")?;
        let blend_if = BlendIf::from_ranges(*first, rest)?;
        out.push((index, blend_if));
    }
    if schema2 && !needs_schema2(&out) {
        return Err("noncanonical native blend-if schema".into());
    }
    Ok(out)
}
