//! Native immutable source assets and per-layer dependency records.
//!
//! Byte v14 / typed v15 use `RSRC`, u32 asset count, then each asset's
//! u32 ASCII identity length/identity, u32 ICC length/bytes, u32 width/height,
//! u64 PNG length/PNG. Original depth equals document depth. Each binding is
//! u32 postorder layer index, u32 asset index, u8 filter (nearest/bilinear/
//! bicubic/Lanczos3=0/1/2/3), three zero reserved bytes and six f64 affine
//! coefficients. Old assets migrate with revision_id=identity and no links.
//!
//! Byte v16 / typed v17 insert an ASCII revision slot and four bytes
//! (actual original depth 8/16, three reserved zeros) after each asset identity.
//! After each binding's affine they append u8 linked 0/1; a linked binding adds
//! a u32 UTF-8 absolute-path slot (at most 16 KiB, no NUL) and 64 lowercase ASCII
//! hex SHA256 bytes. Paths/fingerprints describe the last explicitly accepted
//! dependency; the core never reads them. Cached original PNGs and usable
//! fallback records are always embedded, even for linked dependencies.
//!
//! Shared assets deduplicate by immutable allocation identity; distinct content
//! revisions may share a stable source identity. Duplicate(identity,revision)
//! asset records and duplicate binding targets refuse. All profiles, original
//! depths, references, dependency fields and capacities preflight before pixels
//! decode. Source-free byte/typed files keep v10/v13; legacy-compatible retained
//! assets may continue writing v14/v15 without losing any model state.
//!
//! Byte v18 / typed v19 retain v16/v17 framing and append after each asset PNG
//! a u8 RAW-present flag. A RAW asset adds u32 encoded-payload index and a u32
//! UTF-8 descriptor slot (64 KiB maximum), containing versioned effective recipe
//! and decoder identity. Its developed master must be genuine RGBA16. After the
//! bindings comes `RRAW`, u32 payload count, then u64 byte length and untouched
//! original encoded RAW bytes per payload (256 MiB each, 512 MiB aggregate).
//! Payload allocation identity deduplicates originals across recipe revisions.
//! No decoder is consulted on reopen: both developed master and fallback retain
//! their saved samples even when the original camera decoder is unavailable.
//!
//! Byte v21 / typed v22 begin the source tail with u32 base version16/17,
//! followed by the unchanged advanced RSRC pixel/binding body (without RRAW).
//! `ATCH`, u32 asset count, then exactly one positional record per asset:
//! u8 RAW presence; when present u32 RBIN reference + u32 JSON length/RAW JSON;
//! u8 generic presence; when present u32 JSON length/envelope + u32 reference
//! count + ordered u32 RBIN references. `RBIN`, u32 blob count, then repeated
//! u64 length/verbatim bytes completes the source tail. One pool deduplicates
//! RAW and generic resource allocations together. Every reference is in range,
//! every pool entry is used, manifest lengths match, and the shared aggregate
//! is 1 GiB (formerly 512 MiB; 256MiB/blob). An attachment holds one to 16
//! references (formerly four); a pool over 512 MiB or an attachment over
//! four needs word 79 (`raw_locals_native`). Generic resourceIDs are unique;
//! references may alias.
//! SHA256 is host-verified metadata, as in older RAW files: the core validates
//! syntax/length/framing, never invokes a codec or cryptographic interpretation.
//! The host must check original bytes before explicit re-decode. Unknown generic
//! kinds/state versions retain cached pixels without authorizing interpretation.
//! Typed22 appends ADSC even with zero descriptions; artwork-free source tables
//! and existing v20 wrappers retain the older compatible writers when possible.
//! `raw_local_masks` reserves a known generic attachment contract: state v1
//! lists ordered node IDs and matching full-RAW dimensions; each resource is
//! `RZLM`, u32LE version1/width/height and exactly width*height u16LE samples.
//! Known mask records are preflighted before PNG decode and share RBIN storage
//! with mask handles. Unknown state/binary versions stay opaque and preserve
//! cached rendering; materializing or editing their coverage explicitly refuses.
//!
//! Byte v23 / typed v24 (written until 2026-10-10, read still) reuse the
//! v21/v22 prefix, RSRC and ATCH, and the same RBIN pool, and append `LFLT`
//! after RBIN: u32 binding count (exactly RSRC's binding count), then one
//! positional u8 presence slot per binding. A present slot adds u32 UTF-8 JSON
//! length/descriptor (at most 64 KiB), u32 mask-ref count (at most 16) and
//! that many u32 RBIN references; UINT32_MAX means unmasked. At least one slot
//! is present. Typed24 ends with ADSC even when empty.
//!
//! Smart objects, Stack Mode and live filters were removed on 2026-10-10
//! (`designs/convert-to-smart-object-status.md`). Their words and tables are
//! still READ, never written: words 65/66 (and 73/68, float contents) and
//! the `SOCT` table, words 69/70 and the `SOSM` table, and the v23/v24 `LFLT`
//! table are bounds-checked and skipped ([`skip_contents_table`],
//! [`skip_stack_mode_table`], `native_source_resources`). A binding whose
//! asset had contents, or whose `LFLT` slot was present, is DROPPED
//! ([`Binding::dropped`]): its layer keeps its stored pixels, as a
//! `rasterize` source edit leaves them, and the nested documents and filter
//! masks are discarded undecoded.
//!
//! An asset's profile-refusal sentence is not in any of these bodies:
//! it rides in the conditional `RSPR` table (`source_refusal_native.rs`,
//! words 56/57), keyed by the positional RSRC asset index, which attaches it
//! to [`PendingAsset::profile_refusal`] before any PNG decodes.
//!
//! An asset's advanced depth byte may be 32 (in a typed or a byte file): a
//! 32-bit float master, whose u64 length/payload slot holds `w · h · 16`
//! raw LE f32 RGBA bytes instead of a PNG
//! (`float_native::put_samples`). It is admitted only when the file declares
//! word 67 (typed) or 72 (byte) (`float_native`, `float_masters`), and its
//! length must equal `w · h · 16` exactly. Such an asset carries a RAW record
//! (the v19 flag or the v22 `ATCH` slot) only when the typed file also
//! declares word 78 (`float_raw_masters`): a 32-bit float (linear)
//! Camera RAW development. No byte file does.
//!
//! `native_sources*.rs` stores deduplicated assets/bindings in byte v16 or typed v17 when needed;
//! compatible v14/v15 and source-free v10/v13 writers remain.
use std::collections::HashSet;
use std::sync::Arc;

