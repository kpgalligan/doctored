//! LAYER COMPS through whole-canvas GEOMETRY, at both depths.
//!
//! **The rule.** A whole-canvas geometry op — Crop, Canvas Size, Image Size,
//! the three quarter turns, the two flips, Rotate Canvas and Straighten —
//! remaps every BOUND raster entry's recorded offset to the offset the op
//! would have given that layer had it sat at the recorded offset. The
//! recording is then still a statement about the picture the user sees, so
//! applying the comp after the op puts the layer where applying it before the
//! op would have put it, and a position-locked layer that sits where its comp
//! says is not reported `locked` merely because the canvas moved under it.
//!
//! This module holds NO per-op geometry. Each op passes the very expression
//! its own layer loop uses — `doc::translated_offset`, `doc::scaled_offset`,
//! `geometry()`'s closure, `transformed_plane_bounds`, the typed
//! `plan::whole` — so the comp and the layer cannot disagree by construction.
//! `transformed_plane_bounds` is the box `doc_transform::transformed_bounds`
//! computes for a layer (same four corners, same `bounds_of_corners`), which
//! is why the byte straighten can remap a recorded offset without a `Layer`
//! sitting there.
//!
//! **Retained sources.** A layer whose retained source is effective is
//! re-rendered by a filtered op, and its new offset is the `floor` of the
//! source bounds rather than the plain rule's `round` — the two differ by a
//! pixel (a resize from 8×8 to 11×9 takes an entry at (2, 2) to (2, 2)
//! where the plain rule says (3, 2)). So when the op re-renders, a sourced
//! entry is routed through [`sourced_offset`], which is exactly what
//! `effective_source` plus `render8` / `render16` give for the layer moved to
//! the recorded offset. The exact turns and flips keep the plain offsets for
//! sourced layers too (`exact_source_geometry`), so they pass no re-render.
//!
//! **The guarantee is per op.** One op remaps exactly. A CHAIN of filtered
//! ops can drift by ±1 px, because after a filtered op a layer's SIZE
//! depends on its sub-pixel phase, and the comp's layer (the one that never
//! sat at the recorded offset) was never resampled at that phase.
//!
//! **What stays unchanged:** a stale entry (no layer to ask), an entry with
//! `offset: None` (a group, or a comp that does not capture position), a
//! byte ADJUSTMENT under Straighten (`transformed_entries` leaves it where it
//! is), visibility, opacity and blend, and the recorded style — except under
//! Image Size, and under a Straighten whose matrix scales, where the style
//! is scaled by exactly the factor the live style is.
//!
//! **Host-regenerated layers.** Text and shape layers are
//! re-rendered or refitted by the HOST after a whole-canvas op (their
//! descriptions are regenerated in Swift), so their live offset can differ
//! from the pixel rule this module applies by a pixel or two. The host then
//! sends one `shift` comp request per such layer (`CompRequest::Shift`, the
//! host's offset minus the core's), and [`DrDocument::comps_shifted`] /
//! [`PrecisionDocument::comps_shifted`] move that layer's recorded offsets by
//! exactly that delta, so applying the comp after the op leaves the layer
//! where the host put it and no false `locked` appears.
//!
//! **INVARIANT: every new whole-canvas geometry op builds its comps through
//! `remapped_comps`.** A literal `comps: self.comps.clone()` in a
//! whole-canvas op breaks the rule.
//!
//! **Storage growth.** A per-layer growth moves one raster's
//! buffer ORIGIN without moving its content: Edit ▸ Stroke…'s layer edge and
//! the retouch expansion (`retouch_transform::grown_layer`), native paint,
//! bucket fill and gradient past the layer's left or top edge, a typed
//! retouch with Expand destination, and a selection moved or transformed
//! past the layer's left or top edge, at both depths. A recorded offset is an
//! absolute buffer origin, so each of them shifts that layer's bound entries
//! by `origin − offset` through [`DrDocument::comps_after_growth`] /
//! [`PrecisionDocument::comps_after_growth`], the one place the shift is
//! written; otherwise the next Apply would misplace the content. A growth
//! that keeps the origin (right or down only) leaves comps identical.
//!
//! **Merges.** Merge Down, Merge Layers and Merge Visible keep the
//! LOWEST merged entry's id (`doc_identity`), so a comp row bound to that id
//! now describes the merged layer. The merge moves that layer's buffer origin
//! — to the union extent at 8 bits, to (0, 0) for Merge Visible and every
//! typed merge — without moving its content, which is the growth case above:
//! the kept entry's recorded offsets shift through the same
//! `comps_after_growth`, so applying the comp puts the lower layer's content
//! where it was recorded and the baked upper content moves with it. The
//! merged layer is Normal at opacity 1 with no style, its pixels carrying the
//! old appearance baked in, so [`merged_comps`] also rewrites every recorded
//! APPEARANCE of the kept id to exactly that — what Update would record right
//! after the merge — or an Apply would bake it a second time. Entries for the
//! merged-away ids stay as they are (stale by id), and visibility is never
//! touched.
//!
//! **Comps follow whole-canvas GEOMETRY (`comps_geometry`)**: every whole-canvas op (Crop,
//! Canvas Size, Image Size, the turns and flips, Rotate Canvas, Straighten) remaps each BOUND
//! RASTER entry's recorded offset to the offset the op would have given the layer had it sat there,
//! through the op's OWN expression (`doc::translated_offset` / `doc::scaled_offset`, `geometry()`'s
//! closure, `transformed_plane_bounds`, the typed `plan::whole`), so apply-after-op equals
//! apply-before-op; an entry on a layer with an effective retained source goes through the source
//! bounds (`sourced_offset` — the plain rule's `round` is 1 px off the source's `floor`),
//! Image Size and a scaling straighten scale a recorded style by the live style's factor, and
//! stale, group, position-less and straightened byte-adjustment entries are untouched. The
//! guarantee is per op (a filtered chain may drift ±1 px); a host-regenerated text or shape
//! layer's entries then follow the host's offset through the `shift` request
//! (`comps_shifted`). **Comps follow per-layer STORAGE GROWTH at both depths**: a growth
//! that moves one raster's buffer ORIGIN without moving its content shifts that layer's bound
//! recorded offsets by `origin − offset` through `comps_after_growth` (`DrDocument` /
//! `PrecisionDocument`, over `remapped_comps`), the ONE place that shift is written — called by
//! `retouch_transform::grown_layer` (Edit ▸ Stroke…'s layer edge,
//! `dr_doc_expand_retouch_destination`), `precision_paint_tools` (paint, bucket fill, gradient past
//! the left/top edge), `precision_retouch`'s grown `expand_destination` extent and the
//! `MovedSamples::Plane` arm of `selected_pixels` / `precision_selected_pixels` (move and transform
//! of the selected pixels past the left/top edge), and the three MERGES at both depths for the kept
//! entry (through `merged_comps`, which also resets that entry's recorded appearance to the
//! merged layer's Normal/1/no style; `core/tests/merge_comps.rs`).

