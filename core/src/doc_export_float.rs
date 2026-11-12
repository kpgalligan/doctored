//! The 32-bit FLOAT deliverable stage of the export workspace:
//! a float document's extended-range samples written to a float TIFF, an
//! OpenEXR file or a Radiance RGBE file, and the preview of what was written.
//!
//! `doc_export::export_stages` hands over here as soon as a float document
//! meets a 32-bit request, with the composite already resampled when a size
//! asked for it. No other stage of the ordinary chain runs, which is why
//! `ExportRequest::validate` refuses every field that would ask one to
//! (sharpen, profile, dither).
//!
//! # No pixel math
//!
//! The container, the sample width and the alpha convention are `float_io`'s
//! ([`crate::float_io::encode_float`]); the sidecar is the shared
//! `image_sidecar_with_policy`, which for OpenEXR and Radiance carries
//! nothing because neither container has a slot for a profile or a packet
//! (`format_carries` is 0 for both — the file states the document profile's
//! PRIMARIES instead). A 32-bit Grayscale document that keeps gray (#1
//! phase 3c; a TIFF, the one container that gets here kept) writes one or two
//! samples through `float_io::encode_float_gray` with its Gray ICC embedded
//! in place of the twin's, and its preview reads them as R = G = B.
//!
//! # The exact float TIFF
//!
//! A 32-bit float TIFF at the canvas size, of a document whose render only
//! re-derives its one layer, is written from that layer's own samples
//! ([`PrecisionDocument::exact_float_source`]): `export_encoded` then neither
//! renders nor reads a supplied projection. The render composites a
//! translucent pixel over the transparent canvas through one multiplication
//! and one division by its alpha, which can move a colour sample by an ulp
//! (0.925 came back 0.92499995 at alpha 0.6), so the TIFF File ▸ Save keeps in
//! place would not reopen as the document its empty flat-save list says it
//! does. The layer qualifies when it is the one root, a raster at (0, 0) the
//! canvas's size, visible, opacity 1, Normal, unclipped, with no mask, style
//! or Blend If, in a document with no spot channel; and when no pixel at
//! alpha 0 holds a colour, which the render writes as zero and a
//! straight-alpha TIFF would keep (`precision_preservation::alpha_facts`, the
//! flat-save list's own test). A 32-bit Gray document's kept-gray TIFF takes
//! the same source. An alpha below the render's 2^-96 floor, which the render
//! refuses, is written as stored. OpenEXR and Radiance keep the render: their
//! writers multiply by alpha and their opens divide, so the layer would not
//! make them exact.
//!
//! # Admission
//!
//! Every container charges `export_plan_for`'s existing 32-bit term (the
//! flat f32 sample vector plus the encoder's buffer, `source·16·2 + 1 MiB`),
//! so this stage adds no plan of its own. Measured on a
//! 6000×4000 float document with its projection supplied
//! (`tests/float_interchange_24mp.rs::measure`: Release, one case per
//! process, `MallocLargeCache=0`, two sets of three runs, a baseline twin
//! reading 0), against the one plan of 2,440,410,880: OpenEXR f32 max E
//! 421,757,288, OpenEXR half 401,736,016, Radiance 589,283,760, half TIFF
//! 607,879,624.
//! The largest is 0.2491 of the plan, so the term stands; that test pins the
//! floors (⌈1.25 × max E⌉).
//!
//! # The preview
//!
//! `ExportResult::decode_preview` promises a decode of the bytes that will be
//! written. For a float deliverable that means the container's own seam, not
//! the 8-bit `DrImage::open_bytes` (which clamps a float TIFF and refuses the
//! other two): [`FloatDeliverable::preview`] decodes the result's bytes,
//! undoes the container's alpha convention exactly as the open does, and
//! shows them through the EXPORTING document's tone map and profile curve —
//! the one encode loop, `float_document::encoded_u16_gained`, then the
//! undithered quantizer, which is `display_rgba8`'s own pair. A half or RGBE
//! export therefore previews its own rounding.
//!
//! The preview's memory is charged to a plan before anything is decoded: it
//! inspects the bytes and admits, at the process working limit, the OPEN's
//! plan for them (`float_io::admit_open`: the file's bytes, the seam's own
//! scratch, 16 B/px of tiles and 1 MiB) plus the display's
//! (`float_document::display_scratch`: 12 B/px and 1 MiB for the tone-mapped
//! u16 copy and the RGBA8), and decodes at the seam's figure, so a preview
//! the limit cannot hold refuses with the open's capacity sentence instead of
//! decoding unbounded. The stages: the tile plane fills while the decoded
//! samples are held (the open's own peak), the samples are dropped, the u16
//! codes are made beside the tiles, the tiles are dropped, and the RGBA8 is
//! quantized from the codes.
//!
//! Measured at 6000×4000 (2026-10-12, `tests/float_interchange_24mp.rs::
//! measure`, cases `preview_exr`, `preview_hdr` and `preview_tiff`: Release,
//! one case per process, `MallocLargeCache=0`, two sets of three runs, a
//! baseline twin reading 0; E counts the file's bytes, read inside the
//! interval):
//!
//! | file | bytes | max E | open plan | preview plan | preview plan / max E |
//! |---|---|---|---|---|---|
//! | OpenEXR f32 | 31,603,599 | 770,769,472 | 1,185,700,751 | 1,474,749,327 | 1.913 |
//! | Radiance | 12,786,251 | 783,680,088 | 974,883,403 | 1,263,931,979 | 1.613 |
//! | float TIFF | 78,395,286 | 849,379,952 | 1,232,492,438 | 1,521,541,014 | 1.791 |
//!
//! The display's term is there because the open's plan alone missed on
//! Radiance: ⌈1.25 × 783,680,088⌉ = 979,600,110 against 974,883,403. There
//! E is the file's bytes plus 32.1 B/px, which that plan's 40 B/px covers at
//! 5/4 with nothing left for the file's own bytes. That test pins the
//! OpenEXR and Radiance floors.
//!
//! # The HDR HEIC and AVIF
//!
//! A 32-bit `heic` or `avif` request with `hdr_encoding` is PENDING, like
//! every host-encoded export: this stage writes the intermediate and the host
//! encodes it (the gain map through Core Image's `heifRepresentation` with
//! `.hdrImage` and `.hdrGainMapAsRGB`, PQ through ImageIO's
//! `kCGImageDestinationEncodeToISOHDR`).
//!
//! - `gain_map`: the intermediate is the `"tiff"` 32-bit request's bytes —
//!   f32 linear light with the document profile — which the host relabels in
//!   that profile's extended-linearized space and hands to Core Image with its
//!   content headroom.
//! - `pq`: the intermediate is a 16-bit TIFF of BT.2100 PQ codes,
//!   `round(65535 · hdr_transfer::linear_to_pq_signal(M · v))` with `M` the
//!   linear conversion from the document primaries (`float_io::primaries_of`)
//!   to BT.2020 (`float_primaries::conversion`), 1.0 written as 203 cd/m²
//!   (the open's one rule), alpha `round(65535 · a)`, and no ICC (the host
//!   labels it `itur_2100_PQ`).
//!
//! `content_headroom` is `max(1, the largest linear RGB the output holds)`:
//! for a gain map the source's own, for PQ the converted value capped at
//! PQ's 10000 cd/m² (≈ 49.26). Both charge the 32-bit plan term
//! (`source·16·2 + 1 MiB`): the PQ intermediate's u16 codes, their tiles and
//! the TIFF buffer are 24 B/px under its 32.
//!
//! Measured with the host's encode included (the f32 or PQ TIFF re-decoded,
//! Core Image's HDR image, `CIToneMapHeadroom`, the colour gain map and the
//! HEIF write, or ImageIO's PQ HEIC, and the read-back): Release app,
//! `scripts/verify_v1x_hdr_photos.py --measure 3 --measure-phases export`,
//! macOS 26.5.1, `DR_WORKING_LIMIT_BYTES` 8 GiB, `MallocLargeCache=0`, one
//! export per fresh instance after the open settled, a baseline twin per run
//! reading 0 / 0 / 0, E = `ri_interval_max_phys_footprint` − pre once the
//! footprint was still for 2 s. E per run: gain map 12 MP (4032×3024)
//! 523,321,680 / 652,493,208 / 523,190,584; 48 MP (8064×6048) 2,943,944,720 /
//! 2,944,419,856 / 2,943,649,784; PQ 12 MP 439,877,968 / 439,681,360 /
//! 439,746,896; 48 MP 1,755,481,312 / 1,755,792,632 / 1,755,628,768. The
//! whole 32-bit plan (`export_plan_for`: 96 B/px + 16·width + 130 MiB) is
//! 1,306,885,120 at 12 MP and 4,818,466,816 at 48 MP: 2.003× and 1.6365× of
//! the largest E (both gain maps), so the term stands unraised.

