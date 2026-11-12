//! The styled-layer compositing path: how a layer with a style meets the
//! accumulator. The plain path in `doc::composite_layer_into` is never
//! touched for unstyled layers; a styled layer (or a styled clip BASE) is
//! routed here and assembled in this order:
//!
//! 1. If the style has a Blend If, the accumulator is snapshotted BEFORE
//!    anything of this layer is drawn — the "Underlying Layer" is the
//!    composite beneath the layer, and the layer's own shadow is not
//!    underlying. Only where a Below contribution can land inside the
//!    layer rect, though: everywhere else the accumulator still holds the
//!    underlying composite when the pixel pass reads it, so a Blend If
//!    with no Below effect — the common case, a plain styled layer — takes
//!    no snapshot at all rather than copying 16 bytes per layer pixel on
//!    every composite (320 MB per frame for a canvas-sized 20 MP layer).
//!    A styled clip BASE draws its pixels into a private buffer, not the
//!    accumulator, so it snapshots its whole layer rect (the buffer is
//!    already an accumulator-sized allocation; the snapshot is not the
//!    cost there).
//! 2. The Below group (drop shadow, outer glow, outside stroke, outer bevel
//!    halves) composites straight onto the accumulator, each contribution
//!    with its OWN blend mode and `sa = coverage x effect opacity` — never
//!    the layer's blend mode. An outside stroke band and an outer glow land
//!    at `sa / (1 − a)` under a pixel the layer will cover at `a`
//!    ([`occluding_fill`] · shape, `style_stroke_distance::band_under_layer`,
//!    #2), so band and layer join as the measured disjoint union and the
//!    glow reads the measured `g (1 − a)`. A drop shadow with Layer Knocks
//!    Out on is knocked out at its landing to `m (1 − a)` and lands there
//!    the same way (#2 part 4, [`SHADOW_UNDER_LAYER`]), so the visible
//!    shadow is the measured `m (1 − a)` at every Fill.
//! 3. The interior unit. A plain styled layer draws its pixels through the
//!    ordinary per-pixel loop with `sa = alpha x mask x fill_opacity x
//!    blend-if weight` and the LAYER's blend mode — no accumulator-sized
//!    buffer is allocated, so a fill-only or blend-if-only style costs the
//!    plain path plus two multiplies per pixel. A styled clip BASE (clipped
//!    members present) instead renders its pixels at Normal / opacity 1
//!    into a private accumulator-aligned buffer — the SAME full-window
//!    allocation the unstyled clip group makes today, a pre-existing cost —
//!    then each visible member through `composite_layer_into` onto the base
//!    with its alpha frozen relative to the SHAPE (`clip_unit`'s Open /
//!    Settle / Close, #2; the footprint is the shape at fill 1, so a base
//!    at 0 % fill still confines its members and its Fill stays relative), and
//!    finally composites the buffer with the layer's blend mode through
//!    `blend::composite_buffer_into`. A styled ADJUSTMENT has no
//!    pixels: its adjustment is applied in place here instead, after its
//!    own Below group (`style_adjustment`).
//!    Before step 3 draws anything, the accumulator under an inside band
//!    is kept as B0 ([`BandBackdrop`]: one index and one pixel per band
//!    pixel) — the stack beneath the layer after its Below group, or under a
//!    knockout the backdrop the layer punches to.
//! 4. The Interior group (gradient overlay, colour overlay, satin, inner
//!    glow, inner shadow, inside/centre stroke, inner bevel) composites
//!    straight onto the accumulator, each with its own blend mode — "Blend
//!    Interior Effects as Group" is OFF by default, so a Multiply
//!    text layer with a white colour overlay shows white. Every interior
//!    contribution is already bounded by the shape, which is what makes this
//!    correct without a buffer. The INSIDE BAND (an Inside Stroke, or a
//!    Center Stroke's inner half) is the one exception: it REPLACES
//!    the layer under it rather than drawing over it, as measured against
//!    Photoshop at every depth (`float-styles-473/take5`,
//!    `hard_normal_inside_beneath_*`).
//!    With `s = band / shape` the pixel `X` (step 3 and the interiors ranked
//!    before the band) becomes the premultiplied `lerp(X, over(B0, band at
//!    shape · opacity), s)` ([`replace_under_band_sample`], the one body the
//!    typed path and every regional sample call); the bevel and anything
//!    ranked after it composite over the result as before. No pass gains a
//!    parameter, so the replacement is applied exactly once whatever step 3
//!    was. Stacked strokes are several bands: B0 is kept once over
//!    their union, and each band in render order replaces over that same B0
//!    (measured against Photoshop, `stacked-effects-557/take1`), so a later
//!    band hides an earlier one where it lands.
//!
//! # Knockout and "Blend Interior Effects as Group"
//!
//! Step 3 is ROUTED to `style_knockout::composite_interior_unit_into` when
//! the style carries `knockout != none`, or `blend_interior_as_group` with
//! something to group — and only then. `style_knockout` owns every rule
//! (the evaluation order D3, the boundary rule D4, the Background D5, the
//! grouped interiors D6, what a merge bakes D7 and the cost D8); this file
//! holds NO knockout arithmetic of its own. Three things here serve it: the
//! routing above, the Blend If snapshot widened to the whole layer rect
//! (the unit pass writes into a private buffer, so "Underlying" cannot be
//! read from the target), and the styled clip BASE's member loop, which
//! takes the base-only snapshot and substitutes it through
//! `style_knockout::member_env` — D4's rule, shared with the other two
//! member loops. `composite_package_into` windows the backdrop with each
//! strip, since a backdrop is accumulator-aligned.
//!
//! Fill opacity therefore never touches effects: a text layer at 0 % fill
//! with a stroke and a drop shadow is outlined text.
//!
//! # Layer opacity: the package
//!
//! The LAYER opacity applies once, to the finished package of pixels and
//! effects, not to each piece: a colour overlay at opacity 1 on a layer at
//! 50 % shows pure overlay colour at 50 %, never a tint of the pixels under
//! it. So at opacity < 1 the four steps above run at opacity 1 against a
//! COPY of the accumulator's affected window, and the result is
//! mixed back in premultiplied space, `acc = acc + (packaged - acc) x
//! opacity` for colour x alpha and for alpha — which is exactly the W3C
//! formula for one source at `sa = alpha x opacity` (so a layer with no
//! effects composites the same bytes either way), and for several sources
//! the package model. The window is processed in strips of
//! `PACKAGE_STRIP_ROWS` rows so the copy stays small on a photo-sized
//! layer; every kernel here clips to its accumulator, so a strip is just a
//! smaller accumulator with a shifted origin (the Dissolve dither and the
//! Blend If snapshot both key on canvas coordinates, unaffected). Three
//! cases keep the direct per-contribution path: opacity 1 (nothing to mix),
//! a style that renders no planes (one source — identical result, no copy),
//! and a layer in Dissolve mode (its per-pixel coin toss on `sa` has no
//! package form).
//!
//! # The render context
//!
//! Effects are rendered against the DOCUMENT canvas (`CompositeEnv`: the
//! global light and the canvas size), never against the accumulator: Merge
//! Down composites into the union of two layer rects, and a canvas-aligned
//! gradient must bake exactly the pixels the projection showed.
//!
//! # A style's colours are AUTHORED, and convert here
//!
//! Every colour inside a style — an effect's `color`, a bevel's two lit
//! sides, a gradient's stops — is stored as the sRGB `#rrggbb` the user or
//! the agent authored, the same convention every colour argument this crate
//! is handed follows. It is converted into the DOCUMENT's colour space
//! exactly once, in [`composite_contribution`], which is the one place a
//! contribution's colour is read; `CompositeEnv::colors` carries the
//! conversion (absent when the document is already in that space, so an
//! sRGB document is byte-for-byte what it was before colour management).
//!
//! Converting HERE rather than when the plane is rendered is what keeps the
//! two properties that matter. The plane cache holds the AUTHORED colour, so
//! a document that changes profile — Assign, Convert to Profile, or an undo
//! of either — invalidates nothing and re-renders nothing; and the style
//! JSON is never rewritten, so `set_layer_style` stays idempotent, a `.rz`
//! round trip stays byte-identical, and Convert to Profile preserves a
//! style's APPEARANCE (its authored colour re-converts into the new space)
//! without touching a single stored value. The conversion is
//! `authored_color::AuthoredColors`: the shipped byte transform on a
//! matrix/TRC profile, and on an RGB LUT profile the relative, BPC-off LUT
//! pipeline. A LUT profile that cannot convert both ways keeps its
//! authored numbers verbatim, which is exactly what the host does when it
//! paints into such a document.
//!
//! # Merge Down's extent
//!
//! `merge_extent` is the rect a baked layer needs: its pixel rect grown by
//! the style's pad on every side (where the in-plane effects can reach),
//! and, for an enabled drop shadow, that rect shifted by the shadow's
//! composite-time offset — in the shadow's direction only, never `distance`
//! on all four sides — and clipped to the canvas, because the shadow beyond
//! the canvas is never visible and a legal 30000 px distance would
//! otherwise grow a merge to tens of megapixels or refuse it outright.
//!
//! the styled-layer compositing path `doc.rs` routes to: the Below / pixels / Interior order, Blend
//! If, the layer-opacity package model, Merge Down's extent, and the ONE place a style's AUTHORED
//! sRGB colour converts into the document's space (downstream of the plane cache, so a profile
//! change invalidates nothing and the stored JSON is never rewritten).

