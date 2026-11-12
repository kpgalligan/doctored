//! The export workspace: one pure render → resample → sharpen → convert →
//! reduce → encode chain that produces the ENCODED BYTES of a deliverable
//! without touching the editable master.
//!
//! The point of returning bytes rather than writing a file is the preview
//! contract: the host decodes exactly what it is about to write
//! ([`ExportResult::decode_preview`]) and then writes exactly those bytes
//! ([`ExportResult::write`]), so "the preview matches the encoded output" is
//! an identity rather than a promise.
//!
//! # No new pixel math
//!
//! Every stage routes through the shipped kernel: `ops::resize` /
//! `precision_sampling::resize_image` for the resample,
//! `ops_photographic::unsharp_mask_samples` (the typed chain's entry is its
//! `unsharp_mask_runs`) for the optional output sharpening,
//! `retained_source_render::color8` / `color16` for the profile
//! conversion, `PrecisionImage::quantize_with_context` for the depth
//! reduction, and `dr_image::encode_into` / `precision_codec::encode` behind
//! the same `MetadataInjector` selection `save_flat` and `precision_codec::save`
//! use. With the request left at its defaults the bytes are therefore the
//! bytes `dr_doc_save_image` and `dr_precision_doc_save_projection16` already
//! wrote.
//!
//! # Stage order
//!
//! Resample first, sharpen second: output sharpening is a deliberate
//! correction for the resampling loss, so sharpening before the resample
//! would throw most of the added detail away again. The profile conversion
//! follows, because it is a per-pixel colour map and commutes with neither
//! resampling nor sharpening in a way worth relying on, and the depth
//! reduction is last so every earlier stage keeps the document's own
//! precision.
//!
//! It writes NO pixel math of its own — `ops::resize` / `precision_sampling::resize_image`,
//! `ops_photographic::unsharp_mask_samples` (the typed chain through
//! `ops_photographic::unsharp_mask_runs`, since 13B-3e-1),
//! `retained_source_render::color8`/`color16`, `PrecisionImage::quantize_with_context` and
//! `dr_image::encode_into` / `precision_codec::encode` behind the SAME `MetadataInjector` selection
//! `save_flat` and `precision_codec::save` use (PNG16 without `MetadataInjector::png(…,
//! side.png_chunks())` would silently lose its `pHYs`/XMP). One request JSON serves the FFI, the
//! host's presets and MCP; an unknown key refuses. With every optional field omitted the bytes
//! equal `dr_doc_save_image` and `dr_precision_doc_save_projection16` (tested). `output_dimensions`
//! keeps the aspect (round then max(1), side ≤ the format's own cap from `max_output_side` — 65535
//! JPEG/GIF, 16384 WebP, 130048 HEIC (measured), none for PNG/TIFF/BMP — scale in (0,16], pixel count ≤
//! `MAX_PIXELS`); `expand_name_pattern` requires `{name}`, or `{layer}`/`{index}` when the tokens
//! say `layer_export`, and refuses a separator, a drive colon, a control character or a
//! leading dot so a pattern can never escape its destination folder. `write` refuses an existing
//! path unless `overwrite`. Phase 11D adds HOST-ENCODED formats: `Format::Heic` has no encoder in
//! this crate (every flat writer refuses it with one sentence), the export workspace substitutes
//! `Format::Tiff` for the sidecar, the injector selection AND the encoder under `request.format ==
//! Heic` (so the pending bytes are byte-identical to the `"tiff"` request's), and the result is
//! PENDING — `write` and `decode_preview` refuse it by name so an intermediate can never reach disk
//! or a preview, while `bytes()` stays readable because it is what the host transcodes.
//! `adopt_host_bytes` takes the host's bytes, the depth the written file DECODES at and the
//! `DR_CARRIES_*` bits an inspection MEASURED, and rebuilds the packet report from them rather than
//! from intent. `format_carries(Heic)` governs only `validate()`'s policy refusal and the adopted
//! result. `Format::Avif` follows HEIC at every one of those sites (side cap 16384). `Format::Jxl`
//! is the opposite case: lossless JPEG XL is encoded HERE (`jxl_encode`, 8-bit only, sRGB pixels
//! only, a side of at least 2), so its result is never pending and `write` takes it as it is; but
//! the crate decodes none, so `decode_preview` refuses all three by name and `info_json`'s
//! `host_preview` tells the host to decode the result's own bytes, while `host_encoded` keeps
//! meaning "pending an intermediate" (HEIC and AVIF only). The sharpen stage is `export_sharpen`:
//! Unsharp Mask, unchanged, or Smart Sharpen (`"method": "smart_sharpen"`, the filter's
//! keys; `ops_spatial::spatial_bytes` at 8 bits, its plane and combine on the u16 codes at 16). The
//! typed `export_plan` charges `OutputSharpen::plan_bytes`: for Unsharp Mask `neighborhood_plan(…)?.0
//! + n×16 + n×8` — the Gaussian's two premultiplied planes, the straight f32 input `apply` builds
//! and the u16 output — for Smart Sharpen its kernel plan + n×8, and 0 on a bypass. `export_plan` is `export_plan_for(self.dimensions(), …)`, the ONE
//! plan formula over a source of any size. A 32-bit float export writes the samples exactly at size
//! none and, with a size mode, first
//! resizes the rendered composite through `precision_sampling::resize_image` (the extended-range
//! finish, RGB clamped to the composite's own visible range); its plan term adds the f32 output,
//! the float source copy and the larger of the separable temporaries and the output's adoption
//! copy. Sharpen, profile and dither still refuse at 32. The 32-bit stage
//! is `doc_export_float`: TIFF, OpenEXR (`"exr"`) or Radiance RGBE (`"hdr"`), f32 or half
//! (`float_samples`), under the same plan term; `exr` and `hdr` exist only at 32 bits.
//!
//! A 32-bit `heic` or `avif` request with `hdr_encoding` (`"gain_map"` or
//! `"pq"`) is a PENDING host-encoded HDR result: `doc_export_float` writes its intermediate (the
//! `"tiff"` 32-bit bytes for a gain map, a 16-bit PQ TIFF for PQ), and `info_json` adds
//! `hdr_encoding` and `content_headroom`; without `hdr_encoding` a 32-bit request still
//! requires tiff, exr or hdr.

use std::io::Cursor;
use std::sync::Arc;

use image::imageops::FilterType;
use image::RgbaImage;
use serde_json::{json, Value};

use crate::doc::{DrDocument, MAX_PIXELS};
use crate::doc_export_float::{FloatDeliverable, HdrDeliverable, HdrExportEncoding};
use crate::doc_export_layers::LayerExportInfo;
use crate::dr_image::Format;
use crate::export_metadata::{MetadataOutcome, MetadataPolicy};
use crate::export_sharpen::OutputSharpen;
use crate::float_io::FloatContainer;
use crate::icc::IccProfile;
use crate::icc_intent::{ColorPipeline, RenderingIntent};
use crate::job_control::{DrJobControl, JobContext};
use crate::metadata_write::MetadataInjector;
use crate::precision_codec::{self, PrecisionFormat, MAX_ENCODED_BYTES};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, pixel_count, PrecisionImage, BOOKKEEPING_BYTES};
use crate::precision_quantize::PrecisionDither;
use crate::precision_sampling::{self, GeometryFilter};
use crate::resources::Inventory;
use crate::tiff_float::FloatSampleType;
use crate::DrImage;

/// The largest side a deliverable in `format` may have, or `None` when only
/// the document ceiling [`MAX_PIXELS`] applies. The caps are the FORMAT's own
/// limits, not a house rule: JPEG and GIF store a side in 16 bits (65535),
/// WebP's bitstream stores it in 14 (16384), and HEIC's is the ImageIO
/// encoder's own, measured below (130048). PNG, TIFF and BMP store 32-bit
/// sides, so a long panorama strip is bounded by the pixel count alone.
///
/// HEIC measurement, macOS 26.5.1, a scratch Swift program encoding exactly as
/// `HostImageEncoder.heic(fromTIFF:)` does: an RGBA8 or RGBA16 sRGB strip
/// (R = u & 255, G = 4v & 255, B = (u >> 8) & 255 along the long side u, A
/// opaque) written to a TIFF intermediate through ImageIO, read back, then
/// a `CGImageDestination` HEIC at quality 1.0. A side S passes when Finalize
/// succeeds with non-empty output, the reopen reports S × h, the full decode's
/// provider has `bytesPerRow · height` bytes, and every 512-wide block along
/// the long side (the first and last 64 columns included) is within a mean of
/// 8 codes (8-bit scale) of the pattern. The end-column check alone is not
/// enough: past 131072 the encoder writes a file whose two ends decode right
/// and whose middle is wrong (worst block mean 84.8). Binary search over
/// [30000, 195312] (195312 · 512 is `MAX_PIXELS`), both depths, both
/// orientations: h = 2 passes to 130560 (255 · 512) and fails at 130561
/// (Finalize fails at 195312; 130561 decodes wrong, mean 116.8); h = 512
/// passes to 130048 (254 · 512) and fails at 130049 (the encoder writes an
/// empty grid, mean 96). 130048 also passes at h = 1, 3, 64, 256, 300, 511,
/// 513 and 765 at both depths, and 130560 fails at h = 256 and more, so the
/// cap is 130048: the minimum passing side over every variant, with 130049
/// confirmed failing. The encoder fails silently (Finalize succeeds), so the
/// cap is the only guard.
///
/// AVIF takes 16384, the widest single coded image the AVIF Advanced Profile
/// allows (a larger picture has to be a grid of tiles, which not every reader
/// assembles). The ImageIO AVIF encoder has not been measured the way HEIC's
/// was, so this is a conservative cap and not that encoder's own limit. JPEG
/// XL stores 30-bit sides, so it has no cap of its own; OpenEXR stores 32-bit
/// window corners and Radiance writes its sides as decimal text, so neither
/// has one.
fn max_output_side(format: Format) -> Option<u32> {
    match format {
        Format::Jpeg | Format::Gif => Some(65_535),
        Format::Webp | Format::Avif => Some(16_384),
        Format::Heic => Some(130_048),
        Format::Png | Format::Tiff | Format::Bmp | Format::Jxl | Format::Exr | Format::Hdr => None,
    }
}

