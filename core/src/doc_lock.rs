//! Layer LOCKS: the four bits `Layer::locks` carries, what each one forbids,
//! and [`DrDocument::lock_block`] — the query a host asks to phrase a refusal
//! that names the lock that stopped an edit. The ops themselves refuse
//! regardless, so a host that forgets to ask cannot get through.
//!
//! # The bits
//!
//! ```text
//! bit 0  LOCK_TRANSPARENCY = 1   the alpha channel is frozen
//! bit 1  LOCK_PIXELS       = 2   no pixel edit at all
//! bit 2  LOCK_POSITION     = 4   no move, no transform, no offset change
//!        LOCK_ALL          = 7   all three — "Lock All" is the three bits
//!                                together, not a 4th bit. Bits 0..2 share
//!                                `lspf`'s positions, but `lspf` spells Lock
//!                                All as bit 31 (`psd_locks`)
//! ```
//!
//! Bits 3..31 are reserved and are masked off wherever a value enters (the
//! setter, the `.rz` reader, the FFI), so a file from a newer build loads with
//! the locks this one understands rather than being refused.
//!
//! # What each lock means
//!
//! **Transparency** freezes the layer's ALPHA CHANNEL: every pixel op's result
//! has the layer's original alpha restored, so an eraser cannot punch a hole.
//! PAINT goes further and writes the frozen-alpha colour rule itself: the
//! byte, 16-bit and float paint walks see the lock ([`DrDocument::alpha_frozen`])
//! and leave an alpha-0 pixel alone, mix a semi-transparent pixel's colour as
//! `lerp(Cb, B(Cb, Cs), sa)` — the paint at its own opacity × coverage, never
//! boosted by the restore — and keep its alpha (`blend::composite_source_frozen`).
//! For paint the restore below is then a BACKSTOP that finds nothing to undo;
//! for every other pixel op (filters, heal, dodge/burn, plane and mask
//! painting) it is still the rule. Two kinds of pixel get their COLOUR back
//! with their alpha:
//! one the layer already holds at zero alpha (nothing there is visible, so
//! those bytes are not part of the picture) and one the OP drove to zero (an
//! erase or a clear, which zero the colour as they go — restoring only the
//! alpha would resurrect the pixel in black). Both together are what make an
//! edit that only tried to remove coverage a byte-exact no-op, which is what
//! the purity latch needs to see so it can return `None` instead of minting an
//! undo step.
//!
//! **Pixels** refuses every pixel edit. Mask edits are NOT blocked by it:
//! a pixel-locked layer's mask stays paintable.
//!
//! **Position** refuses `with_layer_offset`, `move_layers`, `transform_layers_with_job`
//! and `perspective_layer`. Moving a GROUP checks the group's own position bit
//! AND every descendant's, because moving a group moves them.
//!
//! **A transform is a POSITION edit only, never a Pixels edit.** A transform
//! resamples the whole buffer INCLUDING its alpha, so "the alpha channel is
//! frozen" has no meaning there: transparency lock gates painting INTO an
//! existing buffer, and position lock is the flag that refuses a transform.
//! Were a transform "Pixels + Position", a layer with ONLY transparency locked
//! would become untransformable, because a transform derives its own extent
//! and so almost always changes the layer's size and offset. Lock
//! Transparency does not block Free Transform; Lock Position does.
//!
//! **All three set** additionally freezes the MASK (`add_mask`,
//! `remove_mask`, `paint_mask`, `set_mask_enabled` refuse). That is the one
//! extra meaning "Lock All" carries beyond the union of the three.
//!
//! **APPLYING a mask is the one mask operation TRANSPARENCY also refuses**
//! ([`EditKind::MaskApply`], which `remove_mask(idx, apply: true)` runs
//! under). Adding, painting, enabling and deleting a mask leave the layer's
//! alpha alone, which is why Lock Pixels does not gate them; APPLYING one
//! multiplies the mask straight into that alpha, so it is exactly the edit
//! "the alpha channel is frozen" promises cannot happen — a hide-all mask
//! applied to a transparency-locked layer would otherwise erase the whole
//! layer, which is the hole an eraser is not allowed to punch. Blocking it in
//! `lock_block` rather than restoring the alpha afterwards is what lets the
//! host NAME the lock: an apply that silently came back unchanged would look
//! like a failure with no reason.
//!
//! **A MERGE answers to the DESTINATION entry's locks**
//! ([`EditKind::Merge`], which `merging_down` and `merge_layers` ask about the
//! lower operand). Pixels refuses it because a merge replaces that entry's
//! whole picture — the one op that destroys a layer's contents outright.
//! Transparency refuses it for the reason [`DrDocument::freezing_alpha`]
//! refuses any resized Pixels edit: a merge derives a new extent from the
//! union of both operands, and a frozen alpha channel cannot follow a buffer
//! that moved. The UPPER operand is only REMOVED, and removal is never
//! blocked, so its locks do not count.
//!
//! # A group's locks cover its contents
//!
//! An entry's EFFECTIVE locks are its own bits OR every enclosing group's
//! bits, for every bit: locking a folder locks what is in it.
//! [`entry_block`] is the per-entry table and [`DrDocument::lock_block`]
//! ORs it over the entry and each ancestor, so a pixels-locked group refuses
//! paint on a grandchild, a position-locked group refuses moving a child, and
//! a transparency-locked group freezes a child's alpha. Lock All still means
//! ONE entry holding all three bits: a mask freezes when any entry in the
//! chain is Lock All, not when the chain's union happens to reach 7. Moving a
//! group still also checks its descendants' position bits.
//! [`DrDocument::lock_owner`] names the entry whose own bits did the blocking,
//! so a host can say "inside group G" rather than blame the child.
//!
//! Locks never block delete, duplicate, reorder, group/ungroup, rename,
//! opacity, blend mode, visibility or style: blocking them would make a
//! locked layer unmanageable.
//!
//! # The ONE enforcement point
//!
//! Every op that writes a layer wraps its body in [`DrDocument::under_locks`]
//! (or [`DrDocument::under_locks_fallible`], for the healing family, which
//! reports through `Result`). There is no second place a lock is consulted, so
//! a new pixel op is either wrapped or it is a bug `structure_tests`' lock
//! sweep catches.
//!
//! The two MERGES are the exception the wrapper cannot express, and they are
//! not an exemption: `merging_down` and `merge_layers` drain their operands'
//! subtrees, so the entry the lock belongs to does not survive the edit at the
//! index it was asked about and `under_locks`' after-the-fact alpha restore
//! has nothing to write to. Both therefore ask [`DrDocument::lock_block`]
//! directly, with [`EditKind::Merge`], BEFORE touching anything — the same
//! question, asked at the only point in those ops where the answer still
//! addresses a live entry. The lock sweep covers them like any other writer.
//!
//! ONE site is deliberately NOT wrapped, and this is the note that keeps
//! someone from "fixing" it: `doc_heal::heal_window_into_layer`, the
//! whole-layer form kept for `core/tests/heal_tests.rs`'s window-contract
//! test — since 13B-2c-2a every production caller (heal, spot heal and
//! content-aware fill) runs
//! `heal_window_into_pixels_with_context` over a `layer_rect::rect_image`.
//! It takes a bare `&DrDocument`, reads `layers.get` and is NOT lock-gated:
//! a module-level free function, so `self.under_locks(...)` cannot wrap it.
//! The locks are enforced one level up, at `heal_layer` /
//! `spot_heal_layer_job` / `content_aware_fill_source_job`, which is where the
//! caller's real layer index enters.
//!
//! MOVING SELECTED PIXELS (`selected_pixels`) is the other writer that
//! asks [`DrDocument::lock_block`] with [`EditKind::Pixels`] directly and never
//! wraps [`DrDocument::under_locks`]. The move is allowed on a
//! transparency- or position-locked layer: the hole it cuts IS an alpha
//! edit, which `freezing_alpha`'s restore would undo (turning the move into a
//! stamp), and a landing that grows the extent would be refused outright. So
//! Lock Pixels and Lock All refuse up front and Transparency / Position do not
//! gate it at all; the typed twin asks `lock_refusal` the same way.