use crate::authored_color::AuthoredColors;
use crate::blend::{
    blend_kind, composite_buffer_into, composite_source_into, BlendKind, BlendMode,
};
use crate::blend_fill::{fill_blend, FillBlend};
use crate::clip_unit::{clip_alpha, clip_pass, ClipStep};
use crate::doc::{composite_layer_into, DrDocument, Layer};
use crate::doc_evaluation::{encoded_view, ByteTransfer};
use crate::job_control::JobContext;
use crate::style::{BlendIf, Effect, GlobalLight, Knockout, LayerStyle, StrokePosition};
use crate::style_adjustment::Adjusted;
use crate::style_blend_if::source_weight;
use crate::style_effects::EffectKind;
use crate::style_knockout;
use crate::style_render::{
    Contribution, Ink, InkDecode, Placement, RenderContext, RenderedEffects, Shape, StyleColors,
};
use crate::style_stroke_distance::band_under_layer;

/// What every composite reads beyond the layers: the global light every
/// "use global light" effect resolves against, the document canvas size
/// canvas-aligned gradients span, and the conversion an authored style
/// colour takes into the document's space (module doc).
///
/// `colors` is a BORROW so this stays `Copy` — a `Transform` is twelve
/// kilobytes of lookup tables (a LUT pipeline more), built once per projection by
/// `DrDocument::style_colors` and handed down. `cut` is a borrow for the same
/// reason, and because it is `None` for every projection the app draws.
#[derive(Clone, Copy)]
pub(crate) struct CompositeEnv<'a> {
    pub light: GlobalLight,
    pub canvas: (u32, u32),
    pub colors: Option<&'a AuthoredColors>,
    pub profile: &'a crate::icc::IccProfile,
    /// Entries that CONTRIBUTE NOTHING to this composite ([`Cut`]).
    pub cut: Cut<'a>,
    /// What a KNOCKOUT punches through to, in the accumulator now in play
    /// (`style_knockout`, D4). `KnockoutBackdrop::NONE` — both transparent —
    /// is every merge, bake, isolated group buffer and per-item accumulator,
    /// and is where `composite_env` starts; only the document's own
    /// projection (`style_knockout::composite_projection_into`) and the two
    /// wiring points below ever fill it in.
    pub knockout: crate::style_knockout::KnockoutBackdrop<'a>,
    /// The merge's `B0` CAPTURE, or `None` for every ordinary composite
    /// (`style_knockout_merge`). When it is `Some`, this projection is being
    /// run only to record the knockout backdrop one merged unit would have
    /// read, and every recursive arm returns as soon as it is filled — nothing
    /// downstream may work on a half-rendered buffer.
    pub capture: Option<&'a crate::style_knockout_merge::KnockoutCapture>,
    /// The document's EVALUATION POLICY as tables: `None` is `encoded` — every
    /// projection before Phase 11E, and the arithmetic below is then
    /// statement for statement what it was — and `Some` is `linear_light`,
    /// under which the accumulator holds LINEAR components. Obtained by the
    /// caller from `DrDocument::transfer`, never built here (`doc_evaluation`).
    pub transfer: Option<&'a ByteTransfer>,
    /// The job the chain polls (Phase 13A-5b-ii-a): `JobContext::NONE` for
    /// every full-frame composite — so every `?` the chain gained is a
    /// never-taken branch there — and live only inside the regional render's
    /// materialization (`doc_regional_style::resolve_materialized`, narrowed
    /// `checkpoints_only`).
    pub job: JobContext<'a>,
}

impl<'a> CompositeEnv<'a> {
    /// How a contribution's authored colour becomes its ink here: the byte
    /// statements (`Encoded`), decoded per sample into the accumulator under
    /// `linear_light` (`Linear8`, `style_render::StyleColors`).
    pub(crate) fn style_colors(&self) -> StyleColors<'a> {
        match self.transfer {
            None => StyleColors::Encoded(self.colors),
            Some(transfer) => StyleColors::Linear8(self.colors, transfer),
        }
    }
}

/// Entries that contribute nothing to one composite — the retouch sampling
/// modes' "everything above my layer", and nothing else today.
///
/// # Why this is not `visible`
///
/// The obvious spelling of "composite the stack without these entries" is to
/// clone the layers and clear `visible`. It is wrong, and the compositor's own
/// tests pin why: [`doc_group::is_isolated`] takes `has_visible_clipped_members`
/// (`doc_group::composite_unit_into` derives it from the real `visible` flags),
/// so hiding a clipped entry ABOVE the target flips the target's own enclosing
/// group from isolated to pass-through — and an adjustment layer inside that
/// group, which was confined to it, then reaches the whole backdrop.
/// `group_tests::clipping_over_a_pass_through_group_isolates_it` ends on
/// exactly that identity (`hidden == open`), so the flip is deliberate,
/// shipped behaviour for a HIDE. It is the wrong answer for a sampling plate,
/// whose whole job is to be the picture the user is looking at minus some
/// entries' contribution — the pixels BELOW the target must not move.
///
/// So a cut gates CONTRIBUTION only:
///
/// * [`doc_group::composite_level_into`] does not composite a cut unit;
/// * `extent_of_level` does not count one toward a group's buffer extent;
/// * `resolved_member` resolves one to an INVISIBLE placeholder, kept
///   positionally exactly as an invisible member already is;
/// * and `has_visible_clipped_members`, [`doc_group::is_isolated`] and the
///   positional clip-run scan keep reading the document's own flags, so every
///   structural decision is the one the screen made.
///
/// `Cut::NONE` is every existing caller, and `is_none` short-circuits the
/// index test, so a projection with no cut runs the statements it always ran.
#[derive(Clone, Copy, Default)]
pub(crate) struct Cut<'a> {
    excluded: Option<&'a [bool]>,
}

impl<'a> Cut<'a> {
    /// No entry is cut: every projection, export, thumbnail and merge.
    pub(crate) const NONE: Self = Self { excluded: None };

    /// `excluded[i]` marks `layers[i]` as contributing nothing. A slice
    /// shorter than the stack leaves the rest contributing, so a stale cut
    /// can never silently hide an entry it was not built for.
    pub(crate) fn new(excluded: &'a [bool]) -> Self {
        Self {
            excluded: Some(excluded),
        }
    }

