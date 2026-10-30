//! Stamp Visible (`stamp_visible`) and Flatten Image (`flatten_image`) on
//! `PrecisionDocument` (Phase 12E-2c): the two PROJECTION operations that
//! sit beside the three typed merges in `precision_merge`.
//!
//! Both are the BYTE ops verbatim — `doc_structure::stamp_visible` (through
//! `doc::adding_image_layer`) and `doc::flattening` (through
//! `doc::from_pixels`) are the specification — and both take their pixels
//! from the document's OWN projection, which is `merge_visible`'s own source
//! (`render_with_context`, charged as `merge_scratch_bytes(&[])`). The nine
//! rules that decide what they mean, each of them the byte one:
//!
//! 1. **A stamp does NOT join a clip run.** The byte `adding_image_layer`
//!    sets only the level and leaves `clipped` false, so the typed op
//!    resolves the ordinary `Above` slot and then FORCES `clipped = false`
//!    before splicing. This is the one place `layer_insert`'s join-the-run
//!    rule is deliberately overridden, because the byte op never joined.
//! 2. **A flatten DROPS the whole comp list.** The byte flatten builds on
//!    `..from_pixels`, whose `comps` is empty, while `rebuilt` COPIES the
//!    comps (a rebuild is the SAME document with changed records). So
//!    `comps.clear()` after `rebuilt` is required, not incidental.
//! 3. **A flatten's one record is named `"Background"`** — `from_pixels`'
//!    own name — with a newly minted id, no label, no locks, no link,
//!    visible, opacity 1, Normal, unclipped, at level 0 and offset (0, 0),
//!    with no mask, style, Blend If, description or retained source. A
//!    stamp's record is that same shape with the CALLER's name (the host
//!    default is `"Stamp"`), landing at the ANCHOR's level directly above
//!    the anchor's whole subtree.
//! 4. **Both ops are lock-EXEMPT, here as at byte depth.** Neither byte op
//!    consults `lock_block`: a stamp ADDS a record and writes none (which is
//!    `layer_insert`'s exemption), and a flatten replaces the whole stack
//!    rather than editing an entry. No `lock_block` call is added.
//! 5. **A flatten never answers a no-op.** A one-record document still
//!    answers `Ok(Some(_))`, because the flattened picture is a new one even
//!    when the stack was already a single layer. Only the MENU asks for
//!    more than one layer.
//! 6. **A flatten keeps** the saved channels, the guides, the ruler origin,
//!    the profile, the metadata, the resolution and the evaluation policy —
//!    all of them through `rebuilt`, with the import warnings riding along.
//!    Hidden records are discarded, because the projection is what survives.
//! 7. **A stamp's floor is ONE contributing raster record**, the same
//!    `contributing` predicate the merge uses, narrowed to `Content::Raster`.
//!    That is 12E-1b's disclosed narrowing again: the byte predicate counts
//!    `LayerKind::Raster`, which at byte depth includes an adjustment layer.
//!    The typed op refuses BY NAME where the byte op answers `None`; both
//!    reach the host as nil.
//! 8. **The pixel source is the document's own projection**, not a derived
//!    members document: `render_with_context` + `merged_leaf`, which is
//!    exactly `merge_visible`'s path.
//! 9. **Both are canvas-sized at (0, 0) at every depth, like 8-bit.** The
//!    byte `flattened()` renders the CANVAS, so the byte Stamp and the byte
//!    Flatten are already one canvas-sized picture at (0, 0) and the typed
//!    ops produce the same thing — as Merge Visible does, while Merge Layers
//!    and Merge Down keep their members' union extent at both depths
//!    (`precision_merge`'s module doc).
//!
//! `stamp_visible` and `flatten_image` on `PrecisionDocument`, reached as
//! `StructureEdit::StampVisible {name, anchor}` and `StructureEdit::Flatten {}` and charged as
//! `merge_scratch_bytes(&[])`: both render the document's OWN projection (`render_with_context` +
//! `precision_merge`'s `pub(crate)` `merged_leaf`, Merge Visible's path, with its `contributing`
//! predicate) into ONE canvas-sized leaf, and the module doc's nine numbered rules are the contract
//! — a STAMP does NOT join a clip run (the byte `adding_image_layer` sets only the level, so the
//! resolved `Above` slot's `clipped` is FORCED off; the one deliberate override of `layer_insert`'s
//! join rule), a FLATTEN clears the comps after `rebuilt` (which copies them) and names its record
//! `"Background"`, both ops are lock-EXEMPT, a flatten is never an exact no-op, a flatten keeps
//! channels/guides/ruler origin/profile/metadata/resolution/evaluation while hidden records are
//! discarded, a STAMP's floor is ONE contributing `Content::Raster` record (the same 12E-1b
//! narrowing, refused BY NAME where byte answers `None`), and both are **canvas-sized at (0, 0) at
//! every depth, like 8-bit** (and like Merge Visible), because `doc::flattened()` already renders
//! the canvas. `Flatten {}` and (since the same run) `MergeVisible {}` are EMPTY STRUCT variants
//! because an internally tagged serde UNIT variant silently discards extra keys —
//! `raw_local_coverage.rs`'s rule; a new no-field `StructureEdit` variant must follow it. Families
//! `layer_stamp` / `layer_flatten`, target `document`, in both capability tables and in
//! `float_document::ADMITTED`.