use std::collections::HashMap;
use std::sync::Arc;

use crate::blend::BlendMode;
use crate::doc::{DrDocument, Layer, LayerKind};
use crate::doc_comps::{CompAppearance, LayerComp};
use crate::doc_transform::{transformed_plane_bounds, Affine};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::source_transform::{self, SourceTransform};
use crate::style::{scaled_style, LayerStyle};

/// What one op does to one bound entry: its new offset (`None` leaves the
/// recorded one alone) and the factor its recorded style scales by.
pub(crate) struct EntryRemap {
    pub(crate) offset: Option<(i32, i32)>,
    pub(crate) style_factor: f64,
}

/// `comps` with every entry `remap` answers for rewritten. `remap(id,
/// recorded)` answers `None` for an unbound entry, which stays identical. An
/// offset is replaced only when the entry recorded one AND the remap has one;
/// a recorded style is scaled only when `scaled_style` hands back a NEW
/// allocation, and one that no longer parses is left as it was. Name, id,
/// visibility, opacity and blend are never touched.
pub(crate) fn remapped(
    comps: &[LayerComp],
    remap: impl Fn(u64, Option<(i32, i32)>) -> Option<EntryRemap>,
) -> Vec<LayerComp> {
    comps
        .iter()
        .map(|comp| {
            let mut comp = comp.clone();
            for entry in &mut comp.entries {
                let Some(next) = remap(entry.layer_id, entry.offset) else {
                    continue;
                };
                if let (Some(_), Some(offset)) = (entry.offset, next.offset) {
                    entry.offset = Some(offset);
                }
                let Some(appearance) = &mut entry.appearance else {
                    continue;
                };
                let Some(json) = &appearance.style else {
                    continue;
                };
                let Ok(style) = LayerStyle::from_json(json) else {
                    continue;
                };
                let current = Arc::new(style);
                if let Some(scaled) = scaled_style(&Some(Arc::clone(&current)), next.style_factor) {
                    if !Arc::ptr_eq(&current, &scaled) {
                        appearance.style = Some(scaled.to_json());
                    }
                }
            }
            comp
        })
        .collect()
}