    pub(crate) fn excludes(self, idx: usize) -> bool {
        self.excluded
            .is_some_and(|e| e.get(idx).copied().unwrap_or(false))
    }
}

impl DrDocument {
    /// The conversion every AUTHORED style colour takes into this document's
    /// space (module doc), or `None` when there is nothing to do: the
    /// document is already in sRGB — the overwhelmingly common case, and the
    /// one that must stay byte-for-byte what it was — or its profile is a
    /// LUT profile that cannot convert both ways (`authored_color::lut_ink`),
    /// where the honest answer is the numbers as authored, exactly as the
    /// host paints them.
    ///
    /// Owned by the caller and borrowed into [`CompositeEnv`], so one
    /// projection builds one conversion however many styled layers it has.
    pub(crate) fn style_colors(&self) -> Option<AuthoredColors> {
        AuthoredColors::into_document(&self.profile)
    }

    /// The document's compositing environment. `transfer` comes from
    /// [`DrDocument::transfer`], which is also the evaluation policy's refusal
    /// walk — every caller asks for it there rather than reading the field, so
    /// a projection of an unmigrated stack cannot be built at all.
    pub(crate) fn composite_env<'a>(
        &'a self,
        colors: Option<&'a AuthoredColors>,
        transfer: Option<&'a ByteTransfer>,
    ) -> CompositeEnv<'a> {
        CompositeEnv {
            light: self.global_light,
            canvas: (self.width, self.height),
            colors,
            profile: &self.profile,
            cut: Cut::NONE,
            knockout: crate::style_knockout::KnockoutBackdrop::NONE,
            capture: None,
            transfer,
            job: JobContext::NONE,
        }
    }
}

/// Rows per strip of the package path (module doc): 256 rows of a
/// 5000 px wide window are 20 MB of f32 pixels, small enough to stay in
/// cache-friendly memory and large enough that the per-strip pass over the
/// planes is a small fraction of the work.
const PACKAGE_STRIP_ROWS: i64 = 256;

