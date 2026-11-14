//! LAYER COMPS: named recordings of per-layer VISIBILITY, POSITION and
//! APPEARANCE, and the ops that save, update, delete, apply and reorder them.
//!
//! A comp is deliberately NOT a snapshot. A snapshot (the host's 5D feature)
//! holds the whole document — pixels, selection, edit target — and restoring
//! one puts every byte back. A comp holds NO pixels, no selection and no
//! history: it is a table of small per-layer facts, recorded by STABLE
//! IDENTITY (`doc_identity`), so applying one is an ordinary property edit
//! that the undo stack handles like any other and that a save carries as a
//! few hundred bytes of JSON rather than as a second copy of the picture.
//!
//! **Which attributes a comp captures is its own, fixed at save.** The three
//! flags are visibility, position and appearance. A flag that is
//! off records nothing for any layer, and applying the comp leaves that
//! attribute exactly as it is. `position` is recorded for RASTER entries
//! only: a group has no buffer of its own (its `offset` is a phantom derived
//! from its descendants, `DrDocument::with_layer_offset`) and a typed
//! adjustment has none either, so recording one would record a number the
//! writer never meant.
//!
//! **The binding rule.** An entry names a layer by its session identity. An
//! entry whose id no longer matches any layer is STALE: it is reported by the
//! name the layer carried when the comp was saved, and applying the comp
//! skips it. Deleting, merging or flattening a layer is what makes an entry
//! stale; a RENAME does not, because the id carries through it. A layer NO
//! entry names is UNRECORDED, and applying the comp HIDES it unless it is a
//! group, at any depth, whatever the comp captured and lock-exempt —
//! measured against Photoshop (the predicate lives in
//! `comps_unrecorded`).
//!
//! **The lock rule.** Visibility and appearance are lock-EXEMPT, like a
//! rename and a colour label (`doc_label`): they change no pixel and no
//! coordinate. POSITION is not — it is exactly the edit `LOCK_POSITION`
//! exists to forbid — so a bound entry whose recorded offset differs from
//! the layer's current one on a position-locked layer is reported `locked`
//! and its offset is SKIPPED, while the same entry's visibility and
//! appearance still apply. The lock is read with `lock_block` BEFORE the
//! setter is called, because `under_locks` answers a refusal with the same
//! `None` a zero-length move answers (`doc_lock.rs`), so the setter itself
//! can never tell the host which of the two happened.
//!
//! **Purity.** Every op returns `None` for an exact no-op, so `applyEdit`
//! never registers a phantom undo step.
//!
//! **Geometry.** A comp follows the canvas: every whole-canvas geometry op
//! (Crop, Canvas Size, Image Size, the turns and flips, Rotate Canvas,
//! Straighten) remaps each bound raster entry's recorded offset to where the
//! op would have put a layer sitting there, and Image Size (or a scaling
//! straighten) scales a recorded style as it scales the live one. The rule
//! and its guarantees live in `comps_geometry`; a layer the host re-renders
//! after the op (text, shape) has its entries moved to the
//! host's offset by the `shift` request; per-layer transforms leave
//! comps alone.
//!
//! Persistence is the conditional `comps_native` wrapper (byte 43 / typed
//! 44); a document with no comp is byte-for-byte the file the prior writer
//! made.
//!
//! `DrDocument.comps` / `PrecisionDocument.comps`, named recordings of per-layer VISIBILITY,
//! POSITION and APPEARANCE by stable identity (`doc_identity`) and never pixels, the selection or
//! the history — that last clause is what keeps them distinct from 5D's snapshots. `doc_comps` owns
//! EVERY rule and `precision_comps` restates them against typed layers: the three per-comp capture
//! flags; the whole-document capture (every layer gets a row, `offset` for RASTER entries only);
//! the binding rule (an entry whose id is gone is STALE and reported by its recorded NAME, a rename
//! keeps it bound; an UNRECORDED non-group layer, one no entry names, is hidden on apply at any
//! depth whatever the capture set, groups exempt — `comps_unrecorded`); the lock rule
//! (visibility and appearance are lock-EXEMPT like a label; POSITION honours `LOCK_POSITION`
//! through `lock_block` read BEFORE the setter, because `under_locks` hides a refusal in the
//! same `None` a zero move answers — a locked entry is reported, never a failure —
//! and since 12E-1b a TYPED layer carries the same locks, so typed `locked` is a real answer too);
//! the purity rule; and the 64-comp / 128-byte name / 1024-byte comment / 8 MiB payload caps, the
//! last checked on the COMP op so the refusal never lands on Save. `CompRequest::parse` is the ONE
//! request parser both depths share
//! (`{"op":"layer_comp","action":"save|update|delete|apply|reorder|shift", …}`; a reorder's `to`
//! is the FINAL index, `comp == to` the exact no-op; a shift moves one layer's recorded offsets by
//! `dx`/`dy`, lock-exempt bookkeeping the host sends after it re-renders a described layer),
//! and naming ANY capture flag states the whole set.
//! `rebind` runs after construction — at the typed depth on BOTH reader exits, because the pending
//! nodes have no ids until `materialize` mints them. PSD comps (resource 1065 and each record's
//! `cmls`) cross in every mode the PSD codecs carry layers in, with their names, comments, capture
//! flags, visibility and appearance (`psd_comps`; a recorded position is written and not imported,
//! and a style's Blend If, knockout and interior flag stay the layer's own):
//! a layer's settings ride in its own record, so an imported entry binds to the layer built from
//! that record.

