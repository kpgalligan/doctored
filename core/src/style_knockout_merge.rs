//! What a MERGE punches through to, at 8 bits — the half of D7 that Phase
//! 12E-3b makes EXACT. `style_knockout` owns every knockout RULE; this file
//! owns one mechanism: capturing the `B0` a merged unit would have read from
//! the document's OWN projection, and windowing it onto the merge's private
//! accumulator.
//!
//! # Why a capture and not a re-derivation
//!
//! `B0` is not a property of the merge set: it is the accumulator state the
//! projection had reached when that unit composited (D4 — the Background
//! snapshot at the root, a pass-through level's entry state, absent inside an
//! isolated group). The only thing that knows it is the projection, so the
//! merge RUNS one, with [`KnockoutCapture`] in the env, and stops the moment
//! the state is recorded.
//!
//! # The no-half-buffer rule
//!
//! A capture render is ABANDONED as soon as it is filled. Every arm that can
//! recurse into `doc_group::composite_level_into` therefore returns
//! immediately afterwards, BEFORE it would work on its half-rendered buffer:
//! `rendered_group` answers `Ok(None)` before `encode_projection` and the mask
//! crop, `composite_gated_pass` returns before its lerp loop, and
//! `composite_unit_into` returns after each `rendered_group` / `resolved_member`
//! and never reaches `composite_clip_group_into`. The capture accumulator is
//! never quantized. With `capture: None` — every ordinary composite, export,
//! bake and thumbnail — each of those is one `Option` test.
//!
//! # What the capture yields, by construction
//!
//! * at the ROOT — both planes are D5's Background snapshot, or absent when
//!   entry 0 does not qualify;
//! * inside a PASS-THROUGH parent (plain or gated, at any nesting) — `shallow`
//!   is the entry state its children began with, `deep` the Background;
//! * inside an ISOLATED group — `deep` absent and `shallow` that buffer's own
//!   pass-through entry state, or absent;
//! * a level the projection never reaches (a hidden ancestor) — absent.
//!
//! # The stated LIMIT (decision vi of 12E-3b)
//!
//! A merged raster cannot un-draw what lies between, so exactness holds
//! wherever `B0` is PRESENT and OPAQUE under the shape. Where `B0` is ABSENT
//! BY STRUCTURE the hole still bakes as TRANSPARENCY: inside an ISOLATED group
//! `B0` is always transparent (D4's hard stop), so a knockout merge there with
//! a visible sibling beneath the merged set is NEVER exact — before the merge
//! the hole shows what lies beneath the GROUP, after it shows that sibling.
//! The same holds at the root of a document with no qualifying Background.
//! That residual is DISCLOSED on the `merge_down` MCP reply
//! (`LayerStyle.knockoutMergeResidualNote`), and one byte test pins it.
//!
//! # One set is deliberately excluded
//!
//! A merge whose LOWEST member is CLIPPED to a base OUTSIDE the set is
//! BASELESS in the merge's private level, so it would read the LEVEL window
//! where the projection gave it the base-only snapshot — a wrong picture where
//! today's `NONE` is exact. [`merge_wants_backdrop`] excludes it and that merge
//! keeps `KnockoutBackdrop::NONE`, byte for byte what it bakes today.
//!
//! # Accounting (decision x)
//!
//! A charge lives exactly as long as its buffer. [`KnockoutCapture::offer`]
//! holds ONE charge across its copies — the PEAK CHECK, taken at the single
//! moment the nest, its snapshots and the copies coexist; after it the
//! projection only unwinds and the early returns allocate nothing, so that
//! moment is the maximum. Back in [`DrDocument::merge_backdrop`] the copies are
//! re-charged for as long as they live, each present window is charged
//! `uw·uh`, the copies and their charge drop, and the WINDOW charge travels
//! inside [`MergeBackdrop`] so it is held across the merge's own composite.
//! A refusal is `RenderBudget`'s existing sentence, which both callers answer
//! `None` to through `.ok()?`, exactly as `materialized` already does.
//!
//! `KnockoutCapture` rides in `CompositeEnv::capture` (beside `CompositeEnv::job`, the job the
//! chain polls: `NONE` for every full-frame composite, live only inside the regional render's
//! materialization, 13A-5b-ii-a) and records the knockout backdrop of the level holding the merged
//! unit; every arm that can recurse into `composite_level_into` returns the instant it is filled,
//! BEFORE it would work on its half-rendered buffer (`rendered_group` before `encode_projection`,
//! `composite_gated_pass` before its lerp, `composite_unit_into` before
//! `composite_clip_group_into`), and the capture accumulator is never quantized. `merge_backdrop`
//! windows the captured planes onto the merge's union extent (transparent outside the captured
//! accumulator) and returns them inside a `MergeBackdrop` that HOLDS their budget charge for as
//! long as they live (decision x's D8-style accounting, with ONE peak check inside `offer`).

