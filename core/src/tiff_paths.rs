//! PATHS IN A FLAT TIFF: the saved paths, the work path and the clipping
//! path (image resources 2000–2997, 1025 and 2999) in a flat TIFF's tag
//! 34377, the 8BIM run beside the spot channels' resources (`tiff_spot`). A
//! flat TIFF's run holds the same blocks a PSD's resource section does, so
//! the records, their rules and their sentences are `psd_paths`': one
//! reader (`run_paths`) and one writer (`resources`).
//!
//! READ ([`DrDocument::with_tiff_paths`],
//! [`PrecisionDocument::with_tiff_paths`]). The three flat opens that read a
//! TIFF's spot channels — the byte open, the typed RGB open and the CMYK
//! open — read its paths from the same run (the document's
//! `metadata.iptc`), each record a fraction of the stored raster, and then
//! take every path resource out of that run, read or not: the model is the
//! paths' one home from then on, so a deleted path cannot come back from the
//! run and an edited one is not written twice. A file stored rotated
//! (Orientation other than 1) opens upright, where those fractions name
//! other places, so its paths are neither read nor kept, with a sentence.
//! The typed opens join the sentences to the import warnings; the byte
//! document has none, so its sentences are the flat import report's
//! ([`report_lines`]). Grayscale, Lab, Indexed, Bitmap and 32-bit float
//! documents open their TIFFs elsewhere and read no path.
//!
//! MOVE ([`canvas_moved`], [`moves_canvas`]). A run that still holds path
//! resources came from a file whose open does not read them (a JPEG's
//! APP13). Its records are fractions of the canvas that file stored, so
//! Image Size keeps them right, and every other whole-canvas edit — Crop,
//! Canvas Size, a quarter turn or flip, Rotate Canvas, Straighten,
//! Perspective Crop — leaves them naming other places. Each of those takes
//! them out of the run at both depths, beside the moved paths of the
//! document's own, so no writer passes them on and no TIFF reopen reads
//! them where they no longer belong.
//!
//! WRITE ([`attach`]). Every TIFF writer of those document kinds — the byte
//! save and export, the typed RGB save and export, the CMYK save and kept
//! export — writes the document's own path resources after the inherited
//! run. A document with no path of its own passes the run's path resources
//! through as before (the canvas they describe is still its own, MOVE), and
//! one with paths drops them, as resource ids would otherwise collide, and
//! its flat save lists "paths" among the losses. Every other writer passes
//! the inherited run as it did. The records are fractions of the canvas, so
//! they stay right at an export's output size. A path whose coordinates
//! leave the fixed-point range is not written, and the flat save lists
//! "paths" then too ([`DrDocument::tiff_keeps_paths`]).

use std::sync::Arc;

use crate::doc::DrDocument;
use crate::doc_path::PathEntry;
use crate::metadata::Metadata;
use crate::metadata_write::{filtered_run, EncodeSidecar};
use crate::precision_document::PrecisionDocument;
use crate::precision_geometry::GeometryEdit;
use crate::psd_container::resource_entries;
use crate::psd_export_encode::resource_named;
use crate::psd_paths::{
    all_written, resources, run_paths, CLIPPING_PATH_RESOURCE, FIRST_SAVED, LAST_SAVED,
    WORK_PATH_RESOURCE,
};
use crate::tiff_directory::{bytes_reader, Directory};

/// Whether image resource `id` is a path's or the clipping path's.
fn is_path_resource(id: u16) -> bool {
    id == WORK_PATH_RESOURCE
        || id == CLIPPING_PATH_RESOURCE
        || (FIRST_SAVED..=LAST_SAVED).contains(&id)
}

/// Whether the 8BIM `run` holds a path resource.
fn holds_paths(run: Option<&[u8]>) -> bool {
    run.and_then(|run| resource_entries(run).ok())
        .is_some_and(|entries| entries.iter().any(|(id, _, _)| is_path_resource(*id)))
}

/// `run` without its path resources: `None` when nothing else is left, and
/// the run itself when it holds none or the walk cannot read it.
fn without_paths(run: Option<Arc<[u8]>>) -> Option<Arc<[u8]>> {
    let run = run?;
    if !holds_paths(Some(&run[..])) {
        return Some(run);
    }
    match filtered_run(&run, |id| !is_path_resource(id)) {
        Some(rest) if rest.is_empty() => None,
        Some(rest) => Some(Arc::from(rest)),
        None => Some(run),
    }
}

/// Whether `edit` moves the canvas other than by a scale (module doc, MOVE):
/// every whole-canvas edit but Image Size and a whole turn. A per-layer
/// transform moves no canvas point.
pub(crate) fn moves_canvas(edit: &GeometryEdit) -> bool {
    use crate::doc_rotate::{rotation_kind, RotationKind};
    match edit {
        GeometryEdit::Crop { .. }
        | GeometryEdit::CanvasSize { .. }
        | GeometryEdit::Straighten { .. }
        | GeometryEdit::Rotate90
        | GeometryEdit::Rotate180
        | GeometryEdit::Rotate270
        | GeometryEdit::FlipHorizontal
        | GeometryEdit::FlipVertical => true,
        GeometryEdit::RotateCanvas { degrees, .. } => {
            !matches!(rotation_kind(*degrees), Some((RotationKind::Identity, _)))
        }
        GeometryEdit::Resize { .. }
        | GeometryEdit::Affine { .. }
        | GeometryEdit::Perspective { .. }
        | GeometryEdit::AffineSet { .. }
        | GeometryEdit::PerspectiveSet { .. } => false,
    }
}

