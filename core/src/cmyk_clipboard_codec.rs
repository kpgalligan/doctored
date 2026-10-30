//! The CMYK clipboard PAYLOAD and its RZCB v4 codec (#1 phase 4c). The
//! operations over it live in `cmyk_clipboard`, its conversions in
//! `cmyk_payload_convert`.
//!
//! # The payload
//!
//! [`CmykClipboardPayload`] is one window of a CMYK document's raw ink —
//! straight CMYKA16 (`cmyk_image`), ink 0 = none — with the document's
//! policy (its profile's verbatim bytes), its DECLARED depth, the canvas
//! position the window came from, the resolution and, when a selection was
//! in force, the window's u16 coverage (already multiplied into alpha, as an
//! RGB payload's is) and, since #1's artwork batch, the host's EDITABLE
//! SECTION of a whole-layer Copy of a type, shape or fill layer (v3's
//! section, opaque beyond its shape). It never carries a tone map or a
//! byte-origin flag: a CMYK layer is never float, and its 8-bit codes are
//! already ×257.
//!
//! # RZCB v4
//!
//! v3's layout (`precision_clipboard_codec`'s module doc) with storage 3 =
//! CMYKA16 — `w·h·5` u16 `[c, m, y, k, a]` — and ONE more header word after
//! `editable_length`: `u32 cmyk_depth` (8 or 16, the source's declared
//! depth). All little-endian: `b"RZCB"`, u32 version = 4, u32 storage = 3,
//! u32 width, u32 height, i32 offset x, i32 offset y, f32 ppi x, f32 ppi y,
//! u32 profile length + the CMYK ICC bytes, u8 flags (bit0 coverage, bit2
//! editable), u32 tone-map operator (0) + f32 white, u32 editable_length,
//! u32 cmyk_depth, the editable section's `editable_length` UTF-8 bytes, the
//! samples, then `w·h` u16 coverage when bit0 is set. The section is v3's:
//! bit2 is set iff its length is nonzero, it is at most `MAX_DRDC_BLOB_LEN`
//! bytes and `precision_clipboard_codec::validate_section` checks its shape.
//!
//! The decoder REFUSES every malformed stream: a wrong magic, a version that
//! is not 4 ("…is not a CMYK payload…"), a storage code that is not 3, an
//! empty window, a non-finite or non-positive PPI, a profile length out of
//! range or a profile `CmykPolicy::new` refuses, flag bit 1 or bit 2
//! disagreeing with the editable length, a nonzero tone-map operator, an
//! editable section v3 would refuse, a depth word that is not 8 or 16 and a
//! length that does not match the header. Decoding is admitted
//! before the samples are allocated. [`AnyClipboardPayload`] is what the FFI
//! handle holds: it peeks the version word and hands a v4 stream here, a v5
//! stream to `gray_clipboard` (Grayscale phase 4), a v6 stream to
//! `lab_clipboard` (Lab phase 4c) and every other one to
//! `ClipboardPayload::decode`, which is untouched.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::cmyk_document::{CmykDepth, CmykPolicy};
use crate::cmyk_image::{CmykImage, CmykSample, CMYK_BYTES_PER_PIXEL};
use crate::drdc::MAX_DRDC_BLOB_LEN;
use crate::gray_clipboard::{GrayClipboardPayload, VERSION_GRAY};
use crate::lab_clipboard::{LabClipboardPayload, VERSION_LAB};
use crate::metadata::Resolution;
use crate::precision_clipboard_codec::{
    sane_resolution, section_info, validate_section, ClipboardPayload, Reader, FIXED_HEADER,
    FLAG_COVERAGE, FLAG_EDITABLE, MAGIC, TONE_WORDS,
};
use crate::precision_image::{admit, pixel_count, reserved_vec, BOOKKEEPING_BYTES};

/// The CMYK payload's version.
const VERSION_CMYK: u32 = 4;
/// CMYKA16 UNORM.
const STORAGE_CODE_CMYK: u32 = 3;

