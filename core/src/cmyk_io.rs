//! CMYK files as CMYK documents (#1 phase 1): the TIFF, JPEG and PSD
//! saved-composite opens, the TIFF, PSD and JPEG-intermediate saves, and
//! their plans. A layered CMYK PSD, PSB or TIFF (Import Layers, mode 0) is
//! `psd_import_cmyk`'s (#1 phase 3).
//!
//! # Which files open as CMYK
//!
//! Exactly those `cmyk_source` answers TIFF or JPEG for (its module doc).
//! The embedded CMYK profile is KEPT verbatim (`CmykPolicy`); a file with no
//! usable one takes the working CMYK profile (`working_cmyk`) under
//! [`untagged_sentence`]; a PSD whose embedded profile is unusable is
//! refused by name.
//!
//! Samples are kept as INK (0 = none): a TIFF's CMYK is ink already, read
//! through THE CMYK TIFF reader (`cmyk_tiff_read`: chunky or planar, its
//! transparency kept, extra samples counted and named in a warning), a
//! JPEG's planes come through `cmyk_jpeg::ink_samples` (Adobe-inverted and
//! YCCK files undone there), and a PSD stores `max − ink`, which is flipped
//! here. An 8-bit file stores each code ×257. The EXIF orientation is
//! applied to the samples, as the flat open applies it. A PSD's merged
//! alpha is not read: the composite opens as stored, matted on white, and
//! the import warning says so.
//!
//! # Saves
//!
//! Every save writes the composite flattened onto paper
//! (`cmyk_composite::on_paper`) at the DECLARED depth (8-bit: `round(v /
//! 257)`, exact for every unedited sample): a TIFF through
//! `tiff_encode::encode_cmyk8` / `encode_cmyk16` with the shared metadata
//! policy (`doc_color::image_sidecar`), the CMYK profile embedded on
//! request; a PSD with colour mode 4, four channels, no layer records, the
//! samples inverted and resources 1039/1005 (`psd_export_encode::write_psd`);
//! and a `jpeg_intermediate`, an 8-bit CMYK TIFF the host's ImageIO
//! transcodes to a CMYK JPEG.

use std::io::Cursor;
use std::sync::Arc;

use serde_json::{json, Value};
use tiff::decoder::Decoder;

use crate::cmyk_composite::{codes, on_paper};
use crate::cmyk_document::{CmykDepth, CmykPolicy};
use crate::cmyk_image::{CmykImage, CmykSample};
use crate::job_control::{DrJobControl, JobContext};
use crate::metadata::{Metadata, Resolution};
use crate::open_profile::{cmyk_container, cmyk_gate, CmykContainer};
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, pixel_count, BOOKKEEPING_BYTES};
use crate::psd_container::Container;

/// The sentence an untagged (or unusably tagged) CMYK file opens under.
pub(crate) fn untagged_sentence(name: &str) -> String {
    format!(
        "The image is CMYK with no embedded colour profile, so the working CMYK profile “{name}” was assigned as it was read."
    )
}

/// The policy a file's embedded profile gives, else the working profile's
/// with the untagged sentence; `None` when neither is usable.
pub(crate) fn file_policy(
    icc: Option<&[u8]>,
    depth: CmykDepth,
) -> Option<(CmykPolicy, Option<String>)> {
    if let Some(policy) = icc.and_then(|bytes| CmykPolicy::new(bytes, depth).ok()) {
        return Some((policy, None));
    }
    let working = crate::working_cmyk::working_cmyk_bytes()?;
    let policy = CmykPolicy::new(&working, depth).ok()?;
    let sentence = untagged_sentence(policy.name());
    Some((policy, Some(sentence)))
}

/// Where an upright pixel `(x, y)` reads from in the stored samples.
type SourceOf = dyn Fn(usize, usize) -> (usize, usize);

