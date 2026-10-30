//! KNOCKOUT (None / Shallow / Deep) and "Blend Interior Effects as Group" —
//! the two remaining Blending Options, at 8 bits. The model's two keys
//! live in `style` (`LayerStyle::knockout`, `LayerStyle::blend_interior_as_group`);
//! everything about what they MEAN is here, and `style_composite` /
//! `doc_group` / `doc.rs` only wire it up. The TYPED twin restates none of
//! these rules and calls into them: `precision_style_knockout`. What a MERGE
//! reads is `style_knockout_merge` (8 bits) and `precision_style_knockout`
//! (16). The PSD `knko`/`infx` records SHIP since 12E-3c, in both directions
//! and at both depths (`psd_layer_effects`, the ONE blending-options codec).
//!
//! Notation, straight RGBA: `over(B, C, a, M)` is the W3C source-over of
//! colour `C` at alpha `a` onto `B` with blend mode `M` (one implementation:
//! [`crate::blend::composite_source_into`]); `mix(A, K, t)` is the
//! premultiplied lerp [`crate::style_composite::mix_premultiplied`].
//!
//! # D3 — the order inside a styled unit
//!
//! The order: a non-Pass-Through group composites first in isolation,
//! and interior effects follow the pixels unless they are grouped. For a
//! styled unit:
//!
//! 1. the Blend If snapshot — "Underlying" reads the ACCUMULATED composite
//!    beneath the layer, NEVER the knockout backdrop;
//! 2. the Below effects onto the accumulator, untouched by knockout;
//! 3. the INTERIOR UNIT, evaluated against the knockout backdrop `B0` (D4)
//!    and mixed in by the SHAPE;
//! 4. the remaining Interior effects onto the accumulator as today — the
//!    inside band (`style_composite::replace_under_band`) replacing the
//!    unit's result under it with `over(B0, band)`, `B0` this same knockout
//!    backdrop, so a knocked-out layer's stroke punches as its pixels do.
//!
//! Layer opacity stays the package (`style_composite`'s strip path). Per
//! pixel, with `s = alpha·mask` (the shape), `q = fill·w` (`w` the Blend If
//! weight — Blend If is OPACITY, never shape: a pixel it hides still punches):
//!
//! ```text
//!     K   = over(B0, C, q, M)
//!     out = mix(acc, K, s·opacity)
//! ```
//!
//! With `B0 = acc` this IS today's pixel statement (`sa = s·q·opacity`, proved
//! by expanding both): so a layer with `Knockout::None` composites byte for
//! byte what it always did, and the routing below never engages for it. With
//! `q = 1` under Normal it equals the plain render for ANY `B0`, so a soft
//! edge gets no fringe. A clip BASE with visible members uses today's unit
//! buffer `U` (its own pixels at fill, each member frozen to the footprint
//! `s`): `K = over(B0, U.rgb, min(1, U.a/s), M)`, same mix. Dissolve tosses
//! its coin on `q`.
//!
//! # D4 — where the hole stops
//!
//! `B0` is a buffer ALIGNED with the accumulator in play, or transparent
//! ([`KnockoutBackdrop`], whose `None` means transparent):
//!
//! * inside an ISOLATED group's buffer, a merge, a bake, `rendered_group` or a
//!   per-item accumulator, BOTH modes stop at that buffer's initial state —
//!   transparent, so the hole shows what lies beneath the group once the group
//!   itself composites. An isolated buffer is a HARD STOP for Deep as well;
//! * for a CLIPPED MEMBER, both modes take the clip buffer after the BASE
//!   ALONE rendered. There are THREE member loops and all three obey it
//!   through [`member_env`]: the unstyled base
//!   (`doc::composite_clip_group_into`), the STYLED base
//!   (`style_composite::composite_direct_into`, which is also what each
//!   package strip runs) and a knockout/grouped base's own unit pass
//!   ([`composite_interior_unit_into`]). Each replaces `env.knockout` with the
//!   unit buffer as it stood right after the base's pixel pass and before any
//!   member — strip-aligned inside a package strip, because that buffer is
//!   cloned from the strip's own;
//! * Shallow inside a PASS-THROUGH group (plain or gated) is the accumulator
//!   as it stood when the group's children began. Shallow at the ROOT is Deep;
//! * Deep through any chain of pass-through groups is the BACKGROUND (D5),
//!   else transparent.
//!
//! # D5 — the Background, which Doctored has no kind for
//!
//! Entry 0 qualifies as the Background iff it is at depth 0, Raster and not an
//! adjustment, visible, not cut, Normal, opacity 1, without an enabled mask,
//! without a style, at offset (0, 0), exactly canvas-sized, and entry 1 (if
//! any) is unclipped ([`background_entry`]). `B0` is then the root accumulator
//! right after entry 0 composited.
//!
//! DISCLOSED DIVERGENCE (**D13**, decided by 12E-3c): Photoshop tells a true
//! Background layer from an ordinary opaque bottom layer and Doctored cannot
//! — the PSD record is where a Background is recognizable. D5 is KEPT
//! STRUCTURAL: `D5's Background divergence is a DELIBERATE PRODUCT DECISION,
//! not a measurement result: Doctored keeps the structural Background rule
//! and has no Background layer kind, so a Deep knockout (or a root Shallow
//! one) over an ordinary opaque canvas-sized bottom layer shows that layer in
//! Doctored and transparency in Photoshop. It is disclosed on PSD import and
//! export, measured as the DIVERGENCE case deep_over_ordinary_bottom, and
//! owned as carryover P7-38 (10C-3).` The two sides of the disclosure are
//! [`document_reaches_background`] here and
//! `psd_group_records::structural_background_knockout` on the way in.
//!
//! The TYPED twin (`precision_style_knockout::background_record`) is this rule
//! verbatim plus ONE clause the byte rule gets for free: **no Blend If**. The
//! byte model carries Blend If inside the style, so "no style" covers it; at
//! 16 bits Blend If is a layer field and has to be named.
//!
//! # D6 — Blend Interior Effects as Group
//!
//! The layer's blend mode applies to Inner Glow, Satin and the
//! Colour/Gradient/Pattern Overlays. ON moves exactly those five GROUPED
//! contributions — already the first five of the Interior order
//! ([`is_grouped_interior`]) — INTO the unit: the pixels go into `U` at fill
//! 1, each grouped contribution composites into `U` with its OWN blend and
//! opacity and `U`'s alpha is restored after each (the clip-member rule: they
//! recolour, they never add coverage), and the unit then meets `B0` (or `acc`
//! without a knockout) ONCE with the layer's mode at `q = fill·min(1, U.a/s)`.
//! Inner shadow, inside/centre stroke and bevel stay AFTER the unit; the
//! inside band then replaces the unit's result under it (step 4 of D3).
//!
//! On a clip base WITH visible members the unit is `U` = the base's pixels at
//! fill 1, then each visible member (frozen to the footprint `s`, `clip_unit`, as for a
//! knockout base), then the five grouped contributions, each restoring
//! `U.a`; the unit meets `B0` once at `q = fill·min(1, U.a/s)`. This is
//! P7-39: 12E-3b had narrowed the flag to OFF there without Photoshop
//! evidence (decision ii), 12E-3d measured Photoshop evaluating it ON
//! (max 255 / mean 34.08), and the narrowing is now LIFTED at both depths in
//! the full, regional and budget paths — Photoshop renders `Multiply(base,
//! overlay)` over the whole base footprint, the member's rectangle included
//! (`knockout_bottom/interior_on_clip_base_{8,16}.png`). Fill < 1 on such a
//! base pays the fill once by the unit, as D6 does, and is unmeasured.
//! The member LOOP inside [`composite_interior_unit_into`] reads the
//! resolved members, because that one IS a contribution question.
//!
//! # D7 — what a MERGE bakes
//!
//! Flatten, Stamp Visible and MERGE VISIBLE render the document's own
//! projection, so knockout bakes EXACTLY in all three — `doc_structure::
//! merge_visible` bakes `self.flattened()`, the one accumulator that carries
//! the Background snapshot, and is therefore not narrowed at all.
//!
//! Merge Down and Merge Layers bake into PRIVATE accumulators
//! (`doc::merging_down`, `doc_structure::merged_layer`), and since 12E-3b
//! those accumulators are HANDED the backdrop the merged unit would have read
//! from the document's own projection: `style_knockout_merge` runs one capture
//! render, windows the two planes onto the merge's extent and puts them in
//! `env.knockout`. So a hole is filled with the real backdrop's pixels and the
//! picture is KEPT wherever that backdrop is opaque.
//!
//! The RESIDUAL, which is real and disclosed rather than smoothed over: a
//! merged raster cannot un-draw what lies between, so where `B0` is ABSENT BY
//! STRUCTURE the hole still bakes as transparency. Inside an ISOLATED group
//! `B0` is always transparent (the hard stop above), so a knockout merge there
//! with a visible sibling beneath the merged set is NEVER exact; the same
//! holds at the root of a document with no qualifying Background, and for a
//! knockout nested in a merged pass-through group. The PSD export still bakes
//! against transparency. That residual is disclosed AT THE POINT OF ACTION —
//! the `merge_down` MCP reply carries `LayerStyle.knockoutMergeResidualNote`
//! (both branches, computed before the edit) and that tool's catalog text says
//! the same; `merge_visible` carries neither. The MENU items have no message
//! surface and gain none: the canvas shows the result and one Undo restores
//! it, while an MCP caller has no canvas, which is why the REPLY carries it.
//!
//! # D8 — what it costs
//!
//! One accumulator-sized f32 snapshot for the Background (only when a visible
//! style has `knockout != None` and D5 holds), one per pass-through level
//! holding a visible Shallow child (a GATED pass borrows the untouched
//! accumulator instead and allocates nothing extra), and one per clip unit
//! with a visible knockout member — unstyled, styled or unit-pass base alike,
//! and strip-sized inside a package strip. Each is charged to
//! `doc_group::RenderBudget` and counted by
//! `render_scratch::projection_scratch_bytes` through [`snapshot_pixels`]. A
//! document carrying NEITHER key allocates nothing here and projects
//! byte-identically: every entry point below short-circuits on the model's own
//! defaults before touching an allocation.
//!
//! # D10 — byte `.rz`
//!
//! [`BYTE_VERSION`] (**49**) is a CONDITIONAL version word with NO table of
//! its own — the keys ride inside the style JSON the record already carries —
//! nested INSIDE the always-written identity word 45
//! (`45 → 49 → 43 → 41 → 38 → 28 → body`), and written iff
//! [`styles_carry_keys`]. `DRDC_VERSION` is the byte reader's body-version
//! ceiling (57) and the FIRST word stays 45, so the host's version sniffer is
//! unchanged.
//!
//! The word exists because the style JSON alone cannot protect itself: an
//! older build reads a style LENIENTLY, would drop both keys, and a
//! knockout-only style would then look like an IDENTITY and be cleared — the
//! document would silently lose the setting. (Note the two depths differ
//! here: a TYPED knockout-only style IS stored, decision iv of 12E-3b, while
//! the byte identity rule keeps its own wording above.) With the word, an
//! older build
//! refuses the file by name (`unsupported DRDC version 49`), because 49 is
//! past its own `DRDC_VERSION`. This build makes the claim exact in both
//! directions, before a single PNG decodes: 49 with no such style is
//! `invalid native knockout word`, and such a style with no 49 is
//! `native knockout styles require DRDC 49`. The TYPED twin is word **50**
//! nested inside 46 after 47 and 48, with the same two sentences naming 50
//! (`precision_style_knockout`).
//!
//! `KnockoutBackdrop` is the pair of accumulator-ALIGNED backdrops in `CompositeEnv` (`NONE` — both
//! transparent — is every merge, bake, isolated-group buffer and per-item accumulator, and where
//! `composite_env` starts); `composite_projection_into` is the ONE place a backdrop is born from
//! real pixels (D5's Background: entry 0 when it is a plain, visible, unmasked, unstyled, Normal,
//! opacity-1, canvas-sized raster at (0,0) with entry 1 unclipped); `member_env` is the ONE
//! substitution all THREE clipped-member loops share (the unstyled base in `doc.rs`, the styled
//! base in `style_composite`, and the unit pass's own), and `composite_interior_unit_into` is the
//! unit itself — `K = over(B0, C, q, M)` then `out = mix(acc, K, s·opacity)`, which with `B0 = acc`
//! IS today's pixel statement, so a layer with `Knockout::None` never routes here. Blend If is
//! OPACITY and never shape (a pixel it hides still punches) and its Underlying is read from the
//! `BackdropSnapshot`, never from the unit buffer. Merge Down / Merge Layers bake a hole as
//! TRANSPARENCY (D7, disclosed on the `merge_down` MCP reply and in its catalog entry); Flatten,
//! Stamp Visible and Merge Visible render the projection and bake it EXACTLY, so none of those
//! three is narrowed or carries a note. **Phase 12E-3b** adds the TYPED twin
//! (`precision_style_knockout.rs`) and the EXACT merges (`style_knockout_merge.rs`): Merge Down and
//! Merge Layers now CAPTURE the backdrop the merged unit would have read from the document's own
//! projection and hand it to their private accumulator, so a hole is filled with the real
//! backdrop's pixels; only where `B0` is absent BY STRUCTURE (inside an isolated group, inside a
//! merged pass-through group, or clipped to a base outside the merge) does it still bake as
//! transparency, and THAT residual is what `LayerStyle.knockoutMergeResidualNote` discloses on the
//! `merge_down` reply. **Phase 12E-3c** carries PSD `knko`/`infx` in both directions and at both depths
//! through the ONE blending-options codec (`psd_layer_effects.rs`) and records **D13**: D5's
//! Background divergence is a DELIBERATE PRODUCT DECISION, disclosed once per file on BOTH import
//! and export — `document_reaches_background` here and
//! `psd_group_records::structural_background_knockout` on the way in are the two halves of that one
//! predicate — and owned as carryover P7-38 (10C-3). **12E-3d** measured D6's clip-base narrowing
//! against Photoshop (Photoshop evaluates `blend_interior_as_group` on a clip base with visible
//! members); P7-39 then LIFTED the narrowing at both depths, and its PSD disclosures went with it.

