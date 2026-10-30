//! KNOCKOUT (None / Shallow / Deep) and "Blend Interior Effects as Group" at
//! 16 bits and at 32-bit float — the TYPED TWIN of `style_knockout`, Phase
//! 12E-3b.
//!
//! **`style_knockout` owns every RULE and this file restates none of its
//! own.** D3's order, D4's boundaries, D5's Background, D6's grouped
//! interiors (on a clip base with visible members too: P7-39 lifted D6's
//! narrowing), D7's merges and D8's cost are written there and are the
//! specification for everything here; the arithmetic is the crate's existing
//! one, called out to: [`crate::style_knockout::routes_unit`],
//! [`crate::style_knockout::grouped_interior`],
//! [`crate::style_knockout::is_grouped_interior`],
//! [`crate::style_knockout::backdrop_at`],
//! `style_composite::composite_contribution` / `clip_pass_shape` /
//! `mix_premultiplied`, `blend::composite_source_into` and
//! `style_blend_if::source_weight_16`. No new pixel math is written here.
//!
//! Since 12E-3c the PSD `knko`/`infx` records travel at this depth too, and
//! D5's Background divergence is recorded as **D13** — a deliberate product
//! decision, not a measurement (`style_knockout`'s module doc states it
//! verbatim). [`document_reaches_background16`] is the export half here.
//!
//! # The one genuinely typed piece: the SEED
//!
//! A byte projection always knows whether it is the document's own; a typed
//! render does not, because a MERGE renders a DERIVED members document through
//! the very same compositor. So the scene carries a [`KnockoutSeed`]:
//!
//! * `Projection { background }` — the document's own render. With
//!   `background` true, record 0 draws first with NO capture, the accumulator
//!   is snapshotted (Shallow ≡ Deep at the root, D4) and the rest of the level
//!   renders against it. That is D5.
//! * `Handed { shallow, deep }` — a DERIVED document. D5 can NEVER fire here,
//!   which is exactly the point: a knockout nested inside a Pass Through
//!   member of a merge would otherwise take the merge's LOWEST member for a
//!   Background and punch to a picture that was never on screen. Every members
//!   document renders `Handed` (`precision_merge::merged_pixels`), with the
//!   planes [`PrecisionDocument::level_backdrop`] captured when a DIRECT member
//!   wanted them and `Handed { None, None }` — transparent, D4's rule for a
//!   private accumulator — otherwise.
//!
//! # Accumulator alignment
//!
//! Every typed accumulator is ONE FRAME — the scene's own: the canvas at
//! (0, 0) for every projection, or a merge's union extent at its origin for a
//! members document (`precision_merge_extent`). A group buffer, a
//! pass-through copy and a clip unit's buffer are each one plane of that same
//! frame, and every knockout plane is indexed accumulator-locally, so the
//! byte [`crate::style_knockout::KnockoutBackdrop`] is used verbatim inside a
//! render. The one plane that changes frame is a merge's captured `B0`: it is
//! captured over the canvas and windowed onto the merge's frame by
//! `precision_merge_extent::windowed_seed`, through the byte merge's own
//! `style_knockout_merge::window_plane` — transparent outside the canvas.
//!
//! # The no-half-buffer rule
//!
//! A [`TypedCapture`] render is ABANDONED the moment it is filled. After every
//! recursive `render_layers` call — the clip arm's base and member calls, the
//! `Kind::Group` arm, the `Kind::PassThrough` arm, `render_styled`'s
//! group-pixel render and its `Members` callback — `env.capture` is tested
//! BEFORE `prepare_mask` and any `checked_alpha`, so a partial group can never
//! raise the alpha-floor `Err` a full render would not, and the capture
//! accumulator is never encoded.
//!
//! # `.rz` word 50
//!
//! [`TYPED_VERSION`] is a CONDITIONAL version word with NO table of its own —
//! the keys ride inside the style JSON the `LSTY` record already carries —
//! nested INSIDE the always-written identity word 46, after 47 and 48
//! (`46 → [47] → [48] → [50] → 44 → …`), and written iff
//! [`styles_carry_keys16`]. It exists for D10's reason: an older build reads a
//! style LENIENTLY, would drop both keys, and a knockout-only style would then
//! look like an IDENTITY and be cleared. With the word an older build refuses
//! the file by name. Both directions are made exact by the reader, ONCE and
//! UNCONDITIONALLY — so a file with no `LSTY` table is checked too — before a
//! single PNG or f32 payload decodes.
//!
//! the TYPED twin of `style_knockout` (Phase 12E-3b), restating none of its rules: it calls
//! `routes_unit`, `grouped_interior`, `is_grouped_interior`, `backdrop_at`,
//! `composite_contribution`, `clip_pass_shape`, `mix_premultiplied` and
//! `composite_source_into` and adds no pixel math. Every typed accumulator is one frame (the canvas
//! at (0, 0), or a merge's union extent) and every knockout plane is indexed accumulator-locally,
//! so the BYTE `KnockoutBackdrop` is used verbatim; only a merge's captured `B0` is windowed onto
//! its frame (`precision_merge_extent::windowed_seed`). The one genuinely typed piece is the SEED:
//! `KnockoutSeed::Projection { background }` is the document's own render (where D5 may fire) and
//! `KnockoutSeed::Handed` is every DERIVED members document of a merge, where D5 must never fire —
//! otherwise a knockout nested inside a Pass Through member would take the merge's lowest member
//! for a Background. `TypedCapture` is the typed merge capture, probed by
//! `PrecisionLayer::knockout_probe` and answered by `PrecisionScene::capture_level_backdrop`.
//! `TYPED_VERSION` 50 is the conditional `.rz` word with NO table, nested inside 46 after 47 and
//! 48; both of its reader sentences are evaluated ONCE and UNCONDITIONALLY after the optional
//! `LSTY` block, so a style-free file carrying 50 is refused too. A key-free typed file is pinned
//! against a stored 48-writer file (`tests/fixtures/rz_typed_48/`).

