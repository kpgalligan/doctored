//! A blank 16-bit document — File ▸ New… at 16 bits and MCP `new_document`
//! with `depth: 16`:
//! one canvas-sized "Background" (or caller-named) raster of four fill
//! samples, built by the core so the host never materializes `w·h` samples
//! and so the size refusal is ONE sentence in the user's terms.
//!
//! # The creation ceiling is the reopen plan
//!
//! A document is admitted when `native_preflight` admits it — the plan its
//! own `.rz` will be reopened under — and no separate creation threshold
//! exists. Since the banded reopen (`precision_native_plan`'s
//! `# Banded reopen`) that plan's reopen envelope is about 20 B/px for
//! one 16-bit canvas (8 decoded + 8 stored, plus a quarter), below the 2
//! GiB `REFERENCE_WORKING_BYTES` at every size the format can store. The
//! native PNG record limit (`native_png_encoded_bound ≤
//! MAX_DRDC_PNG_LEN`, `drdc.rs`, checked in `checked_plan` before the reopen
//! `admit`) is 1 GiB and `plane_capacity` holds a 16-bit plane to no decoder
//! cap, so the ceiling is the document's pixel limit: `pixel_count` refuses a
//! canvas past `MAX_PIXELS` (100,000,000) before anything is built, and
//! 10000² and 12500×8000 build (a stored plane of about 800 MB). Formerly
//! the record limit was 512 MiB and the ceiling about 67.1 MP (8192² refused).
//! A looser creation ceiling would make files that cannot be reopened; a
//! stricter one would refuse documents that work.
//!
//! # Why every build refusal is the size sentence
//!
//! For one canvas raster with a profile already checked here, the same
//! profile `Arc` on the document and the raster, default metadata and no
//! channels, `build`'s only reachable refusal is `native_preflight`, whose
//! reachable refusals are the record-limit bound and the reopen `admit` (at
//! a working limit below the 2 GiB reference) — each "this canvas is too
//! large at 16 bits". The other build checks (record count, profile size, one captured
//! profile handle, storage mode, channel shape) cannot fire for this input,
//! which is why [`PrecisionDocument::create_blank`] maps `from_parts`' error
//! wholesale. Every other caller gets the plan's `too_large_to_reopen` /
//! `too_large_to_save` sentences.
//!
//! File ▸ New… / `new_document` at 16 bits builds one canvas raster of four fill samples through
//! `filled_rgba16` (≤ 4 tiles; the host builds no `w·h` buffer) and ONE profile `Arc` shared by the
//! raster and the document. Admission charges only the profile and its parsed state (the intake's
//! figures without its sample terms). The creation ceiling IS the reopen plan (`native_preflight`):
//! with a 1 GiB stored-plane record limit (`MAX_DRDC_PNG_LEN`) and no decoder cap on a
//! 16-bit plane, every canvas within `MAX_PIXELS` builds, 10000² included — the module doc
//! derives it. `plane_capacity`'s and `admit`'s sentences stay for every other caller.

use std::sync::Arc;

use crate::icc::IccProfile;
use crate::metadata::{Metadata, Resolution};
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, pixel_count, PrecisionImage, BOOKKEEPING_BYTES};

/// The refusal a canvas too large to reopen at 16 bits earns (S1).
fn too_large(width: u32, height: u32) -> String {
    format!(
        "A 16-bit {width} \u{d7} {height} document is too large to reopen at 16 bits, so \
         closing other documents will not help. Choose a smaller canvas or 8 bits."
    )
}

/// The refusal a profile that is not complete RGB matrix/TRC bytes earns.
pub(crate) const NO_PROFILE: &str = "A 16-bit document needs an RGB matrix/TRC ICC profile.";

impl PrecisionDocument {
    /// A `width`×`height` 16-bit document whose one raster layer, `name`,
    /// is `fill` (straight RGBA16) everywhere, tagged with `profile`.
    /// Admission charges the profile and its parsed state (the host intake's
    /// figures without its sample terms: a filled plane retains at most four
    /// tiles, inside the bookkeeping term). A canvas the reopen plan refuses
    /// answers the size sentence (module doc).
    pub fn create_blank(
        width: u32,
        height: u32,
        fill: [u16; 4],
        profile: &[u8],
        name: &str,
        limit: u64,
        other: u64,
    ) -> Result<PrecisionDocument, String> {
        pixel_count(width, height)?;
        // The native blob cap, so an oversized profile is never answered
        // with the size sentence (module doc).
        if profile.len() > crate::drdc::MAX_DRDC_BLOB_LEN as usize {
            return Err(NO_PROFILE.into());
        }
        // ICC parsers can materialize sampled curves; 64 times the encoded
        // bytes bounds their parsed state, as in the host intake.
        admit(
            profile.len() as u64,
            4 * BOOKKEEPING_BYTES + 64 * profile.len() as u64,
            other,
            limit,
        )?;
        let parsed = IccProfile::parse(profile).ok_or(NO_PROFILE)?;
        if parsed.bytes().len() != profile.len() || parsed.model().is_none() {
            return Err(NO_PROFILE.into());
        }
        // ONE profile `Arc`, shared by the raster and the document, so the
        // "one captured profile handle" check cannot fire (module doc).
        let profile = Arc::new(parsed);
        let image = PrecisionImage::filled_rgba16(width, height, fill, profile.clone())?;
        let layer = PrecisionDocumentLayer::raster(name, image, (0, 0), 1.0, true)?;
        PrecisionDocument::from_parts(
            width,
            height,
            profile,
            Metadata::default(),
            Resolution::default(),
            vec![layer],
        )
        .map_err(|_| too_large(width, height))
    }
}