use std::cell::RefCell;

use crate::doc::{DrDocument, Layer};
use crate::doc_group::{level_range, top_level, units, GroupCharge, RenderBudget, Unit};
use crate::style::Knockout;
use crate::style_composite::CompositeEnv;
use crate::style_knockout::KnockoutBackdrop;

/// The two planes one level of the projection held, with the accumulator they
/// were aligned to. `None` is a backdrop that was transparent to begin with.
pub(crate) struct CapturedBackdrop {
    pub shallow: Option<Vec<[f32; 4]>>,
    pub deep: Option<Vec<[f32; 4]>>,
    pub width: u32,
    pub height: u32,
    pub origin: (i32, i32),
}

/// The capture slot a merge puts into `CompositeEnv::capture`: "record the
/// knockout backdrop of the level holding unit `at`, then stop".
pub(crate) struct KnockoutCapture {
    at: usize,
    slot: RefCell<Option<CapturedBackdrop>>,
}

impl KnockoutCapture {
    pub(crate) fn new(at: usize) -> Self {
        Self {
            at,
            slot: RefCell::new(None),
        }
    }

    /// Whether the backdrop has been recorded — the ONE test every recursive
    /// arm makes before it touches its buffer again.
    pub(crate) fn is_filled(&self) -> bool {
        self.slot.borrow().is_some()
    }

    pub(crate) fn take(self) -> Option<CapturedBackdrop> {
        self.slot.into_inner()
    }

    /// Records `knockout` if this level holds the unit being merged. FIRST
    /// VISIT WINS, and a filled slot is left alone: the projection may reach
    /// the same level index again through an unrelated accumulator (a package
    /// strip, a gated pass), and the first arrival is the one the screen made.
    pub(crate) fn offer(
        &self,
        level: &[Unit],
        knockout: KnockoutBackdrop<'_>,
        acc_w: u32,
        acc_h: u32,
        origin: (i32, i32),
        budget: &RenderBudget,
    ) -> Result<(), String> {
        if self.is_filled() || !level.iter().any(|u| u.idx == self.at) {
            return Ok(());
        }
        let planes = u64::from(knockout.shallow.is_some()) + u64::from(knockout.deep.is_some());
        // The PEAK CHECK (module doc): held across the copies below, which is
        // the one moment the nest, its snapshots and these copies coexist.
        let _charge = budget.charge(planes * u64::from(acc_w) * u64::from(acc_h))?;
        *self.slot.borrow_mut() = Some(CapturedBackdrop {
            shallow: knockout.shallow.map(<[[f32; 4]]>::to_vec),
            deep: knockout.deep.map(<[[f32; 4]]>::to_vec),
            width: acc_w,
            height: acc_h,
            origin,
        });
        Ok(())
    }
}

/// The merge's two `B0` windows, aligned to its private accumulator, together
/// with the budget charge that keeps them accounted for as long as they live.
pub(crate) struct MergeBackdrop<'b> {
    pub shallow: Option<Vec<[f32; 4]>>,
    pub deep: Option<Vec<[f32; 4]>>,
    /// Dropped with the windows, never read — the charge IS the accounting
    /// (decision x).
    _charge: Option<GroupCharge<'b>>,
}

impl MergeBackdrop<'_> {
    /// Both planes transparent: every merge with no knockout member, every
    /// excluded set, and every capture the projection never filled. The merge
    /// then executes the statements it executes today.
    pub(crate) fn none() -> Self {
        Self {
            shallow: None,
            deep: None,
            _charge: None,
        }
    }

    pub(crate) fn backdrop(&self) -> KnockoutBackdrop<'_> {
        KnockoutBackdrop {
            shallow: self.shallow.as_deref(),
            deep: self.deep.as_deref(),
        }
    }
}

/// The layer's own knockout, or `None` when it renders no style — the same
/// question `style_knockout` asks of every entry (a styled adjustment's
/// included, `Layer::renders_any_style`).
fn knockout_of(layer: &Layer) -> Knockout {
    layer
        .renders_any_style()
        .map_or(Knockout::None, |style| style.knockout)
}

/// Whether this merge needs a real `B0` at all: some VISIBLE member's own
/// rendered style has `knockout != none`, AND the lowest member is not clipped
/// to a base outside the set (module doc). A nested knockout inside a member
/// GROUP does not count — the group is rasterized with `NONE` (D4's hard stop),
/// so it never reads the window.
pub(crate) fn merge_wants_backdrop(layers: &[Layer], members: &[usize]) -> bool {
    let Some(&lowest) = members.first() else {
        return false;
    };
    let wants = members.iter().any(|&i| {
        layers
            .get(i)
            .is_some_and(|l| l.visible && knockout_of(l) != Knockout::None)
    });
    wants && !clipped_to_outside_base(layers, lowest)
}

