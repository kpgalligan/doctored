//! Bitmap PSD and PSB files, colour mode 0 (#1 Bitmap phase 1c): the
//! compatibility report ([`import_report`]), the open of the stored
//! composite ([`PrecisionDocument::open_bitmap_psd`]), and the export's
//! report and writer (`designs/bitmap-mode.md`).
//!
//! # The file
//!
//! Adobe's Photoshop File Format Specification
//! (https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/): a Bitmap
//! file's header declares depth 1 and colour mode 0, its Color Mode Data
//! section is empty, and its image data holds one channel of rows of
//! `ceil(width/8)` bytes, the leftmost pixel in the most significant bit,
//! where a bit 1 is BLACK: the inverse of the plane's polarity, so both
//! directions go through `bitmap_codec::flip_polarity`, which also clears
//! the pad bits. Photoshop writes no layer records in Bitmap mode.
//!
//! # Import
//!
//! The depth must be 1 and the composite Raw (compression 0) or RLE (1, the
//! one `psd_framing` PackBits decoder); a ZIP composite refuses by name.
//! Only the first `height` rows are decoded; the rows of any further saved
//! channel are framed and validated, never decoded. The side limit is the
//! container's (`LayeredContainer::max_side`). A Gray ICC in resource 1039
//! is kept, else the working Gray is the profile; the resolution and colour
//! samplers are the file's (`Container`). The open's plan is
//! `bitmap_io::open_scratch`. There are no layers to import: the layered
//! choice refuses with [`LAYERED_REFUSAL`].
//!
//! # Export
//!
//! Both modes write the same file, since the one "Bitmap" layer IS the
//! composite: the header (one channel, depth 1, mode 0), an empty Color
//! Mode Data section, the image resources
//! (`psd_export_encode::image_resources` with no profile: the resolution
//! and the colour samplers), an empty layer-and-mask section (a u64 length
//! in a PSB) and the plane inverted, RLE-encoded
//! (`psd_export_encode::encode_plane`), written atomically. A layered TIFF
//! cannot hold colour mode 0 and refuses; so does a side past the
//! container's limit. The Gray profile is not embedded (Photoshop writes a
//! Bitmap file without one) and is reported as a loss.

use serde_json::{json, Value};

use crate::bitmap_codec::flip_polarity;
use crate::bitmap_method::stride;
use crate::gray_document::{GrayDepth, GrayPolicy};
use crate::gray_working::GrayWorkingSpace;
use crate::job_control::{DrJobControl, JobContext};
use crate::metadata::Metadata;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, BOOKKEEPING_BYTES};
use crate::psd_container::{unpack_packbits, Container};
use crate::psd_export::PsdExportMode;
use crate::psd_framing::{rle_rows, validate_packbits, LayeredContainer};

/// The layered choice's refusal: a Bitmap file holds no layers.
pub(crate) const LAYERED_REFUSAL: &str =
    "A Bitmap file holds no layers; open its stored composite.";

/// The export's refusal for the layered TIFF container.
const TIFF_REFUSAL: &str =
    "A Bitmap document writes a 1-bit PSD or PSB; a TIFF with layers cannot \
                            hold it. Save a Copy writes its 1-bit TIFF.";

const STORED_COMPOSITE_NOTE: &str = "Opens the file's saved 1-bit composite as a Bitmap document: \
                                     one layer of black and white pixels.";

const IMPORT_LIMITATIONS: &[&str] = &[
    "A Bitmap PSD or PSB opens as a Bitmap document from its saved 1-bit composite, with its \
     resolution, its colour samplers and a Gray profile from resource 1039 (else the working \
     Gray); its metadata is not read.",
];

const EXPORT_LIMITATIONS: &[&str] = &[
    "A Bitmap PSD or PSB stores one 1-bit channel (1 is black), the resolution and the colour \
     samplers; it holds no layers, no profile and no metadata.",
];

/// Why a Bitmap file cannot open, before its rows are read: the depth, the
/// side limit and the composite's compression.
fn admission(container: &Container<'_>) -> Result<(), String> {
    if container.depth != 1 {
        return Err(format!(
            "a Bitmap PSD stores 1 bit a pixel; this file declares {}",
            container.depth
        ));
    }
    // `Container::support_refusal`'s side sentences (the master copy, which
    // refuses colour mode 0 before it reaches them).
    let side = container.kind.max_side();
    if container.width > side || container.height > side {
        return Err(match container.kind {
            LayeredContainer::Psd => {
                "PSD canvas exceeds the classic format's 30000-pixel dimension limit".into()
            }
            kind => format!(
                "{} canvas exceeds the format's {side}-pixel dimension limit",
                kind.name()
            ),
        });
    }
    match container.composite.get(..2) {
        Some([0, 0] | [0, 1]) => Ok(()),
        Some([0, 2] | [0, 3]) => {
            Err("a Bitmap PSD's composite is Raw or RLE; this file's uses ZIP".into())
        }
        Some(_) => Err("PSD stored composite uses unsupported compression".into()),
        None => Err("truncated PSD composite".into()),
    }
}