/// The samples turned upright by an EXIF orientation (module doc).
fn oriented(
    samples: Vec<CmykSample>,
    (w, h): (u32, u32),
    orientation: image::metadata::Orientation,
) -> (Vec<CmykSample>, (u32, u32)) {
    use image::metadata::Orientation::*;
    let (w, h) = (w as usize, h as usize);
    let (dw, dh, map): (usize, usize, Box<SourceOf>) = match orientation {
        NoTransforms => return (samples, (w as u32, h as u32)),
        Rotate90 => (h, w, Box::new(move |x, y| (y, h - 1 - x))),
        Rotate180 => (w, h, Box::new(move |x, y| (w - 1 - x, h - 1 - y))),
        Rotate270 => (h, w, Box::new(move |x, y| (w - 1 - y, x))),
        FlipHorizontal => (w, h, Box::new(move |x, y| (w - 1 - x, y))),
        FlipVertical => (w, h, Box::new(move |x, y| (x, h - 1 - y))),
        Rotate90FlipH => (h, w, Box::new(|x, y| (y, x))),
        Rotate270FlipH => (h, w, Box::new(move |x, y| (w - 1 - y, h - 1 - x))),
    };
    let mut out = Vec::with_capacity(samples.len());
    for y in 0..dh {
        for x in 0..dw {
            let (sx, sy) = map(x, y);
            out.push(samples[sy * w + sx]);
        }
    }
    (out, (dw as u32, dh as u32))
}

/// A one-layer CMYK document from contiguous samples.
fn single_layer(
    samples: Vec<CmykSample>,
    (w, h): (u32, u32),
    policy: CmykPolicy,
    metadata: Metadata,
    resolution: Resolution,
    warnings: Vec<String>,
    job: JobContext<'_>,
) -> Result<PrecisionDocument, String> {
    let row = w as usize;
    let image = CmykImage::from_rows_with_job(
        w,
        h,
        |y, out| {
            let at = y as usize * row;
            out.copy_from_slice(&samples[at..at + row]);
            Ok(())
        },
        job,
    )?;
    drop(samples);
    let leaf = PrecisionDocumentLayer::raster_cmyk("Background", image, (0, 0), 1.0, true)?;
    PrecisionDocument::build_cmyk(
        w,
        h,
        metadata,
        resolution,
        vec![leaf],
        Vec::new(),
        Arc::new(policy),
    )?
    .with_import_warnings(warnings)
}

/// Bytes a canvas pixel the open charges beside the encoded file: the
/// decoded samples (up to 8 B/px), the contiguous CMYKA16 copy (10 B/px) and
/// the tiles (10 B/px), and — the larger — the opened document's display
/// preparation the host runs right after (the document, its view render and
/// display bytes, ≈ 27 B/px). `scripts/cmyk-admission/measure.py` holds every
/// open plan to ⌈1.25 × its measured peak⌉ (28 missed the 8-bit TIFF and the
/// JPEG at 24 MP, 640 MB measured); a miss raises this term.
pub(crate) const OPEN_BYTES_PER_PIXEL: u64 = 35;

/// The open's working memory: [`OPEN_BYTES_PER_PIXEL`] a pixel, the encoded
/// file the open holds, the decoded bytes of a TIFF's transparency and extra
/// samples past its four inks (`extra`, 0 for every other file) and
/// bookkeeping.
fn open_scratch(w: u32, h: u32, encoded: usize, extra: u64) -> Result<u64, String> {
    Ok(pixel_count(w, h)? as u64 * OPEN_BYTES_PER_PIXEL
        + encoded as u64
        + extra
        + BOOKKEEPING_BYTES)
}

/// The warning a CMYK TIFF's unread extra samples give.
pub(crate) fn extra_samples_sentence(extras: u16) -> String {
    format!("This CMYK TIFF has {extras} extra samples (spot or alpha channels) beyond its transparency; they are not read.")
}