/// Whether `lowest` is clipped and some UNCLIPPED sibling lies below it in its
/// level — i.e. its clip base is outside the merge set.
fn clipped_to_outside_base(layers: &[Layer], lowest: usize) -> bool {
    if !layers.get(lowest).is_some_and(|l| l.clipped) {
        return false;
    }
    let depth = layers[lowest].depth;
    units(layers, level_range(layers, lowest), depth)
        .into_iter()
        .take_while(|u| u.idx != lowest)
        .any(|u| !layers[u.idx].clipped)
}

/// `plane` — an accumulator of `plane_dims` whose pixel (0, 0) is canvas pixel
/// `plane_origin` — re-indexed onto the merge's accumulator of `dims` at
/// `origin`: `W[x, y] = P[x + origin.x − p.origin.x, y + origin.y −
/// p.origin.y]` in i64 when inside the captured accumulator, transparent
/// outside it (and so outside the canvas). The ONE windowing of a captured
/// `B0`, at both depths: the typed merge calls it through
/// `precision_merge_extent::windowed_seed`.
pub(crate) fn window_plane(
    plane: &[[f32; 4]],
    plane_origin: (i32, i32),
    plane_dims: (u32, u32),
    origin: (i32, i32),
    dims: (u32, u32),
) -> Vec<[f32; 4]> {
    let (uw, uh) = dims;
    let dx = i64::from(origin.0) - i64::from(plane_origin.0);
    let dy = i64::from(origin.1) - i64::from(plane_origin.1);
    let mut out = Vec::with_capacity(uw as usize * uh as usize);
    for y in 0..i64::from(uh) {
        let py = y + dy;
        for x in 0..i64::from(uw) {
            let px = x + dx;
            let inside =
                px >= 0 && py >= 0 && px < i64::from(plane_dims.0) && py < i64::from(plane_dims.1);
            out.push(if inside {
                plane[(py * i64::from(plane_dims.0) + px) as usize]
            } else {
                [0.0; 4]
            });
        }
    }
    out
}

impl DrDocument {
    /// The `B0` windows for a merge of `members` into an accumulator of `dims`
    /// whose pixel (0, 0) is canvas pixel `origin` (module doc).
    /// [`MergeBackdrop::none`] — both planes transparent, today's behaviour
    /// statement for statement — whenever [`merge_wants_backdrop`] is false or
    /// the projection never reached the merged unit's level.
    pub(crate) fn merge_backdrop<'b>(
        &self,
        members: &[usize],
        origin: (i32, i32),
        dims: (u32, u32),
        budget: &'b RenderBudget,
    ) -> Result<MergeBackdrop<'b>, String> {
        let Some(&at) = members.first() else {
            return Ok(MergeBackdrop::none());
        };
        if !merge_wants_backdrop(&self.layers, members) {
            return Ok(MergeBackdrop::none());
        }
        let transfer = self.transfer()?;
        let colors = self.style_colors();
        let capture = KnockoutCapture::new(at);
        let env = CompositeEnv {
            capture: Some(&capture),
            ..self.composite_env(colors.as_ref(), transfer)
        };
        let px = u64::from(self.width) * u64::from(self.height);
        {
            // The capture accumulator, charged like every other projection
            // buffer and NEVER quantized: this render exists only to fill the
            // slot, and it is abandoned the instant it does.
            let _acc_charge = budget.charge(px)?;
            let mut acc = vec![[0.0f32; 4]; px as usize];
            let level = top_level(&self.layers);
            crate::style_knockout::composite_projection_into(
                &mut acc,
                self.width,
                self.height,
                &self.layers,
                &level,
                env,
                budget,
            )?;
        }
        let Some(captured) = capture.take() else {
            return Ok(MergeBackdrop::none());
        };
        let planes = u64::from(captured.shallow.is_some()) + u64::from(captured.deep.is_some());
        // The copies are re-charged for as long as they live here, and the
        // windows are charged before they are built (decision x).
        let copies =
            budget.charge(planes * u64::from(captured.width) * u64::from(captured.height))?;
        let charge = budget.charge(planes * u64::from(dims.0) * u64::from(dims.1))?;
        let shallow = captured.shallow.as_deref().map(|p| {
            window_plane(
                p,
                captured.origin,
                (captured.width, captured.height),
                origin,
                dims,
            )
        });
        let deep = captured.deep.as_deref().map(|p| {
            window_plane(
                p,
                captured.origin,
                (captured.width, captured.height),
                origin,
                dims,
            )
        });
        drop(captured);
        drop(copies);
        Ok(MergeBackdrop {
            shallow,
            deep,
            _charge: Some(charge),
        })
    }
}