use std::sync::Arc;

use crate::blend::BlendMode;
use crate::doc_export::{output_dimensions, ExportRequest, ExportResult};
use crate::float_document::{display_scratch, encoded_u16_gained, sanitize};
use crate::float_document::{FloatPolicy, FLOAT_RANGE_MAX};
use crate::float_io::FloatContainer;
use crate::float_primaries;
use crate::hdr_transfer;
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{process_working_limit, reserved_vec, PrecisionImage};
use crate::precision_preservation::alpha_facts;
use crate::precision_quantize::PrecisionDither;
use crate::tiff_float::FloatSampleType;
use crate::tile_store::TilePlane;
use crate::DrImage;

/// The HDR encoding of a 32-bit HEIC or AVIF (module doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HdrExportEncoding {
    GainMap,
    Pq,
}

impl HdrExportEncoding {
    /// The request and `info_json` spelling.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::GainMap => "gain_map",
            Self::Pq => "pq",
        }
    }
}

/// What an HDR HEIC or AVIF result reports: its encoding and the content
/// headroom the host's encode declares.
#[derive(Clone, Copy, Debug)]
pub(crate) struct HdrDeliverable {
    pub(crate) encoding: HdrExportEncoding,
    pub(crate) content_headroom: f64,
}

/// What a float deliverable keeps beside its bytes: the container and sample
/// form it was written in, and the exporting document's tone policy and
/// profile, which its preview is shown under.
#[derive(Clone)]
pub(crate) struct FloatDeliverable {
    container: FloatContainer,
    samples: FloatSampleType,
    policy: Arc<FloatPolicy>,
    profile: Arc<IccProfile>,
}