fn saturating_i32(v: i64) -> i32 {
    v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// The styled-layer path (module doc). `opacity` is the layer's, already
/// sanitized and > 0; `group` is the clipped run above a clip BASE (empty
/// for a plain styled layer). `adjusted` is `None` for every layer but a
/// styled ADJUSTMENT (`style_adjustment`), whose `layer` is its shape
/// and whose interior step 3 applies in place of a pixel pass.
#[allow(clippy::too_many_arguments)]
pub(crate) fn composite_styled_into(
    acc: &mut [[f32; 4]],
    acc_w: u32,
    acc_h: u32,
    origin: (i32, i32),
    layer: &Layer,
    group: &[Layer],
    style: &LayerStyle,
    opacity: f32,
    env: CompositeEnv<'_>,
    adjusted: Option<&Adjusted<'_>>,
) -> Result<(), String> {
    // Warm the layer's aligned mask under the job (Phase 13A-5b-ii-a): every
    // later `active_mask()` reader of this layer is then a cache hit.
    layer.active_mask_with_job(env.job.checkpoints_only())?;
    let ctx = RenderContext {
        light: env.light,
        canvas: env.canvas,
        layer_offset: layer.offset,
        lab_tiles: false,
    };
    let planes = if style.has_enabled_effects() {
        style.rendered_with_job(layer, &ctx, env.job.checkpoints_only())?
    } else {
        None
    };
    match planes.as_deref() {
        Some(planes) if opacity < 1.0 && layer.blend != BlendMode::Dissolve => {
            composite_package_into(
                acc, acc_w, acc_h, origin, layer, group, style, opacity, env, planes, adjusted,
            )
        }
        planes => composite_direct_into(
            acc, acc_w, acc_h, origin, layer, group, style, opacity, env, planes, adjusted,
        ),
    }
}

/// The four steps of the module doc straight onto `acc`, every
/// contribution and the pixels scaled by `opacity`.
#[allow(clippy::too_many_arguments)]
fn composite_direct_into(
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
    adjusted: Option<&Adjusted<'_>>,
) -> Result<(), String> {
    let rel = (
        saturating_i32(i64::from(layer.offset.0) - i64::from(origin.0)),
        saturating_i32(i64::from(layer.offset.1) - i64::from(origin.1)),
    );
    // Knockout / "Blend Interior Effects as Group" route step 3 through
    // `style_knockout`'s unit pass; everything else is untouched.
    let routed = style_knockout::routes_unit(style, planes);
    // Module doc, step 1: the snapshot covers only what the Below group
    // can overwrite inside the layer rect; a plain layer with nothing
    // Below reads the accumulator itself. A unit pass ALWAYS needs the full
    // rect, because its pixel pass writes into a private buffer and would
    // otherwise read "Underlying" from that buffer's transparency.
    let snapshot_rect = match (group.is_empty() && !routed, planes) {
        (true, None) => None,
        (true, Some(p)) => below_landing(rel, &p.shape, &p.below),
        (false, _) => Some((i64::MIN, i64::MIN, i64::MAX, i64::MAX)),
    };
    let backdrop = style
        .blend_if
        .filter(|b| !b.is_identity())
        .map(|b| {
            Ok::<_, String>((
                b,
                snapshot_rect
                    .map(|r| {
                        BackdropSnapshot::of(
                            acc,
                            acc_w,
                            acc_h,
                            rel,
                            layer.pixels.dimensions(),
                            r,
                            env.job,
                        )
                    })
                    .transpose()?,
            ))
        })
        .transpose()?;
    let blend_if = backdrop.as_ref().map(|(b, s)| (b, s.as_ref()));
    let occluding = occluding_fill(
        style,
        style.blend_if.as_ref(),
        layer.blend,
        false,
        adjusted.is_some(),
    );

    if let Some(planes) = planes {
        for c in &planes.below {
            env.job.check()?;
            composite_contribution(
                acc,
                acc_w,
                acc_h,
                origin,
                rel,
                &planes.shape,
                c,
                opacity,
                occluding,
                env.style_colors(),
                env.job,
            )?;
        }
    }

    // Module doc, step 4: B0 for the inside bands, taken HERE — after the
    // Below group, before anything of the layer's own interior draws — ONCE
    // over the union of every band (stacked strokes replace in turn
    // over the same B0).
    let band = planes
        .map(|p| (p.inside_bands(), p))
        .filter(|(bands, _)| !bands.is_empty())
        .map(|(bands, p)| {
            let of: Vec<&Contribution> = bands.iter().map(|&j| &p.interior[j]).collect();
            BandBackdrop::of(
                acc,
                acc_w,
                acc_h,
                rel,
                &p.shape,
                &of,
                env.knockout,
                style.knockout,
                env.job,
            )
            .map(|b0| (bands, b0))
        })
        .transpose()?;

    env.job.check()?;
    // A styled ADJUSTMENT's interior is read HERE, after its own Below
    // group (POST), and applied in place (`style_adjustment`).
    if routed {
        let interior = adjusted
            .map(|a| a.interior_layer(acc, acc_w, acc_h, origin, layer, env))
            .transpose()?;
        let pixels = interior.as_ref().unwrap_or(layer);
        style_knockout::composite_interior_unit_into(
            acc, acc_w, acc_h, origin, pixels, group, style, opacity, env, planes, blend_if,
        )?;
    } else if group.is_empty() {
        match adjusted {
            Some(a) => a.interior_into(
                acc,
                acc_w,
                acc_h,
                origin,
                layer,
                style.fill_opacity,
                blend_if,
                opacity,
                None,
                env,
            )?,
            None => composite_pixels_into(
                acc,
                acc_w,
                acc_h,
                origin,
                layer,
                style.fill_opacity,
                blend_if,
                layer.blend,
                opacity,
                env.transfer,
                env.job,
            )?,
        }
    } else {
        let mut buf = vec![[0.0f32; 4]; acc.len()];
        // An adjustment base puts no pixels in the unit: its interior meets
        // the members' buffer below.
        if adjusted.is_none() {
            composite_pixels_into(
                &mut buf,
                acc_w,
                acc_h,
                origin,
                layer,
                style.fill_opacity,
                blend_if,
                BlendMode::Normal,
                1.0,
                env.transfer,
                env.job,
            )?;
        }
        // The footprint is the SHAPE: the effects' plane when there is
        // one, else a pad-0 shape built for the purpose. Only a layer too
        // large for even that (over MAX_PIXELS) falls back to the pixel
        // pass's own alpha, the unstyled group's rule.
        let local;
        let footprint: Option<&Shape> = match planes {
            Some(p) => Some(&p.shape),
            None => {
                local = Shape::of_layer(layer, 0, env.job)?;
                local.as_ref()
            }
        };
        let base_alpha: Vec<f32> = if footprint.is_none() {
            buf.iter().map(|px| px[3]).collect()
        } else {
            Vec::new()
        };
        // `clip_unit`'s Open, then D4's clipped-member rule, the STYLED
        // base's loop: the members punch to this buffer as it stands after
        // the base's pixel pass (opened) and before any member, never to the
        // enclosing backdrop. The clone happens only when a visible member
        // actually asks for it.
        let clip = |buf: &mut [[f32; 4]], step| match footprint {
            Some(shape) => clip_pass_shape(buf, acc_w, acc_h, rel, shape, step, env.job),
            None => {
                clip_pass(buf, &base_alpha, step);
                Ok(())
            }
        };
        // Only a visible member opens the unit: a run of hidden members
        // leaves the base's pass exactly as it rendered.
        let opened = group.iter().any(|l| l.visible);
        if opened {
            clip(&mut buf, ClipStep::Open)?;
        }
        let base_only = style_knockout::members_want_base_snapshot(group).then(|| buf.clone());
        let members = style_knockout::member_env(env, base_only.as_deref());
        for member in group.iter().filter(|l| l.visible) {
            env.job.check()?;
            composite_layer_into(&mut buf, acc_w, acc_h, origin, member, members)?;
            clip(&mut buf, ClipStep::Settle)?;
        }
        if opened {
            clip(&mut buf, ClipStep::Close)?;
        }
        env.job.check()?;
        match adjusted {
            Some(a) => a.interior_into(
                acc,
                acc_w,
                acc_h,
                origin,
                layer,
                style.fill_opacity,
                blend_if,
                opacity,
                Some(&buf),
                env,
            )?,
            None => composite_buffer_into(
                acc,
                &buf,
                acc_w,
                acc_h,
                origin,
                blend_kind(layer.blend),
                opacity,
                env.job,
            )?,
        }
    }

    if let Some(planes) = planes {
        // Under "Blend Interior Effects as Group" the five GROUPED
        // contributions were already composited INSIDE the unit; only inner
        // shadow, inside/centre stroke and bevel remain out here (D6).
        let grouped = style_knockout::grouped_interior(style, Some(planes));
        for (j, c) in planes
            .interior
            .iter()
            .enumerate()
            .filter(|(_, c)| !(grouped && style_knockout::is_grouped_interior(c.kind)))
        {
            env.job.check()?;
            match band.as_ref().filter(|(bands, _)| bands.contains(&j)) {
                // An inside band REPLACES what lies under it (module doc,
                // step 4): it composites over B0, not over the layer.
                Some((_, b0)) => replace_under_band(
                    acc,
                    acc_w,
                    acc_h,
                    origin,
                    rel,
                    &planes.shape,
                    c,
                    b0,
                    opacity,
                    env.style_colors(),
                    env.job,
                )?,
                None => composite_contribution(
                    acc,
                    acc_w,
                    acc_h,
                    origin,
                    rel,
                    &planes.shape,
                    c,
                    opacity,
                    occluding,
                    env.style_colors(),
                    env.job,
                )?,
            }
        }
    }
    Ok(())
}

/// Whether `style` can emit an inside band (an enabled Inside or Center
/// Stroke of size > 0), i.e. whether a composite of it may hold a
/// [`BandBackdrop`]: the predicate every scratch plan charges
/// [`BAND_BACKDROP_BYTES`] per layer pixel on.
pub(crate) fn holds_inside_band(style: &LayerStyle) -> bool {
    style.enabled_effects().any(|e| match e {
        Effect::Stroke(s) => s.position != StrokePosition::Outside && s.size > 0.0,
        _ => false,
    })
}

/// Bytes a [`BandBackdrop`] holds per band pixel: one u32 index and one
/// f32×4 pixel. The band lies inside the layer's shape, so a layer of
/// `w × h` holds at most `w·h` of them.
pub(crate) const BAND_BACKDROP_BYTES: u64 = 20;

/// B0 at the nonzero pixels of the UNION of the inside bands inside one
/// target, sorted by target index (module doc, step 4): the target after the
/// Below group and before step 3 — or, under a knockout,
/// `style_knockout::backdrop_at` of it, the backdrop the layer's own pixels
/// punch to. Stacked strokes share it: each band replaces over the
/// same B0, in render order. The union lies inside the layer's shape, so the
/// store is still at most one entry per layer pixel.
pub(crate) struct BandBackdrop {
    pixels: Vec<(u32, [f32; 4])>,
}

impl BandBackdrop {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn of(
        target: &[[f32; 4]],
        acc_w: u32,
        acc_h: u32,
        rel: (i32, i32),
        shape: &Shape,
        bands: &[&Contribution],
        knockout: style_knockout::KnockoutBackdrop<'_>,
        mode: Knockout,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let bands: Vec<&Contribution> = bands
            .iter()
            .copied()
            .filter(|b| b.coverage.len() == shape.coverage.len())
            .collect();
        let Some(first) = bands.first() else {
            return Ok(BandBackdrop { pixels: Vec::new() });
        };
        let banded = |i: usize| bands.iter().any(|b| b.coverage[i] != 0);
        // The shift is applied as `replace_under_band` applies it, so the two
        // loops visit the same pixels (always zero for an Interior band, so
        // every band shares the first one's).
        let (sx, sy) = (i64::from(first.shift.0), i64::from(first.shift.1));
        let base_x = i64::from(rel.0) + shape.origin.0 + sx;
        let base_y = i64::from(rel.1) + shape.origin.1 + sy;
        let px0 = (-base_x).max(0);
        let py0 = (-base_y).max(0);
        let px1 = (i64::from(acc_w) - base_x).min(i64::from(shape.w));
        let py1 = (i64::from(acc_h) - base_y).min(i64::from(shape.h));
        // Counted first, so the store is exactly `BAND_BACKDROP_BYTES` per
        // band pixel — what the scratch plans charge — and never a doubling
        // growth's spare capacity.
        let mut count = 0usize;
        for py in py0..py1 {
            job.check()?;
            for px in px0..px1 {
                count += usize::from(banded(shape.idx(px as u32, py as u32)));
            }
        }
        let mut pixels = Vec::with_capacity(count);
        for py in py0..py1 {
            job.check()?;
            let ay = base_y + py;
            for px in px0..px1 {
                if !banded(shape.idx(px as u32, py as u32)) {
                    continue;
                }
                let ai = (ay as u64 * u64::from(acc_w) + (base_x + px) as u64) as usize;
                // An index past u32 is never stored, as `at` never finds one,
                // so the store stays sorted and the band pixel is untouched.
                let Ok(key) = u32::try_from(ai) else {
                    continue;
                };
                let b0 = style_knockout::backdrop_at(knockout, mode, target[ai], ai);
                pixels.push((key, b0));
            }
        }
        Ok(BandBackdrop { pixels })
    }

    /// B0 at target index `ti`; `None` where the band has no pixel.
    pub(crate) fn at(&self, ti: usize) -> Option<[f32; 4]> {
        let ti = u32::try_from(ti).ok()?;
        self.pixels
            .binary_search_by_key(&ti, |&(i, _)| i)
            .ok()
            .map(|k| self.pixels[k].1)
    }
}

/// One plane pixel of the inside band's REPLACEMENT (module doc, step 4),
/// statement for statement — the one body the byte and typed direct paths
/// and every regional sample call. With `s = band / shape` (0 where the
/// shape is 0) the target `X` (step 3 and the interiors ranked before the
/// band) becomes the premultiplied `lerp(X, Y, s)`, `Y = over(B0, ink at
/// shape / 255 · scale)` with the band's own blend: at `s = 1` the band sits
/// over B0 alone, at `s = 0` the pixel is untouched. The share is the band's
/// part of the shape, so a gradient stroke's stop opacity (already in the
/// stored band) leaves the layer in place under a transparent stop. The mix
/// is unclamped: it is a convex combination of two in-range pixels at 8 and
/// 16 bits, and at 32 bits it must keep extended-range colour.
/// The caller guarantees what `composite_contribution_sample`'s does, but
/// for `scale`, which may be 0 (a band at opacity 0 shows B0 — `Ocut`).
/// `decode` is the scene's (`StyleColors::decode`).
#[allow(clippy::too_many_arguments)]
#[inline]
pub(crate) fn replace_under_band_sample(
    target: &mut [[f32; 4]],
    ti: usize,
    b0: [f32; 4],
    shape: &Shape,
    c: &Contribution,
    ink: &Ink,
    decode: InkDecode<'_>,
    plane_xy: (i64, i64),
    scale: f32,
    canvas_xy: (i64, i64),
) {
    let (px, py) = plane_xy;
    let i = shape.idx(px as u32, py as u32);
    let band = c.coverage[i];
    let cov = shape.coverage[i];
    if band == 0 || cov == 0 {
        return;
    }
    let s = (f32::from(band) / f32::from(cov)).min(1.0);
    let sa = f32::from(cov) / 255.0 * scale;
    let mut y = [b0];
    if sa > 0.0 {
        let cs = decode.apply(ink.color.at(px + shape.origin.0, py + shape.origin.1));
        composite_source_into(&mut y, 0, cs, sa, ink.kind, canvas_xy);
    }
    target[ti] = mix_premultiplied_in(target[ti], y[0], s, false);
}

/// `composite_contribution`'s loop over the band with
/// [`replace_under_band_sample`] as its body; `b0` is the band's
/// [`BandBackdrop`] over this same target.
#[allow(clippy::too_many_arguments)]
pub(crate) fn replace_under_band(
    target: &mut [[f32; 4]],
    acc_w: u32,
    acc_h: u32,
    origin: (i32, i32),
    rel: (i32, i32),
    shape: &Shape,
    c: &Contribution,
    b0: &BandBackdrop,
    opacity_scale: f32,
    colors: StyleColors<'_>,
    job: JobContext<'_>,
) -> Result<(), String> {
    // A band at opacity 0 still replaces (`style_fx_stroke`'s `Ocut`): only
    // NaN or a negative scale is nothing.
    let scale = c.opacity * opacity_scale;
    if scale.is_nan() || scale < 0.0 || c.coverage.len() != shape.coverage.len() {
        return Ok(());
    }
    let ink = c.ink(colors);
    let decode = colors.decode();
    let (sx, sy) = (i64::from(c.shift.0), i64::from(c.shift.1));
    let base_x = i64::from(rel.0) + shape.origin.0 + sx;
    let base_y = i64::from(rel.1) + shape.origin.1 + sy;
    let px0 = (-base_x).max(0);
    let py0 = (-base_y).max(0);
    let px1 = (i64::from(acc_w) - base_x).min(i64::from(shape.w));
    let py1 = (i64::from(acc_h) - base_y).min(i64::from(shape.h));
    if px0 >= px1 || py0 >= py1 {
        return Ok(());
    }
    for py in py0..py1 {
        job.check()?;
        let ay = base_y + py;
        for px in px0..px1 {
            // Outside the band the body is a no-op; skip it before the
            // store's binary search, so interior pixels cost one read.
            if c.coverage[shape.idx(px as u32, py as u32)] == 0 {
                continue;
            }
            let ax = base_x + px;
            let ai = (ay as u64 * u64::from(acc_w) + ax as u64) as usize;
            let Some(under) = b0.at(ai) else {
                continue;
            };
            let canvas_xy = (ax + i64::from(origin.0), ay + i64::from(origin.1));
            replace_under_band_sample(
                target,
                ai,
                under,
                shape,
                c,
                &ink,
                decode,
                (px, py),
                scale,
                canvas_xy,
            );
        }
    }
    Ok(())
}

/// The package path of the module doc: the direct path at opacity 1 over
/// strips of a copy of the affected window, mixed back at `opacity`.
#[allow(clippy::too_many_arguments)]
fn composite_package_into(
    acc: &mut [[f32; 4]],
    acc_w: u32,
    acc_h: u32,
    origin: (i32, i32),
    layer: &Layer,
    group: &[Layer],
    style: &LayerStyle,
    opacity: f32,
    env: CompositeEnv<'_>,
    planes: &RenderedEffects,
    adjusted: Option<&Adjusted<'_>>,
) -> Result<(), String> {
    let rel = (
        saturating_i32(i64::from(layer.offset.0) - i64::from(origin.0)),
        saturating_i32(i64::from(layer.offset.1) - i64::from(origin.1)),
    );
    let (x0, y0, x1, y1) = affected_window(rel, &planes.shape, &planes.below);
    let (x0, y0) = (x0.max(0), y0.max(0));
    let (x1, y1) = (x1.min(i64::from(acc_w)), y1.min(i64::from(acc_h)));
    if x1 <= x0 || y1 <= y0 {
        return Ok(());
    }
    let width = (x1 - x0) as usize;
    let mut top = y0;
    while top < y1 {
        let rows = (y1 - top).min(PACKAGE_STRIP_ROWS);
        env.job.check()?;
        let mut strip: Vec<[f32; 4]> = Vec::with_capacity(width * rows as usize);
        for y in top..top + rows {
            env.job.check()?;
            let row = y as usize * acc_w as usize;
            strip.extend_from_slice(&acc[row + x0 as usize..row + x1 as usize]);
        }
        let strip_origin = (
            saturating_i32(i64::from(origin.0) + x0),
            saturating_i32(i64::from(origin.1) + top),
        );
        // A knockout backdrop is accumulator-ALIGNED (D4), so a strip gets
        // the same window of it that the strip itself is; without a knockout
        // both planes are absent and nothing is copied.
        let (shallow, deep) = style_knockout::windowed(env.knockout, acc_w, x0, x1, top, rows);
        let strip_env = CompositeEnv {
            knockout: style_knockout::KnockoutBackdrop {
                shallow: shallow.as_deref(),
                deep: deep.as_deref(),
            },
            ..env
        };
        composite_direct_into(
            &mut strip,
            width as u32,
            rows as u32,
            strip_origin,
            layer,
            group,
            style,
            1.0,
            strip_env,
            Some(planes),
            adjusted,
        )?;
        env.job.check()?;
        for (i, y) in (top..top + rows).enumerate() {
            env.job.check()?;
            let row = y as usize * acc_w as usize + x0 as usize;
            for x in 0..width {
                let dst = &mut acc[row + x];
                *dst = mix_premultiplied(*dst, strip[i * width + x], opacity);
            }
        }
        top += rows;
    }
    Ok(())
}

/// The window (`(x0, y0, x1, y1)`, exclusive, in accumulator coordinates,
/// UNCLIPPED) a styled layer at `rel` can touch: its plane — which covers
/// the layer rect and every unshifted contribution — extended by the Below
/// contributions' shifts. Callers clip it to their accumulator. Shared with
/// the typed package path (`precision_style_render`).
pub(crate) fn affected_window(
    rel: (i32, i32),
    shape: &Shape,
    below: &[Contribution],
) -> (i64, i64, i64, i64) {
    let base_x = i64::from(rel.0) + shape.origin.0;
    let base_y = i64::from(rel.1) + shape.origin.1;
    let (mut min_sx, mut min_sy, mut max_sx, mut max_sy) = (0i64, 0i64, 0i64, 0i64);
    for c in below {
        min_sx = min_sx.min(i64::from(c.shift.0));
        min_sy = min_sy.min(i64::from(c.shift.1));
        max_sx = max_sx.max(i64::from(c.shift.0));
        max_sy = max_sy.max(i64::from(c.shift.1));
    }
    (
        base_x + min_sx,
        base_y + min_sy,
        base_x + i64::from(shape.w) + max_sx,
        base_y + i64::from(shape.h) + max_sy,
    )
}

/// The union `(x0, y0, x1, y1)` (window coordinates, exclusive, unclipped)
/// of where `below` can land: each contribution's plane rect shifted by its
/// shift. `None` with no Below contribution.
pub(crate) fn below_landing(
    rel: (i32, i32),
    shape: &Shape,
    below: &[Contribution],
) -> Option<(i64, i64, i64, i64)> {
    let base_x = i64::from(rel.0) + shape.origin.0;
    let base_y = i64::from(rel.1) + shape.origin.1;
    below
        .iter()
        .map(|c| {
            let (x, y) = (base_x + i64::from(c.shift.0), base_y + i64::from(c.shift.1));
            (x, y, x + i64::from(shape.w), y + i64::from(shape.h))
        })
        .reduce(|a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)))
}

