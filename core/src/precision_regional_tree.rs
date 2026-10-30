//! The 16-bit and float regional compositor's TREE (Phase 13A-3a): the
//! document scene's layers (`PrecisionScene::layers`, exactly as the
//! reference's own builder produced them) prepared ONCE — every mask through
//! `precision_render::prepare_mask`, each clip base's `full` clone, each
//! adjustment's whole-layer skip, and (13A-3b-i) each styled layer's
//! `PreparedStyle`: a raster's planes from `typed_shape` + `render_effects_with_job`,
//! the reference's own preparation, and (13A-3b-ii) a styled group with an
//! enabled effect MATERIALIZED as `render_styled` materializes it — its
//! children rendered by `render_layers` itself into a canvas-sized buffer —
//! with the planes built over that buffer — and evaluated one sample at a
//! time, a sample of `precision_render::render_layers` (the styled bodies
//! in `precision_regional_style`; an isolated group whose level holds a
//! staged guide reads its held plane, `Node::staged`, 13A-4b-i). It holds NO
//! admission logic (`precision_regional`'s walk decides every refusal) and
//! NO pixel math of its own: every statement is index arithmetic around
//! `composite_raster_sample16`, `adjust_backdrop_sample16`,
//! `composite_group_sample16`, `gate_pass_through_sample16`,
//! `composite_pixels_sample16`, `composite_contribution_sample` (through
//! `doc_regional_style::contributions_sample`), `mix_premultiplied`,
//! `typed_coverage_byte`, `checked_alpha`, `source_weight_16` (`StyleEnv::blend_if_weight`),
//! `composite_source_into` and `PreparedMask::at`. Since 13A-3c every sample
//! carries the reference's own `style_knockout::KnockoutBackdrop` in
//! `StyleEnv::knockout` over one-element slices — `NONE` into an isolated
//! group, the sample as the children began into a pass-through group, the
//! base-only sample into a clip unit's members — and a ROUTED style
//! (`routes_unit`) takes the unit pass (`regional_knockout::
//! unit_pass_sample16`). The contract is `regional`'s module doc.

use std::sync::Arc;

use crate::blend::{composite_source_into, BlendMode};
use crate::clip_unit::{clip_alpha, ClipStep};
use crate::precision_image::pixel_count;
use crate::precision_mask::PreparedMask;
use crate::precision_regional_style::styled_sample16;
use crate::precision_render::members_want_base_snapshot16;
use crate::precision_render::{
    adjust_backdrop_sample16, checked_alpha, composite_group_sample16, composite_raster_sample16,
    gate_pass_through_sample16, prepare_mask, render_layers, zero_plane, Kind, PrecisionLayer,
    Progress,
};
use crate::precision_style_render::{style_scratch, typed_shape, StyleEnv, StyledPixels};
use crate::regional_guide::{PreparedGuide, StagedPlane};
use crate::regional_knockout::{base_only_backdrop, isolated_env};
use crate::style::{Knockout, LayerStyle};
use crate::style_composite::affected_window;
use crate::style_knockout::{grouped_interior, routes_unit, KnockoutBackdrop};
use crate::style_render::{render_effects_with_job, Ink, RenderContext, RenderedEffects};

/// One scene layer, prepared.
pub(crate) struct Node<'a> {
    pub(crate) layer: &'a PrecisionLayer,
    /// The reference's `full` clone for a clip base (opacity 1, Normal, no
    /// members / Blend If / style); `None` otherwise.
    pub(crate) full: Option<PrecisionLayer>,
    /// The layer's prepared mask; `None` without one, and for a node whose
    /// whole-layer skip holds (the reference never prepares it there).
    pub(crate) mask: Option<PreparedMask<'a>>,
    /// `render_layers`' whole-layer Adjustment skip, evaluated on the layer
    /// the reference actually composites through its arms: `full` for a clip
    /// base, the layer itself otherwise.
    pub(crate) skips: bool,
    /// A group's children, prepared; empty for a MATERIALIZED group, whose
    /// children the reference renders inside (`PreparedStyle::buffer`).
    pub(crate) children: Vec<Node<'a>>,
    /// Exactly `layer.clipped_members` (contributing members only, the
    /// reference's own vector), prepared.
    pub(crate) members: Vec<Node<'a>>,
    /// A styled layer's `render_styled` inputs; `None` without a style.
    pub(crate) style: Option<PreparedStyle>,
    /// A guided ROOT adjustment's staged guide (13A-4a, set by the staged
    /// driver); `None` from `prepare` and everywhere else.
    pub(crate) guide: Option<PreparedGuide>,
    /// An isolated group whose level holds a staged guide (13A-4b-i): the
    /// children's held final plane (`precision_regional_guide::
    /// stage_nested`), read in place of the children; `None` otherwise.
    pub(crate) staged: Option<StagedPlane>,
}

