//! The typed clipboard's PAYLOAD and its RZCB codec (Phase 12G-1, v2 in
//! 12G-2, v3 in 12G-3). The operations over it live in `precision_clipboard`.
//!
//! # The payload
//!
//! [`ClipboardPayload`] is one window of raw samples in the SOURCE
//! document's storage and profile, the canvas position the window came from,
//! the document's resolution and, when a selection was in force, the
//! window's u16 coverage. A float payload also carries its source document's
//! tone map, so it renders the same wherever it lands, and a payload made
//! from 8-bit pixels (`from_rgba8`) carries the BYTE-ORIGIN flag: its u16
//! samples are exactly `v · 257`. The public TIFF/PNG a host writes beside it
//! is a RENDERING; this is the master. A whole-layer Copy of an editable
//! layer (12G-3) also carries that layer's EDITABLE SECTION — the host's
//! record, opaque to the core beyond the shape checked below.
//!
//! # RZCB v2
//!
//! All little-endian: `b"RZCB"`, u32 version = 2, u32 storage (1 = RGBA16
//! UNORM, 2 = RGBA32 float), u32 width, u32 height, i32 offset x, i32 offset
//! y, f32 ppi x, f32 ppi y, u32 profile length + the ICC bytes, u8 flags
//! (bit0 coverage present, bit1 byte origin), u32 tone-map operator (0 clip /
//! 1 reinhard) + f32 white — the native `FLOT` table's two words
//! (`float_native`), read through `ToneMap::from_code` — then `w·h·4`
//! samples (u16 or f32), then `w·h` u16 coverage when bit0 is set. A u16
//! payload writes operator 0 with `ToneMap::Clip.white()` and must carry
//! operator 0 (its white is ignored); the byte-origin bit is refused on f32
//! storage and unless EVERY u16 sample, alpha included, is a multiple of 257.
//!
//! # RZCB v3
//!
//! Version 3 is v2 with ONE more header word after `f32 white`: `u32
//! editable_length`. Flag bit2 (editable) is set iff that length is nonzero
//! — a disagreement is refused — and the section is the LAST
//! `editable_length` bytes, after the coverage. A v3 flag byte is at most 7;
//! a v2 one stays at most 3 with no length word. The section is UTF-8, at
//! most `MAX_DRDC_BLOB_LEN` bytes, and a JSON object holding exactly a string
//! `type`, a non-empty string `record`, an optional string `name` and an
//! optional `anchor` of two finite numbers; anything else is refused by
//! name. The core never interprets `record`. The encoder writes version 3
//! only.
//!
//! # RZCB v4
//!
//! Version 4 is a CMYK payload, read and written only by
//! `cmyk_clipboard_codec::CmykClipboardPayload` (#547 phase 4c; that
//! module's doc is the format), whose editable section shares
//! `validate_section` and `section_info` with v3. [`ClipboardPayload::decode`] still refuses it
//! at its version check, and storage code 3 under versions 1–3 is still
//! unknown here.
//!
//! # RZCB v5
//!
//! Version 5 is a Grayscale payload (#547 phase 4), read and written only by
//! `gray_clipboard::GrayClipboardPayload` (that module's doc is the format):
//! a depth byte and the Gray ICC, then one complete v3 stream of this
//! module's, R = G = B in the Gray profile's twin. [`ClipboardPayload::decode`]
//! refuses it at its version check.
//!
//! Version 1 — 12G-1's stream, which existed only transiently on a
//! pasteboard — still decodes: its flag byte is `has_coverage` ∈ {0, 1}, it
//! has no tone-map words, and an f32 payload reads as `Clip`.
//!
//! The decoder REFUSES — it never sanitizes — every malformed stream: a
//! wrong magic, version or storage code, an empty or oversized window, a
//! flag or tone-map word outside the rules above, a length that does not
//! match the header, a profile `IccProfile::parse` does not accept (or
//! accepts only by trimming), a non-finite or non-positive PPI, and any f32
//! sample outside the stored extended range
//! (`precision_image::extended_pixel`). Decoding is admitted BEFORE the
//! samples are allocated.
//!
//! `ClipboardPayload` (the CODEC module) is one canvas window of raw samples in the source
//! storage/profile with its offset, PPI, optional u16 coverage, a BYTE-ORIGIN flag (u16 samples
//! exactly `v·257` of 8-bit pixels, `from_rgba8`), on f32 the source document's tone map and, since
//! 12G-3, an optional host EDITABLE SECTION. RZCB v3 is its little-endian codec — v1 plus a `u8
//! flags` byte (bit0 coverage, bit1 byte origin, bit2 editable), the native `FLOT` words (`u32`
//! operator, `f32` white, through `ToneMap::from_code`) and a `u32 editable_length` word, with the
//! section as the stream's LAST bytes — and the decoder refuses (never sanitizes) a bad
//! magic/version/storage, an empty/oversized window, flags above 7 (above 3 on v2), bit2
//! disagreeing with the length, bit1 on f32 or with any u16 sample not a multiple of 257, a nonzero
//! operator on u16, a map `from_code` refuses, a length mismatch, a profile `IccProfile::parse`
//! rejects or trims, a non-finite PPI, any f32 sample failing `extended_pixel`, and a section that
//! is not UTF-8 JSON of exactly a string `type`, a non-empty string `record`, an optional string
//! `name` and an optional two-finite-number `anchor` (≤ `MAX_DRDC_BLOB_LEN`; `validate_section` is
//! shared with `with_editable`, which admits the new handle — payload + profile + `2·len` retained,
//! bookkeeping + `2·len` scratch — before parsing or cloning); v1/v2 still decode (a v1 float
//! payload reads as `Clip`), the encoder writes v3, the core never reads `record`, and `converted`
//! carries the section. `to_rgba8_in(profile, mode)` (`dr_clipboard_payload_rgba8`'s
//! `profile`/`preserve`) is `converted_with(Rgba16Unorm, profile, mode)` — an f32 payload under its
//! own tone map — then the u16 read with the intermediate counted as other; `to_rgba8` is it with
//! the payload's own profile under Convert.

