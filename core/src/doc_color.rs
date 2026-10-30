//! Colour management and metadata as DOCUMENT operations: assign versus
//! convert, one-shot working-space adoption for ordinary byte-image opens,
//! the print resolution, the metadata packets, and the document-level image
//! save that embeds all of it. The profile model is `icc`, the numerics
//! `icc_transform`, the packets and the container walk `metadata` /
//! `metadata_write`; `doc.rs` only carries the three fields through its ops.
//!
//! # Assign is not Convert
//!
//! **Assign** reinterprets: the pixels are untouched and the profile is
//! replaced, so the image's NUMBERS stay and its appearance changes.
//! **Convert** transforms: every layer's pixels move through the transform
//! so the appearance stays and the numbers change. The two stay separate:
//! collapsing them is the classic way to destroy a photograph quietly.
//!
//! Convert touches **layer pixels only**. Layer masks and alpha channels are
//! coverage rather than colour and are left byte-identical; an adjustment
//! layer's `meta` is left alone for a different reason, worth stating because
//! it is the one place "the picture looks the same" stops being true — a
//! curve point or a saturation amount is a PARAMETER, not a colour, so there
//! is nothing to convert it into, and it is then applied to numbers that have
//! moved. Blend modes are the same story one level up. Each LAYER keeps its
//! appearance; a stack whose result is computed from the numbers does not,
//! and the host says so rather than promising otherwise
//! (`ColorProfileControls.appearanceDependsOnNumbers`).
//!
//! So are the colours inside a layer STYLE and a text layer's `meta`, and
//! that is correct rather than a limit: those are AUTHORED sRGB values, and
//! they reach the document's space at the point they are used — a style's at
//! composite time (`style_composite`, whose module doc says why there rather
//! than in the cache), a text layer's when the host re-renders it through a
//! document-space context. Converting them here as well would convert them
//! twice.
//!
//! # What a save actually writes
//!
//! Not every format can carry everything, so [`format_carries`] is the
//! capability table as a query — the host asks the same question the core
//! answers, and [`DrDocument::save_image`] reports what was ACTUALLY
//! written. A packet the format cannot carry, one too large for its segment,
//! or an 8BIM run that filters down to nothing is **dropped and reported,
//! never an error**: a save must not fail because a source file had a fat
//! profile.
//!
//! TIFF uses a typed encoder directly and a bounded capture-IFD relocator.
//! Its ICC and rational density tags round-trip. Standard EXIF/GPS/Interop
//! fields relocate with valid offsets and a MakerNote relocates as one
//! opaque value; any other private EXIF field refuses the TIFF EXIF packet
//! as a whole so loss reports stay true.

use std::ffi::c_int;
use std::io::Read;
use std::sync::Arc;

use crate::doc::DrDocument;
use crate::dr_image::{save_flat, Format};
use crate::drdc::MAX_DRDC_BLOB_LEN;
use crate::export_metadata::{MetadataOutcome, MetadataPolicy};
use crate::icc::IccProfile;
use crate::metadata::{self, Metadata, Resolution};
use crate::metadata_write::{
    exif_normalized, iptc_filtered, EncodeSidecar, JPEG_EXIF_PREFIX, JPEG_MAX_PAYLOAD,
};
use crate::metadata_xmp::{xmp_minimal, xmp_normalized, xmp_stamped};
use crate::metadata_xmp_split::jpeg_xmp_segments;
use crate::DrImage;

/// The capability bits, mirroring `DR_CARRIES_*` in the C header.
pub const CARRIES_PROFILE: u32 = 1;
pub const CARRIES_EXIF: u32 = 2;
pub const CARRIES_XMP: u32 = 4;
pub const CARRIES_IPTC: u32 = 8;
pub const CARRIES_RESOLUTION: u32 = 16;

/// The largest ICC profile a JPEG can carry: 255 APP2 chunks of
/// `65533 - 14` payload bytes each. Checked here so an oversize profile is
/// DROPPED rather than turned into a failed export by the encoder's own
/// "ICC profile too large" error.
const JPEG_MAX_ICC: usize = (JPEG_MAX_PAYLOAD - 14) * 255;

/// What an open did to the document's colour, so a host can say so once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdoptOutcome {
    /// The file's profile already IS the working space (within the
    /// equivalence tolerance), so nothing was touched.
    Unchanged,
    /// The pixels were transformed into the working space.
    Converted,
    /// The file's profile is one this crate cannot convert FROM, so it was
    /// kept as the document's profile and no pixel was touched. Display,
    /// painting and re-embedding are all still correct; only Convert to
    /// Profile refuses.
    KeptUnconvertible,
}

