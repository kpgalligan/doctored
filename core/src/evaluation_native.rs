//! Conditional linear-light evaluation wrappers. DRDC38 (byte) /39 (typed) is
//! the OUTERMOST version word — it inserts its u32 before the exact prior
//! version/body, so every earlier optional feature keeps its own framing — and
//! appends one table after every prior tail: `EVAL`, u32 schema=1, u32 policy=1.
//! `mask_outside_native.rs` is the shape this follows.
//!
//! The table is written ONLY for a `linear_light` document: an encoded one is
//! byte-for-byte the file the prior writer made, first word included. Policy 0
//! is therefore never written and is refused on read — a file that says
//! "encoded" in the one slot that only exists to say "linear" is malformed,
//! not a document with a default. Any schema or policy this build does not
//! know, and any trailing word after the table, refuses BEFORE a single PNG is
//! decoded, because an evaluation policy is a claim about every pixel in the
//! file.
//!
//! `evaluation_native` is the outermost conditional `.rz` wrapper — byte 38 / typed 39 before every
//! prior version word, one `EVAL` table after every prior tail, written only for `linear_light` —
//! following `mask_outside_native.rs`'s shape.
use crate::drdc::Reader;
use crate::job_control::JobContext;
use crate::precision_native::append;

pub const BYTE_VERSION: u32 = 38;
pub const PRECISION_VERSION: u32 = 39;

/// The version word plus the table, or 0 when neither is written: the exact
/// count the writer writes (`precision_native`'s word 39 and `encode`'s 12
/// bytes), since the exact PNG bound leaves the plan no slack to hide a
/// short count in.
pub(crate) fn header_bytes(present: bool) -> u64 {
    if present {
        16
    } else {
        0
    }
}

/// Writes the table for a linear-light document; writes nothing otherwise.
pub(crate) fn encode(
    present: bool,
    out: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    if !present {
        return Ok(());
    }
    append(out, b"EVAL", bound, job)?;
    for value in [1u32, 1u32] {
        append(out, &value.to_le_bytes(), bound, job)?;
    }
    Ok(())
}

/// Reads the table. The only admitted contents are schema 1 and policy 1
/// (`linear_light`); everything else is a refusal, not a repair.
pub(crate) fn parse(reader: &mut Reader<'_>) -> Result<(), String> {
    if reader.take(4)? != b"EVAL" || reader.u32()? != 1 {
        return Err("unsupported native evaluation schema".into());
    }
    if reader.u32()? != 1 {
        return Err("invalid native evaluation policy".into());
    }
    Ok(())
}