/// The stored composite's first channel as the plane (module doc): rows of
/// `stride(width)` bytes, the PSD's 1-black inverted to 1-white, pad bits 0.
fn composite_plane(container: &Container<'_>) -> Result<Vec<u8>, String> {
    admission(container)?;
    let line = stride(container.width);
    let height = container.height as usize;
    let channels = usize::from(container.channels);
    let size = line
        .checked_mul(height)
        .ok_or("PSD composite size overflow")?;
    let stored = if container.composite[1] == 0 {
        let expected = size
            .checked_mul(channels)
            .and_then(|v| v.checked_add(2))
            .ok_or("PSD composite size overflow")?;
        if container.composite.len() != expected {
            return Err("PSD raw composite length mismatch".into());
        }
        container.composite[2..2 + size].to_vec()
    } else {
        let rows = rle_rows(
            &container.composite[2..],
            channels * height,
            container.kind.large(),
        )?;
        let mut out = crate::precision_image::reserved_vec(size)?;
        for (index, row) in rows.iter().enumerate() {
            if index < height {
                out.extend_from_slice(&unpack_packbits(row, line)?);
            } else {
                validate_packbits(row, line)?;
            }
        }
        out
    };
    Ok(flip_polarity(&stored, container.width))
}

/// The report's twin of [`composite_plane`] (the open's, which this mirrors
/// check for check): the admission, then the raw composite's length, or
/// every RLE row framed by `validate_packbits`, with no buffer decoded, so
/// the report refuses exactly what the open refuses by the same sentence.
fn composite_frames(container: &Container<'_>) -> Result<(), String> {
    admission(container)?;
    let line = stride(container.width);
    let height = container.height as usize;
    let channels = usize::from(container.channels);
    let size = line
        .checked_mul(height)
        .ok_or("PSD composite size overflow")?;
    if container.composite[1] == 0 {
        let expected = size
            .checked_mul(channels)
            .and_then(|v| v.checked_add(2))
            .ok_or("PSD composite size overflow")?;
        if container.composite.len() != expected {
            return Err("PSD raw composite length mismatch".into());
        }
        return Ok(());
    }
    let rows = rle_rows(
        &container.composite[2..],
        channels * height,
        container.kind.large(),
    )?;
    for row in &rows {
        validate_packbits(row, line)?;
    }
    Ok(())
}

/// The Gray profile of a Bitmap file: resource 1039 when a Gray document
/// takes it, else the working Gray.
fn gray_policy(container: &Container<'_>, working: GrayWorkingSpace) -> Result<GrayPolicy, String> {
    if let Some(icc) = container.profile_bytes()? {
        if let Ok(policy) = GrayPolicy::new(icc, GrayDepth::Eight) {
            return Ok(policy);
        }
    }
    GrayPolicy::for_space(working, GrayDepth::Eight)
}

/// `dr_psd_import_report`'s answer for a colour-mode-0 file: every key
/// `psd_import::report` writes, `import_precision` "bitmap", no layers.
pub(crate) fn import_report(
    container: &Container<'_>,
    bytes: &[u8],
    path: &str,
) -> Result<Value, String> {
    let resolution = container.resolution();
    let composite_error = composite_frames(container)
        .err()
        .or_else(|| resolution.as_ref().err().cloned());
    let mut losses = Vec::<String>::new();
    let mut observations = Vec::<String>::new();
    let extra = container.channels.saturating_sub(1);
    if extra > 0 {
        losses.push(format!(
            "{extra} additional saved channels detected in the header; the Bitmap open does not \
             retain them."
        ));
    }
    if let Ok(samplers) = container.color_samplers() {
        losses.extend(samplers.losses);
        observations.extend(samplers.observations);
    }
    Ok(json!({
        "format": container.kind.name(), "path": path,
        "source_fingerprint": crate::psd_import::fingerprint(bytes),
        "width": container.width, "height": container.height,
        "depth": container.depth, "color_mode": container.color_name(),
        "import_precision": "bitmap",
        "layer_count": 0, "group_count": 0,
        "layered_available": false, "layered_refusal": LAYERED_REFUSAL,
        "layered_note": "A Bitmap file holds one 1-bit image and no layer records.",
        "stored_composite_available": composite_error.is_none(),
        "stored_composite_refusal": composite_error,
        "stored_composite_note": STORED_COMPOSITE_NOTE,
        "verified_losses": losses, "observations": observations,
        "general_limitations": IMPORT_LIMITATIONS,
        "profile": Value::Null,
        "resolution": resolution.ok().flatten().map(|r| json!({"x": r.x, "y": r.y})),
        "metadata_captured": false,
        "requires_explicit_choice": true
    }))
}