impl PrecisionDocument {
    /// A CMYK TIFF or JPEG as a CMYK document (module doc). `Err` for every
    /// file [`cmyk_source`](crate::cmyk_source::cmyk_source) does not answer TIFF or JPEG for.
    pub(crate) fn open_cmyk_file(
        bytes: &[u8],
        path: &str,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let fail = |what: String| format!("failed to decode {path}: {what}");
        let container =
            cmyk_container(bytes).ok_or_else(|| fail("the file holds no CMYK samples".into()))?;
        cmyk_gate(bytes, path, container)?;
        let scan = crate::metadata::scan(bytes);
        let icc = crate::dr_image::embedded_icc(bytes);
        let mut warnings: Vec<String> = Vec::new();
        let (samples, dims, depth) = match container {
            CmykContainer::Tiff => {
                let mut decoder =
                    Decoder::new(Cursor::new(bytes)).map_err(|e| fail(e.to_string()))?;
                let layout = crate::open_profile::cmyk_tiff_layout(&mut decoder, path)?;
                let (w, h) = (layout.width, layout.height);
                let scratch = open_scratch(w, h, bytes.len(), layout.extra_decoded_bytes())?;
                admit(0, scratch, other, limit)?;
                job.reserve(scratch)?;
                let samples = crate::cmyk_tiff_read::read_samples(bytes, path, &layout)?;
                job.check()?;
                // Spot channels kept (#1 1c-2a, `tiff_spot`) are not unread.
                let kept =
                    crate::tiff_spot::tiff_spot_records(bytes, scan.metadata.iptc.as_deref())
                        .map_or(0, |(_, records)| records.kept.len() as u16);
                let extras = layout.extras.saturating_sub(kept);
                if extras > 0 {
                    warnings.push(extra_samples_sentence(extras));
                }
                let depth = if layout.bytes == 1 {
                    CmykDepth::Eight
                } else {
                    CmykDepth::Sixteen
                };
                (samples, (w, h), depth)
            }
            CmykContainer::Jpeg => {
                let (w, h) = crate::cmyk_jpeg::cmyk_dimensions(bytes)
                    .ok_or_else(|| fail("the JPEG is not four-component CMYK or YCCK".into()))?;
                admit(0, open_scratch(w, h, bytes.len(), 0)?, other, limit)?;
                job.reserve(open_scratch(w, h, bytes.len(), 0)?)?;
                let (w, h, ink) =
                    crate::cmyk_jpeg::ink_samples(bytes, path, scan.adobe_transform.is_some())?;
                job.check()?;
                let samples = ink
                    .chunks_exact(4)
                    .map(|s| [s[0], s[1], s[2], s[3], 255].map(|c| u16::from(c) * 257))
                    .collect();
                (samples, (w, h), CmykDepth::Eight)
            }
        };
        let (policy, sentence) = file_policy(icc.as_deref(), depth).ok_or_else(|| {
            fail(
                "the CMYK image has no usable colour profile and no working CMYK profile is set"
                    .into(),
            )
        })?;
        let orientation = crate::metadata_tiff::orientation_of_file(bytes)
            .or_else(|| {
                scan.metadata
                    .exif
                    .as_deref()
                    .and_then(crate::metadata_tiff::orientation)
            })
            .unwrap_or(image::metadata::Orientation::NoTransforms);
        let quarter = matches!(
            orientation,
            image::metadata::Orientation::Rotate90
                | image::metadata::Orientation::Rotate270
                | image::metadata::Orientation::Rotate90FlipH
                | image::metadata::Orientation::Rotate270FlipH
        );
        let (samples, dims) = oriented(samples, dims, orientation);
        let resolution = scan
            .resolution
            .map(|r| r.transposed_if(quarter))
            .unwrap_or_default();
        warnings.splice(0..0, sentence);
        let tiff = container == CmykContainer::Tiff;
        let doc = single_layer(
            samples,
            dims,
            policy,
            scan.metadata,
            resolution,
            warnings,
            job,
        )?;
        if tiff {
            return doc.with_tiff_spots(bytes, limit, other.saturating_add(bytes.len() as u64));
        }
        Ok(doc)
    }