use std::sync::Arc;

use image::RgbaImage;
use serde_json::{json, Value};

use crate::drdc::MAX_DRDC_BLOB_LEN;
use crate::float_document::ToneMap;
use crate::icc::IccProfile;
use crate::job_control::{DrJobControl, JobContext};
use crate::metadata::Resolution;
use crate::precision_clipboard::PasteProfile;
use crate::precision_image::{
    admit, extended_pixel, pixel_count, reserved_vec, rgba8_preview_scratch, PrecisionImage,
    PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::precision_quantize::PrecisionDither;

pub(crate) const MAGIC: &[u8; 4] = b"RZCB";
/// The version this build writes.
const VERSION: u32 = 3;
/// 12G-2's stream, still read: no editable-length word.
const VERSION_2: u32 = 2;
/// 12G-1's stream, still read.
const VERSION_1: u32 = 1;
const STORAGE_U16: u32 = 1;
const STORAGE_F32: u32 = 2;
/// Magic through the profile length word: ten 4-byte fields.
pub(crate) const FIXED_HEADER: usize = 40;
/// The v2 words after the flag byte: tone-map operator and white.
pub(crate) const TONE_WORDS: usize = 8;
pub(crate) const FLAG_COVERAGE: u8 = 1;
pub(crate) const FLAG_BYTE_ORIGIN: u8 = 2;
/// v3: the stream ends with an editable section.
pub(crate) const FLAG_EDITABLE: u8 = 4;

/// One typed clipboard window. Immutable; pixels are `Arc`-shared with every
/// document a paste produces.
#[derive(Clone)]
pub struct ClipboardPayload {
    pub(crate) image: PrecisionImage,
    pub(crate) offset: (i32, i32),
    pub(crate) resolution: Resolution,
    pub(crate) coverage: Option<Arc<Vec<u16>>>,
    /// The samples are exactly `v · 257` of 8-bit pixels (u16 storage only).
    pub(crate) byte_origin: bool,
    /// `Some` iff the storage is f32: the source document's map.
    pub(crate) tone_map: Option<ToneMap>,
    /// The host's editable record (v3), validated by [`validate_section`].
    pub(crate) editable: Option<Arc<str>>,
}

fn storage_name(storage: PrecisionStorage) -> &'static str {
    match storage {
        PrecisionStorage::Rgba16Unorm => "rgba16_unorm",
        PrecisionStorage::Rgba32Float => crate::float_document::STORAGE_FLOAT,
    }
}