/// One CMYK clipboard window (module doc). Immutable; the ink is shared with
/// every document a paste produces.
#[derive(Clone)]
pub struct CmykClipboardPayload {
    pub(crate) image: CmykImage,
    pub(crate) policy: Arc<CmykPolicy>,
    pub(crate) depth: CmykDepth,
    pub(crate) offset: (i32, i32),
    pub(crate) resolution: Resolution,
    pub(crate) coverage: Option<Arc<Vec<u16>>>,
    /// The host's editable record (module doc), validated by
    /// `validate_section`.
    pub(crate) editable: Option<Arc<str>>,
}

/// The samples and the optional coverage plane of a `w × h` CMYK window.
pub(crate) fn cmyk_payload_bytes(width: u32, height: u32, coverage: bool) -> u64 {
    let n = u64::from(width) * u64::from(height);
    n * CMYK_BYTES_PER_PIXEL + if coverage { n * 2 } else { 0 }
}

impl CmykClipboardPayload {
    pub fn image(&self) -> &CmykImage {
        &self.image
    }
    pub fn policy(&self) -> &Arc<CmykPolicy> {
        &self.policy
    }
    pub fn depth(&self) -> CmykDepth {
        self.depth
    }
    pub fn offset(&self) -> (i32, i32) {
        self.offset
    }
    pub fn resolution(&self) -> Resolution {
        self.resolution
    }
    pub fn coverage(&self) -> Option<&[u16]> {
        self.coverage.as_deref().map(Vec::as_slice)
    }
    pub fn dimensions(&self) -> (u32, u32) {
        self.image.dimensions()
    }

    /// The host's editable section, if the payload carries one.
    pub fn editable(&self) -> Option<&str> {
        self.editable.as_deref()
    }

    /// The admission unit: the samples, the optional coverage plane and the
    /// optional editable section.
    pub fn payload_bytes(&self) -> u64 {
        let (w, h) = self.dimensions();
        cmyk_payload_bytes(w, h, self.coverage.is_some())
            + self.editable.as_ref().map_or(0, |s| s.len() as u64)
    }

    /// This payload with `section` as its editable section, validated and
    /// admitted exactly as `ClipboardPayload::with_editable` does (its rule
    /// and its sentences); an existing section is replaced.
    pub fn with_editable(&self, section: &str, limit: u64, other: u64) -> Result<Self, String> {
        let len = section.len() as u64;
        let retained = self
            .payload_bytes()
            .checked_add(self.policy.profile_bytes().len() as u64)
            .and_then(|n| n.checked_add(len.checked_mul(2)?))
            .ok_or("clipboard payload size overflow")?;
        admit(retained, BOOKKEEPING_BYTES + 2 * len, other, limit)?;
        let text = validate_section(section.as_bytes())?;
        let mut next = self.clone();
        next.editable = Some(Arc::from(text));
        Ok(next)
    }

    /// The host-facing description (never the samples).
    pub fn info(&self) -> Value {
        let (w, h) = self.dimensions();
        json!({
            "version": VERSION_CMYK,
            "mode": "cmyk",
            "storage": crate::cmyk_document::STORAGE_CMYK,
            "depth": 16,
            "origin_depth": self.depth.bits(),
            "cmyk_depth": self.depth.bits(),
            "width": w,
            "height": h,
            "offset": [self.offset.0, self.offset.1],
            "ppi": [self.resolution.x, self.resolution.y],
            "profile_description": self.policy.name(),
            "has_coverage": self.coverage.is_some(),
            "editable": section_info(self.editable.as_deref()),
        })
    }