/// A styled layer (`render_styled` → `composite_styled_into`) resolved once:
/// the sanitized fill, a raster's planes when an effect is enabled (the
/// reference's own `typed_shape` + `render_effects_with_job`, rendered per render as
/// the reference renders them), each contribution's [`Ink`] resolved once
/// (Below then Interior, `composite_contribution`'s `env.style_colors()`), the package
/// window (`affected_window` within the canvas-sized accumulator), whether
/// the package model applies, — a group whose style renders planes — its
/// materialized children and — 13A-3c — the style's knockout, whether it
/// takes the unit pass (`routes_unit`) and whether its grouped interiors
/// move into the unit (`grouped_interior`), over the planes.
pub(crate) struct PreparedStyle {
    pub(crate) fill: f32,
    pub(crate) planes: Option<RenderedEffects>,
    pub(crate) inks: Vec<Ink>,
    pub(crate) window: Option<(i64, i64, i64, i64)>,
    pub(crate) package: bool,
    /// The materialized children (canvas-sized) of a group whose style
    /// renders planes: `render_styled`'s own group buffer.
    pub(crate) buffer: Option<Vec<[f32; 4]>>,
    pub(crate) knockout: Knockout,
    pub(crate) routed: bool,
    pub(crate) grouped: bool,
}

/// `render_styled`'s refusal of a kind no style can sit on, verbatim (a
/// styled adjustment, is refused by name before the tree is built:
/// `regional::STYLED_ADJUSTMENT`).
pub(crate) const UNSTYLABLE: &str = crate::precision_render::UNSTYLED_KIND;

/// The mask-preparation work `prepare_mask` advances for `layer`.
fn mask_work(layer: &PrecisionLayer) -> Result<u64, String> {
    layer.mask.as_ref().map_or(Ok(0), |mask| Ok(mask.plan()?.1))
}

/// `render_layers`' whole-layer Adjustment skip, verbatim: an identity kernel
/// is a no-op only under Normal; opacity 0 on every mode.
fn skips(layer: &PrecisionLayer) -> bool {
    match &layer.kind {
        Kind::Adjustment(state) => {
            layer.opacity == 0.0
                || (state.kernel.is_identity() && layer.blend_mode == BlendMode::Normal)
        }
        _ => false,
    }
}

/// The scene's `layers` prepared once, before the window loop: every mask the
/// render will read is prepared (advancing its work), a skipped node's mask
/// work is advanced unprepared, every styled raster's planes are rendered
/// (advancing their work), so the progress total stays the plan's.
pub(crate) fn prepare<'a>(
    layers: &'a [PrecisionLayer],
    env: &StyleEnv<'_>,
    progress: &mut Progress<'_>,
) -> Result<Vec<Node<'a>>, String> {
    let mut out = Vec::with_capacity(layers.len());
    for layer in layers {
        progress.job.check()?;
        // A styled layer takes `render_styled` BEFORE the clip branch, so it
        // has no `full` clone and no whole-layer skip.
        let styled = layer.style.is_some();
        let full = (!styled && !layer.clipped_members.is_empty()).then(|| {
            let mut full = layer.clone();
            full.opacity = 1.0;
            full.blend_mode = BlendMode::Normal;
            full.clipped_members = Arc::new(Vec::new());
            full.blend_if = None;
            full.style = None;
            full.knockout_probe = false;
            full
        });
        let skips = !styled && skips(full.as_ref().unwrap_or(layer));
        let mask = if skips {
            progress.advance(mask_work(layer)? as usize)?;
            None
        } else {
            prepare_mask(layer, progress)?
        };
        // A styled group with an enabled effect is materialized by
        // `prepare_style` (`render_styled` renders its children itself).
        let materialized = layer
            .style
            .as_deref()
            .is_some_and(LayerStyle::has_enabled_effects);
        let children = match &layer.kind {
            Kind::Group(children) | Kind::PassThrough(children) if !materialized => {
                prepare(children, env, progress)?
            }
            _ => Vec::new(),
        };
        let members = prepare(&layer.clipped_members, env, progress)?;
        let style = match layer.style.as_deref() {
            Some(style) => Some(prepare_style(layer, style, mask.as_ref(), env, progress)?),
            None => None,
        };
        out.push(Node {
            layer,
            full,
            mask,
            skips,
            children,
            members,
            style,
            guide: None,
            staged: None,
        });
    }
    Ok(out)
}