/// A little-endian cursor over a borrowed stream; every read is bounded.
pub(crate) struct Reader<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) at: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(n)
            .filter(|&end| end <= self.bytes.len())
            .ok_or("clipboard payload is truncated")?;
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }
    pub(crate) fn word(&mut self) -> Result<[u8; 4], String> {
        let mut out = [0; 4];
        out.copy_from_slice(self.take(4)?);
        Ok(out)
    }
    pub(crate) fn u32(&mut self) -> Result<u32, String> {
        self.word().map(u32::from_le_bytes)
    }
    pub(crate) fn i32(&mut self) -> Result<i32, String> {
        self.word().map(i32::from_le_bytes)
    }
    pub(crate) fn f32(&mut self) -> Result<f32, String> {
        self.word().map(f32::from_le_bytes)
    }
}

/// The editable section's shape (module doc), shared by `decode` and
/// `with_editable` so the two can never admit different sections — and by
/// the CMYK payload's v4 section (`cmyk_clipboard_codec`).
pub(crate) fn validate_section(bytes: &[u8]) -> Result<&str, String> {
    if bytes.len() as u64 > u64::from(MAX_DRDC_BLOB_LEN) {
        return Err("clipboard editable section exceeds the size cap".into());
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "clipboard editable section is not UTF-8".to_string())?;
    let value: Value = serde_json::from_str(text)
        .map_err(|e| format!("clipboard editable section is not JSON: {e}"))?;
    let object = value
        .as_object()
        .ok_or("clipboard editable section is not a JSON object")?;
    for (key, value) in object {
        match key.as_str() {
            "type" | "name" if !value.is_string() => {
                return Err(format!(
                    "clipboard editable section `{key}` must be a string"
                ))
            }
            "record" if value.as_str().is_none_or(str::is_empty) => {
                return Err("clipboard editable section `record` must be a non-empty string".into())
            }
            "anchor" => {
                let pair = value.as_array().filter(|a| a.len() == 2);
                let finite = pair
                    .is_some_and(|a| a.iter().all(|v| v.as_f64().is_some_and(|n| n.is_finite())));
                if !finite {
                    return Err(
                        "clipboard editable section `anchor` must be two finite numbers".into(),
                    );
                }
            }
            "type" | "name" | "record" => {}
            other => {
                return Err(format!(
                    "clipboard editable section key `{other}` is not recognized"
                ))
            }
        }
    }
    if !object.contains_key("type") {
        return Err("clipboard editable section has no `type`".into());
    }
    if !object.contains_key("record") {
        return Err("clipboard editable section has no `record`".into());
    }
    Ok(text)
}

pub(crate) fn sane_resolution(resolution: Resolution) -> Result<(), String> {
    if [resolution.x, resolution.y]
        .iter()
        .any(|v| !v.is_finite() || *v <= 0.0)
    {
        return Err("clipboard payload resolution must be finite and positive".into());
    }
    Ok(())
}