use serde::Deserialize;

use crate::blend::BlendMode;
use crate::doc::{DrDocument, LayerKind};
use crate::doc_lock::EditKind;
use crate::style::LayerStyle;

/// The most comps one document may hold. A comp is a table over every layer,
/// so the cost is comps × layers; 64 is generous and small enough that the
/// whole list stays a few hundred KiB.
pub const MAX_LAYER_COMPS: usize = 64;
/// The longest comp name, in bytes.
pub const MAX_COMP_NAME_LEN: usize = 128;
/// The longest comp comment, in bytes.
pub const MAX_COMP_COMMENT_LEN: usize = 1024;

/// Which attributes a comp records: the three flags, and the comp's own —
/// two comps of one document may capture different subsets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompCaptures {
    pub visibility: bool,
    pub position: bool,
    pub appearance: bool,
}

impl Default for CompCaptures {
    /// All three, which is what a Save with no flags means.
    fn default() -> Self {
        Self {
            visibility: true,
            position: true,
            appearance: true,
        }
    }
}

impl CompCaptures {
    /// True when the comp records nothing at all — a comp that could never
    /// change anything, which Save and Update both refuse.
    pub fn is_empty(&self) -> bool {
        !self.visibility && !self.position && !self.appearance
    }
}

/// One entry's recorded APPEARANCE: the three values a layer's own
/// compositing reads that are not its pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct CompAppearance {
    /// Layer opacity, 0…1.
    pub opacity: f32,
    /// The blend mode's wire code (`BlendMode::to_c`).
    pub blend: u32,
    /// The layer style as canonical JSON (`LayerStyle::to_json`; the typed
    /// depth's `reported_style`), or `None` for a layer carrying none.
    pub style: Option<String>,
}

/// One layer's row in a comp.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerCompEntry {
    /// The layer's session identity (`doc_identity`); 0 means UNBOUND — the
    /// layer is gone, or the file this comp came from named a layer this
    /// document no longer has.
    pub layer_id: u64,
    /// The layer's name when the comp was saved (or, for a bound entry,
    /// refreshed by the last Update). The only handle a stale entry has.
    pub layer_name: String,
    /// The recorded visibility, or `None` when the comp does not capture it.
    pub visible: Option<bool>,
    /// The recorded canvas offset, or `None` when the comp does not capture
    /// position or the entry is not a raster layer.
    pub offset: Option<(i32, i32)>,
    /// The recorded appearance, or `None` when the comp does not capture it.
    pub appearance: Option<CompAppearance>,
}

/// One named comp.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerComp {
    pub name: String,
    pub comment: String,
    pub captures: CompCaptures,
    pub entries: Vec<LayerCompEntry>,
}

/// One layer applying a comp would HIDE because no entry of the comp names
/// it and it is not a group (`comps_unrecorded`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnrecordedLayer {
    pub index: usize,
    pub id: u64,
    pub name: String,
}

