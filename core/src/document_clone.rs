//! Borrowed planning and owned-clone estimates for the byte document model.
//! Pixel/profile/style/mask/source allocations are Arc-shared; record vectors,
//! layer names/descriptions and channel names are copied by DrDocument::clone.
//!
//! `document_clone.rs` supplies byte deep-copy estimates and bounded temporary capacity views;
//! source plans and final edits compare weighted future storage against the original document.
use crate::doc::{DrDocument, Layer};
use crate::doc_channel::Channel;
use crate::job_control::JobContext;
use std::mem::size_of_val;

impl DrDocument {
    /// Three layer-record lengths cover cloning plus insertion reallocation.
    /// Pixel work and fixed handle/cache overhead belong to the calling plan.
    pub(crate) fn cloned_structure_bytes(&self) -> u64 {
        let strings = self
            .layers
            .iter()
            .fold(0_u64, |sum, layer| {
                sum.saturating_add(layer.name.len() as u64)
                    .saturating_add(layer.meta.as_ref().map_or(0, |s| s.len() as u64))
            })
            .saturating_add(
                self.channels
                    .iter()
                    .map(|c| c.name.len() as u64)
                    .sum::<u64>(),
            );
        (3 * size_of_val(self.layers.as_slice())
            + size_of_val(self.channels.as_slice())
            + size_of_val(self.guides.as_slice())) as u64
            + strings
    }
    /// A temporary capacity view, never a rendered or persisted document.
    /// Native dimension/source growth checks do not read name/description
    /// contents. Preserve the meta presence bit so a description-bearing layer
    /// cannot acquire a stale ordinary-source binding in this view.
    pub(crate) fn source_capacity_view(&self) -> Self {
        Self {
            width: self.width,
            height: self.height,
            global_light: self.global_light,
            profile: self.profile.clone(),
            metadata: self.metadata.clone(),
            resolution: self.resolution,
            linear_light: self.linear_light.clone(),
            // A CAPACITY VIEW measures serialized planes; a comp holds no
            // plane, so the view carries none.
            comps: Vec::new(),
            ruler_origin: self.ruler_origin,
            color_samplers: self.color_samplers.clone(),
            paths: self.paths.clone(),
            guides: self.guides.clone(),
            layers: self
                .layers
                .iter()
                .map(|l| Layer {
                    id: l.id,
                    retained_source: l.retained_source.clone(),
                    warp: l.warp.clone(),
                    generated: l.generated.clone(),
                    pixels: l.pixels.clone(),
                    offset: l.offset,
                    name: String::new(),
                    opacity: l.opacity,
                    blend: l.blend,
                    visible: l.visible,
                    mask: l.mask.clone(),
                    mask_enabled: l.mask_enabled,
                    mask_properties: l.mask_properties.clone(),
                    meta: l.meta.as_ref().map(|_| String::new()),
                    clipped: l.clipped,
                    style: l.style.clone(),
                    kind: l.kind,
                    depth: l.depth,
                    locks: l.locks,
                    link: l.link,
                    open: l.open,
                    label: l.label,
                    reference: l.reference,
                })
                .collect(),
            channels: self
                .channels
                .iter()
                .map(|c| Channel {
                    id: c.id,
                    name: String::new(),
                    data: c.data.clone(),
                    overlay_color: c.overlay_color,
                    overlay_opacity: c.overlay_opacity,
                    color_indicates_selected: c.color_indicates_selected,
                    spot: c.spot,
                })
                .collect(),
        }
    }
}

/// Copies only owned record/string storage. Pixel/profile/style/source and
/// document metadata allocations remain Arc-shared exactly as ordinary Clone.
/// Chunked UTF-8 copies bound cancellation even for 16 MiB layer metadata.
impl DrDocument {
    pub(crate) fn clone_with_job(&self, job: JobContext<'_>) -> Result<Self, String> {
        let total = self.cloned_structure_bytes().max(1);
        let mut copied = 0_u64;
        let mut string = |value: &str| -> Result<String, String> {
            let mut result = String::with_capacity(value.len());
            let mut start = 0;
            while start < value.len() {
                job.report(copied as f64 / total as f64, 2)?;
                let mut end = (start + 65_536).min(value.len());
                while !value.is_char_boundary(end) {
                    end -= 1;
                }
                result.push_str(&value[start..end]);
                copied += (end - start) as u64;
                start = end;
            }
            Ok(result)
        };
        let mut layers = Vec::with_capacity(self.layers.len());
        for layer in &self.layers {
            job.check()?;
            layers.push(Layer {
                id: layer.id,
                name: string(&layer.name)?,
                meta: layer.meta.as_deref().map(&mut string).transpose()?,
                retained_source: layer.retained_source.clone(),
                warp: layer.warp.clone(),
                generated: layer.generated.clone(),
                pixels: layer.pixels.clone(),
                offset: layer.offset,
                opacity: layer.opacity,
                blend: layer.blend,
                visible: layer.visible,
                mask: layer.mask.clone(),
                mask_enabled: layer.mask_enabled,
                mask_properties: layer.mask_properties.clone(),
                clipped: layer.clipped,
                style: layer.style.clone(),
                kind: layer.kind,
                depth: layer.depth,
                locks: layer.locks,
                link: layer.link,
                open: layer.open,
                label: layer.label,
                reference: layer.reference,
            });
        }
        let mut channels = Vec::with_capacity(self.channels.len());
        for channel in &self.channels {
            job.check()?;
            channels.push(Channel {
                name: string(&channel.name)?,
                id: channel.id,
                data: channel.data.clone(),
                overlay_color: channel.overlay_color,
                overlay_opacity: channel.overlay_opacity,
                color_indicates_selected: channel.color_indicates_selected,
                spot: channel.spot,
            });
        }
        let mut guides = Vec::with_capacity(self.guides.len());
        for guide in &self.guides {
            job.check()?;
            guides.push(*guide);
        }
        job.report(1.0, 2)?;
        Ok(Self {
            width: self.width,
            height: self.height,
            global_light: self.global_light,
            layers,
            channels,
            guides,
            ruler_origin: self.ruler_origin,
            color_samplers: self.color_samplers.clone(),
            paths: self.paths.clone(),
            profile: self.profile.clone(),
            metadata: self.metadata.clone(),
            resolution: self.resolution,
            linear_light: self.linear_light.clone(),
            comps: self.comps.clone(),
        })
    }
}