use crate::blend::{blend_kind, composite_source_into, BlendMode};
use crate::clip_unit::{clip_pass, ClipStep};
use crate::doc::{composite_layer_into, sane_opacity, DrDocument, Layer, LayerKind};
use crate::doc_group::{composite_level_into, RenderBudget, Unit};
use crate::style::{BlendIf, Knockout, LayerStyle};
use crate::style_composite::{
    clip_pass_shape, composite_contribution, composite_pixels_into, mix_premultiplied,
    BackdropSnapshot, CompositeEnv, Cut,
};
use crate::style_effects::{Effect, EffectKind};
use crate::style_render::{RenderedEffects, Shape, StyleColors};

/// Transparent black — what a knockout punches to when its backdrop is absent
/// (D4: an isolated buffer, a merge, a bake, a document with no Background).
const TRANSPARENT: [f32; 4] = [0.0; 4];

/// The two backdrops a knockout can punch to, each a buffer ALIGNED with the
/// accumulator in play; `None` means TRANSPARENT (D4), which is also what
/// every composite that is not the document's own projection gets.
///
/// A borrow, so [`CompositeEnv`] stays `Copy` — the same reason its `colors`
/// and `cut` are borrows.
#[derive(Clone, Copy)]
pub(crate) struct KnockoutBackdrop<'a> {
    pub shallow: Option<&'a [[f32; 4]]>,
    pub deep: Option<&'a [[f32; 4]]>,
}

