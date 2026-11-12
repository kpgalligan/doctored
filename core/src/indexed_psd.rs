//! Indexed Color PSD and PSB files, colour mode 2 (#1): the compatibility
//! report ([`import_report`]), the open of the stored composite
//! ([`PrecisionDocument::open_indexed_psd`]), and the export's report and
//! writer (`designs/indexed-mode.md`). `bitmap_psd` is the template; the
//! colour table's records are `indexed_psd_records`'.
//!
//! # The file
//!
//! Adobe's Photoshop File Format Specification
//! (https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/): an
//! Indexed Color file's header declares depth 8 and colour mode 2, its
//! Color Mode Data section is the 768-byte table, resources 1046 and 1047
//! hold the count in use and the transparent index, and its image data holds
//! the index plane as the first channel, one byte a pixel.
//!
//! # Import
//!
//! The depth must be 8, the file must hold at least one channel and the
//! composite must be Raw (compression 0) or RLE (1, the one `psd_framing`
//! PackBits decoder); a ZIP composite refuses by name. The table is
//! `indexed_psd_records::read_table`'s and passes `IndexedPolicy::new`'s
//! palette check (2–256 entries, the transparent index inside them). Only
//! the first `height` rows are decoded; the rows of any further saved
//! channel are framed and validated, never decoded, and each is one loss
//! line. The report runs every one of those checks too ([`composite_frames`],
//! the open's twin), so it refuses what the open refuses by the same
//! sentence; the one check it does not run is "an index is outside the
//! palette", which needs the decode, so it stays the open's. An RGB profile
//! in resource 1039 is kept; any other profile, or none, reads the palette as
//! sRGB with a warning. The resolution and colour samplers are the file's
//! (`Container`). A layer record (Photoshop writes none, a transparent index
//! or not; another writer may) is not read: the composite is the image, and
//! a record name other than "Index" is one import warning. The open's plan is
//! `indexed_io::open_scratch`. The layered choice refuses with
//! [`LAYERED_REFUSAL`].
//!
//! # Export
//!
//! Both modes write the same file, since the one "Index" layer IS the
//! composite, in Photoshop's own form (the `indexed_547` fixtures, take1:
//! one channel whether or not the image has a transparent index, the table
//! zero-padded past the count, 1046 and 1047 in every file, 1047 0xFFFF with
//! no transparent index, and no layer record at all, the image a Background
//! even with a transparent index): the header (one channel, depth 8, mode 2),
//! the table, the image resources (`psd_export_encode::image_resources`: the
//! document's RGB profile, the resolution and the colour samplers) followed
//! by 1046 and 1047, an empty layer-and-mask section (a u64 length in a PSB)
//! and the index plane, RLE-encoded (`psd_export_encode::encode_plane`),
//! written atomically. Photoshop's own tail blocks (`CAI `, `OCIO`, `GenI`,
//! `FMsk`, `cinf`) carry nothing the document holds and are not written. A
//! layered TIFF cannot hold colour mode 2 and refuses, as do a side past the
//! container's limit and a hidden layer.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::icc::IccProfile;
use crate::indexed_document::{is_rgb_profile, IndexedPolicy, LAYER_NAME};
use crate::indexed_psd_records::{
    color_mode_data, count_body, read_table, transparent_body, IndexedTable, TABLE_COUNT,
    TRANSPARENT_INDEX,
};
use crate::job_control::{DrJobControl, JobContext};
use crate::metadata::Metadata;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, pixel_count, BOOKKEEPING_BYTES};
use crate::psd_container::{unpack_packbits, Container};
use crate::psd_export::PsdExportMode;
use crate::psd_export_encode::{encode_plane, plane_worst_case, resource};
use crate::psd_framing::{rle_rows, validate_packbits, LayeredContainer};

/// The layered choice's refusal: an Indexed Color document holds one layer.
pub(crate) const LAYERED_REFUSAL: &str =
    "An Indexed Color file opens from its stored composite: an Indexed Color document holds one layer.";

/// The export's loss when the layer is not named "Index": the file carries
/// no layer record (module doc).
pub(crate) const NAME_LOSS: &str =
    "The layer's name is not written: an Indexed Color PSD holds no layer record, as Photoshop writes it.";

/// The export's refusal for the layered TIFF container.
const TIFF_REFUSAL: &str = "An Indexed Color document writes an 8-bit Indexed PSD or PSB; a TIFF \
                            with layers cannot hold it. Save a Copy writes its palette TIFF.";