/// What applying a comp would do, computed without applying it: the layer
/// INDICES its bound entries name, and the NAMES of the entries it cannot
/// fully honour.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompReport {
    /// Bound entries, as current layer indices.
    pub bound: Vec<usize>,
    /// Stale entries, by the name they were recorded under.
    pub stale: Vec<String>,
    /// Bound entries whose recorded POSITION a lock forbids, by name.
    pub locked: Vec<String>,
    /// Non-group layers no entry names, ascending index, whatever their
    /// current visibility; Apply hides them.
    pub unrecorded: Vec<UnrecordedLayer>,
}

/// One comp operation.
/// `{"op":"layer_comp","action":"save|update|delete|apply|reorder|shift", …}` — the
/// same request at both depths, so the byte FFI and the typed
/// `dr_precision_doc_edit` dispatch parse one schema.
#[derive(Clone, Debug, PartialEq)]
pub enum CompRequest {
    Save {
        name: String,
        comment: String,
        captures: CompCaptures,
    },
    Update {
        comp: usize,
        name: Option<String>,
        comment: Option<String>,
        captures: Option<CompCaptures>,
        recapture: bool,
    },
    Delete {
        comp: usize,
    },
    Apply {
        comp: usize,
    },
    /// Moves comp `comp` to FINAL index `to` (remove, then insert), the
    /// `reorder_layer` convention.
    Reorder {
        comp: usize,
        to: usize,
    },
    /// Moves every recorded offset of layer `layer` (the byte index, or the
    /// typed postorder record index) by (`dx`, `dy`) in every comp — the
    /// host's follow-up after it re-renders a described layer to an offset
    /// the core's geometry rule did not give it (`comps_shifted`).
    /// Lock-exempt bookkeeping, like a growth.
    Shift {
        layer: usize,
        dx: i32,
        dy: i32,
    },
}

/// The wire form, parsed strictly. Every field is optional here and the
/// per-action requirements are checked in [`CompRequest::parse`], so a
/// missing `name` on a Save is refused with a sentence that names the action
/// rather than with serde's field message.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCompRequest {
    op: String,
    action: String,
    #[serde(default)]
    comp: Option<usize>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    comment: Option<String>,
    #[serde(default)]
    visibility: Option<bool>,
    #[serde(default)]
    position: Option<bool>,
    #[serde(default)]
    appearance: Option<bool>,
    #[serde(default)]
    recapture: Option<bool>,
    #[serde(default)]
    to: Option<usize>,
    #[serde(default)]
    layer: Option<usize>,
    #[serde(default)]
    dx: Option<i32>,
    #[serde(default)]
    dy: Option<i32>,
}

impl RawCompRequest {
    /// The capture set a request states. `None` when it names no flag at
    /// all, which on an Update means "leave the comp's own set alone".
    ///
    /// Naming ANY flag states the WHOLE set: an unnamed flag becomes `true`,
    /// the same default a Save with no flags takes. Recorded deliberately —
    /// the alternative, merging one named flag into the comp's stored set,
    /// makes the same call mean two things on two comps.
    fn captures(&self) -> Option<CompCaptures> {
        let default = CompCaptures::default();
        (self.visibility.is_some() || self.position.is_some() || self.appearance.is_some()).then(
            || CompCaptures {
                visibility: self.visibility.unwrap_or(default.visibility),
                position: self.position.unwrap_or(default.position),
                appearance: self.appearance.unwrap_or(default.appearance),
            },
        )
    }
}

impl CompRequest {
    /// Parses the request. `Err` names the field or the action.
    pub fn parse(text: &str) -> Result<Self, String> {
        let raw: RawCompRequest =
            serde_json::from_str(text).map_err(|e| format!("invalid layer comp request: {e}"))?;
        if raw.op != "layer_comp" {
            return Err("invalid layer comp request: op must be layer_comp".into());
        }
        let comp = || {
            raw.comp
                .ok_or_else(|| format!("layer comp {} requires a comp index", raw.action))
        };
        match raw.action.as_str() {
            "save" => Ok(Self::Save {
                name: raw.name.clone().ok_or("layer comp save requires a name")?,
                comment: raw.comment.clone().unwrap_or_default(),
                captures: raw.captures().unwrap_or_default(),
            }),
            "update" => Ok(Self::Update {
                comp: comp()?,
                name: raw.name.clone(),
                comment: raw.comment.clone(),
                captures: raw.captures(),
                recapture: raw.recapture.unwrap_or(true),
            }),
            "delete" => Ok(Self::Delete { comp: comp()? }),
            "apply" => Ok(Self::Apply { comp: comp()? }),
            "reorder" => Ok(Self::Reorder {
                comp: comp()?,
                to: raw
                    .to
                    .ok_or("layer comp reorder requires a destination index")?,
            }),
            "shift" => {
                let layer = raw.layer.ok_or("layer comp shift requires a layer index")?;
                let (Some(dx), Some(dy)) = (raw.dx, raw.dy) else {
                    return Err("layer comp shift requires dx and dy".into());
                };
                Ok(Self::Shift { layer, dx, dy })
            }
            other => Err(format!("unknown layer comp action {other}")),
        }
    }
}