use std::cell::RefCell;
use std::sync::Arc;

use crate::blend::{composite_source_into, BlendMode};
use crate::clip_unit::{clip_pass, ClipStep};
use crate::doc::sane_opacity;
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_render::{PrecisionLayer, Progress};
use crate::precision_style_render::{
    composite_pixels_into, typed_shape, Members, StyleEnv, StyledLayer,
};
use crate::style::{BlendIf, Knockout, LayerStyle};
use crate::style_composite::{
    clip_pass_shape, composite_contribution, mix_premultiplied_in, BackdropSnapshot, Cut,
};
use crate::style_knockout::{backdrop_at, grouped_interior, is_grouped_interior, KnockoutBackdrop};
use crate::style_render::{RenderedEffects, Shape};

/// The conditional typed `.rz` version word (module doc). No table: the two
/// keys are already inside each `LSTY` record's style JSON, and the word is
/// the claim that some record uses them.
pub(crate) const TYPED_VERSION: u32 = 50;

/// D10's predicate at 16 bits, asked by the WRITER to decide the word and by
/// the READER to check it: some record's STORED style carries a non-default
/// key. It reads what the record HOLDS, exactly as the byte twin does.
pub(crate) fn styles_carry_keys16(document: &PrecisionDocument) -> bool {
    fn walk(layers: &[PrecisionDocumentLayer]) -> bool {
        layers.iter().any(|l| {
            l.style()
                .is_some_and(|s| s.knockout != Knockout::None || s.blend_interior_as_group)
                || l.children().is_some_and(walk)
        })
    }
    walk(&document.layers)
}

// ------------------------------------------------------------- the scans --

