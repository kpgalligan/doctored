//! The Indexed Color document in the typed `.rz` (#547 Indexed phase 1a,
//! DRDC 108).
//!
//! Word 108 sits in the CMYK slot — the FIRST word inside the identity
//! wrapper 46 — written iff the document is Indexed; a document is never
//! Indexed and CMYK, Gray or Lab, so the slot holds one of 82–88 or 106–108.
//! The body is the RGB format's own (storage tag 1, u16): its one layer is
//! the policy's render, in an RGB profile. The `INDX` table is written last
//! before `LIDS`, where the CMYK, GRAY and LAB tables go: `"INDX"`, u32
//! schema 1, u16 count (2..=256), count × 3 RGB bytes, i16 transparent
//! index (−1 for none), u32 width, u32 height, width × height index bytes.
//! The reader requires the word iff the table, u16 storage, one
//! canvas-sized raster at the origin, every index inside the palette and the
//! body equal to the table's render ([`check`]), else it refuses by name;
//! the writer runs the same [`check`] first. A reader that knows no 108
//! meets it where it expects a body version and refuses the file; every
//! earlier file reads unchanged. `DRDC_VERSION` stays 89: 108 is a
//! conditional word like 90 to 107.

use crate::drdc::Reader;
use crate::indexed_document::IndexedPolicy;
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_native::append;

/// The Indexed document word.
pub(crate) const WORD: u32 = 108;

/// The word plus the table (module doc); 0 for every other document.
pub(crate) fn header_bytes(policy: Option<&IndexedPolicy>) -> u64 {
    policy.map_or(0, |p| {
        4 + 4 + 4 + 2 + 3 * p.palette().len() as u64 + 2 + 4 + 4 + p.indices().len() as u64
    })
}

/// The `INDX` table's bytes (module doc).
pub(crate) fn table_bytes(policy: &IndexedPolicy) -> Vec<u8> {
    let (width, height) = policy.dimensions();
    let mut out = Vec::with_capacity(header_bytes(Some(policy)) as usize);
    out.extend_from_slice(b"INDX");
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(&(policy.palette().len() as u16).to_le_bytes());
    for rgb in policy.palette() {
        out.extend_from_slice(rgb);
    }
    let transparent = policy.transparent().map_or(-1, i16::from);
    out.extend_from_slice(&transparent.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(policy.indices());
    out
}

/// Writes the `INDX` table for an Indexed document; nothing otherwise.
pub(crate) fn encode(
    policy: Option<&IndexedPolicy>,
    out: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    match policy {
        Some(policy) => append(out, &table_bytes(policy), bound, job),
        None => Ok(()),
    }
}

/// Reads the table into a policy. A schema, count, transparent index or
/// index this build does not admit refuses before any pixel decodes.
pub(crate) fn parse(reader: &mut Reader<'_>) -> Result<IndexedPolicy, String> {
    if reader.take(4)? != b"INDX" || reader.u32()? != 1 {
        return Err("unsupported native Indexed Color schema".into());
    }
    let count = usize::from(reader.u16()?);
    if !(crate::indexed_palette::MIN_COLORS..=crate::indexed_palette::MAX_COLORS).contains(&count) {
        return Err("native Indexed Color palette must hold 2 to 256 colours".into());
    }
    let palette: Vec<[u8; 3]> = reader
        .take(count * 3)?
        .chunks_exact(3)
        .map(|c| [c[0], c[1], c[2]])
        .collect();
    let transparent = match reader.u16()? as i16 {
        -1 => None,
        t if (0..count as i16).contains(&t) => Some(t as u8),
        _ => return Err("native Indexed Color transparent index is outside the palette".into()),
    };
    let width = reader.u32()?;
    let height = reader.u32()?;
    let pixels = crate::precision_image::pixel_count(width, height)?;
    let indices = reader.take(pixels)?.to_vec();
    IndexedPolicy::new(palette, transparent, indices, width, height)
        .map_err(|e| format!("native Indexed Color document: {e}"))
}

/// The reader's (and writer's) checks of a policy against the document
/// (module doc): u16 storage, one canvas-sized raster at the origin, and
/// its pixels the policy's render.
pub(crate) fn check(
    policy: &IndexedPolicy,
    document: &PrecisionDocument,
    job: JobContext<'_>,
) -> Result<(), String> {
    if document.is_float() {
        return Err("native Indexed Color document storage must be 16-bit".into());
    }
    if policy.dimensions() != document.dimensions() {
        return Err("native Indexed Color document plane does not match the canvas".into());
    }
    let image = match document.root_layers() {
        [layer] => match &layer.content {
            Content::Raster {
                image,
                offset: (0, 0),
            } => image,
            _ => return Err("native Indexed Color document must hold one raster layer".into()),
        },
        _ => return Err("native Indexed Color document must hold one raster layer".into()),
    };
    if !crate::indexed_document::is_rgb_profile(document.profile()) {
        return Err("native Indexed Color document profile is not an RGB profile".into());
    }
    if !policy.renders(image, job)? {
        return Err("native Indexed Color document pixels are not its palette's".into());
    }
    Ok(())
}
