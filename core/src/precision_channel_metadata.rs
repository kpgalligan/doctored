//! Native channel display settings and declared auxiliary-source precision.
//! Version25 wraps one unchanged typed body with its prior version marker, then
//! appends CHPR, u32 channel count, and per-channel RGB bytes, f32 opacity, u8
//! selected-side flag, u32 provenance length and UTF-8 JSON (zero = absent).
//! The body is borrowed in place: no duplicate encoded image allocation. A
//! default-only document keeps its prior writer, and every prior reader remains.
//! A spot channel's ink (`spot`) is not CHPR's: `spot_channel_native` writes it.
use std::sync::Arc;

use crate::drdc::Reader;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_native::append;
use serde::Deserialize;

pub const NATIVE_VERSION: u32 = 25;
pub const MAX_PROVENANCE_BYTES: usize = 32 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct ChannelMetadata {
    pub overlay_color: [u8; 3],
    pub overlay_opacity: f32,
    pub color_indicates_selected: bool,
    pub provenance: Option<Arc<String>>,
    /// The ink of a SPOT channel (`spot_channel`); persisted by
    /// `spot_channel_native`'s `SPOT` table, never by CHPR.
    pub spot: Option<crate::spot_channel::SpotInk>,
}
impl Default for ChannelMetadata {
    fn default() -> Self {
        Self {
            overlay_color: crate::doc_channel::DEFAULT_OVERLAY_COLOR,
            overlay_opacity: crate::doc_channel::DEFAULT_OVERLAY_OPACITY,
            color_indicates_selected: false,
            provenance: None,
            spot: None,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuxiliaryProvenance {
    version: u32,
    kind: String,
    name: String,
    source_format: String,
    source_bits: u32,
    precision_basis: String,
    encoded_precision: String,
    source_width: u32,
    source_height: u32,
    exif_orientation: u32,
    orientation_applied: bool,
    coverage_mapping: String,
    alignment: String,
    finite_range: Option<[f64; 2]>,
}

pub(crate) fn provenance(text: &str) -> Result<Arc<String>, String> {
    if text.len() > MAX_PROVENANCE_BYTES {
        return Err("auxiliary channel provenance exceeds32KiB".into());
    }
    let value: AuxiliaryProvenance = serde_json::from_str(text)
        .map_err(|e| format!("invalid auxiliary channel provenance: {e}"))?;
    let bits = match value.source_format.as_str() {
        "one_component8" => 8,
        "one_component16" | "float16" | "disparity_float16" | "depth_float16" => 16,
        "float32" | "disparity_float32" | "depth_float32" => 32,
        _ => return Err("unsupported auxiliary source coverage format".into()),
    };
    let depth =
        value.source_format.starts_with("depth_") || value.source_format.starts_with("disparity_");
    if value.version != 1
        || value.kind != "camera_auxiliary"
        || value.name.len() > crate::drdc::MAX_DRDC_NAME_LEN as usize
        || value.source_bits != bits
        || value.precision_basis != "decoded_auxiliary_buffer"
        || value.encoded_precision != "not_inspected"
        || !(1..=8).contains(&value.exif_orientation)
        || !value.orientation_applied
        || value.alignment != "oriented_full_image_bilinear"
        || value.coverage_mapping
            != if depth {
                "finite_scene_range_near_white"
            } else {
                "identity_unorm"
            }
        || value
            .finite_range
            .is_some_and(|r| !r[0].is_finite() || !r[1].is_finite() || r[0] > r[1])
    {
        return Err("invalid auxiliary coverage precision or alignment contract".into());
    }
    crate::precision_image::pixel_count(value.source_width, value.source_height)?;
    // Canonical sorted JSON makes equivalent caller field ordering a no-op.
    let canonical: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let canonical = canonical.to_string();
    if canonical.len() > MAX_PROVENANCE_BYTES {
        return Err("canonical auxiliary provenance exceeds32KiB".into());
    }
    Ok(Arc::new(canonical))
}

impl ChannelMetadata {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !self.overlay_opacity.is_finite() || !(0.0..=1.0).contains(&self.overlay_opacity) {
            return Err("channel overlay opacity must be finite and within0...1".into());
        }
        if let Some(text) = &self.provenance {
            provenance(text)?;
        }
        Ok(())
    }
    pub(crate) fn bytes(&self) -> u64 {
        12 + self.provenance.as_ref().map_or(0, |s| s.len() as u64)
    }
}

pub(crate) fn source_contract(text: &str, dimensions: (u32, u32)) -> Result<bool, String> {
    let value: AuxiliaryProvenance = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if (value.source_width, value.source_height) != dimensions {
        return Err("auxiliary provenance dimensions differ from oriented input coverage".into());
    }
    Ok(value.source_format == "one_component8")
}

pub(crate) fn present(document: &PrecisionDocument) -> bool {
    // `spot` is SPOT's, not CHPR's: a channel differing only in its ink
    // writes no CHPR.
    document.channels().iter().any(|c| {
        let metadata = ChannelMetadata {
            spot: None,
            ..c.metadata().clone()
        };
        metadata != ChannelMetadata::default()
    })
}

pub(crate) fn header_bytes(document: &PrecisionDocument) -> u64 {
    if present(document) {
        12 + document
            .channels()
            .iter()
            .map(|c| c.metadata().bytes())
            .sum::<u64>()
    } else {
        0
    }
}

pub(crate) fn encode(
    document: &PrecisionDocument,
    out: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    if !present(document) {
        return Ok(());
    }
    append(out, b"CHPR", bound, job)?;
    append(
        out,
        &(document.channels().len() as u32).to_le_bytes(),
        bound,
        job,
    )?;
    for channel in document.channels() {
        let metadata = channel.metadata();
        metadata.validate()?;
        append(out, &metadata.overlay_color, bound, job)?;
        append(out, &metadata.overlay_opacity.to_le_bytes(), bound, job)?;
        append(
            out,
            &[u8::from(metadata.color_indicates_selected)],
            bound,
            job,
        )?;
        let text = metadata.provenance.as_ref().map_or("", |s| s.as_str());
        append(out, &(text.len() as u32).to_le_bytes(), bound, job)?;
        append(out, text.as_bytes(), bound, job)?;
    }
    Ok(())
}

pub(crate) fn parse<'a>(
    reader: &mut Reader<'a>,
    count: usize,
    job: JobContext<'_>,
) -> Result<Vec<(ChannelMetadata, &'a str)>, String> {
    if reader.take(4)? != b"CHPR" || reader.u32()? as usize != count {
        return Err("native channel metadata count differs from channel records".into());
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        job.check()?;
        let color = reader.take(3)?;
        let overlay_color = [color[0], color[1], color[2]];
        let overlay_opacity = reader.f32()?;
        let color_indicates_selected = match reader.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid native channel overlay flag".into()),
        };
        let length = reader.u32()? as usize;
        if length > MAX_PROVENANCE_BYTES {
            return Err("native channel provenance exceeds32KiB".into());
        }
        let text = std::str::from_utf8(reader.take(length)?)
            .map_err(|_| "native channel provenance is not UTF-8")?;
        // Keep text borrowed until complete native memory admission. Parsing
        // transiently uses at most one bounded descriptor, not their aggregate.
        if !text.is_empty() {
            provenance(text)?;
        }
        let metadata = ChannelMetadata {
            overlay_color,
            overlay_opacity,
            color_indicates_selected,
            provenance: None,
            spot: None,
        };
        metadata.validate()?;
        out.push((metadata, text));
    }
    Ok(out)
}