use crate::doc::{DrDocument, Layer, LayerKind};
use crate::doc_group::{parent, subtree};
use crate::job_control::JobContext;

/// The layer's alpha channel is frozen; colour may change, coverage may not.
pub const LOCK_TRANSPARENCY: u32 = 1;
/// No pixel edit at all.
pub const LOCK_PIXELS: u32 = 2;
/// No move, no transform, no offset change.
pub const LOCK_POSITION: u32 = 4;
/// All three — "Lock All", and the mask frozen with them.
pub const LOCK_ALL: u32 = LOCK_TRANSPARENCY | LOCK_PIXELS | LOCK_POSITION;

/// What kind of edit is being attempted, for [`DrDocument::lock_block`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum EditKind {
    /// Anything that writes the layer's pixels.
    Pixels = 0,
    /// Anything that moves or resamples it: offset, move, transform,
    /// perspective.
    Position = 1,
    /// Anything that adds, removes, enables or paints its mask.
    Mask = 2,
    /// APPLYING a mask: a mask operation that also bakes the coverage into
    /// the layer's ALPHA. Blocked by Lock Transparency as well as by Lock
    /// All — see the module doc.
    MaskApply = 3,
    /// A MERGE writing this entry: its whole buffer is replaced, at a new
    /// extent derived from both operands. Blocked by Lock Pixels and by Lock
    /// Transparency — see the module doc.
    Merge = 4,
}

