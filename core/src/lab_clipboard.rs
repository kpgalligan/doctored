//! The Lab Color clipboard (#1 Lab phase 4c): the Lab payload, its RZCB
//! v6 codec and the paste across modes. Gray's clipboard is the template
//! (`gray_clipboard`); nothing here adds pixel math — every conversion is
//! `lab_convert::converted_image` through a `ColorPipeline`.
//!
//! # The payload
//!
//! [`LabClipboardPayload`] is an RGB payload (`ClipboardPayload`) holding
//! the L\*a\*b\* codes in the Lab twin, u16 storage — exactly what Copy, Copy
//! Merged and Cut read from a Lab document, whose storage is the RGB
//! precision document's (`lab_document`'s module doc) — and the document's
//! Lab policy, of which only the declared depth (8 or 16) is written: the
//! twin is built in. It never carries the byte-origin flag or a tone map.
//!
//! # RZCB v6
//!
//! All little-endian: `b"RZCB"`, u32 version = 6, u8 depth (8 or 16), then
//! ONE complete RZCB v3 stream (`precision_clipboard_codec`'s module doc).
//! The decoder admits bookkeeping first, then refuses — reading the inner
//! header in place, before the inner decoder allocates any sample — a
//! version that is not 6, a depth that is not 8 or 16, an inner stream that
//! is not version 3, inner storage that is not RGBA16, an inner profile not
//! byte-equal to the twin and the byte-origin flag; the inner decoder
//! refuses trailing bytes (its own length check). v1 to v5 decode as before
//! (`cmyk_clipboard_codec::AnyClipboardPayload`).
//!
//! # Paste
//!
//! [`paste`] is `ffi_clipboard`'s dispatch whenever the document or the
//! payload is Lab, asked before Gray's:
//!
//! * **Lab → Lab** is the same twin, so Convert and Don't Convert both keep
//!   the codes; a payload of either depth into an 8-bit document is first
//!   put on the ×257 grid ([`LabClipboardPayload::quantized_to`],
//!   `lab_depth`'s grid; phase 6a-i), any payload into a 16-bit document is
//!   unchanged. An 8-bit payload is quantized too because the declared depth
//!   promises nothing about its codes: 8-bit Lab paint, geometry and selected
//!   pixels leave off-grid u16 codes until an 8-bit save (the companion's
//!   Decisions), so the app's own 8-bit copy may be off the grid, and a
//!   decode refusal would reject it. New from Clipboard quantizes an 8-bit
//!   payload the same way (`ffi_clipboard`).
//! * **RGB → Lab** ([`ClipboardPayload::converted_to_lab`]) is Image ▸ Mode
//!   ▸ Lab Color's rule: a float payload to u16 in its own profile under its
//!   tone map first (Gray's step), then relative colorimetric into the twin
//!   with no BPC (`lab_convert::to_lab_pipeline`), each code quantized by
//!   the document's `LabPolicy::code`. A Gray payload converts its RGB twin
//!   pixels (Mode ▸ Lab Color's Gray route); a CMYK payload goes to 16-bit
//!   ProPhoto RGB first (CMYK → Lab's stage).
//! * **Lab → RGB** ([`LabClipboardPayload::converted_to_rgb`]) runs the twin
//!   into the destination relative colorimetric WITH black point
//!   compensation (`lab_cmyk::stage_options`: the clipboard's cross-mode
//!   rule, as RGB ↔ CMYK pastes, and Color Settings' default), full u16,
//!   alpha kept. Into an RGB document it lands in the document's profile
//!   and pastes as RGB (a float document promotes); into a Gray or CMYK
//!   document it goes to 16-bit ProPhoto RGB and pastes through that mode's
//!   own RGB arm (`ffi_clipboard::dispatch_paste`).
//! * Across modes Don't Convert refuses ([`CROSS_MODE_PRESERVE`]), both
//!   ways, and so does [`LabClipboardPayload::to_rgba8_in`] given a target.
//!
//! # Memory
//!
//! Each conversion admits bookkeeping, `lab_convert::TO_LAB_BYTES_PER_PIXEL`
//! a window pixel (the converted output and the display preparation the
//! host runs on the result, the Mode conversions' measured term) and the
//! pipeline's prepared tables before allocating. The converting paste then
//! admits Gray's insertion term — `doc_paste_layers::
//! LAYER_PASTE_MARGIN_PER_PIXEL` over the canvas plus four bookkeeping units
//! — with twice the converted payload held. `scripts/lab-admission/
//! measure.py` (`paste-rgb-into-lab-16`, `paste-lab-into-rgb-16`) holds the
//! sum to ⌈1.25 × the measured peak⌉; a miss raises `TO_LAB_BYTES_PER_PIXEL`
//! alone.
//!
//! # The pasteboard image
//!
//! Other apps see the RGB rendering (the host's TIFF from
//! [`LabClipboardPayload::to_rgba8_in`] with no target: Display P3, the Lab
//! view's space); the internal payload keeps the codes. The conversion runs
//! under the first half of the job's progress and the 8-bit read under the
//! second (the CMYK
//! payload's shape), so progress reaches 1 and a cancel in either is seen.

