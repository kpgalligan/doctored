//! The spot channels' overprint on a TYPED composite (#547 1c-1,
//! `designs/spot-channels.md`): the adapter between a typed
//! document's spot channels and the `spot_overprint` kernel, for the
//! flattened exports (`doc_export`'s typed `export_encoded`) and Merge Spot
//! Channel (`spot_merge`). Each listed spot channel runs once, in list
//! order, its coverage the channel's own u16 plane (65535 = full ink). A
//! CMYK document overprints in ink instead (`spot_cmyk`).
//!
//! Per mode:
//!
//! * **RGB 16 / 32-bit**: the ink encoded under the composite's profile
//!   (`spot_view::ink_encoded`, the byte view's rule), then
//!   `overprint_rgba16` or `overprint_rgba32f` by storage; a float
//!   composite's range above white survives.
//! * **Duotone**: the caller passes the RGB view (`duotone_view_job`), so
//!   the RGB rule under the view's profile — the inks print over what the
//!   Duotone inks show.
//! * **Grayscale**: the ink's GRAY, Lab → Grayscale's take-confirmed rule
//!   (`lab_gray`: `L*` alone, `Y = L*⁻¹(L*)`, the Gray profile's tone curve),
//!   written R = G = B, so a Gray composite stays gray.
//! * **Lab**: the Lab codes have no RGB of their own, so the composite runs
//!   through the built-in ProPhoto RGB (the Lab colour operations' space,
//!   `lab_adjust::color_space`, relative colorimetric, no BPC) with
//!   `lab_rgb::lab_to_rgb_image`, takes the RGB rule there, and comes back
//!   through `lab_convert::to_lab_pipeline` / `converted_image` at the
//!   document's depth. That round trip clips and requantizes, so the result
//!   takes the converted-back sample ONLY where some listed channel's
//!   coverage is above 0 and keeps the original Lab codes everywhere else: a
//!   coverage of 0 leaves the pixel exact, the kernel's own promise.
//!
//! # Memory
//!
//! [`PrecisionDocument::spot_overprint_scratch_bytes`]: the canvas at the
//! arm's per-pixel constant (the owned copy the kernel writes and the
//! rebuilt image's tiles; Lab adds its two conversions and their images)
//! plus Lab's two prepared pipelines and bookkeeping. Held by
//! `scripts/spot-admission/measure.py`; a miss raises only the arm's own
//! constant here.

use crate::doc_export::{ExportRequest, ExportResult};
use crate::icc::IccProfile;
use crate::icc_intent::RenderingIntent;
use crate::job_control::JobContext;
use crate::lab_document::lab_twin;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{pixel_count, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES};
use crate::spot_channel::NOT_SPOT_REFUSAL;
use crate::spot_overprint::{overprint_rgba16, overprint_rgba32f};
use crate::spot_view::ink_encoded;

/// Bytes a canvas pixel the RGB16 (and Duotone view) arm charges: the owned
/// u16 copy the kernel writes (8) and the rebuilt image's tiles (8).
/// `merge-rgb16` / `export-rgb16-png16`; a miss raises this term.
pub(crate) const RGB16_BYTES_PER_PIXEL: u64 = 16;

/// The same for the f32 arm (16 + 16). `export-float-exr32`; a miss raises
/// this term.
pub(crate) const FLOAT_BYTES_PER_PIXEL: u64 = 32;

/// The same for the Gray arm, at the Gray storage's u16 (8 + 8); a 32-bit
/// Gray composite charges [`FLOAT_BYTES_PER_PIXEL`].
pub(crate) const GRAY_BYTES_PER_PIXEL: u64 = 16;

/// The same for the Lab arm, whose stages each drop their input before the
/// next begins: at most three canvas planes at once (8 each) — the ProPhoto
/// image, the owned copy the kernel writes and its rebuilt tiles; then the
/// printed image, the converted-back image and its owned samples; then
/// those samples, the original codes' copy and the result's tiles.
/// `merge-lab16` (24 MP, Release: E 581 MB against a 1.93 GB plan at the
/// first draft's 48 B/px); a miss raises this term.
pub(crate) const LAB_BYTES_PER_PIXEL: u64 = 24;

