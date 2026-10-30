//! Conditional LAYER COMP wrappers. DRDC 43 (byte) / 44 (typed) is the
//! OUTERMOST version word — it inserts its u32 before the exact prior
//! version/body, 41/42 included, so every earlier optional feature keeps its
//! own framing — and appends one table after every prior tail: `LCMP`, u32
//! schema, u32 length, then that many bytes of UTF-8 JSON.
//! `label_native.rs` is the shape this follows.
//!
//! The payload is JSON rather than a packed table because a comp entry
//! carries a STYLE, which is already JSON everywhere else in this crate
//! (`style.rs`), and a second binary encoding of it would be a second
//! implementation of the same schema.
//!
//! **SCHEMA 2 — a comp is written by the PERSISTED LAYER ID** (12E-1b), at
//! BOTH depths. Identity has been persisted since 12E-1 (`identity_native`),
//! so an entry writes `{"id": <the entry's layer_id>, "name": …}` with no
//! `layer` key at all, and the reader binds an entry when — and only when —
//! some record in the file carries that id. The NAME is not compared: the id
//! IS the identity, and that is the whole point of the migration. A reopened
//! comp therefore survives a reorder AND a rename, which schema 1 could not
//! do (it bound by index + name, so either one broke it).
//!
//! The binary word and the JSON `schema` are the SAME number and MUST agree;
//! a file claiming one in its framing and the other in its payload is refused
//! ("unsupported native layer comp schema"), so a crafted file cannot pick
//! the looser rule. Schema 2 additionally requires the file to carry a layer
//! identity table (`has_ids`): without one, every id in the payload would be
//! meaningless and binding by coincidence is exactly what must not happen.
//!
//! **SCHEMA 1 is still READ, byte for byte as it always was.** Its entries
//! carry `"layer": <index>` or `"layer": null` and bind by INDEX + NAME: the
//! reader re-binds an entry only when the layer at that index still has that
//! name, and the `>= records` / duplicate-index refusals apply to BOUND
//! entries only. It is never WRITTEN again — a reopened schema-1 file resaves
//! as schema 2 — and an `id` key inside a schema-1 payload refuses, as a
//! `layer` key inside a schema-2 one does.
//!
//! **A stale entry is written, not dropped.** Schema 2 writes the entry's
//! `layer_id` verbatim, which for a deleted layer is a DANGLING id no record
//! carries (and 0 for an entry that never bound); the reader treats both as
//! stale rather than refusing, because the writer must never emit a file its
//! own reader refuses and a comp that quietly lost the row naming a deleted
//! layer would lose the only report the host has. A repeated non-zero id
//! WITHIN one comp does refuse — that is a structural claim, since two rows
//! would address one record.
//!
//! The reader raises the process id counter past every id it reads
//! (`reserve_layer_ids_through`), so a dangling id can never be re-minted
//! onto a different layer later in the session and silently resurrect a comp
//! row.
//!
//! The table is written ONLY when the document holds a comp: a document with
//! none is byte-for-byte the file the prior writer made, first word included.
//! A bad schema, a zero or oversize length, malformed JSON, more than
//! [`crate::doc_comps::MAX_LAYER_COMPS`] comps, a duplicate comp name, a bad
//! or repeated bound index, a non-finite opacity and a trailing word all
//! refuse BEFORE a single PNG is decoded, as every other conditional table
//! does.
//!
//! `comps_native` is the outermost conditional `.rz` wrapper — byte 43 / typed 44 before every
//! prior version word, 41/42 included, and one `LCMP` table (u32 schema 1, u32 length, UTF-8 JSON)
//! after every prior tail — following `label_native.rs`'s shape. Written only when a comp exists,
//! so a comp-less document's bytes are exactly the prior writer's. **Since 12E-1b comps bind by the
//! PERSISTED id at BOTH depths: `LCMP` SCHEMA 2**, written by the shared `payload` so neither depth
//! can word one schema two ways — an entry is `{"id": <the entry's layer_id VERBATIM>, "name": S,
//! …}` with no `layer` key, and it binds when some record in the FILE carries that id, the NAME
//! deliberately NOT compared, so a reopened comp survives a reorder AND a rename. a schema-2
//! payload in a file with no `LIDS` table is refused ("native layer comps by id require a layer
//! identity table"), so nothing can bind by coincidence; a repeated non-zero id within one comp, a
//! `layer` key in a schema-2 payload and an `id` key in a schema-1 one all refuse, while an id NO
//! record carries is STALE (0 = written stale, a dangling id = the layer is gone).