/// D5 at 16 bits: whether record 0 qualifies as the BACKGROUND. Every clause
/// is [`crate::style_knockout::background_entry`]'s, plus ONE the byte rule
/// gets for free — **no Blend If**. The byte model carries Blend If INSIDE the
/// style, so its "no style" clause covers it; at this depth Blend If is a
/// LAYER field and has to be named.
pub(crate) fn background_record(
    layers: &[PrecisionDocumentLayer],
    canvas: (u32, u32),
    cut: Cut<'_>,
    float: bool,
) -> bool {
    let Some(entry) = layers.first() else {
        return false;
    };
    // A CMYK raster qualifies on the same clauses (#1, `cmyk_stack`'s D5).
    let (dimensions, offset) = match &entry.content {
        Content::Raster { image, offset } => (image.dimensions(), offset),
        Content::CmykRaster { image, offset } => (image.dimensions(), offset),
        // A group and an adjustment both fail the byte rule's Raster clause.
        _ => return false,
    };
    if !entry.visible() || cut.excludes(entry.records - 1) {
        return false;
    }
    if entry.blend_mode != BlendMode::Normal
        || crate::opacity_byte::composite(entry.opacity(), float) < 1.0
    {
        return false;
    }
    if entry.mask().is_some_and(|m| m.enabled()) {
        return false;
    }
    if entry.style().is_some() || entry.blend_if.is_some() {
        return false;
    }
    if *offset != (0, 0) || dimensions != canvas {
        return false;
    }
    // A clipped record 1 would make record 0 a clip BASE, whose own composite
    // is not finished when its pixel pass is: the snapshot would be half a unit.
    !layers.get(1).is_some_and(|l| l.clipped())
}

/// Whether ANY contributing record asks for a knockout — the one test that
/// decides whether a typed projection pays for a Background snapshot at all
/// (D8). A document with no knockout takes the shipped path untouched.
pub(crate) fn document_wants_background16(
    layers: &[PrecisionDocumentLayer],
    start: usize,
    cut: Cut<'_>,
) -> bool {
    let mut offset = start;
    for layer in layers {
        let node_start = offset;
        let index = offset + layer.records - 1;
        offset += layer.records;
        if !layer.visible() || cut.excludes(index) {
            continue;
        }
        if layer.style().is_some_and(|s| s.knockout != Knockout::None) {
            return true;
        }
        if let Some(children) = layer.children() {
            if document_wants_background16(children, node_start, cut) {
                return true;
            }
        }
    }
    false
}

/// D13's typed twin of [`crate::style_knockout::document_reaches_background`]:
/// whether some visible record's knockout REACHES the document's bottom —
/// Deep anywhere, Shallow only in the ROOT slice. `start` is the postorder
/// index the slice begins at, exactly as `document_wants_background16` uses it.
/// `written` is the byte twin's parameter verbatim: a BAKED style writes no
/// `knko`, so the file it produces punches nothing in Photoshop.
pub(crate) fn document_reaches_background16(
    layers: &[PrecisionDocumentLayer],
    start: usize,
    cut: Cut<'_>,
    written: &dyn Fn(&PrecisionDocumentLayer) -> bool,
) -> bool {
    // `start` alone cannot say "root": the first child of the first group
    // also begins at 0. The flag is passed down instead.
    fn walk(
        layers: &[PrecisionDocumentLayer],
        start: usize,
        cut: Cut<'_>,
        root: bool,
        written: &dyn Fn(&PrecisionDocumentLayer) -> bool,
    ) -> bool {
        let mut offset = start;
        for layer in layers {
            let node_start = offset;
            let index = offset + layer.records - 1;
            offset += layer.records;
            if !layer.visible() || cut.excludes(index) {
                continue;
            }
            match layer.style().map_or(Knockout::None, |s| s.knockout) {
                Knockout::Deep if written(layer) => return true,
                Knockout::Shallow if root && written(layer) => return true,
                _ => {}
            }
            if let Some(children) = layer.children() {
                if walk(children, node_start, cut, false, written) {
                    return true;
                }
            }
        }
        false
    }
    walk(layers, start, cut, true, written)
}