/// The largest scale factor a request may ask for. Upsampling past 16x is
/// not resampling any more, and at the pixel ceiling it is already refused
/// by [`MAX_PIXELS`]; naming it separately makes the refusal legible.
const MAX_OUTPUT_SCALE: f64 = 16.0;

/// The longest expanded file name, in characters. Comfortably inside every
/// filesystem's own limit, and short enough that a pattern that ran away —
/// a document named from a whole sentence — is refused rather than written.
const MAX_NAME_CHARS: usize = 200;

// ------------------------------------------------------------ the request --

/// How the output dimensions are derived from the source. Aspect ratio is
/// always kept: a deliverable that silently changed the shape of a
/// photograph would be a bug, and a crop is a document edit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OutputSize {
    /// No resampling at all — the source dimensions.
    None,
    Width(u32),
    Height(u32),
    /// Whichever of width and height is larger in the SOURCE.
    LongEdge(u32),
    /// A multiplier in (0, 16].
    Scale(f64),
}

/// The destination colour space: the document's own, one of the two
/// built-ins, or (Phase 11C) any RGB profile on disk the core can ENCODE
/// into — a file chosen in the export sheet, or one the host resolved out of
/// the ColorSync folders before the request reached here.
#[derive(Clone, Debug)]
pub enum OutputProfile {
    /// Keep the document's own profile — no conversion, no-op.
    Document,
    Srgb,
    DisplayP3,
    /// A profile read from `path`. The bytes are read at PARSE time, so an
    /// unreadable, non-RGB, unencodable or oversized file refuses the
    /// request rather than half of an export.
    File {
        path: String,
        profile: Arc<IccProfile>,
    },
}

/// The largest destination profile an export request may name. The same
/// 16 MiB a document may carry (`MAX_DRDC_BLOB_LEN`), so a profile that can
/// be exported INTO can also be assigned.
const MAX_EXPORT_PROFILE_BYTES: u64 = crate::drdc::MAX_DRDC_BLOB_LEN as u64;

impl OutputProfile {
    fn name(&self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Srgb => "srgb",
            Self::DisplayP3 => "display-p3",
            Self::File { .. } => "file",
        }
    }

    /// The destination profile, or `None` for the document's own.
    fn destination(&self) -> Option<Arc<IccProfile>> {
        match self {
            Self::Document => None,
            Self::Srgb => Some(IccProfile::srgb()),
            Self::DisplayP3 => Some(IccProfile::display_p3()),
            Self::File { profile, .. } => Some(profile.clone()),
        }
    }

    /// Reads and validates a destination profile file.
    fn from_path(path: &str) -> Result<OutputProfile, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("failed to read {path}: {e}"))?;
        if bytes.len() as u64 > MAX_EXPORT_PROFILE_BYTES {
            return Err(format!(
                "the profile {path} is larger than the {} MiB limit",
                MAX_EXPORT_PROFILE_BYTES >> 20
            ));
        }
        let profile = IccProfile::parse(&bytes)
            .ok_or_else(|| format!("{path} is not an RGB ICC profile a document can carry"))?;
        if !profile.can_encode() {
            return Err(format!(
                "the profile {path} carries no model this build can convert INTO"
            ));
        }
        Ok(OutputProfile::File {
            path: path.to_string(),
            profile: Arc::new(profile),
        })
    }
}

/// One export request, parsed from the single JSON schema the FFI, the
/// preset library and the MCP tools all speak.
pub struct ExportRequest {
    pub(crate) format: Format,
    pub(crate) jpeg_quality: u8,
    pub(crate) bit_depth: u32,
    pub(crate) dither: PrecisionDither,
    pub(crate) size: OutputSize,
    pub(crate) filter: FilterType,
    pub(crate) profile: OutputProfile,
    /// The rendering intent the convert stage runs under (Phase 11C);
    /// relative colorimetric by default, which is 11A's behaviour.
    pub(crate) intent: RenderingIntent,
    /// Black-point compensation for that stage; off by default.
    pub(crate) black_point_compensation: bool,
    pub(crate) sharpen: Option<OutputSharpen>,
    pub(crate) embed_profile: bool,
    pub(crate) strip_metadata: bool,
    /// The per-packet policy (Phase 11B). Omitted from the request means
    /// [`MetadataPolicy::DEFAULT`], which is `strip_metadata` alone — so the
    /// bytes are 11A's bytes.
    pub(crate) metadata: MetadataPolicy,
    /// The sample width of a 32-bit float deliverable: f32 by
    /// default, half for a TIFF or OpenEXR that asks for it.
    pub(crate) float_samples: FloatSampleType,
    /// The HDR encoding of a 32-bit HEIC or AVIF; `None` for
    /// every other request.
    pub(crate) hdr_encoding: Option<HdrExportEncoding>,
    /// `"spot_channels": "keep"` (#1 1c-2a, `tiff_spot`): a full-size
    /// TIFF keeps the spot channels as extra samples instead of compositing
    /// their overprint (`"overprint"`, the default).
    pub(crate) keep_spots: bool,
}

/// Every key the request object recognizes. An unknown key is refused rather
/// than ignored: a misspelled `jpeg_qualtiy` that quietly exported at 90
/// would be discovered only in the delivered file.
const REQUEST_KEYS: [&str; 17] = [
    "format",
    "jpeg_quality",
    "bit_depth",
    "float_samples",
    "hdr_encoding",
    "dither",
    "size",
    "filter",
    "profile",
    "profile_path",
    "intent",
    "black_point_compensation",
    "sharpen",
    "embed_profile",
    "strip_metadata",
    "metadata",
    "spot_channels",
];

pub(crate) fn object_of<'a>(
    value: &'a Value,
    what: &str,
) -> Result<&'a serde_json::Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("export {what} must be a JSON object"))
}

pub(crate) fn known_keys(
    object: &serde_json::Map<String, Value>,
    allowed: &[&str],
    what: &str,
) -> Result<(), String> {
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("export {what} has an unknown key \"{key}\""));
        }
    }
    Ok(())
}

fn string_of(object: &serde_json::Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(format!("export request {key} must be a string")),
    }
}

fn bool_of(
    object: &serde_json::Map<String, Value>,
    key: &str,
    default: bool,
) -> Result<bool, String> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(format!("export request {key} must be a boolean")),
    }
}

pub(crate) fn number_of(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<f64>, String> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_f64()
            .filter(|v| v.is_finite())
            .map(Some)
            .ok_or_else(|| format!("export request {key} must be a finite number")),
    }
}

fn integer_of(object: &serde_json::Map<String, Value>, key: &str) -> Result<Option<u64>, String> {
    match number_of(object, key)? {
        None => Ok(None),
        Some(value) if value >= 0.0 && value.fract() == 0.0 && value <= u64::MAX as f64 => {
            Ok(Some(value as u64))
        }
        Some(_) => Err(format!(
            "export request {key} must be a non-negative integer"
        )),
    }
}