    /// A CMYK PSD, PSB or layered TIFF as a CMYK document: mode 0 imports
    /// its layers (`psd_import_cmyk::open_cmyk_layers`), mode 1 opens its
    /// saved composite (module doc; a TIFF has none here and refuses with
    /// `cmyk_admission`'s sentence).
    pub(crate) fn open_cmyk_psd(
        bytes: &[u8],
        path: &str,
        mode: i32,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        if mode == 0 {
            return crate::psd_import_cmyk::open_cmyk_layers(bytes, path, limit, other, job);
        }
        if mode != 1 {
            return Err("unknown PSD import mode; use 0 (layers) or 1 (stored composite)".into());
        }
        let container = Container::read(bytes).map_err(|e| format!("PSD {path}: {e}"))?;
        Self::open_cmyk_composite(bytes, path, &container, limit, other, job)
    }

    /// The saved composite of an already-read CMYK container as a one-layer
    /// CMYK document: mode 1, and Import Layers of a file that retains no
    /// record (`psd_import_cmyk`). The caller has begun the job control.
    pub(crate) fn open_cmyk_composite(
        bytes: &[u8],
        path: &str,
        container: &Container<'_>,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        container
            .cmyk_admission()
            .map_err(|e| format!("PSD {path}: {e}"))?;
        let depth = if container.depth == 16 {
            CmykDepth::Sixteen
        } else {
            CmykDepth::Eight
        };
        let (policy, sentence) =
            psd_policy(container, depth).map_err(|e| format!("PSD {path}: {e}"))?;
        let (w, h) = (container.width, container.height);
        admit(0, open_scratch(w, h, bytes.len(), 0)?, other, limit)?;
        job.reserve(open_scratch(w, h, bytes.len(), 0)?)?;
        let samples = container
            .cmyk_composite()
            .map_err(|e| format!("PSD {path}: {e}"))?;
        job.check()?;
        let resolution = container
            .resolution()
            .map_err(|e| format!("PSD {path}: {e}"))?
            .unwrap_or_default();
        let mut warnings: Vec<String> = sentence.into_iter().collect();
        if container.merged_alpha && container.channels > 4 {
            warnings.push(MERGED_ALPHA.into());
        }
        let out = single_layer(
            samples,
            (w, h),
            policy,
            Metadata::default(),
            resolution,
            warnings,
            job,
        )?
        .with_psd_spots(container, limit, other)?;
        job.report(1.0, 5)?;
        Ok(out)
    }
}

/// A CMYK PSD's merged transparency is not reconstructed.
pub(crate) const MERGED_ALPHA: &str = "This CMYK PSD's saved composite has merged transparency, which is not read: the composite opens as stored, matted on white.";

/// A CMYK file's policy: its embedded profile
/// (`Container::cmyk_profile_bytes` — a PSD's resource 1039, a TIFF's ICC
/// tag) through `CmykPolicy` (never `Container::profile`, which reads RGB
/// and gray profiles only); untagged takes the working profile; an unusable
/// profile is refused by name.
pub(crate) fn psd_policy(
    container: &Container<'_>,
    depth: CmykDepth,
) -> Result<(CmykPolicy, Option<String>), String> {
    match container.cmyk_profile_bytes()? {
        Some(bytes) => Ok((CmykPolicy::new(bytes, depth)?, None)),
        None => file_policy(None, depth).ok_or_else(|| {
            "the CMYK PSD has no embedded colour profile and no working CMYK profile is set".into()
        }),
    }
}

/// What a CMYK save writes (`dr_precision_doc_save_cmyk`'s `request`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SaveFormat {
    Tiff,
    Psd,
    JpegIntermediate,
}

/// Bytes a canvas pixel a TIFF or JPEG-intermediate save charges: the
/// flattened samples as u16 (8 B/px), the encoder's copy and its output
/// (384 MB measured at 24 MP for both).
pub(crate) const SAVE_BYTES_PER_PIXEL: u64 = 24;

/// The same for a PSD save: the samples, the four stored planes, their
/// PackBits rows and the assembled file (771 MB measured at 24 MP, against
/// 24 B/px's 577 MB); a miss raises this term.
pub(crate) const SAVE_PSD_BYTES_PER_PIXEL: u64 = 42;

pub(crate) struct SaveRequest {
    pub format: SaveFormat,
    pub embed_profile: bool,
    pub strip_metadata: bool,
}