/// `backdrop + (packaged - backdrop) x t` in premultiplied space (module
/// doc), returned straight; transparent black when the mixed alpha is 0.
pub(crate) fn mix_premultiplied(backdrop: [f32; 4], packaged: [f32; 4], t: f32) -> [f32; 4] {
    mix_premultiplied_in(backdrop, packaged, t, true)
}

/// [`mix_premultiplied`] with its colour clamp to [0, 1] as a parameter: a
/// 32-bit float scene (`clamp == false`) mixes extended-range
/// colour, which the clamp would cut — the 32-bit package model, measured
/// against Photoshop, keeps a 4.0 backdrop and a 2.0 source unclamped
/// (`float_styles.py`). Every byte and 16-bit caller clamps, statement for
/// statement.
pub(crate) fn mix_premultiplied_in(
    backdrop: [f32; 4],
    packaged: [f32; 4],
    t: f32,
    clamp: bool,
) -> [f32; 4] {
    mix_premultiplied_planes(backdrop, packaged, t, clamp)
}

/// [`mix_premultiplied_in`] over `N` planes, the LAST one alpha: the ONE
/// premultiplied mix, `N = 4` for RGBA (statement for statement the body
/// above always ran) and `N = 5` for a CMYK document's CMYKA (#1,
/// `cmyk_style_row`: the layer-opacity package and D3's `mix`).
pub(crate) fn mix_premultiplied_planes<const N: usize>(
    backdrop: [f32; N],
    packaged: [f32; N],
    t: f32,
    clamp: bool,
) -> [f32; N] {
    let a = N - 1;
    let (ba, pa) = (backdrop[a], packaged[a]);
    let alpha = ba + (pa - ba) * t;
    if alpha <= 0.0 {
        return [0.0; N];
    }
    let mut out = [0.0f32; N];
    for c in 0..a {
        let b = backdrop[c] * ba;
        let p = packaged[c] * pa;
        let v = (b + (p - b) * t) / alpha;
        out[c] = if clamp { v.clamp(0.0, 1.0) } else { v };
    }
    out[a] = alpha.min(1.0);
    out
}