impl FloatDeliverable {
    /// `info_json`'s `float_samples`: `f32`, `f16`, or `rgbe` for Radiance,
    /// whose one sample form is neither.
    pub(crate) fn samples_name(&self) -> &'static str {
        match (self.container, self.samples) {
            (FloatContainer::Radiance, _) => "rgbe",
            (_, FloatSampleType::F16) => "f16",
            (_, FloatSampleType::F32) => "f32",
        }
    }

    /// The written `bytes` as display RGBA8, admitted as the open of those
    /// bytes is (module doc, "The preview").
    pub(crate) fn preview(&self, bytes: &[u8]) -> Result<DrImage, String> {
        const LABEL: &str = "the encoded export";
        let admitted = |dimensions: (u32, u32), seam: u64| {
            let scratch = seam
                .checked_add(display_scratch(dimensions.0, dimensions.1)?)
                .ok_or("float export preview working-memory calculation overflow")?;
            crate::float_io::admit_open(
                self.container,
                bytes,
                LABEL,
                dimensions,
                scratch,
                process_working_limit(),
                0,
                JobContext::NONE,
            )
        };
        // (width, height, samples per pixel, associated alpha, samples)
        let (width, height, channels, associated, mut samples) = match self.container {
            FloatContainer::Tiff => {
                let plan = crate::tiff_float::inspect(bytes, LABEL)?;
                admitted((plan.width, plan.height), plan.scratch_bytes)?;
                let decoded = crate::tiff_float::decode(bytes, LABEL, plan.scratch_bytes)?;
                (
                    decoded.width,
                    decoded.height,
                    usize::from(decoded.channels),
                    decoded.alpha == Some(true),
                    decoded.samples,
                )
            }
            FloatContainer::Exr => {
                let plan = crate::exr_float::inspect(bytes, LABEL)?;
                admitted((plan.width, plan.height), plan.scratch_bytes)?;
                let decoded = crate::exr_float::decode(bytes, LABEL, plan.scratch_bytes)?;
                (
                    decoded.plan.width,
                    decoded.plan.height,
                    4,
                    true,
                    decoded.samples,
                )
            }
            FloatContainer::Radiance => {
                let plan = crate::radiance_hdr::inspect(bytes, LABEL)?;
                admitted((plan.width, plan.height), plan.scratch_bytes)?;
                let (plan, samples) =
                    crate::radiance_hdr::decode(bytes, LABEL, plan.scratch_bytes)?;
                (plan.width, plan.height, 3, false, samples)
            }
            FloatContainer::HostStill => {
                return Err("an HDR still is a float source, not a float export container".into())
            }
        };
        if !(1..=4).contains(&channels) {
            return Err(
                "a 32-bit float export preview requires gray, gray and alpha, RGB or RGBA samples"
                    .into(),
            );
        }
        sanitize(&mut samples, channels);
        let stride = width as usize;
        let fill_row = |y: u32, row: &mut [[f32; 4]]| {
            let first = y as usize * stride;
            for (x, pixel) in row.iter_mut().enumerate() {
                let at = (first + x) * channels;
                // A kept-gray TIFF (#1 phase 3c) is R = G = B.
                let (rgb, a) = match channels {
                    1 => ([samples[at]; 3], 1.0),
                    2 => ([samples[at]; 3], samples[at + 1]),
                    3 => ([samples[at], samples[at + 1], samples[at + 2]], 1.0),
                    _ => (
                        [samples[at], samples[at + 1], samples[at + 2]],
                        samples[at + 3],
                    ),
                };
                // `float_io`'s open rule: an associated file divides, and
                // alpha 0 has no colour to recover.
                let rgb = match (associated, a > 0.0) {
                    (true, true) => rgb.map(|v| v / a),
                    (true, false) => [0.0; 3],
                    (false, _) => rgb,
                };
                let rgb = rgb.map(|v| v.clamp(-FLOAT_RANGE_MAX, FLOAT_RANGE_MAX));
                *pixel = [rgb[0], rgb[1], rgb[2], a];
            }
            Ok(())
        };
        let plane = TilePlane::from_rows_with_job(width, height, fill_row, JobContext::NONE)?;
        drop(samples);
        let image = PrecisionImage::from_rgba32_tiles_decoded(plane, self.profile.clone())?;
        let mapped = encoded_u16_gained(&self.policy, &image, 1.0, JobContext::NONE)?;
        drop(image);
        Ok(DrImage {
            pixels: mapped.quantize_with_context(PrecisionDither::None, JobContext::NONE)?,
        })
    }
}