/// Trims and validates a comp name. The name is the host's handle for a comp
/// everywhere (the window's list, the six MCP tools), so it must be present,
/// bounded and unique.
pub(crate) fn validated_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("a layer comp needs a name".into());
    }
    if trimmed.len() > MAX_COMP_NAME_LEN {
        return Err(format!(
            "a layer comp name is limited to {MAX_COMP_NAME_LEN} bytes"
        ));
    }
    Ok(trimmed.to_string())
}

/// Validates a comment. Empty is ordinary; only the bound is enforced.
pub(crate) fn validated_comment(comment: &str) -> Result<String, String> {
    if comment.len() > MAX_COMP_COMMENT_LEN {
        return Err(format!(
            "a layer comp comment is limited to {MAX_COMP_COMMENT_LEN} bytes"
        ));
    }
    Ok(comment.to_string())
}

/// The shared "that name is taken" check. Case-SENSITIVE: a comp name is a
/// user string the host echoes verbatim, and folding it would make "Hero" and
/// "hero" the same comp in one document and two in a file another build wrote.
pub(crate) fn refuse_duplicate_name(
    comps: &[LayerComp],
    name: &str,
    except: Option<usize>,
) -> Result<(), String> {
    if comps
        .iter()
        .enumerate()
        .any(|(i, c)| Some(i) != except && c.name == name)
    {
        return Err(format!("a layer comp called \"{name}\" already exists"));
    }
    Ok(())
}

/// The comp list with comp `comp` moved to FINAL index `to` (remove, then
/// insert), shared by the byte and the typed op. `Ok(None)` when `comp == to`,
/// the exact no-op; `Err` names the out-of-range index, `comp` checked first.
/// No payload-cap check (the same bytes in another order) and no lock check
/// (the order is bookkeeping, like a delete).
pub(crate) fn reordered(
    comps: &[LayerComp],
    comp: usize,
    to: usize,
) -> Result<Option<Vec<LayerComp>>, String> {
    if comp >= comps.len() {
        return Err("layer comp index is out of range".into());
    }
    if to >= comps.len() {
        return Err(format!(
            "layer comp destination index {to} is out of range (0..{})",
            comps.len() - 1
        ));
    }
    if comp == to {
        return Ok(None);
    }
    let mut next = comps.to_vec();
    let moved = next.remove(comp);
    next.insert(to, moved);
    Ok(Some(next))
}

/// The refusal a `shift` naming a layer the document does not have earns.
/// Named here so the byte and the typed op word it identically.
pub(crate) const SHIFT_RANGE_REFUSAL: &str = "layer comp shift layer index is out of range";

/// The refusal a comp list too large for the `.rz` payload earns. Named here
/// so the byte and the typed op word it identically.
pub(crate) const PAYLOAD_CAP_REFUSAL: &str = "layer comps exceed the .doctored payload cap";

/// `Err` when this comp list would not fit the `.rz` payload cap, so the
/// refusal lands on the comp op that grew it and never on the Save that would
/// otherwise be the first call unable to write the file.
pub(crate) fn refuse_oversize_payload(payload_len: usize, cap: u64) -> Result<(), String> {
    if payload_len as u64 > cap {
        return Err(PAYLOAD_CAP_REFUSAL.into());
    }
    Ok(())
}

