//! File Info as a DOCUMENT operation, at both depths: the thin
//! adapters over `file_info`, which holds the model, the limits and the
//! packet edits. The fields live in the document's metadata packets, so they
//! are undoable, persist in `.rz` and travel through every export that
//! writes the packets, with nothing new in the file format.

use crate::doc::DrDocument;
use crate::file_info::{read, write, FileInfo};
use crate::metadata::Metadata;
use crate::precision_document::PrecisionDocument;

/// The packets after setting `info`, or `None` when nothing would change:
/// the document already states exactly these four values, or writing them
/// leaves every packet as it is. Asking the VALUES first matters: another
/// program's packet can state the same fields in a different spelling, and
/// re-spelling it would be an edit that changes nothing a person can see.
fn edited(metadata: &Metadata, info: &FileInfo) -> Result<Option<Metadata>, String> {
    let wanted = info.clone().checked()?;
    if wanted == read(metadata) {
        return Ok(None);
    }
    let next = write(metadata, &wanted)?;
    Ok((next != *metadata).then_some(next))
}

impl DrDocument {
    /// The creator, copyright notice, description and keywords the
    /// document's metadata states (`file_info::read`).
    pub fn file_info(&self) -> FileInfo {
        read(&self.metadata)
    }

    /// Sets the four fields (`file_info::write`). `Err` for a value past
    /// the limits or a packet that cannot take the edit; `Ok(None)` when the
    /// metadata would not change, so the host registers no undo step.
    /// Pixels, the profile and the resolution are untouched.
    pub fn set_file_info(&self, info: &FileInfo) -> Result<Option<Self>, String> {
        Ok(edited(&self.metadata, info)?.map(|metadata| {
            let mut out = self.clone();
            out.metadata = metadata;
            out
        }))
    }
}

impl PrecisionDocument {
    /// The typed twin of [`DrDocument::file_info`].
    pub fn file_info(&self) -> FileInfo {
        read(&self.metadata)
    }

    /// The typed twin of [`DrDocument::set_file_info`], the same contract.
    pub fn set_file_info(&self, info: &FileInfo) -> Result<Option<Self>, String> {
        Ok(edited(&self.metadata, info)?.map(|metadata| {
            let mut out = self.clone();
            out.metadata = metadata;
            out
        }))
    }
}