/// Which metadata packet an FFI call means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetadataKind {
    Exif,
    Xmp,
    Iptc,
}

impl MetadataKind {
    /// Maps a raw `DrMetadataKind` value coming across the FFI. Mapped,
    /// never transmuted: callers do pass values outside the enum, and
    /// materializing an enum from an out-of-range discriminant would be
    /// undefined behaviour.
    pub(crate) fn from_c(value: c_int) -> Option<Self> {
        match value {
            0 => Some(MetadataKind::Exif),
            1 => Some(MetadataKind::Xmp),
            2 => Some(MetadataKind::Iptc),
            _ => None,
        }
    }

    /// [`from_c`](Self::from_c) for a setter that names its refusal — the
    /// ONE home of the sentence both depths' `set_metadata` shims answer.
    pub(crate) fn from_c_checked(value: c_int) -> Result<Self, String> {
        Self::from_c(value).ok_or_else(|| format!("unknown metadata kind {value}"))
    }

    pub(crate) fn get(self, meta: &Metadata) -> Option<&Arc<[u8]>> {
        match self {
            MetadataKind::Exif => meta.exif.as_ref(),
            MetadataKind::Xmp => meta.xmp.as_ref(),
            MetadataKind::Iptc => meta.iptc.as_ref(),
        }
    }

    pub(crate) fn slot(self, meta: &mut Metadata) -> &mut Option<Arc<[u8]>> {
        match self {
            MetadataKind::Exif => &mut meta.exif,
            MetadataKind::Xmp => &mut meta.xmp,
            MetadataKind::Iptc => &mut meta.iptc,
        }
    }
}

/// The DRDC blob cap every metadata setter enforces, so a document can never
/// hold a packet the writer would refuse — the ONE home of the sentence both
/// depths' setters answer. A clear (`None`) always passes.
pub(crate) fn check_metadata_packet(bytes: Option<&[u8]>) -> Result<(), String> {
    if bytes.is_some_and(|b| b.len() > MAX_DRDC_BLOB_LEN as usize) {
        return Err("metadata packet exceeds the 16 MiB limit".into());
    }
    Ok(())
}

/// Whether the opener scans this container's capture fields. True for JPEG,
/// PNG, classic TIFF and native .rz. Packet-specific limits still apply;
/// TIFF's diagnostic report distinguishes supported fields from detected
/// omissions. False for unreadable files and other containers, whose capture
/// fields this core does not read.
///
/// Asked of the PATH rather than of the document because it describes the
/// OPEN, not the document: a document carries no memory of which container
/// it came out of, and inventing one would mean a field every pure op had to
/// carry and the native format had to persist. Reading the magic costs eight
/// bytes.
pub fn metadata_walked(path: &str) -> bool {
    // `read_to_end` over a `take`, not one `read`: a single read is allowed
    // to return fewer bytes than asked for, and answering "not walked"
    // because of a short read would be a wrong answer about a real JPEG.
    let mut head = Vec::with_capacity(8);
    let read = std::fs::File::open(path).and_then(|f| f.take(8).read_to_end(&mut head));
    if read.is_err() {
        return false;
    }
    crate::native_magic::is_native_magic(&head) || metadata::walks(&head)
}

