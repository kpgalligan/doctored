//! The typed twins of the three pure MERGE operations: Merge Layers
//! (`merge_layers`), Merge Down (`merge_down`) and Merge Visible
//! (`merge_visible`) on `PrecisionDocument`. The two PROJECTION operations
//! that sit beside them, Stamp Visible and Flatten Image, live in
//! `precision_stamp_flatten`.
//!
//! **Every rule here is the BYTE one, verbatim.** `doc_structure::merge_layers`
//! owns the multi-selection form (one shared parent, at least two members, a
//! visible LOWEST member whose locks admit a merge, and the merged entry
//! keeping that member's slot, name, identity, clipped flag, locks, link,
//! label and depth), `doc::merging_down` owns the two-adjacent-entries form
//! (the previous SIBLING is the destination, an invisible upper entry is
//! simply removed, a hidden or lock-refusing lower entry refuses the merge,
//! and a clipped upper entry bakes through its clipping), and
//! `doc_structure::merge_visible` owns the contribution form (the single
//! `contributing` predicate for both halves, at least two contributing raster
//! leaves, and the result landing at the bottom-most contributing root's
//! subtree start with that leaf's name, identity and label). Nothing about
//! what a merge MEANS is decided here. That includes the LAYER COMPS: a comp
//! bound to the kept record follows the merge exactly as at 8 bits —
//! its offsets shift to the merged origin and its appearance becomes the
//! merged record's own — through `comps_geometry::merged_comps`.
//!
//! # The two halves
//!
//! The structural half runs over `precision_structure_ops`' flat postorder
//! list, the same splice model the five structure ops use.
//!
//! The PIXEL half clones the members into a derived document through
//! `rebuilt` (which carries the profile, the evaluation policy's tables and
//! the float policy, so a member subset of a renderable document is itself
//! renderable) and renders that document through the ORDINARY typed
//! compositor. Clip runs, blend modes, opacities, masks, Blend If and styles
//! among the members therefore bake exactly as they composite, because it is
//! literally the projection.
//!
//! # The UNION-EXTENT rule
//!
//! Merge Layers and Merge Down render that document over the members' UNION
//! EXTENT rather than the canvas, and the merged leaf sits at the union's
//! origin — the byte rule, `style_composite::merge_extent` over every visible
//! member, so pixels a member holds outside the canvas survive the merge at
//! every depth. `precision_merge_extent` owns the union, its three refusals
//! and the windowing of a handed knockout backdrop onto the frame; the
//! compositor renders any frame through `PrecisionScene::with_frame`. A
//! visible upper record that adds nothing to the union (an empty group) takes
//! Merge Down's removal path, as the byte merge removes an upper entry that
//! "was invisible or empty". Merge Visible, like Stamp Visible and Flatten,
//! renders the document's own projection and so stays one canvas-sized leaf
//! at (0, 0) — which is what the byte op does too.
//!
//! One difference from the byte merge remains, in the MASKED ADJUSTMENT: a
//! typed adjustment's mask gates the whole frame, reading its own
//! `outside_coverage` beyond its plane, where the byte twin skips every pixel
//! outside its mask plane (`doc::composite_adjustment_into`). The two agree
//! wherever the mask plane reaches, and everywhere when `outside_coverage` is
//! 0.
//!
//! The structural half runs over `precision_structure_ops`' flat postorder list; the PIXEL half
//! clones the members into a derived document through `rebuilt` and renders it through the ORDINARY
//! typed compositor, so clip runs, blend modes, opacities, masks, Blend If and styles bake exactly
//! as they project and a FLOAT merge is ADMITTED rather than refused. **Merge Layers and
//! Merge Down keep the UNION EXTENT at every depth**: the members document renders over the union
//! frame (`PrecisionScene::with_frame`, the frame origin threaded through `render_layers` /
//! `render_projection` / `render_styled`, every non-merge caller at (0, 0)) and the merged leaf
//! sits at the union's origin, so off-canvas pixels survive as at 8 bits; a visible upper record
//! that adds nothing to the union (an empty group) takes Merge Down's removal path, and Merge
//! Visible stays canvas-sized at (0, 0) like 8-bit. One byte behaviour is MIRRORED rather than
//! "fixed": `merge_visible`'s two-leaf floor counts `Content::Raster` where the byte predicate
//! counts `LayerKind::Raster` — which at byte depth includes an adjustment layer. Since 12E-2
//! `merge_down` keeps the LOWER record's id and label, like the other two, so `doc_identity`'s
//! merge rule has no exception at either depth.

