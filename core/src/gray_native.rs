//! The Grayscale document in the typed `.rz` (#1 phase 1a, DRDC 106).
//!
//! Word 106 sits in the CMYK slot — the FIRST word inside the identity
//! wrapper 46 — written iff the document is Gray; a document is never both
//! Gray and CMYK, so the slot holds one or the other. The body is the RGB
//! format's own (storage tag 1, or the float tag under 40 for a 32-bit
//! document), whose profile slot holds the policy's RGB twin
//! (`icc_builtin::twin_blob`). The `GRAY` table — u32 schema 1, u8 declared
//! depth (8, 16 or 32), u32 length plus the Gray ICC bytes verbatim (≤
//! `MAX_DRDC_BLOB_LEN`) — is written last before `LIDS`, where the CMYK
//! table goes. The reader requires the word iff the table, the depth to
//! agree with the storage (32 ⇔ float), and the body's profile to be
//! byte-equal to `twin_blob` of the table's ICC, else it refuses by name. A
//! reader that knows no 106 meets it where it expects a body version and
//! refuses the file; every earlier file reads unchanged. `DRDC_VERSION`
//! stays 89: 106 is a conditional word like 90 to 105.

use crate::drdc::{Reader, MAX_DRDC_BLOB_LEN};
use crate::gray_document::{GrayDepth, GrayPolicy};
use crate::job_control::JobContext;
use crate::precision_native::append;

/// The Gray document word.
pub(crate) const WORD: u32 = 106;

/// The word plus the table: 4 + `"GRAY"` 4 + schema 4 + depth 1 + length 4
/// + the profile; 0 for every other document.
pub(crate) fn header_bytes(policy: Option<&GrayPolicy>) -> u64 {
    policy.map_or(0, |p| 17 + p.bytes().len() as u64)
}

/// The `GRAY` table's bytes (module doc).
pub(crate) fn table_bytes(policy: &GrayPolicy) -> Vec<u8> {
    let bytes = policy.bytes();
    let mut out = Vec::with_capacity(13 + bytes.len());
    out.extend_from_slice(b"GRAY");
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.push(policy.depth().bits() as u8);
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
    out
}

/// Writes the `GRAY` table for a Gray document; nothing otherwise.
pub(crate) fn encode(
    policy: Option<&GrayPolicy>,
    out: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    match policy {
        Some(policy) => append(out, &table_bytes(policy), bound, job),
        None => Ok(()),
    }
}

/// Reads the table into a policy. A schema, depth or profile this build does
/// not admit refuses before any pixel decodes.
pub(crate) fn parse(reader: &mut Reader<'_>) -> Result<GrayPolicy, String> {
    if reader.take(4)? != b"GRAY" || reader.u32()? != 1 {
        return Err("unsupported native Gray schema".into());
    }
    let depth = GrayDepth::from_bits(u32::from(reader.u8()?)).ok_or("invalid native Gray depth")?;
    let length = reader.u32()?;
    if length > MAX_DRDC_BLOB_LEN {
        return Err("native Gray profile exceeds its byte limit".into());
    }
    GrayPolicy::new(reader.take(length as usize)?, depth)
}

/// The reader's checks of a parsed policy against the body (module doc):
/// 32 ⇔ float, and the body's profile byte-equal to the twin of the ICC.
pub(crate) fn check(policy: &GrayPolicy, float: bool, profile: &[u8]) -> Result<(), String> {
    if float != (policy.depth() == GrayDepth::ThirtyTwo) {
        return Err("native Gray depth does not match the document's storage".into());
    }
    if crate::icc_builtin::twin_blob(policy.bytes())? != profile {
        return Err("native Gray document profile is not its Gray profile's twin".into());
    }
    Ok(())
}