/// `composite_styled_into`'s preparation for one styled layer: with an
/// enabled effect the shape over its prepared mask and the planes, polling
/// the job before and after and advancing the work the plan charged
/// (`style_scratch`). A raster's shape is its image's; a GROUP's is
/// `render_styled`'s own: the children rendered by `render_layers` into a
/// canvas-sized buffer (D4's hard stop, `KnockoutBackdrop::NONE`), which is
/// kept for the sample loop.
fn prepare_style<'a>(
    layer: &'a PrecisionLayer,
    style: &'a LayerStyle,
    mask: Option<&PreparedMask<'_>>,
    env: &StyleEnv<'_>,
    progress: &mut Progress<'_>,
) -> Result<PreparedStyle, String> {
    let mut out = PreparedStyle {
        fill: crate::opacity_byte::composite(style.fill_opacity, env.float),
        planes: None,
        inks: Vec::new(),
        window: None,
        package: false,
        buffer: None,
        knockout: style.knockout,
        routed: routes_unit(style, None),
        grouped: false,
    };
    if !style.has_enabled_effects() {
        return Ok(out);
    }
    let (width, height) = env.canvas;
    let (offset, plane) = match &layer.kind {
        Kind::Raster { image, offset } => (*offset, image.dimensions()),
        Kind::Group(children) => {
            let mut group = zero_plane(pixel_count(width, height)?, progress)?;
            // A STYLED group is isolated, so its buffer is D4's hard stop.
            let inner = StyleEnv {
                knockout: crate::style_knockout::KnockoutBackdrop::NONE,
                ..*env
            };
            render_layers(children, env.canvas, (0, 0), &mut group, progress, &inner)?;
            out.buffer = Some(group);
            ((0, 0), env.canvas)
        }
        _ => return Err(UNSTYLABLE.into()),
    };
    progress.job.check()?;
    let pixels = match (&layer.kind, &out.buffer) {
        (Kind::Raster { image, offset }, _) => StyledPixels::Image {
            image,
            offset: *offset,
        },
        (_, Some(group)) => StyledPixels::Buffer {
            pixels: group,
            width,
            height,
            origin: (0, 0),
        },
        _ => return Err(UNSTYLABLE.into()),
    };
    let shape = typed_shape(&pixels, mask, style.pad())
        .ok_or("typed layer style plane exceeds the pixel limit")?;
    progress.job.check()?;
    // The planes report inside the work the plan charged (13A-5b-iii-c-1):
    // `completed/total → (completed + work)/total`, then the advance.
    let (_, work) = style_scratch(style, plane, false, None)?;
    let start = progress.completed as f64 / progress.total as f64;
    let end = (progress.completed + work) as f64 / progress.total as f64;
    let planes = render_effects_with_job(
        style,
        shape,
        &RenderContext {
            light: env.light,
            canvas: env.canvas,
            layer_offset: offset,
            lab_tiles: env
                .colors
                .is_some_and(crate::authored_color::AuthoredColors::converts_tiles),
        },
        progress.job.range(start, end),
    )?;
    progress.job.check()?;
    progress.advance(work as usize)?;
    out.inks = planes
        .below
        .iter()
        .chain(&planes.interior)
        .map(|c| c.ink(env.style_colors()))
        .collect();
    // Every typed accumulator is canvas-sized at (0, 0).
    let (x0, y0, x1, y1) = affected_window(offset, &planes.shape, &planes.below);
    out.window = Some((
        x0.max(0),
        y0.max(0),
        x1.min(i64::from(env.canvas.0)),
        y1.min(i64::from(env.canvas.1)),
    ));
    out.package = layer.opacity < 1.0 && layer.blend_mode != BlendMode::Dissolve;
    out.routed = routes_unit(style, Some(&planes));
    out.grouped = grouped_interior(style, Some(&planes));
    out.planes = Some(planes);
    Ok(out)
}