/// The open's warning for an embedded profile that is not RGB.
const NOT_RGB: &str = "the embedded profile is not an RGB profile; the palette is read as sRGB";

const STORED_COMPOSITE_NOTE: &str = "Opens the file's saved index plane as an Indexed Color \
                                     document: its palette, transparent index and every index.";

const IMPORT_LIMITATIONS: &[&str] = &[
    "An Indexed Color PSD or PSB opens as an Indexed Color document from its saved composite, \
     with its palette, transparent index, resolution, colour samplers and an RGB profile from \
     resource 1039 (else sRGB); its metadata is not read.",
];

const EXPORT_LIMITATIONS: &[&str] = &[
    "An Indexed Color PSD or PSB stores the palette, the transparent index, the 8-bit index \
     plane, the profile, the resolution and the colour samplers; it holds one layer and no \
     metadata.",
];

/// Why an Indexed Color file cannot open, before its table or rows are
/// read: the depth, the channel count, the side limit and the composite's
/// compression.
fn admission(container: &Container<'_>) -> Result<(), String> {
    if container.depth != 8 {
        return Err(format!(
            "an Indexed Color PSD stores 8 bits a pixel; this file declares {}",
            container.depth
        ));
    }
    if container.channels == 0 {
        return Err(
            "an Indexed Color PSD stores at least one channel; this file declares none".into(),
        );
    }
    // `Container::support_refusal`'s side sentences (the master copy, which
    // refuses colour mode 2 before it reaches them).
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
            Err("an Indexed Color PSD's composite is Raw or RLE; this file's uses ZIP".into())
        }
        Some(_) => Err("PSD stored composite uses unsupported compression".into()),
        None => Err("truncated PSD composite".into()),
    }
}

/// The file's colour table, held to `IndexedPolicy::new`'s palette check
/// (an empty plane, so only the palette and the transparent index are
/// asked).
fn table(container: &Container<'_>) -> Result<IndexedTable, String> {
    let table = read_table(
        container.color_mode_data,
        container.resource(TABLE_COUNT)?,
        container.resource(TRANSPARENT_INDEX)?,
    )?;
    IndexedPolicy::new(table.palette.clone(), table.transparent, Vec::new(), 0, 0)?;
    Ok(table)
}

/// The plane's size and the Raw composite's exact length.
fn raw_lengths(container: &Container<'_>) -> Result<(usize, usize), String> {
    let size = pixel_count(container.width, container.height)?;
    let expected = size
        .checked_mul(usize::from(container.channels))
        .and_then(|v| v.checked_add(2))
        .ok_or("PSD composite size overflow")?;
    Ok((size, expected))
}

/// The stored composite's first channel as the index plane, with the table
/// (module doc).
fn composite_indices(container: &Container<'_>) -> Result<(IndexedTable, Vec<u8>), String> {
    admission(container)?;
    let table = table(container)?;
    let line = container.width as usize;
    let height = container.height as usize;
    let (size, expected) = raw_lengths(container)?;
    let indices = if container.composite[1] == 0 {
        if container.composite.len() != expected {
            return Err("PSD raw composite length mismatch".into());
        }
        container.composite[2..2 + size].to_vec()
    } else {
        let rows = rle_rows(
            &container.composite[2..],
            usize::from(container.channels) * height,
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
    if indices
        .iter()
        .any(|&i| usize::from(i) >= table.palette.len())
    {
        return Err("an index is outside the palette".into());
    }
    Ok((table, indices))
}

/// The report's twin of [`composite_indices`] (the open's, which this
/// mirrors check for check, module doc): the admission, the table and its
/// palette check, then the raw composite's length, or every RLE row framed
/// by `validate_packbits`, with no buffer decoded.
fn composite_frames(container: &Container<'_>) -> Result<IndexedTable, String> {
    admission(container)?;
    let table = table(container)?;
    let line = container.width as usize;
    let (_, expected) = raw_lengths(container)?;
    if container.composite[1] == 0 {
        if container.composite.len() != expected {
            return Err("PSD raw composite length mismatch".into());
        }
        return Ok(table);
    }
    let rows = rle_rows(
        &container.composite[2..],
        usize::from(container.channels) * container.height as usize,
        container.kind.large(),
    )?;
    for row in &rows {
        validate_packbits(row, line)?;
    }
    Ok(table)
}

/// The first layer record's Pascal name, when the layer information holds
/// a record: a bounded walk of Adobe's fixed record fields to the name and
/// nothing further (the full record walk is `psd_group_records`', which an
/// open of the composite does not run).
fn first_record_name(info: &[u8], large: bool) -> Option<String> {
    let u32_at = |bytes: &[u8], at: usize| -> Option<usize> {
        Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?) as usize)
    };
    let count = i16::from_be_bytes(info.get(..2)?.try_into().ok()?);
    if count == 0 {
        return None;
    }
    // The count, then the bounds (16 bytes) and the channel count.
    let channels = usize::from(u16::from_be_bytes(info.get(18..20)?.try_into().ok()?));
    let mut at = channels
        .checked_mul(if large { 10 } else { 6 })?
        .checked_add(20)?;
    // The blend signature and key, opacity, clipping, flags and filler.
    at = at.checked_add(12)?;
    let extra_len = u32_at(info, at)?;
    let extra = info.get(at + 4..at.checked_add(4)?.checked_add(extra_len)?)?;
    let mut cursor = u32_at(extra, 0)?.checked_add(4)?;
    cursor = u32_at(extra, cursor)?.checked_add(cursor)?.checked_add(4)?;
    let len = usize::from(*extra.get(cursor)?);
    let name = extra.get(cursor + 1..cursor + 1 + len)?;
    Some(String::from_utf8_lossy(name).into_owned())
}