impl KnockoutBackdrop<'_> {
    /// Both backdrops transparent: every merge, bake, isolated group buffer
    /// and per-item accumulator, and the state `composite_env` starts from.
    pub(crate) const NONE: Self = Self {
        shallow: None,
        deep: None,
    };
}

/// The conditional byte `.rz` version word (D10). No table: the two keys are
/// already inside each record's style JSON, and the word is the claim that
/// some record uses them.
pub(crate) const BYTE_VERSION: u32 = 49;

/// D10's predicate, asked by the WRITER to decide the word and by the READER
/// to check it: some layer's stored style carries a non-default key. It reads
/// `Layer::style` rather than `renders_style`, because the file stores what
/// the layer HOLDS — an adjustment layer's ignored style included.
pub(crate) fn styles_carry_keys(layers: &[Layer]) -> bool {
    layers.iter().any(|l| {
        l.style
            .as_deref()
            .is_some_and(|s| s.knockout != Knockout::None || s.blend_interior_as_group)
    })
}

// ------------------------------------------------------------- the scans --

/// Whether `style` needs the unit pass at all: a knockout, or a grouped
/// interior that has something to group. Everything else takes the shipped
/// path statement for statement.
pub(crate) fn routes_unit(style: &LayerStyle, planes: Option<&RenderedEffects>) -> bool {
    style.knockout != Knockout::None || grouped_interior(style, planes)
}