impl EditKind {
    /// Maps a raw `DrEditKind` value coming across the FFI.
    pub fn from_c(value: i32) -> Option<Self> {
        match value {
            0 => Some(EditKind::Pixels),
            1 => Some(EditKind::Position),
            2 => Some(EditKind::Mask),
            3 => Some(EditKind::MaskApply),
            4 => Some(EditKind::Merge),
            _ => None,
        }
    }
}

/// Which of ONE entry's own `locks` would block an edit of `kind`, with no
/// ancestor and no descendant consulted — the per-entry table both depths'
/// `lock_block` OR over the entry and its enclosing groups.
///
/// A MASK edit is blocked only by Lock All, which is the one extra meaning
/// that spelling carries; a MASK APPLY — the one mask op that writes the
/// layer's alpha — is blocked by Lock Transparency too, as is a MERGE that
/// would replace the entry's picture.
pub(crate) fn entry_block(locks: u32, kind: EditKind) -> u32 {
    let locks = locks & LOCK_ALL;
    match kind {
        EditKind::Pixels => locks & LOCK_PIXELS,
        EditKind::Position => locks & LOCK_POSITION,
        EditKind::Mask => {
            if locks == LOCK_ALL {
                LOCK_ALL
            } else {
                0
            }
        }
        EditKind::MaskApply => {
            if locks == LOCK_ALL {
                LOCK_ALL
            } else {
                locks & LOCK_TRANSPARENCY
            }
        }
        EditKind::Merge => locks & (LOCK_PIXELS | LOCK_TRANSPARENCY),
    }
}

impl DrDocument {
    /// Which of entry `idx`'s EFFECTIVE lock bits would block an edit of
    /// `kind` (0 = allowed): [`entry_block`] of its own bits OR'd with
    /// [`entry_block`] of every enclosing group's. The host asks this
    /// to phrase a refusal that names the lock; the ops themselves refuse
    /// regardless, so a host that forgets cannot get through. 0 for an
    /// out-of-range index — there is no entry to be locked, and the op
    /// refuses on the index alone.
    ///
    /// A POSITION edit on a group also answers with its descendants' position
    /// bits, because moving a group moves them.
    pub fn lock_block(&self, idx: usize, kind: EditKind) -> u32 {
        let Some(entry) = self.layers.get(idx) else {
            return 0;
        };
        let mut blocked = entry_block(entry.locks, kind);
        if kind == EditKind::Position && entry.kind == LayerKind::Group {
            for i in subtree(&self.layers, idx) {
                blocked |= self.layers[i].locks & LOCK_POSITION;
            }
        }
        let mut at = idx;
        while let Some(group) = parent(&self.layers, at) {
            blocked |= entry_block(self.layers[group].locks, kind);
            at = group;
        }
        blocked
    }

    /// The entry whose OWN bits hold every bit of `locks` (masked to
    /// [`LOCK_ALL`]): `idx` itself first, then its enclosing groups outward,
    /// nearest first. `None` when the mask is empty, `idx` is out of range,
    /// or no entry in the chain holds all of them. The host names this entry
    /// in a refusal, so a child of a locked group reads "inside group G".
    pub fn lock_owner(&self, idx: usize, locks: u32) -> Option<usize> {
        let wanted = locks & LOCK_ALL;
        if wanted == 0 {
            return None;
        }
        let mut at = idx;
        loop {
            if self.layers.get(at)?.locks & wanted == wanted {
                return Some(at);
            }
            at = parent(&self.layers, at)?;
        }
    }