/// The offset a re-rendered retained source lands at when its layer sits at
/// `recorded`: the top-left of the source bounds under `incremental` composed
/// with the STORED transform translated by `recorded − rendered_offset` —
/// `effective_source`'s translation followed by `render8` / `render16`'s
/// bounds. `None` on any refusal, which leaves the recorded offset alone.
pub(crate) fn sourced_offset(
    asset: (u32, u32),
    transform: SourceTransform,
    rendered_offset: (i32, i32),
    recorded: (i32, i32),
    incremental: SourceTransform,
) -> Option<(i32, i32)> {
    let at = source_transform::translated(
        transform,
        f64::from(recorded.0) - f64::from(rendered_offset.0),
        f64::from(recorded.1) - f64::from(rendered_offset.1),
    )
    .ok()?;
    let accumulated = source_transform::compose(incremental, at).ok()?;
    let (x, y, _, _) = source_transform::bounds(asset, accumulated).ok()?;
    Some((x, y))
}

impl DrDocument {
    /// This document's comps remapped through one whole-canvas op. `offset`
    /// is the op's own rule for a raster at a recorded offset; `rerender` is
    /// the op's incremental source map when it re-renders retained sources
    /// (`None` for the ops that keep their offsets plain); `style_factor` is
    /// the factor the op scales that layer's live style by.
    pub(crate) fn remapped_comps(
        &self,
        offset: impl Fn(&Layer, (i32, i32)) -> Option<(i32, i32)>,
        rerender: Option<SourceTransform>,
        style_factor: impl Fn(&Layer) -> f64,
    ) -> Vec<LayerComp> {
        if self.comps.is_empty() {
            return Vec::new();
        }
        let index: HashMap<u64, usize> = self
            .layers
            .iter()
            .enumerate()
            .map(|(i, layer)| (layer.id, i))
            .collect();
        remapped(&self.comps, |id, recorded| {
            let layer = self.layers.get(*index.get(&id)?)?;
            let offset = match recorded {
                Some(rec) if layer.kind == LayerKind::Raster => {
                    let source = rerender.and_then(|incremental| {
                        layer.effective_source(&self.profile)?;
                        Some((incremental, layer.retained_source.as_ref()?))
                    });
                    match source {
                        Some((incremental, source)) => sourced_offset(
                            source.asset.dimensions(),
                            source.transform,
                            source.rendered_offset,
                            rec,
                            incremental,
                        ),
                        None => offset(layer, rec),
                    }
                }
                _ => None,
            };
            Some(EntryRemap {
                offset,
                style_factor: style_factor(layer),
            })
        })
    }