/// Whether the five GROUPED interior contributions move into the unit (D6):
/// the flag is on and at least one of them is actually rendered — on a clip
/// base with visible members too, since P7-39 lifted D6's narrowing.
pub(crate) fn grouped_interior(style: &LayerStyle, planes: Option<&RenderedEffects>) -> bool {
    style.blend_interior_as_group
        && planes.is_some_and(|p| p.interior.iter().any(|c| is_grouped_interior(c.kind)))
}

/// The five contributions "Blend Interior Effects as Group" moves (D6), which
/// are exactly the first five ranks of the Interior order.
pub(crate) fn is_grouped_interior(kind: EffectKind) -> bool {
    matches!(
        kind,
        EffectKind::PatternOverlay
            | EffectKind::GradientOverlay
            | EffectKind::ColorOverlay
            | EffectKind::Satin
            | EffectKind::InnerGlow
    )
}

/// C2's "renders": some ENABLED effect of `style` is one of the five D6
/// moves and is not skipped by its settings alone. It mirrors the renderers'
/// settings-only skip (`skipped_by_settings`), so an enabled colour overlay at
/// opacity 0 does not count; a shape-dependent no-op (all-zero coverage, a
/// colour-identity blend) still counts, because a question asked without
/// pixels cannot see it.
///
/// It rests on C2's premise: the flag moves only the five grouped
/// contributions, so a style that renders none of them composites the same
/// with the flag on or off. Its caller asks it about a style it has not
/// rendered: the 8-bit regional dry run (`doc_regional_budget::lone`), where
/// it is false only where the reference's `grouped_interior` is provably
/// false, so the planned unit pass never undercounts the real one.
pub(crate) fn has_grouped_interior_effect(style: &LayerStyle) -> bool {
    style.enabled_effects().any(moves_under_d6)
}

