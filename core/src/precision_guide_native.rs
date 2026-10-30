//! The conditional typed GUIDES / RULER-ORIGIN table. DRDC **48** is the
//! second version word that sits INSIDE the identity wrapper, beside 47:
//! `46 → 47 → 48 → 44 → …`, and its `LGDE` table lands after every prior
//! tail but BEFORE `LIDS` (`… → LCMP → LLNK → LGDE → LIDS`).
//!
//! That slot is `precision_lock_native`'s, and it is taken for exactly the
//! same reason: 12E-1 fixed two invariants — 46 is the FIRST word of every
//! typed `.rz` this build writes, and `LIDS` is the LAST table — and the
//! host's only reader of the version word
//! (`RasterDocument+PrecisionMetadata.swift`'s sniffer) together with
//! `tests/common/identity.rs`' identity peel both rest on them. Guides need
//! neither broken. An older build refuses a 48 body word by name, which is
//! the same forward contract every wrapper carries.
//!
//! The table is written ONLY when the document carries a guide or has moved
//! its ruler origin, so a plain document is byte-for-byte the file the
//! 12E-1b writer made — and clearing the last guide with the origin back at
//! `(0, 0)` restores those bytes exactly. Reopen → resave is byte-identical
//! because the reader adopts what it reads.
//!
//! Framing: `LGDE`, u32 schema = 1, f64 ruler-origin x, f64 ruler-origin y,
//! u32 guide count (at most `doc_guide::MAX_GUIDES`), then per guide u8
//! orientation (0 horizontal, 1 vertical) and f64 position — the BYTE
//! version-8 block's layout verbatim, after this format's 8-byte tag/schema
//! prefix, so the two depths store the same nine bytes per guide.
//!
//! The REPAIR/REFUSE split is `drdc_read`'s and is deliberate: a position is
//! REPAIRED (non-finite drops that guide, out of canvas clamps, everything
//! quantizes) because a nonsense number is a broken value the format mends,
//! while the orientation byte and the count are structural claims and are
//! REFUSED. The writer runs the same `sane_origin` / `sane_position` the
//! reader does, so it can never emit a file its own reader would change.
//!
//! Guide IDENTITIES are deliberately NOT stored, at either depth: `Guide::id`
//! is a host handle for as long as a document is open, so the reader mints
//! fresh ones through `Guide::new` (`doc_guide`).
//!
//! the writer runs the reader's own `sane_origin`/`sane_position`, positions are REPAIRED and the
//! orientation byte and the count REFUSED, and guide IDS are deliberately not persisted at either
//! depth.

use crate::doc_guide::{
    normalized, sane_origin, sane_position, Guide, GuideOrientation, MAX_GUIDES,
};
use crate::drdc::Reader;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_native::append;

pub const PRECISION_VERSION: u32 = 48;

/// What the writer stores: the sanitized ruler origin, then one
/// `(orientation byte, position)` pair per guide. Named because the plan,
/// the writer and the size helper all pass it around, and an anonymous tuple
/// that deep reads as noise at every one of them.
pub(crate) type GuideEntries = ((f64, f64), Vec<(u8, f64)>);

/// The sanitized origin and the sanitized `(orientation, position)` list, or
/// `None` when the document has NOTHING to say — no guide and the origin
/// still at the canvas's top-left — which is what makes the table
/// conditional.
pub(crate) fn precision_entries(document: &PrecisionDocument) -> Option<GuideEntries> {
    let (width, height) = document.dimensions();
    let origin = sane_origin(document.ruler_origin(), width, height);
    let guides: Vec<(u8, f64)> = document
        .guides()
        .iter()
        .filter_map(|guide| {
            let extent = match guide.orientation {
                GuideOrientation::Horizontal => f64::from(height),
                GuideOrientation::Vertical => f64::from(width),
            };
            Some((
                guide.orientation.to_c() as u8,
                sane_position(guide.position, extent)?,
            ))
        })
        .collect();
    if guides.is_empty() && origin == (0.0, 0.0) {
        return None;
    }
    Some((origin, guides))
}

/// The table's own size, or 0 when it is not written: the version word the
/// writer emits alongside it, then the tag, the schema word, the two origin
/// doubles and the count word ahead of the nine-byte guides.
pub(crate) fn header_bytes(entries: Option<&GuideEntries>) -> u64 {
    match entries {
        None => 0,
        Some((_, guides)) => 32 + guides.len() as u64 * 9,
    }
}

pub(crate) fn encode(
    entries: &Option<GuideEntries>,
    out: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    let Some((origin, guides)) = entries else {
        return Ok(());
    };
    // The writer enforces the reader's cap, as the byte writer does.
    let count = u32::try_from(guides.len())
        .ok()
        .filter(|&c| c as usize <= MAX_GUIDES)
        .ok_or_else(|| format!("too many guides (max {MAX_GUIDES})"))?;
    append(out, b"LGDE", bound, job)?;
    append(out, &1u32.to_le_bytes(), bound, job)?;
    for value in [origin.0, origin.1] {
        append(out, &value.to_le_bytes(), bound, job)?;
    }
    append(out, &count.to_le_bytes(), bound, job)?;
    for (orientation, position) in guides {
        append(out, &[*orientation], bound, job)?;
        append(out, &position.to_le_bytes(), bound, job)?;
    }
    Ok(())
}

pub(crate) fn parse(
    reader: &mut Reader<'_>,
    width: u32,
    height: u32,
    job: JobContext<'_>,
) -> Result<((f64, f64), Vec<Guide>), String> {
    if reader.take(4)? != b"LGDE" || reader.u32()? != 1 {
        return Err("unsupported native guide schema".into());
    }
    let origin = sane_origin((reader.f64()?, reader.f64()?), width, height);
    let count = reader.u32()? as usize;
    if count > MAX_GUIDES {
        return Err("invalid native guide count".into());
    }
    let mut guides: Vec<Guide> = Vec::with_capacity(count);
    for _ in 0..count {
        job.check()?;
        let raw = reader.u8()?;
        let orientation = GuideOrientation::from_c(i32::from(raw))
            .ok_or("unsupported native guide orientation")?;
        let extent = match orientation {
            GuideOrientation::Horizontal => f64::from(height),
            GuideOrientation::Vertical => f64::from(width),
        };
        // Through `Guide::new`, so a loaded guide takes a FRESH identity:
        // the id is a session handle, never a persisted property.
        if let Some(position) = sane_position(reader.f64()?, extent) {
            guides.push(Guide::new(orientation, position));
        }
    }
    Ok((origin, normalized(guides)))
}