/// `acc` inside (layer rect ∩ window ∩ `rect`), copied before the layer
/// draws anything (module doc, step 1). `x0`/`y0` are window coordinates.
/// Shared with the typed styled path, which hands in the layer's size.
pub(crate) struct BackdropSnapshot {
    x0: i64,
    y0: i64,
    w: usize,
    h: usize,
    px: Vec<[f32; 4]>,
}

impl BackdropSnapshot {
    pub(crate) fn of(
        acc: &[[f32; 4]],
        acc_w: u32,
        acc_h: u32,
        rel: (i32, i32),
        (lw, lh): (u32, u32),
        rect: (i64, i64, i64, i64),
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let x0 = i64::from(rel.0).max(0).max(rect.0);
        let y0 = i64::from(rel.1).max(0).max(rect.1);
        let x1 = (i64::from(rel.0) + i64::from(lw))
            .min(i64::from(acc_w))
            .min(rect.2);
        let y1 = (i64::from(rel.1) + i64::from(lh))
            .min(i64::from(acc_h))
            .min(rect.3);
        if x1 <= x0 || y1 <= y0 {
            return Ok(BackdropSnapshot {
                x0,
                y0,
                w: 0,
                h: 0,
                px: Vec::new(),
            });
        }
        let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
        let mut px = Vec::with_capacity(w * h);
        for ay in y0..y1 {
            job.check()?;
            let row = ay as usize * acc_w as usize;
            px.extend_from_slice(&acc[row + x0 as usize..row + x1 as usize]);
        }
        Ok(BackdropSnapshot { x0, y0, w, h, px })
    }

    /// The snapshotted pixel at window position `(ax, ay)`; `None` outside
    /// the snapshot, where the accumulator is still the backdrop.
    pub(crate) fn at(&self, ax: i64, ay: i64) -> Option<[f32; 4]> {
        let x = ax - self.x0;
        let y = ay - self.y0;
        if x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 {
            return None;
        }
        Some(self.px[y as usize * self.w + x as usize])
    }
}

/// The layer's pixels into `target` with
/// `sa = alpha x mask x blend-if weight x joint(fill_opacity, opacity)` (the
/// two bytes' product rounded to a byte, `opacity_byte::joint`) and blend
/// `mode`; the Blend If weight reads the backdrop snapshot where there is
/// one and `target` itself (still the underlying composite there, read
/// before the pixel is written) everywhere else; `None` = weight 1.
/// Master copy of the iteration: `doc::composite_layer_into` —
/// this is its variant with one extra multiply (the Blend If weight); the
/// arithmetic is kept identical, and both values are the opacity byte (`opacity_byte::
/// quantized`, #2), so a fill-opacity edit and a layer-opacity edit of the
/// same value composite byte-identically. A fresh transparent target pixel is
/// written directly as `[cs, sa]` under Normal (the kernel's `sa*cs/sa`
/// would only add rounding), so the clip-base buffer holds the base pixels
/// exactly.
#[allow(clippy::too_many_arguments)]
pub(crate) fn composite_pixels_into(
    target: &mut [[f32; 4]],
    acc_w: u32,
    acc_h: u32,
    origin: (i32, i32),
    layer: &Layer,
    fill_opacity: f32,
    blend_if: Option<(&BlendIf, Option<&BackdropSnapshot>)>,
    mode: BlendMode,
    opacity: f32,
    transfer: Option<&ByteTransfer>,
    job: JobContext<'_>,
) -> Result<(), String> {
    let fill = crate::opacity_byte::composite(fill_opacity, false);
    if fill <= 0.0 || opacity <= 0.0 {
        return Ok(());
    }
    let kind = blend_kind(mode);
    let direct = mode == BlendMode::Normal;
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
    let pass = PixelPass {
        joint: crate::opacity_byte::joint(fill, opacity, false),
        blend_if: blend_if.map(|(b, _)| b),
        kind,
        direct,
        opacity,
        fill_blend: fill_blend(mode, fill, false),
        transfer,
    };
    for ay in y0..y1 {
        job.check()?;
        let ly = (ay - rel_y) as u64;
        for (sx, run) in layer.pixels.row_segments(ly as u32, sx0, sx1) {
            let raw = run.as_flattened();
            for i in 0..run.len() {
                let lx = sx as u64 + i as u64;
                let ax = lx as i64 + rel_x;
                let mi = (ly * u64::from(lw) + lx) as usize;
                let li = i * 4;
                let coverage = mask.map_or(1.0, |m| f32::from(m[mi]) / 255.0);
                let ai = (ay as u64 * u64::from(acc_w) + ax as u64) as usize;
                let under = match blend_if {
                    None => target[ai],
                    Some((_, snapshot)) => {
                        snapshot.and_then(|s| s.at(ax, ay)).unwrap_or(target[ai])
                    }
                };
                let canvas_xy = (ax + i64::from(origin.0), ay + i64::from(origin.1));
                composite_pixels_sample(target, ai, raw, li, coverage, under, &pass, canvas_xy);
            }
        }
    }
    Ok(())
}

