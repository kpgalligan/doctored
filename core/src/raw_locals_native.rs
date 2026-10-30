//! Conditional word 79: a `.rz` document whose retained RAW sources
//! carry more than four local masks on one layer, or more than 512 MiB of
//! RAW and attachment resources together, at both depths.
//!
//! Word **79** is ONE number at both depths and never a first word: it is
//! the FIRST word inside the identity wrapper, ahead of the path word, at 8
//! bits (`45 -> (79) -> (74) -> (72) -> …`) and at high depth (`46 -> (79) ->
//! (75) -> (71) -> …`), so the wrapper alone says which depth the file is. It
//! has no table: the masks ride in the existing ATCH/RBIN pool
//! (`native_source_resources`) as they always have. Both 12E-1 invariants
//! hold: 45/46 is still the first word of every file and `LIDS` the last
//! table.
//!
//! The word exists because the pool alone cannot protect itself. An older
//! build reads an attachment's reference count against one to four and the
//! pool against 512 MiB, so it would refuse such a file with a sentence about
//! a corrupt table; with the word it refuses it by name (`unsupported DRDC
//! version 79`). A document whose every attachment holds four or fewer
//! resources and whose resources total 512 MiB or less (every document an
//! earlier build could write) writes no word, so its bytes are exactly the
//! prior writer's. This build checks the claim in BOTH directions right after
//! the source table is parsed, before a single PNG decodes: the word with no
//! such sources is `invalid native RAW locals word`, and such sources with
//! no word refuse with [`MISSING_WORD`].
use crate::raw_local_limits::PRE_79_ATTACHMENT_RESOURCES;
use crate::raw_source::MAX_RAW_TOTAL_BYTES;
use crate::source_asset::SourceAsset;
use std::collections::HashSet;
use std::sync::Arc;

/// The word, at both depths.
pub(crate) const VERSION: u32 = 79;
/// The reader's refusal of the word with no sources that need it.
pub(crate) const INVALID_WORD: &str = "invalid native RAW locals word";
/// The reader's refusal of sources that need the word when it is absent.
pub(crate) const MISSING_WORD: &str =
    "RAW sources with more than four local masks or over 512 MiB of resources require DRDC 79";

/// The predicate both sides share: the pool's deduplicated byte total
/// exceeds the pre-79 aggregate, or some attachment holds more than the
/// pre-79 resource count.
#[doc(hidden)]
pub fn claim(resource_bytes: u64, most_resources: usize) -> bool {
    resource_bytes > MAX_RAW_TOTAL_BYTES || most_resources > PRE_79_ATTACHMENT_RESOURCES
}

/// The writer's predicate over the assets it writes: their RAW originals and
/// attachment resources deduplicated by allocation, exactly as the ATCH/RBIN
/// pool stores them, and the largest attachment.
pub(crate) fn carries<'a>(assets: impl IntoIterator<Item = &'a SourceAsset>) -> bool {
    let mut seen = HashSet::new();
    let (mut bytes, mut most) = (0_u64, 0_usize);
    for asset in assets {
        let raw = asset.raw.iter().map(|raw| &raw.encoded);
        let resources = asset.attachment.iter().flat_map(|a| a.resources.iter());
        for blob in raw.chain(resources) {
            if seen.insert(Arc::as_ptr(blob) as usize) {
                bytes = bytes.saturating_add(blob.len() as u64);
            }
        }
        if let Some(attachment) = &asset.attachment {
            most = most.max(attachment.resources.len());
        }
    }
    claim(bytes, most)
}

/// The header bytes the word adds to a save plan: the version word alone,
/// since there is no table.
pub(crate) fn header_bytes(carries: bool) -> u64 {
    if carries {
        4
    } else {
        0
    }
}