impl ExportRequest {
    /// The ONE parser. Every field is optional but `format`; the defaults
    /// are today's export behaviour, which is what makes "default settings
    /// reproduce today's bytes" checkable.
    pub fn parse(json: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(json)
            .map_err(|e| format!("export request is not valid JSON: {e}"))?;
        let object = object_of(&value, "request")?;
        known_keys(object, &REQUEST_KEYS, "request")?;

        let format = match string_of(object, "format")?.as_deref() {
            Some("png") => Format::Png,
            Some("jpeg") => Format::Jpeg,
            Some("tiff") => Format::Tiff,
            Some("bmp") => Format::Bmp,
            Some("gif") => Format::Gif,
            Some("webp") => Format::Webp,
            Some("heic") => Format::Heic,
            Some("avif") => Format::Avif,
            Some("jxl") => Format::Jxl,
            Some("exr") => Format::Exr,
            Some("hdr") => Format::Hdr,
            Some(other) => {
                return Err(format!(
                "export format \"{other}\" is not one of png, jpeg, tiff, bmp, gif, webp, heic, avif, jxl, exr or hdr"
            ))
            }
            None => return Err("export request requires a format".into()),
        };

        let jpeg_quality = match integer_of(object, "jpeg_quality")? {
            None => 90,
            Some(value) if (1..=100).contains(&value) => value as u8,
            Some(_) => return Err("export jpeg_quality must be an integer in [1,100]".into()),
        };

        let bit_depth = match integer_of(object, "bit_depth")? {
            None => 8,
            Some(8) => 8,
            Some(16) => 16,
            // 32 is the 32-bit FLOAT document's exact deliverable (Phase
            // 11F); every other document refuses it when the request meets
            // its own pixels.
            Some(32) => 32,
            Some(_) => return Err("export bit_depth must be 8, 16 or 32".into()),
        };

        let float_samples = match string_of(object, "float_samples")?.as_deref() {
            None | Some("f32") => FloatSampleType::F32,
            Some("f16") => FloatSampleType::F16,
            Some(other) => {
                return Err(format!(
                    "export float_samples \"{other}\" is not one of f32 or f16"
                ))
            }
        };

        let hdr_encoding = match string_of(object, "hdr_encoding")?.as_deref() {
            None => None,
            Some("gain_map") => Some(HdrExportEncoding::GainMap),
            Some("pq") => Some(HdrExportEncoding::Pq),
            Some(other) => {
                return Err(format!(
                    "export hdr_encoding \"{other}\" is not one of gain_map or pq"
                ))
            }
        };

        let dither = match string_of(object, "dither")?.as_deref() {
            None | Some("none") => PrecisionDither::None,
            Some("ordered") => PrecisionDither::Ordered,
            Some(other) => {
                return Err(format!(
                    "export dither \"{other}\" is not one of none or ordered"
                ))
            }
        };

        let size = match object.get("size") {
            None | Some(Value::Null) => OutputSize::None,
            Some(value) => parse_size(value)?,
        };

        let filter = match string_of(object, "filter")?.as_deref() {
            None | Some("lanczos3") => FilterType::Lanczos3,
            Some("nearest") => FilterType::Nearest,
            Some("bilinear") => FilterType::Triangle,
            Some("catmull-rom") => FilterType::CatmullRom,
            Some(other) => {
                return Err(format!(
                    "export filter \"{other}\" is not one of nearest, bilinear, catmull-rom or lanczos3"
                ))
            }
        };

        let profile_path = string_of(object, "profile_path")?;
        let profile = match string_of(object, "profile")?.as_deref() {
            None | Some("document") => OutputProfile::Document,
            Some("srgb") => OutputProfile::Srgb,
            Some("display-p3") => OutputProfile::DisplayP3,
            Some("file") => {
                let path = profile_path
                    .as_deref()
                    .ok_or("export profile \"file\" requires a profile_path")?;
                OutputProfile::from_path(path)?
            }
            Some(other) => {
                return Err(format!(
                    "export profile \"{other}\" is not one of document, srgb, display-p3 or file"
                ))
            }
        };

        let intent = match string_of(object, "intent")?.as_deref() {
            None => RenderingIntent::RelativeColorimetric,
            Some(text) => RenderingIntent::parse(text).ok_or_else(|| {
                format!(
                    "export intent \"{text}\" is not one of perceptual, relative, saturation or absolute"
                )
            })?,
        };

        let sharpen = match object.get("sharpen") {
            None | Some(Value::Null) => None,
            Some(value) => Some(OutputSharpen::parse(value)?),
        };

        let metadata = match object.get("metadata") {
            None | Some(Value::Null) => MetadataPolicy::DEFAULT,
            Some(value) => MetadataPolicy::parse(value)?,
        };

        let keep_spots = match string_of(object, "spot_channels")?.as_deref() {
            None | Some("overprint") => false,
            Some("keep") => true,
            Some(other) => {
                return Err(format!(
                    "export spot_channels \"{other}\" is not one of overprint or keep"
                ))
            }
        };

        let request = ExportRequest {
            format,
            jpeg_quality,
            bit_depth,
            dither,
            size,
            filter,
            profile,
            intent,
            black_point_compensation: bool_of(object, "black_point_compensation", false)?,
            sharpen,
            embed_profile: bool_of(object, "embed_profile", true)?,
            strip_metadata: bool_of(object, "strip_metadata", false)?,
            metadata,
            float_samples,
            hdr_encoding,
            keep_spots,
        };
        request.validate()?;
        Ok(request)
    }

    /// The cross-field rules, checked once so both depths refuse the same
    /// combinations with the same sentence.
    fn validate(&self) -> Result<(), String> {
        if self.bit_depth != 8 && self.format == Format::Jxl {
            return Err("a JPEG XL export is 8-bit only; set bit_depth to 8".into());
        }
        // OpenEXR and Radiance are float containers: there is no 8- or
        // 16-bit form of either to write.
        if self.bit_depth != 32 {
            match self.format {
                Format::Exr => {
                    return Err(
                        "an OpenEXR export is 32-bit float only; set bit_depth to 32".into(),
                    )
                }
                Format::Hdr => {
                    return Err(
                        "a Radiance HDR export is 32-bit float only; set bit_depth to 32".into(),
                    )
                }
                _ => {}
            }
        }
        if self.bit_depth == 16
            && !matches!(
                self.format,
                Format::Png | Format::Tiff | Format::Heic | Format::Avif
            )
        {
            return Err(
                "a 16-bit export requires png, tiff, heic or avif; every other format is 8-bit only"
                    .into(),
            );
        }
        if self.bit_depth == 16 && self.dither != PrecisionDither::None {
            return Err("dithering applies only when reducing to 8 bits".into());
        }
        // Half samples exist in the two containers that define them.
        if self.float_samples == FloatSampleType::F16
            && !(self.bit_depth == 32 && matches!(self.format, Format::Tiff | Format::Exr))
        {
            return Err(
                "export float_samples \"f16\" requires bit_depth 32 and tiff or exr".into(),
            );
        }
        // A 32-bit export writes the float projection's own extended-range
        // samples, so every stage that would move a sample other than the
        // resample is refused BY NAME rather than silently bypassed. The
        // resample is the extended-range resize.
        // An HDR HEIC or AVIF is the float document's host-encoded HDR
        // deliverable: it exists only at 32 bits.
        if self.hdr_encoding.is_some()
            && !(self.bit_depth == 32 && matches!(self.format, Format::Heic | Format::Avif))
        {
            return Err("export hdr_encoding requires bit_depth 32 and heic or avif".into());
        }
        if self.bit_depth == 32 {
            if self.float_container().is_none() && self.hdr_encoding.is_none() {
                return Err("a 32-bit float export requires tiff, exr or hdr".into());
            }
            if self.sharpen.is_some_and(|s| !s.is_bypass()) {
                return Err("a 32-bit float export cannot sharpen".into());
            }
            if !matches!(self.profile, OutputProfile::Document) {
                return Err(
                    "a 32-bit float export writes the document profile; use profile document"
                        .into(),
                );
            }
            if self.dither != PrecisionDither::None {
                return Err("dithering applies only when reducing to 8 bits".into());
            }
        }
        if let Some(reason) = self.metadata.refusal(self.format, self.strip_metadata) {
            return Err(reason);
        }
        if self.keep_spots
            && (self.format != Format::Tiff || !matches!(self.size, OutputSize::None))
        {
            return Err(
                "export request spot_channels \"keep\" requires format tiff and size none".into(),
            );
        }
        Ok(())
    }

    /// The refusal of an export whose convert stage would encode into a
    /// matrix/TRC profile whose curves fail the precision check —
    /// `dr_icc_describe`'s `export_refusal` sentence, asked before any work.
    /// `"document"` never converts (`export_stages` skips the stage), and the
    /// document's OWN bytes under a non-absolute intent with BPC off are the
    /// no-op conversion that never reaches the encoder, so neither refuses.
    pub(crate) fn destination_refusal(&self, document: &IccProfile) -> Option<String> {
        let to = self.profile.destination()?;
        if to.bytes() == document.bytes()
            && !self.intent.is_absolute()
            && !self.black_point_compensation
        {
            return None;
        }
        crate::icc_encode_refusal::encode_into_refusal(&to, "export")
    }

    /// The profile the export's pixels end in, or `None` for the document's
    /// own: a Lab Color document's own profile (the twin) is not one an RGB
    /// file can carry, so `"document"` writes sRGB there (#1 phase 1b,
    /// `lab_rgb`).
    pub(crate) fn destination_for(&self, document: &PrecisionDocument) -> Option<Arc<IccProfile>> {
        if document.is_lab() && matches!(self.profile, OutputProfile::Document) {
            return Some(IccProfile::srgb());
        }
        self.profile.destination()
    }

    /// The typed codec's format, for a 16-bit request.
    fn precision_format(&self) -> Result<PrecisionFormat, String> {
        match self.format {
            _ if self.bit_depth == 32 => Ok(PrecisionFormat::Tiff),
            Format::Png => Ok(PrecisionFormat::Png),
            Format::Tiff => Ok(PrecisionFormat::Tiff),
            // HEIC's and AVIF's intermediate is a TIFF: the typed chain
            // encodes exactly the `"tiff"` request's bytes and the host
            // transcodes them.
            Format::Heic | Format::Avif => Ok(PrecisionFormat::Tiff),
            _ => Err("a 16-bit export requires png, tiff, heic or avif".into()),
        }
    }