use crate::drdc::{Reader, MAX_DRDC_BLOB_LEN, MAX_DRDC_LAYERS, MAX_DRDC_PNG_LEN};
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::native_stream::{NativeSink, Payload};
use crate::precision_image::pixel_count;
use crate::precision_sampling::GeometryFilter;
use crate::retained_source::{SourceAsset, SourceLink};

#[path = "native_sources_assets.rs"]
mod assets;
#[path = "native_source_resources.rs"]
mod binary;
#[path = "native_sources_byte.rs"]
mod byte;
#[path = "native_sources_precision.rs"]
mod precision;
#[path = "native_sources_raw.rs"]
mod raw;
pub(crate) use assets::{
    byte_source_png_encode_scratch, png_bound as source_png_encoded_bound, single_image_capacity,
    source_plane_capacity, SourceTable,
};
pub(crate) use byte::byte_sources;
pub(crate) use precision::precision_sources;

pub(crate) const BYTE_SOURCE_VERSION: u32 = 14;
pub(crate) const PRECISION_SOURCE_VERSION: u32 = 15;
pub(crate) const BYTE_LINK_VERSION: u32 = 16;
pub(crate) const PRECISION_LINK_VERSION: u32 = 17;
pub(crate) const BYTE_RAW_VERSION: u32 = 18;
pub(crate) const PRECISION_RAW_VERSION: u32 = 19;
pub(crate) const BYTE_RESOURCE_VERSION: u32 = 21;
pub(crate) const PRECISION_RESOURCE_VERSION: u32 = 22;
pub(crate) const BYTE_FILTER_VERSION: u32 = 23;
pub(crate) const PRECISION_FILTER_VERSION: u32 = 24;
/// Four existing blob slots worth of aggregate ICC bytes bounds profile/TRC
/// parsing even for 1024 tiny source assets. This is unchanged from v14/v15.
const MAX_SOURCE_PROFILE_BYTES: u64 = 4 * MAX_DRDC_BLOB_LEN as u64;