use std::sync::Arc;

use image::RgbaImage;
use serde_json::Value;

use crate::cmyk_clipboard_codec::AnyClipboardPayload;
use crate::icc::IccProfile;
use crate::icc_intent::ColorPipeline;
use crate::job_control::{DrJobControl, JobContext};
use crate::lab_document::{lab_twin, LabDepth, LabPolicy};
use crate::precision_clipboard::{ClipboardPayload, PasteProfile, PasteRequest};
use crate::precision_clipboard_codec::{Reader, FIXED_HEADER, FLAG_BYTE_ORIGIN, MAGIC};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{
    admit, pixel_count, rgba8_preview_scratch, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::precision_quantize::PrecisionDither;

/// The Lab payload's version.
pub(crate) const VERSION_LAB: u32 = 6;
/// The inner stream's version.
const VERSION_INNER: u32 = 3;
/// The inner stream's RGBA16 storage code (`precision_clipboard_codec`).
const STORAGE_U16: u32 = 1;
/// Magic, version and depth byte.
const HEADER: usize = 9;

/// The refusal of Don't Convert across colour modes (module doc).
pub(crate) const CROSS_MODE_PRESERVE: &str = "Don't Convert keeps numbers within one colour mode; pixels pasted between Lab Color and another mode are always converted";

const OVERFLOW: &str = "clipboard size overflow";
const NOT_TWIN: &str = "a Lab Color clipboard payload's pixels must be in the Lab twin";
const NOT_U16: &str = "a Lab Color clipboard payload must hold 16-bit storage";
const BYTE_ORIGIN: &str = "a Lab Color clipboard payload never carries the byte-origin flag";

/// One Lab clipboard window (module doc).
#[derive(Clone)]
pub(crate) struct LabClipboardPayload {
    pub(crate) rgb: ClipboardPayload,
    pub(crate) policy: Arc<LabPolicy>,
}

/// What one conversion of a `w × h` window through `pipeline` admits
/// (module doc, "Memory").
fn conversion_scratch(dimensions: (u32, u32), pipeline: &ColorPipeline) -> Result<u64, String> {
    let (w, h) = dimensions;
    (pixel_count(w, h)? as u64)
        .checked_mul(crate::lab_convert::TO_LAB_BYTES_PER_PIXEL)
        .and_then(|n| n.checked_add(pipeline.scratch_bytes()))
        .and_then(|n| n.checked_add(BOOKKEEPING_BYTES))
        .ok_or_else(|| OVERFLOW.to_string())
}

/// `rgb` with `image` in place of its pixels: no byte origin and no tone
/// map (the result is u16 in a converted profile), every other field kept.
fn with_image(
    rgb: &ClipboardPayload,
    image: crate::precision_image::PrecisionImage,
) -> ClipboardPayload {
    ClipboardPayload {
        image,
        offset: rgb.offset(),
        resolution: rgb.resolution(),
        coverage: rgb.coverage.clone(),
        byte_origin: false,
        tone_map: None,
        editable: rgb.editable.clone(),
    }
}

impl LabClipboardPayload {
    /// `rgb` under `policy`. Refuses pixels not in the twin (by bytes),
    /// storage that is not RGBA16 and the byte-origin flag (module doc).
    pub(crate) fn new(rgb: ClipboardPayload, policy: Arc<LabPolicy>) -> Result<Self, String> {
        if rgb.image().profile().bytes() != lab_twin().bytes() {
            return Err(NOT_TWIN.into());
        }
        if rgb.storage() != PrecisionStorage::Rgba16Unorm {
            return Err(NOT_U16.into());
        }
        if rgb.byte_origin() {
            return Err(BYTE_ORIGIN.into());
        }
        Ok(Self { rgb, policy })
    }

    /// The RZCB v6 stream (module doc).
    pub(crate) fn encode(&self) -> Result<Vec<u8>, String> {
        let mut header = Vec::with_capacity(HEADER);
        header.extend_from_slice(MAGIC);
        header.extend_from_slice(&VERSION_LAB.to_le_bytes());
        header.push(self.policy.depth().bits() as u8);
        // The inner stream is the bulk: prefix it in place rather than copy
        // it into a second buffer (Gray's rule).
        let mut out = self.rgb.encode()?;
        out.try_reserve_exact(header.len())
            .map_err(|_| "clipboard payload size overflow")?;
        out.splice(0..0, header);
        Ok(out)
    }

    /// The RZCB v6 reader (module doc): every refusal before the inner
    /// stream's samples are allocated, which `ClipboardPayload::decode`
    /// admits.
    pub(crate) fn decode(bytes: &[u8], limit: u64, other: u64) -> Result<Self, String> {
        admit(0, BOOKKEEPING_BYTES, other, limit)?;
        if bytes.len() < HEADER || bytes[..4] != MAGIC[..] {
            return Err("clipboard payload is not an RZCB stream".into());
        }
        let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if version != VERSION_LAB {
            return Err(format!(
                "clipboard payload version {version} is not a Lab Color payload (Lab Color payloads are version {VERSION_LAB})"
            ));
        }
        let bits = bytes[8];
        let depth = LabDepth::from_bits(u32::from(bits))
            .ok_or_else(|| format!("clipboard payload Lab depth {bits} is not 8 or 16"))?;
        let inner = &bytes[HEADER..];
        // The inner header in place (`precision_clipboard_codec`'s layout:
        // magic, version, storage, … the profile length word, the profile,
        // the flag byte), so a payload that is not Lab codes refuses before
        // any sample is allocated.
        let mut reader = Reader {
            bytes: inner,
            at: 0,
        };
        let magic = reader.take(4).ok();
        let inner_version = reader.u32().ok();
        if magic != Some(&MAGIC[..]) || inner_version != Some(VERSION_INNER) {
            return Err("a Lab Color clipboard payload must hold one RZCB version 3 stream".into());
        }
        if reader.u32()? != STORAGE_U16 {
            return Err(NOT_U16.into());
        }
        reader.at = FIXED_HEADER - 4;
        let profile_len = reader.u32()? as usize;
        let twin = lab_twin();
        if reader.take(profile_len)? != &twin.bytes()[..] {
            return Err(NOT_TWIN.into());
        }
        if reader.take(1)?[0] & FLAG_BYTE_ORIGIN != 0 {
            return Err(BYTE_ORIGIN.into());
        }
        let rgb = ClipboardPayload::decode(inner, limit, other)?;
        Self::new(rgb, Arc::new(LabPolicy::for_depth(depth)))
    }

    /// The inner payload's description with `"mode":"lab"`, the version and
    /// the declared depth.
    pub(crate) fn info(&self) -> Value {
        let mut info = self.rgb.info();
        info["version"] = VERSION_LAB.into();
        info["mode"] = "lab".into();
        info["lab_depth"] = self.policy.depth().bits().into();
        info
    }

    /// This payload as u16 RGB pixels in `profile` (module doc, Lab → RGB):
    /// the twin through `lab_cmyk::stage_options` (relative colorimetric
    /// with BPC), full u16, alpha kept. Admits the conversion's charge
    /// (module doc, "Memory") before allocating.
    pub(crate) fn converted_to_rgb(
        &self,
        profile: &Arc<IccProfile>,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<ClipboardPayload, String> {
        job.check()?;
        let options = crate::lab_cmyk::stage_options();
        let pipeline = ColorPipeline::new(
            self.policy.twin(),
            profile,
            options.intent,
            options.black_point_compensation,
        )?;
        let scratch = conversion_scratch(self.rgb.dimensions(), &pipeline)?;
        admit(0, scratch, other, limit)?;
        job.reserve(scratch)?;
        let wide = |unit: f64| (unit.clamp(0.0, 1.0) * 65535.0).round() as u16;
        let image =
            crate::lab_convert::converted_image(self.rgb.image(), &pipeline, profile, &wide, job)?;
        Ok(with_image(&self.rgb, image))
    }

    /// This payload at `policy`'s depth: into an 8-bit document a payload of
    /// EITHER depth with every sample on the ×257 grid (`lab_depth::on_grid`,
    /// the 16 → 8 depth edit's rule, which is `LabPolicy::code`'s; an 8-bit
    /// payload may hold off-grid codes, module doc "Paste"); into a 16-bit
    /// document unchanged. Admits bookkeeping and the grid image first.
    pub(crate) fn quantized_to(
        &self,
        policy: &Arc<LabPolicy>,
        limit: u64,
        other: u64,
    ) -> Result<Self, String> {
        if policy.depth() != LabDepth::Eight {
            return Ok(Self {
                rgb: self.rgb.clone(),
                policy: policy.clone(),
            });
        }
        let scratch = self
            .rgb
            .payload_bytes()
            .checked_add(BOOKKEEPING_BYTES)
            .ok_or(OVERFLOW)?;
        admit(0, scratch, other, limit)?;
        let image = crate::lab_depth::on_grid(self.rgb.image(), JobContext::NONE)?;
        Self::new(with_image(&self.rgb, image), policy.clone())
    }

    /// The payload's pixels as 8-bit RGBA (module doc, "The pasteboard
    /// image"): `profile` `None` is Display P3, the Lab view's space (the
    /// CMYK payload's rule); else [`Self::converted_to_rgb`] into it under
    /// the job's first half, then `PrecisionImage::to_rgba8`'s rule under its
    /// second, the intermediate counted as other retained bytes (the CMYK
    /// payload's shape). Don't Convert with a target refuses
    /// ([`CROSS_MODE_PRESERVE`]).
    pub(crate) fn to_rgba8_in(
        &self,
        profile: Option<&Arc<IccProfile>>,
        mode: PasteProfile,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<RgbaImage, String> {
        if mode == PasteProfile::Preserve && profile.is_some() {
            return Err(CROSS_MODE_PRESERVE.into());
        }
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let target = profile.cloned().unwrap_or_else(IccProfile::display_p3);
        let rgb = self.converted_to_rgb(&target, limit, other, job.range(0.0, 0.5))?;
        let held = other.checked_add(rgb.payload_bytes()).ok_or(OVERFLOW)?;
        let (w, h) = rgb.dimensions();
        let scratch = rgba8_preview_scratch(w, h)?;
        admit(rgb.image().retained_bytes(), scratch, held, limit)?;
        job.reserve(scratch)?;
        rgb.image()
            .quantize_with_context(PrecisionDither::None, job.range(0.5, 1.0))
    }
}

impl ClipboardPayload {
    /// This payload as Lab codes in `policy`'s twin at its depth (module
    /// doc, RGB → Lab): a float payload to u16 in its own profile under its
    /// tone map first, then `lab_convert::to_lab_pipeline` of its profile,
    /// each code quantized by `policy.code`. Admits the conversion's charge
    /// (module doc, "Memory") before allocating.
    pub(crate) fn converted_to_lab(
        &self,
        policy: &LabPolicy,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<ClipboardPayload, String> {
        job.check()?;
        let own = self.image().profile().clone();
        let staged = self.converted_with_context(
            PrecisionStorage::Rgba16Unorm,
            &own,
            PasteProfile::Convert,
            limit,
            other,
            job.range(0.0, 0.3),
        )?;
        let source = staged.as_ref().unwrap_or(self);
        let held = other
            .checked_add(staged.as_ref().map_or(0, ClipboardPayload::payload_bytes))
            .ok_or(OVERFLOW)?;
        let pipeline = crate::lab_convert::to_lab_pipeline(&own)?;
        let scratch = conversion_scratch(self.dimensions(), &pipeline)?;
        admit(0, scratch, held, limit)?;
        job.reserve(scratch)?;
        let image = crate::lab_convert::converted_image(
            source.image(),
            &pipeline,
            policy.twin(),
            &|unit| policy.code(unit),
            job.range(0.3, 1.0),
        )?;
        Ok(with_image(source, image))
    }
}

/// The insertion and the host's display preparation of a converted paste's
/// result into `doc`, admitted with `held` (module doc, "Memory": Gray's
/// term).
fn admit_insertion(doc: &PrecisionDocument, held: u64, limit: u64) -> Result<(), String> {
    let (w, h) = doc.dimensions();
    let insertion = (pixel_count(w, h)? as u64)
        .checked_mul(crate::doc_paste_layers::LAYER_PASTE_MARGIN_PER_PIXEL)
        .and_then(|n| n.checked_add(4 * BOOKKEEPING_BYTES))
        .ok_or(OVERFLOW)?;
    admit(doc.retained_bytes(), insertion, held, limit)
}

/// `other` plus twice `payload`'s bytes: a converted payload and its
/// encoding, held while the paste runs.
fn holding(other: u64, payload: &ClipboardPayload) -> Result<u64, String> {
    payload
        .payload_bytes()
        .checked_mul(2)
        .and_then(|n| n.checked_add(other))
        .ok_or_else(|| OVERFLOW.to_string())
}

/// The paste whenever `doc` or `payload` is Lab (module doc, "Paste").
pub(crate) fn paste(
    doc: &PrecisionDocument,
    payload: &AnyClipboardPayload,
    request: &PasteRequest,
    limit: u64,
    other: u64,
    control: Option<&DrJobControl>,
) -> Result<PrecisionDocument, String> {
    crate::cmyk_clipboard::paste_request_refusal(request)?;
    let convert = request.profile == PasteProfile::Convert;
    if let Some(policy) = doc.lab_policy() {
        // The conversions take no job: the RGB paste that follows begins the
        // job control (`ffi_clipboard`'s CMYK → RGB arm).
        let converted = match payload {
            // Into 16 bits as-is; into 8 bits on the grid whatever the
            // payload declares (module doc, "Paste").
            AnyClipboardPayload::Lab(lab) if policy.depth() == LabDepth::Sixteen => {
                return doc.clipboard_paste(&lab.rgb, request, limit, other, control);
            }
            AnyClipboardPayload::Lab(lab) => {
                let quantized = lab.quantized_to(policy, limit, other)?;
                let held = holding(other, &quantized.rgb)?;
                return doc.clipboard_paste(&quantized.rgb, request, limit, held, control);
            }
            _ if !convert => return Err(CROSS_MODE_PRESERVE.into()),
            AnyClipboardPayload::Rgb(rgb) => {
                rgb.converted_to_lab(policy, limit, other, JobContext::NONE)?
            }
            AnyClipboardPayload::Gray(gray) => {
                gray.rgb
                    .converted_to_lab(policy, limit, other, JobContext::NONE)?
            }
            AnyClipboardPayload::Cmyk(cmyk) => {
                let rgb = cmyk.converted_to_rgb(
                    &IccProfile::prophoto_rgb(),
                    PrecisionStorage::Rgba16Unorm,
                    PasteProfile::Convert,
                    limit,
                    other,
                    JobContext::NONE,
                )?;
                let held = other.checked_add(rgb.payload_bytes()).ok_or(OVERFLOW)?;
                rgb.converted_to_lab(policy, limit, held, JobContext::NONE)?
            }
        };
        let held = holding(other, &converted)?;
        admit_insertion(doc, held, limit)?;
        return doc.clipboard_paste(&converted, request, limit, held, control);
    }
    let AnyClipboardPayload::Lab(lab) = payload else {
        return Err("the Lab Color paste needs a Lab Color document or payload".into());
    };
    if !convert {
        return Err(CROSS_MODE_PRESERVE.into());
    }
    if doc.is_gray() || doc.is_cmyk() {
        // Through 16-bit ProPhoto RGB, then that mode's own RGB arm.
        let rgb =
            lab.converted_to_rgb(&IccProfile::prophoto_rgb(), limit, other, JobContext::NONE)?;
        let held = holding(other, &rgb)?;
        let staged = AnyClipboardPayload::Rgb(rgb);
        return crate::ffi_clipboard::dispatch_paste(doc, &staged, request, limit, held, control);
    }
    let rgb = lab.converted_to_rgb(doc.profile(), limit, other, JobContext::NONE)?;
    let held = holding(other, &rgb)?;
    admit_insertion(doc, held, limit)?;
    doc.clipboard_paste(&rgb, request, limit, held, control)
}