/// The profile the open reads the palette in, and its warning (module doc).
fn profile_of(container: &Container<'_>) -> Result<(Arc<IccProfile>, Option<String>), String> {
    let Some(bytes) = container.profile_bytes()? else {
        return Ok((IccProfile::srgb(), Some(crate::indexed_io::UNTAGGED.into())));
    };
    match IccProfile::parse(bytes) {
        Some(profile) if is_rgb_profile(&profile) => Ok((Arc::new(profile), None)),
        _ => Ok((IccProfile::srgb(), Some(NOT_RGB.into()))),
    }
}

/// `dr_psd_import_report`'s answer for a colour-mode-2 file: every key
/// `psd_import::report` writes, `import_precision` "indexed", no layered
/// choice.
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
    for channel in 2..=container.channels {
        losses.push(format!(
            "Saved channel {channel} is not retained: an Indexed Color document holds its index \
             plane only."
        ));
    }
    if let Ok(samplers) = container.color_samplers() {
        losses.extend(samplers.losses);
        observations.extend(samplers.observations);
    }
    let layer_count = container
        .layer_info()
        .ok()
        .and_then(|info| info.get(..2).map(|b| i16::from_be_bytes([b[0], b[1]])))
        .map_or(0, i16::unsigned_abs);
    let profile = container
        .profile_bytes()
        .ok()
        .flatten()
        .and_then(IccProfile::parse)
        // A non-RGB profile is replaced by sRGB on open (`profile_of`), so
        // the report names only one the open keeps.
        .filter(is_rgb_profile)
        .map(|p| p.name().to_string());
    Ok(json!({
        "format": container.kind.name(), "path": path,
        "source_fingerprint": crate::psd_import::fingerprint(bytes),
        "width": container.width, "height": container.height,
        "depth": container.depth, "color_mode": container.color_name(),
        "import_precision": "indexed",
        "layer_count": layer_count, "group_count": 0,
        "layered_available": false, "layered_refusal": LAYERED_REFUSAL,
        "layered_note": "An Indexed Color file holds one 8-bit index plane; its stored composite \
                         is the image.",
        "stored_composite_available": composite_error.is_none(),
        "stored_composite_refusal": composite_error,
        "stored_composite_note": STORED_COMPOSITE_NOTE,
        "verified_losses": losses, "observations": observations,
        "general_limitations": IMPORT_LIMITATIONS,
        "profile": profile,
        "resolution": resolution.ok().flatten().map(|r| json!({"x": r.x, "y": r.y})),
        "metadata_captured": false,
        "requires_explicit_choice": true
    }))
}