/// Whether one enabled effect is a D6 move its settings do not skip.
fn moves_under_d6(effect: &Effect) -> bool {
    match effect {
        Effect::PatternOverlay(fx) => !fx.skipped_by_settings(),
        Effect::GradientOverlay(fx) => !fx.skipped_by_settings(),
        Effect::ColorOverlay(fx) => !fx.skipped_by_settings(),
        Effect::Satin(fx) => !fx.skipped_by_settings(),
        Effect::InnerGlow(fx) => !fx.skipped_by_settings(),
        _ => false,
    }
}

/// The layer's own knockout, or `Knockout::None` when it renders no style
/// (a styled ADJUSTMENT's included: `Layer::renders_any_style`).
fn knockout_of(layer: &Layer) -> Knockout {
    layer
        .renders_any_style()
        .map_or(Knockout::None, |style| style.knockout)
}

/// Whether ANY visible entry of the stack asks for a knockout — the one test
/// that decides whether the projection pays for a Background snapshot at all
/// (D8). A document with no knockout takes the shipped path untouched.
pub(crate) fn document_wants_background(layers: &[Layer]) -> bool {
    layers
        .iter()
        .any(|l| l.visible && knockout_of(l) != Knockout::None)
}

/// Whether this LEVEL holds a visible Shallow child, i.e. whether a
/// pass-through group wrapping it must snapshot the accumulator its children
/// begin with (D4). Only the level's own entries are asked: a Shallow deeper
/// inside a nested pass-through group wants THAT level's snapshot, and one
/// inside an isolated group gets a transparent backdrop instead.
pub(crate) fn level_wants_shallow(layers: &[Layer], level: &[Unit]) -> bool {
    level.iter().any(|u| {
        layers
            .get(u.idx)
            .is_some_and(|l| l.visible && knockout_of(l) == Knockout::Shallow)
    })
}

/// Whether a clip unit must keep its BASE-ONLY snapshot (D4): some visible
/// member wants to punch, and the thing it punches to is the clip buffer right
/// after the base alone rendered. False for every clip unit in every document
/// without knockout, which is what keeps the clone off the shipped path.
pub(crate) fn members_want_base_snapshot(group: &[Layer]) -> bool {
    group
        .iter()
        .any(|l| l.visible && knockout_of(l) != Knockout::None)
}

/// D13: whether some visible entry's knockout REACHES the document's bottom —
/// Deep always, Shallow only at depth 0, where the two are the same thing.
/// This is the half of the PSD export disclosure that asks about the
/// knockout; [`background_entry`] asks about the bottom entry.
///
/// `written` is the caller's answer to "does this layer's style travel as
/// Photoshop's own blending options?" — a BAKED style writes no `knko` at
/// all, so the file it produces cannot punch through to transparency in
/// Photoshop and the disclosure would be false for it.
pub(crate) fn document_reaches_background(
    layers: &[Layer],
    written: impl Fn(&Layer) -> bool,
) -> bool {
    layers.iter().any(|layer| {
        layer.visible
            && match knockout_of(layer) {
                Knockout::Deep => true,
                Knockout::Shallow => layer.depth == 0,
                Knockout::None => false,
            }
            && written(layer)
    })
}

/// D5: the index of the entry that qualifies as the BACKGROUND, or `None`.
/// Only entry 0 can ever qualify, and every clause is a property the
/// projection can read without rendering anything.
pub(crate) fn background_entry(
    layers: &[Layer],
    canvas: (u32, u32),
    cut: Cut<'_>,
) -> Option<usize> {
    let entry = layers.first()?;
    if entry.depth != 0 || entry.kind != LayerKind::Raster {
        return None;
    }
    // An adjustment layer has no pixels of its own, so it can never BE the
    // backdrop — the same rule `Layer::renders_style` applies to styles.
    if entry
        .meta
        .as_deref()
        .and_then(crate::adjust::Adjustment::from_meta)
        .is_some()
    {
        return None;
    }
    if !entry.visible || cut.excludes(0) {
        return None;
    }
    if entry.blend != BlendMode::Normal || crate::opacity_byte::quantized(entry.opacity) < 1.0 {
        return None;
    }
    if entry.mask_enabled && entry.mask.is_some() {
        return None;
    }
    if entry.style.is_some() || entry.offset != (0, 0) {
        return None;
    }
    if entry.pixels.dimensions() != canvas {
        return None;
    }
    // A clipped entry 1 would make entry 0 a clip BASE, whose own composite is
    // not finished when its pixel pass is: the snapshot would be half a unit.
    if layers.get(1).is_some_and(|l| l.clipped) {
        return None;
    }
    Some(0)
}