impl PrecisionDocument {
    /// [`FloatDeliverable::preview`] of `bytes`, a float export in
    /// `request`'s container, under this document's tone map and profile: the
    /// seam `tests/float_interchange_24mp.rs` measures the preview's memory
    /// through without exporting first.
    #[doc(hidden)]
    pub fn float_export_preview(
        &self,
        bytes: &[u8],
        request: &ExportRequest,
    ) -> Result<DrImage, String> {
        let container = request
            .float_container()
            .ok_or("a 32-bit float export requires tiff, exr or hdr")?;
        let policy = self
            .float
            .clone()
            .ok_or("a 32-bit float export requires a 32-bit float document")?;
        FloatDeliverable {
            container,
            samples: request.float_samples,
            policy,
            profile: self.profile().clone(),
        }
        .preview(bytes)
    }

    /// The one layer whose samples this document's render only re-derives,
    /// when `request` writes the samples as they are: a 32-bit float TIFF at
    /// the canvas size (module doc, "The exact float TIFF"). `None` for any
    /// other request or document, whose export encodes the render.
    pub(crate) fn exact_float_source(&self, request: &ExportRequest) -> Option<&PrecisionImage> {
        let canvas = self.dimensions();
        if !self.is_float()
            || request.bit_depth != 32
            || request.float_container() != Some(FloatContainer::Tiff)
            || output_dimensions(canvas, request).ok() != Some(canvas)
            || self.has_spot_channel()
        {
            return None;
        }
        let [layer] = self.root_layers() else {
            return None;
        };
        let Content::Raster { image, offset } = &layer.content else {
            return None;
        };
        let rendered_as_stored = *offset == (0, 0)
            && image.dimensions() == canvas
            && layer.visible
            && layer.opacity == 1.0
            && layer.blend_mode == BlendMode::Normal
            && !layer.clipped
            && layer.mask.is_none()
            && layer.style.is_none()
            && layer.blend_if.is_none();
        (rendered_as_stored && !alpha_facts(layer).1).then_some(image)
    }