/// Which of the profile, packets and resolution `format` is able to carry —
/// the ONE capability table, asked as a number so the host and the core can
/// never drift:
///
/// | Format | profile | exif | xmp | iptc | resolution |
/// |---|---|---|---|---|---|
/// | PNG | `iCCP` | `eXIf` | our `iTXt` | — | our `pHYs` |
/// | JPEG | APP2 x n | APP1 | our APP1 + ExtendedXMP APP1 x n | our APP13 x n | JFIF density |
/// | TIFF | ICC tag | standard IFDs | tag 700 | tag 34377 | rational PPI |
/// | WebP | chunk | chunk | — | — | — |
/// | HEIC, AVIF | ICC (host) | host EXIF | host XMP | — | host TIFF/EXIF |
/// | BMP, GIF, JPEG XL, OpenEXR, Radiance HDR | — | — | — | — | — |
///
/// JPEG XL's row is empty because `jxl_encode` writes a bare codestream: no
/// container, so no profile and no packet (the export refuses pixels that are
/// not sRGB instead). OpenEXR's and Radiance's are empty because neither
/// container has an ICC slot or a packet slot this crate writes: the float
/// export states the document profile's PRIMARIES instead (`float_io`).
///
/// The HEIC and AVIF row is the HOST encoder's capability, not this crate's:
/// nothing here writes either. It governs exactly two things — the export request's
/// `metadata` policy refusal and the packet report on the result the host's
/// bytes are adopted into — and never a sidecar, because the export
/// workspace encodes HEIC's intermediate as a TIFF.
///
/// The resolution column is the format's OWN density slot, which is why
/// WebP's is empty even though a WebP export can still state the ppi: it
/// does so inside the EXIF chunk, and only when there is a packet to write.
/// [`DrDocument::save_image`] therefore reports `CARRIES_RESOLUTION` from
/// this table **or** from the normalized packet, never from the table
/// alone — a report that said "dropped" about a number the file plainly
/// contains would be worse than saying nothing.
pub(crate) fn format_carries(format: Format) -> u32 {
    match format {
        Format::Png => CARRIES_PROFILE | CARRIES_EXIF | CARRIES_XMP | CARRIES_RESOLUTION,
        Format::Jpeg => {
            CARRIES_PROFILE | CARRIES_EXIF | CARRIES_XMP | CARRIES_IPTC | CARRIES_RESOLUTION
        }
        Format::Tiff => {
            CARRIES_PROFILE | CARRIES_EXIF | CARRIES_XMP | CARRIES_IPTC | CARRIES_RESOLUTION
        }
        Format::Webp => CARRIES_PROFILE | CARRIES_EXIF,
        Format::Heic | Format::Avif => {
            CARRIES_PROFILE | CARRIES_EXIF | CARRIES_XMP | CARRIES_RESOLUTION
        }
        Format::Bmp | Format::Gif | Format::Jxl | Format::Exr | Format::Hdr => 0,
    }
}

impl DrDocument {
    /// Reinterprets the document in `p`: pixels unchanged, profile replaced.
    /// `Ok(None)` when the document already carries exactly those bytes, so
    /// an assign that changes nothing registers no undo step. `Err`, in this
    /// order: `assign_refusal`'s sentence while any layer still holds a
    /// RETAINED SOURCE, then `assign_profile_refusal`'s,
    /// then a failed rebuild of the evaluation tables.
    ///
    /// The first refusal is the 8A rule made explicit rather than silent:
    /// the fallback pixels are a render of an original asset in a specific
    /// profile, so retagging them while the original keeps its own profile
    /// reinterprets a master the user never agreed to reinterpret. The
    /// disclosed way through is `assign_profile_rasterizing`; both live in
    /// `doc_color_policy`.
    ///
    /// The second (`doc_color_destination`) is a destination with no
    /// matrix/TRC model while a `document_v2` Exposure/White Balance layer
    /// exists or the document uses linear-light evaluation, which would land
    /// a document that cannot render; and a destination whose
    /// matrix/TRC tone curves fail the precision check while the document
    /// uses linear-light evaluation, holds text/shape artwork or holds a
    /// `document_v2` Exposure/White Balance layer, which would have to encode
    /// into it (`icc_encode_refusal`'s Assign sentences).
    pub fn assign_profile(&self, p: &Arc<IccProfile>) -> Result<Option<Self>, String> {
        if Arc::ptr_eq(&self.profile, p) || self.profile.bytes() == p.bytes() {
            return Ok(None);
        }
        if let Some(refusal) = self.assign_refusal() {
            return Err(refusal);
        }
        if let Some(refusal) = self.assign_profile_refusal(p) {
            return Err(refusal);
        }
        let mut out = self.clone();
        out.profile = Arc::clone(p);
        // Assign changes the meaning of fallback codes. An original-profile
        // source may only survive an appearance-preserving conversion.
        for layer in &mut out.layers {
            layer.retained_source = None;
        }
        // The evaluation policy's tables belong to the profile they were built
        // from, so a profile write rebuilds them (`doc_evaluation`).
        out.rebind_evaluation()?;
        Ok(Some(out))
    }