impl SaveRequest {
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(text)
            .map_err(|e| format!("CMYK save request is not valid JSON: {e}"))?;
        let object = value
            .as_object()
            .ok_or("CMYK save request must be a JSON object")?;
        for key in object.keys() {
            if !matches!(key.as_str(), "format" | "embed_profile" | "strip_metadata") {
                return Err(format!("CMYK save request has an unknown key \"{key}\""));
            }
        }
        let format = match object.get("format").and_then(Value::as_str) {
            Some("tiff") => SaveFormat::Tiff,
            Some("psd") => SaveFormat::Psd,
            Some("jpeg_intermediate") => SaveFormat::JpegIntermediate,
            Some(other) => {
                return Err(crate::cmyk_document::refused_sentence(&format!(
                    "Saving as {other}"
                )))
            }
            None => return Err("CMYK save request needs a \"format\"".into()),
        };
        let flag = |key: &str, default: bool| match object.get(key) {
            None | Some(Value::Null) => Ok(default),
            Some(Value::Bool(v)) => Ok(*v),
            Some(_) => Err(format!("CMYK save request {key} must be a boolean")),
        };
        Ok(Self {
            format,
            embed_profile: flag("embed_profile", true)?,
            strip_metadata: flag("strip_metadata", false)?,
        })
    }
}

impl PrecisionDocument {
    /// The flattened composite's four ink planes at `depth`, row-major,
    /// interleaved CMYK (8-bit: one byte a sample; 16-bit: native u16).
    pub(crate) fn cmyk_flat_samples(
        &self,
        depth: CmykDepth,
        job: JobContext<'_>,
    ) -> Result<Vec<u16>, String> {
        let (w, h) = self.dimensions();
        let mut stack = self.cmyk_stack(None, job)?;
        let mut acc = vec![[0f32; 5]; w as usize];
        let mut out = crate::precision_image::reserved_vec(pixel_count(w, h)? * 4)?;
        for y in 0..h {
            job.check()?;
            stack.row(i64::from(y), 0, &mut acc);
            for ink in &acc {
                let paper = on_paper(*ink);
                let c = codes([paper[0], paper[1], paper[2], paper[3], 1.0]);
                for v in &c[..4] {
                    out.push(match depth {
                        CmykDepth::Sixteen => *v,
                        CmykDepth::Eight => ((u32::from(*v) + 128) / 257) as u16,
                    });
                }
            }
        }
        Ok(out)
    }

    /// The working memory of a TIFF or JPEG-intermediate save
    /// ([`SAVE_BYTES_PER_PIXEL`]) or a PSD one ([`SAVE_PSD_BYTES_PER_PIXEL`]),
    /// one ink row and bookkeeping.
    pub(crate) fn cmyk_save_scratch_bytes(&self) -> Result<u64, String> {
        self.cmyk_save_scratch_for(SAVE_BYTES_PER_PIXEL)
    }

    /// …plus, for a PSD, the spot channels' planes and resources it writes
    /// (`psd_spot`), at the PSB row-table width so either container is held.
    pub(crate) fn cmyk_psd_save_scratch_bytes(&self) -> Result<u64, String> {
        let eight = self
            .cmyk
            .as_ref()
            .is_some_and(|policy| policy.depth() == CmykDepth::Eight);
        Ok(self
            .cmyk_save_scratch_for(SAVE_PSD_BYTES_PER_PIXEL)?
            .saturating_add(self.psd_spot_worst_case(eight, true)))
    }

    fn cmyk_save_scratch_for(&self, per_pixel: u64) -> Result<u64, String> {
        let (w, h) = self.dimensions();
        let pixels = pixel_count(w, h)? as u64;
        Ok(pixels * per_pixel
            + u64::from(w) * 20
            + self.cmyk_stack_scratch_bytes()?
            + BOOKKEEPING_BYTES)
    }