use crate::comps_geometry::merged_comps;
use crate::doc_lock::EditKind;
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{pixel_count, BOOKKEEPING_BYTES};
use crate::precision_lock::lock_sentence;
use crate::precision_merge_extent::{record_extent16, windowed_seed};
use crate::precision_structure_ops::{flatten, level_range, parent, rebuild, subtree, FlatRecord};
use crate::precision_style_knockout::KnockoutSeed;

impl PrecisionDocument {
    /// Merge Layers over a multi-selection: the given records —
    /// which must all share ONE parent — become ONE raster record sized to
    /// their union extent (module doc) at the lowest member's slot and level,
    /// keeping that member's
    /// name, identity, clipped flag, locks, link and colour label
    /// (`doc_structure::merge_layers`).
    ///
    /// Refuses by name for fewer than two members, members that do not share
    /// one parent, an out-of-range index, a hidden LOWEST member (which would
    /// silently discard everything above it) and a LOWEST member whose locks
    /// refuse a merge (`EditKind::Merge` — the merged record takes its slot
    /// and replaces its picture), and with `precision_merge_extent`'s
    /// sentences a union that is empty, oversized or out of range.
    pub(crate) fn merge_layers(
        &self,
        indices: &[usize],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let flat = flatten(&self.layers);
        let members = merge_members(&flat, indices)?;
        let lowest = members[0];
        let lowest_layer = self
            .layer(lowest)
            .ok_or("high-depth layer index is out of range")?;
        if !lowest_layer.visible() {
            return Err("high-depth merge requires a visible lowest layer".into());
        }
        // The DESTINATION's locks, asked before anything is built: the merge
        // drains every member's subtree, so this is the last point at which
        // the index still names the record the lock belongs to (`doc_lock`).
        let blocking = self.lock_block(lowest, EditKind::Merge);
        if blocking != 0 {
            return Err(lock_sentence(
                lowest,
                self.blocking_owner(lowest, blocking),
                blocking,
            ));
        }
        let mut merged = self.merged_record(lowest_layer.name(), &members, limit, other, job)?;
        // The merged record IS the lowest member with new pixels, so it keeps
        // that member's identity and every panel property (`doc_identity`,
        // `doc_label`).
        merged.id = lowest_layer.id();
        merged.clipped = lowest_layer.clipped();
        merged.locks = lowest_layer.locks();
        merged.link = lowest_layer.link();
        merged.label = lowest_layer.label();
        let origin = merged.offset();
        let depth = flat[lowest].depth;
        let subs: Vec<_> = members.iter().map(|&i| subtree(&flat, i)).collect();
        let at = subs[0].start;
        let mut flat = flat;
        // TOP-DOWN, so every removal is above the ones still to come and
        // `at` — the lowest member's own slot — never moves.
        for sub in subs.iter().rev() {
            flat.drain(sub.clone());
        }
        flat.splice(
            at..at,
            std::iter::once(FlatRecord {
                layer: merged,
                depth,
            }),
        );
        self.rebuilt_merge(flat, lowest, origin)
    }