/// D8's typed scratch for one layer, in BYTES: `16·n` for a routed unit
/// buffer, `4·n` for the grouped-interior alpha record and `16·n` for a clip
/// unit's base-only snapshot. Deliberately conservative — it charges a routed
/// unit for a `blend_interior_as_group` flag whose grouped contributions may
/// turn out not to be enabled, because the plan runs before any plane does.
pub(crate) fn knockout_scratch(
    routed: bool,
    grouped: bool,
    knockout_member: bool,
    canvas: (u32, u32),
) -> u64 {
    let n = u64::from(canvas.0) * u64::from(canvas.1);
    let mut bytes = 0u64;
    if routed {
        bytes = bytes.saturating_add(16u64.saturating_mul(n));
    }
    if grouped {
        bytes = bytes.saturating_add(4u64.saturating_mul(n));
    }
    if knockout_member {
        bytes = bytes.saturating_add(16u64.saturating_mul(n));
    }
    bytes
}

// --------------------------------------------------------------- the seed --

/// A captured backdrop plane, shared so a seed stays cheap to clone
/// (`PrecisionScene` derives `Clone`).
pub(crate) type Plane = Arc<Vec<[f32; 4]>>;

/// What the render's TOP accumulator seeds `env.knockout` with (module doc).
#[derive(Clone)]
pub(crate) enum KnockoutSeed {
    /// The document's OWN projection: D5 may establish a Background.
    Projection { background: bool },
    /// A DERIVED document (every members document of a merge): the planes are
    /// handed in and D5 never fires.
    Handed {
        shallow: Option<Plane>,
        deep: Option<Plane>,
    },
}

impl Default for KnockoutSeed {
    fn default() -> Self {
        KnockoutSeed::Projection { background: false }
    }
}

/// The two planes a capture records: Shallow and Deep, each absent when that
/// backdrop was transparent to begin with (D4).
pub(crate) type CapturedPlanes = (Option<Vec<[f32; 4]>>, Option<Vec<[f32; 4]>>);

/// The capture slot a merge puts into `StyleEnv::capture`: "record the
/// knockout backdrop of the level holding the PROBE record, then stop".
#[derive(Default)]
pub(crate) struct TypedCapture {
    slot: RefCell<Option<CapturedPlanes>>,
}

impl TypedCapture {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Whether the backdrop has been recorded — the ONE test every recursive
    /// arm makes before it touches its buffer again.
    pub(crate) fn is_filled(&self) -> bool {
        self.slot.borrow().is_some()
    }

    /// Records `knockout` if this level holds the probe. FIRST VISIT WINS.
    pub(crate) fn offer(&self, level: &[PrecisionLayer], knockout: KnockoutBackdrop<'_>) {
        if self.is_filled() || !level.iter().any(PrecisionLayer::is_probe) {
            return;
        }
        *self.slot.borrow_mut() = Some((
            knockout.shallow.map(<[[f32; 4]]>::to_vec),
            knockout.deep.map(<[[f32; 4]]>::to_vec),
        ));
    }

    /// Records `knockout` unconditionally — `render_layers` has already
    /// matched the probe at its loop top.
    pub(crate) fn fill(&self, knockout: KnockoutBackdrop<'_>) {
        if self.is_filled() {
            return;
        }
        *self.slot.borrow_mut() = Some((
            knockout.shallow.map(<[[f32; 4]]>::to_vec),
            knockout.deep.map(<[[f32; 4]]>::to_vec),
        ));
    }

    pub(crate) fn take(self) -> CapturedPlanes {
        self.slot.into_inner().unwrap_or((None, None))
    }
}

// --------------------------------------------------------- the projection --