    /// Comps with every bound entry of raster `index` that recorded an
    /// offset shifted by `delta`, through `remapped_comps` — the one place a
    /// per-layer shift is written (a growth, a merge, and the host's `shift`
    /// request after it re-renders a described layer). `None` = leave
    /// comps as they are: no comps, `index` not a raster, `delta == (0, 0)`,
    /// or the result equals `self.comps`. An entry whose shifted offset
    /// leaves i32 stays as recorded (`shifted`).
    pub(crate) fn comps_shifted(&self, index: usize, delta: (i64, i64)) -> Option<Vec<LayerComp>> {
        if self.comps.is_empty() || delta == (0, 0) {
            return None;
        }
        let id = self.raster_layer(index)?.id;
        let next = self.remapped_comps(
            |l, rec| (l.id == id).then(|| shifted(rec, delta)).flatten(),
            None,
            |_| 1.0,
        );
        (next != self.comps).then_some(next)
    }

    /// Comps after layer `index`'s buffer origin moves to `origin` without
    /// moving its content (a storage growth or a merge, module doc): each bound entry of
    /// that layer that recorded an offset shifts by `origin − layer.offset`,
    /// through [`Self::comps_shifted`]. `None` = leave comps as they are: no comps,
    /// `index` not a raster, `origin == layer.offset`, or no bound entry
    /// recorded an offset ([`Self::comps_shifted`]). `self` is the
    /// document BEFORE the growth.
    pub(crate) fn comps_after_growth(
        &self,
        index: usize,
        origin: (i32, i32),
    ) -> Option<Vec<LayerComp>> {
        let layer = self.raster_layer(index)?;
        let delta = growth_delta(&self.comps, layer.offset, origin)?;
        self.comps_shifted(index, delta)
    }

    /// The comps `straighten_layers(m)` leaves: every non-adjustment raster
    /// through `transformed_plane_bounds` (its retained source, if any,
    /// re-rendered under `m`) and its style scaled by `sqrt(|det m|)`, as
    /// `transform_layer_unlocked_with_job` scales the live one. An adjustment
    /// is left where it is, as `transformed_entries` leaves it.
    pub(crate) fn straightened_comps(&self, m: &Affine) -> Vec<LayerComp> {
        let moves = |layer: &Layer| {
            layer.kind == LayerKind::Raster
                && !crate::doc_geometry_set::is_adjustment(layer.meta.as_deref())
        };
        self.remapped_comps(
            |layer, rec| {
                if !moves(layer) {
                    return None;
                }
                transformed_plane_bounds(layer.pixels.dimensions(), rec, m).map(|b| (b.0, b.1))
            },
            Some(SourceTransform::Affine([m.a, m.b, m.c, m.d, m.tx, m.ty])),
            |layer| {
                if moves(layer) {
                    m.determinant().abs().sqrt()
                } else {
                    1.0
                }
            },
        )
    }
}

impl PrecisionDocument {
    /// [`DrDocument::remapped_comps`] over typed records, addressed by the
    /// postorder record index `PrecisionDocument::layer` uses.
    pub(crate) fn remapped_comps(
        &self,
        offset: impl Fn(usize, (i32, i32)) -> Option<(i32, i32)>,
        rerender: Option<SourceTransform>,
        style_factor: impl Fn(usize) -> f64,
    ) -> Vec<LayerComp> {
        if self.comps.is_empty() {
            return Vec::new();
        }
        let mut records = Vec::with_capacity(self.records);
        postorder(&self.layers, &mut records);
        let index: HashMap<u64, usize> = records
            .iter()
            .enumerate()
            .map(|(i, layer)| (layer.id(), i))
            .collect();
        remapped(&self.comps, |id, recorded| {
            let i = *index.get(&id)?;
            let layer = records.get(i)?;
            let offset = match recorded {
                Some(rec) if matches!(layer.content, Content::Raster { .. }) => {
                    let source = rerender.and_then(|incremental| {
                        layer.effective_source()?;
                        Some((incremental, layer.retained_source.as_ref()?))
                    });
                    match source {
                        Some((incremental, source)) => sourced_offset(
                            source.asset.dimensions(),
                            source.transform,
                            source.rendered_offset,
                            rec,
                            incremental,
                        ),
                        None => offset(i, rec),
                    }
                }
                _ => None,
            };
            Some(EntryRemap {
                offset,
                style_factor: style_factor(i),
            })
        })
    }
}