    pub(crate) fn cmyk_save_scratch_of(&self, format: SaveFormat) -> Result<u64, String> {
        match format {
            SaveFormat::Psd => self.cmyk_psd_save_scratch_bytes(),
            // …plus the spot channels it keeps (1c-2a, `tiff_spot`).
            SaveFormat::Tiff => {
                let eight = self
                    .cmyk
                    .as_ref()
                    .is_some_and(|policy| policy.depth() == CmykDepth::Eight);
                Ok(self
                    .cmyk_save_scratch_bytes()?
                    .saturating_add(self.tiff_spot_scratch_bytes(if eight { 1 } else { 2 }, 4)))
            }
            // The spot inks overprinted on paper first (`spot_cmyk`).
            SaveFormat::JpegIntermediate if self.has_spot_channel() => Ok(self
                .cmyk_save_scratch_bytes()?
                .saturating_add(self.cmyk_spot_scratch_bytes())),
            SaveFormat::JpegIntermediate => self.cmyk_save_scratch_bytes(),
        }
    }

    /// `dr_precision_doc_save_cmyk_plan`'s answer.
    pub fn save_cmyk_plan(&self, request: &str) -> Result<Value, String> {
        let parsed = SaveRequest::parse(request);
        let bytes = match &parsed {
            Ok(request) => self.cmyk_save_scratch_of(request.format)?,
            Err(_) => self.cmyk_psd_save_scratch_bytes()?,
        };
        let refusal = match parsed {
            Ok(_) => self.require_cmyk_policy("Saving CMYK").err(),
            Err(e) => Some(e),
        };
        Ok(json!({"working_memory_plan_bytes":bytes,"refusal":refusal,"flatten_required":false}))
    }

    /// Saves the composite (module doc) to `path`, atomically.
    pub fn save_cmyk(
        &self,
        path: &str,
        request: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<(), String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let request = SaveRequest::parse(request)?;
        self.require_cmyk_policy("Saving CMYK")?;
        let scratch = self.cmyk_save_scratch_of(request.format)?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let bytes = self.cmyk_encoded(&request, job)?;
        job.check()?;
        crate::dr_image::save_atomically(path, |tmp| {
            std::fs::write(tmp, &bytes).map_err(|e| format!("failed to write {path}: {e}"))
        })?;
        if let Some(control) = control {
            control.report(1.0, 5);
        }
        Ok(())
    }

    /// The bytes a CMYK save writes for `request` (module doc), shared by
    /// the save and a kept-CMYK export (`cmyk_export`). Admission belongs to
    /// the caller.
    pub(crate) fn cmyk_encoded(
        &self,
        request: &SaveRequest,
        job: JobContext<'_>,
    ) -> Result<Vec<u8>, String> {
        let policy = self.require_cmyk_policy("Saving CMYK")?.clone();
        let depth = match request.format {
            SaveFormat::JpegIntermediate => CmykDepth::Eight,
            _ => policy.depth(),
        };
        let spots = self.spot_indices();
        // A JPEG intermediate is the flattened deliverable, so it carries
        // the spot inks overprinted on paper (`spot_cmyk`); a TIFF and a PSD
        // write them as channels instead.
        let samples = if request.format == SaveFormat::JpegIntermediate && !spots.is_empty() {
            let printed = self.cmyk_spot_on_paper(&spots, job.range(0.0, 0.4))?;
            Self::cmyk_interleaved(&printed, depth, job.range(0.4, 0.6))?
        } else {
            self.cmyk_flat_samples(depth, job.range(0.0, 0.6))?
        };
        let (w, h) = self.dimensions();
        let bytes = match request.format {
            SaveFormat::Tiff | SaveFormat::JpegIntermediate => {
                let (mut side, _) = crate::doc_color::image_sidecar(
                    self.profile(),
                    self.metadata(),
                    self.resolution(),
                    crate::dr_image::Format::Tiff,
                    false,
                    request.strip_metadata,
                    (w, h),
                );
                side.icc = request.embed_profile.then_some(policy.profile_bytes());
                // A TIFF keeps the spot channels as extra samples (1c-2a,
                // `tiff_spot`); a CMYK file has no transparency sample.
                if request.format == SaveFormat::Tiff && !spots.is_empty() {
                    side.spots = Some(self.tiff_spot_planes(depth == CmykDepth::Eight, false));
                }
                cmyk_tiff_bytes(&samples, (w, h), depth, &side)?
            }
            SaveFormat::Psd => {
                let icc: &[u8] = if request.embed_profile {
                    policy.profile_bytes()
                } else {
                    &[]
                };
                flat_psd(
                    &samples,
                    (w, h),
                    depth,
                    icc,
                    self.resolution(),
                    false,
                    &self.psd_spot_planes(depth == CmykDepth::Eight),
                )?
            }
        };
        Ok(bytes)
    }

