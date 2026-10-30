//! The Grayscale clipboard (#547 phase 4): the Gray payload, its RZCB v5
//! codec, the paste across modes and the one-channel TIFF the pasteboard
//! carries beside it.
//!
//! # The payload
//!
//! [`GrayClipboardPayload`] is an RGB payload (`ClipboardPayload`) holding
//! R = G = B in the Gray profile's twin — exactly what Copy, Copy Merged and
//! Cut read from a Gray document, whose storage is the RGB precision
//! document's (`gray_document`'s module doc) — and the document's Gray
//! policy: the Gray ICC bytes and the declared depth (8, 16 or 32; f32
//! storage exactly at 32).
//!
//! # RZCB v5
//!
//! All little-endian: `b"RZCB"`, u32 version = 5, u8 depth (8, 16 or 32),
//! u32 ICC length (at most `MAX_DRDC_BLOB_LEN`) + the Gray ICC bytes, then
//! ONE complete RZCB v3 stream (`precision_clipboard_codec`'s module doc).
//! The decoder refuses a bad depth, an ICC `GrayPolicy::new` refuses, an
//! inner stream that is not version 3, an inner profile that is not
//! byte-equal to the policy's twin, storage disagreeing with the depth
//! (f32 ⇔ 32) and trailing bytes (the inner stream's own length check);
//! v1 to v4 decode as before (`cmyk_clipboard_codec::AnyClipboardPayload`).
//!
//! # Paste
//!
//! [`paste`] is `ffi_clipboard`'s dispatch whenever the document or the
//! payload is Gray. Between Gray documents the same ICC bytes paste bit for
//! bit; another Gray profile keeps the numbers under Don't Convert (the
//! depth step only, then a relabel) and converts under Convert
//! ([`ClipboardPayload::converted_to_gray`]: the luminance through the
//! source twin, encoded into the destination's). An RGB payload into a Gray
//! document converts by Image ▸ Mode ▸ Grayscale's rule; a CMYK payload
//! goes to ProPhoto RGB first (Mode ▸ Grayscale's CMYK stage), then to Gray.
//! A Gray payload into an RGB document is the RGB paste of its twin pixels
//! (Mode ▸ RGB Color's conversion), and into a CMYK document the RGB → CMYK
//! paste of them. Across modes Don't Convert refuses
//! ([`CROSS_MODE_PRESERVE`]).
//!
//! # Memory
//!
//! A conversion into Gray admits bookkeeping plus the Gray payload and its
//! encoding (`2·payload_bytes`) before allocating; the paste that follows
//! holds them, and before it runs the converting arm admits the insertion and
//! the host's display preparation of the result as a CMYK paste does
//! (`cmyk_clipboard::cmyk_paste_insertion`):
//! `doc_paste_layers::LAYER_PASTE_MARGIN_PER_PIXEL` over the canvas plus four
//! bookkeeping units. `scripts/gray-admission/measure.py`
//! (`paste-rgb-into-gray-16`) holds the sum to ⌈1.25 × the measured peak⌉;
//! the conversion term alone was 0.389 × the measured peak. The
//! pasteboard TIFF admits bookkeeping plus 8 B a window pixel
//! (`copy-gray-16`).
//!
//! # The pasteboard TIFF
//!
//! [`GrayClipboardPayload::gray_tiff`] writes the payload's gray (and its
//! alpha only when some sample is not opaque) as one-channel TIFF, 8 bits
//! (`round(v / 257)`) for a depth-8 policy, else 16, a float payload reduced
//! to 16 bits under its own tone map first, with the Gray ICC verbatim.
//! [`GrayClipboardPayload::from_gray_tiff`] reads such a TIFF from another
//! app (any gray TIFF `gray_io::gray_source` answers for) back as a payload.

use std::io::Cursor;
use std::sync::Arc;

use serde_json::Value;