    /// The typed resize kernel matching the byte one, so a 16-bit export and
    /// its 8-bit twin resample through the same interpolation.
    pub(crate) fn geometry_filter(&self) -> GeometryFilter {
        match self.filter {
            FilterType::Nearest => GeometryFilter::Nearest,
            FilterType::Triangle => GeometryFilter::Bilinear,
            FilterType::CatmullRom => GeometryFilter::Bicubic,
            _ => GeometryFilter::Lanczos3,
        }
    }

    pub(crate) fn format_name(&self) -> &'static str {
        match self.format {
            Format::Png => "png",
            Format::Jpeg => "jpeg",
            Format::Tiff => "tiff",
            Format::Bmp => "bmp",
            Format::Gif => "gif",
            Format::Webp => "webp",
            Format::Heic => "heic",
            Format::Avif => "avif",
            Format::Jxl => "jxl",
            Format::Exr => "exr",
            Format::Hdr => "hdr",
        }
    }

    /// The float container a 32-bit request writes; `None` for a format with
    /// no float form.
    pub(crate) fn float_container(&self) -> Option<FloatContainer> {
        match self.format {
            Format::Tiff => Some(FloatContainer::Tiff),
            Format::Exr => Some(FloatContainer::Exr),
            Format::Hdr => Some(FloatContainer::Radiance),
            _ => None,
        }
    }

    /// The format the bytes coming out of the encode stage actually are.
    /// Every format but HEIC and AVIF encodes itself; those two have no
    /// encoder in this crate, so the chain writes the TIFF the host
    /// transcodes — which is
    /// also why the substitution happens HERE rather than at the sidecar
    /// call: the sidecar, the injector selection and the encoder must all
    /// see one format or the packets would describe a different container
    /// from the bytes.
    pub(crate) fn encode_format(&self) -> Format {
        match self.format {
            Format::Heic | Format::Avif => Format::Tiff,
            other => other,
        }
    }

    /// True while this request's bytes are an intermediate the host still
    /// has to encode. A JPEG XL result never is: this crate encodes it.
    fn is_host_encoded(&self) -> bool {
        matches!(self.format, Format::Heic | Format::Avif)
    }

    /// A JPEG XL codestream carries no profile and is read as sRGB, so the
    /// pixels about to be encoded must BE sRGB. `profile` is the space those
    /// pixels are in after the convert stage.
    fn jxl_refusal(&self, profile: &IccProfile) -> Option<String> {
        (self.format == Format::Jxl && !profile.describes_srgb())
            .then(|| "JPEG XL is written as sRGB; set the export profile to sRGB".to_string())
    }
}

fn parse_size(value: &Value) -> Result<OutputSize, String> {
    let object = object_of(value, "size")?;
    known_keys(object, &["mode", "value"], "size")?;
    let mode =
        string_of(object, "mode")?.ok_or_else(|| "export size requires a mode".to_string())?;
    if mode == "none" {
        return Ok(OutputSize::None);
    }
    let number = number_of(object, "value")?
        .ok_or_else(|| format!("export size mode \"{mode}\" requires a value"))?;
    if mode == "scale" {
        return Ok(OutputSize::Scale(number));
    }
    if number < 0.0 || number.fract() != 0.0 || number > f64::from(u32::MAX) {
        return Err(format!(
            "export size mode \"{mode}\" requires a whole number of pixels"
        ));
    }
    let pixels = number as u32;
    match mode.as_str() {
        "width" => Ok(OutputSize::Width(pixels)),
        "height" => Ok(OutputSize::Height(pixels)),
        "long_edge" => Ok(OutputSize::LongEdge(pixels)),
        other => Err(format!(
            "export size mode \"{other}\" is not one of none, width, height, long_edge or scale"
        )),
    }
}

// ------------------------------------------------------ output dimensions --

/// The output dimensions for `request.size` against a `source` canvas. Aspect
/// ratio is kept: the requested side is exact and the other is `round()` then
/// `max(1)`, so a 3 px tall thumbnail never collapses to nothing. A side past
/// the request format's [`max_output_side`] refuses, and so does a pixel
/// count past [`MAX_PIXELS`].
pub fn output_dimensions(
    source: (u32, u32),
    request: &ExportRequest,
) -> Result<(u32, u32), String> {
    let size = request.size;
    let (sw, sh) = source;
    if sw == 0 || sh == 0 {
        return Err("export source dimensions must be positive".into());
    }
    let scaled = |from: u32, to: u32, other: u32| -> u32 {
        let value = f64::from(other) * f64::from(to) / f64::from(from);
        value.round().max(1.0).min(f64::from(u32::MAX)) as u32
    };
    let (w, h) = match size {
        OutputSize::None => (sw, sh),
        OutputSize::Width(0) | OutputSize::Height(0) | OutputSize::LongEdge(0) => {
            return Err("export size value must be a positive number of pixels".into())
        }
        OutputSize::Width(w) => (w, scaled(sw, w, sh)),
        OutputSize::Height(h) => (scaled(sh, h, sw), h),
        OutputSize::LongEdge(v) => {
            if sw >= sh {
                (v, scaled(sw, v, sh))
            } else {
                (scaled(sh, v, sw), v)
            }
        }
        OutputSize::Scale(scale) => {
            if !scale.is_finite() || scale <= 0.0 || scale > MAX_OUTPUT_SCALE {
                return Err(format!(
                    "export scale must be greater than 0 and at most {MAX_OUTPUT_SCALE}"
                ));
            }
            let side = |v: u32| (f64::from(v) * scale).round().max(1.0);
            let (w, h) = (side(sw), side(sh));
            if w > f64::from(u32::MAX) || h > f64::from(u32::MAX) {
                return Err("export dimensions exceed the supported pixel limit".into());
            }
            (w as u32, h as u32)
        }
    };
    if let Some(cap) = max_output_side(request.format) {
        if w > cap || h > cap {
            return Err(format!(
                "export dimensions must be at most {cap} pixels on a side for {}",
                request.format_name()
            ));
        }
    }
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err("export dimensions exceed the supported pixel limit".into());
    }
    Ok((w, h))
}

// ---------------------------------------------------------- name patterns --

/// The tokens a naming pattern may use. The HOST supplies `date` (as a local
/// `YYYY-MM-DD` string) because a pure core has no calendar and no time zone.
pub struct NameTokens<'a> {
    pub name: &'a str,
    pub width: u32,
    pub height: u32,
    pub format: &'a str,
    pub date: &'a str,
    pub preset: &'a str,
    /// The layer or group being exported (Phase 11B). `None` for a whole
    /// document export, where `{layer}` is REFUSED rather than expanded to
    /// nothing: a pattern that silently lost a token would name every item
    /// in a batch the same thing.
    pub layer: Option<&'a str>,
    /// `(index, count)` within the batch, 1-based. `{index}` is zero-padded
    /// to the digit width of `count`, so the delivered files sort correctly
    /// in every file browser.
    pub index: Option<(u32, u32)>,
    /// True for a layer or group export (Phase 11B): the mandatory token is
    /// then `{layer}` or `{index}` instead of `{name}`.
    pub layer_export: bool,
}

impl NameTokens<'_> {
    fn value(&self, token: &str) -> Result<String, String> {
        let absent = |what: &str| {
            Err(format!(
                "a naming pattern cannot use {{{token}}} here: {what}"
            ))
        };
        match token {
            "name" => Ok(self.name.to_string()),
            "width" => Ok(self.width.to_string()),
            "height" => Ok(self.height.to_string()),
            "format" => Ok(self.format.to_string()),
            "date" => Ok(self.date.to_string()),
            "preset" => Ok(self.preset.to_string()),
            "layer" => match self.layer {
                Some(layer) => Ok(sanitized_layer(layer)),
                None => absent("this export has no layer"),
            },
            "index" => match self.index {
                Some((index, count)) => {
                    let width = count.max(1).to_string().len();
                    Ok(format!("{index:0width$}"))
                }
                None => absent("this export has no item index"),
            },
            _ => Err(format!(
                "a naming pattern cannot use the unknown token {{{token}}}"
            )),
        }
    }
}