/// The loop-invariant inputs of one `composite_pixels_into` pass.
pub(crate) struct PixelPass<'a> {
    /// The byte Fill and `opacity` as the one weight they scale the pixels
    /// by (`opacity_byte::joint`, #2).
    pub joint: f32,
    /// The Blend If ranges; `None` = weight 1.
    pub blend_if: Option<&'a BlendIf>,
    pub kind: BlendKind,
    /// `mode == BlendMode::Normal`: a transparent target pixel is written
    /// directly.
    pub direct: bool,
    /// Already > 0.
    pub opacity: f32,
    /// The special Fill of five modes (`blend_fill`, #2), `None` for the
    /// plain one.
    pub fill_blend: Option<FillBlend>,
    /// The document's evaluation policy (`CompositeEnv::transfer`): `Some`
    /// decodes the stored pixel into the linear-light accumulator.
    pub transfer: Option<&'a ByteTransfer>,
}

/// One sample of `composite_pixels_into`'s loop, statement for statement: the
/// weight from `under` (the underlying composite the Blend If reads), `sa =
/// a·coverage·weight·joint`, the direct transparent-target write under
/// Normal, else `composite_source_into`; a special Fill (`pass.fill_blend`)
/// composites at `sa` without the Fill through `FillBlend::composite`, the
/// ONE route to that rule. The regional compositor (`doc_regional`) runs a
/// styled sample through this same body. Under `linear_light` the Blend If
/// reads the stored pixel as it is and `under` through
/// `doc_evaluation::encoded_view` — both encoded, as the ranges are — and
/// only the pixel that composites is decoded (`doc::composite_layer_sample`'s
/// rule).
#[allow(clippy::too_many_arguments)]
#[inline]
pub(crate) fn composite_pixels_sample(
    target: &mut [[f32; 4]],
    ai: usize,
    raw: &[u8],
    li: usize,
    coverage: f32,
    under: [f32; 4],
    pass: &PixelPass<'_>,
    canvas_xy: (i64, i64),
) {
    let stored = [
        f32::from(raw[li]) / 255.0,
        f32::from(raw[li + 1]) / 255.0,
        f32::from(raw[li + 2]) / 255.0,
    ];
    let weight = match pass.blend_if {
        None => 1.0,
        Some(b) => source_weight(b, stored, encoded_view(under, pass.transfer)),
    };
    let cs = pass.transfer.map_or(stored, |t| t.decode(stored));
    if let Some(special) = pass.fill_blend {
        let sa = f32::from(raw[li + 3]) / 255.0 * coverage * weight * pass.opacity;
        if sa > 0.0 {
            special.composite(&mut target[ai][..], &cs, sa);
        }
        return;
    }
    let sa = f32::from(raw[li + 3]) / 255.0 * coverage * weight * pass.joint;
    if sa <= 0.0 {
        return;
    }
    if pass.direct && target[ai][3] <= 0.0 {
        target[ai] = [cs[0], cs[1], cs[2], sa];
        return;
    }
    composite_source_into(target, ai, cs, sa, pass.kind, canvas_xy);
}

/// One contribution onto `target` with `sa = coverage x c.opacity x
/// opacity_scale`, honouring the shift and the knock-out: plane pixel
/// `(px, py)` lands at window pixel `(rel + shape.origin + (px, py) +
/// shift)`, and a knocked-out contribution is multiplied by
/// `1 - shape(px + shift)` — the shape at the landing position. A gradient
/// colour is evaluated at the pixel's layer coordinate. Shared with the
/// typed styled path, whose contributions are the same u8 planes; `colors`
/// is how the contribution's authored colour and blend become its [`Ink`]
/// (`StyleColors::Encoded` at 8 bits, `Encoded16` at 16, their `Linear*`
/// twins under `linear_light`) and what it decodes through.
#[allow(clippy::too_many_arguments)]
pub(crate) fn composite_contribution(
    target: &mut [[f32; 4]],
    acc_w: u32,
    acc_h: u32,
    origin: (i32, i32),
    rel: (i32, i32),
    shape: &Shape,
    c: &Contribution,
    opacity_scale: f32,
    occluding_fill: f32,
    colors: StyleColors<'_>,
    job: JobContext<'_>,
) -> Result<(), String> {
    let scale = c.opacity * opacity_scale;
    if scale.is_nan() || scale <= 0.0 || c.coverage.len() != shape.coverage.len() {
        return Ok(());
    }
    // The style's AUTHORED sRGB colour into the document's space, once for
    // the whole contribution (module doc). A gradient converts its stops, so
    // this is a stop list either way, never per-pixel work.
    let ink = c.ink(colors);
    let decode = colors.decode();
    let (sx, sy) = (i64::from(c.shift.0), i64::from(c.shift.1));
    // Window position of plane pixel (0, 0).
    let base_x = i64::from(rel.0) + shape.origin.0 + sx;
    let base_y = i64::from(rel.1) + shape.origin.1 + sy;
    let px0 = (-base_x).max(0);
    let py0 = (-base_y).max(0);
    let px1 = (i64::from(acc_w) - base_x).min(i64::from(shape.w));
    let py1 = (i64::from(acc_h) - base_y).min(i64::from(shape.h));
    if px0 >= px1 || py0 >= py1 {
        return Ok(());
    }
    for py in py0..py1 {
        job.check()?;
        let ay = base_y + py;
        for px in px0..px1 {
            let ax = base_x + px;
            let ai = (ay as u64 * u64::from(acc_w) + ax as u64) as usize;
            let canvas_xy = (ax + i64::from(origin.0), ay + i64::from(origin.1));
            composite_contribution_sample(
                target,
                ai,
                shape,
                c,
                &ink,
                decode,
                (px, py),
                scale,
                occluding_fill,
                canvas_xy,
            );
        }
    }
    Ok(())
}

/// One plane pixel of `composite_contribution`'s loop, statement for
/// statement: the coverage at `plane_xy` (0 → nothing), the shape knock-out
/// at the landing position, `sa = cov / 255 · scale`, the ink's colour at the
/// pixel's layer coordinate and `composite_source_into` with the ink's kind
/// onto `target[ti]`.
/// The caller guarantees `plane_xy` lies inside the plane, `scale` passed the
/// loop's own early return (not NaN, > 0) and `c.coverage` is the shape's
/// size. The regional compositor (`doc_regional_tree`,
/// `precision_regional_tree`) runs a styled sample through this same body.
/// `decode` is the scene's (`StyleColors::decode`): the ink's colour enters a
/// linear-light accumulator decoded, here and nowhere else.
#[allow(clippy::too_many_arguments)]
#[inline]
pub(crate) fn composite_contribution_sample(
    target: &mut [[f32; 4]],
    ti: usize,
    shape: &Shape,
    c: &Contribution,
    ink: &Ink,
    decode: InkDecode<'_>,
    plane_xy: (i64, i64),
    scale: f32,
    occluding_fill: f32,
    canvas_xy: (i64, i64),
) {
    let sa = contribution_alpha(shape, c, plane_xy, scale, occluding_fill);
    if sa <= 0.0 {
        return;
    }
    let (px, py) = plane_xy;
    let cs = decode.apply(ink.color.at(px + shape.origin.0, py + shape.origin.1));
    composite_source_into(target, ti, cs, sa, ink.kind, canvas_xy);
}