use crate::cmyk_clipboard_codec::AnyClipboardPayload;
use crate::dr_image::Format;
use crate::drdc::MAX_DRDC_BLOB_LEN;
use crate::gray_document::{GrayDepth, GrayPolicy};
use crate::gray_io::GraySource;
use crate::gray_working::GrayWorkingSpace;
use crate::icc::IccProfile;
use crate::job_control::{DrJobControl, JobContext};
use crate::metadata::Resolution;
use crate::metadata_write::EncodeSidecar;
use crate::precision_clipboard::{ClipboardPayload, PasteProfile, PasteRequest};
use crate::precision_clipboard_codec::{payload_bytes, MAGIC};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{
    admit, pixel_count, reserved_vec, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES,
};

/// The Gray payload's version.
pub(crate) const VERSION_GRAY: u32 = 5;
/// The inner stream's version.
const VERSION_INNER: u32 = 3;
/// Magic, version, depth byte and ICC length word.
const HEADER: usize = 13;

/// The refusal of Don't Convert across colour modes (module doc).
pub(crate) const CROSS_MODE_PRESERVE: &str = "Don't Convert keeps numbers within one colour mode; pixels pasted between Grayscale and another mode are always converted";

const OVERFLOW: &str = "clipboard size overflow";

/// One Gray clipboard window (module doc).
#[derive(Clone)]
pub(crate) struct GrayClipboardPayload {
    pub(crate) rgb: ClipboardPayload,
    pub(crate) policy: Arc<GrayPolicy>,
}

/// The storage a Gray document of `depth` holds.
fn storage_of(depth: GrayDepth) -> PrecisionStorage {
    if depth == GrayDepth::ThirtyTwo {
        PrecisionStorage::Rgba32Float
    } else {
        PrecisionStorage::Rgba16Unorm
    }
}

impl GrayClipboardPayload {
    /// `rgb` under `policy`. Refuses pixels not in the policy's twin (by
    /// bytes) and storage that disagrees with the depth (f32 ⇔ 32).
    pub(crate) fn new(rgb: ClipboardPayload, policy: Arc<GrayPolicy>) -> Result<Self, String> {
        if rgb.image().profile().bytes() != policy.twin().bytes() {
            return Err(
                "a Grayscale clipboard payload's pixels must be in its Gray profile's twin".into(),
            );
        }
        if rgb.storage() != storage_of(policy.depth()) {
            return Err("a Grayscale clipboard payload's storage does not match its depth".into());
        }
        Ok(Self { rgb, policy })
    }

    /// The RZCB v5 stream (module doc).
    pub(crate) fn encode(&self) -> Result<Vec<u8>, String> {
        let icc = self.policy.bytes();
        let icc_len = u32::try_from(icc.len()).map_err(|_| "clipboard profile is too large")?;
        let mut header: Vec<u8> = reserved_vec(HEADER + icc.len())?;
        header.extend_from_slice(MAGIC);
        header.extend_from_slice(&VERSION_GRAY.to_le_bytes());
        header.push(self.policy.depth().bits() as u8);
        header.extend_from_slice(&icc_len.to_le_bytes());
        header.extend_from_slice(icc);
        // The inner stream is the bulk: prefix it in place rather than copy
        // it into a second buffer.
        let mut out = self.rgb.encode()?;
        out.try_reserve_exact(header.len())
            .map_err(|_| "clipboard payload size overflow")?;
        out.splice(0..0, header);
        Ok(out)
    }