    /// The RZCB v4 stream (module doc).
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        let (w, h) = self.dimensions();
        let profile = self.policy.profile_bytes();
        let section = self.editable.as_deref().unwrap_or("");
        let total = (FIXED_HEADER as u64 + profile.len() as u64 + 1 + TONE_WORDS as u64 + 8)
            .checked_add(self.payload_bytes())
            .and_then(|n| usize::try_from(n).ok())
            .ok_or("clipboard payload size overflow")?;
        let mut out: Vec<u8> = reserved_vec(total)?;
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION_CMYK.to_le_bytes());
        out.extend_from_slice(&STORAGE_CODE_CMYK.to_le_bytes());
        out.extend_from_slice(&w.to_le_bytes());
        out.extend_from_slice(&h.to_le_bytes());
        out.extend_from_slice(&self.offset.0.to_le_bytes());
        out.extend_from_slice(&self.offset.1.to_le_bytes());
        out.extend_from_slice(&self.resolution.x.to_le_bytes());
        out.extend_from_slice(&self.resolution.y.to_le_bytes());
        let profile_len =
            u32::try_from(profile.len()).map_err(|_| "clipboard profile is too large")?;
        out.extend_from_slice(&profile_len.to_le_bytes());
        out.extend_from_slice(profile);
        let mut flags = 0u8;
        if self.coverage.is_some() {
            flags |= FLAG_COVERAGE;
        }
        if !section.is_empty() {
            flags |= FLAG_EDITABLE;
        }
        out.push(flags);
        let map = crate::float_document::ToneMap::Clip;
        out.extend_from_slice(&map.code().to_le_bytes());
        out.extend_from_slice(&map.white().to_le_bytes());
        let section_len =
            u32::try_from(section.len()).map_err(|_| "clipboard editable section is too large")?;
        out.extend_from_slice(&section_len.to_le_bytes());
        out.extend_from_slice(&self.depth.bits().to_le_bytes());
        out.extend_from_slice(section.as_bytes());
        for y in 0..h {
            for (_, run) in self.image.row_segments(y, 0, w) {
                for v in run.as_flattened() {
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
        if let Some(coverage) = &self.coverage {
            for v in coverage.iter() {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        if out.len() != total {
            return Err("clipboard payload encoding length mismatch".into());
        }
        Ok(out)
    }

    /// The RZCB v4 reader (module doc): refuses every malformed stream and
    /// admits the decoded payload (and the handle's re-encoding) before
    /// allocating its samples.
    pub fn decode(bytes: &[u8], limit: u64, other: u64) -> Result<Self, String> {
        admit(0, BOOKKEEPING_BYTES, other, limit)?;
        let mut reader = Reader { bytes, at: 0 };
        if reader.take(4).ok() != Some(&MAGIC[..]) {
            return Err("clipboard payload is not an RZCB stream".into());
        }
        let version = reader.u32()?;
        if version != VERSION_CMYK {
            return Err(format!(
                "clipboard payload version {version} is not a CMYK payload (CMYK payloads are version {VERSION_CMYK})"
            ));
        }
        let storage = reader.u32()?;
        if storage != STORAGE_CODE_CMYK {
            return Err(format!(
                "clipboard payload version {VERSION_CMYK} holds only CMYK storage; storage code {storage} is not CMYK"
            ));
        }
        let width = reader.u32()?;
        let height = reader.u32()?;
        if width == 0 || height == 0 {
            return Err("clipboard payload has an empty window".into());
        }
        let count = pixel_count(width, height)?;
        let offset = (reader.i32()?, reader.i32()?);
        let resolution = Resolution {
            x: reader.f32()?,
            y: reader.f32()?,
        };
        sane_resolution(resolution)?;
        let profile_len = reader.u32()? as usize;
        if profile_len == 0 || profile_len > MAX_DRDC_BLOB_LEN as usize {
            return Err("clipboard payload profile length is out of range".into());
        }
        let profile_bytes = reader.take(profile_len)?;
        let flags = reader.take(1)?[0];
        if flags & !(FLAG_COVERAGE | FLAG_EDITABLE) != 0 {
            return Err(format!("clipboard payload flags {flags} are invalid"));
        }
        let operator = reader.u32()?;
        let _white = reader.f32()?;
        if operator != 0 {
            return Err(format!(
                "clipboard payload tone-map operator {operator} is invalid on CMYK storage"
            ));
        }
        let section_len = u64::from(reader.u32()?);
        if (flags & FLAG_EDITABLE != 0) != (section_len > 0) {
            return Err(format!(
                "clipboard payload editable flag disagrees with its section length {section_len}"
            ));
        }
        if section_len > u64::from(MAX_DRDC_BLOB_LEN) {
            return Err("clipboard editable section exceeds the size cap".into());
        }
        let bits = reader.u32()?;
        let depth = CmykDepth::from_bits(bits)
            .ok_or_else(|| format!("clipboard payload CMYK depth {bits} is not 8 or 16"))?;
        let has_coverage = flags & FLAG_COVERAGE != 0;
        let samples = cmyk_payload_bytes(width, height, has_coverage);
        let expected = (reader.at as u64)
            .checked_add(section_len)
            .and_then(|n| n.checked_add(samples))
            .ok_or("clipboard payload size overflow")?;
        if bytes.len() as u64 != expected {
            return Err(format!(
                "clipboard payload is {} bytes; its header declares {expected}",
                bytes.len()
            ));
        }
        // ICC parsers can materialize sampled curves; 64 times the encoded
        // bytes bounds their parsed state (`ffi_precision_input`'s rule).
        let scratch = BOOKKEEPING_BYTES + profile_len as u64 * 64;
        // The decoded samples and section plus the handle's own
        // re-encoding; the caller's input bytes arrive in `other`.
        admit(2 * (samples + section_len), scratch, other, limit)?;
        let editable = if section_len > 0 {
            Some(Arc::from(validate_section(
                reader.take(section_len as usize)?,
            )?))
        } else {
            None
        };
        let policy = Arc::new(CmykPolicy::new(profile_bytes, depth)?);
        let raw = reader.take(count * 10)?;
        let row = width as usize;
        let image = CmykImage::from_rows(width, height, |y, out| {
            let start = y as usize * row * 10;
            for (x, slot) in out.iter_mut().enumerate() {
                let at = start + x * 10;
                *slot = std::array::from_fn(|c| {
                    u16::from_le_bytes([raw[at + 2 * c], raw[at + 2 * c + 1]])
                });
            }
            Ok(())
        })?;
        let coverage = if has_coverage {
            let raw = reader.take(count * 2)?;
            let mut plane = reserved_vec(count)?;
            plane.extend(
                raw.chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]])),
            );
            Some(Arc::new(plane))
        } else {
            None
        };
        Ok(Self {
            image,
            policy,
            depth,
            offset,
            resolution,
            coverage,
            editable,
        })
    }

    /// This payload trimmed to its content box — the tightest rectangle
    /// holding every pixel whose alpha is above zero
    /// (`cmyk_stroke::cmyk_alpha_bounds`) — with its coverage windowed the
    /// same way and its offset moved by the box's origin: `ClipboardPayload::
    /// trim`'s rule (`precision_clipboard_trim`, the master). `Ok(None)`
    /// for a fully transparent or already tight payload. Admits bookkeeping,
    /// then the trimmed payload, its encoding and the profile bytes before
    /// allocating.
    pub fn trim(&self, limit: u64, other: u64) -> Result<Option<Self>, String> {
        admit(0, BOOKKEEPING_BYTES, other, limit)?;
        let (sw, sh) = self.dimensions();
        let job = crate::job_control::JobContext::NONE;
        let Some((x, y, w, h)) = crate::cmyk_stroke::cmyk_alpha_bounds(&self.image, job)? else {
            return Ok(None);
        };
        if (x, y, w, h) == (0, 0, sw, sh) {
            return Ok(None);
        }
        let offset = i32::try_from(x)
            .ok()
            .and_then(|x| self.offset.0.checked_add(x))
            .zip(
                i32::try_from(y)
                    .ok()
                    .and_then(|y| self.offset.1.checked_add(y)),
            )
            .ok_or("the trimmed clipboard payload's offset overflows")?;
        let retained = cmyk_payload_bytes(w, h, self.coverage.is_some())
            .checked_mul(2)
            .and_then(|n| n.checked_add(self.policy.profile_bytes().len() as u64))
            .ok_or("clipboard payload size overflow")?;
        admit(retained, BOOKKEEPING_BYTES, other, limit)?;
        let samples: Vec<CmykSample> = self.image.read_rect((x, y, w, h), job)?;
        let row = w as usize;
        let image = CmykImage::from_rows(w, h, |r, out| {
            let at = r as usize * row;
            out.copy_from_slice(&samples[at..at + row]);
            Ok(())
        })?;
        let coverage = match &self.coverage {
            None => None,
            Some(plane) => {
                let mut out = reserved_vec(pixel_count(w, h)?)?;
                for r in y..y + h {
                    let start = r as usize * sw as usize + x as usize;
                    out.extend_from_slice(&plane[start..start + w as usize]);
                }
                Some(Arc::new(out))
            }
        };
        Ok(Some(Self {
            image,
            policy: self.policy.clone(),
            depth: self.depth,
            offset,
            resolution: self.resolution,
            coverage,
            editable: self.editable.clone(),
        }))
    }
}