    /// The 32-bit deliverable: the float projection's own samples (already
    /// resampled when a size asked for it), written in the request's
    /// container and sample width with the document's profile and packets
    /// where the container can carry them; a kept-gray TIFF of a 32-bit Gray
    /// document as one or two samples under its Gray profile (module doc).
    pub(crate) fn export_float(
        &self,
        source: &PrecisionImage,
        request: &ExportRequest,
        job: JobContext<'_>,
    ) -> Result<ExportResult, String> {
        if let Some(encoding) = request.hdr_encoding {
            return self.export_hdr(source, request, encoding, job);
        }
        let container = request
            .float_container()
            .ok_or("a 32-bit float export requires tiff, exr or hdr")?;
        let policy = self
            .float
            .clone()
            .ok_or("a 32-bit float export requires a 32-bit float document")?;
        let (w, h) = source.dimensions();
        let profile = self.profile();
        let (mut side, carried, outcome) = crate::doc_color::image_sidecar_with_policy(
            profile,
            self.metadata(),
            self.resolution(),
            request.encode_format(),
            request.embed_profile,
            request.strip_metadata,
            (w, h),
            &request.metadata,
        );
        job.check()?;
        // A TIFF carries the paths in its 8BIM run (`tiff_paths`).
        if let Some(paths) = self
            .tiff_path_run()
            .filter(|_| container == FloatContainer::Tiff)
        {
            crate::tiff_paths::attach(&mut side, paths);
        }
        // A kept-gray deliverable is a TIFF only (`gray_export_refusal` refuses EXR and HDR first);
        // the container is checked here too, so a skipped refusal can never mislabel TIFF bytes.
        let kept = self
            .gray_policy()
            .filter(|_| self.keeps_gray(request) && container == FloatContainer::Tiff);
        let (bytes, name) = match kept {
            Some(gray) => {
                // The sidecar decided WHETHER a profile is embedded; the Gray
                // profile's bytes are what is embedded (`gray_export`'s rule).
                if side.icc.is_some() {
                    side.icc = Some(gray.bytes());
                }
                let bytes =
                    crate::float_io::encode_float_gray(source, request.float_samples, &side, job)?;
                (bytes, gray.name().to_string())
            }
            None => (
                crate::float_io::encode_float(
                    source,
                    container,
                    request.float_samples,
                    &side,
                    job,
                )?,
                profile.name().to_string(),
            ),
        };
        job.report(1.0, 5)?;
        ExportResult::float_deliverable(
            bytes,
            (w, h),
            carried,
            request,
            name,
            outcome,
            FloatDeliverable {
                container,
                samples: request.float_samples,
                policy,
                profile: profile.clone(),
            },
        )
    }