impl PrecisionDocument {
    /// A Bitmap PSD or PSB's stored composite as a Bitmap document (module
    /// doc); `working` is the working Gray's wire name.
    pub fn open_bitmap_psd(
        bytes: &[u8],
        path: &str,
        working: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let working = GrayWorkingSpace::from_name(working)
            .ok_or_else(|| format!("\"{working}\" is not a Gray working space"))?;
        let container = Container::read(bytes).map_err(|e| format!("PSD {path}: {e}"))?;
        if container.color != 0 {
            return Err(format!("PSD {path}: the file is not a Bitmap PSD or PSB"));
        }
        admission(&container).map_err(|e| format!("PSD {path}: {e}"))?;
        let scratch =
            crate::bitmap_io::open_scratch(container.width, container.height, bytes.len() as u64)?;
        admit(0, scratch, other, limit)?;
        job.reserve(scratch)?;
        job.check()?;
        let plane = composite_plane(&container).map_err(|e| format!("PSD {path}: {e}"))?;
        job.check()?;
        let policy = gray_policy(&container, working)?;
        let resolution = container
            .resolution()
            .map_err(|e| format!("PSD {path}: {e}"))?
            .unwrap_or_default();
        let samplers = container
            .color_samplers()
            .map_err(|e| format!("PSD {path}: {e}"))?
            .kept;
        let mut document = Self::from_bitmap_plane(
            plane,
            container.width,
            container.height,
            policy,
            Metadata::default(),
            resolution,
            Vec::new(),
            job,
        )?;
        document.color_samplers = samplers;
        // Resources 1025 and 2000–2997 (`psd_paths`).
        document
            .paths
            .set_entries(crate::psd_paths::file_paths(&container).0);
        job.report(1.0, 5)?;
        Ok(document)
    }

    /// The export's refusal for `container` (module doc), in order: the
    /// layered TIFF, then a side past the container's limit.
    fn bitmap_psd_refusal(&self, container: LayeredContainer) -> Option<String> {
        if container == LayeredContainer::Tiff {
            return Some(TIFF_REFUSAL.into());
        }
        // `psd_export_preflight::export_refusal`'s side sentence, word for
        // word (the master copy, private to the preflight's inventory).
        let (w, h) = self.dimensions();
        let max_side = container.max_side();
        if w > max_side || h > max_side {
            return Some(match container {
                LayeredContainer::Psd => format!(
                    "This canvas is {w} x {h} pixels; the classic PSD format stores at most \
                     {max_side} pixels per side. Export it as a Large Document (PSB), which \
                     stores up to 300000."
                ),
                _ => format!(
                    "This canvas is {w} x {h} pixels; the PSB format stores at most {max_side} \
                     pixels per side."
                ),
            });
        }
        None
    }

    /// The image resources the export writes (module doc).
    fn bitmap_psd_resources(&self) -> Vec<u8> {
        crate::psd_export_encode::image_resources(
            &[],
            self.resolution(),
            None,
            self.color_samplers(),
            &crate::psd_paths::resources(self.paths(), self.dimensions()),
        )
    }

    /// The largest file the export can write: the header, the empty Color
    /// Mode Data section, the resources, the empty layer-and-mask section
    /// and the RLE plane's worst case.
    fn bitmap_psd_worst_case(&self, large: bool) -> u64 {
        let (w, h) = self.dimensions();
        let layers: u64 = if large { 8 } else { 4 };
        (26 + 4 + 4 + self.bitmap_psd_resources().len() as u64 + layers).saturating_add(
            crate::psd_export_encode::plane_worst_case(stride(w) as u64, u64::from(h), large),
        )
    }

