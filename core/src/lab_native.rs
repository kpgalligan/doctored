//! The Lab Color document in the typed `.rz` (#1 phase 1, DRDC 107).
//!
//! Word 107 sits in the CMYK slot — the FIRST word inside the identity
//! wrapper 46 — written iff the document is Lab; a document is never Lab and
//! CMYK or Gray, so the slot holds one of 82–88, 106 or 107. The body is the
//! RGB format's own (storage tag 1, u16), whose profile slot holds the Lab
//! twin (`icc_builtin::lab_twin_blob`). The `LAB ` table — u32 schema 1, u8
//! declared depth (8 or 16) — is written last before `LIDS`, where the CMYK
//! and GRAY tables go. The reader requires the word iff the table, u16
//! storage, and the body's profile byte-equal to the twin, else it refuses
//! by name; the writer runs the same [`check`] first. A reader that knows no
//! 107 meets it where it expects a body version and refuses the file; every
//! earlier file reads unchanged. `DRDC_VERSION` stays 89: 107 is a
//! conditional word like 90 to 106.

use crate::drdc::Reader;
use crate::job_control::JobContext;
use crate::lab_document::{LabDepth, LabPolicy};
use crate::precision_native::append;

/// The Lab document word.
pub(crate) const WORD: u32 = 107;

/// The word plus the table: 4 + `"LAB "` 4 + schema 4 + depth 1; 0 for
/// every other document.
pub(crate) fn header_bytes(policy: Option<&LabPolicy>) -> u64 {
    policy.map_or(0, |_| 13)
}

/// The `LAB ` table's bytes (module doc).
pub(crate) fn table_bytes(policy: &LabPolicy) -> Vec<u8> {
    let mut out = Vec::with_capacity(9);
    out.extend_from_slice(b"LAB ");
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.push(policy.depth().bits() as u8);
    out
}

/// Writes the `LAB ` table for a Lab document; nothing otherwise.
pub(crate) fn encode(
    policy: Option<&LabPolicy>,
    out: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    match policy {
        Some(policy) => append(out, &table_bytes(policy), bound, job),
        None => Ok(()),
    }
}

/// Reads the table into a policy. A schema or depth this build does not
/// admit refuses before any pixel decodes.
pub(crate) fn parse(reader: &mut Reader<'_>) -> Result<LabPolicy, String> {
    if reader.take(4)? != b"LAB " || reader.u32()? != 1 {
        return Err("unsupported native Lab schema".into());
    }
    let depth = LabDepth::from_bits(u32::from(reader.u8()?)).ok_or("invalid native Lab depth")?;
    Ok(LabPolicy::for_depth(depth))
}

/// The reader's (and writer's) checks of a policy against the body (module
/// doc): u16 storage, and the body's profile byte-equal to the twin.
pub(crate) fn check(_policy: &LabPolicy, float: bool, profile: &[u8]) -> Result<(), String> {
    if float {
        return Err("native Lab document storage must be 16-bit".into());
    }
    if profile != crate::icc_builtin::lab_twin_blob() {
        return Err("native Lab document profile is not the Lab twin".into());
    }
    Ok(())
}