    /// The pending intermediate of a 32-bit HDR HEIC or AVIF (module doc,
    /// "The HDR HEIC and AVIF").
    fn export_hdr(
        &self,
        source: &PrecisionImage,
        request: &ExportRequest,
        encoding: HdrExportEncoding,
        job: JobContext<'_>,
    ) -> Result<ExportResult, String> {
        if self.float.is_none() {
            return Err("a 32-bit float export requires a 32-bit float document".into());
        }
        let runs = source
            .runs_rgba32()
            .ok_or("a 32-bit float export requires rgba32_float storage")?;
        let (w, h) = source.dimensions();
        let profile = self.profile();
        let (side, carried, outcome) = crate::doc_color::image_sidecar_with_policy(
            profile,
            self.metadata(),
            self.resolution(),
            request.encode_format(),
            request.embed_profile && encoding == HdrExportEncoding::GainMap,
            request.strip_metadata,
            (w, h),
            &request.metadata,
        );
        job.check()?;
        let (bytes, content_headroom) = match encoding {
            HdrExportEncoding::GainMap => {
                let peak = runs
                    .flatten()
                    .fold(1.0_f32, |m, p| m.max(p[0]).max(p[1]).max(p[2]));
                let bytes = crate::float_io::encode_float(
                    source,
                    FloatContainer::Tiff,
                    FloatSampleType::F32,
                    &side,
                    job,
                )?;
                (bytes, f64::from(peak))
            }
            HdrExportEncoding::Pq => {
                let primaries = crate::float_io::primaries_of(profile)?;
                let matrix = float_primaries::conversion(&primaries, &float_primaries::BT2020)
                    .ok_or("the document profile's primaries do not describe a colour basis")?;
                let ceiling = 10000.0 / hdr_transfer::REFERENCE_WHITE_NITS;
                let count = w as usize * h as usize;
                let mut codes = reserved_vec(count)?;
                let mut peak = 1.0_f64;
                for (i, pixel) in runs.flatten().enumerate() {
                    job.every(i)?;
                    let linear = crate::adjust_white_balance::mat_vec(
                        matrix,
                        [pixel[0], pixel[1], pixel[2]].map(f64::from),
                    );
                    for v in linear {
                        peak = peak.max(v.min(ceiling));
                    }
                    let signal = hdr_transfer::linear_to_pq_signal(linear);
                    let code = |v: f64| (v * 65535.0).round().clamp(0.0, 65535.0) as u16;
                    codes.push([
                        code(signal[0]),
                        code(signal[1]),
                        code(signal[2]),
                        code(f64::from(pixel[3].clamp(0.0, 1.0))),
                    ]);
                }
                let image =
                    PrecisionImage::from_rgba16_unseeded(w, h, codes, profile.clone(), None)?;
                let mut writer = std::io::Cursor::new(Vec::new());
                crate::precision_codec::encode(
                    &image,
                    &mut writer,
                    crate::precision_codec::PrecisionFormat::Tiff,
                    &side,
                    job,
                )?;
                (writer.into_inner(), peak)
            }
        };
        job.report(1.0, 5)?;
        ExportResult::hdr_intermediate(
            bytes,
            (w, h),
            carried,
            request,
            profile.name().to_string(),
            outcome,
            HdrDeliverable {
                encoding,
                content_headroom,
            },
        )
    }
}