/// `metadata` after an edit that moves the canvas other than by a scale
/// (module doc, MOVE): its 8BIM run without path resources.
pub(crate) fn canvas_moved(metadata: &Metadata) -> Metadata {
    Metadata {
        iptc: without_paths(metadata.iptc.clone()),
        ..metadata.clone()
    }
}

/// The paths of the flat TIFF `bytes`, whose 8BIM run is `run`, on its
/// stored `canvas`, and the sentences about them (module doc): nothing but
/// one sentence for a file stored rotated that holds a path resource.
fn file_paths(
    bytes: &[u8],
    run: Option<&[u8]>,
    canvas: (u32, u32),
) -> (Vec<PathEntry>, Vec<String>) {
    let Some(run) = run else {
        return (Vec::new(), Vec::new());
    };
    let (entries, losses) = run_paths(run, canvas);
    if entries.is_empty() && losses.is_empty() {
        return (entries, losses);
    }
    let orientation = crate::metadata_tiff::orientation_of_file(bytes).map_or(1, |o| o.to_exif());
    if orientation != 1 {
        return (
            Vec::new(),
            vec![format!(
                "Paths in this TIFF are not read or kept: it is stored rotated (Orientation {orientation})."
            )],
        );
    }
    (entries, losses)
}

/// The flat import report's lines about the paths of the TIFF `bytes`
/// whose 8BIM run is `run` (module doc): what the byte, typed RGB and CMYK
/// opens find. Never decodes pixels.
pub(crate) fn report_lines(bytes: &[u8], run: Option<&[u8]>) -> Vec<String> {
    let mut read = bytes_reader(bytes);
    let Some(directory) = Directory::first(&mut read) else {
        return Vec::new();
    };
    let (Some(width), Some(height)) = (directory.scalar(256), directory.scalar(257)) else {
        return Vec::new();
    };
    file_paths(bytes, run, (width, height)).1
}

/// The 8BIM blocks a flat TIFF writes for `entries` on a `canvas`
/// (`psd_paths::resources`, each block as a PSD writes it).
fn run_of(entries: &[PathEntry], canvas: (u32, u32)) -> Vec<u8> {
    resources(entries, canvas)
        .iter()
        .flat_map(|block| resource_named(block.id, &block.name, &block.body))
        .collect()
}

/// `side` writing `paths` (this save's own path resources) after the
/// inherited run; when there are any, the path resources that run still
/// holds are dropped (module doc, WRITE).
pub(crate) fn attach(side: &mut EncodeSidecar<'_>, paths: Vec<u8>) {
    if !paths.is_empty() {
        side.iptc = side
            .iptc
            .take()
            .and_then(|run| filtered_run(&run, |id| !is_path_resource(id)))
            .filter(|run| !run.is_empty());
    }
    side.paths = paths;
}

impl DrDocument {
    /// This document with the flat TIFF `bytes`' paths (module doc, READ),
    /// its run without any path resource, read or not: none is read from a
    /// file stored rotated.
    pub(crate) fn with_tiff_paths(mut self, bytes: &[u8]) -> Self {
        let canvas = (self.width, self.height);
        let (entries, _) = file_paths(bytes, self.metadata.iptc.as_deref(), canvas);
        if !entries.is_empty() {
            self.paths.set_entries(entries);
        }
        self.metadata.iptc = without_paths(self.metadata.iptc.take());
        self
    }

    /// The path resources this document's flat TIFF writes (module doc,
    /// WRITE).
    pub(crate) fn tiff_path_run(&self) -> Vec<u8> {
        run_of(&self.paths.entries, (self.width, self.height))
    }

    /// Whether this document's flat TIFF keeps every one of its paths and
    /// drops none its run holds (module doc, WRITE): the flat save's "paths"
    /// loss is dropped for a TIFF then.
    pub(crate) fn tiff_keeps_paths(&self) -> bool {
        all_written(&self.paths.entries, (self.width, self.height))
            && !holds_paths(self.metadata.iptc.as_deref())
    }
}

impl PrecisionDocument {
    /// Whether this document's flat TIFF opens read paths and its TIFF
    /// writers write them (module doc): RGB at 8 or 16 bits, and CMYK.
    pub(crate) fn tiff_carries_paths(&self) -> bool {
        self.is_cmyk()
            || !(self.is_float()
                || self.is_gray()
                || self.is_duotone()
                || self.is_lab()
                || self.is_indexed()
                || self.is_bitmap())
    }

    /// This document with the flat TIFF `bytes`' paths (module doc, READ),
    /// its run without any path resource, read or not, and their sentences
    /// joined to the import warnings.
    pub(crate) fn with_tiff_paths(mut self, bytes: &[u8]) -> Result<Self, String> {
        let (entries, lines) =
            file_paths(bytes, self.metadata().iptc.as_deref(), self.dimensions());
        if !entries.is_empty() {
            self.paths.set_entries(entries);
        }
        self.metadata.iptc = without_paths(self.metadata.iptc.take());
        self.with_warning_lines(lines)
    }

    /// The path resources this document's flat TIFF writes (module doc,
    /// WRITE); `None` for a document kind whose TIFF passes the inherited
    /// run as it is.
    pub(crate) fn tiff_path_run(&self) -> Option<Vec<u8>> {
        self.tiff_carries_paths()
            .then(|| run_of(self.paths(), self.dimensions()))
    }

    /// [`DrDocument::tiff_keeps_paths`] for a document kind whose TIFF
    /// carries the paths: a CMYK document's flat save drops "paths" from its
    /// losses for a TIFF then (`precision_preservation`).
    pub(crate) fn tiff_keeps_paths(&self) -> bool {
        self.tiff_carries_paths()
            && all_written(self.paths(), self.dimensions())
            && !holds_paths(self.metadata().iptc.as_deref())
    }
}