/// The report form of one comp, shared by the byte and the typed export and
/// by the MCP list. `index` is the comp's position in the list; each entry's
/// `layer` is its CURRENT bound index or null.
pub(crate) fn report_value(
    index: usize,
    comp: &LayerComp,
    report: &CompReport,
    bound_index_of: impl Fn(u64) -> Option<usize>,
    entry_locked: impl Fn(&LayerCompEntry) -> bool,
) -> serde_json::Value {
    let entries: Vec<serde_json::Value> = comp
        .entries
        .iter()
        .map(|entry| {
            let bound = bound_index_of(entry.layer_id);
            let mut row = serde_json::Map::new();
            row.insert(
                "layer".into(),
                bound.map_or(serde_json::Value::Null, |i| serde_json::json!(i)),
            );
            row.insert("id".into(), serde_json::json!(entry.layer_id));
            row.insert("name".into(), serde_json::json!(entry.layer_name));
            if let Some(visible) = entry.visible {
                row.insert("visible".into(), serde_json::json!(visible));
            }
            if let Some((x, y)) = entry.offset {
                row.insert("offset".into(), serde_json::json!([x, y]));
            }
            if let Some(appearance) = &entry.appearance {
                row.insert("opacity".into(), serde_json::json!(appearance.opacity));
                row.insert("blend".into(), serde_json::json!(appearance.blend));
                if let Some(style) = &appearance.style {
                    row.insert("style".into(), serde_json::json!(style));
                }
            }
            // Per ENTRY, never by name: two layers may share a name, and a
            // report that spread one entry's staleness over its twin would
            // name the wrong row in the window and in the MCP reply.
            row.insert("stale".into(), serde_json::json!(bound.is_none()));
            row.insert("locked".into(), serde_json::json!(entry_locked(entry)));
            serde_json::Value::Object(row)
        })
        .collect();
    let unrecorded: Vec<serde_json::Value> = report
        .unrecorded
        .iter()
        .map(|layer| serde_json::json!({"layer": layer.index, "id": layer.id, "name": layer.name}))
        .collect();
    serde_json::json!({
        "index": index,
        "name": comp.name,
        "comment": comp.comment,
        "captures": {
            "visibility": comp.captures.visibility,
            "position": comp.captures.position,
            "appearance": comp.captures.appearance,
        },
        "stale_count": report.stale.len(),
        "locked_count": report.locked.len(),
        "entries": entries,
        "unrecorded": unrecorded,
    })
}

impl DrDocument {
    /// The document's comps, in list order.
    pub fn layer_comps(&self) -> &[LayerComp] {
        &self.comps
    }

    /// What applying comp `comp` would do; `None` for an out-of-range index.
    pub fn layer_comp_report(&self, comp: usize) -> Option<CompReport> {
        let comp = self.comps.get(comp)?;
        let mut report = CompReport::default();
        for entry in &comp.entries {
            match self.layer_index_by_id(entry.layer_id) {
                Some(index) => {
                    report.bound.push(index);
                    if self.comp_entry_locked(entry) {
                        report.locked.push(entry.layer_name.clone());
                    }
                }
                None => report.stale.push(entry.layer_name.clone()),
            }
        }
        report.unrecorded = self
            .unrecorded_positions(comp)
            .into_iter()
            .filter_map(|index| {
                let layer = self.layers.get(index)?;
                Some(UnrecordedLayer {
                    index,
                    id: layer.id,
                    name: layer.name.clone(),
                })
            })
            .collect();
        Some(report)
    }

    /// The layer indices applying `comp` hides (`comps_unrecorded`'s rule).
    fn unrecorded_positions(&self, comp: &LayerComp) -> Vec<usize> {
        let layers: Vec<(u64, bool)> = self
            .layers
            .iter()
            .map(|layer| (layer.id, layer.kind == LayerKind::Group))
            .collect();
        crate::comps_unrecorded::unrecorded_positions(comp, &layers)
    }

    /// Whether this bound entry's recorded POSITION is one a lock forbids.
    ///
    /// Computed BEFORE the setter, because `under_locks` answers a lock
    /// refusal with the same `None` a zero-length move answers, so the setter
    /// cannot be asked (`doc_lock.rs`). An entry recording the offset the
    /// layer already has is not locked: nothing would move.
    pub(crate) fn comp_entry_locked(&self, entry: &LayerCompEntry) -> bool {
        let Some(offset) = entry.offset else {
            return false;
        };
        let Some(index) = self.layer_index_by_id(entry.layer_id) else {
            return false;
        };
        let Some(layer) = self.layers.get(index) else {
            return false;
        };
        layer.kind == LayerKind::Raster
            && layer.offset != offset
            && self.lock_block(index, EditKind::Position) != 0
    }