    /// The export's working memory: the file's worst case, the inverted
    /// plane and bookkeeping.
    fn bitmap_psd_plan(&self, large: bool) -> u64 {
        let (w, h) = self.dimensions();
        self.bitmap_psd_worst_case(large)
            .saturating_add(stride(w) as u64 * u64::from(h))
            .saturating_add(BOOKKEEPING_BYTES)
    }

    /// The export report (`dr_precision_doc_psd_export_report`'s Bitmap
    /// arm): `PsdExportReport::json`'s keys with depth 1, `color_mode`
    /// "bitmap" and the plan.
    pub(crate) fn bitmap_psd_report_json(
        &self,
        container: LayeredContainer,
        mode: PsdExportMode,
    ) -> Value {
        let refusal = self.bitmap_psd_refusal(container);
        let large = container.large();
        let mut losses = Vec::<String>::new();
        if let Some(gray) = self.gray_policy() {
            losses.push(format!(
                "The Gray profile “{}” is not embedded; a Bitmap PSD stores no profile.",
                gray.name()
            ));
        }
        if !self.guides().is_empty() || self.ruler_origin() != (0.0, 0.0) {
            losses.push("Guides and the ruler origin are not written.".into());
        }
        if self.color_samplers().iter().any(|s| s.sample_size != 1) {
            losses.push("Color sampler sample sizes are not written; Photoshop reads every sampler at its own Sample Size.".into());
        }
        if !self.metadata().is_empty() {
            losses.push("EXIF/XMP/IPTC packets are not written.".into());
        }
        let mut observations = Vec::<String>::new();
        if mode == PsdExportMode::Layers {
            observations.push(
                "A Bitmap document's one layer is its composite, so Layers writes the same 1-bit \
                 file as Flattened."
                    .into(),
            );
        }
        let (worst, plan) = if refusal.is_some() {
            (0, 0)
        } else {
            (
                self.bitmap_psd_worst_case(large),
                self.bitmap_psd_plan(large),
            )
        };
        json!({
            "format": container.name(),
            "container": container.name(),
            "bake_scratch_bytes": 0,
            "mode": mode.name(),
            "depth": 1,
            "bit_depth": 1,
            "color_mode": "bitmap",
            "available": refusal.is_none(),
            "refusal": refusal,
            "losses": losses,
            "observations": observations,
            "general_limitations": EXPORT_LIMITATIONS,
            "layer_count": 0,
            "group_count": 0,
            "worst_case_bytes": worst,
            "working_memory_plan_bytes": plan,
        })
    }

    /// Writes this Bitmap document as a colour-mode-0 PSD or PSB (module
    /// doc). Pure, and atomic through the shared temp-file rename; the
    /// report's refusal is this call's error, before anything is written.
    pub(crate) fn save_bitmap_psd(
        &self,
        path: &str,
        container: LayeredContainer,
        _mode: PsdExportMode,
        limit: u64,
        other: u64,
    ) -> Result<(), String> {
        if let Some(refusal) = self.bitmap_psd_refusal(container) {
            return Err(refusal);
        }
        let bitmap = self
            .bitmap_policy()
            .ok_or("a Bitmap PSD export requires a Bitmap document")?
            .clone();
        let large = container.large();
        admit(
            self.retained_bytes(),
            self.bitmap_psd_plan(large),
            other,
            limit,
        )?;
        let (w, h) = self.dimensions();
        let resources = self.bitmap_psd_resources();
        let resources_len = u32::try_from(resources.len())
            .map_err(|_| "PSD image resources exceed the format's 4 GiB section".to_string())?;
        let inverted = flip_polarity(bitmap.packed(), w);
        let plane = crate::psd_export_encode::encode_plane(&inverted, stride(w) as u32, h, large);
        drop(inverted);
        let mut out =
            crate::precision_image::reserved_vec(26 + 4 + 4 + resources.len() + 8 + plane.len())?;
        out.extend_from_slice(b"8BPS");
        out.extend_from_slice(&(if large { 2u16 } else { 1 }).to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&h.to_be_bytes());
        out.extend_from_slice(&w.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&resources_len.to_be_bytes());
        out.extend_from_slice(&resources);
        if large {
            out.extend_from_slice(&0u64.to_be_bytes());
        } else {
            out.extend_from_slice(&0u32.to_be_bytes());
        }
        out.extend_from_slice(&plane);
        crate::dr_image::save_atomically(path, |tmp| {
            std::fs::write(tmp, &out).map_err(|e| format!("failed to write {path}: {e}"))
        })
    }
}