/// The scene's TOP-LEVEL render, with the seed applied (module doc). A scene
/// whose seed is `Projection { background: false }` — every scene before this
/// phase and every scene with no knockout — runs `render_layers` on the whole
/// list exactly as it did.
pub(crate) fn render_projection(
    layers: &[PrecisionLayer],
    dims: (u32, u32),
    origin: (i32, i32),
    acc: &mut [[f32; 4]],
    progress: &mut Progress<'_>,
    env: &StyleEnv<'_>,
    seed: &KnockoutSeed,
) -> Result<(), String> {
    match seed {
        KnockoutSeed::Handed { shallow, deep } => {
            let seeded = StyleEnv {
                knockout: KnockoutBackdrop {
                    shallow: shallow.as_ref().map(|p| p.as_slice()),
                    deep: deep.as_ref().map(|p| p.as_slice()),
                },
                ..*env
            };
            crate::precision_render::render_layers(layers, dims, origin, acc, progress, &seeded)
        }
        KnockoutSeed::Projection { background: false } => {
            crate::precision_render::render_layers(layers, dims, origin, acc, progress, env)
        }
        KnockoutSeed::Projection { background: true } => {
            if layers.is_empty() {
                return Ok(());
            }
            // Record 0 draws with NO capture: the Background's own composite
            // must finish before anything may read the snapshot it produces.
            let first = StyleEnv {
                capture: None,
                ..*env
            };
            crate::precision_render::render_layers(
                &layers[..1],
                dims,
                origin,
                acc,
                progress,
                &first,
            )?;
            let snapshot = acc.to_vec();
            // Shallow at the ROOT is Deep (D4).
            let seeded = StyleEnv {
                knockout: KnockoutBackdrop {
                    shallow: Some(&snapshot),
                    deep: Some(&snapshot),
                },
                ..*env
            };
            // The FULL root level is offered here rather than `layers[1..]`,
            // so a merge INTO the Background captures this snapshot rather
            // than the `NONE` the remaining level would have offered.
            if let Some(capture) = seeded.capture {
                capture.offer(layers, seeded.knockout);
                if capture.is_filled() {
                    return Ok(());
                }
            }
            crate::precision_render::render_layers(
                &layers[1..],
                dims,
                origin,
                acc,
                progress,
                &seeded,
            )
        }
    }
}

// ---------------------------------------------------------- the unit pass --

