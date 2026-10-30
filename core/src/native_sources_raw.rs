//! Opaque original RAW payloads, independently shared across developed revisions.
//!
//! The native reader never invokes a camera decoder. It preflights every RAW
//! descriptor, reference and encoded length, then copies the original bytes only
//! after document admission. Rendering uses the separately saved RGBA16 master
//! and layer fallback. The descriptor captures effective host development state;
//! reading it does not grant permission to reinterpret any saved pixels.
//!
//! `native_sources_raw.rs` adds deduplicated original payloads in byte v18 / typed v19; earlier
//! compatible writers/readers stay available.
use super::{append, put_slot, slot};
use crate::drdc::{Reader, MAX_DRDC_LAYERS};
use crate::job_control::JobContext;
use crate::native_stream::NativeSink;
use crate::raw_source::{
    RawDescriptor, RawSource, MAX_RAW_BYTES, MAX_RAW_DESCRIPTOR_BYTES, MAX_RAW_TOTAL_BYTES,
    RAW_DESCRIPTOR_EXPANSION,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

fn payload_capacity(length: u64, total: &mut u64) -> Result<(), String> {
    if length == 0 || length > MAX_RAW_BYTES {
        return Err("retained RAW payload exceeds its nonempty 256 MiB limit".into());
    }
    *total = total
        .checked_add(length)
        .ok_or("retained RAW payload byte overflow")?;
    if *total > MAX_RAW_TOTAL_BYTES {
        return Err("retained RAW payload aggregate exceeds 512 MiB".into());
    }
    Ok(())
}

struct RawRecord {
    payload: usize,
    raw: Arc<RawSource>,
    descriptor_bytes: usize,
}
#[derive(Default)]
pub(super) struct RawTable {
    records: Vec<Option<RawRecord>>,
    payloads: Vec<Arc<Vec<u8>>>,
    pointers: HashMap<usize, usize>,
    payload_bytes: u64,
}
impl RawTable {
    pub fn push(&mut self, raw: Option<&Arc<RawSource>>) -> Result<(), String> {
        let record = if let Some(raw) = raw {
            raw.validate()?;
            let pointer = Arc::as_ptr(&raw.encoded) as usize;
            let next = self.payloads.len();
            let payload = *self.pointers.entry(pointer).or_insert(next);
            if payload == next {
                payload_capacity(raw.encoded.len() as u64, &mut self.payload_bytes)?;
                self.payloads.push(raw.encoded.clone());
            }
            Some(RawRecord {
                payload,
                raw: raw.clone(),
                descriptor_bytes: raw.descriptor.encoded_len()?,
            })
        } else {
            None
        };
        self.records.push(record);
        Ok(())
    }
    pub fn present(&self) -> bool {
        !self.payloads.is_empty()
    }
    /// Includes original encoded bytes: they are stored verbatim, never PNG.
    pub fn header_bytes(&self) -> u64 {
        if !self.present() {
            return 0;
        }
        self.records.len() as u64
            + self
                .records
                .iter()
                .flatten()
                .map(|r| 8 + r.descriptor_bytes as u64)
                .sum::<u64>()
            + 8
            + 8 * self.payloads.len() as u64
            + self.payload_bytes
    }
    pub fn extra_retained_bound(&self) -> u64 {
        // The native header estimate already includes one descriptor copy.
        self.records
            .iter()
            .flatten()
            .map(|r| (RAW_DESCRIPTOR_EXPANSION - 1) * r.descriptor_bytes as u64)
            .sum()
    }
    pub fn encode_record(
        &self,
        index: usize,
        out: &mut dyn NativeSink,
        bound: u64,
        job: JobContext<'_>,
    ) -> Result<(), String> {
        if !self.present() {
            return Ok(());
        }
        let record = &self.records[index];
        append(out, &[u8::from(record.is_some())], bound, job)?;
        if let Some(record) = record {
            append(out, &(record.payload as u32).to_le_bytes(), bound, job)?;
            // Only the admitted writer creates one bounded descriptor copy.
            let descriptor = record.raw.descriptor.to_json()?;
            put_slot(out, descriptor.as_bytes(), bound, job)?;
        }
        Ok(())
    }
    pub fn encode_payloads(
        &self,
        out: &mut dyn NativeSink,
        bound: u64,
        job: JobContext<'_>,
    ) -> Result<(), String> {
        if !self.present() {
            return Ok(());
        }
        append(out, b"RRAW", bound, job)?;
        append(out, &(self.payloads.len() as u32).to_le_bytes(), bound, job)?;
        for payload in &self.payloads {
            append(out, &(payload.len() as u64).to_le_bytes(), bound, job)?;
            append(out, payload, bound, job)?;
        }
        Ok(())
    }
}

struct PendingRawRecord<'a> {
    payload: usize,
    descriptor: &'a str,
}
#[derive(Default)]
pub(super) struct PendingRawTable<'a> {
    records: Vec<Option<PendingRawRecord<'a>>>,
    payloads: Vec<&'a [u8]>,
}
impl<'a> PendingRawTable<'a> {
    /// Whether asset `index` carries a RAW record.
    pub fn has(&self, index: usize) -> bool {
        self.records.get(index).is_some_and(Option::is_some)
    }
    /// `float_masters` is typed word 78's claim (`float_native`): only then
    /// may a depth-32 asset carry a RAW record.
    pub fn parse_record(
        &mut self,
        reader: &mut Reader<'a>,
        depth: u8,
        float_masters: bool,
    ) -> Result<(), String> {
        let record = match reader.u8()? {
            0 => None,
            1 => {
                if depth != 16 && !(depth == 32 && float_masters) {
                    return Err("retained RAW requires a genuine RGBA16 developed master".into());
                }
                let payload = reader.u32()? as usize;
                let descriptor =
                    std::str::from_utf8(slot(reader, MAX_RAW_DESCRIPTOR_BYTES as u32)?)
                        .map_err(|_| "retained RAW descriptor is not UTF-8")?;
                Some(PendingRawRecord {
                    payload,
                    descriptor,
                })
            }
            _ => return Err("invalid retained RAW presence flag".into()),
        };
        self.records.push(record);
        Ok(())
    }
    pub fn parse_payloads(&mut self, reader: &mut Reader<'a>) -> Result<(), String> {
        if reader.take(4)? != b"RRAW" {
            return Err("missing retained RAW payload table".into());
        }
        let count = reader.u32()? as usize;
        if count == 0 || count > self.records.len() || count > MAX_DRDC_LAYERS as usize {
            return Err("invalid retained RAW payload count".into());
        }
        let mut total = 0;
        for _ in 0..count {
            let length = reader.u64()?;
            payload_capacity(length, &mut total)?;
            self.payloads.push(reader.take(length as usize)?);
        }
        let mut used = HashSet::new();
        for record in self.records.iter().flatten() {
            if record.payload >= count {
                return Err("invalid retained RAW payload reference".into());
            }
            used.insert(record.payload);
        }
        if used.len() != count {
            return Err("unreferenced retained RAW payload".into());
        }
        Ok(())
    }
    pub fn validate_descriptors(&self) -> Result<(), String> {
        for record in self.records.iter().flatten() {
            // One bounded descriptor at a time, dropped before validating the
            // next. The typed reader calls this after whole-document admission,
            // which includes the shared recipe expansion bound, before pixels.
            RawDescriptor::parse(record.descriptor)?;
        }
        Ok(())
    }
    pub fn retained_bound(&self) -> u64 {
        self.payloads.iter().map(|p| p.len() as u64).sum::<u64>()
            + self
                .records
                .iter()
                .flatten()
                .map(|r| RAW_DESCRIPTOR_EXPANSION * r.descriptor.len() as u64)
                .sum::<u64>()
    }
    pub fn materialize(&self, job: JobContext<'_>) -> Result<Vec<Option<Arc<RawSource>>>, String> {
        let payloads = self
            .payloads
            .iter()
            .map(|payload| {
                let mut owned = Vec::new();
                owned
                    .try_reserve_exact(payload.len())
                    .map_err(|_| "could not allocate retained RAW payload")?;
                for chunk in payload.chunks(64 * 1024) {
                    job.check()?;
                    owned.extend_from_slice(chunk);
                }
                Ok(Arc::new(owned))
            })
            .collect::<Result<Vec<_>, String>>()?;
        self.records
            .iter()
            .map(|record| {
                job.check()?;
                record
                    .as_ref()
                    .map(|record| {
                        Ok(Arc::new(RawSource {
                            encoded: payloads[record.payload].clone(),
                            descriptor: RawDescriptor::parse(record.descriptor)?,
                        }))
                    })
                    .transpose()
            })
            .collect()
    }
}