    /// Every comp in the REPORT form, as JSON. The host's one read.
    pub fn layer_comps_json(&self) -> Result<String, String> {
        let comps: Vec<serde_json::Value> = self
            .comps
            .iter()
            .enumerate()
            .map(|(index, comp)| {
                let report = self.layer_comp_report(index).unwrap_or_default();
                report_value(
                    index,
                    comp,
                    &report,
                    |id| self.layer_index_by_id(id),
                    |entry| self.comp_entry_locked(entry),
                )
            })
            .collect();
        serde_json::to_string(&serde_json::json!({
            "schema": 1, "count": comps.len(), "comps": comps
        }))
        .map_err(|e| format!("could not report layer comps: {e}"))
    }

    /// Records EVERY layer under `captures`. A comp is a whole-document
    /// table: a layer with no row would silently keep whatever state the
    /// document happened to be in, which is the one thing a comp exists to
    /// remove.
    fn captured_entries(&self, captures: CompCaptures) -> Vec<LayerCompEntry> {
        self.layers
            .iter()
            .map(|layer| LayerCompEntry {
                layer_id: layer.id,
                layer_name: layer.name.clone(),
                visible: captures.visibility.then_some(layer.visible),
                // Position for RASTER entries only: a group's offset is a
                // phantom derived from its descendants.
                offset: (captures.position && layer.kind == LayerKind::Raster)
                    .then_some(layer.offset),
                appearance: captures.appearance.then(|| CompAppearance {
                    opacity: layer.opacity,
                    blend: layer.blend.to_c() as u32,
                    style: layer.style.as_ref().map(|s| s.to_json()),
                }),
            })
            .collect()
    }

    /// Runs one comp operation. `Ok(None)` is the exact no-op; `Err` is a
    /// refusal the host shows verbatim.
    pub fn edit_layer_comps(&self, request: &CompRequest) -> Result<Option<Self>, String> {
        self.edit_layer_comps_capped(request, crate::comps_native::MAX_COMPS_JSON_LEN)
    }