    /// The RZCB v5 reader (module doc): every refusal before the inner
    /// stream's samples are allocated, which `ClipboardPayload::decode`
    /// admits.
    pub(crate) fn decode(bytes: &[u8], limit: u64, other: u64) -> Result<Self, String> {
        admit(0, BOOKKEEPING_BYTES, other, limit)?;
        if bytes.len() < HEADER || bytes[..4] != MAGIC[..] {
            return Err("clipboard payload is not an RZCB stream".into());
        }
        let word = |at: usize| {
            u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
        };
        let version = word(4);
        if version != VERSION_GRAY {
            return Err(format!(
                "clipboard payload version {version} is not a Grayscale payload (Grayscale payloads are version {VERSION_GRAY})"
            ));
        }
        let bits = bytes[8];
        let depth = GrayDepth::from_bits(u32::from(bits))
            .ok_or_else(|| format!("clipboard payload Gray depth {bits} is not 8, 16 or 32"))?;
        let icc_len = word(9) as usize;
        if icc_len == 0 || icc_len > MAX_DRDC_BLOB_LEN as usize {
            return Err("clipboard payload Gray profile length is out of range".into());
        }
        let inner_at = HEADER
            .checked_add(icc_len)
            .filter(|end| *end <= bytes.len())
            .ok_or("clipboard payload is shorter than its Gray profile")?;
        // ICC parsers can materialize sampled curves; 64 times the encoded
        // bytes bounds their parsed state (`ffi_precision_input`'s rule).
        admit(0, BOOKKEEPING_BYTES + icc_len as u64 * 64, other, limit)?;
        let policy = GrayPolicy::new(&bytes[HEADER..inner_at], depth)?;
        let inner = &bytes[inner_at..];
        let inner_version = (inner.len() >= 8 && inner[..4] == MAGIC[..])
            .then(|| u32::from_le_bytes([inner[4], inner[5], inner[6], inner[7]]));
        if inner_version != Some(VERSION_INNER) {
            return Err("a Grayscale clipboard payload must hold one RZCB version 3 stream".into());
        }
        let rgb = ClipboardPayload::decode(inner, limit, other)?;
        Self::new(rgb, Arc::new(policy))
    }

    /// The inner payload's description with `"mode":"gray"`, the version,
    /// the declared depth and the Gray profile's name.
    pub(crate) fn info(&self) -> Value {
        let mut info = self.rgb.info();
        info["version"] = VERSION_GRAY.into();
        info["mode"] = "gray".into();
        info["gray_depth"] = self.policy.depth().bits().into();
        info["gray_profile"] = self.policy.name().into();
        info
    }

    /// The payload's one-channel TIFF (module doc). Admits bookkeeping and
    /// 8 B a window pixel (the samples and the encoding) first; a float
    /// payload's 16-bit staging admits its own conversion.
    pub(crate) fn gray_tiff(&self, limit: u64, other: u64) -> Result<Vec<u8>, String> {
        let (w, h) = self.rgb.dimensions();
        let count = pixel_count(w, h)?;
        let scratch = (count as u64)
            .checked_mul(8)
            .and_then(|n| n.checked_add(BOOKKEEPING_BYTES))
            .ok_or(OVERFLOW)?;
        admit(0, scratch, other, limit)?;
        let own = self.rgb.image().profile().clone();
        let staged = self.rgb.converted_with_context(
            PrecisionStorage::Rgba16Unorm,
            &own,
            PasteProfile::Convert,
            limit,
            other.checked_add(scratch).ok_or(OVERFLOW)?,
            JobContext::NONE,
        )?;
        let source = staged.as_ref().unwrap_or(&self.rgb);
        let runs = || {
            source
                .image()
                .runs_rgba16()
                .ok_or("the Grayscale clipboard payload has no 16-bit samples")
        };
        let alpha = runs()?.flatten().any(|p| p[3] < u16::MAX);
        let channels = if alpha { 2 } else { 1 };
        let side = EncodeSidecar {
            icc: Some(self.policy.bytes()),
            dpi: Some(self.rgb.resolution()),
            ..EncodeSidecar::NONE
        };
        if self.policy.depth() == GrayDepth::Eight {
            let byte = |v: u16| ((u32::from(v) + 128) / 257) as u8;
            let mut samples: Vec<u8> = reserved_vec(count * channels)?;
            for pixel in runs()?.flatten() {
                samples.push(byte(pixel[0]));
                if alpha {
                    samples.push(byte(pixel[3]));
                }
            }
            crate::gray_encode::encode_gray8(&samples, alpha, (w, h), Format::Tiff, 0, &side)
        } else {
            let mut samples: Vec<u16> = reserved_vec(count * channels)?;
            for pixel in runs()?.flatten() {
                samples.push(pixel[0]);
                if alpha {
                    samples.push(pixel[3]);
                }
            }
            crate::gray_encode::encode_gray16(&samples, alpha, (w, h), Format::Tiff, &side)
        }
    }