    /// Merges record `index` into the previous SIBLING — the record
    /// immediately below it within its own level (`doc::merging_down`). The
    /// merged record keeps only the LOWER record's name, identity, label,
    /// visibility, clipped flag, level, locks and link, and is Normal at
    /// opacity 1 because both
    /// operands' modes, opacities, masks and styles are baked into its pixels.
    ///
    /// An invisible or EMPTY upper record (a group with nothing visible
    /// inside) contributes nothing and is simply removed; a
    /// hidden LOWER record refuses the merge by name, so the upper record's
    /// content cannot silently vanish. A CLIPPED upper record bakes through
    /// its clipping, because the derived two-record document derives the same
    /// clip unit the projection would.
    pub(crate) fn merge_down(
        &self,
        index: usize,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let flat = flatten(&self.layers);
        if index >= flat.len() {
            return Err("high-depth layer index is out of range".into());
        }
        let upper_sub = subtree(&flat, index);
        if upper_sub.start == level_range(&flat, index).start {
            return Err("high-depth merge down requires a layer below it in its own level".into());
        }
        // A sibling's subtree ENDS with its own record, so the record
        // immediately below this subtree is exactly the previous sibling.
        let lower = upper_sub.start - 1;
        let lower_layer = self
            .layer(lower)
            .ok_or("high-depth layer index is out of range")?;
        if !lower_layer.visible() {
            return Err("high-depth merge down requires a visible lower layer".into());
        }
        let blocking = self.lock_block(lower, EditKind::Merge);
        if blocking != 0 {
            return Err(lock_sentence(
                lower,
                self.blocking_owner(lower, blocking),
                blocking,
            ));
        }
        let upper = self
            .layer(index)
            .ok_or("high-depth layer index is out of range")?;
        if !upper.visible() || record_extent16(upper, self).is_none() {
            // Nothing to merge in: the upper record (or the whole group) was
            // invisible or empty, so it is simply removed (`doc::merging_down`).
            return self.remove_layers(&[index]);
        }
        let members = vec![lower, index];
        let mut merged = self.merged_record(lower_layer.name(), &members, limit, other, job)?;
        // The merged record IS the lower one with new pixels, so it keeps
        // that record's identity and colour label as well as its flags —
        // `doc_identity`'s ONE merge rule, the same two lines `merge_layers`
        // above carries.
        merged.id = lower_layer.id();
        merged.label = lower_layer.label();
        merged.clipped = lower_layer.clipped();
        merged.locks = lower_layer.locks();
        merged.link = lower_layer.link();
        let origin = merged.offset();
        let depth = flat[lower].depth;
        let lower_sub = subtree(&flat, lower);
        let mut flat = flat;
        flat.drain(upper_sub);
        flat.splice(
            lower_sub,
            std::iter::once(FlatRecord {
                layer: merged,
                depth,
            }),
        );
        self.rebuilt_merge(flat, lower, origin)
    }

    /// Merge Visible: every record that CONTRIBUTES to the projection — its
    /// own `visible` true AND every ancestor's true — is replaced by ONE
    /// canvas-sized raster record holding the document's projection
    /// (`doc_structure::merge_visible`).
    ///
    /// "Contributes to the projection" is the single predicate for BOTH halves
    /// of the op, which is what makes it safe: a visible record inside a
    /// hidden group contributes nothing, so it is never merged away. A GROUP
    /// is removed only when its whole subtree contributed. Refuses by name
    /// when fewer than two RASTER leaves contribute — counting records would
    /// count group rows, so a single layer inside one visible group would
    /// wrongly satisfy a "two records" floor and merge itself into a copy of
    /// itself.
    pub(crate) fn merge_visible(
        &self,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let flat = flatten(&self.layers);
        let contributes = contributing(&flat);
        // A CMYK raster counts as a raster (#547 phase 4a).
        let raster = |i: usize| {
            self.layer(i).is_some_and(|l| {
                matches!(
                    l.content,
                    Content::Raster { .. } | Content::CmykRaster { .. }
                )
            })
        };
        if (0..flat.len())
            .filter(|&i| contributes[i] && raster(i))
            .count()
            < 2
        {
            return Err("high-depth merge visible requires two contributing raster layers".into());
        }
        // Removed = contributes AND its whole subtree contributes. A group
        // whose children are hidden contributes itself but must survive to
        // hold them.
        let removed: Vec<bool> = (0..flat.len())
            .map(|i| contributes[i] && subtree(&flat, i).all(|d| contributes[d]))
            .collect();
        // The SLOT comes from the bottom-most contributing record, which may
        // be a group whose own children are hidden; the NAME, IDENTITY and
        // LABEL come from the bottom-most contributing RASTER leaf, which is
        // the layer a person means by "the bottom visible layer".
        let bottom = contributes
            .iter()
            .position(|&c| c)
            .ok_or("high-depth merge visible requires two contributing raster layers")?;
        let (kept, source) = (0..flat.len())
            .find(|&i| contributes[i] && raster(i))
            .and_then(|i| Some((i, self.layer(i)?)))
            .ok_or("high-depth merge visible requires two contributing raster layers")?;
        let (name, identity, label) = (source.name().to_string(), source.id(), source.label());
        let mut ancestor = bottom;
        while let Some(above) = parent(&flat, ancestor) {
            ancestor = above;
        }
        let slot = subtree(&flat, ancestor).start;
        let at = (0..slot).filter(|&i| !removed[i]).count();
        let mut merged = self.projection_leaf(&name, limit, other, job)?;
        merged.id = identity;
        merged.label = label;
        let origin = merged.offset();
        let mut next: Vec<FlatRecord> = flat
            .into_iter()
            .enumerate()
            .filter(|(i, _)| !removed[*i])
            .map(|(_, record)| record)
            .collect();
        next.insert(
            at,
            FlatRecord {
                layer: merged,
                depth: 0,
            },
        );
        self.rebuilt_merge(next, kept, origin)
    }

