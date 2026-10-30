//! The conditional typed GLOBAL LIGHT table. DRDC **58** is the FIRST
//! word inside the identity wrapper when present — `46 → (58) → (57) → (55)
//! → (54) → …` — and its `LGLT` table is the last one before LIDS (`… → WARP
//! → LOPN → RSPR → LGLT → LIDS`).
//!
//! The slot is `precision_group_open_native`'s and is taken for the same
//! reason: 12E-1 fixed two invariants — 46 is the FIRST word of every typed
//! `.rz` this build writes, and `LIDS` is the LAST table — and the host's
//! version sniffer and `tests/common/identity.rs`' identity peel both rest on
//! them. A light needs neither broken. An older typed build refuses a `46,
//! 58` file with its generic "not a supported … high-depth DRDC document"
//! sentence, which is the same forward contract every wrapper carries.
//!
//! The light is the byte `DrDocument.global_light` twin
//! (`precision_global_light`). The default 120°/30° writes neither the word
//! nor the table, so a document that never moved its light is byte-for-byte
//! the file the 57 writer made, and setting the default back restores those
//! bytes exactly.
//!
//! Framing: `LGLT`, u32 schema = 1, f32 angle, f32 altitude (LE). A bad tag
//! or schema refuses BEFORE a single PNG is decoded, as every other
//! conditional table does. The stored values are repaired through
//! `GlobalLight::sane` — the byte reader's v4 rule (`drdc_read.rs`) — so a
//! non-finite value reopens as the default and the resave drops the table.
//!
//! The BYTE depth needs no table: the light has been in the v4 document
//! header since layer styles arrived.
//!
//! the conditional typed word **58** and its `LGLT` table (u32 schema 1, f32 angle, f32 altitude):
//! the FIRST word inside 46 (`46 → (58) → (57) → (55) → (54) → …`) and the last table before `LIDS`
//! (`… → LOPN → RSPR → LGLT → LIDS`), written only for a non-default light, so a default-light
//! document writes the 57 writer's bytes exactly. A bad tag or schema refuses before any PNG
//! decodes, and so does word 58 in a float file.

use crate::drdc::Reader;
use crate::job_control::JobContext;
use crate::precision_native::append;
use crate::style::GlobalLight;

pub const PRECISION_VERSION: u32 = 58;

/// The table's own size, or 0 when it is not written: the version word the
/// writer emits alongside it, then the tag, the schema word and the two
/// f32 components.
pub(crate) fn header_bytes(light: GlobalLight) -> u64 {
    if light == GlobalLight::default() {
        0
    } else {
        20
    }
}

/// Writes the table, or nothing for the default light.
pub(crate) fn encode(
    light: GlobalLight,
    out: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    if light == GlobalLight::default() {
        return Ok(());
    }
    append(out, b"LGLT", bound, job)?;
    append(out, &1u32.to_le_bytes(), bound, job)?;
    append(out, &light.angle.to_le_bytes(), bound, job)?;
    append(out, &light.altitude.to_le_bytes(), bound, job)
}

/// Reads the table. The values are repaired, never refused, through the one
/// sanitizer.
pub(crate) fn parse(reader: &mut Reader<'_>) -> Result<GlobalLight, String> {
    if reader.take(4)? != b"LGLT" || reader.u32()? != 1 {
        return Err("unsupported native global light schema".into());
    }
    Ok(GlobalLight {
        angle: reader.f32()?,
        altitude: reader.f32()?,
    }
    .sane())
}