/// What the FFI handle holds: an RGB payload (RZCB v1–v3), a CMYK one (v4),
/// since Grayscale phase 4 a Gray one (v5, `gray_clipboard`) or, since Lab
/// phase 4c, a Lab one (v6, `lab_clipboard`).
#[derive(Clone)]
pub(crate) enum AnyClipboardPayload {
    Rgb(ClipboardPayload),
    Cmyk(CmykClipboardPayload),
    Gray(GrayClipboardPayload),
    Lab(LabClipboardPayload),
}

impl AnyClipboardPayload {
    /// The version word decides: 4 is [`CmykClipboardPayload::decode`], 5
    /// `GrayClipboardPayload::decode`, 6 `LabClipboardPayload::decode`,
    /// every other stream
    /// `ClipboardPayload::decode` (which refuses what it does not read).
    pub(crate) fn decode(bytes: &[u8], limit: u64, other: u64) -> Result<Self, String> {
        let version = (bytes.len() >= 8 && bytes[..4] == MAGIC[..])
            .then(|| u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]));
        match version {
            Some(VERSION_CMYK) => CmykClipboardPayload::decode(bytes, limit, other).map(Self::Cmyk),
            Some(VERSION_GRAY) => GrayClipboardPayload::decode(bytes, limit, other).map(Self::Gray),
            Some(VERSION_LAB) => LabClipboardPayload::decode(bytes, limit, other).map(Self::Lab),
            _ => ClipboardPayload::decode(bytes, limit, other).map(Self::Rgb),
        }
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, String> {
        match self {
            Self::Rgb(payload) => payload.encode(),
            Self::Cmyk(payload) => payload.encode(),
            Self::Gray(payload) => payload.encode(),
            Self::Lab(payload) => payload.encode(),
        }
    }

    pub(crate) fn info(&self) -> Value {
        match self {
            Self::Rgb(payload) => payload.info(),
            Self::Cmyk(payload) => payload.info(),
            Self::Gray(payload) => payload.info(),
            Self::Lab(payload) => payload.info(),
        }
    }

    /// The payload's profile bytes: the RGB ICC, the CMYK policy's, the
    /// Gray ICC or the Lab twin's.
    pub(crate) fn profile_bytes(&self) -> &[u8] {
        match self {
            Self::Rgb(payload) => payload.image().profile().bytes(),
            Self::Cmyk(payload) => payload.policy.profile_bytes(),
            Self::Gray(payload) => payload.policy.bytes(),
            Self::Lab(payload) => payload.policy.twin().bytes(),
        }
    }
}
