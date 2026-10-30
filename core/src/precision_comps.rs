//! LAYER COMPS at high depth: `doc_comps`'s four methods on
//! `PrecisionDocument`, plus the `CompEdit` the typed request dispatch wraps.
//!
//! Every RULE is `doc_comps`'s — the binding rule, the capture rule, the
//! apply rule, the LOCK rule, the purity rule, the name/comment/count
//! validation and the payload cap all come from there, and this file only
//! restates them against typed layers. The GEOMETRY rule (a whole-canvas op
//! remaps each bound raster entry's recorded offset) is shared too: it lives
//! in `comps_geometry` and `precision_geometry` calls it. Since 12E-1b a
//! typed layer carries `locks` (`precision_lock`), so `comp_entry_locked` is
//! a real question at both depths and `locked` is no longer always false.
//! The one remaining difference is recorded here and nowhere else:
//!
//! - The typed style is the layer's two fields merged (`reported_style`), set
//!   back through `with_layer_style`. A FLOAT document holds no style at all,
//!   so a recorded style is SKIPPED there rather than refusing the whole
//!   apply; an RGB adjustment record takes it like a raster (a
//!   CMYK one refuses it, and is skipped the same way). A GRAYSCALE document
//!   skips the same way a recorded blend mode or style the mode cannot hold
//!   (`gray_comps::admits_mode` / `admits_style`, #547 phase 6a-ii), and a
//!   LAB document the same way (`lab_comps::admits_mode` / `admits_style`,
//!   #547 Lab phase 6a-ii).
//!
//! The index vocabulary is the flattened postorder record index
//! `PrecisionDocument::layer` uses — `LLBL`'s and `LCLP`'s convention.
//!
//! The UNRECORDED rule is `doc_comps`'s too: apply hides every record
//! no entry names unless it is a `Group` / `PassThrough` (`comps_unrecorded`).

use crate::blend::BlendMode;
use crate::doc_comps::{
    refuse_duplicate_name, refuse_oversize_payload, reordered, report_value, validated_comment,
    validated_name, CompAppearance, CompCaptures, CompReport, CompRequest, LayerComp,
    LayerCompEntry, UnrecordedLayer, MAX_LAYER_COMPS, SHIFT_RANGE_REFUSAL,
};
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{admit, BOOKKEEPING_BYTES};
use crate::style::LayerStyle;