    /// Whether entry `idx`'s alpha channel is frozen by a transparency lock —
    /// its own bit or an enclosing group's ([`Self::lock_owner`]). The paint
    /// walks ask this to write the frozen-alpha colour rule
    /// instead of leaving it to [`Self::freezing_alpha`]'s restore.
    pub(crate) fn alpha_frozen(&self, idx: usize) -> bool {
        self.lock_owner(idx, LOCK_TRANSPARENCY).is_some()
    }

    /// Pure setter: replaces entry `idx`'s lock flags, masking off the
    /// reserved bits so a caller cannot store a value this build would not
    /// understand. `None` on an out-of-range index, and `None` when the entry
    /// already carries exactly those flags — the core's purity rule, which is
    /// load-bearing here: the host registers an undo step and dirties the
    /// document for every non-nil answer, so an identical copy would put a
    /// phantom step between the user and their last real edit.
    pub fn with_layer_locks(&self, idx: usize, locks: u32) -> Option<Self> {
        let locks = locks & LOCK_ALL;
        if self.layers.get(idx)?.locks == locks {
            return None;
        }
        let mut doc = self.clone();
        doc.layers[idx].locks = locks;
        Some(doc)
    }

    /// Runs `op` under entry `idx`'s locks — the ONE place locks are enforced.
    /// Refuses (`None`) when a lock forbids the edit outright, and restores
    /// the layer's original alpha afterwards when transparency is locked (a
    /// backstop for paint, whose walks already kept the alpha — module doc).
    ///
    /// Refuses too when a transparency-locked edit changed the layer's SIZE or
    /// OFFSET, since a frozen alpha channel cannot follow a buffer that moved
    /// — which is exactly why a transform is a Position edit and not a Pixels
    /// one (see the module doc).
    ///
    /// AFTER the alpha restore it compares the resulting layer's pixels and
    /// mask against the input's and returns `None` when nothing moved. The
    /// core's purity rule is that an op which changes nothing returns `None`,
    /// never an identical copy, and the alpha restore is precisely what CAN
    /// mint one: `clear_selection`, an eraser stroke, or a brush stroke that
    /// lands entirely where alpha is 0 all come back byte-identical on a
    /// transparency-locked layer — and all three are MCP-reachable, so each
    /// would otherwise add an undo step and dirty the document for nothing.
    /// This is the same latch `painting_layer` already applies for an
    /// out-of-extent overlay.
    pub(crate) fn under_locks(
        &self,
        idx: usize,
        kind: EditKind,
        op: impl FnOnce(&Self) -> Option<Self>,
    ) -> Option<Self> {
        if self.lock_block(idx, kind) != 0 {
            return None;
        }
        // A MASK edit of a layer with a live vector mask edits the RASTER
        // part: it runs with the raster part alone in the slot, and the
        // product is baked back (`doc_vector_mask`). `MaskApply` is not
        // wrapped: applying consumes the combined coverage.
        if kind == EditKind::Mask {
            if let Some((exposed, record)) = self.exposing_vector_mask(idx) {
                let edited = op(&exposed)?;
                return Self::rebaking_vector_mask(edited, record, JobContext::NONE).ok();
            }
        }
        self.freezing_alpha(idx, kind, op(self)?)
    }

    /// The `Result`-returning mirror of [`Self::under_locks`], for the healing
    /// and inpainting ops, whose bodies report I/O-shaped failures through
    /// `Err`. A lock refusal is a DOMAIN refusal, so it comes back as
    /// `Ok(None)` — the same shape those ops already use for "nothing to do" —
    /// and never as an error string the host would show as a failure.
    pub(crate) fn under_locks_fallible(
        &self,
        idx: usize,
        kind: EditKind,
        op: impl FnOnce(&Self) -> Result<Option<Self>, String>,
    ) -> Result<Option<Self>, String> {
        if self.lock_block(idx, kind) != 0 {
            return Ok(None);
        }
        // The vector-mask wrap, as in `under_locks`.
        if kind == EditKind::Mask {
            if let Some((exposed, record)) = self.exposing_vector_mask(idx) {
                let Some(edited) = op(&exposed)? else {
                    return Ok(None);
                };
                return Self::rebaking_vector_mask(edited, record, JobContext::NONE).map(Some);
            }
        }
        let Some(out) = op(self)? else {
            return Ok(None);
        };
        Ok(self.freezing_alpha(idx, kind, out))
    }