/// A layer name as a file-name fragment: the three characters
/// `expand_name_pattern` refuses in its result, plus control characters,
/// become `-`, surrounding whitespace is trimmed, and a name that has nothing
/// left becomes the literal `layer`. Sanitized rather than refused, because a
/// layer called "Sky / Water" is an ordinary layer and refusing the whole
/// batch over it would be useless.
fn sanitized_layer(name: &str) -> String {
    let mapped: String = name
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c == ':' || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    let trimmed = mapped.trim();
    if trimmed.is_empty() {
        "layer".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Expands `pattern` into a BASE file name (no extension, no directory).
///
/// For a whole-document export `{name}` is mandatory: it is the only thing
/// that tells documents apart in one folder, and a pattern without it names
/// every document the same thing, which is how a batch silently overwrites
/// its own output. A layer export's items (`tokens.layer_export`) all
/// share the document's name, so there `{layer}` or `{index}` is what tells
/// them apart and is mandatory instead; `{name}` is optional. The result must
/// also be a plain file name — no separator, no drive colon, no leading dot —
/// because the destination folder is chosen separately and a pattern must not
/// be able to escape it.
pub fn expand_name_pattern(pattern: &str, tokens: &NameTokens) -> Result<String, String> {
    if tokens.layer_export {
        if !pattern.contains("{layer}") && !pattern.contains("{index}") {
            return Err("a layer export naming pattern must contain {layer} or {index}".into());
        }
    } else if !pattern.contains("{name}") {
        return Err("a naming pattern must contain {name}".into());
    }
    let mut out = String::new();
    let mut rest = pattern;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        let end = tail
            .find('}')
            .ok_or_else(|| "a naming pattern has an unterminated { token".to_string())?;
        let token = &tail[..end];
        out.push_str(&tokens.value(token)?);
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    if out.is_empty() {
        return Err("a naming pattern must expand to a non-empty file name".into());
    }
    if out.chars().count() > MAX_NAME_CHARS {
        return Err(format!(
            "a naming pattern must expand to at most {MAX_NAME_CHARS} characters"
        ));
    }
    if out
        .chars()
        .any(|c| c == '/' || c == '\\' || c == ':' || c.is_control())
    {
        return Err(
            "a naming pattern must expand to a plain file name with no / \\ : or control characters"
                .into(),
        );
    }
    if out.starts_with('.') {
        return Err("a naming pattern must not expand to a name beginning with a dot".into());
    }
    Ok(out)
}

// ---------------------------------------------------------------- results --

/// One encoded deliverable: the exact bytes, what they describe, and the two
/// things a host does with them — decode a preview, or write the file.
#[derive(Clone)]
pub struct ExportResult {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
    carried: u32,
    bit_depth: u32,
    format: &'static str,
    profile: &'static str,
    /// The destination profile's display NAME (Phase 11C). "document" keeps
    /// the document's own, which is what a host shows beside a deliverable.
    profile_name: String,
    intent: &'static str,
    black_point_compensation: bool,
    /// What the encode wrote packet by packet (Phase 11B). Always present:
    /// a default policy simply reports what `carried` already says.
    metadata: MetadataOutcome,
    /// Present only on a layer or group export; see `doc_export_layers`.
    layer: Option<LayerExportInfo>,
    /// `Some("tiff")` while this is a HOST-ENCODED export whose bytes are
    /// still the intermediate the host has to transcode (Phase 11D), `None`
    /// after [`ExportResult::adopt_host_bytes`] and for every other format.
    host_intermediate: Option<&'static str>,
    /// Present only on a 32-bit FLOAT deliverable: what its preview needs
    /// (`doc_export_float`).
    float: Option<FloatDeliverable>,
    /// Present only on a 32-bit HDR HEIC or AVIF: its encoding and content
    /// headroom (`doc_export_float`), kept through `adopt_host_bytes`.
    hdr: Option<HdrDeliverable>,
    /// True for a CMYK document's deliverable that keeps CMYK (`cmyk_export`):
    /// a TIFF, or a JPEG pending the host's CMYK transcode.
    cmyk: bool,
}

impl ExportResult {
    /// A 32-bit float deliverable (`doc_export_float::export_float`): the
    /// encoded bytes, bounded like every other export's, plus the exporting
    /// document's tone policy for the preview.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn float_deliverable(
        bytes: Vec<u8>,
        dimensions: (u32, u32),
        carried: u32,
        request: &ExportRequest,
        profile_name: String,
        metadata: MetadataOutcome,
        float: FloatDeliverable,
    ) -> Result<Self, String> {
        Ok(ExportResult {
            bytes: bounded(bytes)?,
            width: dimensions.0,
            height: dimensions.1,
            carried,
            bit_depth: 32,
            format: request.format_name(),
            profile: request.profile.name(),
            profile_name,
            intent: request.intent.name(),
            black_point_compensation: request.black_point_compensation,
            metadata,
            layer: None,
            host_intermediate: None,
            float: Some(float),
            hdr: None,
            cmyk: false,
        })
    }

    /// A 32-bit HDR HEIC or AVIF's PENDING result
    /// (`doc_export_float::export_hdr`): the intermediate the host
    /// transcodes, and what the host's encode needs to know.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn hdr_intermediate(
        bytes: Vec<u8>,
        dimensions: (u32, u32),
        carried: u32,
        request: &ExportRequest,
        profile_name: String,
        metadata: MetadataOutcome,
        hdr: HdrDeliverable,
    ) -> Result<Self, String> {
        Ok(ExportResult {
            bytes: bounded(bytes)?,
            width: dimensions.0,
            height: dimensions.1,
            carried,
            bit_depth: 32,
            format: request.format_name(),
            profile: request.profile.name(),
            profile_name,
            intent: request.intent.name(),
            black_point_compensation: request.black_point_compensation,
            metadata,
            layer: None,
            host_intermediate: Some("tiff"),
            float: None,
            hdr: Some(hdr),
            cmyk: false,
        })
    }

    /// A CMYK document's deliverable that keeps CMYK (`cmyk_export`): a
    /// TIFF's final bytes, or a JPEG's 8-bit CMYK TIFF intermediate pending
    /// the host's CMYK transcode.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cmyk_kept(
        bytes: Vec<u8>,
        dimensions: (u32, u32),
        carried: u32,
        bit_depth: u32,
        request: &ExportRequest,
        profile_name: String,
        metadata: MetadataOutcome,
        pending: bool,
    ) -> Result<Self, String> {
        Ok(ExportResult {
            bytes: bounded(bytes)?,
            width: dimensions.0,
            height: dimensions.1,
            carried,
            bit_depth,
            format: request.format_name(),
            profile: request.profile.name(),
            profile_name,
            intent: request.intent.name(),
            black_point_compensation: request.black_point_compensation,
            metadata,
            layer: None,
            host_intermediate: pending.then_some("tiff"),
            float: None,
            hdr: None,
            cmyk: true,
        })
    }

    /// A Grayscale document's deliverable that keeps gray (`gray_export`):
    /// one channel (two with alpha), the Gray profile's name.
    pub(crate) fn gray_kept(
        bytes: Vec<u8>,
        dimensions: (u32, u32),
        carried: u32,
        bit_depth: u32,
        request: &ExportRequest,
        profile_name: String,
        metadata: MetadataOutcome,
    ) -> Result<Self, String> {
        Ok(ExportResult {
            bytes: bounded(bytes)?,
            width: dimensions.0,
            height: dimensions.1,
            carried,
            bit_depth,
            format: request.format_name(),
            profile: request.profile.name(),
            profile_name,
            intent: request.intent.name(),
            black_point_compensation: request.black_point_compensation,
            metadata,
            layer: None,
            host_intermediate: None,
            float: None,
            hdr: None,
            cmyk: false,
        })
    }

    /// An Indexed Color document's deliverable that keeps Indexed
    /// (`indexed_export`): the palette and every index, at 8 bits, the
    /// document profile's name.
    pub(crate) fn indexed_kept(
        bytes: Vec<u8>,
        dimensions: (u32, u32),
        carried: u32,
        request: &ExportRequest,
        profile_name: String,
        metadata: MetadataOutcome,
    ) -> Result<Self, String> {
        Ok(ExportResult {
            bytes: bounded(bytes)?,
            width: dimensions.0,
            height: dimensions.1,
            carried,
            bit_depth: 8,
            format: request.format_name(),
            profile: request.profile.name(),
            profile_name,
            intent: request.intent.name(),
            black_point_compensation: request.black_point_compensation,
            metadata,
            layer: None,
            host_intermediate: None,
            float: None,
            hdr: None,
            cmyk: false,
        })
    }

    /// The result reporting `request`'s profile choice: a CMYK document
    /// converted to RGB (`cmyk_export`) runs the stages over pixels already
    /// in the destination, so the stages saw `"document"`.
    pub(crate) fn reported_as(mut self, request: &ExportRequest) -> Self {
        self.profile = request.profile.name();
        self
    }

    /// Attach the item description a layer export carries. Called only by
    /// `doc_export_layers`, immediately after the stages that built the
    /// bytes, so a whole-document result can never grow one.
    pub(crate) fn with_layer(mut self, layer: LayerExportInfo) -> Self {
        self.layer = Some(layer);
        self
    }

    /// The encoded bytes. On a PENDING host-encoded result these are the
    /// INTERMEDIATE the host transcodes rather than the deliverable — which
    /// is why they stay readable: `adopt_host_bytes` has to be given
    /// something to transcode. Nothing writes them to disk in that state
    /// ([`ExportResult::write`] refuses).
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// True while the bytes are a host-encoded export's intermediate.
    pub fn is_host_pending(&self) -> bool {
        self.host_intermediate.is_some()
    }

    /// Replace a pending host-encoded result's intermediate with the bytes
    /// the HOST encoder wrote, and with what an inspection of those bytes
    /// measured: `bit_depth` is the depth the host actually wrote (8 or 10
    /// for HEIC, never the request's 16) and `carried` the `DR_CARRIES_*`
    /// bits an independent read of the written file found.
    ///
    /// Pure, like every other op: a new result. Refuses when this result is
    /// not pending, because adopting bytes into a finished export would put
    /// a file's contents behind a report that describes a different encode.
    /// The packet report is rebuilt from the MEASURED `carried` bits rather
    /// than kept from the intermediate — the host drops what its container
    /// cannot hold (IPTC, today) and a report that said otherwise would be
    /// contradicted by the file. `stamped` survives only with the XMP packet
    /// that holds it.
    pub fn adopt_host_bytes(
        &self,
        bytes: Vec<u8>,
        bit_depth: u32,
        carried: u32,
    ) -> Result<ExportResult, String> {
        if !self.is_host_pending() {
            return Err(
                "this export is not a host-encoded export awaiting the host's bytes".into(),
            );
        }
        let mut out = self.clone();
        out.bytes = bounded(bytes)?;
        out.bit_depth = bit_depth;
        out.carried = carried;
        out.metadata = MetadataOutcome {
            profile: carried & crate::doc_color::CARRIES_PROFILE != 0,
            exif: carried & crate::doc_color::CARRIES_EXIF != 0,
            xmp: carried & crate::doc_color::CARRIES_XMP != 0,
            iptc: carried & crate::doc_color::CARRIES_IPTC != 0,
            density: carried & crate::doc_color::CARRIES_RESOLUTION != 0,
            stamped: self.metadata.stamped && carried & crate::doc_color::CARRIES_XMP != 0,
        };
        out.host_intermediate = None;
        Ok(out)
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The `DR_CARRIES_*` bits the encode actually wrote, exactly as
    /// `dr_doc_save_image` reports them.
    pub fn carried(&self) -> u32 {
        self.carried
    }

    pub fn info_json(&self) -> Value {
        let mut info = json!({
            "bytes": self.bytes.len(),
            "width": self.width,
            "height": self.height,
            "carried": self.carried,
            "bit_depth": self.bit_depth,
            "format": self.format,
            "profile": self.profile,
            "profile_name": self.profile_name,
            "intent": self.intent,
            "black_point_compensation": self.black_point_compensation,
            "metadata": self.metadata.to_json(),
            "host_encoded": matches!(self.format, "heic" | "avif"),
            // The core decodes none of the three, so the host previews them.
            "host_preview": matches!(self.format, "heic" | "avif" | "jxl"),
            "intermediate": self.host_intermediate,
        });
        if let Some(float) = &self.float {
            info["float_samples"] = json!(float.samples_name());
        }
        if self.cmyk {
            info["cmyk"] = json!(true);
        }
        if let Some(hdr) = &self.hdr {
            info["hdr_encoding"] = json!(hdr.encoding.name());
            info["content_headroom"] = json!(hdr.content_headroom);
        }
        if let Some(layer) = &self.layer {
            info["layer"] = json!({
                "index": layer.index,
                "name": layer.name,
                "origin": [layer.origin.0, layer.origin.1],
                "size": [layer.size.0, layer.size.1],
            });
        }
        info
    }

    /// Decode the bytes that WILL BE WRITTEN. This is the preview contract:
    /// there is no second encode, so the preview cannot describe a different
    /// file from the one `write` produces.
    /// A host-encoded export is previewed by the host: this crate decodes
    /// neither the deliverable (no HEIC or AVIF decoder) nor its intermediate
    /// (the preview would then be of a file nobody receives). A JPEG XL
    /// export is encoded here but has no decoder here either, so the host
    /// previews it too, from the result's own bytes (`"host_preview"`).
    /// A 32-bit float deliverable is decoded through its own seam and shown
    /// under the exporting document's tone map (`doc_export_float`), never
    /// through the 8-bit decoder, which would clamp it. A palette TIFF (a
    /// kept-Indexed export, `indexed_export`) is decoded through
    /// `indexed_tiff`, since the generic decoder refuses Photometric 3.
    pub fn decode_preview(&self) -> Result<DrImage, String> {
        if matches!(self.format, "heic" | "avif") {
            return Err("a host-encoded export is previewed by the host".into());
        }
        if self.format == "jxl" {
            return Err("a JPEG XL export is previewed by the host".into());
        }
        if let Some(float) = &self.float {
            return float.preview(&self.bytes);
        }
        if self.format == "tiff" {
            if let Some(preview) = crate::indexed_tiff::preview_rgba8(&self.bytes) {
                return preview;
            }
        }
        Ok(DrImage::open_bytes(&self.bytes, "the encoded export")?.0)
    }

    /// Write the bytes atomically. An existing `path` is REFUSED unless
    /// `overwrite`: an export never clobbers a file by accident, and the one
    /// caller that passes `true` is a save panel that already asked.
    pub fn write(&self, path: &str, overwrite: bool) -> Result<(), String> {
        // The ONE core path from a result to a file, which is what makes
        // "a pending intermediate never reaches disk" checkable in one place.
        if self.is_host_pending() {
            return Err(
                "a host-encoded export must adopt the host encoder's bytes before it is written"
                    .into(),
            );
        }
        if !overwrite && std::path::Path::new(path).exists() {
            return Err(format!("refusing to overwrite the existing file {path}"));
        }
        crate::dr_image::save_atomically(path, |temporary| {
            std::fs::write(temporary, &self.bytes)
                .map_err(|e| format!("failed to write {path}: {e}"))
        })
    }
}

