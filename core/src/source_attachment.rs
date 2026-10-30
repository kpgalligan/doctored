//! Immutable binary source resources and a bounded opaque host-state envelope.
//! Resources are never images by implication: the host owns codec interpretation.
//! Shared Arc allocations survive frame revisions and count once across owners.
//! SHA256 values are host-verified claims: core checks syntax and byte lengths,
//! while the host must rehash before selecting a new image/frame. Cached native
//! masters load without consulting encoded resources or any external codec.
//!
//! Generic resource bytes and RAW originals share one 1 GiB aggregate and allocation inventory
//! (`raw_local_limits::MAX_SOURCE_RESOURCE_TOTAL_BYTES`, formerly 512 MiB; RAW originals alone
//! keep 512 MiB); per-resource limit is 256 MiB, descriptor64 KiB and state48 KiB, at most 16
//! resources per attachment (formerly 4; more needs DRDC 79, `raw_locals_native`). SHA256 is host-verified
//! metadata, matching RAW: core validates syntax, lengths and references; hosts must hash before
//! re-decode. Unknown attachment kinds/state remain opaque and cannot authorize decoding. Presence
//! is allocation-free and independent of admitted full inspection; original-master ICC is borrowed
//! separately from fallback profile. Resource copies stage before caller-buffer publication and
//! support cancellation. Fresh attachment revisions are exclusive; frame no-ops belong before
//! replacement.
use crate::raw_local_limits::{MAX_RAW_LOCALS, MAX_SOURCE_RESOURCE_TOTAL_BYTES};
use crate::raw_source::{serialized_length, MAX_RAW_BYTES};
use crate::resources::Inventory;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;

pub const MAX_ATTACHMENT_BYTES: usize = 64 * 1024;
pub const MAX_ATTACHMENT_RESOURCES: usize = MAX_RAW_LOCALS;
pub const ATTACHMENT_EXPANSION: u64 = crate::raw_source::RAW_DESCRIPTOR_EXPANSION;
pub type SourceBlob = Arc<Vec<u8>>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentResource {
    pub id: String,
    pub media_type: String,
    pub sha256: String,
    pub byte_length: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentDescriptor {
    pub schema_version: u32,
    pub kind: String,
    pub state: Value,
    pub resources: Vec<AttachmentResource>,
}
impl AttachmentDescriptor {
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > MAX_ATTACHMENT_BYTES {
            return Err("source attachment descriptor exceeds 64 KiB".into());
        }
        let value: Self = serde_json::from_str(text)
            .map_err(|e| format!("invalid source attachment descriptor: {e}"))?;
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<(), String> {
        let token = |s: &str| {
            !s.is_empty()
                && s.len() <= 64
                && s.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
        };
        if self.schema_version != 1 || !token(&self.kind) {
            return Err("unsupported source attachment envelope version or kind".into());
        }
        if self.state.as_object().is_none_or(|o| o.is_empty())
            || serialized_length(&self.state)? > 48 * 1024
        {
            return Err("source attachment state requires a nonempty object up to 48 KiB".into());
        }
        if !(1..=MAX_ATTACHMENT_RESOURCES).contains(&self.resources.len()) {
            return Err("source attachment requires one to sixteen binary resources".into());
        }
        let mut ids = HashSet::new();
        for resource in &self.resources {
            if !token(&resource.id)
                || !ids.insert(&resource.id)
                || resource.media_type.is_empty()
                || resource.media_type.len() > 1024
                || resource.media_type.contains('\0')
            {
                return Err(
                    "source attachment resource identifiers must be unique and bounded".into(),
                );
            }
            crate::source_asset::SourceLink::validate_parts("/resource", &resource.sha256)?;
            if resource.byte_length == 0 || resource.byte_length > MAX_RAW_BYTES {
                return Err("source resource requires 1 byte to 256 MiB".into());
            }
        }
        if self.encoded_len()? > MAX_ATTACHMENT_BYTES {
            return Err("source attachment descriptor exceeds 64 KiB".into());
        }
        Ok(())
    }
    pub fn encoded_len(&self) -> Result<usize, String> {
        serialized_length(self)
    }
    pub fn to_json(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string(self).map_err(|e| e.to_string())
    }
}

#[derive(Clone)]
pub struct SourceAttachment {
    pub descriptor: AttachmentDescriptor,
    pub resources: Vec<SourceBlob>,
}
impl SourceAttachment {
    pub fn validate(&self) -> Result<(), String> {
        self.descriptor.validate()?;
        if self.resources.len() != self.descriptor.resources.len()
            || self
                .resources
                .iter()
                .zip(&self.descriptor.resources)
                .any(|(bytes, info)| bytes.len() as u64 != info.byte_length)
        {
            return Err(
                "source attachment resource count or byte length disagrees with descriptor".into(),
            );
        }
        Ok(())
    }
    pub(crate) fn record_resources(&self, inventory: &mut Inventory) {
        inventory.object(self, "source_attachment_structure", false);
        inventory.vec(&self.resources, "source_attachment_structure", false);
        inventory.string(&self.descriptor.kind, "source_attachment_descriptor", false);
        inventory.vec(
            &self.descriptor.resources,
            "source_attachment_descriptor",
            false,
        );
        for resource in &self.descriptor.resources {
            for text in [&resource.id, &resource.media_type, &resource.sha256] {
                inventory.string(text, "source_attachment_descriptor", false);
            }
        }
        crate::raw_source::record_value_as(
            &self.descriptor.state,
            inventory,
            "source_attachment_descriptor",
        );
        for resource in &self.resources {
            inventory.object(&**resource, "source_attachment_structure", false);
            inventory.vec(resource, "source_resource_encoded", false);
        }
    }
}

/// The aggregate limit spans RAW originals and generic attachments together
/// (1 GiB). Public so that a test can reach the aggregate refusal
/// without a gigabyte allocation.
#[derive(Default)]
pub struct ResourceCapacity {
    pointers: HashSet<usize>,
    pub(crate) bytes: u64,
}
impl ResourceCapacity {
    pub(crate) fn add(&mut self, resource: &SourceBlob) -> Result<(), String> {
        if self.pointers.insert(Arc::as_ptr(resource) as usize) {
            self.add_length(resource.len() as u64)?;
        }
        Ok(())
    }
    pub fn add_length(&mut self, length: u64) -> Result<(), String> {
        if length == 0 || length > MAX_RAW_BYTES {
            return Err("source binary resource exceeds its nonempty 256 MiB limit".into());
        }
        self.bytes = self
            .bytes
            .checked_add(length)
            .ok_or("source resource byte overflow")?;
        if self.bytes > MAX_SOURCE_RESOURCE_TOTAL_BYTES {
            return Err(
                "retained RAW and source resources exceed the 1 GiB aggregate limit".into(),
            );
        }
        Ok(())
    }
    pub(crate) fn asset(&mut self, asset: &crate::source_asset::SourceAsset) -> Result<(), String> {
        if let Some(raw) = &asset.raw {
            self.add(&raw.encoded)?;
        }
        if let Some(attachment) = &asset.attachment {
            attachment.validate()?;
            crate::raw_local_attachment::validate(
                &attachment.descriptor,
                attachment.resources.iter().map(|r| r.as_slice()),
                asset.raw.is_some(),
                asset.dimensions(),
            )?;
            if asset.raw.is_some() && attachment.descriptor.kind == "host_media" {
                return Err("host media cannot replace the interpretation of a RAW source".into());
            }
            for resource in &attachment.resources {
                self.add(resource)?;
            }
        }
        Ok(())
    }
}
