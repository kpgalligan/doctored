//! Bounded immutable native channel management and auxiliary-plane admission.
//! Color planes never participate: channel display metadata cannot alter pixels.
//! Auxiliary coverage uses the same scalar bilinear resize as native geometry.
use std::sync::Arc;

use crate::job_control::{DrJobControl, JobContext};
use crate::precision_channel_metadata::provenance;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, pixel_count, BOOKKEEPING_BYTES};
use crate::precision_mask::{PrecisionChannel, PrecisionPlane};
use crate::precision_sampling::{resize_cost, resize_plane, GeometryFilter};
use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChannelEdit {
    RenameChannel {
        index: usize,
        name: String,
    },
    DuplicateChannel {
        index: usize,
    },
    ChannelProperties {
        index: usize,
        name: Option<String>,
        overlay_color: Option<[u8; 3]>,
        overlay_opacity: Option<f32>,
        color_indicates_selected: Option<bool>,
        provenance: Option<String>,
    },
    ImportChannel {
        name: String,
        width: u32,
        height: u32,
        provenance: Option<String>,
    },
}

impl ChannelEdit {
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > 64 * 1024 {
            return Err("typed channel request exceeds64KiB".into());
        }
        serde_json::from_str(text).map_err(|e| format!("invalid typed channel edit: {e}"))
    }
    pub fn expected_samples(&self, _: &PrecisionDocument) -> Result<usize, String> {
        match self {
            Self::ImportChannel { width, height, .. } => pixel_count(*width, *height),
            _ => Ok(0),
        }
    }
    pub fn scratch_bytes(&self, doc: &PrecisionDocument) -> Result<u64, String> {
        let mut scratch = 4 * BOOKKEEPING_BYTES;
        let name = match self {
            Self::RenameChannel { name, .. } | Self::ImportChannel { name, .. } => Some(name),
            Self::ChannelProperties { name, .. } => name.as_ref(),
            _ => None,
        };
        if name.is_some_and(|s| s.len() > crate::drdc::MAX_DRDC_NAME_LEN as usize) {
            return Err("typed channel name exceeds native limit".into());
        }
        match self {
            Self::ImportChannel {
                width,
                height,
                provenance: text,
                ..
            } => {
                let (output, temporary, _) = resize_cost(
                    (*width, *height),
                    doc.dimensions(),
                    GeometryFilter::Bilinear,
                    false,
                )?;
                if let Some(text) = text {
                    provenance(text)?;
                    crate::precision_channel_metadata::source_contract(text, (*width, *height))?;
                }
                scratch += 4 * pixel_count(*width, *height)? as u64 + output + temporary;
            }
            Self::RenameChannel { index, .. }
            | Self::DuplicateChannel { index }
            | Self::ChannelProperties { index, .. } => {
                let old = doc
                    .channels()
                    .get(*index)
                    .ok_or("typed channel index is out of range")?;
                if matches!(self, Self::DuplicateChannel { .. })
                    && old.name().len() + 5 > crate::drdc::MAX_DRDC_NAME_LEN as usize
                {
                    return Err("duplicated channel name exceeds native limit".into());
                }
                if let Self::ChannelProperties {
                    overlay_opacity,
                    provenance: text,
                    ..
                } = self
                {
                    if overlay_opacity.is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
                    {
                        return Err("channel overlay opacity must be finite and within0...1".into());
                    }
                    if let Some(text) = text {
                        provenance(text)?;
                    }
                }
            }
        }
        if matches!(
            self,
            Self::ImportChannel { .. } | Self::DuplicateChannel { .. }
        ) {
            let (r, m) = doc.native_dimensions();
            doc.native_fits_geometry(doc.dimensions(), &r, &m, doc.channels().len() + 1)?;
        }
        Ok(scratch)
    }
}

impl PrecisionDocument {
    pub fn channel_edit(
        &self,
        edit: &ChannelEdit,
        samples: &[u16],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        self.channel_edit_with_context(edit, samples, limit, other, job)
    }
    pub(crate) fn channel_edit_with_context(
        &self,
        edit: &ChannelEdit,
        samples: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        job.check()?;
        let scratch = edit.scratch_bytes(self)?;
        if samples.len() != edit.expected_samples(self)? {
            return Err("typed channel input length differs from its source dimensions".into());
        }
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let mut channels = self.channels.as_ref().clone();
        match edit {
            ChannelEdit::ImportChannel {
                name,
                width,
                height,
                provenance: text,
            } => {
                if let Some(text) = text {
                    if crate::precision_channel_metadata::source_contract(text, (*width, *height))?
                    {
                        for (i, sample) in samples.iter().enumerate() {
                            job.every(i)?;
                            if sample % 257 != 0 {
                                return Err("declared byte auxiliary input must use exact multiplication by257".into());
                            }
                        }
                    }
                }
                let source = PrecisionPlane::copy(*width, *height, samples, job)?;
                let plane = resize_plane(
                    &source,
                    self.width,
                    self.height,
                    GeometryFilter::Bilinear,
                    job,
                )?;
                let mut channel = PrecisionChannel::new(name, plane)?;
                channel.metadata.provenance = text.as_deref().map(provenance).transpose()?;
                channels.push(channel);
            }
            ChannelEdit::DuplicateChannel { index } => {
                let mut channel = channels[*index].duplicate();
                channel.name = Arc::new(format!("{} copy", channel.name()));
                channels.insert(index + 1, channel);
            }
            ChannelEdit::RenameChannel { index, name } => {
                if channels[*index].name() == name {
                    job.report(1.0, 5)?;
                    return Ok(None);
                }
                channels[*index].name = Arc::new(name.clone());
            }
            ChannelEdit::ChannelProperties {
                index,
                name,
                overlay_color,
                overlay_opacity,
                color_indicates_selected,
                provenance: text,
            } => {
                let channel = &mut channels[*index];
                if let Some(name) = name {
                    channel.name = Arc::new(name.clone());
                }
                if let Some(color) = overlay_color {
                    channel.metadata.overlay_color = *color;
                }
                if let Some(opacity) = overlay_opacity {
                    channel.metadata.overlay_opacity = *opacity;
                }
                if let Some(selected) = color_indicates_selected {
                    channel.metadata.color_indicates_selected = *selected;
                }
                if let Some(text) = text {
                    channel.metadata.provenance = Some(provenance(text)?);
                }
                let old = &self.channels[*index];
                if channel.name == old.name && channel.metadata == old.metadata {
                    job.report(1.0, 5)?;
                    return Ok(None);
                }
            }
        }
        let result = self.rebuilt(
            self.width,
            self.height,
            self.layers.as_ref().clone(),
            channels,
        )?;
        job.report(1.0, 5)?;
        Ok(Some(result))
    }
}