    /// Transforms every LAYER's pixels into `p` and replaces the profile, so
    /// the picture looks the same and its numbers change. Masks, channels
    /// and layer `meta` are untouched (see the module doc).
    ///
    /// `Ok(None)` when the two describe the same colour space, so no phantom
    /// undo step is registered. `Err` is `convert_to_profile_with`'s refusal
    /// verbatim: the preflight's sentences and a profile this crate cannot
    /// read.
    ///
    /// Since Phase 11C this is exactly `convert_to_profile_with` at its
    /// DEFAULT options (relative colorimetric, no BPC, keep parameters),
    /// which on two matrix/TRC profiles runs the same `Transform` it always
    /// did — the bytes are unchanged and `export_pipeline`'s equalities
    /// check it. The report is dropped.
    pub fn convert_to_profile(&self, p: &Arc<IccProfile>) -> Result<Option<Self>, String> {
        Ok(self
            .convert_to_profile_with(
                p,
                &crate::doc_color_policy::ConversionOptions::DEFAULT,
                crate::job_control::JobContext::NONE,
            )?
            .map(|(document, _)| document))
    }

    /// The open-time colour rule for ordinary byte images, run once by the
    /// host: whatever produced the pixels, the document is first
    /// assigned the profile those pixel NUMBERS actually belong to, and then
    /// this converts them into the working space if the two differ.
    ///
    /// The outcome is reported even when no document comes back, so a host
    /// can tell "already in the working space" from "kept, could not be
    /// converted from".
    ///
    /// Native documents and explicit PSD imports skip this: they retain their
    /// source profile and samples, including single-layer/composite PSD choices.
    /// Converting them to a preference on every open would silently rewrite
    /// the user's document.
    pub fn adopt_working_space(&self, working: &Arc<IccProfile>) -> (Option<Self>, AdoptOutcome) {
        if self.profile.model().is_none() {
            return (None, AdoptOutcome::KeptUnconvertible);
        }
        match self.convert_to_profile(working) {
            Ok(Some(doc)) => (Some(doc), AdoptOutcome::Converted),
            // Either the working space is not modellable (impossible for a
            // built-in), the conversion refused, or the two spaces already
            // agree.
            _ => (None, AdoptOutcome::Unchanged),
        }
    }

    /// Sets the print resolution in ppi. Pixels are never touched — only the
    /// print size changes. `Err` for a non-finite or non-positive component
    /// (`Resolution::checked`); `Ok(None)` when nothing changes after
    /// sanitizing (clamped to [1, 30000] and quantized to four decimals like
    /// the global light, so a host echoing a reported value back is refused
    /// instead of registering a phantom edit).
    pub fn set_resolution(&self, x: f32, y: f32) -> Result<Option<Self>, String> {
        let next = Resolution::checked(x, y)?;
        if next == self.resolution {
            return Ok(None);
        }
        let mut out = self.clone();
        out.resolution = next;
        Ok(Some(out))
    }

    /// Stores one metadata packet verbatim, or clears it with `None`.
    /// `Err` for a payload past the DRDC blob cap (`check_metadata_packet`)
    /// — enforced here so a document can never hold a packet the
    /// writer would refuse. `Ok(None)` for a value the document already
    /// carries.
    pub fn set_metadata(
        &self,
        kind: MetadataKind,
        bytes: Option<&[u8]>,
    ) -> Result<Option<Self>, String> {
        check_metadata_packet(bytes)?;
        let current = kind.get(&self.metadata).map(|b| &**b);
        if current == bytes {
            return Ok(None);
        }
        let mut out = self.clone();
        *kind.slot(&mut out.metadata) = bytes.map(|b| Arc::from(b.to_vec()));
        Ok(Some(out))
    }

    /// Encodes the document to `path` as a flat image, embedding its colour
    /// profile and — unless `strip_metadata` — its EXIF, XMP and IPTC
    /// packets, format permitting, and always its print resolution where the
    /// format has somewhere to put one. Returns the `CARRIES_*` bits that
    /// were ACTUALLY written.
    ///
    /// `flat` is a caller-supplied composite of THIS document (the host's
    /// always-warm projection); passing an unrelated image is a caller bug,
    /// and the canvas dimensions are not re-checked. `None` flattens here.
    /// The parameter exists because re-compositing the whole layer stack —
    /// styles, masks, clipping — on every save would be a visible stall on
    /// the app's most frequent command, and because the export path captures
    /// its composite deliberately BEFORE the save panel opens.
    ///
    /// `strip_metadata` governs the three inherited packets only. The
    /// document's own resolution and colour profile are its state, not
    /// something it inherited from a file, so they follow the format's
    /// capability and `embed_profile`.
    ///
    /// Atomic exactly like `dr_image_save`.
    pub(crate) fn save_image(
        &self,
        flat: Option<&DrImage>,
        path: &str,
        format: Format,
        jpeg_quality: u8,
        embed_profile: bool,
        strip_metadata: bool,
    ) -> Result<u32, String> {
        let flattened;
        let pixels = match flat {
            Some(image) => &image.pixels,
            None => {
                flattened = self.flattened()?;
                &flattened
            }
        };
        let (w, h) = pixels.dimensions();
        let (mut sidecar, carried) =
            self.image_sidecar(format, embed_profile, strip_metadata, w, h);
        // A TIFF keeps the spot channels as extra samples (#547 1c-2a).
        if format == Format::Tiff && self.has_spot_channel() {
            let transparency = pixels.pixels().any(|p| p[3] < 255);
            sidecar.spots = Some(self.tiff_spot_planes(transparency));
        }
        save_flat(pixels, path, format, jpeg_quality, &sidecar)?;
        Ok(carried)
    }