impl PrecisionDocument {
    /// An Indexed Color PSD or PSB's stored composite as an Indexed Color
    /// document (module doc).
    pub fn open_indexed_psd(
        bytes: &[u8],
        path: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let container = Container::read(bytes).map_err(|e| format!("PSD {path}: {e}"))?;
        if container.color != 2 {
            return Err(format!(
                "PSD {path}: the file is not an Indexed Color PSD or PSB"
            ));
        }
        admission(&container).map_err(|e| format!("PSD {path}: {e}"))?;
        let scratch =
            crate::indexed_io::open_scratch(container.width, container.height, bytes.len() as u64)?;
        admit(0, scratch, other, limit)?;
        job.reserve(scratch)?;
        job.check()?;
        let (table, indices) =
            composite_indices(&container).map_err(|e| format!("PSD {path}: {e}"))?;
        job.check()?;
        let (profile, warning) = profile_of(&container).map_err(|e| format!("PSD {path}: {e}"))?;
        let mut warnings: Vec<String> = warning.into_iter().collect();
        if let Some(name) = container
            .layer_info()
            .ok()
            .and_then(|info| first_record_name(info, container.kind.large()))
            .filter(|name| name != LAYER_NAME)
        {
            warnings.push(format!(
                "The layer name “{name}” is not kept: an Indexed Color document's one layer is \
                 “{LAYER_NAME}”."
            ));
        }
        let resolution = container
            .resolution()
            .map_err(|e| format!("PSD {path}: {e}"))?
            .unwrap_or_default();
        let samplers = container
            .color_samplers()
            .map_err(|e| format!("PSD {path}: {e}"))?
            .kept;
        let policy = IndexedPolicy::new(
            table.palette,
            table.transparent,
            indices,
            container.width,
            container.height,
        )?;
        let image = policy.render(&profile, job)?;
        let mut document =
            Self::from_image_named(image, Metadata::default(), resolution, LAYER_NAME)?
                .with_import_warnings(warnings)?
                .with_indexed(Arc::new(policy))?;
        document.color_samplers = samplers;
        // Resources 1025 and 2000–2997 (`psd_paths`).
        document
            .paths
            .set_entries(crate::psd_paths::file_paths(&container).0);
        job.report(1.0, 5)?;
        Ok(document)
    }

    /// The export's refusal for `container` (module doc), in order: the
    /// layered TIFF, a side past the container's limit, a hidden layer.
    fn indexed_psd_refusal(&self, container: LayeredContainer) -> Option<String> {
        if container == LayeredContainer::Tiff {
            return Some(TIFF_REFUSAL.into());
        }
        // `bitmap_psd_refusal`'s side sentences, word for word (the master
        // copy is `psd_export_preflight::export_refusal`'s).
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
        if self.root_layers().iter().any(|layer| !layer.visible) {
            return Some(crate::indexed_export::HIDDEN_LAYER.into());
        }
        None
    }

    /// The image resources the export writes (module doc): the shared ones,
    /// then 1046 and 1047.
    fn indexed_psd_resources(&self, policy: &IndexedPolicy) -> Vec<u8> {
        let mut out = crate::psd_export_encode::image_resources(
            self.profile().bytes(),
            self.resolution(),
            None,
            self.color_samplers(),
            &crate::psd_paths::resources(self.paths(), self.dimensions()),
        );
        out.extend_from_slice(&resource(TABLE_COUNT, &count_body(policy.palette().len())));
        out.extend_from_slice(&resource(
            TRANSPARENT_INDEX,
            &transparent_body(policy.transparent()),
        ));
        out
    }

    /// The one layer's name.
    fn indexed_layer_name(&self) -> String {
        self.root_layers()
            .first()
            .map(|layer| (*layer.name).clone())
            .unwrap_or_else(|| LAYER_NAME.to_string())
    }

    /// The empty layer-and-mask section's length field: 8 bytes in a PSB,
    /// else 4.
    fn indexed_section_width(large: bool) -> usize {
        if large {
            8
        } else {
            4
        }
    }

    /// The largest file the export can write: the header, the table and its
    /// length, the resources, the empty layer section and the composite's
    /// worst case. `Err` only for a document that is not Indexed.
    fn indexed_psd_worst_case(&self, large: bool) -> Result<u64, String> {
        let policy = self
            .indexed_policy()
            .ok_or("an Indexed Color PSD export requires an Indexed Color document")?;
        let (w, h) = self.dimensions();
        let plane = plane_worst_case(u64::from(w), u64::from(h), large);
        Ok(
            (26 + 4 + crate::indexed_psd_records::COLOR_MODE_DATA_BYTES as u64 + 4)
                .saturating_add(self.indexed_psd_resources(policy).len() as u64)
                .saturating_add(Self::indexed_section_width(large) as u64)
                .saturating_add(plane),
        )
    }

    /// The export's working memory: the file's worst case, the planes the
    /// encoder takes and bookkeeping.
    fn indexed_psd_plan(&self, large: bool) -> Result<u64, String> {
        let (w, h) = self.dimensions();
        Ok(self
            .indexed_psd_worst_case(large)?
            .saturating_add(
                (pixel_count(w, h)? as u64)
                    .saturating_mul(crate::indexed_export::EXPORT_BYTES_PER_PIXEL),
            )
            .saturating_add(BOOKKEEPING_BYTES))
    }

