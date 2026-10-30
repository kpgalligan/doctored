//! Conditional `CSMP` table: the document's COLOUR SAMPLERS in `.rz`,
//! at both depths, with one framing.
//!
//! Byte word **59** is the FIRST word inside the identity wrapper 45, ahead
//! of every other conditional word (`45 -> (59) -> (56) -> (53) -> …`), and
//! typed word **60** the first inside 46 (`46 -> (60) -> (58) -> (57) ->
//! …`). Tables close in reverse, so `CSMP` is the LAST table before `LIDS`:
//! byte `… WARP -> RSPR -> CSMP -> LIDS`, typed `… LOPN -> RSPR -> LGLT ->
//! CSMP -> LIDS`. Both 12E-1 invariants hold — 45/46 is still the first
//! word of every file and `LIDS` the last table — so the host's version
//! sniffer and the identity peel are untouched.
//!
//! Wire format, little-endian: `CSMP`, u32 schema = 1, u32 count (1…10),
//! then per sampler u32 x, u32 y and u8 sample size. Written ONLY when the
//! list is non-empty, so a sampler-free document is byte-for-byte the file
//! the prior writer made, and clearing the last sampler restores those
//! bytes.
//!
//! The reader REFUSES the structural claims — a bad tag or schema, a count
//! of 0 or past `doc_sampler::MAX_COLOR_SAMPLERS`, a sample size other than
//! 1, 3 or 5 — before a single PNG decodes, and REPAIRS a broken value: an
//! entry off the canvas is dropped (the guide reader's clamp-not-refuse
//! rule, with the drop every geometry op applies to a sampler). The writer
//! enforces the reader's caps, so every file it writes reads back.
//!
//! An older BYTE build refuses a 59 file as "unsupported DRDC version 59";
//! an older TYPED build refuses a `46, 60` file with its generic "not a
//! supported … high-depth DRDC document" sentence.
//!
//! `sampler_native` is the conditional `.rz` word **59** (byte) / **60** (typed), the FIRST word
//! inside 45/46 (`45 → (59) → (56) → …`, `46 → (60) → (58) → …`), and its `CSMP` table (u32 schema
//! 1, u32 count 1…10, then u32 x, u32 y, u8 size per sampler, LE) is the LAST table before `LIDS`
//! (byte after RSPR, typed after LGLT; the typed plan charges it); written only for a non-empty
//! list, a bad tag/schema/count/size refuses before any PNG decodes, an off-canvas entry is
//! dropped.

use crate::doc_sampler::{valid_size, ColorSampler, MAX_COLOR_SAMPLERS};
use crate::drdc::Reader;

pub(crate) const BYTE_VERSION: u32 = 59;
pub(crate) const PRECISION_VERSION: u32 = 60;

/// The table's encoded size; 0 when there is nothing to write.
pub(crate) fn encoded_len(s: &[ColorSampler]) -> u64 {
    if s.is_empty() {
        0
    } else {
        12 + 9 * s.len() as u64
    }
}

/// The table, or an empty Vec when there is nothing to write. It re-checks
/// the reader's caps so the writer never emits a file its reader refuses.
pub(crate) fn encode(s: &[ColorSampler]) -> Result<Vec<u8>, String> {
    if s.is_empty() {
        return Ok(Vec::new());
    }
    if s.len() > MAX_COLOR_SAMPLERS {
        return Err(format!(
            "too many color samplers (max {MAX_COLOR_SAMPLERS})"
        ));
    }
    let mut output = Vec::with_capacity(encoded_len(s) as usize);
    output.extend_from_slice(b"CSMP");
    output.extend_from_slice(&1_u32.to_le_bytes());
    output.extend_from_slice(&(s.len() as u32).to_le_bytes());
    for sampler in s {
        if !valid_size(sampler.sample_size) {
            return Err("color sampler sample size must be 1, 3 or 5".into());
        }
        output.extend_from_slice(&sampler.x.to_le_bytes());
        output.extend_from_slice(&sampler.y.to_le_bytes());
        output.push(sampler.sample_size);
    }
    Ok(output)
}

/// Reads the table for a `w × h` canvas. Allocates at most ten entries and
/// decodes no pixel.
pub(crate) fn parse(r: &mut Reader<'_>, w: u32, h: u32) -> Result<Vec<ColorSampler>, String> {
    if r.take(4)? != b"CSMP" || r.u32()? != 1 {
        return Err("invalid native color sampler table tag or schema".into());
    }
    let count = r.u32()? as usize;
    if count == 0 || count > MAX_COLOR_SAMPLERS {
        return Err("invalid native color sampler count".into());
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let x = r.u32()?;
        let y = r.u32()?;
        let sample_size = r.u8()?;
        if !valid_size(sample_size) {
            return Err("invalid native color sampler sample size".into());
        }
        if x < w && y < h {
            out.push(ColorSampler { x, y, sample_size });
        }
    }
    Ok(out)
}