pub(crate) fn profile_capacity(bytes: usize, total: &mut u64) -> Result<(), String> {
    if bytes > MAX_DRDC_BLOB_LEN as usize {
        return Err("embedded source profile exceeds its byte limit".into());
    }
    *total = total
        .checked_add(bytes as u64)
        .ok_or("embedded source profile byte overflow")?;
    if *total > MAX_SOURCE_PROFILE_BYTES {
        return Err("embedded source profile aggregate exceeds 64 MiB".into());
    }
    Ok(())
}

pub(crate) struct PendingAsset<'a> {
    pub id: &'a str,
    pub revision_id: &'a str,
    pub depth: u8,
    pub profile: &'a [u8],
    pub dimensions: (u32, u32),
    /// The PNG or f32 body, a span (`native_stream`).
    pub png: Payload<'a>,
    /// Attached by `source_refusal_native::parse`; `None` without RSPR.
    pub profile_refusal: Option<&'a str>,
}
#[derive(Clone)]
pub(crate) struct Binding {
    pub layer: usize,
    pub asset: usize,
    pub transform: crate::source_transform::SourceTransform,
    pub filter: GeometryFilter,
    pub link: Option<Arc<SourceLink>>,
    /// Read from a record that carried removed state (smart-object contents
    /// or a live filter, module doc): validated with the file, then never
    /// materialized, so its layer opens as a plain raster. Always false on
    /// the writer side.
    pub dropped: bool,
}
struct PendingLink<'a> {
    path: &'a str,
    sha256: &'a str,
}
#[derive(Default)]
pub(crate) struct PendingSources<'a> {
    pub assets: Vec<PendingAsset<'a>>,
    pub bindings: Vec<Binding>,
    links: Vec<Option<PendingLink<'a>>>,
    raw: raw::PendingRawTable<'a>,
    binary: binary::PendingResources<'a>,
}

pub(crate) fn valid_id(id: &str) -> bool {
    crate::retained_source::validate_id(id).is_ok()
}

pub(crate) fn validate_binding(
    binding: &Binding,
    source: (u32, u32),
    fallback: (u32, u32),
    offset: (i32, i32),
) -> Result<(), String> {
    if crate::source_transform::bounds(source, binding.transform)?
        != (offset.0, offset.1, fallback.0, fallback.1)
    {
        return Err("invalid embedded source transform or fallback extent".into());
    }
    Ok(())
}
fn slot<'a>(reader: &mut Reader<'a>, cap: u32) -> Result<&'a [u8], String> {
    let len = reader.u32()?;
    if len > cap {
        return Err("embedded source slot exceeds its byte limit".into());
    }
    reader.take(len as usize)
}
fn identity<'a>(reader: &mut Reader<'a>) -> Result<&'a str, String> {
    let value = std::str::from_utf8(slot(reader, 128)?)
        .map_err(|_| "embedded source identity is not ASCII")?;
    if !valid_id(value) {
        return Err("invalid embedded source identity or revision".into());
    }
    Ok(value)
}