/// D8's snapshot count for one document, in accumulator PIXELS — what
/// `render_scratch::projection_scratch_bytes` charges 16 bytes each for.
/// Deliberately conservative (an admission bound, never live usage): a level
/// that could hold a Shallow child is counted whether or not it does.
pub(crate) fn snapshot_pixels(doc: &DrDocument) -> u64 {
    if !document_wants_background(&doc.layers) {
        return 0;
    }
    let canvas = u64::from(doc.width) * u64::from(doc.height);
    // The Background snapshot, when entry 0 qualifies.
    let mut snapshots =
        u64::from(background_entry(&doc.layers, (doc.width, doc.height), Cut::NONE).is_some());
    if doc
        .layers
        .iter()
        .any(|l| l.visible && knockout_of(l) == Knockout::Shallow)
    {
        snapshots = snapshots.saturating_add(
            doc.layers
                .iter()
                .filter(|l| l.kind == LayerKind::Group)
                .count() as u64,
        );
    }
    snapshots = snapshots.saturating_add(
        doc.layers
            .iter()
            .filter(|l| l.clipped && l.visible && knockout_of(l) != Knockout::None)
            .count() as u64,
    );
    canvas.saturating_mul(snapshots)
}

// ------------------------------------------------------- the substitutions --

/// D4's ONE shared substitution for a clipped MEMBER, used by all three
/// member loops: both modes take the clip unit's base-only snapshot, never the
/// enclosing backdrop. `None` (no snapshot taken) means transparent, which is
/// what a member with no knockout would never read anyway.
pub(crate) fn member_env<'s>(
    env: CompositeEnv<'s>,
    base_only: Option<&'s [[f32; 4]]>,
) -> CompositeEnv<'s> {
    CompositeEnv {
        knockout: KnockoutBackdrop {
            shallow: base_only,
            deep: base_only,
        },
        ..env
    }
}

/// The two OWNED strip windows [`windowed`] answers, which the caller
/// re-borrows into a [`KnockoutBackdrop`]; `None` is a backdrop that was
/// absent (transparent) to begin with.
pub(crate) type BackdropWindows = (Option<Vec<[f32; 4]>>, Option<Vec<[f32; 4]>>);

/// `backdrop` restricted to one package strip: rows `top..top + rows` and
/// columns `x0..x1` of an `acc_w`-wide accumulator, copied so the strip's own
/// composite reads a strip-aligned buffer (D4/D8).
pub(crate) fn windowed(
    backdrop: KnockoutBackdrop<'_>,
    acc_w: u32,
    x0: i64,
    x1: i64,
    top: i64,
    rows: i64,
) -> BackdropWindows {
    let cut = |plane: &[[f32; 4]]| {
        let width = (x1 - x0) as usize;
        let mut out: Vec<[f32; 4]> = Vec::with_capacity(width * rows as usize);
        for y in top..top + rows {
            let row = y as usize * acc_w as usize;
            out.extend_from_slice(&plane[row + x0 as usize..row + x1 as usize]);
        }
        out
    };
    (backdrop.shallow.map(cut), backdrop.deep.map(cut))
}

// ------------------------------------------------------- the projection --

/// The document's own top-level composite, with D5's Background established
/// before anything above it draws. This is the ONE place a knockout backdrop
/// is born from real pixels; every other accumulator in the crate starts at
/// [`KnockoutBackdrop::NONE`], which is D4's rule for merges, bakes and
/// isolated group buffers.
///
/// A document that wants no knockout, or whose entry 0 does not qualify, runs
/// `doc_group::composite_level_into` on the whole level exactly as before.
pub(crate) fn composite_projection_into(
    acc: &mut [[f32; 4]],
    w: u32,
    h: u32,
    layers: &[Layer],
    level: &[Unit],
    env: CompositeEnv<'_>,
    budget: &RenderBudget,
) -> Result<(), String> {
    let background = document_wants_background(layers)
        .then(|| background_entry(layers, (w, h), env.cut))
        .flatten()
        // Only the bottom unit of the level can be the Background, and D5's
        // "entry 1 unclipped" is what guarantees that unit is entry 0 alone.
        .filter(|_| level.first().is_some_and(|u| u.idx == 0 && u.start == 0));
    let Some(_) = background else {
        // No Background: `composite_level_into` offers the whole root level
        // itself, so a merge AT the root captures `env.knockout` — both planes
        // absent, which is D4's transparent `B0`.
        return composite_level_into(acc, w, h, (0, 0), layers, level, env, 0, budget);
    };
    // Entry 0 draws with NO capture: the Background's own composite must
    // finish before anything may read the snapshot it produces.
    composite_level_into(
        acc,
        w,
        h,
        (0, 0),
        layers,
        &level[..1],
        CompositeEnv {
            capture: None,
            ..env
        },
        0,
        budget,
    )?;
    let _charge = budget.charge(acc.len() as u64)?;
    let snapshot = acc.to_vec();
    // Shallow at the ROOT is Deep (D4), so both read the
    // same plane.
    let env = CompositeEnv {
        knockout: KnockoutBackdrop {
            shallow: Some(&snapshot),
            deep: Some(&snapshot),
        },
        ..env
    };
    // The FULL root level is offered here rather than `level[1..]`, so a merge
    // INTO the Background (`at == 0`) captures this snapshot rather than the
    // `NONE` the remaining level would have offered.
    if let Some(capture) = env.capture {
        capture.offer(level, env.knockout, w, h, (0, 0), budget)?;
    }
    composite_level_into(acc, w, h, (0, 0), layers, &level[1..], env, 0, budget)
}