/// Refuse an encode whose output no host would want in memory, before it is
/// handed back across the FFI.
fn bounded(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    if bytes.len() as u64 > MAX_ENCODED_BYTES {
        return Err("the encoded export exceeds the bounded encoded-byte limit".into());
    }
    Ok(bytes)
}

// ----------------------------------------------------------- the byte path --

impl DrDocument {
    /// Encode this document as a deliverable, at 8 bits.
    ///
    /// `flat` has exactly `save_image`'s semantics: a caller-supplied
    /// composite of THIS document (the host's warm projection), or `None` to
    /// flatten here. The document supplies only the profile, the packets and
    /// the resolution.
    pub fn export_encoded(
        &self,
        flat: Option<&DrImage>,
        request: &ExportRequest,
        control: Option<&DrJobControl>,
    ) -> Result<ExportResult, String> {
        if request.bit_depth != 8 {
            return Err("an 8-bit document exports at 8 bits; convert it to 16-bit first".into());
        }
        if let Some(refusal) = request.destination_refusal(&self.profile) {
            return Err(refusal);
        }
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;

        let flattened;
        let source = match flat {
            Some(image) => &image.pixels,
            None => {
                flattened = self.flattened()?;
                &flattened
            }
        };
        self.export_stages(source, request, job)
    }

    /// Stages 1–5 over an ALREADY COMPOSITED source: resample, sharpen,
    /// convert, reduce (nothing at 8 bits) and encode. Split out of
    /// `export_encoded` verbatim so a layer export
    /// (`doc_export_layers::export_layer_encoded`) runs the same chain over
    /// its isolated render instead of growing a second copy of it — the
    /// document still supplies the profile, the packets and the resolution.
    pub(crate) fn export_stages(
        &self,
        source: &RgbaImage,
        request: &ExportRequest,
        job: JobContext<'_>,
    ) -> Result<ExportResult, String> {
        let (w, h) = output_dimensions(source.dimensions(), request)?;
        job.check()?;

        // 1. Resample.
        let resampled = if source.dimensions() == (w, h) {
            Arc::new(source.clone())
        } else {
            Arc::new(
                crate::ops::resize(source, w, h, request.filter)
                    .ok_or("export resampling produced invalid dimensions")?,
            )
        };
        job.check()?;

        // 2. Sharpen (`export_sharpen`).
        let sharpened = match request.sharpen {
            Some(sharpen) if !sharpen.is_bypass() => sharpen.bytes(resampled, job)?,
            _ => resampled,
        };
        job.check()?;

        // 3. Convert. Phase 11C routes the FLAT image through `ColorPipeline`
        // rather than `retained_source_render::color8`, so an intent, BPC and
        // a LUT destination all work here. At the DEFAULTS the pipeline holds
        // the very `Transform` `color8` built, so the bytes are 11A's bytes
        // (`export_pipeline`'s equalities are the measurement).
        let destination = request.profile.destination();
        let converted = match &destination {
            Some(to) => convert8(&sharpened, &self.profile, to, request, job)?,
            None => sharpened,
        };
        job.check()?;

        // 4 & 5. Reduce (nothing to do at 8 bits) and encode.
        let profile = destination.as_deref().unwrap_or(&self.profile);
        if let Some(reason) = request.jxl_refusal(profile) {
            return Err(reason);
        }
        // HEIC substitutes its TIFF intermediate HERE (Phase 11D), so the
        // packets, the injector and the encoder all describe the container
        // the bytes really are.
        let (mut side, carried, metadata) = crate::doc_color::image_sidecar_with_policy(
            profile,
            &self.metadata,
            self.resolution,
            request.encode_format(),
            request.embed_profile,
            request.strip_metadata,
            (w, h),
            &request.metadata,
        );
        // A TIFF carries the paths in its 8BIM run (`tiff_paths`); a HEIC or
        // AVIF intermediate is the host's to transcode, so it does not.
        if request.format == Format::Tiff {
            crate::tiff_paths::attach(&mut side, self.tiff_path_run());
        }
        let bytes = encode_bytes(&converted, request, &side)?;
        job.report(1.0, 5)?;
        Ok(ExportResult {
            bytes: bounded(bytes)?,
            width: w,
            height: h,
            carried,
            bit_depth: 8,
            format: request.format_name(),
            profile: request.profile.name(),
            profile_name: profile.name().to_string(),
            intent: request.intent.name(),
            black_point_compensation: request.black_point_compensation,
            metadata,
            layer: None,
            host_intermediate: request.is_host_encoded().then_some("tiff"),
            float: None,
            hdr: None,
            cmyk: false,
        })
    }
}