use crate::job_control::JobContext;
use crate::layer_insert::{InsertPosition, InsertSlot};
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_merge::contributing;
use crate::precision_structure_ops::flatten;

impl PrecisionDocument {
    /// Stamp Visible: the document's own projection added as ONE new
    /// canvas-sized raster record directly above `anchor`'s whole subtree,
    /// at the anchor's own level, with every existing record left exactly as
    /// it was (`doc_structure::stamp_visible`).
    ///
    /// Merge Visible's non-destructive twin, and its floor is one
    /// contributing RASTER record rather than two: stamping a single visible
    /// layer is a copy of it, which is a thing a person means. Refuses by
    /// name when nothing contributes, where the byte op answers `None` — the
    /// stamp would be a fully transparent layer, an undo step and a dirty
    /// document in exchange for nothing.
    ///
    /// The new record is UNCLIPPED even when the next sibling is clipped:
    /// the byte `adding_image_layer` sets only the level (rule 1 above), so
    /// the resolved slot's `clipped` flag is forced off. Lock-exempt, as
    /// every insertion is.
    pub(crate) fn stamp_visible(
        &self,
        name: &str,
        anchor: usize,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        // The slot is resolved BEFORE anything is rendered, so a refused
        // anchor costs no projection (`precision_insert::add_layer_at`'s
        // order).
        let slot = self.insert_slot(anchor, InsertPosition::Above)?;
        let flat = flatten(&self.layers);
        let contributes = contributing(&flat);
        let contributing_raster = (0..flat.len()).any(|i| {
            contributes[i]
                && self.layer(i).is_some_and(|l| {
                    matches!(
                        l.content,
                        Content::Raster { .. } | Content::CmykRaster { .. }
                    )
                })
        });
        if !contributing_raster {
            return Err("high-depth stamp visible requires a contributing raster layer".into());
        }
        self.preflight_raster_addition((self.width, self.height), name)?;
        // A CMYK document stamps its ink composite (`cmyk_merge`, #1
        // phase 4a) through the same leaf builder Merge Visible uses.
        let leaf = self.projection_leaf(name, limit, other, job)?;
        self.inserting_record(
            leaf,
            InsertSlot {
                clipped: false,
                ..slot
            },
        )
        .map(Some)
    }

    /// Flatten Image: every record is replaced by ONE canvas-sized raster
    /// record named `"Background"` holding the document's projection
    /// (`doc::flattening`).
    ///
    /// Hidden records are DISCARDED — the projection is what survives — and
    /// the whole comp list goes with them, because every entry's identity is
    /// gone (rule 2 above). The channels, guides, ruler origin, profile,
    /// metadata, resolution and evaluation policy are kept, all of them
    /// through `rebuilt`. There is no no-op: a one-record document flattens
    /// to a NEW picture, and only the menu item asks for more than one
    /// layer.
    pub(crate) fn flatten_image(
        &self,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        // A CMYK document flattens its ink onto paper (`cmyk_convert`).
        if self.is_cmyk() {
            return self.cmyk_flatten_image(limit, other, job).map(Some);
        }
        let image = self.render_with_context(limit, other, job)?;
        let leaf = self.merged_leaf("Background", image, (0, 0))?;
        let mut next = self.rebuilt(
            self.width,
            self.height,
            vec![leaf],
            self.channels.as_ref().clone(),
        )?;
        // `rebuilt` carries the comps, because a rebuild is the same
        // document with changed records. A FLATTEN is not: the byte op
        // builds on `..from_pixels`, whose comp list is empty.
        next.comps.clear();
        Ok(Some(next))
    }
}
