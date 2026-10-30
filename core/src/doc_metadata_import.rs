//! Attaches packets serialized by a platform metadata reader, without decoding
//! its carrier image or changing the receiving document's pixels, ICC or PPI.
//! The ordinary container scanner remains the one parser on both open paths.

use crate::doc::DrDocument;
use crate::drdc::MAX_DRDC_BLOB_LEN;
use crate::metadata::{self, Metadata};

/// A carrier contains at most three native-sized packets and a small image.
/// This bound is checked before the FFI reads caller memory. It is not a limit
/// on photographic source files, which use the normal file-open scanner.
pub(crate) const MAX_METADATA_CARRIER: usize = MAX_DRDC_BLOB_LEN as usize * 3 + 1024 * 1024;

impl DrDocument {
    /// Replaces capture packets from a bounded JPEG, PNG or TIFF carrier.
    /// Its pixels, resolution and profile are deliberately not adopted: the
    /// platform already decoded/developed and normalized the real photograph.
    /// Returns None for identical metadata, including a carrier with no packets
    /// and a receiver that has none. The warning list is returned even for
    /// no-op attachment. Unsupported carriers/malformed TIFF headers error.
    pub fn import_metadata(
        &self,
        bytes: &[u8],
    ) -> Result<(Option<Self>, Vec<&'static str>), String> {
        let (metadata, warnings) = carrier_packets(bytes)?;
        if metadata == self.metadata {
            return Ok((None, warnings));
        }
        let mut out = self.clone();
        out.metadata = metadata;
        Ok((Some(out), warnings))
    }
}

/// The carrier's packets and the omissions its scan detected — the WHOLE
/// rule, extracted (Phase 11D) so the typed twin
/// (`precision_metadata_edit`) reads one carrier parser rather than a second
/// copy of these four refusals. The byte bytes are unchanged: `DrDocument`
/// calls this and compares exactly as it did.
pub(crate) fn carrier_packets(bytes: &[u8]) -> Result<(Metadata, Vec<&'static str>), String> {
    if bytes.len() > MAX_METADATA_CARRIER {
        return Err("metadata carrier exceeds the bounded packet import limit".into());
    }
    if !metadata::walks(bytes) {
        return Err("metadata carrier must be JPEG, PNG or TIFF".into());
    }
    if crate::metadata_tiff::is_tiff(bytes) && crate::tiff_view::Tiff::file(bytes).is_none() {
        return Err("metadata carrier TIFF header is malformed".into());
    }
    let (scan, warnings) = metadata::scan_report(bytes);
    let metadata = scan.metadata;
    if [&metadata.exif, &metadata.xmp, &metadata.iptc]
        .into_iter()
        .flatten()
        .any(|packet| packet.len() > MAX_DRDC_BLOB_LEN as usize)
    {
        return Err("metadata packet exceeds the native 16 MiB limit".into());
    }
    Ok((metadata, warnings))
}