impl ClipboardPayload {
    pub fn image(&self) -> &PrecisionImage {
        &self.image
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
    pub fn storage(&self) -> PrecisionStorage {
        self.image.storage()
    }
    pub fn dimensions(&self) -> (u32, u32) {
        self.image.dimensions()
    }
    /// True when the u16 samples are exactly `v · 257` of 8-bit pixels.
    pub fn byte_origin(&self) -> bool {
        self.byte_origin
    }
    /// The carried tone map; `Some` iff the storage is f32.
    pub fn tone_map(&self) -> Option<ToneMap> {
        self.tone_map
    }

    /// The host's editable section (v3), if the payload carries one.
    pub fn editable(&self) -> Option<&str> {
        self.editable.as_deref()
    }

    /// The admission unit: the samples, the optional coverage plane and the
    /// optional editable section.
    pub fn payload_bytes(&self) -> u64 {
        let (w, h) = self.dimensions();
        payload_bytes(w, h, self.storage(), self.coverage.is_some()) + self.section_bytes()
    }

    fn section_bytes(&self) -> u64 {
        self.editable.as_ref().map_or(0, |s| s.len() as u64)
    }

    /// This payload with `section` as its editable section, validated
    /// exactly as the decoder validates one; an existing section is
    /// replaced. Admits, BEFORE parsing or cloning, what the FFI handle will
    /// hold (`decode`'s rule): retained = `payload_bytes()` + the profile
    /// bytes + `2 · len` (the handle re-encodes the `Arc`-shared samples,
    /// and the section lives in the payload and again in the encoding),
    /// scratch = bookkeeping + `2 · len` (the JSON parse).
    pub fn with_editable(&self, section: &str, limit: u64, other: u64) -> Result<Self, String> {
        let len = section.len() as u64;
        let retained = self
            .payload_bytes()
            .checked_add(self.image.profile().bytes().len() as u64)
            .and_then(|n| n.checked_add(len.checked_mul(2)?))
            .ok_or("clipboard payload size overflow")?;
        admit(retained, BOOKKEEPING_BYTES + 2 * len, other, limit)?;
        let text = validate_section(section.as_bytes())?;
        let mut next = self.clone();
        next.editable = Some(Arc::from(text));
        Ok(next)
    }

    /// Straight RGBA8 → u16 `v · 257` with the byte-origin flag and no
    /// coverage (the byte Copy already multiplied alpha). The profile is
    /// parsed and length-checked exactly as `dr_precision_doc_from_rgba16`
    /// does, and the input, the payload and its encoding are admitted before
    /// anything is allocated.
    #[allow(clippy::too_many_arguments)]
    pub fn from_rgba8(
        pixels: &[u8],
        width: u32,
        height: u32,
        profile: &[u8],
        offset: (i32, i32),
        resolution: Resolution,
        limit: u64,
        other: u64,
    ) -> Result<Self, String> {
        let count = pixel_count(width, height)?;
        if pixels.len() != count.checked_mul(4).ok_or("RGBA8 sample count overflow")? {
            return Err("clipboard RGBA8 pixels must match width times height times four".into());
        }
        if profile.is_empty() || profile.len() > MAX_DRDC_BLOB_LEN as usize {
            return Err("clipboard RGBA8 input requires a bounded original ICC profile".into());
        }
        sane_resolution(resolution)?;
        let input_bytes = pixels.len() as u64 + profile.len() as u64;
        let payload = payload_bytes(width, height, PrecisionStorage::Rgba16Unorm, false);
        // ICC parsers can materialize sampled curves; 64 times the encoded
        // bytes bounds their parsed state (`ffi_precision_input`'s rule).
        let scratch = 4 * BOOKKEEPING_BYTES + 2 * payload + profile.len() as u64 * 64;
        admit(input_bytes, scratch, other, limit)?;
        let parsed = Arc::new(
            IccProfile::parse(profile)
                .ok_or("clipboard RGBA8 profile is not a supported RGB ICC profile")?,
        );
        if parsed.bytes().len() != profile.len() || parsed.model().is_none() {
            return Err(
                "clipboard RGBA8 profile must be exact complete RGB matrix/TRC ICC bytes".into(),
            );
        }
        let mut samples = reserved_vec(count)?;
        for pixel in pixels.chunks_exact(4) {
            samples.push(std::array::from_fn(|c| u16::from(pixel[c]) * 257));
        }
        let image = PrecisionImage::from_rgba16_decoded(width, height, samples, parsed, None)?;
        Ok(Self {
            image,
            offset,
            resolution,
            coverage: None,
            byte_origin: true,
            tone_map: None,
            editable: None,
        })
    }

    /// The payload's 8-bit pixels in its own profile:
    /// [`Self::to_rgba8_in`] with no profile, under Convert.
    pub fn to_rgba8(
        &self,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<RgbaImage, String> {
        self.to_rgba8_in(None, PasteProfile::Convert, limit, other, control)
    }

    /// The payload's 8-bit pixels in `profile` (`None` = its own) under
    /// `mode`: `converted_with(Rgba16Unorm, profile, mode)` — an f32
    /// payload maps under its own tone map, another profile converts under
    /// Convert and is relabelled under Preserve; a payload already there is
    /// read as is — then `PrecisionImage::to_rgba8`'s rule over the u16
    /// samples, with the intermediate's `payload_bytes` counted as other
    /// retained bytes.
    pub fn to_rgba8_in(
        &self,
        profile: Option<&Arc<IccProfile>>,
        mode: PasteProfile,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<RgbaImage, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let target = profile.unwrap_or(self.image.profile());
        let converted = self.converted_with_context(
            PrecisionStorage::Rgba16Unorm,
            target,
            mode,
            limit,
            other,
            job.range(0.0, 0.5),
        )?;
        let (image, other) = match &converted {
            Some(next) => (
                &next.image,
                other
                    .checked_add(next.payload_bytes())
                    .ok_or("clipboard payload size overflow")?,
            ),
            None => (&self.image, other),
        };
        let (w, h) = image.dimensions();
        let scratch = rgba8_preview_scratch(w, h)?;
        admit(image.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        image.quantize_with_context(PrecisionDither::None, job.range(0.5, 1.0))
    }

    /// The RZCB v3 stream (module doc).
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        let (w, h) = self.dimensions();
        let profile = self.image.profile().bytes();
        let total = (FIXED_HEADER as u64 + profile.len() as u64 + 1 + TONE_WORDS as u64 + 4)
            .checked_add(self.payload_bytes())
            .and_then(|n| usize::try_from(n).ok())
            .ok_or("clipboard payload size overflow")?;
        let mut out: Vec<u8> = reserved_vec(total)?;
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        let storage = match self.storage() {
            PrecisionStorage::Rgba16Unorm => STORAGE_U16,
            PrecisionStorage::Rgba32Float => STORAGE_F32,
        };
        out.extend_from_slice(&storage.to_le_bytes());
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
        if self.byte_origin {
            flags |= FLAG_BYTE_ORIGIN;
        }
        let section = self.editable.as_deref().unwrap_or("");
        if !section.is_empty() {
            flags |= FLAG_EDITABLE;
        }
        out.push(flags);
        let map = self.tone_map.unwrap_or(ToneMap::Clip);
        out.extend_from_slice(&map.code().to_le_bytes());
        out.extend_from_slice(&map.white().to_le_bytes());
        let section_len =
            u32::try_from(section.len()).map_err(|_| "clipboard editable section is too large")?;
        out.extend_from_slice(&section_len.to_le_bytes());
        if let Some(runs) = self.image.runs_rgba16() {
            for run in runs {
                for v in run.as_flattened() {
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
        } else if let Some(runs) = self.image.runs_rgba32() {
            for run in runs {
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
        out.extend_from_slice(section.as_bytes());
        if out.len() != total {
            return Err("clipboard payload encoding length mismatch".into());
        }
        Ok(out)
    }

    /// The RZCB reader (versions 1, 2 and 3). Refuses every malformed stream
    /// (module doc) and admits the decoded payload before allocating its
    /// samples.
    pub fn decode(bytes: &[u8], limit: u64, other: u64) -> Result<Self, String> {
        admit(0, BOOKKEEPING_BYTES, other, limit)?;
        let mut reader = Reader { bytes, at: 0 };
        if reader.take(4).ok() != Some(&MAGIC[..]) {
            return Err("clipboard payload is not an RZCB stream".into());
        }
        let version = reader.u32()?;
        if ![VERSION_1, VERSION_2, VERSION].contains(&version) {
            return Err(format!(
                "clipboard payload version {version} is not supported (this build reads versions {VERSION_1} to {VERSION})"
            ));
        }
        let storage = match reader.u32()? {
            STORAGE_U16 => PrecisionStorage::Rgba16Unorm,
            STORAGE_F32 => PrecisionStorage::Rgba32Float,
            other => return Err(format!("clipboard payload storage code {other} is unknown")),
        };
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
        let (has_coverage, byte_origin, tone_map, section_len) = if version == VERSION_1 {
            let has_coverage = match flags {
                0 => false,
                1 => true,
                _ => return Err("clipboard payload coverage flag is invalid".into()),
            };
            let map = (storage == PrecisionStorage::Rgba32Float).then_some(ToneMap::Clip);
            (has_coverage, false, map, 0)
        } else {
            let allowed = if version == VERSION {
                FLAG_COVERAGE | FLAG_BYTE_ORIGIN | FLAG_EDITABLE
            } else {
                FLAG_COVERAGE | FLAG_BYTE_ORIGIN
            };
            if flags > allowed {
                return Err(format!("clipboard payload flags {flags} are invalid"));
            }
            let byte_origin = flags & FLAG_BYTE_ORIGIN != 0;
            if byte_origin && storage == PrecisionStorage::Rgba32Float {
                return Err(
                    "clipboard payload byte-origin flag is invalid on 32-bit float storage".into(),
                );
            }
            let operator = reader.u32()?;
            let white = reader.f32()?;
            let map = match storage {
                PrecisionStorage::Rgba16Unorm if operator != 0 => {
                    return Err(format!(
                    "clipboard payload tone-map operator {operator} is invalid on 16-bit storage"
                ))
                }
                PrecisionStorage::Rgba16Unorm => None,
                PrecisionStorage::Rgba32Float => Some(
                    ToneMap::from_code(operator, white)
                        .map_err(|e| format!("clipboard payload tone map is refused: {e}"))?,
                ),
            };
            let section_len = if version == VERSION {
                u64::from(reader.u32()?)
            } else {
                0
            };
            if (flags & FLAG_EDITABLE != 0) != (section_len > 0) {
                return Err(format!(
                    "clipboard payload editable flag disagrees with its section length {section_len}"
                ));
            }
            if section_len > u64::from(MAX_DRDC_BLOB_LEN) {
                return Err("clipboard editable section exceeds the size cap".into());
            }
            (flags & FLAG_COVERAGE != 0, byte_origin, map, section_len)
        };
        let samples = payload_bytes(width, height, storage, has_coverage);
        let expected = (reader.at as u64)
            .checked_add(samples)
            .and_then(|n| n.checked_add(section_len))
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
        // The decoded samples and section plus the handle's own re-encoding
        // (Copy's `2·payload_bytes`); the caller's input bytes arrive in
        // `other`.
        admit(2 * (samples + section_len), scratch, other, limit)?;
        let section_at = bytes.len() - section_len as usize;
        let editable = if section_len > 0 {
            Some(Arc::from(validate_section(&bytes[section_at..])?))
        } else {
            None
        };
        let profile = IccProfile::parse(profile_bytes)
            .filter(|p| p.bytes().len() == profile_len)
            .ok_or("clipboard payload profile is not a supported RGB ICC profile")?;
        let profile = Arc::new(profile);
        let image = match storage {
            PrecisionStorage::Rgba16Unorm => {
                let raw = reader.take(count * 8)?;
                let mut pixels = reserved_vec(count)?;
                for chunk in raw.chunks_exact(8) {
                    let v = |i: usize| u16::from_le_bytes([chunk[2 * i], chunk[2 * i + 1]]);
                    let pixel = [v(0), v(1), v(2), v(3)];
                    if byte_origin && pixel.iter().any(|s| s % 257 != 0) {
                        return Err(
                            "clipboard payload is flagged byte-origin but a sample is not a multiple of 257"
                                .into(),
                        );
                    }
                    pixels.push(pixel);
                }
                PrecisionImage::from_rgba16_decoded(width, height, pixels, profile, None)?
            }
            PrecisionStorage::Rgba32Float => {
                let raw = reader.take(count * 16)?;
                let mut pixels = reserved_vec(count)?;
                for chunk in raw.chunks_exact(16) {
                    let v = |i: usize| {
                        f32::from_le_bytes([
                            chunk[4 * i],
                            chunk[4 * i + 1],
                            chunk[4 * i + 2],
                            chunk[4 * i + 3],
                        ])
                    };
                    let pixel = [v(0), v(1), v(2), v(3)];
                    if !extended_pixel(pixel) {
                        return Err(
                            "clipboard payload float sample is non-finite or outside the stored range"
                                .into(),
                        );
                    }
                    pixels.push(pixel);
                }
                PrecisionImage::from_rgba32_extended_decoded(width, height, pixels, profile, None)?
            }
        };
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
            offset,
            resolution,
            coverage,
            byte_origin,
            tone_map,
            editable,
        })
    }

    /// The host-facing description (never the samples).
    pub fn info(&self) -> Value {
        let (w, h) = self.dimensions();
        let depth = match self.storage() {
            PrecisionStorage::Rgba16Unorm => 16,
            PrecisionStorage::Rgba32Float => 32,
        };
        let mut info = json!({
            "version": VERSION,
            "mode": "rgb",
            "storage": storage_name(self.storage()),
            "depth": depth,
            "origin_depth": if self.byte_origin { 8 } else { depth },
            "width": w,
            "height": h,
            "offset": [self.offset.0, self.offset.1],
            "ppi": [self.resolution.x, self.resolution.y],
            "profile_description": self.image.profile().name(),
            "has_coverage": self.coverage.is_some(),
        });
        if let Some(map) = self.tone_map {
            info["tone_map"] = json!(map);
        }
        info["editable"] = section_info(self.editable.as_deref());
        info
    }
}

/// `info()["editable"]`: null, or the section's type, name, anchor and the
/// record's byte length (never the record itself). Shared with the CMYK
/// payload's v4 section (`cmyk_clipboard_codec`).
pub(crate) fn section_info(section: Option<&str>) -> Value {
    let Some(section) = section else {
        return Value::Null;
    };
    let Ok(Value::Object(object)) = serde_json::from_str::<Value>(section) else {
        return Value::Null;
    };
    let mut out = serde_json::Map::new();
    out.insert("type".into(), object["type"].clone());
    for key in ["name", "anchor"] {
        if let Some(value) = object.get(key) {
            out.insert(key.into(), value.clone());
        }
    }
    let record = object.get("record").and_then(Value::as_str).unwrap_or("");
    out.insert("record_bytes".into(), json!(record.len()));
    Value::Object(out)
}

pub(crate) fn payload_bytes(
    width: u32,
    height: u32,
    storage: PrecisionStorage,
    coverage: bool,
) -> u64 {
    let n = u64::from(width) * u64::from(height);
    n * storage.bytes_per_pixel() + if coverage { n * 2 } else { 0 }
}