    /// The save and preservation preflight share normalization and packet limits.
    /// No pixels are rendered or encoded while constructing this sidecar.
    pub(crate) fn image_sidecar(
        &self,
        format: Format,
        embed_profile: bool,
        strip_metadata: bool,
        w: u32,
        h: u32,
    ) -> (EncodeSidecar<'_>, u32) {
        image_sidecar(
            &self.profile,
            &self.metadata,
            self.resolution,
            format,
            embed_profile,
            strip_metadata,
            (w, h),
        )
    }
}

/// Shared packet policy for byte and typed image writers. Pixel precision does
/// not change metadata normalization, relocation or carried-packet reporting.
///
/// This is the [`MetadataPolicy::DEFAULT`] case of
/// [`image_sidecar_with_policy`], kept as its own entry point because
/// `save_image`, the preservation preflight and the typed codec ask exactly
/// this question and must keep writing exactly these bytes.
pub(crate) fn image_sidecar<'a>(
    profile: &'a IccProfile,
    metadata: &Metadata,
    resolution: Resolution,
    format: Format,
    embed_profile: bool,
    strip_metadata: bool,
    dimensions: (u32, u32),
) -> (EncodeSidecar<'a>, u32) {
    let (sidecar, carried, _) = image_sidecar_with_policy(
        profile,
        metadata,
        resolution,
        format,
        embed_profile,
        strip_metadata,
        dimensions,
        &MetadataPolicy::DEFAULT,
    );
    (sidecar, carried)
}

