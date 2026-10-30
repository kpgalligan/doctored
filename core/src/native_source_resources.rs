//! One binary pool for RAW/generic resources in v21/v22 (native_sources.rs).
//! The prior advanced RSRC body keeps only pixels/bindings. ATCH owns per-asset
//! descriptors/references; RBIN owns each immutable allocation once. All records
//! are borrowed and preflighted before any pixel decode or payload copy.
//!
//! byte v21 / typed v22 reuse advanced RSRC16/17 pixels/bindings and add per-asset ATCH records
//! plus one deduplicated RBIN pool for RAW and generic resources together. Typed22 also carries
//! ADSC, including its empty form. A byte23/typed24 file (written until 2026-10-10) appends the
//! per-binding LFLT table of live-filter descriptors and RZLM mask references into the same pool.
//! The reader still range-checks every LFLT mask reference and counts it as used, so the pool's
//! every-entry-used check holds, then reports which bindings had a present slot: those are dropped
//! (`native_sources`' module doc), and the blobs only they referenced are never copied. All
//! refs/descriptors/caps preflight before pixel decode; materialization preserves RAW/resource
//! aliases. RAW-only18/19, artwork-only20 and earlier compatible writers remain unchanged.
use super::{append, put_slot, slot, Binding, PendingAsset};
use crate::drdc::{Reader, MAX_DRDC_LAYERS};
use crate::job_control::JobContext;
use crate::native_stream::NativeSink;
use crate::raw_source::{RawDescriptor, RawSource, MAX_RAW_DESCRIPTOR_BYTES};
use crate::source_asset::SourceAsset;
use crate::source_attachment::{
    AttachmentDescriptor, ResourceCapacity, SourceAttachment, SourceBlob, ATTACHMENT_EXPANSION,
    MAX_ATTACHMENT_BYTES, MAX_ATTACHMENT_RESOURCES,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// The bound on one LFLT slot's descriptor (64 KiB) and its mask
/// references (16), as the v23/v24 writer enforced them.
const MAX_FILTER_DESCRIPTOR: usize = 64 * 1024;
const MAX_STORED_FILTERS: usize = 16;

struct Record {
    raw: Option<(Arc<RawSource>, usize)>,
    attachment: Option<(Arc<SourceAttachment>, Vec<usize>)>,
    descriptor_bytes: u64,
}
#[derive(Default)]
pub(super) struct ResourceTable {
    records: Vec<Record>,
    resources: Vec<SourceBlob>,
    indices: HashMap<usize, usize>,
    capacity: ResourceCapacity,
    present: bool,
}
impl ResourceTable {
    fn resource(&mut self, data: &SourceBlob) -> Result<usize, String> {
        let pointer = Arc::as_ptr(data) as usize;
        if let Some(index) = self.indices.get(&pointer) {
            return Ok(*index);
        }
        self.capacity.add(data)?;
        let index = self.resources.len();
        self.indices.insert(pointer, index);
        self.resources.push(data.clone());
        Ok(index)
    }
    pub fn push(&mut self, asset: &Arc<SourceAsset>) -> Result<(), String> {
        if asset.raw.is_some()
            && asset
                .attachment
                .as_ref()
                .is_some_and(|a| a.descriptor.kind == "host_media")
        {
            return Err("host media cannot reinterpret a RAW source".into());
        }
        let mut descriptor_bytes = 0;
        let raw = if let Some(raw) = &asset.raw {
            raw.validate()?;
            descriptor_bytes += raw.descriptor.encoded_len()? as u64;
            Some((raw.clone(), self.resource(&raw.encoded)?))
        } else {
            None
        };
        let attachment = if let Some(attachment) = &asset.attachment {
            attachment.validate()?;
            crate::raw_local_attachment::validate(
                &attachment.descriptor,
                attachment.resources.iter().map(|r| r.as_slice()),
                asset.raw.is_some(),
                asset.dimensions(),
            )?;
            self.present = true;
            descriptor_bytes += attachment.descriptor.encoded_len()? as u64;
            let references = attachment
                .resources
                .iter()
                .map(|r| self.resource(r))
                .collect::<Result<_, _>>()?;
            Some((attachment.clone(), references))
        } else {
            None
        };
        self.records.push(Record {
            raw,
            attachment,
            descriptor_bytes,
        });
        Ok(())
    }
    pub fn present(&self) -> bool {
        self.present
    }
    pub fn header_bytes(&self) -> u64 {
        16 + self.capacity.bytes
            + self.resources.len() as u64 * 8
            + self
                .records
                .iter()
                .map(|r| {
                    2 + r.descriptor_bytes
                        + r.raw.as_ref().map_or(0, |_| 8)
                        + r.attachment
                            .as_ref()
                            .map_or(0, |(_, refs)| 8 + 4 * refs.len() as u64)
                })
                .sum::<u64>()
    }
    pub fn extra_retained_bound(&self) -> u64 {
        self.records
            .iter()
            .map(|r| r.descriptor_bytes * (ATTACHMENT_EXPANSION - 1))
            .sum::<u64>()
    }
    pub fn encode(
        &self,
        out: &mut dyn NativeSink,
        bound: u64,
        job: JobContext<'_>,
    ) -> Result<(), String> {
        append(out, b"ATCH", bound, job)?;
        append(out, &(self.records.len() as u32).to_le_bytes(), bound, job)?;
        for record in &self.records {
            append(out, &[u8::from(record.raw.is_some())], bound, job)?;
            if let Some((raw, reference)) = &record.raw {
                append(out, &(*reference as u32).to_le_bytes(), bound, job)?;
                put_slot(out, raw.descriptor.to_json()?.as_bytes(), bound, job)?;
            }
            append(out, &[u8::from(record.attachment.is_some())], bound, job)?;
            if let Some((attachment, references)) = &record.attachment {
                put_slot(out, attachment.descriptor.to_json()?.as_bytes(), bound, job)?;
                append(out, &(references.len() as u32).to_le_bytes(), bound, job)?;
                for reference in references {
                    append(out, &(*reference as u32).to_le_bytes(), bound, job)?;
                }
            }
        }
        append(out, b"RBIN", bound, job)?;
        append(
            out,
            &(self.resources.len() as u32).to_le_bytes(),
            bound,
            job,
        )?;
        for resource in &self.resources {
            append(out, &(resource.len() as u64).to_le_bytes(), bound, job)?;
            append(out, resource, bound, job)?;
        }
        Ok(())
    }
}

struct PendingRecord<'a> {
    dimensions: (u32, u32),
    raw: Option<(usize, &'a str)>,
    attachment: Option<(&'a str, Vec<usize>)>,
}
type MaterializedAttachments = (Option<Arc<RawSource>>, Option<Arc<SourceAttachment>>);
#[derive(Default)]
pub(super) struct PendingResources<'a> {
    records: Vec<PendingRecord<'a>>,
    resources: Vec<&'a [u8]>,
    /// The pool's byte total, as `ResourceCapacity` summed it.
    pool_bytes: u64,
}
fn text_slot<'a>(reader: &mut Reader<'a>, maximum: usize) -> Result<&'a str, String> {
    std::str::from_utf8(slot(reader, maximum as u32)?)
        .map_err(|_| "source descriptor is not UTF-8".into())
}
fn present(reader: &mut Reader<'_>) -> Result<bool, String> {
    match reader.u8()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err("invalid source attachment presence flag".into()),
    }
}
impl<'a> PendingResources<'a> {
    /// Whether asset `index` carries a RAW record.
    pub fn has_raw(&self, index: usize) -> bool {
        self.records.get(index).is_some_and(|r| r.raw.is_some())
    }
    /// The tables, and per binding whether its v23/v24 LFLT slot was
    /// present (all false without `has_live`). `float_raw_masters` is typed
    /// word 78's claim (`float_native`): only then may a depth-32 asset
    /// carry a RAW record.
    pub fn parse(
        reader: &mut Reader<'a>,
        assets: &[PendingAsset<'_>],
        bindings: &[Binding],
        has_live: bool,
        float_raw_masters: bool,
    ) -> Result<(Self, Vec<bool>), String> {
        if reader.take(4)? != b"ATCH" || reader.u32()? as usize != assets.len() {
            return Err("source attachment table must match asset count".into());
        }
        let mut records = Vec::with_capacity(assets.len());
        for asset in assets {
            let raw = if present(reader)? {
                if asset.depth != 16 && !(asset.depth == 32 && float_raw_masters) {
                    return Err("retained RAW requires genuine u16 master".into());
                }
                Some((
                    reader.u32()? as usize,
                    text_slot(reader, MAX_RAW_DESCRIPTOR_BYTES)?,
                ))
            } else {
                None
            };
            let attachment = if present(reader)? {
                let descriptor = text_slot(reader, MAX_ATTACHMENT_BYTES)?;
                let count = reader.u32()? as usize;
                if !(1..=MAX_ATTACHMENT_RESOURCES).contains(&count) {
                    return Err("source attachment reference count exceeds one to sixteen".into());
                }
                let mut references = Vec::with_capacity(count);
                for _ in 0..count {
                    references.push(reader.u32()? as usize);
                }
                Some((descriptor, references))
            } else {
                None
            };
            records.push(PendingRecord {
                raw,
                attachment,
                dimensions: asset.dimensions,
            });
        }
        if reader.take(4)? != b"RBIN" {
            return Err("missing source binary resource pool".into());
        }
        let count = reader.u32()? as usize;
        let live_bound = if has_live {
            bindings.len() * MAX_STORED_FILTERS
        } else {
            0
        };
        if (!has_live && count == 0)
            || count > assets.len() * (MAX_ATTACHMENT_RESOURCES + 1) + live_bound
            || count
                > MAX_DRDC_LAYERS as usize * (MAX_ATTACHMENT_RESOURCES + 1 + MAX_STORED_FILTERS)
        {
            return Err("invalid source binary resource count".into());
        }
        let mut capacity = ResourceCapacity::default();
        let mut resources = Vec::with_capacity(count);
        for _ in 0..count {
            let length = reader.u64()?;
            capacity.add_length(length)?;
            resources.push(reader.take(length as usize)?);
        }
        let mut used = HashSet::new();
        for record in &records {
            let references = record
                .raw
                .iter()
                .map(|r| r.0)
                .chain(record.attachment.iter().flat_map(|a| a.1.iter().copied()));
            for reference in references {
                if reference >= count {
                    return Err("invalid source binary resource reference".into());
                }
                used.insert(reference);
            }
        }
        let mut filtered = vec![false; bindings.len()];
        if has_live {
            if reader.take(4)? != b"LFLT" || reader.u32()? as usize != bindings.len() {
                return Err("live filter table must match retained binding count".into());
            }
            for flag in &mut filtered {
                if present(reader)? {
                    text_slot(reader, MAX_FILTER_DESCRIPTOR)?;
                    let count = reader.u32()? as usize;
                    if count > MAX_STORED_FILTERS {
                        return Err("live filter mask reference count exceeds its bound".into());
                    }
                    for _ in 0..count {
                        let reference = reader.u32()?;
                        if reference != u32::MAX {
                            let reference = reference as usize;
                            if reference >= resources.len() {
                                return Err("invalid live filter mask reference".into());
                            }
                            used.insert(reference);
                        }
                    }
                    *flag = true;
                }
            }
            if !filtered.contains(&true) {
                return Err("live-filter native version requires a live filter record".into());
            }
        }
        if used.len() != count {
            return Err("unreferenced source binary resource".into());
        }
        let pool_bytes = capacity.bytes;
        Ok((
            Self {
                records,
                resources,
                pool_bytes,
            },
            filtered,
        ))
    }
    /// What word 79 claims (`raw_locals_native::claim`): the pool's byte
    /// total and the largest attachment's reference count.
    pub(super) fn raw_locals_claim(&self) -> (u64, usize) {
        let most = self
            .records
            .iter()
            .filter_map(|r| r.attachment.as_ref().map(|a| a.1.len()))
            .max()
            .unwrap_or(0);
        (self.pool_bytes, most)
    }
    pub fn validate_descriptors(&self) -> Result<(), String> {
        for record in &self.records {
            if let Some((_, text)) = record.raw {
                RawDescriptor::parse(text)?;
            }
            if let Some((text, references)) = &record.attachment {
                let descriptor = AttachmentDescriptor::parse(text)?;
                if record.raw.is_some() && descriptor.kind == "host_media" {
                    return Err("host media cannot reinterpret a RAW source".into());
                }
                if descriptor.resources.len() != references.len()
                    || descriptor
                        .resources
                        .iter()
                        .zip(references)
                        .any(|(info, reference)| {
                            info.byte_length != self.resources[*reference].len() as u64
                        })
                {
                    return Err(
                        "source attachment manifest disagrees with binary references".into(),
                    );
                }
                crate::raw_local_attachment::validate(
                    &descriptor,
                    references.iter().map(|r| self.resources[*r]),
                    record.raw.is_some(),
                    record.dimensions,
                )?;
            }
        }
        Ok(())
    }
    pub fn retained_bound(&self) -> u64 {
        self.resources.iter().map(|r| r.len() as u64).sum::<u64>()
            + self
                .records
                .iter()
                .map(|r| {
                    r.raw.as_ref().map_or(0, |(_, s)| s.len() as u64)
                        + r.attachment.as_ref().map_or(0, |(s, _)| s.len() as u64)
                })
                .sum::<u64>()
                * ATTACHMENT_EXPANSION
    }
    /// Copies each pool entry an ATCH record references, once; an entry
    /// only a skipped LFLT slot referenced is left behind.
    pub fn materialize(&self, job: JobContext<'_>) -> Result<Vec<MaterializedAttachments>, String> {
        let mut referenced = vec![false; self.resources.len()];
        for record in &self.records {
            for index in record
                .raw
                .iter()
                .map(|r| r.0)
                .chain(record.attachment.iter().flat_map(|a| a.1.iter().copied()))
            {
                referenced[index] = true;
            }
        }
        let resources = self
            .resources
            .iter()
            .zip(referenced)
            .map(|(resource, referenced)| {
                if !referenced {
                    return Ok(None);
                }
                let mut bytes = crate::precision_image::reserved_vec(resource.len())?;
                for chunk in resource.chunks(64 * 1024) {
                    job.check()?;
                    bytes.extend_from_slice(chunk);
                }
                Ok(Some(Arc::new(bytes)))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let blob = |index: usize| -> Result<SourceBlob, String> {
            resources[index]
                .clone()
                .ok_or_else(|| "invalid source binary resource reference".into())
        };
        let attachments = self
            .records
            .iter()
            .map(|record| {
                job.check()?;
                let raw = record
                    .raw
                    .map(|(index, text)| {
                        Ok::<_, String>(Arc::new(RawSource {
                            encoded: blob(index)?,
                            descriptor: RawDescriptor::parse(text)?,
                        }))
                    })
                    .transpose()?;
                let attachment = record
                    .attachment
                    .as_ref()
                    .map(|(text, indices)| {
                        Ok::<_, String>(Arc::new(SourceAttachment {
                            descriptor: AttachmentDescriptor::parse(text)?,
                            resources: indices
                                .iter()
                                .map(|i| blob(*i))
                                .collect::<Result<_, _>>()?,
                        }))
                    })
                    .transpose()?;
                Ok((raw, attachment))
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(attachments)
    }
}