    /// The flattened CMYK PSD (`large` = a PSB, version 2) a PSD save
    /// writes, its profile embedded: the flattened mode of a CMYK
    /// `export_psd` (#1 phase 3, `psd_export_cmyk`). Admission belongs to
    /// the caller.
    pub(crate) fn cmyk_flat_psd(
        &self,
        large: bool,
        job: JobContext<'_>,
    ) -> Result<Vec<u8>, String> {
        let policy = self.require_cmyk_policy("Saving CMYK")?.clone();
        let depth = policy.depth();
        let samples = self.cmyk_flat_samples(depth, job.range(0.0, 0.6))?;
        flat_psd(
            &samples,
            self.dimensions(),
            depth,
            policy.profile_bytes(),
            self.resolution(),
            large,
            &self.psd_spot_planes(depth == CmykDepth::Eight),
        )
    }
}

/// A CMYK TIFF of interleaved `samples` at `depth` (8-bit: one code a u16
/// slot, as [`PrecisionDocument::cmyk_flat_samples`] gives them) carrying
/// `side`'s packets: the encode a TIFF or JPEG-intermediate save and a
/// kept-CMYK export (`cmyk_export`) share.
pub(crate) fn cmyk_tiff_bytes(
    samples: &[u16],
    dimensions: (u32, u32),
    depth: CmykDepth,
    side: &crate::metadata_write::EncodeSidecar<'_>,
) -> Result<Vec<u8>, String> {
    let mut out = Cursor::new(Vec::new());
    match depth {
        CmykDepth::Eight => {
            let bytes: Vec<u8> = samples.iter().map(|v| *v as u8).collect();
            crate::tiff_encode::encode_cmyk8(&bytes, dimensions, &mut out, side)?
        }
        CmykDepth::Sixteen => {
            crate::tiff_encode::encode_cmyk16(samples, dimensions, &mut out, side)?
        }
    }
    Ok(out.into_inner())
}

/// A zero-record colour-mode-4 PSD (or PSB) of flattened CMYK `samples`
/// (interleaved, at `depth`), stored `max − ink` (module doc), with the
/// document's spot channels (`psd_spot`).
fn flat_psd(
    samples: &[u16],
    (w, h): (u32, u32),
    depth: CmykDepth,
    icc: &[u8],
    resolution: Resolution,
    large: bool,
    spots: &crate::psd_spot::SpotPlanes,
) -> Result<Vec<u8>, String> {
    let max = match depth {
        CmykDepth::Eight => 255u16,
        CmykDepth::Sixteen => 65535,
    };
    let count = w as usize * h as usize;
    let width = if depth == CmykDepth::Eight { 1 } else { 2 };
    let mut planes: Vec<Vec<u8>> = (0..4).map(|_| Vec::with_capacity(count * width)).collect();
    for pixel in samples.chunks_exact(4) {
        for (plane, v) in planes.iter_mut().zip(pixel) {
            // PSD stores CMYK as `max − ink` (module doc).
            let stored = max - *v;
            if depth == CmykDepth::Eight {
                plane.push(stored as u8);
            } else {
                plane.extend_from_slice(&stored.to_be_bytes());
            }
        }
    }
    crate::psd_export_encode::write_psd(
        w,
        h,
        depth.bits() as u16,
        4,
        icc,
        resolution,
        None,
        &[],
        &[],
        &planes,
        large,
        &[],
        spots,
    )
}