/// The Phase 11C convert stage at 8 bits: one `ColorPipeline` over a flat
/// image. Separate from `retained_source_render::color8` because a RETAINED
/// SOURCE has its own cache and its own matrix/TRC contract, which this
/// stage — a flat deliverable, converted once — does not share.
fn convert8(
    source: &Arc<RgbaImage>,
    from: &Arc<IccProfile>,
    to: &Arc<IccProfile>,
    request: &ExportRequest,
    job: JobContext<'_>,
) -> Result<Arc<RgbaImage>, String> {
    let pipeline = ColorPipeline::new(from, to, request.intent, request.black_point_compensation)?;
    if pipeline.is_identity() {
        return Ok(source.clone());
    }
    let mut bytes = crate::precision_image::reserved_vec(source.as_raw().len())?;
    for (i, pixel) in source.pixels().enumerate() {
        job.every(i)?;
        let rgb = pipeline.color8([pixel[0], pixel[1], pixel[2]]);
        bytes.extend_from_slice(&[rgb[0], rgb[1], rgb[2], pixel[3]]);
    }
    RgbaImage::from_raw(source.width(), source.height(), bytes)
        .map(Arc::new)
        .ok_or_else(|| "export conversion produced invalid dimensions".into())
}

/// The same stage at 16 bits. `is_identity` is deliberately NOT a shortcut
/// here: it is the approximate `MatrixTrc::approx_eq` question, while the
/// typed transform's own no-op test is the EXACT one, so skipping on the
/// looser test would change samples an 11A export already writes.
fn convert16(
    source: &PrecisionImage,
    to: &Arc<IccProfile>,
    request: &ExportRequest,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    if source.profile().bytes() == to.bytes()
        && request.intent == RenderingIntent::RelativeColorimetric
        && !request.black_point_compensation
    {
        return Ok(source.with_profile(to.clone()));
    }
    let pipeline = ColorPipeline::new(
        source.profile(),
        to,
        request.intent,
        request.black_point_compensation,
    )?;
    let runs = source
        .runs_rgba16()
        .ok_or("high-depth export requires RGBA16 UNORM storage")?;
    let (w, h) = source.dimensions();
    let mut pixels = crate::precision_image::reserved_vec(w as usize * h as usize)?;
    let mut i = 0;
    for run in runs {
        for pixel in run {
            job.every(i)?;
            i += 1;
            let rgb = pipeline.color(std::array::from_fn(|c| f32::from(pixel[c]) / 65535.));
            if rgb
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            {
                return Err("export color transform produced an invalid component".into());
            }
            pixels.push([
                (rgb[0] * 65535.).round() as u16,
                (rgb[1] * 65535.).round() as u16,
                (rgb[2] * 65535.).round() as u16,
                pixel[3],
            ]);
        }
    }
    PrecisionImage::from_rgba16_unseeded(w, h, pixels, to.clone(), None)
}

/// The byte encoder, behind the SAME `MetadataInjector` selection
/// `dr_image::save_flat` uses — the only difference is that the sink is a
/// `Cursor<Vec<u8>>` rather than a `BufWriter<File>`.
fn encode_bytes(
    pixels: &RgbaImage,
    request: &ExportRequest,
    side: &crate::metadata_write::EncodeSidecar<'_>,
) -> Result<Vec<u8>, String> {
    let format = request.encode_format();
    let extra = match format {
        Format::Jpeg => side.jpeg_segments(),
        Format::Png => side.png_chunks(),
        _ => Vec::new(),
    };
    let writer = Cursor::new(Vec::new());
    let mut injector = match format {
        _ if extra.is_empty() => MetadataInjector::passthrough(writer),
        Format::Jpeg => MetadataInjector::jpeg(writer, extra),
        Format::Png => MetadataInjector::png(writer, extra),
        _ => MetadataInjector::passthrough(writer),
    };
    crate::dr_image::encode_into(pixels, &mut injector, format, request.jpeg_quality, side)?;
    Ok(injector.finish()?.into_inner())
}

// ------------------------------------------------------------ the u16 path --

impl PrecisionDocument {
    /// Admission for one typed export, before any plane is allocated.
    /// `retained_bytes` is the unique union of the document and (when
    /// supplied) its projection, so a caller's own allowance must exclude
    /// both — exactly `projection_export_plan`'s rule.
    pub fn export_plan(
        &self,
        projection: Option<&PrecisionImage>,
        request: &ExportRequest,
    ) -> Result<Value, String> {
        if let Some(refusal) = self.tiff_keep_refusal(request) {
            return Err(refusal);
        }
        if self.is_cmyk() {
            return self.cmyk_export_plan(request);
        }
        // A Bitmap document (#1 Bitmap phase 1c, `bitmap_export`): its
        // refusals, then a kept 1-bit file; JPEG and the RGB profiles fall
        // through to the Gray storage's stages.
        if let Some(reason) = self.bitmap_export_refusal(request) {
            return Err(reason);
        }
        if self.keeps_bitmap(request) {
            return self.bitmap_export_plan(request);
        }
        // An Indexed Color document that keeps Indexed (`indexed_export`).
        if self.keeps_indexed(request) {
            return self.indexed_export_plan(request);
        }
        let mut plan = self.export_plan_for(self.dimensions(), projection, request)?;
        // The spot channels' overprint of the flattened source (#1 1c-1,
        // `spot_typed`); a layer export never overprints, so the shared
        // `export_plan_for` does not charge it. A TIFF that keeps them
        // charges their planes instead (1c-2a, `tiff_spot`).
        if self.has_spot_channel() {
            let term = if request.keep_spots {
                self.tiff_spot_scratch_bytes(u64::from(request.bit_depth / 8), 3)
            } else {
                self.spot_overprint_scratch_bytes()?
            };
            let scratch = plan["scratch_bytes"]
                .as_u64()
                .unwrap_or(0)
                .saturating_add(term);
            plan["scratch_bytes"] = Value::from(scratch);
        }
        Ok(plan)
    }

    /// [`Self::export_plan`] over a rendered source of `source` dimensions —
    /// the canvas, or a trimmed layer export's cut (`export_layer_encoded`
    /// re-admits with it). The ONE export plan formula.
    pub(crate) fn export_plan_for(
        &self,
        source: (u32, u32),
        projection: Option<&PrecisionImage>,
        request: &ExportRequest,
    ) -> Result<Value, String> {
        // A Duotone document (#1 Duotone phase 1b) exports RGB through its
        // inks: the RGB document its storage is, plus the view's scratch.
        if self.is_duotone() {
            let mut plan = self
                .without_gray()
                .export_plan_for(source, projection, request)?;
            let scratch = plan["scratch_bytes"].as_u64().unwrap_or(0).saturating_add(
                crate::duotone_document::view_scratch_bytes(source.0, source.1),
            );
            plan["scratch_bytes"] = Value::from(scratch);
            return Ok(plan);
        }
        if let Some(refusal) = request.destination_refusal(self.profile()) {
            return Err(refusal);
        }
        if let Some(refusal) = self.gray_export_refusal(request) {
            return Err(refusal);
        }
        if let Some(image) = projection {
            self.check_projection(image)?;
        }
        let (sw, sh) = source;
        let (w, h) = output_dimensions((sw, sh), request)?;
        let source = pixel_count(sw, sh)? as u64;
        let out = pixel_count(w, h)? as u64;
        let float_resample = self.is_float() && request.bit_depth == 32 && (sw, sh) != (w, h);

        let resampled = if (sw, sh) == (w, h) || float_resample {
            0
        } else {
            out * 8
        };
        // The method's own term (`OutputSharpen::plan_bytes`); a bypass
        // spends none of it.
        let sharpen = match request.sharpen {
            Some(sharpen) => sharpen.plan_bytes((w, h))?,
            None => 0,
        };
        let converted = if self.is_lab() {
            // A Lab document converts every export into RGB (`lab_rgb`).
            let to = request
                .destination_for(self)
                .unwrap_or_else(IccProfile::srgb);
            out * crate::lab_rgb::CONVERT_BYTES_PER_PIXEL
                + crate::lab_rgb::lab_to_rgb_scratch(
                    &to,
                    request.intent,
                    request.black_point_compensation,
                )?
        } else if matches!(request.profile, OutputProfile::Document) {
            0
        } else {
            out * 8
        };
        let quantized = if request.bit_depth == 8 { out * 4 } else { 0 };
        // A kept-gray export's one-channel buffers (`gray_export`).
        let gray = if self.keeps_gray(request) {
            out * crate::gray_export::EXPORT_BYTES_PER_PIXEL
        } else {
            0
        };
        // A float document's projection is 16 B/px, and an 8/16-bit request
        // first writes the tone-mapped u16 plane beside it; a 32-bit request
        // instead builds the flat f32 sample vector the encoder takes plus
        // its Deflate buffer. A 32-bit resample adds its f32 output,
        // the source's transient contiguous copy, and the larger of the
        // separable temporaries and the output's tiling copy (the resize
        // drops its temporaries before that adoption).
        let float = if self.is_float() {
            source * 8
                + if float_resample {
                    out * 16 * 2
                        + 1024 * 1024
                        + out * 16
                        + precision_sampling::float_source_copy_bytes((sw, sh))?
                        + precision_sampling::resize_cost(
                            (sw, sh),
                            (w, h),
                            request.geometry_filter(),
                            true,
                        )?
                        .1
                        .max(precision_sampling::float_adoption_copy_bytes((w, h))?)
                } else if request.bit_depth == 32 {
                    source * 16 * 2 + 1024 * 1024
                } else {
                    0
                }
        } else {
            0
        };
        let scratch = source * 8
            + float
            + resampled
            + sharpen
            + converted
            + quantized
            + gray
            + precision_codec::encode_dimensions_scratch((w, h))?
            + BOOKKEEPING_BYTES;

        let mut inventory = Inventory::default();
        self.record_resources(&mut inventory);
        if let Some(image) = projection {
            image.record_resources(&mut inventory);
        }
        Ok(json!({
            "retained_bytes": inventory.unique_bytes(),
            "scratch_bytes": scratch,
            "width": w,
            "height": h,
        }))
    }