/// D3's INTERIOR UNIT at 16 bits: the byte
/// [`crate::style_knockout::composite_interior_unit_into`] statement for
/// statement over the typed pixel twin. The shape `s = alpha · coverage` is
/// read in f32 from the layer's own samples, never from the u8 coverage plane
/// — the plane is rounded to 1/255 and the mix weight would then be a code out
/// on a soft edge, which is the whole thing D3's "no fringe" claim rests on.
#[allow(clippy::too_many_arguments)]
pub(crate) fn composite_interior_unit_into(
    acc: &mut [[f32; 4]],
    dims: (u32, u32),
    origin: (i32, i32),
    layer: &StyledLayer<'_>,
    env: &StyleEnv<'_>,
    planes: Option<&RenderedEffects>,
    blend_if: Option<(&BlendIf, Option<&BackdropSnapshot>)>,
    opacity: f32,
    members: Members<'_>,
    job: JobContext<'_>,
) -> Result<(), String> {
    let (acc_w, acc_h) = dims;
    let style: &LayerStyle = layer.style;
    let offset = layer.pixels.offset();
    let rel = (
        saturating_i32(i64::from(offset.0) - i64::from(origin.0)),
        saturating_i32(i64::from(offset.1) - i64::from(origin.1)),
    );
    let grouped = grouped_interior(style, planes);
    // D6: with the grouped interiors inside the unit the pixels enter at fill
    // 1 and the fill is paid ONCE by the whole unit; otherwise the pixels
    // carry it themselves, which is D3's `q = fill·w`.
    let (fill_in, fill_out) = if grouped {
        (
            1.0,
            crate::opacity_byte::composite(style.fill_opacity, env.float),
        )
    } else {
        (style.fill_opacity, 1.0)
    };
    let mut unit: Vec<[f32; 4]> = crate::precision_image::reserved_vec(acc.len())?;
    unit.resize(acc.len(), [0.0; 4]);
    composite_pixels_into(
        &mut unit,
        dims,
        origin,
        layer,
        fill_in,
        blend_if,
        BlendMode::Normal,
        1.0,
        env.float,
        env.lab(),
        job,
    )?;
    if let Some((count, render_member)) = members {
        let local;
        let footprint: Option<&Shape> = match planes {
            Some(p) => Some(&p.shape),
            None => {
                local = typed_shape(&layer.pixels, layer.mask, 0);
                local.as_ref()
            }
        };
        let base_alpha: Vec<f32> = if footprint.is_none() {
            unit.iter().map(|px| px[3]).collect()
        } else {
            Vec::new()
        };
        let clip = |unit: &mut [[f32; 4]], step| match footprint {
            Some(shape) => clip_pass_shape(unit, acc_w, acc_h, rel, shape, step, job),
            None => {
                clip_pass(unit, &base_alpha, step);
                Ok(())
            }
        };
        // `clip_unit`'s Open (#2), then D4: the members punch to the unit
        // buffer as it stands RIGHT NOW — after the base's pixel pass,
        // opened, before any member.
        clip(&mut unit, ClipStep::Open)?;
        let base_only = layer.members_want_base.then(|| unit.clone());
        for i in 0..count {
            render_member(i, &mut unit, base_only.as_deref())?;
            job.check()?;
            if env.capture.is_some_and(TypedCapture::is_filled) {
                return Ok(());
            }
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
                job.check()?;
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
                    env.style_colors(),
                    job,
                )?;
                for (px, &a) in unit.iter_mut().zip(&unit_alpha) {
                    px[3] = a;
                }
            }
        }
    }

    let kind = env.kind(layer.blend);
    let (lw, lh) = layer.pixels.dimensions();
    let rel_x = i64::from(offset.0) - i64::from(origin.0);
    let rel_y = i64::from(offset.1) - i64::from(origin.1);
    let x0 = rel_x.max(0);
    let y0 = rel_y.max(0);
    let x1 = (rel_x + i64::from(lw)).min(i64::from(acc_w));
    let y1 = (rel_y + i64::from(lh)).min(i64::from(acc_h));
    let opacity = sane_opacity(opacity);
    // The layer-space span, once per layer; `(0, 0)` unless the target span
    // is non-empty, so no negative reaches a `u32`.
    let (lx0, lx1) = if x0 < x1 {
        ((x0 - rel_x) as u32, (x1 - rel_x) as u32)
    } else {
        (0, 0)
    };
    for ay in y0..y1 {
        job.check()?;
        let ly = (ay - rel_y) as u32;
        layer.pixels.visit_row(ly, lx0, lx1, |lx, px| {
            let ax = i64::from(lx) + rel_x;
            let canvas_xy = (ax + i64::from(origin.0), ay + i64::from(origin.1));
            let coverage = layer.mask.map_or(1.0, |m| m.at(canvas_xy.0, canvas_xy.1));
            let s = px[3] * coverage;
            if s <= 0.0 {
                return Ok(());
            }
            let ai = (ay as u64 * u64::from(acc_w) + ax as u64) as usize;
            let u = unit[ai];
            let q = (fill_out * (u[3] / s).min(1.0)).clamp(0.0, 1.0);
            // A `q` of 0 is the HOLE: `K` stays `B0` and the mix writes the
            // backdrop straight in, which is the whole point of the key.
            let mut k = [backdrop_at(env.knockout, style.knockout, acc[ai], ai)];
            if q > 0.0 {
                composite_source_into(&mut k, 0, [u[0], u[1], u[2]], q, kind, canvas_xy);
            }
            acc[ai] = mix_premultiplied_in(acc[ai], k[0], s * opacity, !env.float);
            Ok::<(), String>(())
        })?;
    }
    Ok(())
}

fn saturating_i32(v: i64) -> i32 {
    v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

// ------------------------------------------------------- the level capture --

impl PrecisionDocument {
    /// The `B0` the record `member` would have read from THIS document's own
    /// projection, as a [`KnockoutSeed::Handed`] a derived members document can
    /// be rendered against (module doc). It ALWAYS answers `Handed`, including
    /// when the projection never reached that level — then both planes are
    /// absent, which is D4's transparent backdrop.
    pub(crate) fn level_backdrop(
        &self,
        member: usize,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<KnockoutSeed, String> {
        let (scene, extra) = self.scene_probe(member)?;
        let other = other
            .checked_add(extra)
            .ok_or("high-depth retained byte calculation overflow")?;
        let (shallow, deep) =
            scene.capture_level_backdrop(self.render_storage(), limit, other, job)?;
        Ok(KnockoutSeed::Handed {
            shallow: shallow.map(Arc::new),
            deep: deep.map(Arc::new),
        })
    }
}