impl<'a> PendingSources<'a> {
    /// Borrow every pixel/profile/dependency payload before allocating its state.
    /// `float_masters` is word 67's (typed) or 72's (byte) claim: only then
    /// may an asset be depth 32. `float_raw_masters` is typed word 78's: only
    /// then may a depth-32 asset carry a RAW record (the byte reader passes
    /// false).
    pub fn parse(
        reader: &mut Reader<'a>,
        records: usize,
        version: u32,
        float_masters: bool,
        float_raw_masters: bool,
    ) -> Result<Self, String> {
        if matches!(
            version,
            BYTE_RESOURCE_VERSION
                | PRECISION_RESOURCE_VERSION
                | BYTE_FILTER_VERSION
                | PRECISION_FILTER_VERSION
        ) {
            let base = reader.u32()?;
            if base
                != if matches!(version, BYTE_RESOURCE_VERSION | BYTE_FILTER_VERSION) {
                    BYTE_LINK_VERSION
                } else {
                    PRECISION_LINK_VERSION
                }
            {
                return Err("invalid binary source base-table version".into());
            }
            let mut sources = Self::parse(reader, records, base, float_masters, float_raw_masters)?;
            let (binary, filtered) = binary::PendingResources::parse(
                reader,
                &sources.assets,
                &sources.bindings,
                matches!(version, BYTE_FILTER_VERSION | PRECISION_FILTER_VERSION),
                float_raw_masters,
            )?;
            sources.binary = binary;
            for (binding, filtered) in sources.bindings.iter_mut().zip(filtered) {
                binding.dropped |= filtered;
            }
            return Ok(sources);
        }
        let has_raw = matches!(version, BYTE_RAW_VERSION | PRECISION_RAW_VERSION);
        let advanced = has_raw || matches!(version, BYTE_LINK_VERSION | PRECISION_LINK_VERSION);
        let owner_depth = match version {
            BYTE_SOURCE_VERSION | BYTE_LINK_VERSION | BYTE_RAW_VERSION => 8,
            PRECISION_SOURCE_VERSION | PRECISION_LINK_VERSION | PRECISION_RAW_VERSION => 16,
            _ => return Err("unsupported native source-table version".into()),
        };
        if reader.take(4)? != b"RSRC" {
            return Err("missing embedded source asset table".into());
        }
        let count = reader.u32()? as usize;
        if count == 0 || count > MAX_DRDC_LAYERS as usize {
            return Err("invalid embedded source asset count".into());
        }
        let mut assets = Vec::with_capacity(count);
        let mut identities = HashSet::new();
        let mut profile_bytes = 0;
        let mut raw = raw::PendingRawTable::default();
        for _ in 0..count {
            let id = identity(reader)?;
            let revision_id = if advanced { identity(reader)? } else { id };
            if !identities.insert((id, revision_id)) {
                return Err("duplicate embedded source identity and revision".into());
            }
            let depth = if advanced {
                let depth = reader.u8()?;
                if !(matches!(depth, 8 | 16) || (depth == 32 && float_masters))
                    || reader.take(3)? != [0, 0, 0]
                {
                    return Err("unsupported embedded source depth or flags".into());
                }
                depth
            } else {
                owner_depth
            };
            let profile = slot(reader, MAX_DRDC_BLOB_LEN)?;
            profile_capacity(profile.len(), &mut profile_bytes)?;
            let dimensions = (reader.u32()?, reader.u32()?);
            pixel_count(dimensions.0, dimensions.1)?;
            let len = reader.u64()?;
            if depth == 32 {
                let exact = (pixel_count(dimensions.0, dimensions.1)? as u64)
                    .checked_mul(16)
                    .ok_or("float native raster byte overflow")?;
                if len != exact {
                    return Err(
                        "float native raster byte length differs from its dimensions".into(),
                    );
                }
            } else if len > u64::from(MAX_DRDC_PNG_LEN) {
                return Err("embedded source PNG exceeds its byte limit".into());
            }
            let png = reader.take_payload(len)?;
            if has_raw {
                raw.parse_record(reader, depth, float_raw_masters)?;
            }
            assets.push(PendingAsset {
                id,
                revision_id,
                depth,
                profile,
                dimensions,
                png,
                profile_refusal: None,
            });
        }
        let count = reader.u32()? as usize;
        if count == 0 || count > records {
            return Err("invalid embedded source binding count".into());
        }
        let mut bindings = Vec::with_capacity(count);
        let mut links = Vec::with_capacity(count);
        let mut targets = HashSet::new();
        let mut used_assets = HashSet::new();
        for _ in 0..count {
            let layer = reader.u32()? as usize;
            let asset = reader.u32()? as usize;
            if layer >= records || asset >= assets.len() || !targets.insert(layer) {
                return Err("invalid or duplicate embedded source reference".into());
            }
            used_assets.insert(asset);
            let filter = match reader.u8()? {
                0 => GeometryFilter::Nearest,
                1 => GeometryFilter::Bilinear,
                2 => GeometryFilter::Bicubic,
                3 => GeometryFilter::Lanczos3,
                _ => return Err("unsupported embedded source sampling filter".into()),
            };
            if reader.take(3)? != [0, 0, 0] {
                return Err("unsupported embedded source flags".into());
            }
            let mut transform = [0.0; 6];
            for value in &mut transform {
                *value = reader.f64()?;
            }
            let link = if advanced {
                match reader.u8()? {
                    0 => None,
                    1 => {
                        let path = std::str::from_utf8(slot(reader, 16 * 1024)?)
                            .map_err(|_| "linked source path is not UTF-8")?;
                        let sha256 = std::str::from_utf8(reader.take(64)?)
                            .map_err(|_| "linked source fingerprint is not ASCII")?;
                        SourceLink::validate_parts(path, sha256)?;
                        Some(PendingLink { path, sha256 })
                    }
                    _ => return Err("invalid linked source presence flag".into()),
                }
            } else {
                None
            };
            bindings.push(Binding {
                layer,
                asset,
                transform: crate::source_transform::SourceTransform::Affine(transform),
                filter,
                link: None,
                dropped: false,
            });
            links.push(link);
        }
        if used_assets.len() != assets.len() {
            return Err("unreferenced embedded source asset".into());
        }
        if has_raw {
            raw.parse_payloads(reader)?;
        }
        Ok(Self {
            assets,
            bindings,
            links,
            raw,
            binary: binary::PendingResources::default(),
        })
    }
    pub fn profiles(&self) -> Result<Vec<Arc<IccProfile>>, String> {
        self.assets
            .iter()
            .map(|a| {
                IccProfile::parse(a.profile)
                    .map(Arc::new)
                    .ok_or_else(|| "invalid embedded source RGB profile".into())
            })
            .collect()
    }
    /// Σ `w · h` over the 32-bit float masters: the reopen
    /// charges them as float rasters.
    pub fn float_master_pixels(&self) -> u64 {
        self.assets
            .iter()
            .filter(|a| a.depth == 32)
            .map(|a| u64::from(a.dimensions.0) * u64::from(a.dimensions.1))
            .sum()
    }
    /// Whether some 32-bit float master carries a RAW record, in either
    /// table: what typed word 78 claims (`float_native`).
    pub fn float_raw_master_present(&self) -> bool {
        self.assets
            .iter()
            .enumerate()
            .any(|(index, a)| a.depth == 32 && (self.raw.has(index) || self.binary.has_raw(index)))
    }
    /// Word 79's claim over the parsed pool (`raw_locals_native`): its byte
    /// total and the largest attachment's resource count; (0, 0) without one.
    pub(crate) fn raw_locals_claim(&self) -> (u64, usize) {
        self.binary.raw_locals_claim()
    }
    pub fn validate_raw_descriptors(&self) -> Result<(), String> {
        self.raw.validate_descriptors()?;
        self.binary.validate_descriptors()
    }
    pub fn nonpixel_retained_bound(&self) -> u64 {
        self.assets
            .iter()
            .map(|a| {
                4 * a.profile.len() as u64
                    + a.id.len() as u64
                    + a.revision_id.len() as u64
                    + a.profile_refusal.map_or(0, |text| text.len() as u64)
            })
            .sum::<u64>()
            + self
                .links
                .iter()
                .flatten()
                .map(|l| l.path.len() as u64 + 64)
                .sum::<u64>()
            + self.raw.retained_bound()
            + self.binary.retained_bound()
    }
    /// Drops every binding of an asset in `assets` (a skipped `SOCT`
    /// entry's, module doc).
    pub fn drop_assets(&mut self, assets: &[usize]) {
        for binding in &mut self.bindings {
            binding.dropped |= assets.contains(&binding.asset);
        }
    }
    /// Only after complete native admission: dependencies are metadata, never I/O.
    /// A dropped binding is left out (module doc).
    pub fn materialize_bindings(&self) -> Vec<Binding> {
        self.bindings
            .iter()
            .zip(&self.links)
            .filter(|(binding, _)| !binding.dropped)
            .map(|(binding, link)| {
                let mut out = binding.clone();
                out.link = link.as_ref().map(|l| {
                    Arc::new(SourceLink {
                        path: l.path.into(),
                        sha256: l.sha256.into(),
                    })
                });
                out
            })
            .collect()
    }
}