impl PrecisionDocument {
    /// The typed twin of [`DrDocument::comps_shifted`]; `index` is the
    /// postorder record index `PrecisionDocument::layer` uses.
    pub(crate) fn comps_shifted(&self, index: usize, delta: (i64, i64)) -> Option<Vec<LayerComp>> {
        if self.comps.is_empty() || delta == (0, 0) {
            return None;
        }
        let layer = self.layer(index)?;
        if !matches!(layer.content, Content::Raster { .. }) {
            return None;
        }
        let next = self.remapped_comps(
            |i, rec| (i == index).then(|| shifted(rec, delta)).flatten(),
            None,
            |_| 1.0,
        );
        (next != self.comps).then_some(next)
    }

    /// The typed twin of [`DrDocument::comps_after_growth`]; `index` is the
    /// postorder record index `PrecisionDocument::layer` uses.
    pub(crate) fn comps_after_growth(
        &self,
        index: usize,
        origin: (i32, i32),
    ) -> Option<Vec<LayerComp>> {
        let layer = self.layer(index)?;
        if !matches!(layer.content, Content::Raster { .. }) {
            return None;
        }
        let delta = growth_delta(&self.comps, layer.offset(), origin)?;
        self.comps_shifted(index, delta)
    }
}

/// The comps a MERGE leaves (module doc): `shifted` — what
/// `comps_after_growth` answered for the kept entry and the merged origin —
/// or `comps` as they are when it answered `None`, with every entry bound to
/// `kept_id` that recorded an appearance rewritten to the merged layer's own:
/// opacity 1, Normal, no style. Runs whether or not the origin moved (a
/// typed merge of a layer already at (0, 0) still bakes its appearance).
/// `None` = leave comps as they are: nothing shifted and nothing reset, which
/// is every comp-less merge.
pub(crate) fn merged_comps(
    comps: &[LayerComp],
    shifted: Option<Vec<LayerComp>>,
    kept_id: u64,
) -> Option<Vec<LayerComp>> {
    let moved = shifted.is_some();
    let mut next = shifted.unwrap_or_else(|| comps.to_vec());
    let merged = CompAppearance {
        opacity: 1.0,
        blend: BlendMode::Normal.to_c() as u32,
        style: None,
    };
    let mut reset = false;
    for entry in next.iter_mut().flat_map(|comp| comp.entries.iter_mut()) {
        if entry.layer_id != kept_id {
            continue;
        }
        if let Some(appearance) = entry.appearance.as_mut().filter(|a| **a != merged) {
            *appearance = merged.clone();
            reset = true;
        }
    }
    (moved || reset).then_some(next)
}

/// `origin − offset` in i64, or `None` when there are no comps or the origin
/// did not move (nothing to remap).
fn growth_delta(comps: &[LayerComp], offset: (i32, i32), origin: (i32, i32)) -> Option<(i64, i64)> {
    if comps.is_empty() || origin == offset {
        return None;
    }
    Some((
        i64::from(origin.0) - i64::from(offset.0),
        i64::from(origin.1) - i64::from(offset.1),
    ))
}

/// A recorded offset shifted by `delta`; `None` (left alone) when
/// the shifted offset leaves i32.
fn shifted(rec: (i32, i32), (dx, dy): (i64, i64)) -> Option<(i32, i32)> {
    let x = i32::try_from(i64::from(rec.0) + dx).ok()?;
    let y = i32::try_from(i64::from(rec.1) + dy).ok()?;
    Some((x, y))
}

/// Every record in POSTORDER — a group after its children — which is the
/// numbering `PrecisionDocument::layer` indexes.
fn postorder<'a>(layers: &'a [PrecisionDocumentLayer], out: &mut Vec<&'a PrecisionDocumentLayer>) {
    for layer in layers {
        if let Some(children) = layer.children() {
            postorder(children, out);
        }
        out.push(layer);
    }
}