    /// A pasteboard image that is a gray TIFF `gray_io::gray_source` answers
    /// for, as a payload at (0, 0) of its depth (8 or 16); `Ok(None)` for any
    /// other bytes. An embedded Gray ICC is kept, and one `GrayPolicy::new`
    /// refuses is an `Err`, as `open_gray` refuses it; an untagged TIFF is
    /// ASSIGNED `fallback_icc` (when it is a usable Gray profile), else the
    /// working Gray `working`, and the second value is the untagged sentence
    /// naming it. Admitted as a gray open (`gray_io::open_scratch`).
    pub(crate) fn from_gray_tiff(
        bytes: &[u8],
        fallback_icc: Option<&[u8]>,
        working: GrayWorkingSpace,
        limit: u64,
        other: u64,
    ) -> Result<Option<(Self, Option<String>)>, String> {
        if crate::gray_io::gray_source(bytes) != Some(GraySource::Tiff) {
            return Ok(None);
        }
        let icc = crate::gray_io::tiff_icc(Cursor::new(bytes)).flatten();
        let decoded = crate::gray_io::decode_samples(
            bytes,
            GraySource::Tiff,
            "the clipboard image",
            limit,
            other,
            JobContext::NONE,
        )?;
        let depth = decoded.depth;
        let (policy, assigned) = match icc {
            Some(icc) => (GrayPolicy::new(&icc, depth)?, None),
            None => {
                let policy = match fallback_icc.and_then(|icc| GrayPolicy::new(icc, depth).ok()) {
                    Some(policy) => policy,
                    None => GrayPolicy::for_space(working, depth)?,
                };
                let sentence = crate::gray_io::untagged_sentence(policy.name());
                (policy, Some(sentence))
            }
        };
        let (w, h) = decoded.stored;
        let count = pixel_count(w, h)?;
        let channels = decoded.channels;
        if decoded.samples.len() != count * channels {
            return Err(
                "the clipboard image's sample count differs from its dimensions".to_string(),
            );
        }
        let mut pixels: Vec<[u16; 4]> = reserved_vec(count)?;
        for sample in decoded.samples.chunks_exact(channels) {
            let gray = sample[0];
            let alpha = if channels == 2 { sample[1] } else { u16::MAX };
            pixels.push([gray, gray, gray, alpha]);
        }
        drop(decoded);
        let image = PrecisionImage::from_rgba16_decoded(w, h, pixels, policy.twin().clone(), None)?;
        let rgb = ClipboardPayload {
            image,
            offset: (0, 0),
            resolution: Resolution::default(),
            coverage: None,
            byte_origin: depth == GrayDepth::Eight,
            tone_map: None,
            editable: None,
        };
        Ok(Some((Self::new(rgb, Arc::new(policy))?, assigned)))
    }
}