/// One spot channel's part in a typed overprint: its coverage plane, the
/// ink in the composite's encoding and the solidity as a unit.
struct Pass<'a> {
    coverage: &'a [u16],
    ink: [f32; 3],
    solidity: f32,
}

/// The ProPhoto RGB the Lab arm runs in (module doc).
fn lab_space() -> std::sync::Arc<IccProfile> {
    IccProfile::prophoto_rgb()
}

impl PrecisionDocument {
    /// The indices of the spot channels, in list order.
    pub(crate) fn spot_indices(&self) -> Vec<usize> {
        self.channels()
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.metadata().spot.map(|_| i))
            .collect()
    }

    /// `composite` (a canvas-sized render of this document, or a Duotone
    /// document's RGB view) with spot channels `channels` overprinted in
    /// list order (module doc). `Err` for an index that is not a spot
    /// channel, a CMYK document, a composite that is not canvas-sized, and
    /// a cancel. Admission belongs to the caller
    /// ([`Self::spot_overprint_scratch_bytes`]).
    pub(crate) fn spot_overprinted(
        &self,
        composite: &PrecisionImage,
        channels: &[usize],
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        if self.is_cmyk() {
            return Err("a CMYK document overprints its spot channels in ink".into());
        }
        if composite.dimensions() != self.dimensions() {
            return Err("the composite does not match this document's canvas".into());
        }
        if self.is_lab() {
            return self.lab_overprinted(composite, channels, job);
        }
        let gray = self.gray_policy().filter(|_| !self.is_duotone());
        let mut passes = Vec::with_capacity(channels.len());
        for &index in channels {
            let (coverage, ink) = self.spot_plane(index)?;
            let encoded = match gray {
                // Lab → Grayscale's rule (`lab_gray`): L* alone.
                Some(policy) => {
                    let y = crate::icc_lut::lab_to_xyz([ink.lab()[0], 0.0, 0.0])[1];
                    [policy.encode(y).clamp(0.0, 1.0) as f32; 3]
                }
                None => ink_encoded(&ink, Some(composite.profile()))?,
            };
            passes.push(Pass {
                coverage,
                ink: encoded,
                solidity: f32::from(ink.solidity()) / 100.0,
            });
        }
        overprinted(composite, &passes, job)
    }

    /// Channel `index`'s coverage plane and ink; `Err` for an index out of
    /// range or an alpha channel.
    pub(crate) fn spot_plane(
        &self,
        index: usize,
    ) -> Result<(&[u16], crate::spot_channel::SpotInk), String> {
        let channel = self
            .channels()
            .get(index)
            .ok_or("typed channel index is out of range")?;
        let ink = channel.metadata().spot.ok_or(NOT_SPOT_REFUSAL)?;
        Ok((channel.plane().samples(), ink))
    }

    /// The Lab arm (module doc).
    fn lab_overprinted(
        &self,
        composite: &PrecisionImage,
        channels: &[usize],
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        let policy = self
            .lab_policy()
            .ok_or("the Lab arm needs a Lab document")?;
        let space = lab_space();
        let rgb = crate::lab_rgb::lab_to_rgb_image(
            composite,
            &space,
            RenderingIntent::RelativeColorimetric,
            false,
            job.range(0.0, 0.3),
        )?;
        let mut passes = Vec::with_capacity(channels.len());
        let mut planes = Vec::with_capacity(channels.len());
        for &index in channels {
            let (coverage, ink) = self.spot_plane(index)?;
            planes.push(coverage);
            passes.push(Pass {
                coverage,
                ink: ink_encoded(&ink, Some(&space))?,
                solidity: f32::from(ink.solidity()) / 100.0,
            });
        }
        let printed = overprinted(&rgb, &passes, job.range(0.3, 0.5))?;
        drop(rgb);
        // The converted-back image lives only until its owned samples exist,
        // so at most three canvas planes are ever alive at once here.
        let back = {
            let image = crate::lab_convert::converted_image(
                &printed,
                &crate::lab_convert::to_lab_pipeline(&space)?,
                composite.profile(),
                &|unit| policy.code(unit),
                job.range(0.5, 0.8),
            )?;
            drop(printed);
            image.to_rgba16()?
        };
        let mut samples = composite.to_rgba16()?;
        for (p, sample) in samples.iter_mut().enumerate() {
            job.every(p)?;
            if planes.iter().any(|plane| plane[p] > 0) {
                *sample = back[p];
            }
        }
        drop(back);
        let (w, h) = composite.dimensions();
        composite.rebuilt_rgba16(w, h, samples, composite.profile().clone())
    }

    /// The working memory [`Self::spot_overprinted`] over every spot
    /// channel charges (module doc, "Memory"); 0 without a spot channel.
    /// A CMYK document's is `spot_cmyk`'s.
    pub(crate) fn spot_overprint_scratch_bytes(&self) -> Result<u64, String> {
        if !self.has_spot_channel() {
            return Ok(0);
        }
        if self.is_cmyk() {
            return Ok(self.cmyk_spot_scratch_bytes());
        }
        let (w, h) = self.dimensions();
        let canvas = pixel_count(w, h)? as u64;
        let (per_pixel, pipelines) = if self.is_lab() {
            let space = lab_space();
            let to = crate::lab_rgb::lab_to_rgb_scratch(
                &space,
                RenderingIntent::RelativeColorimetric,
                false,
            )?;
            let back = crate::lab_convert::to_lab_pipeline(&space)?.scratch_bytes();
            (
                LAB_BYTES_PER_PIXEL,
                to + back + lab_twin().bytes().len() as u64,
            )
        } else if self.render_storage() == PrecisionStorage::Rgba32Float {
            (FLOAT_BYTES_PER_PIXEL, 0)
        } else if self.is_gray() && !self.is_duotone() {
            (GRAY_BYTES_PER_PIXEL, 0)
        } else {
            (RGB16_BYTES_PER_PIXEL, 0)
        };
        Ok(canvas.saturating_mul(per_pixel) + pipelines + BOOKKEEPING_BYTES)
    }

    /// Stages 1–5 of a flattened export over `source` (this document's
    /// canvas render) with every spot channel overprinted first: a Duotone
    /// document overprints its RGB view and runs the RGB document's stages,
    /// every other mode overprints the render and runs its own. Admission
    /// and the job token belong to the caller (`export_encoded`, whose plan
    /// adds [`Self::spot_overprint_scratch_bytes`]).
    pub(crate) fn spot_export_stages(
        &self,
        source: &PrecisionImage,
        request: &ExportRequest,
        job: JobContext<'_>,
    ) -> Result<ExportResult, String> {
        let spots = self.spot_indices();
        if self.is_duotone() {
            let view = self.duotone_view_job(source, job)?;
            let printed = self.spot_overprinted(&view, &spots, job)?;
            drop(view);
            return self.without_gray().export_stages(&printed, request, job);
        }
        // `export_stages`' own Gray refusal, asked before the overprint's copy
        // is built rather than after it.
        if let Some(refusal) = self.gray_export_refusal(request) {
            return Err(refusal);
        }
        let printed = self.spot_overprinted(source, &spots, job)?;
        self.export_stages(&printed, request, job)
    }
}

/// `image` with every pass run by its storage's arm, the result rebuilt
/// over `image`'s tiles (an untouched tile stays shared).
fn overprinted(
    image: &PrecisionImage,
    passes: &[Pass<'_>],
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    let (w, h) = image.dimensions();
    let mismatch = || "a spot channel does not match this document's canvas".to_string();
    match image.storage() {
        PrecisionStorage::Rgba16Unorm => {
            let mut samples = image.to_rgba16()?;
            for pass in passes {
                job.check()?;
                if !overprint_rgba16(
                    samples.as_flattened_mut(),
                    pass.coverage,
                    pass.ink,
                    pass.solidity,
                ) {
                    return Err(mismatch());
                }
            }
            job.check()?;
            image.rebuilt_rgba16(w, h, samples, image.profile().clone())
        }
        PrecisionStorage::Rgba32Float => {
            let mut samples = image.to_rgba32()?;
            for pass in passes {
                job.check()?;
                if !overprint_rgba32f(
                    samples.as_flattened_mut(),
                    pass.coverage,
                    pass.ink,
                    pass.solidity,
                ) {
                    return Err(mismatch());
                }
            }
            job.check()?;
            image.rebuilt_rgba32_extended(w, h, samples, image.profile().clone())
        }
    }
}