/// Byte word 65 / typed word 66: the file holds a `SOCT` table (module doc).
pub(crate) const CONTENTS_BYTE_WORD: u32 = 65;
pub(crate) const CONTENTS_PRECISION_WORD: u32 = 66;
/// Byte word 73 / typed word 68: some `SOCT` entry is kind 3, float contents.
pub(crate) const FLOAT_CONTENTS_BYTE_WORD: u32 = 73;
pub(crate) const FLOAT_CONTENTS_PRECISION_WORD: u32 = 68;
/// Byte word 69 / typed word 70: the file holds a `SOSM` table after `SOCT`.
pub(crate) const STACK_MODE_BYTE_WORD: u32 = 69;
pub(crate) const STACK_MODE_PRECISION_WORD: u32 = 70;
/// The refusal of a contents or stack-mode word whose companion is missing.
pub(crate) const CONTENTS_WORD_REFUSAL: &str = "invalid native smart object contents word";

/// Skips the `SOCT` table, checking its framing as the writer laid it out:
/// `SOCT`, u32 schema 1, u32 count (1…`asset_count`), then per entry a u32
/// asset index (strictly ascending, in range), a u32 kind (1 byte, 2 typed,
/// 3 float only under `float_contents`, 4 an alias only under `aliases`), and
/// a u64 length with that many bytes starting `DRDC` — or, for an alias, the
/// asset index of an earlier full entry and no bytes. Nothing is decoded.
/// Answers each entry's asset index and whether some entry was kind 3.
pub(crate) fn skip_contents_table(
    reader: &mut Reader<'_>,
    asset_count: usize,
    float_contents: bool,
    aliases: bool,
) -> Result<(Vec<usize>, bool), String> {
    if reader.take(4)? != b"SOCT" || reader.u32()? != 1 {
        return Err("invalid smart object contents table tag or schema".into());
    }
    let count = reader.u32()? as usize;
    if count == 0 || count > asset_count {
        return Err("invalid smart object contents table count".into());
    }
    let mut assets: Vec<usize> = Vec::with_capacity(count);
    let mut full = Vec::with_capacity(count);
    let mut float = false;
    for _ in 0..count {
        let asset = reader.u32()? as usize;
        if asset >= asset_count || assets.last().is_some_and(|prior| asset <= *prior) {
            return Err("invalid or duplicate smart object contents asset".into());
        }
        let kind = reader.u32()?;
        if aliases && kind == 4 {
            let target = reader.u64()?;
            if !full.iter().any(|&f: &usize| f as u64 == target) {
                return Err("invalid smart object contents alias".into());
            }
        } else {
            if !(kind == 1 || kind == 2 || (float_contents && kind == 3)) {
                return Err("unsupported smart object contents kind".into());
            }
            let length = reader.u64()?;
            if length < 8 || length > reader.remaining() as u64 {
                return Err("smart object contents length is out of range".into());
            }
            if !crate::native_magic::is_native_magic(&reader.take_payload(length)?.head(4)?) {
                return Err("smart object contents are not a DRDC document".into());
            }
            float |= kind == 3;
            full.push(asset);
        }
        assets.push(asset);
    }
    Ok((assets, float))
}