    /// The typed projection must belong to this canvas, exactly as
    /// `projection_export_plan` requires: pixel provenance cannot be
    /// inferred without repeating the render.
    pub(crate) fn check_projection(&self, image: &PrecisionImage) -> Result<(), String> {
        let expected = self.render_storage();
        if image.storage() != expected {
            return Err(if self.is_float() {
                "a 32-bit float export requires rgba32_float storage".into()
            } else {
                "high-depth export requires RGBA16 UNORM storage".to_string()
            });
        }
        if image.dimensions() != self.dimensions() {
            return Err("high-depth projection dimensions do not match the document canvas".into());
        }
        if !Arc::ptr_eq(image.profile(), self.profile())
            && image.profile().bytes() != self.profile().bytes()
        {
            return Err("high-depth projection profile bytes do not match the document".into());
        }
        Ok(())
    }

    /// Encode this typed document as a deliverable, at 16 or 8 bits.
    pub fn export_encoded(
        &self,
        projection: Option<&PrecisionImage>,
        request: &ExportRequest,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<ExportResult, String> {
        if let Some(refusal) = self.tiff_keep_refusal(request) {
            return Err(refusal);
        }
        if self.is_cmyk() {
            let _run = control.map(DrJobControl::begin).transpose()?;
            let job = control.map_or(JobContext::NONE, JobContext::new);
            return self.cmyk_export_encoded(request, limit, other, job);
        }
        // A Bitmap document's kept 1-bit file (#1 Bitmap phase 1c).
        if self.keeps_bitmap(request) {
            let _run = control.map(DrJobControl::begin).transpose()?;
            let job = control.map_or(JobContext::NONE, JobContext::new);
            return self.bitmap_export_encoded(request, limit, other, job);
        }
        if self.keeps_indexed(request) {
            let _run = control.map(DrJobControl::begin).transpose()?;
            let job = control.map_or(JobContext::NONE, JobContext::new);
            return self.indexed_export_encoded(request, limit, other, job);
        }
        let plan = self.export_plan(projection, request)?;
        let retained = plan["retained_bytes"].as_u64().unwrap_or(0);
        let scratch = plan["scratch_bytes"].as_u64().unwrap_or(0);
        admit(retained, scratch, other, limit)?;

        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        job.reserve(scratch)?;

        let rendered;
        let source = match projection {
            Some(image) => image,
            None => {
                rendered = self.render_with_context(limit, other, job)?;
                &rendered
            }
        };
        // A flattened export composites the spot channels' overprint
        // (#1 1c-1, `spot_typed`), unless its TIFF keeps them (1c-2a).
        if self.has_spot_channel() && !request.keep_spots {
            return self.spot_export_stages(source, request, job);
        }
        self.export_stages(source, request, job)
    }

    /// Stages 1–5 over an ALREADY RENDERED typed source — the twin of
    /// `DrDocument::export_stages`, split out for the same reason: a typed
    /// layer export renders ONE isolated root and then runs exactly this
    /// chain. Admission and the job token belong to the caller.
    pub(crate) fn export_stages(
        &self,
        source: &PrecisionImage,
        request: &ExportRequest,
        job: JobContext<'_>,
    ) -> Result<ExportResult, String> {
        // A Duotone document (#1 Duotone phase 1b): the RGB document its
        // storage is, over the view of `source`.
        if self.is_duotone() {
            let view = self.duotone_view_job(source, job)?;
            return self.without_gray().export_stages(&view, request, job);
        }
        let (w, h) = output_dimensions(source.dimensions(), request)?;
        job.check()?;
        if let Some(refusal) = self.gray_export_refusal(request) {
            return Err(refusal);
        }

        // 0. The FLOAT pre-stage (Phase 11F). A 32-bit request writes the
        // extended-range samples, resampled first when a size asks for it
        // (the extended-range resize, RGB clamped to the composite's
        // own visible range; equal dimensions return the same image, so size
        // none stays exact), and leaves the chain here; every other depth
        // first becomes SDR through the document's own tone map, and stages
        // 1-5 below then run on ordinary u16 pixels unchanged.
        let mapped;
        let source = if self.is_float() {
            if request.bit_depth == 32 {
                let resized = precision_sampling::resize_image(
                    source,
                    w,
                    h,
                    request.geometry_filter(),
                    None,
                    job,
                )?;
                return self.export_float(&resized, request, job);
            }
            mapped = self.tone_mapped_u16(source, job)?;
            &mapped
        } else {
            if request.bit_depth == 32 {
                return Err("a 32-bit float export requires a 32-bit float document".into());
            }
            source
        };

        // 1. Resample, at the document's own depth.
        let resampled =
            precision_sampling::resize_image(source, w, h, request.geometry_filter(), None, job)?;
        job.check()?;
        // A Lab Color document leaves Lab here: the resampled composite
        // through Mode ▸ RGB Color's loop into the destination (`lab_rgb`),
        // so the sharpen runs in RGB and step 3 has nothing left to do.
        let lab_destination = if self.is_lab() {
            request.destination_for(self)
        } else {
            None
        };
        let resampled = match &lab_destination {
            Some(to) => {
                let converted = crate::lab_rgb::lab_to_rgb_image(
                    &resampled,
                    to,
                    request.intent,
                    request.black_point_compensation,
                    job,
                )?;
                job.check()?;
                converted
            }
            None => resampled,
        };

        // 2. Sharpen, still at the document's own depth (`export_sharpen`).
        let sharpened = match request.sharpen {
            Some(sharpen) if !sharpen.is_bypass() => sharpen.wide(resampled, job)?,
            _ => resampled,
        };
        job.check()?;
        // A Grayscale document that keeps gray writes one channel from here
        // (`gray_export`).
        if self.keeps_gray(request) {
            return self.gray_export_stages(&sharpened, request, job);
        }

        // 3. Convert, through the same `ColorPipeline` the byte path uses
        // (the 8-bit stage's comment applies verbatim).
        let destination = match &lab_destination {
            Some(to) => Some(to.clone()),
            None => request.profile.destination(),
        };
        let converted = match &destination {
            Some(_) if lab_destination.is_some() => sharpened,
            Some(to) => convert16(&sharpened, to, request, job)?,
            None => sharpened,
        };
        job.check()?;

        let profile = destination.as_deref().unwrap_or(self.profile());
        if let Some(reason) = request.jxl_refusal(profile) {
            return Err(reason);
        }
        let keep_spots = request.keep_spots && self.has_spot_channel();
        let bytes = if request.bit_depth == 16 {
            let format = request.precision_format()?;
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
            if keep_spots {
                let transparency = crate::tiff_spot::transparent16(&converted);
                side.spots = Some(self.tiff_spot_planes(false, transparency));
            }
            if let Some(paths) = self
                .tiff_path_run()
                .filter(|_| request.format == Format::Tiff)
            {
                crate::tiff_paths::attach(&mut side, paths);
            }
            // The SAME injector selection `precision_codec::save` makes: a
            // PNG16 without its pHYs and XMP chunks would silently lose the
            // resolution and the packet.
            let writer = Cursor::new(Vec::new());
            let mut injector = if format == PrecisionFormat::Png {
                MetadataInjector::png(writer, side.png_chunks())
            } else {
                MetadataInjector::passthrough(writer)
            };
            precision_codec::encode(&converted, &mut injector, format, &side, job)?;
            (injector.finish()?.into_inner(), carried, outcome)
        } else {
            // 4. Reduce.
            let reduced = converted.quantize_with_context(request.dither, job)?;
            job.check()?;
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
            if keep_spots {
                let transparency = crate::tiff_spot::transparent8(&reduced);
                side.spots = Some(self.tiff_spot_planes(true, transparency));
            }
            if let Some(paths) = self
                .tiff_path_run()
                .filter(|_| request.format == Format::Tiff)
            {
                crate::tiff_paths::attach(&mut side, paths);
            }
            (encode_bytes(&reduced, request, &side)?, carried, outcome)
        };
        job.report(1.0, 5)?;
        Ok(ExportResult {
            bytes: bounded(bytes.0)?,
            width: w,
            height: h,
            carried: bytes.1,
            bit_depth: request.bit_depth,
            format: request.format_name(),
            profile: request.profile.name(),
            profile_name: profile.name().to_string(),
            intent: request.intent.name(),
            black_point_compensation: request.black_point_compensation,
            metadata: bytes.2,
            layer: None,
            host_intermediate: request.is_host_encoded().then_some("tiff"),
            float: None,
            hdr: None,
            cmyk: false,
        })
    }
}
