//! Multipage TIFF opens: the thin adapters over `tiff_pages`, one per
//! document kind. Each opens page 1 through its ordinary one-page open
//! with page 1 made the file's only page, then each further admitted page
//! the same way, and stacks those pages above it as layers "Page n", hidden,
//! at the canvas's top-left; `tiff_pages`' module doc states the rules. The
//! typed adapter, [`PrecisionDocument::stack_tiff_pages`], takes the
//! one-page open as a parameter, so the RGB, float and CMYK opens
//! (`open_preserving`) and the 8/16-bit Grayscale and Lab Color opens
//! (`gray_io`, `lab_tiff_read`) stack pages by the same code.
//!
//! A page that passes the header rule but fails its own decode fails the
//! open, naming the page (`page 3 of <path>: <the decoder's sentence>`), so
//! no page is dropped without a sentence. The byte open's sentences reach the
//! user through the import report (`dr_path_metadata_import_report`), the
//! typed open's through its import warnings.

use std::sync::Arc;

use crate::doc::DrDocument;
use crate::dr_image::DrImage;
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument};
use crate::tiff_pages::{with_page, Pages};

/// The layer name of page `number`.
fn page_name(number: usize) -> String {
    format!("Page {number}")
}

/// A later page's failure, named.
fn page_error(number: usize, path: &str, error: String) -> String {
    format!("page {number} of {path}: {error}")
}

impl DrDocument {
    /// `DrDocument::open` of a TIFF `tiff_pages::read` found more than one
    /// image in (module doc). `bytes` is restored before this returns.
    /// `job` is checked before each page and polled inside each page's
    /// decode (`DrImage::open_bytes`); a cancel answers exactly
    /// `operation cancelled`, never a page's sentence.
    pub(crate) fn open_tiff_pages(
        bytes: &mut [u8],
        pages: &Pages,
        path: &str,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let admitted = pages.admitted();
        let mut doc = with_page(bytes, pages, admitted[0], |page| {
            Self::open_flat(page, path, job)
        })?;
        if admitted.len() == 1 {
            return Ok(doc);
        }
        if let Some(first) = doc.layers.first_mut() {
            first.name = page_name(1);
        }
        for &number in &admitted[1..] {
            job.check()?;
            let decoded = with_page(bytes, pages, number, |page| {
                DrImage::open_bytes(page, path, job)
            })
            .map_err(|e| page_error(number, path, e));
            let (image, _) = crate::cancel_io::settle(job, decoded)?;
            let top = doc.layers.len() - 1;
            let mut next = doc
                .add_image_layer_at(top, image.pixels, &page_name(number), (0, 0))
                .ok_or_else(|| {
                    page_error(
                        number,
                        path,
                        "the document cannot hold another layer of this size".into(),
                    )
                })?;
            if let Some(layer) = next.layers.last_mut() {
                layer.visible = false;
            }
            doc = next;
        }
        Ok(doc)
    }
}

impl PrecisionDocument {
    /// `PrecisionDocument::open_preserving` of a TIFF `tiff_pages::read`
    /// found more than one image in (module doc): [`Self::stack_tiff_pages`]
    /// over its one-page route, which reads spot channels for page 1 alone.
    pub(crate) fn open_tiff_pages(
        bytes: &mut [u8],
        pages: &Pages,
        path: &str,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        Self::stack_tiff_pages(bytes, pages, path, other, |page, other, first| {
            Self::open_flat_bytes(page, path, limit, other, job, first)
        })
    }

    /// The pages of a multipage TIFF as one typed document (module doc).
    /// `open(page, other, first)` is the caller's one-page open over `page`,
    /// the file with one page made its only page; `other` counts the pages
    /// already built as retained, so each later page is admitted by the
    /// open's own plan beside them, and `first` is true for page 1 alone.
    /// Page 1's document sets the canvas, mode, profile, policy, metadata
    /// and channels; a later page gives its one root record. `bytes` is
    /// restored before this returns.
    pub(crate) fn stack_tiff_pages(
        bytes: &mut [u8],
        pages: &Pages,
        path: &str,
        other: u64,
        open: impl Fn(&[u8], u64, bool) -> Result<Self, String>,
    ) -> Result<Self, String> {
        let admitted = pages.admitted();
        let doc = with_page(bytes, pages, admitted[0], |page| open(page, other, true))?;
        let notes = pages.notes();
        if admitted.len() == 1 {
            return doc.adding_import_warnings(notes);
        }
        let mut layers: Vec<_> = doc.root_layers().to_vec();
        let [first] = layers.as_mut_slice() else {
            return Err(page_error(
                1,
                path,
                "the page did not open as one layer".into(),
            ));
        };
        first.name = Arc::new(page_name(1));
        let mut held = doc.retained_bytes();
        for &number in &admitted[1..] {
            let page = with_page(bytes, pages, number, |page| {
                open(page, other.saturating_add(held), false)
            })
            .map_err(|e| page_error(number, path, e))?;
            held = held.saturating_add(page.retained_bytes());
            let mut record = match page.root_layers() {
                [record] => record.clone(),
                _ => {
                    return Err(page_error(
                        number,
                        path,
                        "the page did not open as one layer".into(),
                    ))
                }
            };
            match (&record.content, doc.is_cmyk()) {
                // A raster record takes the document's profile handle: the
                // header rule admitted only a page whose profile bytes are
                // page 1's (`tiff_pages`), and a document holds one
                // captured handle.
                (Content::Raster { image, offset }, false) => {
                    record.content = Content::Raster {
                        image: image.with_profile(doc.profile().clone()),
                        offset: *offset,
                    };
                }
                // A CMYK record carries no profile; the document's policy,
                // page 1's, is the one profile its pages share.
                (Content::CmykRaster { .. }, true) => {}
                _ => {
                    return Err(page_error(
                        number,
                        path,
                        "the page did not open as a layer of this document".into(),
                    ))
                }
            }
            record.name = Arc::new(page_name(number));
            record.visible = false;
            layers.push(record);
        }
        let (width, height) = doc.dimensions();
        let channels = doc.channels().to_vec();
        doc.rebuilt(width, height, layers, channels)?
            .adding_import_warnings(notes)
    }

    /// This document with `notes` after the warnings it already carries.
    fn adding_import_warnings(self, notes: Vec<String>) -> Result<Self, String> {
        if notes.is_empty() {
            return Ok(self);
        }
        let mut warnings = self.import_warnings.as_ref().clone();
        warnings.extend(notes);
        self.with_import_warnings(warnings)
    }
}