    /// Merge Down's members for the PLAN: the same previous sibling
    /// `merge_down` resolves, charged beside the upper record, so a plan whose
    /// LOWER operand is a group, a masked or a styled record is not
    /// under-counted. Refuses by name exactly as the edit does. A hidden or
    /// EMPTY upper record takes the removal path and renders nothing, so it
    /// is charged no pixel bytes — the plan never refuses a removal the edit
    /// performs.
    pub(crate) fn merge_down_scratch_bytes(&self, index: usize) -> Result<u64, String> {
        let flat = flatten(&self.layers);
        if index >= flat.len() {
            return Err("high-depth layer index is out of range".into());
        }
        let upper_sub = subtree(&flat, index);
        if upper_sub.start == level_range(&flat, index).start {
            return Err("high-depth merge down requires a layer below it in its own level".into());
        }
        if record_extent16(&flat[index].layer, self).is_none() {
            return Ok(0);
        }
        self.merge_scratch_bytes(&[upper_sub.start - 1, index])
    }

    /// What a merge SPENDS beyond the structural bookkeeping: the larger of
    /// this document's own render plan (the CAPTURE render) and the derived
    /// members document's plan over the union FRAME, the two captured `B0`
    /// planes RESERVED across both (`32·n` over the canvas, D8-style — a
    /// charge lives as long as its buffer) and their two windows onto the
    /// frame (`32·f`), plus the frame-sized output the merged leaf retains.
    /// Refuses with `precision_merge_extent`'s sentences, so the plan and the
    /// edit refuse one union alike. An EMPTY member list is Merge Visible,
    /// whose projection is this document's own canvas.
    ///
    /// The members plan may over-count one Background snapshot the `Handed`
    /// render never takes: a conservative admission bound, stated rather than
    /// threaded, because the seed is chosen after the plan.
    pub(crate) fn merge_scratch_bytes(&self, members: &[usize]) -> Result<u64, String> {
        // A CMYK document's merges run `cmyk_merge`'s frame render, charged
        // by its own plan over the members document (#547 phase 4a).
        if self.is_cmyk() {
            if members.is_empty() {
                return self.cmyk_merge_scratch_bytes(((0, 0), (self.width, self.height)));
            }
            let frame = self.merge_frame(members)?;
            let members_bytes = self
                .members_document(members)?
                .cmyk_merge_scratch_bytes(frame)?;
            // A seeded merge renders this document beside it (`cmyk_merge`).
            if merge_wants_backdrop16(self, &flatten(&self.layers), members) {
                return members_bytes
                    .checked_add(self.cmyk_seed_scratch_bytes(frame)?)
                    .ok_or_else(|| "CMYK merge byte calculation overflow".to_string());
            }
            return Ok(members_bytes);
        }
        let samples = pixel_count(self.width, self.height)? as u64;
        let per_sample = if self.is_float() { 16 } else { 8 };
        let overflow = || "high-depth merge byte calculation overflow".to_string();
        if members.is_empty() {
            return self
                .render_plan()?
                .scratch_bytes
                .checked_add(samples * per_sample + BOOKKEEPING_BYTES)
                .ok_or_else(overflow);
        }
        let frame = self.merge_frame(members)?;
        let framed = u64::from(frame.1 .0) * u64::from(frame.1 .1);
        self.render_plan()?
            .scratch_bytes
            .max(
                self.members_document(members)?
                    .framed_scratch_bytes(frame)?,
            )
            .checked_add(samples.saturating_mul(32))
            .and_then(|v| v.checked_add(framed.saturating_mul(32)))
            .and_then(|v| v.checked_add(framed * per_sample + BOOKKEEPING_BYTES))
            .ok_or_else(overflow)
    }