/// One sample of `render_layers` over `nodes` onto `acc[i]` at canvas `xy`:
/// each node's arm, or its clip unit's private-buffer statements — branching
/// on `members.is_empty()`, the reference's own test.
pub(crate) fn sample_layers(
    nodes: &[Node<'_>],
    acc: &mut [[f32; 4]],
    i: usize,
    env: &StyleEnv<'_>,
    xy: (i64, i64),
) -> Result<(), String> {
    for node in nodes {
        let layer = node.layer;
        if let Some(style) = &node.style {
            // `render_layers` takes `render_styled` before the clip branch.
            styled_sample16(acc, i, node, style, env, xy, None)?;
            continue;
        }
        if node.full.is_none() || node.members.is_empty() {
            // A lone entry, including a base whose run is all hidden.
            if !node.skips {
                arm_sample(acc, i, node, layer, env, xy)?;
            }
            continue;
        }
        clip_unit_sample(node, acc, i, acc[i], None, env, xy)?;
    }
    Ok(())
}

/// The clip-group rule (`render_layers`' `clipped_members` branch), one
/// sample (extracted from `sample_layers` in 13A-4b-ii-b-1): the base as the
/// reference's `full` clone into a transparent buffer (its own children and
/// mask), its UNWEIGHTED footprint recorded, the base's own Blend If
/// weighting its alpha against `under` (the accumulator beneath the unit),
/// each member of `members[..stop]` (all without `stop`) through the
/// ordinary arms onto the buffer with the base's alpha frozen (`clip_unit`'s
/// Open / Settle / Close); with `stop` the unit buffer at that member's cursor is left
/// in `acc[i]`, without it `acc[i]` is reset to `under` and the buffer
/// composited once with the base's mode and opacity. `sample_layers` passes
/// `under == acc[i]`, so the reset is a no-op there. A STYLED base
/// (13A-4b-ii-b-2) is `render_styled`'s: `acc[i] = under`, then
/// `styled_sample16` with `stop` (its direct or routed member loop, or the
/// package's copy).
pub(crate) fn clip_unit_sample(
    node: &Node<'_>,
    acc: &mut [[f32; 4]],
    i: usize,
    under: [f32; 4],
    stop: Option<usize>,
    env: &StyleEnv<'_>,
    xy: (i64, i64),
) -> Result<(), String> {
    if let Some(style) = &node.style {
        acc[i] = under;
        return styled_sample16(acc, i, node, style, env, xy, stop);
    }
    let layer = node.layer;
    let Some(full) = node.full.as_ref() else {
        return Ok(());
    };
    let mut buf = [[0.0f32; 4]];
    if !node.skips {
        arm_sample(&mut buf, 0, node, full, env, xy)?;
    }
    let a = buf[0][3];
    if let Some(blend_if) = &layer.blend_if {
        let pixel = buf[0];
        if pixel[3] > 0.0 {
            buf[0][3] *= env.blend_if_weight(
                blend_if,
                [pixel[0], pixel[1], pixel[2]],
                [under[0], under[1], under[2]],
            );
        }
    }
    // `clip_unit`'s Open against the unweighted footprint (#2). Then D4's
    // clipped-member rule, the UNSTYLED base's loop: a member with a
    // knockout punches to the buffer as it stands after the base alone
    // rendered — after its Blend If weighting, opened, as the reference
    // clones it.
    let opened = !node.members.is_empty();
    if opened {
        buf[0][3] = clip_alpha(buf[0][3], a, ClipStep::Open);
    }
    let base_only = [buf[0]];
    let member_env = StyleEnv {
        knockout: base_only_backdrop(
            members_want_base_snapshot16(&layer.clipped_members),
            &base_only,
        ),
        ..*env
    };
    let end = stop.map_or(node.members.len(), |m| m.min(node.members.len()));
    for member in &node.members[..end] {
        sample_layers(std::slice::from_ref(member), &mut buf, 0, &member_env, xy)?;
        buf[0][3] = clip_alpha(buf[0][3], a, ClipStep::Settle);
    }
    // A stop is a member cursor: the unit still open (the reference's
    // buffer as that member composites onto it).
    if stop.is_some() {
        acc[i] = buf[0];
        return Ok(());
    }
    if opened {
        buf[0][3] = clip_alpha(buf[0][3], a, ClipStep::Close);
    }
    acc[i] = under;
    let pixel = buf[0];
    let alpha = checked_alpha(pixel[3], layer.opacity, 1.0)?;
    if alpha > 0.0 {
        composite_source_into(
            acc,
            i,
            [pixel[0], pixel[1], pixel[2]],
            alpha,
            env.kind(layer.blend_mode),
            xy,
        );
    }
    Ok(())
}

/// One sample of `render_layers`' arm for `layer` — the node's own layer, or
/// its `full` clone on a clip base's base pass — onto `acc[i]` at canvas
/// `xy`, over the node's prepared mask and children: index arithmetic only,
/// the bodies are theirs. The caller has already made the Adjustment arm's
/// whole-layer skip.
fn arm_sample(
    acc: &mut [[f32; 4]],
    i: usize,
    node: &Node<'_>,
    layer: &PrecisionLayer,
    env: &StyleEnv<'_>,
    xy: (i64, i64),
) -> Result<(), String> {
    let kind = env.kind(layer.blend_mode);
    let coverage = || node.mask.as_ref().map_or(1.0, |m| m.at(xy.0, xy.1));
    match &layer.kind {
        Kind::Raster { image, offset } => {
            let (iw, ih) = image.dimensions();
            let sx = xy.0 - i64::from(offset.0);
            let sy = xy.1 - i64::from(offset.1);
            if sx < 0 || sy < 0 || sx >= i64::from(iw) || sy >= i64::from(ih) {
                return Ok(()); // outside the source: untouched
            }
            // Unreachable after the bounds test above; `sample_at` is total.
            let Some(pixel) = image.sample_at(sx as u32, sy as u32) else {
                return Ok(());
            };
            composite_raster_sample16(acc, i, pixel, layer, coverage(), kind, env, xy)
        }
        Kind::Adjustment(state) => {
            if acc[i][3] == 0.0 {
                return Ok(());
            }
            // A guided ROOT adjustment reads its staged guide (13A-4a,
            // `render_layers`' `guide.at`); every other adjustment is
            // pointwise and its 0.0 is never read.
            let g = node.guide.as_ref().map_or(0.0, |g| g.at(xy));
            let k = layer.opacity * coverage();
            adjust_backdrop_sample16(&mut acc[i], state, g, k, kind, env, xy)
        }
        Kind::Group(_) => {
            // The children onto a transparent sample — D4's hard stop, both
            // backdrops transparent; a staged level's held plane (13A-4b-i)
            // — then the closing loop's statement (`composite_group_sample16`).
            let group = match &node.staged {
                Some(staged) => staged.at(xy),
                None => {
                    let mut group = [[0.0f32; 4]];
                    sample_layers(&node.children, &mut group, 0, &isolated_env(env), xy)?;
                    group[0]
                }
            };
            composite_group_sample16(acc, i, group, layer, coverage(), kind, env.float, xy)
        }
        Kind::PassThrough(_) => {
            // The children onto a copy of this sample, gated back. D4: a
            // Shallow child punches to the sample as the children began;
            // Deep rides through. ENTER (the sample as the children began),
            // RESUME (the children onto the copy), FINISH (gated back onto
            // the held sample) — the statements `sample_between` splits
            // across stages (13A-4b-ii-a).
            let before = [acc[i]];
            let children = pass_through_env(env, Some(&before));
            let mut evaluated = [acc[i]];
            sample_layers(&node.children, &mut evaluated, 0, &children, xy)?;
            finish_pass_through(&mut evaluated, 0, before[0], node, layer, xy)?;
            acc[i] = evaluated[0];
            Ok(())
        }
    }
}

/// A pass-through node's children env: Shallow the sample as its children
/// began (`None` when not held), Deep riding through (D4).
pub(crate) fn pass_through_env<'e>(
    env: &StyleEnv<'e>,
    shallow: Option<&'e [[f32; 4]]>,
) -> StyleEnv<'e> {
    StyleEnv {
        knockout: KnockoutBackdrop {
            shallow,
            deep: env.knockout.deep,
        },
        ..*env
    }
}

/// A pass-through node's FINISH: its children's result `acc[i]` gated back
/// onto `held`, the sample as the children began
/// (`gate_pass_through_sample16`, the typed arm always copies).
pub(crate) fn finish_pass_through(
    acc: &mut [[f32; 4]],
    i: usize,
    held: [f32; 4],
    node: &Node<'_>,
    layer: &PrecisionLayer,
    xy: (i64, i64),
) -> Result<(), String> {
    let coverage = node.mask.as_ref().map_or(1.0, |m| m.at(xy.0, xy.1));
    let mut a = held;
    gate_pass_through_sample16(&mut a, acc[i], layer.opacity, coverage)?;
    acc[i] = a;
    Ok(())
}
