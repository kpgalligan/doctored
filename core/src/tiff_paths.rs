//! PATHS IN A FLAT TIFF: the saved paths, the work path and the clipping
//! path (image resources 2000–2997, 1025 and 2999) in a flat TIFF's tag
//! 34377, the 8BIM run beside the spot channels' resources (`tiff_spot`). A
//! flat TIFF's run holds the same blocks a PSD's resource section does, so
//! the records, their rules and their sentences are `psd_paths`': one
//! reader (`run_paths`) and one writer (`resources`).
//!
//! READ ([`DrDocument::with_tiff_paths`],
//! [`PrecisionDocument::with_tiff_paths`]). Every flat TIFF open — the byte
//! open, the typed RGB and CMYK open, and the Grayscale, Lab, Indexed,
//! Bitmap and 32-bit float opens — reads its paths from the file's run (the
//! document's `metadata.iptc`), each record a fraction of the stored raster,
//! and then takes every path resource out of that run, read or not: the
//! model is the paths' one home from then on, so a deleted path cannot come
//! back from the run and an edited one is not written twice. A file stored
//! rotated (Orientation other than 1) opens upright, where those fractions
//! name other places, so its paths are neither read nor kept, with a
//! sentence. The typed opens join the sentences to the import warnings;
//! the byte document has none, so its sentences are the flat import
//! report's ([`report_lines`]). A multipage TIFF's paths are its first
//! page's (`doc_tiff_pages`).
//!
//! MOVE ([`canvas_moved`], [`moves_canvas`], [`jpeg_opened`]). A run that
//! still holds path resources came from a file whose open does not read
//! them (a JPEG's APP13). Its records are fractions of the raster that file
//! stored, so they survive only an open that did not turn it: a JPEG whose
//! EXIF Orientation is not 1 opens upright, and every JPEG open (the byte,
//! the Grayscale and the CMYK open) takes them out of the run then, with a
//! sentence the import report repeats ([`jpeg_report_line`]). Image Size
//! keeps them right, and every other whole-canvas edit — Crop, Canvas Size,
//! a quarter turn or flip, Rotate Canvas, Straighten, Perspective Crop —
//! leaves them naming other places. Each of those takes them out of the run
//! at both depths, beside the moved paths of the document's own, so no
//! writer passes them on and no TIFF reopen reads them where they no longer
//! belong.
//!
//! WRITE ([`attach`]). Every flat TIFF writer but Duotone's (which writes no
//! flat TIFF of its own) — the byte save and export, the typed RGB save and
//! export, the CMYK save and kept export, the Grayscale, Indexed and Bitmap
//! kept exports, the Lab save, the 32-bit float TIFF, and the untrimmed
//! Index layer of Export Layers, which is the canvas — writes the
//! document's own path resources after the inherited run. A document with
//! no path of its own passes the run's path resources through as before
//! (the canvas they describe is still its own, MOVE), and one with paths
//! drops them, as resource ids would otherwise collide, and its flat save
//! lists "paths" among the losses. Every other writer passes the inherited
//! run as it did. The records are fractions of the canvas, so they stay
//! right at an export's output size; a trimmed layer export holds only a
//! part of the canvas, so it writes none ([`DrDocument::without_canvas_paths`]).
//! A path whose coordinates leave the fixed-point range is not written, and
//! the flat save lists "paths" then too ([`DrDocument::tiff_keeps_paths`]).

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
/// whose 8BIM run is `run` (module doc): what every flat TIFF open finds.
/// Never decodes pixels.
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

/// `metadata` of a JPEG whose open applied EXIF `orientation` (module doc,
/// MOVE): as it is at Orientation 1, else its 8BIM run without path
/// resources and, when it held any, [`jpeg_report_line`]'s sentence.
pub(crate) fn jpeg_opened(metadata: Metadata, orientation: u8) -> (Metadata, Option<String>) {
    match jpeg_report_line(metadata.iptc.as_deref(), orientation) {
        Some(line) => {
            let iptc = without_paths(metadata.iptc.clone());
            (Metadata { iptc, ..metadata }, Some(line))
        }
        None => (metadata, None),
    }
}

/// The flat import report's line about a JPEG whose EXIF orientation is
/// `orientation` and whose 8BIM run is `run` (module doc, MOVE): the
/// sentence of an open that turns the raster and drops the run's path
/// resources, `None` when it keeps them. Never decodes pixels.
pub(crate) fn jpeg_report_line(run: Option<&[u8]>, orientation: u8) -> Option<String> {
    (orientation != 1 && holds_paths(run)).then(|| {
        format!(
            "Paths in this JPEG are not kept: it is stored rotated (Orientation {orientation})."
        )
    })
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

    /// This document for a deliverable that holds only a part of its
    /// canvas, a trimmed layer export (module doc, WRITE): no path of its
    /// own and none in its run, since every record is a fraction of the
    /// whole canvas.
    pub(crate) fn without_canvas_paths(&self) -> Self {
        let mut out = self.clone();
        out.paths.set_entries(Vec::new());
        out.metadata = canvas_moved(&self.metadata);
        out
    }
}

impl PrecisionDocument {
    /// Whether this document's flat TIFF opens read paths and its TIFF
    /// writers write them (module doc): every kind but Duotone, which has
    /// no flat TIFF of its own.
    pub(crate) fn tiff_carries_paths(&self) -> bool {
        !self.is_duotone()
    }

    /// [`DrDocument::without_canvas_paths`] at the typed depths.
    pub(crate) fn without_canvas_paths(mut self) -> Self {
        self.paths.set_entries(Vec::new());
        self.metadata = canvas_moved(&self.metadata);
        self
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
    /// carries the paths: its flat save drops "paths" from its losses for a
    /// TIFF then (`precision_preservation`).
    pub(crate) fn tiff_keeps_paths(&self) -> bool {
        self.tiff_carries_paths()
            && all_written(self.paths(), self.dimensions())
            && !holds_paths(self.metadata().iptc.as_deref())
    }
}