    /// The transparency half of [`Self::under_locks`]: `out` with entry
    /// `idx`'s original alpha channel restored, or `None` when the edit could
    /// not be reconciled with a frozen alpha (a resized or moved buffer) or
    /// when the restore left the layer byte-identical.
    ///
    /// For paint this is a backstop: the paint walks already kept the alpha
    /// and wrote the frozen-alpha colour rule (module doc), so the
    /// restore changes nothing there.
    ///
    /// A no-op for any entry that is not transparency-locked (by its own bit
    /// or an enclosing group's — [`DrDocument::alpha_frozen`]) and for every
    /// edit kind but `Pixels`: a Position edit resamples the alpha with
    /// everything else, a Mask edit never touches the layer's alpha at all,
    /// and a Mask APPLY and a Merge — the two that do — are refused outright by
    /// [`DrDocument::lock_block`] rather than silently un-applied.
    fn freezing_alpha(&self, idx: usize, kind: EditKind, out: Self) -> Option<Self> {
        self.layers.get(idx)?;
        if kind != EditKind::Pixels || !self.alpha_frozen(idx) {
            return Some(out);
        }
        self.restoring_alpha(idx, out)
    }

    /// `out` with entry `idx`'s original alpha channel restored
    /// UNCONDITIONALLY — [`Self::freezing_alpha`]'s restore without its
    /// kind/owner gate, shared with Edit ▸ Fill's Preserve Transparency
    /// (`doc_fill_stroke`) so there is one alpha restore in the crate.
    /// `None` when the edit resized or moved the buffer (a frozen alpha
    /// cannot be reconciled with it) or when the restore left the layer
    /// byte-identical.
    pub(crate) fn restoring_alpha(&self, idx: usize, out: Self) -> Option<Self> {
        let before = self.layers.get(idx)?;
        let after = out.layers.get(idx)?;
        if after.pixels.dimensions() != before.pixels.dimensions() || after.offset != before.offset
        {
            return None;
        }
        let mut restored = after.pixels.to_contiguous().ok()?;
        let raw: &mut [u8] = &mut restored;
        let (w, h) = before.pixels.dimensions();
        for y in 0..h {
            for (x, run) in before.pixels.row_segments(y, 0, w) {
                let at = (y as usize * w as usize + x as usize) * 4;
                for (dst, src) in raw[at..at + run.len() * 4].chunks_exact_mut(4).zip(run) {
                    if src[3] == 0 || dst[3] == 0 {
                        // Two pixels get their COLOUR back as well as their alpha.
                        //
                        // `src[3] == 0`: the layer holds nothing there, so the colour
                        // bytes under it are not part of the picture. Putting them
                        // back is what makes a stroke that landed only in the empty
                        // area a byte-exact no-op the latch below can see; otherwise
                        // it would come back "changed" in bytes nobody can look at,
                        // and mint an undo step for nothing.
                        //
                        // `dst[3] == 0`: the op drove the alpha to zero — an erase, or
                        // `clear_selection`, both of which zero the colour with it.
                        // Restoring only the alpha would resurrect the pixel in
                        // whatever colour the erase left behind (black), so an edit
                        // that tried to REMOVE coverage is refused outright instead.
                        dst.copy_from_slice(src);
                    } else {
                        dst[3] = src[3];
                    }
                }
            }
        }
        let unchanged = before.pixels.equals_image(&restored) && same_mask(after, before);
        if unchanged {
            return None;
        }
        let mut doc = out;
        doc.layers[idx].pixels = doc.layers[idx].pixels.rebuilt(restored);
        Some(doc)
    }
}

/// Whether two entries carry the same mask bytes and the same enabled flag —
/// the other half of [`DrDocument::freezing_alpha`]'s no-op latch, so an edit
/// that touched only the mask is not thrown away with the pixels.
pub(crate) fn same_mask(a: &Layer, b: &Layer) -> bool {
    a.mask_enabled == b.mask_enabled
        && a.mask_properties == b.mask_properties
        && match (a.mask.as_deref(), b.mask.as_deref()) {
            (None, None) => true,
            (Some(x), Some(y)) => x.dimensions() == y.dimensions() && x.as_raw() == y.as_raw(),
            _ => false,
        }
}