/// Whether a knocked-out drop shadow lands under the layer
/// ([`contribution_alpha`], module doc step 2): #2 part 4's confirmed
/// candidate DK1 (`scripts/photoshop-probe/effect_kernels_578_p4_take.py`).
/// Its knock-out at the landing leaves `m (1 − a)`; landing at `sa / (1 −
/// a·fill)` keeps exactly that after the layer's `over`, where landing at `sa`
/// left `m (1 − a)(1 − a·fill)`. A shadow with Layer Knocks Out off is
/// untouched.
const SHADOW_UNDER_LAYER: bool = true;

/// The effective alpha one contribution lands with at plane pixel
/// `plane_xy`: its coverage (0 → nothing), the shape knock-out at the landing
/// position (`1 − shape(p + shift)`, rounded on the byte grid), then
/// `cov / 255 · scale`; a knocked-out DROP SHADOW then lands under the layer
/// at its landing position (`band_under_layer` at `shape(p + shift) / 255 ·
/// occluding_fill`, [`SHADOW_UNDER_LAYER`]), and a Below STROKE band and an
/// OUTER GLOW under the layer at `shape(p) / 255 · occluding_fill` (module
/// doc step 2; the glow's plane is already `g (1 − a)`, so the layer's
/// `over` leaves the measured `g (1 − a)`, #2, `style_fx_outer_glow`). The
/// ONE coverage rule, shared by [`composite_contribution_sample`]
/// and the CMYK styled row (`cmyk_style_row`, #1); 0 means "draws nothing
/// here".
#[inline]
pub(crate) fn contribution_alpha(
    shape: &Shape,
    c: &Contribution,
    plane_xy: (i64, i64),
    scale: f32,
    occluding_fill: f32,
) -> f32 {
    let (px, py) = plane_xy;
    let (sx, sy) = (i64::from(c.shift.0), i64::from(c.shift.1));
    let i = shape.idx(px as u32, py as u32);
    let mut cov = u32::from(c.coverage[i]);
    if cov == 0 {
        return 0.0;
    }
    let mut landing = None;
    if c.knocked_out_by_shape {
        let under = shape.at(px + sx, py + sy);
        cov = (cov * (255 - u32::from(under)) + 127) / 255;
        if cov == 0 {
            return 0.0;
        }
        landing = Some(under);
    }
    let sa = cov as f32 / 255.0 * scale;
    if let Some(under) = landing.filter(|_| SHADOW_UNDER_LAYER && c.kind == EffectKind::DropShadow)
    {
        // The knocked-out drop shadow lands under the layer (module doc,
        // step 2), so the layer's `over` leaves the measured `m (1 − a)`.
        return band_under_layer(sa, f32::from(under) / 255.0 * occluding_fill);
    }
    if matches!(c.kind, EffectKind::Stroke | EffectKind::OuterGlow)
        && c.placement == Placement::Below
    {
        return band_under_layer(sa, f32::from(shape.at(px, py)) / 255.0 * occluding_fill);
    }
    sa
}

/// The share of a covered pixel the layer's pixel pass lands with, which a
/// Below stroke band's division assumes (`contribution_alpha`, #2): 0 —
/// the band keeps its own alpha — for a styled adjustment (`adjusted`:
/// its pixels occlude nothing), under a non-identity Blend If (the pass lands
/// at `…·weight`), in Dissolve (its coin toss lands a pixel at 0 or full) and
/// at a Fill of 0 (the pass draws nothing); 1 under a special Fill
/// (`fill_blend`: that pass lands at `sa` without the Fill); else the Fill.
/// `blend_if` is the layer's Blend If: the style's own on a byte layer, the
/// layer's on a typed or CMYK one, which stores it apart from the style.
/// Every caller takes its value from here.
pub(crate) fn occluding_fill(
    style: &LayerStyle,
    blend_if: Option<&BlendIf>,
    mode: BlendMode,
    float: bool,
    adjusted: bool,
) -> f32 {
    let fill = crate::opacity_byte::composite(style.fill_opacity, float);
    if adjusted
        || blend_if.is_some_and(|b| !b.is_identity())
        || mode == BlendMode::Dissolve
        || fill <= 0.0
    {
        return 0.0;
    }
    if fill_blend(mode, fill, float).is_some() {
        1.0
    } else {
        fill
    }
}

/// `clip_unit`'s `step` over the whole buffer with the footprint read from
/// the shape — its coverage / 255, 0 outside the plane — the clip-base
/// footprint rule of the module doc (#2: the members composite onto the
/// base with its alpha frozen). Shared with the typed styled clip base.
#[allow(clippy::too_many_arguments)]
pub(crate) fn clip_pass_shape(
    buf: &mut [[f32; 4]],
    acc_w: u32,
    acc_h: u32,
    rel: (i32, i32),
    shape: &Shape,
    step: ClipStep,
    job: JobContext<'_>,
) -> Result<(), String> {
    let base_x = i64::from(rel.0) + shape.origin.0;
    let base_y = i64::from(rel.1) + shape.origin.1;
    for ay in 0..i64::from(acc_h) {
        job.check()?;
        let row = ay as usize * acc_w as usize;
        for ax in 0..i64::from(acc_w) {
            let limit = f32::from(shape.at(ax - base_x, ay - base_y)) / 255.0;
            let px = &mut buf[row + ax as usize];
            px[3] = clip_alpha(px[3], limit, step);
        }
    }
    Ok(())
}

/// The canvas rect `(x0, y0, x1, y1)` (exclusive) Merge Down must give a
/// baked `layer` (module doc): its pixel rect grown by the style's pad, and
/// the drop shadow's shifted copy of that rect clipped to the canvas. The
/// pixel rect alone without a renderable style.
pub(crate) fn merge_extent(layer: &Layer, env: CompositeEnv<'_>) -> (i64, i64, i64, i64) {
    let (lw, lh) = layer.pixels.dimensions();
    let (x, y) = (i64::from(layer.offset.0), i64::from(layer.offset.1));
    let rect = (x, y, x + i64::from(lw), y + i64::from(lh));
    match layer.renders_style() {
        Some(style) => style_grown(rect, style, env),
        None => rect,
    }
}

/// `rect` grown by everything a rendered `style` draws OUTSIDE the thing it
/// decorates: the pad on all four sides, plus each drop shadow's shifted
/// rect clipped to the canvas (every stacked instance's own shift).
///
/// Split out of [`merge_extent`] because a GROUP needs exactly the same
/// arithmetic and has no `Layer` to hand it: `doc_group::extent_of_level`
/// grows a nested group's subtree rect with this, so an enclosing isolated
/// group sizes its buffer to hold the inner group's effects instead of
/// clipping them away. One implementation, two callers.
pub(crate) fn style_grown(
    rect: (i64, i64, i64, i64),
    style: &LayerStyle,
    env: CompositeEnv<'_>,
) -> (i64, i64, i64, i64) {
    let pad = i64::from(style.pad());
    let padded = (rect.0 - pad, rect.1 - pad, rect.2 + pad, rect.3 + pad);
    let (x0, y0, x1, y1) = padded;
    let mut out = padded;
    for (dx, dy) in style.shadow_shifts(env.light) {
        let (dx, dy) = (i64::from(dx), i64::from(dy));
        let sx0 = (x0 + dx).max(0);
        let sy0 = (y0 + dy).max(0);
        let sx1 = (x1 + dx).min(i64::from(env.canvas.0));
        let sy1 = (y1 + dy).min(i64::from(env.canvas.1));
        if sx1 <= sx0 || sy1 <= sy0 {
            continue;
        }
        out = (
            out.0.min(sx0),
            out.1.min(sy0),
            out.2.max(sx1),
            out.3.max(sy1),
        );
    }
    out
}
