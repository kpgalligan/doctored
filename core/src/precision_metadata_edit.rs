//! The typed twins of the three byte metadata edits (Phase 11D): import
//! bounded capture packets from a carrier, store or clear one packet, and set
//! the print resolution.
//!
//! # Why they did not exist before
//!
//! A typed (16-bit) document has carried packets and a resolution since
//! Phase 7 — `precision_codec` reads them out of a PNG/TIFF and
//! `doc_color::image_sidecar_with_policy` writes them back out at export —
//! but there was no way to PUT them there after the open. That was invisible
//! while every typed document came from a file the codec had just scanned,
//! and became a hole the moment the host started building typed documents
//! from its OWN decoders: a host still (8F) or a typed RAW develop (8C) hands
//! the core pixels, and the packets the platform reader recovered had nowhere
//! to go, so the export wrote none and the loss notice had to name them.
//!
//! # No new rules
//!
//! Each of the three is the byte contract verbatim — the same 16 MiB packet
//! cap, the same `None` for a value the document already carries (so a no-op
//! registers no undo step), the same `Resolution::sane` clamp and the same
//! carrier parser (`doc_metadata_import::carrier_packets`, which is shared
//! rather than copied). Nothing here decodes a pixel, adopts a carrier's
//! profile or touches a layer.
//!
//! Both depths name their refusals: a REFUSAL is an `Err` naming why — an
//! over-cap packet, a non-finite or non-positive resolution — so each facade
//! can say it instead of answering a silent nil, while `Ok(None)` stays the
//! no-change answer.
//! `import_metadata` is the precedent. Each sentence has one home, shared by
//! both depths: `doc_color::check_metadata_packet` and
//! `Resolution::checked`.
//!
//! `doc_metadata_import::carrier_packets` is the ONE carrier parser both depths read, extracted for
//! this; the byte bytes are unchanged. They are what lets a typed host still or a typed RAW put the
//! packets and the original PPI the platform reader recovered INTO the document, so a typed export
//! carries them.

use crate::doc_color::{check_metadata_packet, MetadataKind};
use crate::metadata::Resolution;
use crate::precision_document::PrecisionDocument;
use std::sync::Arc;

impl PrecisionDocument {
    /// Replaces capture packets from a bounded JPEG, PNG or TIFF carrier.
    /// Its pixels, resolution and profile are deliberately not adopted: the
    /// platform already decoded and normalized the real photograph. `None`
    /// for identical metadata (including a carrier with no packets and a
    /// receiver that has none); the warning list is returned either way.
    pub fn import_metadata(
        &self,
        bytes: &[u8],
    ) -> Result<(Option<Self>, Vec<&'static str>), String> {
        let (metadata, warnings) = crate::doc_metadata_import::carrier_packets(bytes)?;
        if metadata == self.metadata {
            return Ok((None, warnings));
        }
        let mut out = self.clone();
        out.metadata = metadata;
        Ok((Some(out), warnings))
    }

    /// Stores one metadata packet verbatim, or clears it with `None`.
    /// `Err` for a payload past the DRDC blob cap — enforced here so a
    /// document can never hold a packet the writer would refuse. `Ok(None)`
    /// for a value the document already carries.
    pub fn set_metadata(
        &self,
        kind: MetadataKind,
        bytes: Option<&[u8]>,
    ) -> Result<Option<Self>, String> {
        check_metadata_packet(bytes)?;
        let current = kind.get(&self.metadata).map(|b| &**b);
        if current == bytes {
            return Ok(None);
        }
        let mut out = self.clone();
        *kind.slot(&mut out.metadata) = bytes.map(|b| Arc::from(b.to_vec()));
        Ok(Some(out))
    }

    /// Pure setter for the print resolution. `Err` for a non-finite or
    /// non-positive component; `Ok(None)` for no change after sanitizing.
    /// PIXELS NEVER CHANGE: only the print size does.
    pub fn set_resolution(&self, x: f32, y: f32) -> Result<Option<Self>, String> {
        let next = Resolution::checked(x, y)?;
        if next == self.resolution {
            return Ok(None);
        }
        let mut out = self.clone();
        out.resolution = next;
        Ok(Some(out))
    }
}