use serde::{Deserialize, Serialize};

use crate::doc_comps::{
    CompAppearance, CompCaptures, LayerComp, LayerCompEntry, MAX_COMP_COMMENT_LEN,
    MAX_COMP_NAME_LEN, MAX_LAYER_COMPS,
};
use crate::drdc::Reader;
use crate::job_control::JobContext;
use crate::precision_native::append;

pub const BYTE_VERSION: u32 = 43;
pub const PRECISION_VERSION: u32 = 44;

/// The number BOTH the binary schema word and the JSON `schema` key carry in
/// every file this build writes. Schema 1 is still read (see the module doc)
/// and never written again.
pub const SCHEMA: u32 = 2;

/// The largest `LCMP` payload this build writes or reads: 8 MiB of JSON, far
/// beyond 64 comps over any realistic layer stack, and small enough that a
/// crafted length can never ask for an allocation worth refusing over.
pub const MAX_COMPS_JSON_LEN: u64 = 8 << 20;

// ------------------------------------------------------------- file form --

/// The `LCMP` payload's object. Field order here IS the written key order.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileForm {
    schema: u32,
    comps: Vec<FileComp>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileComp {
    name: String,
    comment: String,
    captures: CompCaptures,
    entries: Vec<FileEntry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileEntry {
    /// SCHEMA 1 ONLY: the layer's index when the file was written, or null
    /// for a stale entry. Never written by this build; refused inside a
    /// schema-2 payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    layer: Option<usize>,
    /// SCHEMA 2: the entry's `layer_id` verbatim — 0 for one that never
    /// bound, a dangling id for one whose layer is gone, both stale on read.
    /// Refused inside a schema-1 payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<u64>,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    visible: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    offset: Option<[i32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    opacity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    blend: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    style: Option<String>,
}

/// The `LCMP` payload for `comps`, or an EMPTY vector when there is none.
/// Always SCHEMA 2 — both depths share this function verbatim, so neither
/// can word one schema two ways.
///
/// `name_of` answers the CURRENT name of the layer an entry's id names, or
/// `None` when that id no longer binds (in which case the entry's last-known
/// name is written) — the one thing this function needs from a document.
pub(crate) fn payload(
    comps: &[LayerComp],
    name_of: impl Fn(u64) -> Option<String>,
) -> Result<Vec<u8>, String> {
    if comps.is_empty() {
        return Ok(Vec::new());
    }
    let form = FileForm {
        schema: SCHEMA,
        comps: comps
            .iter()
            .map(|comp| FileComp {
                name: comp.name.clone(),
                comment: comp.comment.clone(),
                captures: comp.captures,
                entries: comp
                    .entries
                    .iter()
                    .map(|entry| {
                        FileEntry {
                            layer: None,
                            // The id VERBATIM: 0 for an entry that never
                            // bound, a dangling id for one whose layer is
                            // gone. Both reopen stale.
                            id: Some(entry.layer_id),
                            name: match name_of(entry.layer_id) {
                                Some(name) => name,
                                None => entry.layer_name.clone(),
                            },
                            visible: entry.visible,
                            offset: entry.offset.map(|(x, y)| [x, y]),
                            opacity: entry.appearance.as_ref().map(|a| a.opacity),
                            blend: entry.appearance.as_ref().map(|a| a.blend),
                            style: entry.appearance.as_ref().and_then(|a| a.style.clone()),
                        }
                    })
                    .collect(),
            })
            .collect(),
    };
    serde_json::to_vec(&form).map_err(|e| format!("could not encode layer comps: {e}"))
}

/// The table's own size, or 0 when it is not written: the outermost version
/// word the writer emits alongside the table, then the tag, the schema word
/// and the length word ahead of the payload.
pub(crate) fn header_bytes(payload_len: u64) -> u64 {
    if payload_len == 0 {
        0
    } else {
        16 + payload_len
    }
}

pub(crate) fn encode(
    payload: &[u8],
    out: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    if payload.is_empty() {
        return Ok(());
    }
    let length = u32::try_from(payload.len())
        .ok()
        .filter(|&n| u64::from(n) <= MAX_COMPS_JSON_LEN)
        .ok_or(crate::doc_comps::PAYLOAD_CAP_REFUSAL)?;
    append(out, b"LCMP", bound, job)?;
    for value in [SCHEMA, length] {
        append(out, &value.to_le_bytes(), bound, job)?;
    }
    append(out, payload, bound, job)
}

/// How one parsed entry addresses its layer.
pub(crate) enum CompBinding {
    /// Written stale — schema 1's `"layer": null`, schema 2's `"id": 0`.
    Stale,
    /// SCHEMA 1: the FILE index the entry carried; binds when the record at
    /// that index still has the entry's name.
    Index(usize),
    /// SCHEMA 2: the PERSISTED id the entry carried; binds when some record
    /// in the file carries it, whatever its name or index.
    Id(u64),
}

/// What [`parse`] hands back: the comps, and the per-comp list of each
/// entry's binding that [`rebind`] resolves.
pub(crate) type ParsedComps = (Vec<LayerComp>, Vec<Vec<CompBinding>>);

/// Reads the table. The comps come back with `layer_id = 0` on every entry —
/// binding is the caller's, once its own layers exist — and the second value
/// is the per-comp list of each entry's binding. [`rebind`] turns the pair
/// into bound entries.
///
/// `has_ids` is whether the FILE carries a layer identity table. A schema-2
/// payload without one is refused outright: every id in it would be
/// meaningless, and binding by coincidence is exactly what must not happen.
pub(crate) fn parse(
    reader: &mut Reader<'_>,
    records: usize,
    has_ids: bool,
    job: JobContext<'_>,
) -> Result<ParsedComps, String> {
    if reader.take(4)? != b"LCMP" {
        return Err("unsupported native layer comp schema".into());
    }
    let schema = reader.u32()?;
    if schema != 1 && schema != SCHEMA {
        return Err("unsupported native layer comp schema".into());
    }
    let length = u64::from(reader.u32()?);
    if length == 0 || length > MAX_COMPS_JSON_LEN {
        return Err("invalid native layer comp payload length".into());
    }
    let bytes = reader.take(length as usize)?;
    job.check()?;
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "invalid native layer comp payload encoding".to_string())?;
    let form: FileForm = serde_json::from_str(text)
        .map_err(|e| format!("invalid native layer comp payload: {e}"))?;
    // The binary word and the JSON key are ONE statement about the file; a
    // payload claiming the other number can never pick the looser rule.
    if form.schema != schema {
        return Err("unsupported native layer comp schema".into());
    }
    if schema == SCHEMA && !has_ids {
        return Err("native layer comps by id require a layer identity table".into());
    }
    if form.comps.is_empty() || form.comps.len() > MAX_LAYER_COMPS {
        return Err("invalid native layer comp count".into());
    }
    let mut comps = Vec::with_capacity(form.comps.len());
    let mut bindings = Vec::with_capacity(form.comps.len());
    let mut names: Vec<&str> = Vec::with_capacity(form.comps.len());
    let mut highest = 0u64;
    for comp in &form.comps {
        job.check()?;
        if comp.name.trim().is_empty()
            || comp.name.len() > MAX_COMP_NAME_LEN
            || comp.comment.len() > MAX_COMP_COMMENT_LEN
        {
            return Err("invalid native layer comp name or comment".into());
        }
        if names.contains(&comp.name.as_str()) {
            return Err("duplicate native layer comp name".into());
        }
        names.push(&comp.name);
        if comp.captures.is_empty() {
            return Err("invalid native layer comp capture set".into());
        }
        let mut seen_index: Vec<usize> = Vec::new();
        let mut seen_id: Vec<u64> = Vec::new();
        let mut entries = Vec::with_capacity(comp.entries.len());
        let mut positions = Vec::with_capacity(comp.entries.len());
        for entry in &comp.entries {
            // Each schema admits exactly one address key; the other refuses,
            // so a payload cannot smuggle the rule it was not written under.
            let binding = if schema == SCHEMA {
                if entry.layer.is_some() {
                    return Err("invalid native layer comp reference".into());
                }
                match entry.id.unwrap_or(0) {
                    0 => CompBinding::Stale,
                    id => {
                        // Two rows addressing one record is a structural
                        // claim, so it refuses; an id NO record carries is
                        // simply stale.
                        if seen_id.contains(&id) {
                            return Err("invalid native layer comp reference".into());
                        }
                        seen_id.push(id);
                        highest = highest.max(id);
                        CompBinding::Id(id)
                    }
                }
            } else {
                if entry.id.is_some() {
                    return Err("invalid native layer comp reference".into());
                }
                match entry.layer {
                    None => CompBinding::Stale,
                    Some(index) => {
                        if index >= records || seen_index.contains(&index) {
                            return Err("invalid native layer comp reference".into());
                        }
                        seen_index.push(index);
                        CompBinding::Index(index)
                    }
                }
            };
            if entry.name.len() > crate::drdc::MAX_DRDC_NAME_LEN as usize {
                return Err("invalid native layer comp entry name".into());
            }
            let appearance = match (entry.opacity, entry.blend) {
                (Some(opacity), Some(blend)) => {
                    if !opacity.is_finite() {
                        return Err("invalid native layer comp opacity".into());
                    }
                    Some(CompAppearance {
                        opacity,
                        blend,
                        style: entry.style.clone(),
                    })
                }
                (None, None) => {
                    if entry.style.is_some() {
                        return Err("invalid native layer comp appearance".into());
                    }
                    None
                }
                _ => return Err("invalid native layer comp appearance".into()),
            };
            positions.push(binding);
            entries.push(LayerCompEntry {
                // Binding is `rebind`'s, once the reader's own layers exist.
                layer_id: 0,
                layer_name: entry.name.clone(),
                visible: entry.visible,
                offset: entry.offset.map(|[x, y]| (x, y)),
                appearance,
            });
        }
        comps.push(LayerComp {
            name: comp.name.clone(),
            comment: comp.comment.clone(),
            captures: comp.captures,
            entries,
        });
        bindings.push(positions);
    }
    // A DANGLING id must never be re-minted onto a different layer later in
    // the session: that would silently resurrect a stale comp row against the
    // wrong picture. `identity_native::parse` raises the counter past the
    // records' own ids; this raises it past the comps' too.
    crate::doc_identity::reserve_layer_ids_through(highest);
    Ok((comps, bindings))
}