/// Skips the `SOSM` table: `SOSM`, u32 schema 1, u32 count (1…`records`),
/// then per entry a u32 record index (strictly ascending, in range) and a
/// nonzero u32 mode code. Every moded record's asset has a `SOCT` entry, so
/// its binding is already dropped.
pub(crate) fn skip_stack_mode_table(reader: &mut Reader<'_>, records: usize) -> Result<(), String> {
    if reader.take(4)? != b"SOSM" || reader.u32()? != 1 {
        return Err("invalid stack mode table tag or schema".into());
    }
    let count = reader.u32()? as usize;
    if count == 0 || count > records {
        return Err("invalid stack mode table count".into());
    }
    let mut last = None;
    for _ in 0..count {
        let index = reader.u32()? as usize;
        if index >= records || last.is_some_and(|prior| index <= prior) {
            return Err("invalid or duplicate stack mode record".into());
        }
        if reader.u32()? == 0 {
            return Err("invalid stack mode code".into());
        }
        last = Some(index);
    }
    Ok(())
}

pub(crate) fn append(
    out: &mut dyn NativeSink,
    bytes: &[u8],
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    if out
        .position()
        .checked_add(bytes.len() as u64)
        .is_none_or(|n| n > bound)
    {
        return Err("embedded source encoding exceeds admitted byte bound".into());
    }
    if !out.reserve(bytes.len()) {
        return Err("could not allocate embedded source output".into());
    }
    for chunk in bytes.chunks(64 * 1024) {
        job.check()?;
        out.put(chunk)?;
    }
    Ok(())
}
pub(crate) fn put_slot(
    out: &mut dyn NativeSink,
    bytes: &[u8],
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    let len = u32::try_from(bytes.len()).map_err(|_| "embedded source slot length overflow")?;
    append(out, &len.to_le_bytes(), bound, job)?;
    append(out, bytes, bound, job)
}
pub(crate) fn put_asset_header(
    out: &mut dyn NativeSink,
    asset: &SourceAsset,
    advanced: bool,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    put_slot(out, asset.id.as_bytes(), bound, job)?;
    if advanced {
        put_slot(out, asset.revision_id.as_bytes(), bound, job)?;
        append(out, &[asset.depth() as u8, 0, 0, 0], bound, job)?;
    }
    put_slot(out, asset.profile().bytes(), bound, job)?;
    let dimensions = asset.dimensions();
    for v in [dimensions.0, dimensions.1] {
        append(out, &v.to_le_bytes(), bound, job)?;
    }
    Ok(())
}
pub(crate) fn put_bindings(
    out: &mut dyn NativeSink,
    bindings: &[Binding],
    advanced: bool,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    append(out, &(bindings.len() as u32).to_le_bytes(), bound, job)?;
    for b in bindings {
        append(out, &(b.layer as u32).to_le_bytes(), bound, job)?;
        append(out, &(b.asset as u32).to_le_bytes(), bound, job)?;
        let filter = match b.filter {
            GeometryFilter::Nearest => 0,
            GeometryFilter::Bilinear => 1,
            GeometryFilter::Bicubic => 2,
            GeometryFilter::Lanczos3 => 3,
        };
        append(out, &[filter, 0, 0, 0], bound, job)?;
        let legacy = match b.transform {
            crate::source_transform::SourceTransform::Affine(matrix) => matrix,
            crate::source_transform::SourceTransform::Projective(_) => [1., 0., 0., 1., 0., 0.],
        };
        for v in legacy {
            append(out, &v.to_le_bytes(), bound, job)?;
        }
        if advanced {
            append(out, &[u8::from(b.link.is_some())], bound, job)?;
            if let Some(link) = &b.link {
                put_slot(out, link.path.as_bytes(), bound, job)?;
                append(out, link.sha256.as_bytes(), bound, job)?;
            }
        }
    }
    Ok(())
}