    /// [`Self::edit_layer_comps`] with the payload cap as a parameter, so the
    /// refusal is reachable from a test without building an 8 MiB comp list.
    /// `#[doc(hidden)] pub` only so the black-box test
    /// `tests/layer_comps.rs` can reach it; the app goes through
    /// [`Self::edit_layer_comps`].
    #[doc(hidden)]
    pub fn edit_layer_comps_capped(
        &self,
        request: &CompRequest,
        cap: u64,
    ) -> Result<Option<Self>, String> {
        match request {
            CompRequest::Save {
                name,
                comment,
                captures,
            } => {
                if captures.is_empty() {
                    return Err("a layer comp must capture at least one attribute".into());
                }
                let name = validated_name(name)?;
                let comment = validated_comment(comment)?;
                refuse_duplicate_name(&self.comps, &name, None)?;
                if self.comps.len() >= MAX_LAYER_COMPS {
                    return Err(format!(
                        "a document is limited to {MAX_LAYER_COMPS} layer comps"
                    ));
                }
                let mut out = self.clone();
                out.comps.push(LayerComp {
                    name,
                    comment,
                    captures: *captures,
                    entries: self.captured_entries(*captures),
                });
                out.refuse_oversize_comps(cap)?;
                Ok(Some(out))
            }
            CompRequest::Update {
                comp,
                name,
                comment,
                captures,
                recapture,
            } => {
                let existing = self
                    .comps
                    .get(*comp)
                    .ok_or("layer comp index is out of range")?;
                let mut next = existing.clone();
                if let Some(name) = name {
                    let name = validated_name(name)?;
                    refuse_duplicate_name(&self.comps, &name, Some(*comp))?;
                    next.name = name;
                }
                if let Some(comment) = comment {
                    next.comment = validated_comment(comment)?;
                }
                if let Some(captures) = captures {
                    if captures.is_empty() {
                        return Err("a layer comp must capture at least one attribute".into());
                    }
                    next.captures = *captures;
                }
                if *recapture {
                    next.entries = self.captured_entries(next.captures);
                } else {
                    // Not a recapture, but a bound entry's NAME is still
                    // refreshed and a stale one dropped: an update is the
                    // moment the comp is reconciled with the document.
                    next.entries
                        .retain(|e| self.layer_index_by_id(e.layer_id).is_some());
                    for entry in &mut next.entries {
                        if let Some(index) = self.layer_index_by_id(entry.layer_id) {
                            if let Some(layer) = self.layers.get(index) {
                                entry.layer_name.clone_from(&layer.name);
                            }
                        }
                    }
                }
                if next == *existing {
                    return Ok(None);
                }
                let mut out = self.clone();
                out.comps[*comp] = next;
                out.refuse_oversize_comps(cap)?;
                Ok(Some(out))
            }
            CompRequest::Delete { comp } => {
                if *comp >= self.comps.len() {
                    return Err("layer comp index is out of range".into());
                }
                let mut out = self.clone();
                out.comps.remove(*comp);
                Ok(Some(out))
            }
            CompRequest::Reorder { comp, to } => {
                let Some(comps) = reordered(&self.comps, *comp, *to)? else {
                    return Ok(None);
                };
                let mut out = self.clone();
                out.comps = comps;
                Ok(Some(out))
            }
            CompRequest::Shift { layer, dx, dy } => {
                if *layer >= self.layers.len() {
                    return Err(SHIFT_RANGE_REFUSAL.into());
                }
                let Some(comps) = self.comps_shifted(*layer, (i64::from(*dx), i64::from(*dy)))
                else {
                    return Ok(None);
                };
                let mut out = self.clone();
                out.comps = comps;
                out.refuse_oversize_comps(cap)?;
                Ok(Some(out))
            }
            CompRequest::Apply { comp } => {
                let comp = self
                    .comps
                    .get(*comp)
                    .ok_or("layer comp index is out of range")?;
                let mut current = self.clone();
                let mut changed = false;
                for entry in &comp.entries {
                    let Some(index) = current.layer_index_by_id(entry.layer_id) else {
                        continue;
                    };
                    // The lock is read on the document as it stands, before
                    // any setter runs; a locked entry keeps its visibility
                    // and appearance and skips only the move.
                    let locked = current.comp_entry_locked(entry);
                    if let Some(visible) = entry.visible {
                        if let Some(next) = current.with_layer_visible(index, visible) {
                            current = next;
                            changed = true;
                        }
                    }
                    if let (Some((x, y)), false) = (entry.offset, locked) {
                        if let Some(next) = current.with_layer_placement(index, x, y) {
                            current = next;
                            changed = true;
                        }
                    }
                    if let Some(appearance) = &entry.appearance {
                        if let Some(next) = current.with_layer_opacity(index, appearance.opacity) {
                            current = next;
                            changed = true;
                        }
                        if let Some(mode) = BlendMode::from_c(appearance.blend as i32) {
                            // A group-only mode on a raster is refused by the
                            // setter with the same `None` an unchanged mode
                            // answers, which is exactly what should happen: a
                            // comp recorded on a group cannot move onto a
                            // leaf.
                            if let Some(next) = current.with_layer_blend_mode(index, mode) {
                                current = next;
                                changed = true;
                            }
                        }
                        let style = match &appearance.style {
                            // A CMYK-channel Blend If has no meaning on a
                            // byte layer and is dropped (#1).
                            Some(json) => Some(std::sync::Arc::new(
                                LayerStyle::from_json(json)?.without_cmyk_blend_if(),
                            )),
                            None => None,
                        };
                        if let Some(next) = current.set_layer_style(index, style) {
                            current = next;
                            changed = true;
                        }
                    }
                }
                // Every non-group layer no entry names is hidden, lock-exempt
                // like the recorded visibility above; an already hidden one
                // answers `None` and changes nothing.
                for index in current.unrecorded_positions(comp) {
                    if let Some(next) = current.with_layer_visible(index, false) {
                        current = next;
                        changed = true;
                    }
                }
                Ok(changed.then_some(current))
            }
        }
    }

    /// The payload cap, measured on the comp list this document now holds.
    fn refuse_oversize_comps(&self, cap: u64) -> Result<(), String> {
        let payload = crate::comps_native::payload(&self.comps, |id| {
            Some(self.layers.get(self.layer_index_by_id(id)?)?.name.clone())
        })?;
        refuse_oversize_payload(payload.len(), cap)
    }
}
