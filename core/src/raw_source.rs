//! Retained encoded camera input and effective, versioned development settings.
//! The core never decodes camera data or interprets host recipe controls. Unknown
//! decoder identities remain readable so saved master/fallback pixels survive.
//!
//! The host owns recipe semantics and RAW decoding; the core performs no camera/file
//! interpretation. Initial attachment copies after admission; redevelopment shares original bytes
//! from the captured prior asset. Raw payloads and descriptor allocations join the mixed resource
//! inventory. Every record/ref/cap is checked before pixel decode; saved masters/fallbacks require
//! no RAW decoder.
use crate::resources::Inventory;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, Write};
use std::sync::Arc;

pub const MAX_RAW_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_RAW_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_RAW_DESCRIPTOR_BYTES: usize = 64 * 1024;
// A one-entry serde B-tree object allocates capacity for eleven pairs. Deep
// one-key objects therefore expand far more than a dense serialized recipe;
// 256 times JSON bytes bounds those sparse nodes, strings and temporary values.
pub const RAW_DESCRIPTOR_EXPANSION: u64 = 256;

/// Measure serialized state without allocating a JSON copy. Planners and
/// inspection share this counter; the writer creates bytes only after admission.
pub(crate) fn serialized_length(value: &impl Serialize) -> Result<usize, String> {
    struct Counter(usize);
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .ok_or_else(|| io::Error::other("RAW descriptor length overflow"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value).map_err(|e| e.to_string())?;
    Ok(counter.0)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawDecoder {
    pub identifier: String,
    pub version: String,
    pub implementation: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawDescriptor {
    pub schema_version: u32,
    pub original_filename: String,
    pub content_sha256: String,
    pub decoder: RawDecoder,
    pub recipe: Value,
}
impl RawDescriptor {
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > MAX_RAW_DESCRIPTOR_BYTES {
            return Err("RAW descriptor exceeds 64 KiB".into());
        }
        let out: Self =
            serde_json::from_str(text).map_err(|e| format!("invalid RAW descriptor: {e}"))?;
        out.validate()?;
        Ok(out)
    }
    pub fn validate(&self) -> Result<(), String> {
        let bounded = |value: &str, maximum: usize| {
            !value.is_empty() && value.len() <= maximum && !value.contains('\0')
        };
        if self.schema_version != 1 {
            return Err("unsupported RAW descriptor schema version".into());
        }
        if !bounded(&self.original_filename, 1024)
            || self.original_filename.contains('/')
            || self.original_filename == "."
            || self.original_filename == ".."
        {
            return Err("RAW original filename must be one bounded filename".into());
        }
        crate::source_asset::SourceLink::validate_parts("/raw", &self.content_sha256)?;
        if !bounded(&self.decoder.identifier, 1024)
            || !bounded(&self.decoder.version, 1024)
            || !bounded(&self.decoder.implementation, 4096)
        {
            return Err(
                "RAW decoder identity, version and implementation must be bounded nonempty strings"
                    .into(),
            );
        }
        if self.recipe.as_object().is_none_or(|value| value.is_empty())
            || serialized_length(&self.recipe)? > 48 * 1024
        {
            return Err("RAW effective recipe requires a nonempty object of at most 48 KiB".into());
        }
        if self.encoded_len()? > MAX_RAW_DESCRIPTOR_BYTES {
            return Err("RAW descriptor exceeds 64 KiB".into());
        }
        Ok(())
    }
    pub fn json(&self) -> Value {
        json!(self)
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
pub struct RawSource {
    pub encoded: Arc<Vec<u8>>,
    pub descriptor: RawDescriptor,
}
impl RawSource {
    pub fn validate(&self) -> Result<(), String> {
        validate_length(self.encoded.len())?;
        self.descriptor.validate()
    }
    pub fn info(&self) -> Value {
        let mut out = self.descriptor.json();
        out["encoded_bytes"] = json!(self.encoded.len());
        out
    }
    pub(crate) fn record_resources(&self, inventory: &mut Inventory) {
        inventory.object(self, "raw_source_structure", false);
        inventory.object(&*self.encoded, "raw_source_structure", false);
        inventory.vec(&self.encoded, "raw_source_encoded", false);
        for text in [
            &self.descriptor.original_filename,
            &self.descriptor.content_sha256,
            &self.descriptor.decoder.identifier,
            &self.descriptor.decoder.version,
            &self.descriptor.decoder.implementation,
        ] {
            inventory.string(text, "raw_source_descriptor", false);
        }
        record_value_as(&self.descriptor.recipe, inventory, "raw_source_descriptor");
    }
}
pub(crate) fn record_value_as(value: &Value, inventory: &mut Inventory, category: &'static str) {
    match value {
        Value::String(text) => inventory.string(text, category, false),
        Value::Array(items) => {
            inventory.vec(items, category, false);
            for item in items {
                record_value_as(item, inventory, category);
            }
        }
        Value::Object(items) => {
            // serde_json's map owns B-tree nodes. Their allocation addresses are
            // private; charge a conservative node share at each resident Value
            // address, stable across shared RawSource owners, without exposing it.
            for (key, item) in items {
                inventory.record(item as *const Value as usize, 1024, category, false);
                inventory.string(key, category, false);
                record_value_as(item, inventory, category);
            }
        }
        _ => {}
    }
}
pub fn validate_length(length: usize) -> Result<(), String> {
    if length == 0 || length as u64 > MAX_RAW_BYTES {
        return Err("RAW original must contain 1 byte to 256 MiB".into());
    }
    Ok(())
}