impl ClipboardPayload {
    /// This payload as Gray pixels in `policy`'s twin at its depth (module
    /// doc): the depth step first, in the payload's own profile under
    /// Convert (f32 → u16 under its tone map, u16 → f32 promotion), then
    /// `gray_convert::gray_image` (Image ▸ Mode ▸ Grayscale's rule). Admits
    /// bookkeeping plus the result and its encoding before allocating.
    pub(crate) fn converted_to_gray(
        &self,
        policy: &GrayPolicy,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<ClipboardPayload, String> {
        job.check()?;
        let storage = storage_of(policy.depth());
        let own = self.image().profile().clone();
        let staged = self.converted_with_context(
            storage,
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
        let (w, h) = self.dimensions();
        let scratch = payload_bytes(w, h, storage, self.coverage.is_some())
            .checked_mul(2)
            .and_then(|n| n.checked_add(BOOKKEEPING_BYTES))
            .ok_or(OVERFLOW)?;
        admit(0, scratch, held, limit)?;
        job.reserve(scratch)?;
        let image = crate::gray_convert::gray_image(
            source.image(),
            &own,
            policy,
            false,
            job.range(0.3, 1.0),
        )?;
        let tone_map = (storage == PrecisionStorage::Rgba32Float).then(|| {
            source
                .tone_map()
                .unwrap_or(crate::float_document::ToneMap::Clip)
        });
        Ok(ClipboardPayload {
            image,
            offset: self.offset(),
            resolution: self.resolution(),
            coverage: self.coverage.clone(),
            byte_origin: false,
            tone_map,
            editable: self.editable.clone(),
        })
    }
}

/// The paste whenever `doc` or `payload` is Gray (module doc, "Paste").
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
    if let Some(policy) = doc.gray_policy() {
        // The conversion takes no job: the RGB paste that follows begins the
        // job control (`ffi_clipboard`'s CMYK → RGB arm).
        let converted = match payload {
            // `ffi_clipboard::dispatch_paste` sends a Lab payload to
            // `lab_clipboard` before this arm.
            AnyClipboardPayload::Lab(_) => {
                return Err("a Lab Color payload pastes through lab_clipboard".into());
            }
            AnyClipboardPayload::Gray(gray)
                if gray.policy.bytes() == policy.bytes() || !convert =>
            {
                return doc.clipboard_paste(&gray.rgb, request, limit, other, control);
            }
            AnyClipboardPayload::Gray(gray) => {
                gray.rgb
                    .converted_to_gray(policy, limit, other, JobContext::NONE)?
            }
            AnyClipboardPayload::Rgb(_) | AnyClipboardPayload::Cmyk(_) if !convert => {
                return Err(CROSS_MODE_PRESERVE.into());
            }
            AnyClipboardPayload::Rgb(rgb) => {
                rgb.converted_to_gray(policy, limit, other, JobContext::NONE)?
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
                rgb.converted_to_gray(policy, limit, held, JobContext::NONE)?
            }
        };
        let held = other
            .checked_add(2 * converted.payload_bytes())
            .ok_or(OVERFLOW)?;
        // The insertion and the host's display preparation of the result,
        // charged as a CMYK paste charges them (module doc, "Memory").
        let (w, h) = doc.dimensions();
        let insertion = (pixel_count(w, h)? as u64)
            .checked_mul(crate::doc_paste_layers::LAYER_PASTE_MARGIN_PER_PIXEL)
            .and_then(|n| n.checked_add(4 * BOOKKEEPING_BYTES))
            .ok_or(OVERFLOW)?;
        admit(doc.retained_bytes(), insertion, held, limit)?;
        return doc.clipboard_paste(&converted, request, limit, held, control);
    }
    let AnyClipboardPayload::Gray(gray) = payload else {
        return Err("the Grayscale paste needs a Grayscale document or payload".into());
    };
    if !convert {
        return Err(CROSS_MODE_PRESERVE.into());
    }
    if !doc.is_cmyk() {
        // Mode ▸ RGB Color's conversion: the twin's pixels into the
        // document's profile.
        return doc.clipboard_paste(&gray.rgb, request, limit, other, control);
    }
    let _run = control.map(DrJobControl::begin).transpose()?;
    let job = control.map_or(JobContext::NONE, JobContext::new);
    let policy = doc.require_cmyk_policy("Paste")?.clone();
    let converted = gray.rgb.converted_to_cmyk(
        &policy,
        policy.depth(),
        PasteProfile::Convert,
        limit,
        other,
        job.range(0.0, 0.6),
    )?;
    let held = other
        .checked_add(2 * converted.payload_bytes())
        .ok_or(OVERFLOW)?;
    doc.cmyk_clipboard_paste(&converted, request, limit, held, job.range(0.6, 1.0))
}