// -------------------------------------------------------- the unit pass --

/// D3's INTERIOR UNIT: the layer's pixels (then, on a clip base, its visible
/// members, then, under D6, its five grouped interior contributions)
/// evaluated against the knockout backdrop `B0` and mixed into `acc` by the
/// SHAPE.
///
/// Every piece of arithmetic here is the crate's existing one:
/// `composite_pixels_into` for the pixel pass and its Blend If weight,
/// `composite_contribution` for a grouped effect, `composite_layer_into` for a
/// member, `clip_pass_shape` for the footprint, `composite_source_into`
/// for `over` and `mix_premultiplied` for `mix`. Nothing is re-derived.
///
/// `blend_if` must carry a [`BackdropSnapshot`] covering the layer rect when
/// there is a Blend If at all: the pixel pass reads "Underlying" from its
/// TARGET where there is no snapshot, and this pass's target is the unit
/// buffer rather than the accumulator. `style_composite` widens the snapshot
/// rect for exactly that reason.
#[allow(clippy::too_many_arguments)]
pub(crate) fn composite_interior_unit_into(
    acc: &mut [[f32; 4]],
    acc_w: u32,
    acc_h: u32,
    origin: (i32, i32),
    layer: &Layer,
    group: &[Layer],
    style: &LayerStyle,
    opacity: f32,
    env: CompositeEnv<'_>,
    planes: Option<&RenderedEffects>,
    blend_if: Option<(&BlendIf, Option<&BackdropSnapshot>)>,
) -> Result<(), String> {
    let rel = (
        saturating_i32(i64::from(layer.offset.0) - i64::from(origin.0)),
        saturating_i32(i64::from(layer.offset.1) - i64::from(origin.1)),
    );
    let grouped = grouped_interior(style, planes);
    // D6: with the grouped interiors inside the unit the pixels enter at fill
    // 1 and the fill is paid ONCE by the whole unit; otherwise the pixels
    // carry it themselves, which is D3's `q = fill·w` and today's arithmetic.
    let (fill_in, fill_out) = if grouped {
        (1.0, crate::opacity_byte::quantized(style.fill_opacity))
    } else {
        (style.fill_opacity, 1.0)
    };
    env.job.check()?;
    let mut unit = vec![[0.0f32; 4]; acc.len()];
    env.job.check()?;
    composite_pixels_into(
        &mut unit,
        acc_w,
        acc_h,
        origin,
        layer,
        fill_in,
        blend_if,
        BlendMode::Normal,
        1.0,
        env.job,
    )?;
    if group.iter().any(|l| l.visible) {
        let local;
        let footprint: Option<&Shape> = match planes {
            Some(p) => Some(&p.shape),
            None => {
                local = Shape::of_layer(layer, 0, env.job)?;
                local.as_ref()
            }
        };
        let base_alpha: Vec<f32> = if footprint.is_none() {
            unit.iter().map(|px| px[3]).collect()
        } else {
            Vec::new()
        };
        let clip = |unit: &mut [[f32; 4]], step| match footprint {
            Some(shape) => clip_pass_shape(unit, acc_w, acc_h, rel, shape, step, env.job),
            None => {
                clip_pass(unit, &base_alpha, step);
                Ok(())
            }
        };
        // `clip_unit`'s Open (#578), then D4: the members punch to the unit
        // buffer as it stands RIGHT NOW — after the base's pixel pass,
        // opened, before any member — never to the enclosing backdrop.
        clip(&mut unit, ClipStep::Open)?;
        let base_only = members_want_base_snapshot(group).then(|| unit.clone());
        let members = member_env(env, base_only.as_deref());
        for member in group.iter().filter(|l| l.visible) {
            env.job.check()?;
            composite_layer_into(&mut unit, acc_w, acc_h, origin, member, members)?;
            clip(&mut unit, ClipStep::Settle)?;
        }
        clip(&mut unit, ClipStep::Close)?;
    }
    if grouped {
        // The grouped contributions RECOLOUR the unit — the base and, on a
        // clip base, its members already in it (P7-39) — and never add
        // coverage (D6, the clip-member rule), so the unit's alpha is
        // restored after each one.
        let unit_alpha: Vec<f32> = unit.iter().map(|px| px[3]).collect();
        if let Some(planes) = planes {
            for c in planes
                .interior
                .iter()
                .filter(|c| is_grouped_interior(c.kind))
            {
                env.job.check()?;
                // A grouped interior: `occluding_fill` is read for a Below
                // stroke alone.
                composite_contribution(
                    &mut unit,
                    acc_w,
                    acc_h,
                    origin,
                    rel,
                    &planes.shape,
                    c,
                    1.0,
                    0.0,
                    StyleColors::Encoded(env.colors),
                    env.job,
                )?;
                for (px, &a) in unit.iter_mut().zip(&unit_alpha) {
                    px[3] = a;
                }
            }
        }
    }

    // The shape `s` is read from the layer exactly as the pixel pass reads it
    // (`alpha·mask` in f32), never from the u8 coverage plane: the plane is
    // rounded to 1/255 and the mix weight would then be a code out on a soft
    // edge, which is the whole thing the "no fringe" claim rests on.
    let kind = blend_kind(layer.blend);
    let active_mask = layer.active_mask();
    let mask = active_mask.as_deref().map(|m| m.as_raw().as_slice());
    let (lw, lh) = layer.pixels.dimensions();
    let rel_x = i64::from(layer.offset.0) - i64::from(origin.0);
    let rel_y = i64::from(layer.offset.1) - i64::from(origin.1);
    let x0 = rel_x.max(0);
    let y0 = rel_y.max(0);
    let x1 = (rel_x + i64::from(lw)).min(i64::from(acc_w));
    let y1 = (rel_y + i64::from(lh)).min(i64::from(acc_h));
    // The layer-space span, computed once and only for a non-empty canvas
    // span (`doc::composite_layer_into`'s guard: no negative reaches the
    // `u32` cast).
    let (sx0, sx1) = if x0 < x1 {
        ((x0 - rel_x) as u32, (x1 - rel_x) as u32)
    } else {
        (0, 0)
    };
    let opacity = sane_opacity(opacity);
    for ay in y0..y1 {
        env.job.check()?;
        let ly = (ay - rel_y) as u64;
        for (sx, run) in layer.pixels.row_segments(ly as u32, sx0, sx1) {
            for (i, px) in run.iter().enumerate() {
                let lx = sx as u64 + i as u64;
                let ax = lx as i64 + rel_x;
                let mi = (ly * u64::from(lw) + lx) as usize;
                let coverage = mask.map_or(1.0, |m| f32::from(m[mi]) / 255.0);
                let s = f32::from(px[3]) / 255.0 * coverage;
                if s <= 0.0 {
                    continue;
                }
                let ai = (ay as u64 * u64::from(acc_w) + ax as u64) as usize;
                let u = unit[ai];
                let q = (fill_out * (u[3] / s).min(1.0)).clamp(0.0, 1.0);
                // A `q` of 0 is the HOLE: `K` stays `B0` and the mix writes the
                // backdrop straight in, which is the whole point of the key.
                let mut k = [backdrop_at(env.knockout, style.knockout, acc[ai], ai)];
                if q > 0.0 {
                    let canvas_xy = (ax + i64::from(origin.0), ay + i64::from(origin.1));
                    composite_source_into(&mut k, 0, [u[0], u[1], u[2]], q, kind, canvas_xy);
                }
                acc[ai] = mix_premultiplied(acc[ai], k[0], s * opacity);
            }
        }
    }
    Ok(())
}

/// `B0` for one pixel: the accumulator itself without a knockout (which makes
/// the unit pass identical to the plain composite), else the mode's backdrop
/// plane, else transparent (D4).
pub(crate) fn backdrop_at(
    backdrop: KnockoutBackdrop<'_>,
    knockout: Knockout,
    here: [f32; 4],
    ai: usize,
) -> [f32; 4] {
    let plane = match knockout {
        Knockout::None => return here,
        Knockout::Shallow => backdrop.shallow,
        Knockout::Deep => backdrop.deep,
    };
    plane
        .and_then(|b| b.get(ai).copied())
        .unwrap_or(TRANSPARENT)
}

fn saturating_i32(v: i64) -> i32 {
    v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}