    /// The export report (`dr_precision_doc_psd_export_report`'s Indexed
    /// arm): `bitmap_psd_report_json`'s keys with depth 8, `color_mode`
    /// "indexed" and the plan.
    pub(crate) fn indexed_psd_report_json(
        &self,
        container: LayeredContainer,
        mode: PsdExportMode,
    ) -> Value {
        let refusal = self.indexed_psd_refusal(container);
        let large = container.large();
        let mut losses = Vec::<String>::new();
        if !self.channels.is_empty() {
            losses.push(format!(
                "{} saved channels are not written; an Indexed Color PSD stores the index plane \
                 only.",
                self.channels.len()
            ));
        }
        if !self.guides().is_empty() || self.ruler_origin() != (0.0, 0.0) {
            losses.push("Guides and the ruler origin are not written.".into());
        }
        if !self.metadata().is_empty() {
            losses.push("EXIF/XMP/IPTC packets are not written.".into());
        }
        if self
            .root_layers()
            .iter()
            .any(|layer| layer.locks != 0 || layer.label != 0)
        {
            losses.push("The layer's locks and colour label are not written.".into());
        }
        if self.color_samplers().iter().any(|s| s.sample_size != 1) {
            losses.push("Color sampler sample sizes are not written; Photoshop reads every sampler at its own Sample Size.".into());
        }
        if self.indexed_layer_name() != LAYER_NAME {
            losses.push(NAME_LOSS.into());
        }
        let mut observations = Vec::<String>::new();
        if mode == PsdExportMode::Layers {
            observations.push(
                "An Indexed Color document's one layer is its composite, so Layers writes the \
                 same Indexed Color file as Flattened."
                    .into(),
            );
        }
        let (worst, plan) = match refusal {
            Some(_) => (0, 0),
            None => (
                self.indexed_psd_worst_case(large).unwrap_or(0),
                self.indexed_psd_plan(large).unwrap_or(0),
            ),
        };
        json!({
            "format": container.name(),
            "container": container.name(),
            "bake_scratch_bytes": 0,
            "mode": mode.name(),
            "depth": 8,
            "bit_depth": 8,
            "color_mode": "indexed",
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

    /// Writes this Indexed Color document as a colour-mode-2 PSD or PSB
    /// (module doc). Pure, and atomic through the shared temp-file rename;
    /// the report's refusal is this call's error, before anything is
    /// written.
    pub(crate) fn save_indexed_psd(
        &self,
        path: &str,
        container: LayeredContainer,
        _mode: PsdExportMode,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<(), String> {
        if let Some(refusal) = self.indexed_psd_refusal(container) {
            return Err(refusal);
        }
        let policy = self
            .indexed_policy()
            .ok_or("an Indexed Color PSD export requires an Indexed Color document")?
            .clone();
        let large = container.large();
        admit(
            self.retained_bytes(),
            self.indexed_psd_plan(large)?,
            other,
            limit,
        )?;
        let (w, h) = self.dimensions();
        if policy.dimensions() != (w, h) {
            return Err("the Indexed Color index plane does not match the canvas".into());
        }
        let resources = self.indexed_psd_resources(&policy);
        let resources_len = u32::try_from(resources.len())
            .map_err(|_| "PSD image resources exceed the format's 4 GiB section".to_string())?;
        // The empty layer-and-mask section: its length field alone, zero.
        let section = vec![0u8; Self::indexed_section_width(large)];
        let plane = encode_plane(policy.indices(), w, h, large);
        let table = color_mode_data(policy.palette());
        let mut out = crate::precision_image::reserved_vec(
            26 + 4 + table.len() + 4 + resources.len() + section.len() + plane.len(),
        )?;
        out.extend_from_slice(b"8BPS");
        out.extend_from_slice(&(if large { 2u16 } else { 1 }).to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&h.to_be_bytes());
        out.extend_from_slice(&w.to_be_bytes());
        out.extend_from_slice(&8u16.to_be_bytes());
        out.extend_from_slice(&2u16.to_be_bytes());
        out.extend_from_slice(&(table.len() as u32).to_be_bytes());
        out.extend_from_slice(&table);
        out.extend_from_slice(&resources_len.to_be_bytes());
        out.extend_from_slice(&resources);
        out.extend_from_slice(&section);
        out.extend_from_slice(&plane);
        job.check()?;
        crate::dr_image::save_bytes_atomically(path, &out, job)
    }
}