/// The same packet policy under an explicit [`MetadataPolicy`]: each packet's
/// three-valued rule replaces the single `strip_metadata` switch, the
/// container's own density slot may be dropped, and an optional stamp is
/// written into the XMP packet. The normalizers are called exactly as above —
/// orientation is reset and the document's ppi restated in every packet this
/// writes, because the pixels are already upright.
///
/// The third result is the [`MetadataOutcome`] a host reports: the same
/// information as the `CARRIES_*` bits, plus whether the stamp landed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn image_sidecar_with_policy<'a>(
    profile: &'a IccProfile,
    metadata: &Metadata,
    resolution: Resolution,
    format: Format,
    embed_profile: bool,
    strip_metadata: bool,
    dimensions: (u32, u32),
    policy: &MetadataPolicy,
) -> (EncodeSidecar<'a>, u32, MetadataOutcome) {
    let (w, h) = dimensions;
    let caps = format_carries(format);
    let is_jpeg = format == Format::Jpeg;
    let mut carried = 0u32;

    let icc = profile.bytes();
    let icc = if embed_profile
        && caps & CARRIES_PROFILE != 0
        && (!is_jpeg || icc.len() <= JPEG_MAX_ICC)
    {
        carried |= CARRIES_PROFILE;
        Some(&**icc)
    } else {
        None
    };

    // The ONE place the policy's three-valued rule becomes a boolean, so the
    // sidecar and `MetadataPolicy::refusal` can never disagree about which
    // packets this export writes.
    let keep = |rule: crate::export_metadata::PacketRule, bit: u32| {
        MetadataPolicy::keeps(rule, strip_metadata) && caps & bit != 0
    };
    // The EXIF packet is rewritten before it is written: orientation to
    // 1, the resolution, colour-space and dimension tags to this
    // document's, IFD1 unlinked. A packet that does not verify — or one
    // whose resolution states something this document does not and
    // cannot be corrected, which would outrank the density this save
    // writes — is dropped whole.
    let exif = keep(policy.exif, CARRIES_EXIF)
        .then_some(metadata.exif.as_deref())
        .flatten()
        .and_then(|blob| exif_normalized(blob, w, h, resolution.sane(), profile.describes_srgb()))
        .filter(|e| !is_jpeg || e.bytes.len() + JPEG_EXIF_PREFIX <= JPEG_MAX_PAYLOAD)
        .filter(|e| {
            format != Format::Tiff || crate::metadata_tiff::export_exif(&e.bytes).is_some()
        });
    if exif.is_some() {
        carried |= CARRIES_EXIF;
    }

    // The XMP packet carries its own copies of the orientation and the
    // resolution, and XMP's reconciliation rules make THEM the authority
    // — so it gets the same one-way normalization the EXIF block does,
    // and the size check runs on what will actually be written. A JPEG
    // gets its segments built here, ONCE, on that final packet: one APP1,
    // or a standard packet and its ExtendedXMP chunks; a packet that can
    // be neither is dropped.
    //
    // The stamp rides on this packet: a document that carries XMP gets its
    // normalized packet stamped in place (or written unchanged, reporting
    // `stamped: false`, when it has no `rdf:Description` to stamp), and one
    // that carries none gets a minimal packet holding nothing but the credit.
    let mut stamped = false;
    let mut jpeg_xmp = Vec::new();
    let xmp = keep(policy.xmp, CARRIES_XMP)
        .then(|| match (metadata.xmp.as_deref(), policy.stamp.as_ref()) {
            (Some(blob), None) => Some(xmp_normalized(blob, resolution.sane())),
            (Some(blob), Some(stamp)) => {
                let normalized = xmp_normalized(blob, resolution.sane());
                Some(match xmp_stamped(&normalized, stamp) {
                    Some(packet) => {
                        stamped = true;
                        packet
                    }
                    None => normalized,
                })
            }
            (None, Some(stamp)) => {
                stamped = true;
                Some(xmp_minimal(stamp))
            }
            (None, None) => None,
        })
        .flatten()
        .filter(|blob| blob.len() <= MAX_DRDC_BLOB_LEN as usize)
        .filter(|blob| {
            !is_jpeg
                || jpeg_xmp_segments(blob)
                    .map(|segments| jpeg_xmp = segments)
                    .is_some()
        });
    if xmp.is_some() {
        carried |= CARRIES_XMP;
    } else {
        // A packet filtered out by a size limit carries no stamp either.
        stamped = false;
    }

    // The 8BIM run loses the resources that would contradict what this
    // save writes; a run that filters down to nothing means no APP13. No
    // JPEG size limit applies: a run larger than one segment is split
    // across consecutive APP13s (`jpeg_app13_iptc`), and the run is already
    // within the blob cap that `capture` and `set_metadata` enforce.
    let iptc = keep(policy.iptc, CARRIES_IPTC)
        .then_some(metadata.iptc.as_deref())
        .flatten()
        .and_then(iptc_filtered)
        .filter(|run| !run.is_empty());
    if iptc.is_some() {
        carried |= CARRIES_IPTC;
    }

    // The format's own density slot (the JFIF APP0, the PNG `pHYs`). A
    // `density: drop` policy suppresses it — and `MetadataPolicy::refusal`
    // has already established that no kept packet restates the number.
    let dpi = (caps & CARRIES_RESOLUTION != 0 && MetadataPolicy::keeps(policy.density, false))
        .then(|| resolution.sane());
    // The file states this document's ppi if that slot was written OR
    // the EXIF packet came out saying it — which is how a WebP export
    // states a resolution at all.
    //
    // The density slot alone is enough BECAUSE `exif_normalized` never
    // writes a packet that states a different resolution: Exif/DCF makes
    // IFD0 the authority over a JFIF or `pHYs` density, so a packet that
    // could not be corrected is dropped there rather than left to
    // outrank the number written here.
    if dpi.is_some() || exif.as_ref().is_some_and(|e| e.states_resolution) {
        carried |= CARRIES_RESOLUTION;
    }

    let outcome = MetadataOutcome {
        profile: carried & CARRIES_PROFILE != 0,
        exif: exif.is_some(),
        xmp: xmp.is_some(),
        iptc: iptc.is_some(),
        density: carried & CARRIES_RESOLUTION != 0,
        stamped,
    };
    (
        EncodeSidecar {
            icc,
            exif: exif.map(|e| e.bytes),
            xmp,
            jpeg_xmp,
            iptc,
            dpi,
            spots: None,
        },
        carried,
        outcome,
    )
}