/// Binds each parsed entry to the layer it named.
///
/// `id_and_name_at` answers the id and name of the record at a FILE index
/// (schema 1's address); `has_id` answers whether any record carries an id
/// (schema 2's). An entry written stale, a schema-1 index the document no
/// longer has, a schema-1 index whose layer now carries a different name and
/// a schema-2 id no record carries all stay unbound (`layer_id` 0), which is
/// what "stale" is.
///
/// Schema 2 deliberately does NOT compare the name: the id is the identity,
/// so a renamed layer keeps its comp rows.
///
/// A schema-1 HOLD: an in-range index whose record now carries a
/// different name leaves its entry stale, unchanged, and appends one id-only
/// entry for that record (no recorded keys, its current name), so the renamed
/// layer still counts as recorded and applying the comp sets nothing on it
/// rather than hiding it. Schema 2 persists the HOLD as an id-only row, which
/// the reader admits; it cannot collide, because a schema-1 comp never has two
/// rows on one index.
pub(crate) fn rebind(
    comps: &mut [LayerComp],
    bindings: &[Vec<CompBinding>],
    id_and_name_at: impl Fn(usize) -> Option<(u64, String)>,
    has_id: impl Fn(u64) -> bool,
) {
    for (comp, positions) in comps.iter_mut().zip(bindings) {
        let mut held = Vec::new();
        for (entry, binding) in comp.entries.iter_mut().zip(positions) {
            match binding {
                CompBinding::Stale => {}
                CompBinding::Index(index) => {
                    if let Some((id, name)) = id_and_name_at(*index) {
                        if name == entry.layer_name {
                            entry.layer_id = id;
                        } else {
                            held.push(LayerCompEntry {
                                layer_id: id,
                                layer_name: name,
                                visible: None,
                                offset: None,
                                appearance: None,
                            });
                        }
                    }
                }
                CompBinding::Id(id) => {
                    if has_id(*id) {
                        entry.layer_id = *id;
                    }
                }
            }
        }
        comp.entries.extend(held);
    }
}
