//! File orchestration for the internal neutral high-depth document path.
//! Native state and authoritative u16 projections never pass through DrImage.
//! Container I/O and dependency codecs have stage cancellation boundaries;
//! compositing uses the separately cancellable renderer.
//!
//! Core and caller retained state participate in bounded decode/render/encode plans.

use std::fs::File;
use std::io::Read;

use crate::job_control::{DrJobControl, JobContext};
use crate::precision_codec::{self, PrecisionFormat};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, BOOKKEEPING_BYTES};
use crate::resources::Inventory;

impl PrecisionDocument {
    /// Peak additional memory for render followed by either 16-bit writer.
    /// The document remains retained through both stages. The standalone codec
    /// also charges its borrowed profile/metadata; keep that conservative charge
    /// here so host admission agrees with the actual compound call.
    pub fn export_scratch_bytes(&self) -> Result<u64, String> {
        let plan = self.render_plan()?;
        let (width, height) = self.dimensions();
        let mut profile = Inventory::default();
        profile.object(&**self.profile(), "profile", false);
        self.profile().record_resources(&mut profile);
        let packets: u64 = [
            &self.metadata().exif,
            &self.metadata().xmp,
            &self.metadata().iptc,
        ]
        .into_iter()
        .flatten()
        .map(|bytes| bytes.len() as u64)
        .sum();
        let encoded = precision_codec::encode_dimensions_scratch((width, height))?
            + u64::from(width) * u64::from(height) * 8
            + profile.unique_bytes()
            + packets
            + BOOKKEEPING_BYTES;
        let rendered = plan.scratch_bytes
            + plan
                .unique_retained_bytes
                .saturating_sub(self.retained_bytes());
        // The 16-bit PSD export bakes each styled raster's style into its
        // own pixels; the largest such bake bounds that path.
        let mut baked = 0u64;
        for index in 0..self.layer_count() {
            let layer = self.layer(index).ok_or("invalid typed layer")?;
            if layer.style().is_some() && layer.source().is_some() {
                baked = baked.max(crate::precision_style_render::bake_style16_scratch(
                    layer,
                    self.profile(),
                    (width, height),
                    self.global_light(),
                )?);
            }
        }
        Ok(encoded.max(rendered).max(baked + BOOKKEEPING_BYTES))
    }

    /// Strict preserving import. Refusals never request a platform byte fallback
    /// or an automatic conversion to a host working space.
    pub fn open_preserving(
        path: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        if let Some(control) = control {
            control.checkpoint()?;
        }
        precision_codec::require_regular_input(path)?;
        let mut file = File::open(path).map_err(|e| format!("failed to read {path}: {e}"))?;
        let mut prefix = [0; 4];
        file.read_exact(&mut prefix)
            .map_err(|e| format!("failed to read {path}: {e}"))?;
        drop(file);
        if crate::native_magic::is_native_magic(&prefix) {
            return Self::open_native(path, limit, other, control);
        }
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let mut bytes = precision_codec::read_regular(path, limit, other, job)?;
        // OpenEXR and Radiance RGBE are float containers by their signature
        // alone, so each opens as a 32-bit FLOAT document or
        // refuses by its own sentence; neither falls through to the u16 path.
        if crate::exr_float::is_exr(&bytes) {
            return crate::float_io::from_exr(&bytes, path, limit, other, job);
        }
        if crate::radiance_hdr::is_radiance(&bytes) {
            return crate::float_io::from_radiance(&bytes, path, limit, other, job);
        }
        // A TIFF holding more than one image opens each page through the
        // route below, one page at a time (`tiff_pages`, `doc_tiff_pages`).
        if crate::metadata_tiff::is_tiff(&bytes) {
            if let Some(pages) = crate::tiff_pages::read(&bytes) {
                return Self::open_tiff_pages(&mut bytes, &pages, path, limit, other, job);
            }
        }
        Self::open_flat_bytes(&bytes, path, limit, other, job, true)
    }

    /// The float TIFF, CMYK and 16-bit routes of [`Self::open_preserving`]
    /// over bytes already read: a one-image file, or one page of a
    /// multipage TIFF made the file's only page (`tiff_pages::with_page`).
    /// `spots` reads a TIFF's spot channels into the document; a later page
    /// of a multipage file is opened without them.
    pub(crate) fn open_flat_bytes(
        bytes: &[u8],
        path: &str,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
        spots: bool,
    ) -> Result<Self, String> {
        // A genuine floating-point TIFF opens as a 32-bit FLOAT document
        // (Phase 11F). `tiff_float::inspect` is header-only, and every
        // integer TIFF fails it by naming its SampleFormat, so the u16 path
        // below keeps its own refusals verbatim.
        if crate::metadata_tiff::is_tiff(bytes) && crate::tiff_float::inspect(bytes, path).is_ok() {
            return crate::float_io::from_float_tiff(bytes, path, limit, other, job);
        }
        // A CMYK TIFF or JPEG opens as a CMYK document (#1) — exactly the
        // files `cmyk_source::cmyk_source` answers for, so an untagged one with
        // no working CMYK profile set keeps the path below unchanged.
        if matches!(
            crate::cmyk_source::cmyk_source(bytes),
            Some(crate::cmyk_source::CmykSource::Tiff | crate::cmyk_source::CmykSource::Jpeg)
        ) {
            return Self::open_cmyk_file(bytes, path, limit, other, job);
        }
        let length = bytes.len() as u64;
        let decoded = precision_codec::decode_with_context(bytes, path, limit, other, length, job)?;
        let doc = Self::from_image(decoded.image, decoded.metadata, decoded.resolution)?
            .with_import_warnings(decoded.warnings)?;
        // A TIFF's spot channels (#1 1c-2a, `tiff_spot`) and paths
        // (`tiff_paths`).
        if spots && crate::metadata_tiff::is_tiff(bytes) {
            doc.with_tiff_spots(bytes, limit, other.saturating_add(length))?
                .with_tiff_paths(bytes)
        } else {
            Ok(doc)
        }
    }

    /// Capture-style export renders this immutable source in u16, then writes
    /// it through the shared metadata policy. No preview argument is accepted.
    /// Returned CARRIES bits identify the packets actually written, so a host
    /// can compare them with the source's packets and disclose any omission.
    pub fn export16(
        &self,
        path: &str,
        format: PrecisionFormat,
        embed_profile: bool,
        strip_metadata: bool,
        limit: u64,
        other: u64,
    ) -> Result<u32, String> {
        // This compound operation intentionally promises no live cancellation:
        // codecs cannot poll within their dependency calls. The independently
        // exposed projection operation remains cooperatively cancellable.
        if self.is_float() {
            return Err(crate::float_document::refused_sentence(
                "Export at 16 bits without a tone map",
            ));
        }
        if self.is_cmyk() {
            return Err(crate::cmyk_document::refused_sentence(
                "Export as a 16-bit RGB file",
            ));
        }
        let retained = self.retained_bytes();
        admit(retained, self.export_scratch_bytes()?, other, limit)?;
        let image = self.render_with_context(limit, other, JobContext::NONE)?;
        let other = other
            .checked_add(retained)
            .ok_or("precision export memory overflow")?;
        // A TIFF carries the paths (`tiff_paths`).
        precision_codec::save_with_paths(
            &image,
            self.metadata(),
            self.resolution(),
            path,
            format,
            embed_profile,
            strip_metadata,
            (limit, other),
            None,
            self.tiff_path_run(),
        )
    }
}