    /// The members as their own document — the pixel half of every merge.
    /// `rebuilt` carries the profile, the evaluation policy's tables and the
    /// float policy, so a member subset of a renderable document is itself
    /// renderable and a FLOAT merge is admitted rather than refused.
    pub(crate) fn members_document(&self, members: &[usize]) -> Result<Self, String> {
        let layers = members
            .iter()
            .map(|&i| {
                self.layer(i)
                    .cloned()
                    .ok_or_else(|| "high-depth layer index is out of range".to_string())
            })
            .collect::<Result<Vec<_>, String>>()?;
        self.rebuilt(self.width, self.height, layers, Vec::new())
    }

    /// The merged pixels and the document pixel they start at: the members as
    /// their own document, rendered over their union FRAME
    /// (`precision_merge_extent`) with a HANDED knockout seed
    /// (`precision_style_knockout`).
    ///
    /// EVERY members document is rendered `Handed`, never
    /// `Projection { background }` — decision (v) and objection 1 of 12E-3b.
    /// `rebuilt` clones the subtree verbatim and a Pass Through member STAYS
    /// Pass Through at the derived root, so D5 would otherwise take the
    /// merge's LOWEST member for a Background and a knockout nested inside
    /// that member would punch to a picture that was never on screen.
    ///
    /// The planes are the captured `B0` of the LOWEST member's own level when
    /// step 4's two-clause predicate holds — a visible DIRECT member carries
    /// `knockout != none`, and the lowest member is not clipped to a base
    /// outside the set — and transparent (D4's private-accumulator rule)
    /// otherwise; captured over the canvas, they are windowed onto the frame.
    /// The frame is resolved FIRST, so an empty, oversized or out-of-range
    /// union refuses before anything renders.
    fn merged_pixels(
        &self,
        members: &[usize],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<(crate::precision_image::PrecisionImage, (i32, i32)), String> {
        let frame = self.merge_frame(members)?;
        let seed = if merge_wants_backdrop16(self, &flatten(&self.layers), members) {
            self.level_backdrop(members[0], limit, other, job)?
        } else {
            KnockoutSeed::Handed {
                shallow: None,
                deep: None,
            }
        };
        let seed = windowed_seed(seed, (self.width, self.height), frame);
        let image = self
            .members_document(members)?
            .render_seeded_with_context(seed, frame, limit, other, job)?;
        Ok((image, frame.0))
    }

    /// The merged record of `members` named `name`: the RGB pixels of
    /// [`Self::merged_pixels`] through [`Self::merged_leaf`], or on a CMYK
    /// document the members document's composite over the union frame
    /// (`cmyk_merge`) as one CMYK leaf at the frame's origin — Normal at
    /// opacity 1, every other property the caller's to set.
    pub(crate) fn merged_record(
        &self,
        name: &str,
        members: &[usize],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<PrecisionDocumentLayer, String> {
        if self.is_cmyk() {
            let frame = self.merge_frame(members)?;
            // D7 on ink (`cmyk_merge`): the members document's root is handed
            // the lowest member's backdrop, captured from this document's own
            // projection, when a direct member punches; else transparent.
            let seed_from = if merge_wants_backdrop16(self, &flatten(&self.layers), members) {
                let lowest = self
                    .layer(members[0])
                    .ok_or("high-depth layer index is out of range")?;
                Some((self, lowest.id()))
            } else {
                None
            };
            let image = self
                .members_document(members)?
                .cmyk_frame_image(frame, seed_from, job)?;
            return PrecisionDocumentLayer::raster_cmyk(name, image, frame.0, 1.0, true);
        }
        let (image, at) = self.merged_pixels(members, limit, other, job)?;
        self.merged_leaf(name, image, at)
    }

    /// The document's own projection as ONE canvas-sized leaf at (0, 0)
    /// named `name` — Merge Visible's and Stamp Visible's pixels: the RGB
    /// render through [`Self::merged_leaf`], or the CMYK composite over the
    /// canvas (`cmyk_merge`) as a CMYK leaf.
    pub(crate) fn projection_leaf(
        &self,
        name: &str,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<PrecisionDocumentLayer, String> {
        if self.is_cmyk() {
            let image = self.cmyk_projection_image(((0, 0), (self.width, self.height)), job)?;
            return PrecisionDocumentLayer::raster_cmyk(name, image, (0, 0), 1.0, true);
        }
        let image = self.render_with_context(limit, other, job)?;
        self.merged_leaf(name, image, (0, 0))
    }

    /// The merged record: ONE leaf at `origin` (the union's for Merge Layers
    /// and Merge Down, (0, 0) for the canvas-sized projections), Normal at
    /// opacity 1, with no mask, style, Blend If, description or retained
    /// source — everything the merge baked into the pixels is gone from the
    /// model, as at byte depth.
    pub(crate) fn merged_leaf(
        &self,
        name: &str,
        image: crate::precision_image::PrecisionImage,
        origin: (i32, i32),
    ) -> Result<PrecisionDocumentLayer, String> {
        if self.is_float() {
            PrecisionDocumentLayer::raster_float(name, image, origin, 1.0, true)
        } else {
            PrecisionDocumentLayer::raster(name, image, origin, 1.0, true)
        }
    }

    /// The merged document from the spliced flat list. `rebuilt` carries the
    /// comps; the ones bound to `kept` (the pre-merge postorder index of the
    /// record whose id the merged one keeps) then follow the merge to
    /// `origin`, the merged record's offset (module doc).
    fn rebuilt_merge(
        &self,
        flat: Vec<FlatRecord>,
        kept: usize,
        origin: (i32, i32),
    ) -> Result<Option<Self>, String> {
        let layers = rebuild(flat)?;
        let mut next = self.rebuilt(
            self.width,
            self.height,
            layers,
            self.channels.as_ref().clone(),
        )?;
        if let Some(kept_layer) = self.layer(kept) {
            if let Some(comps) = merged_comps(
                &self.comps,
                self.comps_after_growth(kept, origin),
                kept_layer.id(),
            ) {
                next.comps = comps;
            }
        }
        Ok(Some(next))
    }
}

/// The typed twin of `style_knockout_merge::merge_wants_backdrop`: some
/// VISIBLE DIRECT member's own stored style has `knockout != none`, AND the
/// lowest member is not clipped to a base OUTSIDE the set (which would be
/// baseless in the derived document and would read the level backdrop where
/// the projection gave it the base-only snapshot — a wrong picture where a
/// transparent seed is exact).
fn merge_wants_backdrop16(
    document: &PrecisionDocument,
    flat: &[FlatRecord],
    members: &[usize],
) -> bool {
    let Some(&lowest) = members.first() else {
        return false;
    };
    let wants = members.iter().any(|&i| {
        document.layer(i).is_some_and(|l| {
            l.visible()
                && l.style()
                    .is_some_and(|s| s.knockout != crate::style::Knockout::None)
        })
    });
    if !wants {
        return false;
    }
    if !flat.get(lowest).is_some_and(|r| r.layer.clipped()) {
        return true;
    }
    // An UNCLIPPED sibling below it in its own level is a base outside the set.
    let range = level_range(flat, lowest);
    let depth = flat[lowest].depth;
    let outside = (range.start..lowest)
        .filter(|&i| flat[i].depth == depth)
        .any(|i| !flat[i].layer.clipped());
    !outside
}

/// The merge set: deduplicated, ascending, at least two members, in range and
/// sharing ONE parent at ONE level (`doc_structure::merge_layers`).
fn merge_members(flat: &[FlatRecord], indices: &[usize]) -> Result<Vec<usize>, String> {
    let mut members = indices.to_vec();
    members.sort_unstable();
    members.dedup();
    if members.len() < 2 {
        return Err("high-depth merge requires at least two layers".into());
    }
    if *members.last().expect("a non-empty set") >= flat.len() {
        return Err("high-depth layer index is out of range".into());
    }
    let depth = flat[members[0]].depth;
    let group = parent(flat, members[0]);
    if members
        .iter()
        .any(|&i| flat[i].depth != depth || parent(flat, i) != group)
    {
        return Err("high-depth merge requires one shared parent group".into());
    }
    Ok(members)
}

/// Which records CONTRIBUTE to the projection: their own `visible` true and
/// every ancestor's true — the single predicate `merge_visible` uses for both
/// halves of its work (`doc_structure::contributing`).
pub(crate) fn contributing(flat: &[FlatRecord]) -> Vec<bool> {
    let mut out = vec![false; flat.len()];
    for i in 0..flat.len() {
        if !flat[i].layer.visible {
            continue;
        }
        let mut visible = true;
        let mut node = i;
        while let Some(above) = parent(flat, node) {
            if !flat[above].layer.visible {
                visible = false;
                break;
            }
            node = above;
        }
        out[i] = visible;
    }
    out
}