impl PrecisionDocument {
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
                Some(UnrecordedLayer {
                    index,
                    id: self.layer_id(index)?,
                    name: self.layer(index)?.name().to_string(),
                })
            })
            .collect();
        Some(report)
    }

    /// The record indices applying `comp` hides — `doc_comps`' unrecorded
    /// rule over typed records (`comps_unrecorded`).
    fn unrecorded_positions(&self, comp: &LayerComp) -> Vec<usize> {
        let layers: Vec<(u64, bool)> = (0..self.layer_count())
            .map(|index| {
                let group = self.layer(index).is_some_and(|layer| {
                    matches!(layer.content, Content::Group(_) | Content::PassThrough(_))
                });
                (self.layer_id(index).unwrap_or(0), group)
            })
            .collect();
        crate::comps_unrecorded::unrecorded_positions(comp, &layers)
    }

    /// Whether this bound entry's recorded POSITION is one a lock forbids —
    /// `doc_comps::comp_entry_locked` over typed records.
    ///
    /// Computed BEFORE the apply, because a lock refusal and a zero-length
    /// move are not distinguishable afterwards (`doc_lock.rs`). An entry
    /// recording the offset the record already has is not locked: nothing
    /// would move. Only a RASTER record has an offset to move at all.
    pub(crate) fn comp_entry_locked(&self, entry: &LayerCompEntry) -> bool {
        let Some(offset) = entry.offset else {
            return false;
        };
        let Some(index) = self.layer_index_by_id(entry.layer_id) else {
            return false;
        };
        let Some(layer) = self.layer(index) else {
            return false;
        };
        matches!(layer.content, Content::Raster { .. })
            && layer.offset() != offset
            && self.lock_block(index, crate::doc_lock::EditKind::Position) != 0
    }

    /// Every comp in the REPORT form, as JSON — the byte document's twin.
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

    /// Records EVERY record under `captures`. Position is recorded for a
    /// RASTER record only: a group and an adjustment have no buffer, so
    /// their offset is a phantom.
    fn captured_entries(&self, captures: CompCaptures) -> Vec<LayerCompEntry> {
        (0..self.layer_count())
            .filter_map(|index| {
                let layer = self.layer(index)?;
                Some(LayerCompEntry {
                    layer_id: layer.id(),
                    layer_name: layer.name().to_string(),
                    visible: captures.visibility.then_some(layer.visible()),
                    offset: (captures.position && matches!(layer.content, Content::Raster { .. }))
                        .then_some(layer.offset()),
                    appearance: captures.appearance.then(|| CompAppearance {
                        opacity: layer.opacity(),
                        blend: layer.blend_mode().to_c() as u32,
                        style: layer.reported_style(),
                    }),
                })
            })
            .collect()
    }

    /// Runs one comp operation. `Ok(None)` is the exact no-op.
    pub fn edit_layer_comps(&self, request: &CompRequest) -> Result<Option<Self>, String> {
        self.edit_layer_comps_capped(request, crate::comps_native::MAX_COMPS_JSON_LEN)
    }

    /// [`Self::edit_layer_comps`] with the payload cap as a parameter.
    pub(crate) fn edit_layer_comps_capped(
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
                    next.entries
                        .retain(|e| self.layer_index_by_id(e.layer_id).is_some());
                    for entry in &mut next.entries {
                        if let Some(index) = self.layer_index_by_id(entry.layer_id) {
                            if let Some(layer) = self.layer(index) {
                                entry.layer_name = layer.name().to_string();
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
                if *layer >= self.layer_count() {
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
                let canvas = self.dimensions();
                let float = self.is_float();
                let gray = self.gray_policy().cloned();
                let lab = self.is_lab();
                let mut current = self.clone();
                let mut changed = false;
                for entry in &comp.entries {
                    let Some(index) = current.layer_index_by_id(entry.layer_id) else {
                        continue;
                    };
                    // A position a lock forbids is SKIPPED and reported, not
                    // a failure — `doc_comps`' rule, now real at depth.
                    let locked = current.comp_entry_locked(entry);
                    let old = current
                        .layer(index)
                        .ok_or("high-depth layer index is out of range")?
                        .clone();
                    let mut layer = old.clone();
                    if let Some(visible) = entry.visible {
                        layer.visible = visible;
                    }
                    if let Some((x, y)) = entry.offset {
                        if !locked {
                            if let Content::Raster { offset, .. } = &mut layer.content {
                                *offset = (x, y);
                            }
                        }
                    }
                    if let Some(appearance) = &entry.appearance {
                        layer.opacity = crate::doc::sane_opacity(appearance.opacity);
                        if let Some(mode) = BlendMode::from_c(appearance.blend as i32).filter(|m| {
                            gray.as_deref()
                                .is_none_or(|p| crate::gray_comps::admits_mode(p, *m))
                                && (!lab || crate::lab_comps::admits_mode(*m))
                        }) {
                            // An incompatible mode refuses without touching
                            // anything else, exactly as the byte apply leaves
                            // a group-only mode on a leaf where it was: the
                            // recorded mode simply does not move.
                            if let Ok(next) = layer.clone().with_blend_mode(mode) {
                                layer = next;
                            }
                        }
                        // A FLOAT document holds no style, so a recorded
                        // style is skipped there rather than refusing the
                        // whole apply; an adjustment takes one.
                        if !float {
                            let style = match &appearance.style {
                                Some(json) => Some(LayerStyle::from_json(json)?),
                                None => None,
                            };
                            // A Gray or Lab document skips a style its mode
                            // cannot hold, as a float one skips every style.
                            let held = match (gray.as_deref(), style.as_ref()) {
                                (Some(p), Some(s)) => crate::gray_comps::admits_style(p, s),
                                (None, Some(s)) if lab => crate::lab_comps::admits_style(s),
                                _ => true,
                            };
                            if let Some(Ok(next)) =
                                held.then(|| layer.clone().with_layer_style(style.as_ref(), canvas))
                            {
                                layer = next;
                            }
                        }
                    }
                    // `replacing_layer` has no no-op latch of its own, so the
                    // purity rule is enforced here, over exactly the five
                    // values a comp can move.
                    if layer.visible() == old.visible()
                        && layer.opacity() == old.opacity()
                        && layer.blend_mode() == old.blend_mode()
                        && layer.offset() == old.offset()
                        && layer.reported_style() == old.reported_style()
                    {
                        continue;
                    }
                    if let Some(next) = current.replacing_layer(index, layer)? {
                        current = next;
                        changed = true;
                    }
                }
                // Every non-group record no entry names is hidden, lock-exempt.
                // `replacing_layer` has no latch, so an already hidden
                // record is skipped here.
                for index in current.unrecorded_positions(comp) {
                    let Some(old) = current.layer(index) else {
                        continue;
                    };
                    if !old.visible() {
                        continue;
                    }
                    let mut layer = old.clone();
                    layer.visible = false;
                    if let Some(next) = current.replacing_layer(index, layer)? {
                        current = next;
                        changed = true;
                    }
                }
                Ok(changed.then_some(current))
            }
        }
    }

    fn refuse_oversize_comps(&self, cap: u64) -> Result<(), String> {
        let payload = crate::comps_native::payload(&self.comps, |id| {
            Some(self.layer(self.layer_index_by_id(id)?)?.name().to_string())
        })?;
        refuse_oversize_payload(payload.len(), cap)
    }
}

/// `{op:"layer_comp",action:…}` on a typed document — `StructureEdit`'s
/// shape, wrapping the one shared request parser so the two depths cannot
/// word one schema two ways.
pub(crate) struct CompEdit(CompRequest);

impl CompEdit {
    /// The ONE request parser (`doc_comps::CompRequest::parse`), so the two
    /// depths cannot word one schema two ways.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        Ok(Self(CompRequest::parse(text)?))
    }

    /// Bounded bookkeeping only: a comp holds no pixels, so the work is the
    /// comp table plus the one rebuilt ancestor path `replacing_layer` copies.
    pub(crate) fn scratch_bytes(&self, document: &PrecisionDocument) -> Result<u64, String> {
        let names: u64 = (0..document.layer_count())
            .filter_map(|i| document.layer(i))
            .map(|layer| layer.name().len() as u64)
            .sum();
        let comps: u64 = document
            .layer_comps()
            .iter()
            .map(|comp| {
                comp.name.len() as u64
                    + comp.comment.len() as u64
                    + comp
                        .entries
                        .iter()
                        .map(|e| {
                            e.layer_name.len() as u64
                                + e.appearance
                                    .as_ref()
                                    .and_then(|a| a.style.as_ref())
                                    .map_or(0, |s| s.len() as u64)
                                + 64
                        })
                        .sum::<u64>()
            })
            .sum();
        Ok(4 * BOOKKEEPING_BYTES
            + (names + comps) * 4
            + document.layer_count() as u64 * 4096
            + document.channels().len() as u64 * 1024)
    }

    pub(crate) fn apply(
        &self,
        document: &PrecisionDocument,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        let scratch = self.scratch_bytes(document)?;
        admit(document.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        job.check()?;
        let result = document.edit_layer_comps(&self.0)?;
        job.check()?;
        Ok(result)
    }
}
